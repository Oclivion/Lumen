//! Lumen - Self-contained, auto-updating Cardano node distribution
//!
//! This orchestrator manages the cardano-node process, handles automatic updates,
//! and provides Mithril snapshot support for fast initial sync.

mod binary_manager;
mod config;
mod error;
mod mithril;
mod node_manager;
mod system_check;
mod system_detect;
mod updater;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::EnvFilter;

use crate::binary_manager::BinaryManager;
use crate::config::{Config, Network};
use crate::error::Result;
use crate::node_manager::NodeManager;
use crate::system_detect::SystemProfile;
use crate::updater::Updater;

#[derive(Parser)]
#[command(name = "lumen")]
#[command(author, version, about = "Self-contained Cardano node with auto-updates", long_about = None)]
struct Cli {
    /// Configuration file path
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Data directory (overrides config)
    #[arg(short, long, value_name = "DIR")]
    data_dir: Option<PathBuf>,

    /// Network to connect to
    #[arg(short, long, value_enum, default_value = "mainnet")]
    network: Network,

    /// Enable verbose logging
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Cardano node
    Start {
        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,

        /// Skip update check on startup
        #[arg(long)]
        skip_update_check: bool,

        /// Use Mithril for fast sync if no local data exists
        #[arg(long, default_value = "true")]
        mithril: bool,
    },

    /// Stop the running Cardano node
    Stop {
        /// Force kill if graceful shutdown fails
        #[arg(short, long)]
        force: bool,
    },

    /// Show node status
    Status,

    /// Check for updates
    Update {
        /// Check only, don't install
        #[arg(long)]
        check: bool,

        /// Force update even if current version is latest
        #[arg(long)]
        force: bool,
    },

    /// Download Mithril snapshot for fast sync
    Mithril {
        #[command(subcommand)]
        action: MithrilAction,
    },

    /// Initialize configuration and data directories
    Init {
        /// Overwrite existing configuration
        #[arg(long)]
        force: bool,
    },

    /// Show current configuration
    Config,

    /// Show version information
    Version,
}

#[derive(Subcommand)]
enum MithrilAction {
    /// List available snapshots
    List,

    /// Download and apply the latest snapshot
    Download {
        /// Specific snapshot digest to download
        #[arg(long)]
        digest: Option<String>,
    },

    /// Verify an existing snapshot
    Verify,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = match cli.verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive(log_level.into())
                .add_directive("hyper=warn".parse().unwrap())
                .add_directive("reqwest=warn".parse().unwrap()),
        )
        .with_target(false)
        .init();

    // Load or create configuration
    let config = Config::load_or_create(cli.config.as_deref(), cli.data_dir.as_deref(), cli.network)?;

    // GRANDMA-FRIENDLY SMART BINARY: Detect system and prepare optimal cardano-node
    info!("🚀 Starting Lumen v{} - Network: {:?}", env!("CARGO_PKG_VERSION"), config.network);

    let system_profile = SystemProfile::detect()?;
    let binary_manager = BinaryManager::new(config.clone());

    // Ensure we have optimal cardano-node and cardano-cli binaries for this system
    let cardano_node_path = binary_manager.get_optimal_cardano_node(&system_profile).await?;
    info!("🎯 Using cardano-node: {}", cardano_node_path.display());

    let cardano_cli_path = binary_manager.get_cardano_cli(&system_profile)?;
    info!("🎯 Using cardano-cli: {}", cardano_cli_path.display());

    match cli.command {
        Commands::Start {
            foreground,
            skip_update_check,
            mithril,
        } => {
            let mut manager = NodeManager::new_with_binaries(config.clone(), cardano_node_path.clone(), cardano_cli_path.clone())?;

            // Check for updates unless skipped
            if !skip_update_check {
                let updater = Updater::new(config.clone());
                if let Some(update) = updater.check_for_update().await? {
                    info!(
                        "Update available: {} -> {}",
                        env!("CARGO_PKG_VERSION"),
                        update.version
                    );
                    // In a real implementation, prompt user or auto-update based on config
                }
            }

            // Check if Mithril sync is needed
            if mithril && !manager.has_chain_data() {
                info!("No chain data found. Initiating Mithril fast sync...");
                let mithril_client = mithril::MithrilClient::new(config.clone());
                mithril_client.download_latest_snapshot().await?;
            }

            manager.start(foreground).await?;
        }

        Commands::Stop { force } => {
            let manager = NodeManager::new_with_binaries(config, cardano_node_path.clone(), cardano_cli_path.clone())?;
            manager.stop(force).await?;
        }

        Commands::Status => {
            let manager = NodeManager::new_with_binaries(config, cardano_node_path.clone(), cardano_cli_path.clone())?;
            let status = manager.status().await?;
            println!("{}", status);
        }

        Commands::Update { check, force } => {
            let updater = Updater::new(config);

            if check {
                match updater.check_for_update().await? {
                    Some(update) => {
                        println!("Update available: {}", update.version);
                        println!("Release notes:\n{}", update.release_notes);
                        println!("\nRun 'lumen update' to install.");
                    }
                    None => {
                        println!("Already running the latest version.");
                    }
                }
            } else {
                updater.update(force).await?;
            }
        }

        Commands::Mithril { action } => {
            let mithril_client = mithril::MithrilClient::new(config);

            match action {
                MithrilAction::List => {
                    let snapshots = mithril_client.list_snapshots().await?;
                    for snapshot in snapshots {
                        println!(
                            "{} | Epoch {} | {} bytes | {}",
                            snapshot.digest,
                            snapshot.epoch(),
                            snapshot.size,
                            snapshot.created_at
                        );
                    }
                }
                MithrilAction::Download { digest } => {
                    if let Some(digest) = digest {
                        mithril_client.download_snapshot(&digest).await?;
                    } else {
                        mithril_client.download_latest_snapshot().await?;
                    }
                }
                MithrilAction::Verify => {
                    mithril_client.verify_snapshot().await?;
                }
            }
        }

        Commands::Init { force } => {
            Config::initialize(&config.data_dir, config.network, force)?;
            println!("Configuration initialized at: {:?}", config.data_dir);
        }

        Commands::Config => {
            println!("{}", toml::to_string_pretty(&config)?);
        }

        Commands::Version => {
            println!("Lumen v{}", env!("CARGO_PKG_VERSION"));
            println!("Cardano Node: {}", config.node_version.unwrap_or_else(|| "bundled".into()));
            println!("Network: {:?}", config.network);
            println!("Data directory: {:?}", config.data_dir);
        }
    }

    Ok(())
}
