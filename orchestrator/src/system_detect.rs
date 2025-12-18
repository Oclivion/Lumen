//! System detection for optimal binary selection
//!
//! This module detects the target system's characteristics to select
//! the most compatible cardano-node binary from GitHub releases.

use crate::error::{LumenError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemProfile {
    pub os: String,           // "linux"
    pub arch: String,         // "x86_64", "aarch64"
    pub distro: String,       // "ubuntu", "debian", "rhel", "alpine"
    pub distro_version: String, // "22.04", "11", "8", "3.18"
    pub glibc_version: Option<String>, // "2.35", "2.31", None for musl
    pub kernel_version: String,        // "5.15.0"
    pub compatibility_tier: CompatibilityTier,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompatibilityTier {
    /// Exact binary match available (e.g., ubuntu-22.04-x64)
    Exact,
    /// Compatible family binary (e.g., ubuntu-20.04 on ubuntu-22.04)
    Compatible,
    /// Generic static binary required (e.g., Alpine, custom distros)
    Static,
    /// Unsupported system, will use bundled fallback
    Fallback,
}

impl SystemProfile {
    /// Detect current system characteristics
    pub fn detect() -> Result<Self> {
        info!("🔍 Detecting system profile for optimal binary selection...");

        let os = Self::detect_os()?;
        let arch = Self::detect_architecture()?;
        let kernel_version = Self::detect_kernel_version()?;
        let (distro, distro_version) = Self::detect_distribution()?;
        let glibc_version = Self::detect_glibc_version();

        let profile = SystemProfile {
            os: os.clone(),
            arch: arch.clone(),
            distro: distro.clone(),
            distro_version: distro_version.clone(),
            glibc_version: glibc_version.clone(),
            kernel_version,
            compatibility_tier: Self::determine_compatibility_tier(&distro, &distro_version, &glibc_version),
        };

        debug!("System profile detected: {:?}", profile);
        info!("✅ System: {} {} {} ({})", distro, distro_version, arch,
              glibc_version.as_deref().unwrap_or("musl"));

        Ok(profile)
    }

    fn detect_os() -> Result<String> {
        if cfg!(target_os = "linux") {
            Ok("linux".to_string())
        } else {
            Err(LumenError::UnsupportedPlatform(format!("OS: {}", std::env::consts::OS)))
        }
    }

    fn detect_architecture() -> Result<String> {
        let arch = std::env::consts::ARCH;
        match arch {
            "x86_64" | "aarch64" => Ok(arch.to_string()),
            _ => Err(LumenError::UnsupportedPlatform(format!("Architecture: {}", arch))),
        }
    }

    fn detect_kernel_version() -> Result<String> {
        let output = Command::new("uname")
            .arg("-r")
            .output()
            .map_err(|e| LumenError::Process(format!("Failed to get kernel version: {}", e)))?;

        let version = String::from_utf8(output.stdout)
            .map_err(|e| LumenError::Process(format!("Invalid kernel version output: {}", e)))?
            .trim()
            .to_string();

        Ok(version)
    }

    fn detect_distribution() -> Result<(String, String)> {
        // Try /etc/os-release first (modern standard)
        if let Ok(content) = fs::read_to_string("/etc/os-release") {
            if let Some((distro, version)) = Self::parse_os_release(&content) {
                return Ok((distro, version));
            }
        }

        // Fallback to legacy methods
        if let Ok((distro, version)) = Self::detect_legacy_distribution() {
            return Ok((distro, version));
        }

        // Unknown distribution
        Ok(("unknown".to_string(), "unknown".to_string()))
    }

    fn parse_os_release(content: &str) -> Option<(String, String)> {
        let mut id = None;
        let mut version_id = None;

        for line in content.lines() {
            if line.starts_with("ID=") {
                id = Some(line.strip_prefix("ID=")?.trim_matches('"').to_lowercase());
            } else if line.starts_with("VERSION_ID=") {
                version_id = Some(line.strip_prefix("VERSION_ID=")?.trim_matches('"').to_string());
            }
        }

        match (id, version_id) {
            (Some(distro), Some(version)) => Some((Self::normalize_distro_name(&distro), version)),
            _ => None,
        }
    }

    fn normalize_distro_name(distro: &str) -> String {
        match distro {
            "ubuntu" | "debian" | "alpine" => distro.to_string(),
            "rhel" | "centos" | "rocky" | "almalinux" | "fedora" => "rhel".to_string(),
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sle" => "opensuse".to_string(),
            "arch" | "manjaro" => "arch".to_string(),
            _ => "generic".to_string(),
        }
    }

    fn detect_legacy_distribution() -> Result<(String, String)> {
        // Check common release files
        let release_files = [
            "/etc/debian_version",
            "/etc/redhat-release",
            "/etc/alpine-release",
            "/etc/arch-release",
        ];

        for &file in &release_files {
            if let Ok(content) = fs::read_to_string(file) {
                if let Some((distro, version)) = Self::parse_legacy_release(file, &content) {
                    return Ok((distro, version));
                }
            }
        }

        Ok(("generic".to_string(), "unknown".to_string()))
    }

    fn parse_legacy_release(file: &str, content: &str) -> Option<(String, String)> {
        let content = content.trim();

        match file {
            "/etc/debian_version" => {
                // Could be Debian or Ubuntu
                if content.chars().next()?.is_ascii_digit() {
                    Some(("debian".to_string(), content.to_string()))
                } else {
                    Some(("debian".to_string(), "unstable".to_string()))
                }
            }
            "/etc/redhat-release" => {
                // Parse "CentOS Linux release 8.4.2105 (Core)"
                if let Some(version) = content.split_whitespace().find(|w| w.chars().next().unwrap_or('x').is_ascii_digit()) {
                    let major_version = version.split('.').next()?.to_string();
                    Some(("rhel".to_string(), major_version))
                } else {
                    Some(("rhel".to_string(), "unknown".to_string()))
                }
            }
            "/etc/alpine-release" => {
                Some(("alpine".to_string(), content.to_string()))
            }
            "/etc/arch-release" => {
                Some(("arch".to_string(), "rolling".to_string()))
            }
            _ => None,
        }
    }

    fn detect_glibc_version() -> Option<String> {
        // Try multiple approaches to detect GLIBC version

        // Method 1: ldd --version
        if let Ok(output) = Command::new("ldd").arg("--version").output() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                if let Some(version) = Self::parse_glibc_from_ldd(&stdout) {
                    return Some(version);
                }
            }
        }

        // Method 2: getconf GNU_LIBC_VERSION
        if let Ok(output) = Command::new("getconf").arg("GNU_LIBC_VERSION").output() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                if let Some(version) = stdout.split_whitespace().nth(1) {
                    return Some(version.to_string());
                }
            }
        }

        // Method 3: Check for musl
        if Command::new("ldd").arg("--help").output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map_or(false, |s| s.contains("musl"))
        {
            return None; // musl system
        }

        None // Unknown libc
    }

    fn parse_glibc_from_ldd(output: &str) -> Option<String> {
        // Parse "ldd (Ubuntu GLIBC 2.35-0ubuntu3.4) 2.35"
        for line in output.lines() {
            if line.contains("GLIBC") || line.contains("glibc") {
                // Look for version pattern like "2.35"
                for word in line.split_whitespace() {
                    if word.starts_with("2.") && word.chars().skip(2).all(|c| c.is_ascii_digit() || c == '.') {
                        return Some(word.to_string());
                    }
                }
            }
        }
        None
    }

    fn determine_compatibility_tier(distro: &str, version: &str, glibc: &Option<String>) -> CompatibilityTier {
        match distro {
            "ubuntu" => match version {
                "22.04" | "20.04" | "18.04" => CompatibilityTier::Exact,
                _ => CompatibilityTier::Compatible,
            },
            "debian" => match version {
                "11" | "10" | "12" => CompatibilityTier::Exact,
                _ => CompatibilityTier::Compatible,
            },
            "rhel" => match version {
                "8" | "9" => CompatibilityTier::Exact,
                _ => CompatibilityTier::Compatible,
            },
            "alpine" => CompatibilityTier::Static,
            "arch" => CompatibilityTier::Static,
            "generic" | "unknown" => {
                if glibc.is_none() {
                    CompatibilityTier::Static
                } else {
                    CompatibilityTier::Fallback
                }
            },
            _ => CompatibilityTier::Fallback,
        }
    }

    /// Get the optimal binary name for GitHub releases
    pub fn get_optimal_binary_name(&self, version: &str) -> String {
        match self.compatibility_tier {
            CompatibilityTier::Exact => {
                format!("cardano-node-{}-{}-{}-{}", version, self.os, self.distro, self.distro_version)
            },
            CompatibilityTier::Compatible => {
                // Use closest compatible version
                let compat_version = self.get_compatible_version();
                format!("cardano-node-{}-{}-{}-{}", version, self.os, self.distro, compat_version)
            },
            CompatibilityTier::Static => {
                format!("cardano-node-{}-{}-static", version, self.os)
            },
            CompatibilityTier::Fallback => {
                format!("cardano-node-{}-{}-static", version, self.os)
            },
        }
    }

    fn get_compatible_version(&self) -> &str {
        match self.distro.as_str() {
            "ubuntu" => {
                match self.distro_version.as_str() {
                    v if v >= "22.04" => "22.04",
                    v if v >= "20.04" => "20.04",
                    _ => "18.04",
                }
            },
            "debian" => {
                match self.distro_version.as_str() {
                    v if v >= "12" => "12",
                    v if v >= "11" => "11",
                    _ => "10",
                }
            },
            "rhel" => {
                match self.distro_version.as_str() {
                    v if v >= "9" => "9",
                    _ => "8",
                }
            },
            _ => &self.distro_version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_os_release() {
        let ubuntu_content = r#"
NAME="Ubuntu"
VERSION="22.04.1 LTS (Jammy Jellyfish)"
ID=ubuntu
ID_LIKE=debian
PRETTY_NAME="Ubuntu 22.04.1 LTS"
VERSION_ID="22.04"
        "#;

        let (distro, version) = SystemProfile::parse_os_release(ubuntu_content).unwrap();
        assert_eq!(distro, "ubuntu");
        assert_eq!(version, "22.04");
    }

    #[test]
    fn test_normalize_distro_name() {
        assert_eq!(SystemProfile::normalize_distro_name("ubuntu"), "ubuntu");
        assert_eq!(SystemProfile::normalize_distro_name("centos"), "rhel");
        assert_eq!(SystemProfile::normalize_distro_name("rocky"), "rhel");
        assert_eq!(SystemProfile::normalize_distro_name("unknown"), "generic");
    }

    #[test]
    fn test_parse_glibc_from_ldd() {
        let output = "ldd (Ubuntu GLIBC 2.35-0ubuntu3.4) 2.35";
        assert_eq!(SystemProfile::parse_glibc_from_ldd(output), Some("2.35".to_string()));
    }
}