//! Main archival node implementation

use eyre::Result;
use reth_chainspec::ChainSpec;
use reth_node_builder::{NodeBuilder, NodeHandle};
use reth_node_core::node_config::NodeConfig;
use reth_node_ethereum::EthereumNode;
use reth_tasks::TaskExecutor;
use std::sync::Arc;
use tracing::{info, warn};

use crate::{
    cli::ArchivalNodeArgs,
    components::{ArchivalComponentsBuilder, ArchivalNodeComponents, ArchivalSystemStatus},
    config::ArchivalNodeConfig,
};

/// Main archival node structure
#[derive(Debug)]
pub struct ArchivalNode {
    /// Configuration
    config: ArchivalNodeConfig,
    
    /// Node handle (after launch)
    handle: Option<NodeHandle<ArchivalNodeComponents<EthereumNode>, reth_node_ethereum::EthereumAddOns>>,
    
    /// Task executor
    executor: TaskExecutor,
}

impl ArchivalNode {
    /// Create a new archival node
    pub async fn new(config: ArchivalNodeConfig) -> Result<Self> {
        config.validate()?;
        
        let executor = TaskExecutor::default();
        
        Ok(Self {
            config,
            handle: None,
            executor,
        })
    }
    
    /// Create archival node from CLI arguments
    pub async fn from_args(args: ArchivalNodeArgs) -> Result<Self> {
        args.validate()?;
        
        let config = ArchivalNodeConfig::from_args(&args)?;
        Self::new(config).await
    }
    
    /// Launch the archival node
    pub async fn launch(mut self) -> Result<Self> {
        info!("Launching archival node...");
        
        // Build the node
        let node_config = &self.config.node;
        let components_builder = ArchivalComponentsBuilder::new(self.config.clone());
        
        let handle = NodeBuilder::new(node_config.clone())
            .with_types_and_provider::<EthereumNode, _>()
            .with_components(components_builder)
            .with_add_ons(EthereumNode::add_ons())
            .launch()
            .await?;
        
        // Start archival-specific services
        handle.node.start_services(&self.executor).await?;
        
        info!("Archival node launched successfully");
        self.handle = Some(handle);
        
        Ok(self)
    }
    
    /// Get the node handle
    pub fn handle(&self) -> Option<&NodeHandle<ArchivalNodeComponents<EthereumNode>, reth_node_ethereum::EthereumAddOns>> {
        self.handle.as_ref()
    }
    
    /// Get system status
    pub async fn status(&self) -> Option<ArchivalSystemStatus> {
        if let Some(handle) = &self.handle {
            Some(handle.node.get_system_status().await)
        } else {
            None
        }
    }
    
    /// Stop the node gracefully
    pub async fn stop(self) -> Result<()> {
        if let Some(handle) = self.handle {
            info!("Stopping archival node...");
            
            // Stop archival services first
            handle.node.stop_services().await?;
            
            // Stop the main node
            handle.stop().await?;
            
            info!("Archival node stopped successfully");
        }
        
        Ok(())
    }
    
    /// Wait for the node to finish
    pub async fn wait_for_shutdown(self) -> Result<()> {
        if let Some(handle) = self.handle {
            handle.wait_for_shutdown().await?;
        }
        Ok(())
    }
    
    /// Get configuration
    pub fn config(&self) -> &ArchivalNodeConfig {
        &self.config
    }
    
    /// Check if node is running
    pub fn is_running(&self) -> bool {
        self.handle.is_some()
    }
    
    /// Get task executor
    pub fn executor(&self) -> &TaskExecutor {
        &self.executor
    }
    
    /// Reload configuration
    pub async fn reload_config(&mut self, new_config: ArchivalNodeConfig) -> Result<()> {
        new_config.validate()?;
        
        if self.is_running() {
            warn!("Cannot reload configuration while node is running");
            return Err(eyre::eyre!("Node must be stopped before reloading configuration"));
        }
        
        self.config = new_config;
        info!("Configuration reloaded successfully");
        
        Ok(())
    }
    
    /// Create a test archival node
    #[cfg(test)]
    pub async fn test() -> Result<Self> {
        let config = ArchivalNodeConfig::default();
        Self::new(config).await
    }
}

impl Drop for ArchivalNode {
    fn drop(&mut self) {
        if self.handle.is_some() {
            warn!("ArchivalNode dropped without proper shutdown");
        }
    }
}

/// Builder for creating archival nodes with custom configurations
#[derive(Debug, Default)]
pub struct ArchivalNodeBuilder {
    config: Option<ArchivalNodeConfig>,
    chain_spec: Option<ChainSpec>,
    data_dir: Option<std::path::PathBuf>,
}

impl ArchivalNodeBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Set the configuration
    pub fn with_config(mut self, config: ArchivalNodeConfig) -> Self {
        self.config = Some(config);
        self
    }
    
    /// Set the chain specification
    pub fn with_chain_spec(mut self, chain_spec: ChainSpec) -> Self {
        self.chain_spec = Some(chain_spec);
        self
    }
    
    /// Set the data directory
    pub fn with_data_dir(mut self, data_dir: std::path::PathBuf) -> Self {
        self.data_dir = Some(data_dir);
        self
    }
    
    /// Enable archival mode
    pub fn with_archival_mode(mut self, enabled: bool) -> Self {
        let mut config = self.config.unwrap_or_default();
        config.archival.enabled = enabled;
        self.config = Some(config);
        self
    }
    
    /// Set WebSocket configuration
    pub fn with_websocket_config(mut self, ws_config: crate::config::WebSocketConfig) -> Self {
        let mut config = self.config.unwrap_or_default();
        config.websocket = ws_config;
        self.config = Some(config);
        self
    }
    
    /// Build the archival node
    pub async fn build(self) -> Result<ArchivalNode> {
        let mut config = self.config.unwrap_or_default();
        
        // Apply overrides
        if let Some(data_dir) = self.data_dir {
            config.storage.db_path = data_dir.join("archival");
        }
        
        ArchivalNode::new(config).await
    }
    
    /// Build and launch the archival node
    pub async fn launch(self) -> Result<ArchivalNode> {
        let node = self.build().await?;
        node.launch().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ArchivalConfig;

    #[tokio::test]
    async fn test_archival_node_creation() {
        let result = ArchivalNode::test().await;
        assert!(result.is_ok());
        
        let node = result.unwrap();
        assert!(!node.is_running());
        assert!(node.config().archival.enabled);
    }
    
    #[tokio::test]
    async fn test_archival_node_builder() {
        let result = ArchivalNodeBuilder::new()
            .with_archival_mode(true)
            .build()
            .await;
        
        assert!(result.is_ok());
        
        let node = result.unwrap();
        assert!(node.config().archival.enabled);
    }
    
    #[tokio::test]
    async fn test_config_validation() {
        let mut config = ArchivalNodeConfig::default();
        config.archival.proof_workers = 0; // Invalid
        
        let result = ArchivalNode::new(config).await;
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_config_reload() {
        let mut node = ArchivalNode::test().await.unwrap();
        let new_config = ArchivalNodeConfig::default();
        
        let result = node.reload_config(new_config).await;
        assert!(result.is_ok());
    }
}
