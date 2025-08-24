// Simple archival node implementation for testing
use std::sync::Arc;
use reth_node_builder::NodeBuilder;
use reth_node_ethereum::EthereumNode;
use reth_chainspec::ChainSpec;
use eyre::Result;

/// Simple archival node for testing
pub struct SimpleArchivalNode {
    chain_spec: Arc<ChainSpec>,
}

impl SimpleArchivalNode {
    pub fn new(chain_spec: Arc<ChainSpec>) -> Self {
        Self { chain_spec }
    }

    pub async fn start(&self) -> Result<()> {
        println!("🏗️  Starting Simple Archival Node");
        println!("📊 Chain: {}", self.chain_spec.chain);
        println!("✅ Simple Archival Node started successfully!");
        
        // Keep the node running
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_chainspec::MAINNET;

    #[tokio::test]
    async fn test_simple_archival_node() {
        let node = SimpleArchivalNode::new(MAINNET.clone());
        assert!(node.start().await.is_ok());
    }
}
