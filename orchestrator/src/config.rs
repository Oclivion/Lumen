//! Configuration management for the Lumen orchestrator

use crate::error::{LumenError, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

/// Cardano network selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Preview,
    Preprod,
}

impl Network {
    /// Get the Mithril aggregator URL for this network
    pub fn mithril_aggregator_url(&self) -> &'static str {
        match self {
            Network::Mainnet => "https://aggregator.release-mainnet.api.mithril.network/aggregator",
            Network::Preview => "https://aggregator.pre-release-preview.api.mithril.network/aggregator",
            Network::Preprod => "https://aggregator.release-preprod.api.mithril.network/aggregator",
        }
    }

    /// Get the genesis hash for this network
    pub fn genesis_hash(&self) -> &'static str {
        match self {
            Network::Mainnet => "5f20df933584822601f9e3f8c024eb5eb252fe8cefb24d1317dc3d432e940ebb",
            Network::Preview => "268ae601af8f9214804735910a3301881fbe0eec9936f7cd5d88e0a3f1a28310",
            Network::Preprod => "d4b8de7a11d929a323373cbab6c1a9bdc931beffff11db111cf9d57356ee1937",
        }
    }

    /// Default topology peers for this network
    pub fn default_topology(&self) -> Vec<TopologyPeer> {
        match self {
            Network::Mainnet => vec![
                TopologyPeer {
                    address: "relays-new.cardano-mainnet.iohk.io".into(),
                    port: 3001,
                },
            ],
            Network::Preview => vec![
                TopologyPeer {
                    address: "preview-node.play.dev.cardano.org".into(),
                    port: 3001,
                },
            ],
            Network::Preprod => vec![
                TopologyPeer {
                    address: "preprod-node.play.dev.cardano.org".into(),
                    port: 3001,
                },
            ],
        }
    }

    /// Network magic number
    pub fn magic(&self) -> u32 {
        match self {
            Network::Mainnet => 764824073,
            Network::Preview => 2,
            Network::Preprod => 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyPeer {
    pub address: String,
    pub port: u16,
}

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Selected network
    pub network: Network,

    /// Data directory for chain database and config
    pub data_dir: PathBuf,

    /// Path to cardano-node binary (None = use bundled)
    pub node_binary: Option<PathBuf>,

    /// Path to cardano-cli binary (None = use bundled)
    pub cli_binary: Option<PathBuf>,

    /// Detected node version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_version: Option<String>,

    /// Node configuration
    pub node: NodeConfig,

    /// Update configuration
    pub update: UpdateConfig,

    /// Mithril configuration
    pub mithril: MithrilConfig,

    /// Resource limits
    pub resources: ResourceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Host to bind to
    pub host: String,

    /// Port for node-to-node communication
    pub port: u16,

    /// Socket path for local IPC
    pub socket_path: PathBuf,

    /// Topology peers
    pub topology: Vec<TopologyPeer>,

    /// Additional node arguments
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Enable automatic update checks
    pub auto_check: bool,

    /// Check interval in hours
    pub check_interval_hours: u32,

    /// Update manifest URL
    pub manifest_url: String,

    /// Ed25519 public key for signature verification (hex-encoded)
    pub public_key: String,

    /// Mirrors for downloading updates
    pub mirrors: Vec<String>,

    /// Minimum version (force update if running below this)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MithrilConfig {
    /// Enable Mithril for fast sync
    pub enabled: bool,

    /// Aggregator URL (None = use network default)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aggregator_url: Option<String>,

    /// Genesis verification key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genesis_verification_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Maximum memory in MB (0 = no limit)
    pub max_memory_mb: u64,

    /// Number of RTS threads (0 = auto)
    pub rts_threads: u32,

    /// Enable memory compaction
    pub memory_compaction: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self::for_network(Network::Mainnet, None)
    }
}

impl Config {
    /// Create configuration for a specific network
    pub fn for_network(network: Network, data_dir: Option<PathBuf>) -> Self {
        let data_dir = data_dir.unwrap_or_else(|| Self::default_data_dir());
        let socket_path = data_dir.join("node.socket");

        Config {
            network,
            data_dir: data_dir.clone(),
            node_binary: None,
            cli_binary: None,
            node_version: None,
            node: NodeConfig {
                host: "0.0.0.0".into(),
                port: 3001,
                socket_path,
                topology: network.default_topology(),
                extra_args: vec![],
            },
            update: UpdateConfig {
                auto_check: true,
                check_interval_hours: 24,
                manifest_url: "https://github.com/Oclivion/Lumen/releases/latest/download/version.json".into(),
                public_key: "a8c32e3712fc17b6d99548dce6cdb6a79b1278022b01dab113fbcb4cdaadadb5".into(),
                mirrors: vec![
                    "https://github.com/Oclivion/Lumen/releases/download".into(),
                ],
                min_version: None,
            },
            mithril: MithrilConfig {
                enabled: true,
                aggregator_url: None,
                genesis_verification_key: None,
            },
            resources: ResourceConfig {
                max_memory_mb: 8192, // 8 GB default
                rts_threads: 0,      // Auto
                memory_compaction: true,
            },
        }
    }

    /// Get the default data directory
    pub fn default_data_dir() -> PathBuf {
        // Try to use directory next to the binary for better disk space utilization
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                return exe_dir.join(".lumen");
            }
        }

        // Fallback to user data directory if binary location detection fails
        dirs::data_dir()
            .map(|d| d.join("lumen"))
            .unwrap_or_else(|| PathBuf::from(".lumen"))
    }

    /// Get the default config file path
    pub fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .map(|d| d.join("lumen").join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    }

    /// Load configuration from file, or create default
    pub fn load_or_create(
        config_path: Option<&Path>,
        data_dir: Option<&Path>,
        network: Network,
    ) -> Result<Self> {
        let config_path = config_path
            .map(PathBuf::from)
            .unwrap_or_else(Self::default_config_path);

        let mut config = if config_path.exists() {
            info!("Loading configuration from {:?}", config_path);
            let content = fs::read_to_string(&config_path)?;
            toml::from_str(&content)?
        } else {
            info!("Using default configuration for {:?}", network);
            Self::for_network(network, data_dir.map(PathBuf::from))
        };

        // Override data_dir if provided
        if let Some(dir) = data_dir {
            config.data_dir = dir.to_path_buf();
            config.node.socket_path = dir.join("node.socket");
        }

        // Override network if different
        if config.network != network {
            config.network = network;
            config.node.topology = network.default_topology();
        }

        // Ensure directories exist
        fs::create_dir_all(&config.data_dir)?;
        fs::create_dir_all(config.data_dir.join("db"))?;
        fs::create_dir_all(config.data_dir.join("logs"))?;

        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Initialize a new configuration
    pub fn initialize(data_dir: &Path, network: Network, force: bool) -> Result<()> {
        let config_path = Self::default_config_path();

        if config_path.exists() && !force {
            return Err(LumenError::Config(format!(
                "Configuration already exists at {:?}. Use --force to overwrite.",
                config_path
            )));
        }

        let config = Self::for_network(network, Some(data_dir.to_path_buf()));
        config.save(&config_path)?;

        // Create network-specific config files
        Self::write_network_configs(&config)?;

        info!("Configuration initialized at {:?}", config_path);
        Ok(())
    }

    /// Write Cardano network configuration files
    fn write_network_configs(config: &Config) -> Result<()> {
        let config_dir = config.data_dir.join("config");
        fs::create_dir_all(&config_dir)?;

        // Write topology.json
        let topology = TopologyFile {
            producers: config
                .node
                .topology
                .iter()
                .map(|p| TopologyProducer {
                    addr: p.address.clone(),
                    port: p.port,
                    valency: 1,
                })
                .collect(),
        };
        let topology_path = config_dir.join("topology.json");
        fs::write(&topology_path, serde_json::to_string_pretty(&topology)?)?;

        info!("Wrote topology configuration to {:?}", topology_path);
        Ok(())
    }

    /// Get path to chain database
    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("db")
    }

    /// Get path to logs
    pub fn log_path(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Get path to PID file
    pub fn pid_file(&self) -> PathBuf {
        self.data_dir.join("node.pid")
    }

    /// Get the Mithril aggregator URL
    pub fn mithril_aggregator_url(&self) -> &str {
        self.mithril
            .aggregator_url
            .as_deref()
            .unwrap_or_else(|| self.network.mithril_aggregator_url())
    }
}

// Helper structs for topology file format
#[derive(Serialize)]
struct TopologyFile {
    #[serde(rename = "Producers")]
    producers: Vec<TopologyProducer>,
}

#[derive(Serialize)]
struct TopologyProducer {
    addr: String,
    port: u16,
    valency: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_magic() {
        assert_eq!(Network::Mainnet.magic(), 764824073);
        assert_eq!(Network::Preview.magic(), 2);
        assert_eq!(Network::Preprod.magic(), 1);
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.network, Network::Mainnet);
        assert_eq!(config.node.port, 3001);
    }
}
