//! Configuration for the archival node

use crate::cli::ArchivalArgs;
use eyre::Result;
use reth_chainspec::ChainSpec;
use reth_db::DatabaseEnv;
use reth_node_core::node_config::NodeConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Complete configuration for the archival node
#[derive(Debug, Clone)]
pub struct ArchivalNodeConfig {
    /// Basic node configuration
    pub node: NodeConfig<ChainSpec>,
    
    /// Archival-specific configuration
    pub archival: ArchivalConfig,
    
    /// RPC configuration
    pub rpc: ArchivalRpcConfig,
    
    /// Storage configuration
    pub storage: ArchivalStorageConfig,
    
    /// WebSocket configuration
    pub websocket: WebSocketConfig,
}

/// Archival-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivalConfig {
    /// Whether archival mode is enabled
    pub enabled: bool,
    
    /// Maximum number of blocks to cache in memory
    pub memory_cache_blocks: u64,
    
    /// Maximum proof window in blocks
    pub max_proof_window: u64,
    
    /// Enable enhanced proof generation
    pub enhanced_proofs: bool,
    
    /// Enable parallel proof generation
    pub parallel_proofs: bool,
    
    /// Number of proof worker threads
    pub proof_workers: usize,
    
    /// Enable state trie caching
    pub enable_trie_cache: bool,
    
    /// Trie cache size in MB
    pub trie_cache_size_mb: u64,
}

/// RPC configuration for archival node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivalRpcConfig {
    /// HTTP server configuration
    pub http: HttpConfig,
    
    /// WebSocket server configuration  
    pub ws: WebSocketConfig,
    
    /// Maximum request size in bytes
    pub max_request_size: u32,
    
    /// Maximum response size in bytes
    pub max_response_size: u32,
    
    /// Request timeout in seconds
    pub request_timeout: u64,
    
    /// Enable proof API endpoints
    pub enable_proof_api: bool,
    
    /// Enable archival API endpoints
    pub enable_archival_api: bool,
}

/// HTTP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    /// Whether HTTP is enabled
    pub enabled: bool,
    
    /// HTTP server address
    pub addr: String,
    
    /// HTTP server port
    pub port: u16,
    
    /// CORS domains
    pub cors_domains: Vec<String>,
    
    /// Maximum connections
    pub max_connections: u32,
}

/// WebSocket server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// Whether WebSocket is enabled
    pub enabled: bool,
    
    /// WebSocket server address
    pub addr: String,
    
    /// WebSocket server port
    pub port: u16,
    
    /// Allowed origins
    pub allowed_origins: Vec<String>,
    
    /// Maximum connections
    pub max_connections: u32,
    
    /// Enable subscriptions
    pub enable_subscriptions: bool,
    
    /// Maximum subscriptions per connection
    pub max_subscriptions_per_connection: u32,
    
    /// Ping interval in seconds
    pub ping_interval: u64,
}

/// Storage configuration for archival node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivalStorageConfig {
    /// Database path
    pub db_path: PathBuf,
    
    /// Maximum database size in GB
    pub max_db_size_gb: u64,
    
    /// Enable compression
    pub enable_compression: bool,
    
    /// Sync write to disk
    pub sync_write: bool,
    
    /// Enable state snapshots
    pub enable_snapshots: bool,
    
    /// Snapshot interval in blocks
    pub snapshot_interval: u64,
    
    /// Maximum number of snapshots to keep
    pub max_snapshots: u32,
    
    /// Enable pruning for non-archival data
    pub enable_pruning: bool,
}

impl Default for ArchivalNodeConfig {
    fn default() -> Self {
        Self {
            node: NodeConfig::test(),
            archival: ArchivalConfig::default(),
            rpc: ArchivalRpcConfig::default(),
            storage: ArchivalStorageConfig::default(),
            websocket: WebSocketConfig::default(),
        }
    }
}

impl Default for ArchivalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            memory_cache_blocks: 128,
            max_proof_window: 256,
            enhanced_proofs: true,
            parallel_proofs: true,
            proof_workers: 4,
            enable_trie_cache: true,
            trie_cache_size_mb: 512,
        }
    }
}

impl Default for ArchivalRpcConfig {
    fn default() -> Self {
        Self {
            http: HttpConfig::default(),
            ws: WebSocketConfig::default(),
            max_request_size: 15 * 1024 * 1024, // 15MB
            max_response_size: 110 * 1024 * 1024, // 110MB
            request_timeout: 30,
            enable_proof_api: true,
            enable_archival_api: true,
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            addr: "127.0.0.1".to_string(),
            port: 8545,
            cors_domains: vec!["*".to_string()],
            max_connections: 500,
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            addr: "127.0.0.1".to_string(),
            port: 8546,
            allowed_origins: vec!["*".to_string()],
            max_connections: 1000,
            enable_subscriptions: true,
            max_subscriptions_per_connection: 100,
            ping_interval: 30,
        }
    }
}

impl Default for ArchivalStorageConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("./data/archival"),
            max_db_size_gb: 2000, // 2TB
            enable_compression: true,
            sync_write: false,
            enable_snapshots: true,
            snapshot_interval: 32000, // ~4.5 days
            max_snapshots: 10,
            enable_pruning: false,
        }
    }
}

impl ArchivalNodeConfig {
    /// Create configuration from CLI arguments
    pub fn from_args(args: &crate::cli::ArchivalNodeArgs) -> Result<Self> {
        let mut config = Self::default();
        
        // Apply archival settings
        config.archival.enabled = args.archival.archival_mode;
        config.archival.memory_cache_blocks = args.archival.memory_cache_blocks;
        config.archival.max_proof_window = args.archival.max_proof_window;
        config.archival.enhanced_proofs = args.archival.enhanced_proofs;
        config.archival.parallel_proofs = args.archival.parallel_proofs;
        config.archival.proof_workers = args.archival.proof_workers;
        config.archival.enable_trie_cache = args.archival.enable_trie_cache;
        config.archival.trie_cache_size_mb = args.archival.trie_cache_size_mb;
        
        // Apply WebSocket settings
        config.websocket.enabled = args.archival.enable_ws_subscriptions;
        config.websocket.max_connections = args.archival.max_ws_connections;
        
        // Apply storage path
        config.storage.db_path = args.data_dir().join("archival");
        
        Ok(config)
    }
    
    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.archival.enabled && self.archival.proof_workers == 0 {
            return Err(eyre::eyre!("Proof workers must be greater than 0 in archival mode"));
        }
        
        if self.rpc.max_request_size == 0 {
            return Err(eyre::eyre!("Max request size must be greater than 0"));
        }
        
        if self.storage.max_db_size_gb == 0 {
            return Err(eyre::eyre!("Max database size must be greater than 0"));
        }
        
        Ok(())
    }
    
    /// Save configuration to file
    pub fn save_to_file(&self, path: &PathBuf) -> Result<()> {
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
    
    /// Load configuration from file
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_default_config() {
        let config = ArchivalNodeConfig::default();
        assert!(config.validate().is_ok());
        assert!(config.archival.enabled);
        assert!(config.websocket.enabled);
    }

    #[test]
    fn test_config_serialization() {
        let config = ArchivalNodeConfig::default();
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: ArchivalNodeConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(config.archival.enabled, deserialized.archival.enabled);
    }

    #[test]
    fn test_config_file_operations() {
        let config = ArchivalNodeConfig::default();
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("config.toml");
        
        config.save_to_file(&file_path).unwrap();
        let loaded_config = ArchivalNodeConfig::load_from_file(&file_path).unwrap();
        
        assert_eq!(config.archival.enabled, loaded_config.archival.enabled);
    }
}
