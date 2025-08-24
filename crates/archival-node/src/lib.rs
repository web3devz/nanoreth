//! Modular Archival Ethereum Node
//! 
//! This crate provides a modular, high-performance archival Ethereum node implementation
//! with enhanced websocket support, comprehensive archival capabilities, and optimized
//! eth_getProof support.

pub mod archival;
pub mod cli;
pub mod components;
pub mod config;
pub mod node;
pub mod rpc;
pub mod storage;
pub mod websocket;
pub mod simple;

// Re-export key types for easier access
pub use archival::ArchivalProvider;
pub use cli::ArchivalNodeArgs;
pub use components::ArchivalNodeComponents;
pub use config::ArchivalNodeConfig;
pub use node::ArchivalNode;
pub use rpc::ArchivalRpcModule;
pub use storage::ArchivalStorageProvider;
pub use websocket::ArchivalWebSocketServer;
pub use simple::SimpleArchivalNode;

/// Convenience function to create a new archival node
pub fn create_archival_node(config: ArchivalNodeConfig) -> Result<ArchivalNode, Box<dyn std::error::Error>> {
    ArchivalNode::new(config)
}

use eyre::Result;

/// Version information for the archival node
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Initialize the archival node with default configuration
pub async fn init_archival_node() -> Result<ArchivalNode> {
    let config = ArchivalNodeConfig::default();
    ArchivalNode::new(config).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_archival_node_initialization() {
        let result = init_archival_node().await;
        assert!(result.is_ok());
    }
}
