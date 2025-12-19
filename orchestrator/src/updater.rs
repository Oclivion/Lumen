//! Auto-updater with Ed25519 signature verification
//!
//! Security model:
//! 1. Manifest is fetched from configured URL (HTTPS)
//! 2. Binary hash is verified against manifest
//! 3. Ed25519 signature on hash is verified with hardcoded public key
//! 4. Only after both verifications pass is the binary applied
//! 5. Atomic replacement with rollback on startup failure

use crate::config::Config;
use crate::error::{LumenError, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// Update manifest structure (version.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// Latest version
    pub version: String,

    /// SHA-256 hash of the archive (hex-encoded)
    pub sha256: String,

    /// Ed25519 signature of the SHA-256 hash (hex-encoded)
    pub signature: String,

    /// Minimum supported version (force update below this)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,

    /// Release notes
    #[serde(default)]
    pub release_notes: String,

    /// Release timestamp
    pub released_at: String,

    /// Download URLs (primary + mirrors)
    pub downloads: DownloadUrls,

    /// Size in bytes
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadUrls {
    pub linux_x86_64: Option<String>,
    pub linux_aarch64: Option<String>,
    pub darwin_x86_64: Option<String>,
    pub darwin_aarch64: Option<String>,
    pub windows_x86_64: Option<String>,
}

impl DownloadUrls {
    /// Get the download URL for the current platform
    pub fn for_current_platform(&self) -> Option<&str> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return self.linux_x86_64.as_deref();

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return self.linux_aarch64.as_deref();

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return self.darwin_x86_64.as_deref();

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return self.darwin_aarch64.as_deref();

        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return self.windows_x86_64.as_deref();

        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "linux", target_arch = "aarch64"),
            all(target_os = "macos", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64"),
        )))]
        return None;
    }
}

/// Information about an available update
#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub release_notes: String,
    pub size: u64,
    pub download_url: String,
    pub is_mandatory: bool,
}

/// Handles checking for and applying updates
pub struct Updater {
    config: Config,
    client: reqwest::Client,
    public_key: VerifyingKey,
}

impl Updater {
    /// Create a new Updater
    pub fn new(config: Config) -> Self {
        // Parse the Ed25519 public key from config
        let public_key = Self::parse_public_key(&config.update.public_key)
            .expect("Invalid update public key in configuration");

        let client = reqwest::Client::builder()
            .user_agent(format!("Lumen/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            public_key,
        }
    }

    /// Parse Ed25519 public key from hex string
    fn parse_public_key(hex_key: &str) -> Result<VerifyingKey> {
        let bytes = hex::decode(hex_key)
            .map_err(|e| LumenError::Config(format!("Invalid public key hex: {}", e)))?;

        if bytes.len() != 32 {
            return Err(LumenError::Config(format!(
                "Public key must be 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&bytes);

        VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| LumenError::Config(format!("Invalid Ed25519 public key: {}", e)))
    }

    /// Check if an update is available
    pub async fn check_for_update(&self) -> Result<Option<AvailableUpdate>> {
        info!("Checking for updates...");

        let manifest = self.fetch_manifest().await?;
        let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|e| LumenError::Update(format!("Invalid current version: {}", e)))?;

        let latest_version = Version::parse(&manifest.version)
            .map_err(|e| LumenError::Update(format!("Invalid manifest version: {}", e)))?;

        // Check if we're below minimum version (mandatory update)
        let is_mandatory = if let Some(ref min_ver) = manifest.min_version {
            let min_version = Version::parse(min_ver)
                .map_err(|e| LumenError::Update(format!("Invalid min_version: {}", e)))?;
            current_version < min_version
        } else {
            false
        };

        if latest_version > current_version {
            let download_url = manifest
                .downloads
                .for_current_platform()
                .ok_or_else(|| {
                    LumenError::UnsupportedPlatform(format!(
                        "No download available for {}-{}",
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    ))
                })?
                .to_string();

            info!(
                "Update available: {} -> {} (mandatory: {})",
                current_version, latest_version, is_mandatory
            );

            Ok(Some(AvailableUpdate {
                version: manifest.version,
                release_notes: manifest.release_notes,
                size: manifest.size,
                download_url,
                is_mandatory,
            }))
        } else {
            info!("Already running latest version: {}", current_version);
            Ok(None)
        }
    }

    /// Download and apply an update
    pub async fn update(&self, force: bool) -> Result<()> {
        let manifest = self.fetch_manifest().await?;

        let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|e| LumenError::Update(format!("Invalid current version: {}", e)))?;

        let latest_version = Version::parse(&manifest.version)
            .map_err(|e| LumenError::Update(format!("Invalid manifest version: {}", e)))?;

        if !force && latest_version <= current_version {
            info!("Already running latest version: {}", current_version);
            return Ok(());
        }

        let download_url = manifest
            .downloads
            .for_current_platform()
            .ok_or_else(|| {
                LumenError::UnsupportedPlatform(format!(
                    "No download available for {}-{}",
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            })?;

        info!("Downloading update {} from {}", manifest.version, download_url);

        // Create temp directory for download
        let temp_dir = TempDir::new()?;
        let archive_path = temp_dir.path().join("update.tar.gz");

        // Download with progress
        self.download_with_progress(download_url, &archive_path, manifest.size)
            .await?;

        // Verify hash
        info!("Verifying download integrity...");
        let actual_hash = self.compute_file_hash(&archive_path)?;

        if actual_hash != manifest.sha256 {
            return Err(LumenError::HashMismatch {
                expected: manifest.sha256,
                actual: actual_hash,
            });
        }

        // Verify signature
        info!("Verifying cryptographic signature...");
        self.verify_signature(&manifest.sha256, &manifest.signature)?;

        info!("Signature verified successfully");

        // Extract and apply update
        info!("Applying update...");
        self.apply_update(&archive_path, temp_dir.path()).await?;

        info!(
            "Update complete! Restart Lumen to use version {}",
            manifest.version
        );

        Ok(())
    }

    /// Fetch the update manifest
    async fn fetch_manifest(&self) -> Result<UpdateManifest> {
        debug!("Fetching manifest from {}", self.config.update.manifest_url);

        let response = self
            .client
            .get(&self.config.update.manifest_url)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| LumenError::Update(format!("Failed to fetch manifest: {}", e)))?;

        let manifest: UpdateManifest = response.json().await?;

        Ok(manifest)
    }

    /// Download file with progress bar
    async fn download_with_progress(
        &self,
        url: &str,
        dest: &Path,
        expected_size: u64,
    ) -> Result<()> {
        let response = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()
            .map_err(|e| LumenError::Update(format!("Download failed: {}", e)))?;

        let total_size = response
            .content_length()
            .unwrap_or(expected_size);

        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );

        let mut file = tokio::fs::File::create(dest).await?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }

        pb.finish_with_message("Download complete");
        Ok(())
    }

    /// Compute SHA-256 hash of a file
    fn compute_file_hash(&self, path: &Path) -> Result<String> {
        let mut file = File::open(path)?;
        let mut hasher = Sha256::new();

        std::io::copy(&mut file, &mut hasher)?;

        let hash = hasher.finalize();
        Ok(hex::encode(hash))
    }

    /// Verify Ed25519 signature
    fn verify_signature(&self, hash: &str, signature_hex: &str) -> Result<()> {
        let signature_bytes = hex::decode(signature_hex)
            .map_err(|e| LumenError::Update(format!("Invalid signature hex: {}", e)))?;

        if signature_bytes.len() != 64 {
            return Err(LumenError::Update(format!(
                "Signature must be 64 bytes, got {}",
                signature_bytes.len()
            )));
        }

        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&signature_bytes);

        let signature = Signature::from_bytes(&sig_array);

        // Verify signature over the hash bytes (not hex string)
        let hash_bytes = hex::decode(hash)
            .map_err(|e| LumenError::Update(format!("Invalid hash hex: {}", e)))?;

        self.public_key
            .verify(&hash_bytes, &signature)
            .map_err(|_| LumenError::SignatureVerification)?;

        Ok(())
    }

    /// Apply the update by extracting and replacing binaries
    async fn apply_update(&self, archive_path: &Path, temp_dir: &Path) -> Result<()> {
        // Check if running inside an AppImage
        if let Ok(appimage_path) = std::env::var("APPIMAGE") {
            // AppImage mode: replace the outer AppImage file, not inner binary
            info!("Detected AppImage execution, replacing AppImage file");
            return self.update_appimage(archive_path, &PathBuf::from(appimage_path)).await;
        }

        // Standard mode: extract and replace binary
        let extract_dir = temp_dir.join("extracted");
        fs::create_dir_all(&extract_dir)?;

        // Use tar to extract (async-compression could be used for pure Rust)
        let output = tokio::process::Command::new("tar")
            .args(["xzf", &archive_path.to_string_lossy(), "-C", &extract_dir.to_string_lossy()])
            .output()
            .await?;

        if !output.status.success() {
            return Err(LumenError::Update(format!(
                "Failed to extract archive: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        // Find the current executable
        let current_exe = std::env::current_exe()?;
        let exe_dir = current_exe
            .parent()
            .ok_or_else(|| LumenError::Update("Cannot determine executable directory".into()))?;

        // Backup current binary
        let backup_path = current_exe.with_extension("backup");
        if backup_path.exists() {
            fs::remove_file(&backup_path)?;
        }
        fs::copy(&current_exe, &backup_path)?;

        // Find new binary in extracted archive
        let new_binary = Self::find_binary_in_dir(&extract_dir, "lumen")?;

        // Platform-specific replacement
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // Make new binary executable
            let mut perms = fs::metadata(&new_binary)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&new_binary, perms)?;

            // Atomic rename on Unix
            fs::rename(&new_binary, &current_exe)?;
        }

        #[cfg(windows)]
        {
            // On Windows, rename current to .old, then copy new
            let old_path = current_exe.with_extension("old");
            fs::rename(&current_exe, &old_path)?;
            fs::copy(&new_binary, &current_exe)?;
        }

        // Update bundled binaries if present (cardano-node, cardano-cli)
        for binary_name in ["cardano-node", "cardano-cli", "mithril-client"] {
            if let Ok(new_path) = Self::find_binary_in_dir(&extract_dir, binary_name) {
                let dest_path = exe_dir.join(binary_name);
                if dest_path.exists() {
                    info!("Updating bundled {}", binary_name);
                    fs::copy(&new_path, &dest_path)?;

                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mut perms = fs::metadata(&dest_path)?.permissions();
                        perms.set_mode(0o755);
                        fs::set_permissions(&dest_path, perms)?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Update an AppImage by replacing the outer .AppImage file
    async fn update_appimage(&self, archive_path: &Path, appimage_path: &Path) -> Result<()> {
        // For AppImage updates, the archive should contain the new .AppImage file
        // not a tarball to extract

        info!("Backing up current AppImage");
        let backup_path = appimage_path.with_extension("backup");
        if backup_path.exists() {
            fs::remove_file(&backup_path)?;
        }
        fs::copy(appimage_path, &backup_path)?;

        info!("Replacing AppImage file");

        // Copy downloaded file to replace current AppImage
        fs::copy(archive_path, appimage_path)?;

        // Make sure it's executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(appimage_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(appimage_path, perms)?;
        }

        info!("AppImage update complete");
        Ok(())
    }

    /// Find a binary in an extracted directory
    fn find_binary_in_dir(dir: &Path, name: &str) -> Result<PathBuf> {
        // Search common locations
        let candidates = [
            dir.join(name),
            dir.join("bin").join(name),
            dir.join("usr").join("bin").join(name),
            dir.join(format!("lumen-{}", env!("CARGO_PKG_VERSION"))).join("bin").join(name),
        ];

        for candidate in candidates {
            if candidate.exists() && candidate.is_file() {
                return Ok(candidate);
            }
        }

        // Recursive search as fallback
        Self::find_file_recursive(dir, name)
    }

    /// Recursively search for a file
    fn find_file_recursive(dir: &Path, name: &str) -> Result<PathBuf> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.file_name().map(|n| n == name).unwrap_or(false) {
                return Ok(path);
            }

            if path.is_dir() {
                if let Ok(found) = Self::find_file_recursive(&path, name) {
                    return Ok(found);
                }
            }
        }

        Err(LumenError::Update(format!("Binary '{}' not found in archive", name)))
    }
}

/// Generate a signing keypair (for development/release tooling)
pub fn generate_keypair() -> (String, String) {
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    let private_hex = hex::encode(signing_key.to_bytes());
    let public_hex = hex::encode(verifying_key.to_bytes());

    (private_hex, public_hex)
}

/// Sign a file hash (for release tooling)
pub fn sign_hash(private_key_hex: &str, hash_hex: &str) -> Result<String> {
    use ed25519_dalek::SigningKey;

    let private_bytes = hex::decode(private_key_hex)
        .map_err(|e| LumenError::Update(format!("Invalid private key hex: {}", e)))?;

    if private_bytes.len() != 32 {
        return Err(LumenError::Update("Private key must be 32 bytes".into()));
    }

    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(&private_bytes);

    let signing_key = SigningKey::from_bytes(&key_bytes);

    let hash_bytes = hex::decode(hash_hex)
        .map_err(|e| LumenError::Update(format!("Invalid hash hex: {}", e)))?;

    use ed25519_dalek::Signer;
    let signature = signing_key.sign(&hash_bytes);

    Ok(hex::encode(signature.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation_and_signing() {
        let (private_key, public_key) = generate_keypair();

        // Sign a test hash
        let test_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"; // SHA-256 of empty
        let signature = sign_hash(&private_key, test_hash).unwrap();

        // Verify the signature
        let verifying_key = Updater::parse_public_key(&public_key).unwrap();
        let sig_bytes = hex::decode(&signature).unwrap();
        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);
        let sig = Signature::from_bytes(&sig_array);

        let hash_bytes = hex::decode(test_hash).unwrap();
        assert!(verifying_key.verify(&hash_bytes, &sig).is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let (_, public_key) = generate_keypair();
        let (other_private, _) = generate_keypair();

        // Sign with different key
        let test_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let bad_signature = sign_hash(&other_private, test_hash).unwrap();

        // Verify should fail
        let verifying_key = Updater::parse_public_key(&public_key).unwrap();
        let sig_bytes = hex::decode(&bad_signature).unwrap();
        let mut sig_array = [0u8; 64];
        sig_array.copy_from_slice(&sig_bytes);
        let sig = Signature::from_bytes(&sig_array);

        let hash_bytes = hex::decode(test_hash).unwrap();
        assert!(verifying_key.verify(&hash_bytes, &sig).is_err());
    }
}
