//! Node manager - handles starting, stopping, and monitoring cardano-node

use crate::config::Config;
use crate::error::{LumenError, Result};
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::{debug, error, info, warn};

/// Status of the Cardano node
#[derive(Debug)]
pub struct NodeStatus {
    pub running: bool,
    pub pid: Option<u32>,
    pub uptime_secs: Option<u64>,
    pub sync_progress: Option<f64>,
    pub tip_slot: Option<u64>,
    pub tip_epoch: Option<u32>,
    pub peers_connected: Option<u32>,
    pub memory_mb: Option<u64>,
}

impl std::fmt::Display for NodeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.running {
            writeln!(f, "Status: Running")?;
            if let Some(pid) = self.pid {
                writeln!(f, "PID: {}", pid)?;
            }
            if let Some(uptime) = self.uptime_secs {
                let hours = uptime / 3600;
                let mins = (uptime % 3600) / 60;
                writeln!(f, "Uptime: {}h {}m", hours, mins)?;
            }
            if let Some(progress) = self.sync_progress {
                writeln!(f, "Sync Progress: {:.2}%", progress * 100.0)?;
            }
            if let Some(slot) = self.tip_slot {
                writeln!(f, "Tip Slot: {}", slot)?;
            }
            if let Some(epoch) = self.tip_epoch {
                writeln!(f, "Tip Epoch: {}", epoch)?;
            }
            if let Some(peers) = self.peers_connected {
                writeln!(f, "Peers: {}", peers)?;
            }
            if let Some(mem) = self.memory_mb {
                writeln!(f, "Memory: {} MB", mem)?;
            }
        } else {
            writeln!(f, "Status: Stopped")?;
        }
        Ok(())
    }
}

/// Manages the cardano-node process
pub struct NodeManager {
    config: Config,
    node_binary: PathBuf,
    cli_binary: PathBuf,
}

impl NodeManager {
    /// Create a new NodeManager with optimal cardano-node binary
    pub fn new_with_binary(config: Config, node_binary: PathBuf) -> Result<Self> {
        // Use provided optimal binary
        let node_binary = if config.node_binary.is_some() {
            // User explicitly specified binary takes precedence
            config.node_binary.clone().unwrap()
        } else {
            node_binary
        };

        // Find CLI binary (keep existing logic for now)
        let cli_binary = config
            .cli_binary
            .clone()
            .or_else(|| Self::find_bundled_binary("cardano-cli"))
            .or_else(|| which::which("cardano-cli").ok())
            .ok_or_else(|| LumenError::BinaryNotFound("cardano-cli".into()))?;

        debug!("Node binary: {:?}", node_binary);
        debug!("CLI binary: {:?}", cli_binary);

        Ok(Self {
            config,
            node_binary,
            cli_binary,
        })
    }

    /// Create a new NodeManager with both optimal cardano-node and cardano-cli binaries
    pub fn new_with_binaries(config: Config, node_binary: PathBuf, cli_binary: PathBuf) -> Result<Self> {
        // Use provided optimal binaries
        let node_binary = if config.node_binary.is_some() {
            config.node_binary.clone().unwrap()
        } else {
            node_binary
        };

        let cli_binary = if config.cli_binary.is_some() {
            config.cli_binary.clone().unwrap()
        } else {
            cli_binary
        };

        debug!("Node binary: {:?}", node_binary);
        debug!("CLI binary: {:?}", cli_binary);

        Ok(Self {
            config,
            node_binary,
            cli_binary,
        })
    }

    /// Create a new NodeManager (legacy method for compatibility)
    pub fn new(config: Config) -> Result<Self> {
        // Find node binary using old logic (fallback)
        let node_binary = config
            .node_binary
            .clone()
            .or_else(|| Self::find_bundled_binary("cardano-node"))
            .or_else(|| which::which("cardano-node").ok())
            .ok_or_else(|| LumenError::BinaryNotFound("cardano-node".into()))?;

        Self::new_with_binary(config, node_binary)
    }

    /// Find bundled binary relative to the executable
    fn find_bundled_binary(name: &str) -> Option<PathBuf> {
        let exe_dir = std::env::current_exe()
            .ok()?
            .parent()?
            .to_path_buf();

        // Check common bundled locations
        let candidates = [
            exe_dir.join(name),
            exe_dir.join("bin").join(name),
            exe_dir.join("..").join("lib").join("lumen").join(name),
            exe_dir.join("..").join("share").join("lumen").join("bin").join(name),
        ];

        for candidate in candidates {
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
        }

        None
    }

    /// Check if chain data exists
    pub fn has_chain_data(&self) -> bool {
        let db_path = self.config.db_path();
        // Check for immutable DB files which indicate sync progress
        let immutable_path = db_path.join("immutable");
        if immutable_path.exists() {
            if let Ok(entries) = fs::read_dir(&immutable_path) {
                return entries.count() > 0;
            }
        }
        false
    }

    /// Start the Cardano node
    pub async fn start(&mut self, foreground: bool) -> Result<()> {
        // Check if already running
        if let Some(pid) = self.read_pid() {
            if Self::process_exists(pid) {
                return Err(LumenError::NodeAlreadyRunning(pid));
            }
            // Stale PID file, remove it
            let _ = fs::remove_file(self.config.pid_file());
        }

        info!("Starting Cardano node on {:?}", self.config.network);

        // Build command arguments
        let args = self.build_node_args()?;
        debug!("Node arguments: {:?}", args);

        // Prepare log file
        let log_path = self.config.log_path().join("node.log");
        let log_file = fs::File::create(&log_path)?;

        let mut cmd = Command::new(&self.node_binary);
        cmd.args(&args)
            .current_dir(&self.config.data_dir)
            .stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file));

        // Set environment variables for RTS options
        let rts_opts = self.build_rts_options();
        if !rts_opts.is_empty() {
            cmd.env("GHCRTS", rts_opts);
        }

        if foreground {
            // Run in foreground - wait for completion
            info!("Running in foreground. Press Ctrl+C to stop.");
            let mut child = cmd.spawn().map_err(|e| {
                LumenError::NodeStartFailed(format!("Failed to spawn process: {}", e))
            })?;

            // Write PID file
            self.write_pid(child.id())?;

            // Wait for process
            let status = child.wait()?;
            let _ = fs::remove_file(self.config.pid_file());

            if !status.success() {
                return Err(LumenError::Node(format!(
                    "Node exited with status: {:?}",
                    status.code()
                )));
            }
        } else {
            // Daemonize
            let child = cmd.spawn().map_err(|e| {
                LumenError::NodeStartFailed(format!("Failed to spawn process: {}", e))
            })?;

            let pid = child.id();
            self.write_pid(pid)?;

            info!("Node started with PID: {}", pid);
            info!("Logs: {:?}", log_path);
            info!("Socket: {:?}", self.config.node.socket_path);

            // Wait a moment and verify it's still running
            sleep(Duration::from_secs(2)).await;

            if !Self::process_exists(pid) {
                let _ = fs::remove_file(self.config.pid_file());

                // Try to read error from log
                let log_content = fs::read_to_string(&log_path).unwrap_or_default();
                let last_lines: Vec<&str> = log_content.lines().rev().take(10).collect();

                return Err(LumenError::NodeStartFailed(format!(
                    "Node exited immediately. Last log lines:\n{}",
                    last_lines.into_iter().rev().collect::<Vec<_>>().join("\n")
                )));
            }
        }

        Ok(())
    }

    /// Stop the Cardano node
    pub async fn stop(&self, force: bool) -> Result<()> {
        let pid = self.read_pid().ok_or(LumenError::NodeNotRunning)?;

        if !Self::process_exists(pid) {
            let _ = fs::remove_file(self.config.pid_file());
            return Err(LumenError::NodeNotRunning);
        }

        info!("Stopping Cardano node (PID: {})", pid);

        let pid = Pid::from_raw(pid as i32);

        if force {
            // SIGKILL immediately
            warn!("Force killing node");
            signal::kill(pid, Signal::SIGKILL)?;
        } else {
            // Graceful shutdown with SIGINT, escalate to SIGTERM, then SIGKILL
            info!("Sending SIGINT for graceful shutdown...");
            signal::kill(pid, Signal::SIGINT)?;

            // Wait up to 30 seconds for graceful shutdown
            let graceful_timeout = Duration::from_secs(30);
            match timeout(graceful_timeout, self.wait_for_exit(pid)).await {
                Ok(_) => {
                    info!("Node stopped gracefully");
                }
                Err(_) => {
                    warn!("Graceful shutdown timed out, sending SIGTERM...");
                    signal::kill(pid, Signal::SIGTERM)?;

                    // Wait another 10 seconds
                    let term_timeout = Duration::from_secs(10);
                    match timeout(term_timeout, self.wait_for_exit(pid)).await {
                        Ok(_) => {
                            info!("Node stopped after SIGTERM");
                        }
                        Err(_) => {
                            warn!("SIGTERM timed out, sending SIGKILL...");
                            signal::kill(pid, Signal::SIGKILL)?;
                            sleep(Duration::from_secs(1)).await;
                        }
                    }
                }
            }
        }

        // Clean up PID file
        let _ = fs::remove_file(self.config.pid_file());

        // Clean up socket file
        let _ = fs::remove_file(&self.config.node.socket_path);

        info!("Node stopped");
        Ok(())
    }

    /// Get current node status
    pub async fn status(&self) -> Result<NodeStatus> {
        let pid = self.read_pid();
        let running = pid.map(Self::process_exists).unwrap_or(false);

        if !running {
            return Ok(NodeStatus {
                running: false,
                pid: None,
                uptime_secs: None,
                sync_progress: None,
                tip_slot: None,
                tip_epoch: None,
                peers_connected: None,
                memory_mb: None,
            });
        }

        let pid = pid.unwrap();

        // Get process info
        let uptime_secs = Self::get_process_uptime(pid);
        let memory_mb = Self::get_process_memory(pid);

        // Query node via CLI if socket exists
        let (sync_progress, tip_slot, tip_epoch) =
            if self.config.node.socket_path.exists() {
                self.query_tip().await.unwrap_or((None, None, None))
            } else {
                (None, None, None)
            };

        Ok(NodeStatus {
            running: true,
            pid: Some(pid),
            uptime_secs,
            sync_progress,
            tip_slot,
            tip_epoch,
            peers_connected: None, // Would need to parse logs or use different API
            memory_mb,
        })
    }

    /// Build cardano-node command arguments
    fn build_node_args(&self) -> Result<Vec<String>> {
        let mut args = vec![
            "run".to_string(),
            "--topology".to_string(),
            self.config.data_dir.join("config").join("topology.json").to_string_lossy().into(),
            "--database-path".to_string(),
            self.config.db_path().to_string_lossy().into(),
            "--socket-path".to_string(),
            self.config.node.socket_path.to_string_lossy().into(),
            "--host-addr".to_string(),
            self.config.node.host.clone(),
            "--port".to_string(),
            self.config.node.port.to_string(),
        ];

        // Network-specific config
        match self.config.network {
            crate::config::Network::Mainnet => {
                args.push("--config".to_string());
                args.push(self.get_or_download_config("mainnet")?.to_string_lossy().into());
            }
            crate::config::Network::Preview => {
                args.push("--config".to_string());
                args.push(self.get_or_download_config("preview")?.to_string_lossy().into());
                args.push("--testnet-magic".to_string());
                args.push("2".to_string());
            }
            crate::config::Network::Preprod => {
                args.push("--config".to_string());
                args.push(self.get_or_download_config("preprod")?.to_string_lossy().into());
                args.push("--testnet-magic".to_string());
                args.push("1".to_string());
            }
        }

        // Add any extra arguments
        args.extend(self.config.node.extra_args.clone());

        Ok(args)
    }

    /// Get or download network configuration file
    fn get_or_download_config(&self, network: &str) -> Result<PathBuf> {
        let config_dir = self.config.data_dir.join("config");
        let config_path = config_dir.join(format!("{}-config.json", network));

        if config_path.exists() {
            return Ok(config_path);
        }

        // Config not found - automatically download it
        info!("Network config not found, downloading automatically...");

        // Ensure config directory exists
        fs::create_dir_all(&config_dir)?;

        // Download all required config files for this network
        Config::download_network_configs(&self.config)?;

        // Verify the config was downloaded
        if config_path.exists() {
            Ok(config_path)
        } else {
            Err(LumenError::Config(format!(
                "Failed to download network config for {:?}",
                config_path
            )))
        }
    }

    /// Build GHC RTS options for memory management
    fn build_rts_options(&self) -> String {
        let mut opts = Vec::new();

        if self.config.resources.max_memory_mb > 0 {
            opts.push(format!("-M{}M", self.config.resources.max_memory_mb));
        }

        if self.config.resources.rts_threads > 0 {
            opts.push(format!("-N{}", self.config.resources.rts_threads));
        }

        if self.config.resources.memory_compaction {
            opts.push("-c".to_string());
        }

        opts.join(" ")
    }

    /// Read PID from file
    fn read_pid(&self) -> Option<u32> {
        fs::read_to_string(self.config.pid_file())
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    /// Write PID to file
    fn write_pid(&self, pid: u32) -> Result<()> {
        fs::write(self.config.pid_file(), pid.to_string())?;
        Ok(())
    }

    /// Check if a process exists
    fn process_exists(pid: u32) -> bool {
        // Send signal 0 to check if process exists
        signal::kill(Pid::from_raw(pid as i32), None).is_ok()
    }

    /// Wait for process to exit
    async fn wait_for_exit(&self, pid: Pid) {
        loop {
            if signal::kill(pid, None).is_err() {
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    /// Get process uptime in seconds
    fn get_process_uptime(pid: u32) -> Option<u64> {
        // Read from /proc on Linux
        #[cfg(target_os = "linux")]
        {
            let stat_path = format!("/proc/{}/stat", pid);
            let stat = fs::read_to_string(stat_path).ok()?;
            let parts: Vec<&str> = stat.split_whitespace().collect();

            // Field 22 is starttime in clock ticks
            let starttime: u64 = parts.get(21)?.parse().ok()?;

            // Get system uptime
            let uptime_str = fs::read_to_string("/proc/uptime").ok()?;
            let system_uptime: f64 = uptime_str.split_whitespace().next()?.parse().ok()?;

            // Get clock ticks per second (usually 100)
            let ticks_per_sec = 100u64; // sysconf(_SC_CLK_TCK)

            let process_start_secs = starttime / ticks_per_sec;
            let current_uptime = system_uptime as u64;

            Some(current_uptime.saturating_sub(process_start_secs))
        }

        #[cfg(not(target_os = "linux"))]
        None
    }

    /// Get process memory usage in MB
    fn get_process_memory(pid: u32) -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            let status_path = format!("/proc/{}/status", pid);
            let status = fs::read_to_string(status_path).ok()?;

            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    let kb: u64 = parts.get(1)?.parse().ok()?;
                    return Some(kb / 1024);
                }
            }
            None
        }

        #[cfg(not(target_os = "linux"))]
        None
    }

    /// Query node tip via cardano-cli
    async fn query_tip(&self) -> Result<(Option<f64>, Option<u64>, Option<u32>)> {
        let output = Command::new(&self.cli_binary)
            .args([
                "query",
                "tip",
                "--socket-path",
                &self.config.node.socket_path.to_string_lossy(),
            ])
            .args(match self.config.network {
                crate::config::Network::Mainnet => vec!["--mainnet"],
                crate::config::Network::Preview => vec!["--testnet-magic", "2"],
                crate::config::Network::Preprod => vec!["--testnet-magic", "1"],
            })
            .output()?;

        if !output.status.success() {
            return Ok((None, None, None));
        }

        let tip: serde_json::Value = serde_json::from_slice(&output.stdout)?;

        let sync_progress = tip
            .get("syncProgress")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(|p| p / 100.0);

        let slot = tip
            .get("slot")
            .and_then(|v| v.as_u64());

        let epoch = tip
            .get("epoch")
            .and_then(|v| v.as_u64())
            .map(|e| e as u32);

        Ok((sync_progress, slot, epoch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_display() {
        let status = NodeStatus {
            running: true,
            pid: Some(1234),
            uptime_secs: Some(3700),
            sync_progress: Some(0.9523),
            tip_slot: Some(142567890),
            tip_epoch: Some(532),
            peers_connected: Some(5),
            memory_mb: Some(4096),
        };

        let display = format!("{}", status);
        assert!(display.contains("Running"));
        assert!(display.contains("1234"));
        assert!(display.contains("95.23%"));
    }
}
