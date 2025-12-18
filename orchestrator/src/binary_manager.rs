//! Smart binary management with GitHub releases integration
//!
//! This module handles downloading, caching, and managing optimal cardano-node
//! binaries based on system detection results.

use crate::config::Config;
use crate::error::{LumenError, Result};
use crate::system_detect::{SystemProfile, CompatibilityTier};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

const CARDANO_REPO: &str = "IntersectMBO/cardano-node";
const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_RELEASES_BASE: &str = "https://github.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub name: String,
    pub version: String,
    pub download_url: String,
    pub local_path: PathBuf,
    pub sha256: Option<String>,
    pub size: u64,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub struct BinaryManager {
    client: Client,
    cache_dir: PathBuf,
    config: Config,
}

impl BinaryManager {
    /// Create new binary manager
    pub fn new(config: Config) -> Self {
        let cache_dir = config.data_dir.join("binaries");

        Self {
            client: Client::new(),
            cache_dir,
            config,
        }
    }

    /// Get the optimal cardano-node binary for the current system
    pub async fn get_optimal_cardano_node(&self, system: &SystemProfile) -> Result<PathBuf> {
        info!("🔄 Obtaining optimal cardano-node binary...");

        // Create cache directory
        fs::create_dir_all(&self.cache_dir)
            .map_err(|e| LumenError::Io(e))?;

        // Try to get optimal binary from GitHub releases
        if let Ok(binary_path) = self.try_download_optimal_binary(system).await {
            info!("✅ Using downloaded optimal binary");
            return Ok(binary_path);
        }

        // Fallback to bundled binary
        info!("📦 Using bundled fallback binary");
        self.get_bundled_binary()
    }

    /// Get the cardano-cli binary (should be called after get_optimal_cardano_node)
    pub fn get_cardano_cli(&self, system: &SystemProfile) -> Result<PathBuf> {
        // First check if cardano-cli was cached when we downloaded cardano-node
        let latest_version = "10.5.3"; // This should match the version from get_optimal_cardano_node
        let cached_cli_path = self.cache_dir.join(format!("cardano-cli-{}", latest_version));

        if cached_cli_path.exists() {
            Ok(cached_cli_path)
        } else {
            Err(LumenError::BinaryNotFound("cardano-cli not found. Please run node setup first.".to_string()))
        }
    }

    /// Try to download optimal binary from GitHub releases
    async fn try_download_optimal_binary(&self, system: &SystemProfile) -> Result<PathBuf> {
        debug!("Attempting to download optimal binary for {:?}", system);

        // Get latest release info
        let release = self.get_latest_release().await?;
        debug!("Latest release: {}", release.tag_name);

        // Find optimal asset for this system
        let asset = self.find_optimal_asset(&release, system)?;
        info!("🎯 Found optimal binary: {}", asset.name);

        // Check if already cached and valid
        if let Ok(cached_path) = self.get_cached_binary(&asset.name, &release.tag_name) {
            // For extracted binaries, we can't easily verify size since it's different from archive
            // For now, just check that the file exists and is executable
            if cached_path.exists() {
                info!("✅ Using cached binary: {}", cached_path.display());
                return Ok(cached_path);
            } else {
                warn!("🗑️  Cached binary failed verification, re-downloading");
            }
        }

        // Download and cache the binary
        self.download_and_cache_binary(&asset.browser_download_url, &asset.name, &release.tag_name).await
    }

    /// Get latest cardano-node release from GitHub
    async fn get_latest_release(&self) -> Result<GitHubRelease> {
        let url = format!("{}/repos/{}/releases/latest", GITHUB_API_BASE, CARDANO_REPO);

        debug!("Fetching release info from: {}", url);

        let response = self.client
            .get(&url)
            .header("User-Agent", format!("Lumen/{}", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(|e| LumenError::Network(e))?;

        if !response.status().is_success() {
            return Err(LumenError::Update(format!(
                "Failed to fetch releases: HTTP {}",
                response.status()
            )));
        }

        let release: GitHubRelease = response
            .json()
            .await
            .map_err(|e| LumenError::Network(e))?;

        Ok(release)
    }

    /// Find the most optimal asset for the given system
    fn find_optimal_asset<'a>(&self, release: &'a GitHubRelease, system: &SystemProfile) -> Result<&'a GitHubAsset> {
        let preferred_names = self.get_preferred_asset_names(system, &release.tag_name);

        debug!("Looking for assets in order: {:?}", preferred_names);
        debug!("Available assets: {:?}", release.assets.iter().map(|a| &a.name).collect::<Vec<_>>());

        // Try each preferred name in order
        for preferred_name in preferred_names {
            if let Some(asset) = release.assets.iter().find(|asset| asset.name.contains(&preferred_name)) {
                return Ok(asset);
            }
        }

        Err(LumenError::Update(format!(
            "No compatible binary found for {} {} {}",
            system.distro, system.distro_version, system.arch
        )))
    }

    /// Get preferred asset names in order of preference
    fn get_preferred_asset_names(&self, system: &SystemProfile, version: &str) -> Vec<String> {
        let _version = version.trim_start_matches('v'); // Remove 'v' prefix if present
        let mut names = Vec::new();

        match system.compatibility_tier {
            CompatibilityTier::Exact => {
                // Try exact match first
                names.push(format!("{}-{}-{}", system.distro, system.distro_version, system.arch));
                names.push(format!("{}-{}", system.distro, system.distro_version));

                // Then compatible versions
                if system.distro == "ubuntu" {
                    match system.distro_version.as_str() {
                        "22.04" => names.push("ubuntu-20.04".to_string()),
                        "20.04" => names.push("ubuntu-18.04".to_string()),
                        _ => {},
                    }
                }
            },
            CompatibilityTier::Compatible => {
                // Use compatible version first
                let compat_version = self.get_compatible_version(system);
                names.push(format!("{}-{}-{}", system.distro, compat_version, system.arch));
                names.push(format!("{}-{}", system.distro, compat_version));
            },
            CompatibilityTier::Static | CompatibilityTier::Fallback => {
                // Prefer static builds
                names.push(format!("static-{}", system.arch));
                names.push("static".to_string());
                names.push(format!("musl-{}", system.arch));
                names.push("musl".to_string());
            },
        }

        // Always add generic Linux as last resort
        names.push(format!("linux-{}", system.arch));
        names.push("linux".to_string());

        debug!("Asset name preferences: {:?}", names);
        names
    }

    fn get_compatible_version<'a>(&self, system: &'a SystemProfile) -> &'a str {
        match system.distro.as_str() {
            "ubuntu" => {
                if system.distro_version.as_str() >= "22.04" { "22.04" }
                else if system.distro_version.as_str() >= "20.04" { "20.04" }
                else { "18.04" }
            },
            "debian" => {
                if system.distro_version.as_str() >= "12" { "12" }
                else if system.distro_version.as_str() >= "11" { "11" }
                else { "10" }
            },
            "rhel" => {
                if system.distro_version.as_str() >= "9" { "9" }
                else { "8" }
            },
            _ => &system.distro_version,
        }
    }

    /// Check if binary is already cached and return path
    fn get_cached_binary(&self, _asset_name: &str, version: &str) -> Result<PathBuf> {
        let cached_path = self.cache_dir.join(format!("cardano-node-{}", version));

        if cached_path.exists() {
            Ok(cached_path)
        } else {
            Err(LumenError::BinaryNotFound("Not cached".to_string()))
        }
    }

    /// Verify binary integrity (size check for now, could add SHA256)
    async fn verify_binary_integrity(&self, path: &Path, expected_size: u64) -> Result<bool> {
        let metadata = fs::metadata(path)
            .map_err(|e| LumenError::Io(e))?;

        // For now, just check size. Could add SHA256 verification if available
        Ok(metadata.len() == expected_size)
    }

    /// Download and cache a binary
    async fn download_and_cache_binary(&self, url: &str, asset_name: &str, version: &str) -> Result<PathBuf> {
        info!("⬇️  Downloading optimal binary: {}", asset_name);

        let response = self.client
            .get(url)
            .header("User-Agent", format!("Lumen/{}", env!("CARGO_PKG_VERSION")))
            .send()
            .await
            .map_err(|e| LumenError::Network(e))?;

        if !response.status().is_success() {
            return Err(LumenError::Update(format!(
                "Failed to download binary: HTTP {}",
                response.status()
            )));
        }

        let total_size = response.content_length();
        let bytes = response.bytes().await
            .map_err(|e| LumenError::Network(e))?;

        if let Some(size) = total_size {
            info!("📦 Downloaded {} bytes", size);
        }

        // Determine final path
        let binary_path = if asset_name.ends_with(".tar.gz") {
            // Extract tar.gz and find binary
            self.extract_and_cache_tarball(&bytes, asset_name, version)?
        } else {
            // Direct binary file
            let cached_path = self.cache_dir.join(format!("cardano-node-{}-{}", version, asset_name));
            fs::write(&cached_path, &bytes)
                .map_err(|e| LumenError::Io(e))?;

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&cached_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&cached_path, perms)?;
            }

            cached_path
        };

        info!("✅ Binary cached at: {}", binary_path.display());
        Ok(binary_path)
    }

    /// Extract tarball and cache the cardano-node binary
    fn extract_and_cache_tarball(&self, data: &[u8], asset_name: &str, version: &str) -> Result<PathBuf> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        info!("📂 Extracting tarball: {}", asset_name);

        // Create temporary extraction directory
        let temp_dir = self.cache_dir.join(format!("temp-{}", version));
        fs::create_dir_all(&temp_dir)
            .map_err(|e| LumenError::Io(e))?;

        // Decompress gzip
        let cursor = std::io::Cursor::new(data);
        let mut decoder = GzDecoder::new(cursor);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed)
            .map_err(|e| LumenError::Io(e))?;

        // Extract tar
        let mut archive = tar::Archive::new(std::io::Cursor::new(decompressed));
        archive.unpack(&temp_dir)
            .map_err(|e| LumenError::Io(e))?;

        // Find and cache both cardano-node and cardano-cli
        let cardano_node_path = self.find_binary_in_extraction(&temp_dir, "cardano-node")?;
        let final_node_path = self.cache_dir.join(format!("cardano-node-{}", version));
        fs::rename(&cardano_node_path, &final_node_path)
            .map_err(|e| LumenError::Io(e))?;

        // Also extract cardano-cli if present
        if let Ok(cardano_cli_path) = self.find_binary_in_extraction(&temp_dir, "cardano-cli") {
            let final_cli_path = self.cache_dir.join(format!("cardano-cli-{}", version));
            fs::rename(&cardano_cli_path, &final_cli_path)
                .map_err(|e| LumenError::Io(e))?;

            // Make cardano-cli executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&final_cli_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&final_cli_path, perms)?;
            }
        }

        // Cleanup temp directory
        let _ = fs::remove_dir_all(&temp_dir);

        // Make cardano-node executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&final_node_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&final_node_path, perms)?;
        }

        Ok(final_node_path)
    }

    /// Find binary in extracted directory
    fn find_binary_in_extraction(&self, dir: &Path, binary_name: &str) -> Result<PathBuf> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name == binary_name || name.starts_with(binary_name) {
                        return Ok(path);
                    }
                }
            } else if path.is_dir() {
                // Recursively search subdirectories
                if let Ok(found) = self.find_binary_in_extraction(&path, binary_name) {
                    return Ok(found);
                }
            }
        }

        Err(LumenError::BinaryNotFound(format!("{} not found in archive", binary_name)))
    }

    /// Get bundled fallback binary
    fn get_bundled_binary(&self) -> Result<PathBuf> {
        // For now, return an error - we'll embed a static binary later
        Err(LumenError::BinaryNotFound(
            "No bundled binary available yet. Download from GitHub failed.".to_string()
        ))
    }

    /// Clean old cached binaries to save space
    pub fn cleanup_old_binaries(&self, keep_versions: usize) -> Result<()> {
        info!("🧹 Cleaning up old cached binaries...");

        if !self.cache_dir.exists() {
            return Ok(());
        }

        let mut binaries: Vec<_> = fs::read_dir(&self.cache_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name().to_str()
                    .map_or(false, |name| name.starts_with("cardano-node-"))
            })
            .collect();

        // Sort by modification time (newest first)
        binaries.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        binaries.reverse(); // Newest first

        // Remove old binaries beyond keep_versions
        for old_binary in binaries.iter().skip(keep_versions) {
            let path = old_binary.path();
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to remove old binary {:?}: {}", path, e);
            } else {
                debug!("Removed old binary: {:?}", path);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preferred_asset_names() {
        let config = Config::default();
        let manager = BinaryManager::new(config);

        let system = SystemProfile {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            distro: "ubuntu".to_string(),
            distro_version: "22.04".to_string(),
            glibc_version: Some("2.35".to_string()),
            kernel_version: "5.15.0".to_string(),
            compatibility_tier: CompatibilityTier::Exact,
        };

        let names = manager.get_preferred_asset_names(&system, "v8.9.2");
        assert!(names.contains(&"ubuntu-22.04-x86_64".to_string()));
        assert!(names.contains(&"ubuntu-22.04".to_string()));
    }
}