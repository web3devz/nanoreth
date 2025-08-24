//! Command-line interface for the archival node

use clap::Parser;
use eyre::Result;
use reth_chainspec::ChainSpec;
use reth_node_core::args::{DatabaseArgs, LogArgs, NetworkArgs, RpcServerArgs};
use std::path::PathBuf;

/// Archival Ethereum Node
#[derive(Debug, Parser)]
#[command(name = "archival-node")]
#[command(about = "A modular archival Ethereum node with enhanced capabilities")]
pub struct ArchivalNodeArgs {
    /// The chain this node is running.
    #[arg(long, value_name = "CHAIN_OR_PATH", verbatim_doc_comment, default_value = "mainnet")]
    pub chain: String,

    /// The path to the data directory for the node.
    #[arg(long, value_name = "DATA_DIR", verbatim_doc_comment)]
    pub datadir: Option<PathBuf>,

    /// Database configuration
    #[command(flatten)]
    pub db: DatabaseArgs,

    /// Logging configuration
    #[command(flatten)]
    pub logs: LogArgs,

    /// Network configuration
    #[command(flatten)]
    pub network: NetworkArgs,

    /// RPC server configuration
    #[command(flatten)]
    pub rpc: RpcServerArgs,

    /// Archival-specific configuration
    #[command(flatten)]
    pub archival: ArchivalArgs,
}

/// Archival node specific arguments
#[derive(Debug, Parser)]
pub struct ArchivalArgs {
    /// Enable archival mode (stores all historical state)
    #[arg(long, default_value = "true")]
    pub archival_mode: bool,

    /// Maximum number of blocks to store in memory for fast access
    #[arg(long, default_value = "128")]
    pub memory_cache_blocks: u64,

    /// Enable enhanced proof generation
    #[arg(long, default_value = "true")]
    pub enhanced_proofs: bool,

    /// Maximum proof window (blocks)
    #[arg(long, default_value = "256")]
    pub max_proof_window: u64,

    /// Enable WebSocket subscriptions
    #[arg(long, default_value = "true")]
    pub enable_ws_subscriptions: bool,

    /// Maximum concurrent WebSocket connections
    #[arg(long, default_value = "1000")]
    pub max_ws_connections: u32,

    /// Enable state trie caching
    #[arg(long, default_value = "true")]
    pub enable_trie_cache: bool,

    /// Trie cache size in MB
    #[arg(long, default_value = "512")]
    pub trie_cache_size_mb: u64,

    /// Enable parallel proof generation
    #[arg(long, default_value = "true")]
    pub parallel_proofs: bool,

    /// Number of proof worker threads
    #[arg(long, default_value = "4")]
    pub proof_workers: usize,
}

impl Default for ArchivalArgs {
    fn default() -> Self {
        Self {
            archival_mode: true,
            memory_cache_blocks: 128,
            enhanced_proofs: true,
            max_proof_window: 256,
            enable_ws_subscriptions: true,
            max_ws_connections: 1000,
            enable_trie_cache: true,
            trie_cache_size_mb: 512,
            parallel_proofs: true,
            proof_workers: 4,
        }
    }
}

impl ArchivalNodeArgs {
    /// Parse arguments from command line
    pub fn parse_args() -> Self {
        Self::parse()
    }

    /// Get the chain specification
    pub fn chain_spec(&self) -> Result<ChainSpec> {
        // For now, just return mainnet as a placeholder
        // In production, you'd want to implement proper chain parsing
        Ok(reth_chainspec::MAINNET.clone())
            .map_err(|e| eyre::eyre!("Failed to parse chain spec: {}", e))
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> PathBuf {
        self.datadir
            .clone()
            .unwrap_or_else(|| PathBuf::from("./data"))
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        if self.archival.proof_workers == 0 {
            return Err(eyre::eyre!("Proof workers must be greater than 0"));
        }

        if self.archival.max_proof_window == 0 {
            return Err(eyre::eyre!("Max proof window must be greater than 0"));
        }

        if self.archival.trie_cache_size_mb == 0 {
            return Err(eyre::eyre!("Trie cache size must be greater than 0"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archival_args_default() {
        let args = ArchivalArgs::default();
        assert!(args.archival_mode);
        assert_eq!(args.memory_cache_blocks, 128);
        assert!(args.enhanced_proofs);
    }

    #[test]
    fn test_archival_node_args_validation() {
        let mut args = ArchivalNodeArgs::parse_from(&["archival-node"]);
        args.archival.proof_workers = 0;
        
        assert!(args.validate().is_err());
    }
}
