//! System compatibility detection and remediation
//!
//! This module provides a layered approach to ensuring Lumen works across
//! different Linux distributions and configurations:
//!
//! 1. **Detection Layer** - Identifies compatibility issues
//! 2. **Strategy Layer** - Determines appropriate remediation
//! 3. **Action Layer** - Executes fixes with proper error handling
//! 4. **Reporting Layer** - Provides user feedback

use crate::config::Config;
use crate::error::{LumenError, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

/// System compatibility issues that can be detected and potentially resolved
#[derive(Debug, Clone)]
pub enum CompatibilityIssue {
    GlibcVersionMismatch {
        required: String,
        available: String,
    },
    MissingSystemLibrary {
        name: String,
        package_hint: Option<String>,
    },
    InsufficientPermissions {
        path: PathBuf,
        required_access: String,
    },
    InsufficientResources {
        resource_type: ResourceType,
        required: u64,
        available: u64,
    },
}

#[derive(Debug, Clone)]
pub enum ResourceType {
    DiskSpaceGb,
    MemoryGb,
}

/// Strategies for resolving compatibility issues
#[derive(Debug, Clone)]
pub enum RemediationStrategy {
    SwitchToExtractedMode,
    CreateDirectoryWithFallback { path: PathBuf },
    WarnAndContinue { message: String },
    FailWithGuidance { error: String, guidance: Vec<String> },
}

/// Result of a remediation attempt
#[derive(Debug)]
pub enum RemediationResult {
    Success { message: String },
    PartialSuccess { message: String, warnings: Vec<String> },
    Failed { error: String, next_strategy: Option<RemediationStrategy> },
}

/// System environment detector - pure detection, no side effects
#[derive(Debug)]
pub struct SystemEnvironment {
    pub is_appimage: bool,
    pub glibc_version: Option<String>,
    pub available_memory_gb: Option<u64>,
    pub data_dir_writable: bool,
}

impl SystemEnvironment {
    /// Detect current system environment
    pub fn detect(config: &Config) -> Self {
        Self {
            is_appimage: Self::detect_appimage_env(),
            glibc_version: Self::detect_glibc_version(),
            available_memory_gb: Self::detect_available_memory(),
            data_dir_writable: Self::test_directory_writable(&config.data_dir),
        }
    }

    fn detect_appimage_env() -> bool {
        env::var("APPIMAGE").is_ok() || env::var("APPDIR").is_ok()
    }

    fn detect_glibc_version() -> Option<String> {
        Command::new("ldd")
            .arg("--version")
            .output()
            .ok()
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .ok()
                    .and_then(|s| {
                        s.lines()
                            .find(|line| line.contains("GLIBC"))
                            .and_then(|line| {
                                line.split_whitespace()
                                    .find(|word| word.starts_with("2."))
                                    .map(|version| version.to_string())
                            })
                    })
            })
    }

    fn detect_available_memory() -> Option<u64> {
        fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|line| line.starts_with("MemAvailable:"))
                    .and_then(|line| {
                        line.split_whitespace()
                            .nth(1)
                            .and_then(|kb_str| kb_str.parse::<u64>().ok())
                            .map(|kb| kb / 1024 / 1024) // Convert to GB
                    })
            })
    }

    fn test_directory_writable(path: &Path) -> bool {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                return false;
            }
        }

        // Test actual write capability
        let test_file = path.join(".lumen_write_test");
        match fs::write(&test_file, "test") {
            Ok(_) => {
                let _ = fs::remove_file(&test_file);
                true
            }
            Err(_) => false,
        }
    }
}

/// Issue analyzer - determines what problems exist
pub struct CompatibilityAnalyzer;

impl CompatibilityAnalyzer {
    /// Analyze system environment for compatibility issues
    pub fn analyze(env: &SystemEnvironment, config: &Config) -> Vec<CompatibilityIssue> {
        let mut issues = Vec::new();

        // Check GLIBC compatibility for AppImages
        if env.is_appimage {
            if let Some(ref version) = env.glibc_version {
                if Self::has_glibc_compatibility_risk(version) {
                    issues.push(CompatibilityIssue::GlibcVersionMismatch {
                        required: "2.31+".to_string(),
                        available: version.clone(),
                    });
                }
            }
        }

        // Check memory requirements
        if let Some(memory_gb) = env.available_memory_gb {
            if memory_gb < 4 {
                issues.push(CompatibilityIssue::InsufficientResources {
                    resource_type: ResourceType::MemoryGb,
                    required: 4,
                    available: memory_gb,
                });
            }
        }

        // Check data directory access
        if !env.data_dir_writable {
            issues.push(CompatibilityIssue::InsufficientPermissions {
                path: config.data_dir.clone(),
                required_access: "read/write".to_string(),
            });
        }

        issues
    }

    fn has_glibc_compatibility_risk(version: &str) -> bool {
        // Check for scenarios where AppImage bundled libraries might conflict
        // This is a more sophisticated check than the original implementation
        version.starts_with("2.3") && version >= "2.35"
    }
}

/// Strategy planner - determines how to fix issues
pub struct RemediationPlanner;

impl RemediationPlanner {
    /// Plan remediation strategies for detected issues
    pub fn plan_remediation(issues: &[CompatibilityIssue]) -> Vec<(CompatibilityIssue, RemediationStrategy)> {
        issues
            .iter()
            .map(|issue| {
                let strategy = match issue {
                    CompatibilityIssue::GlibcVersionMismatch { .. } => {
                        RemediationStrategy::SwitchToExtractedMode
                    }
                    CompatibilityIssue::InsufficientPermissions { path, .. } => {
                        RemediationStrategy::CreateDirectoryWithFallback { path: path.clone() }
                    }
                    CompatibilityIssue::InsufficientResources {
                        resource_type: ResourceType::MemoryGb,
                        required,
                        available,
                    } => {
                        if *available < 2 {
                            RemediationStrategy::FailWithGuidance {
                                error: format!("Insufficient memory: {}GB available, {}GB required", available, required),
                                guidance: vec![
                                    "Close other applications to free memory".to_string(),
                                    "Consider upgrading your system RAM".to_string(),
                                ],
                            }
                        } else {
                            RemediationStrategy::WarnAndContinue {
                                message: format!("Low memory detected ({}GB). 8GB recommended for optimal performance", available),
                            }
                        }
                    }
                    CompatibilityIssue::InsufficientResources {
                        resource_type: ResourceType::DiskSpaceGb,
                        required,
                        available,
                    } => {
                        RemediationStrategy::FailWithGuidance {
                            error: format!("Insufficient disk space: {}GB available, {}GB required", available, required),
                            guidance: vec![
                                "Free up disk space before running Lumen".to_string(),
                                "Consider using a different data directory with more space".to_string(),
                            ],
                        }
                    }
                    _ => RemediationStrategy::WarnAndContinue {
                        message: "Unknown compatibility issue detected".to_string(),
                    },
                };
                (issue.clone(), strategy)
            })
            .collect()
    }
}

/// Remediation executor - actually fixes issues
pub struct RemediationExecutor;

impl RemediationExecutor {
    /// Execute a remediation strategy
    pub fn execute(strategy: &RemediationStrategy) -> Result<RemediationResult> {
        match strategy {
            RemediationStrategy::SwitchToExtractedMode => {
                Self::enable_extracted_mode()
            }
            RemediationStrategy::CreateDirectoryWithFallback { path } => {
                Self::create_directory_with_fallback(path)
            }
            RemediationStrategy::WarnAndContinue { message } => {
                warn!("{}", message);
                Ok(RemediationResult::Success {
                    message: message.clone(),
                })
            }
            RemediationStrategy::FailWithGuidance { error, guidance } => {
                Err(LumenError::Config(format!("{}\n\nTroubleshooting steps:\n{}",
                    error,
                    guidance.iter().enumerate()
                        .map(|(i, step)| format!("{}. {}", i + 1, step))
                        .collect::<Vec<_>>()
                        .join("\n")
                )))
            }
        }
    }

    fn enable_extracted_mode() -> Result<RemediationResult> {
        env::set_var("APPIMAGE_EXTRACT_AND_RUN", "1");
        env::set_var("LUMEN_COMPATIBILITY_MODE", "extracted");

        debug!("Enabled AppImage extracted mode for compatibility");

        Ok(RemediationResult::Success {
            message: "Enabled compatibility mode for maximum system support".to_string(),
        })
    }

    fn create_directory_with_fallback(path: &Path) -> Result<RemediationResult> {
        match fs::create_dir_all(path) {
            Ok(_) => {
                debug!("Created directory: {:?}", path);
                Ok(RemediationResult::Success {
                    message: format!("Created data directory: {}", path.display()),
                })
            }
            Err(e) => {
                let fallback_path = env::temp_dir().join("lumen_fallback");
                match fs::create_dir_all(&fallback_path) {
                    Ok(_) => {
                        env::set_var("LUMEN_DATA_DIR", &fallback_path);
                        Ok(RemediationResult::PartialSuccess {
                            message: format!("Using fallback directory: {}", fallback_path.display()),
                            warnings: vec![
                                format!("Could not create preferred directory: {}", e),
                                "Data will be stored in temporary location".to_string(),
                            ],
                        })
                    }
                    Err(fallback_err) => {
                        Err(LumenError::Io(fallback_err))
                    }
                }
            }
        }
    }
}

/// Main system compatibility manager - coordinates all layers
pub struct SystemCompatibility;

impl SystemCompatibility {
    /// Ensure system can run Lumen with good user experience
    pub async fn ensure_working_environment(config: &Config) -> Result<()> {
        info!("🔍 Checking system compatibility...");

        // 1. Detection Phase
        let environment = SystemEnvironment::detect(config);
        debug!("Detected environment: {:?}", environment);

        // 2. Analysis Phase
        let issues = CompatibilityAnalyzer::analyze(&environment, config);

        if issues.is_empty() {
            info!("✅ System compatibility verified - ready to run!");
            return Ok(());
        }

        debug!("Found {} compatibility issues", issues.len());

        // 3. Planning Phase
        let remediation_plan = RemediationPlanner::plan_remediation(&issues);

        // 4. Execution Phase
        let mut fixed_issues = Vec::new();
        let mut warnings = Vec::new();

        for (issue, strategy) in remediation_plan {
            match RemediationExecutor::execute(&strategy) {
                Ok(RemediationResult::Success { message }) => {
                    info!("🔧 Fixed: {}", Self::issue_description(&issue));
                    debug!("Remediation: {}", message);
                    fixed_issues.push(issue);
                }
                Ok(RemediationResult::PartialSuccess { message, warnings: warn_list }) => {
                    info!("⚠️  Partial fix: {}", Self::issue_description(&issue));
                    debug!("Remediation: {}", message);
                    warnings.extend(warn_list);
                    fixed_issues.push(issue);
                }
                Ok(RemediationResult::Failed { error, next_strategy }) => {
                    warn!("Could not fix {}: {}", Self::issue_description(&issue), error);
                    if let Some(next) = next_strategy {
                        debug!("Attempting fallback strategy");
                        // Could recursively try fallback strategies here
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        // 5. Summary
        if !warnings.is_empty() {
            for warning in &warnings {
                warn!("{}", warning);
            }
        }

        let unfixed_count = issues.len() - fixed_issues.len();
        if unfixed_count > 0 {
            warn!("{} compatibility issues could not be automatically resolved", unfixed_count);
            info!("✅ System prepared with {} auto-fixes applied", fixed_issues.len());
        } else {
            info!("✅ All compatibility issues resolved automatically - ready to run!");
        }

        Ok(())
    }

    fn issue_description(issue: &CompatibilityIssue) -> String {
        match issue {
            CompatibilityIssue::GlibcVersionMismatch { required, available } => {
                format!("GLIBC compatibility (need {}, have {})", required, available)
            }
            CompatibilityIssue::MissingSystemLibrary { name, .. } => {
                format!("Missing library: {}", name)
            }
            CompatibilityIssue::InsufficientPermissions { path, required_access } => {
                format!("Insufficient {} access to {}", required_access, path.display())
            }
            CompatibilityIssue::InsufficientResources { resource_type, required, available } => {
                format!("Insufficient {:?}: need {}, have {}", resource_type, required, available)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_glibc_compatibility_risk_detection() {
        assert!(CompatibilityAnalyzer::has_glibc_compatibility_risk("2.35"));
        assert!(CompatibilityAnalyzer::has_glibc_compatibility_risk("2.39"));
        assert!(!CompatibilityAnalyzer::has_glibc_compatibility_risk("2.31"));
        assert!(!CompatibilityAnalyzer::has_glibc_compatibility_risk("2.28"));
    }

    #[test]
    fn test_remediation_planning() {
        let issues = vec![
            CompatibilityIssue::GlibcVersionMismatch {
                required: "2.31+".to_string(),
                available: "2.39".to_string(),
            },
        ];

        let plan = RemediationPlanner::plan_remediation(&issues);
        assert_eq!(plan.len(), 1);

        match &plan[0].1 {
            RemediationStrategy::SwitchToExtractedMode => {},
            _ => panic!("Wrong strategy for GLIBC issue"),
        }
    }
}