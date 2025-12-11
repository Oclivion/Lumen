//! Error types for the Lumen orchestrator

use thiserror::Error;

pub type Result<T> = std::result::Result<T, LumenError>;

#[derive(Error, Debug)]
pub enum LumenError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Node error: {0}")]
    Node(String),

    #[error("Node is not running")]
    NodeNotRunning,

    #[error("Node is already running (PID: {0})")]
    NodeAlreadyRunning(u32),

    #[error("Failed to start node: {0}")]
    NodeStartFailed(String),

    #[error("Failed to stop node: {0}")]
    NodeStopFailed(String),

    #[error("Update error: {0}")]
    Update(String),

    #[error("Signature verification failed")]
    SignatureVerification,

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Mithril error: {0}")]
    Mithril(String),

    #[error("Mithril certificate verification failed")]
    MithrilCertificateInvalid,

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML serialization error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("TOML deserialization error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("Binary not found: {0}")]
    BinaryNotFound(String),

    #[error("Insufficient disk space: need {needed} GB, have {available} GB")]
    InsufficientDiskSpace { needed: u64, available: u64 },

    #[error("Process error: {0}")]
    Process(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Unsupported platform: {0}")]
    UnsupportedPlatform(String),
}

impl From<nix::Error> for LumenError {
    fn from(err: nix::Error) -> Self {
        LumenError::Process(err.to_string())
    }
}
