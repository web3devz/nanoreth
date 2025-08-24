//! Enhanced RPC module for the archival node

use alloy_primitives::{Address, B256, U256};
use alloy_rpc_types_eth::{EIP1186AccountProofResponse, Filter, Log};
use alloy_serde::JsonStorageKey;
use eyre::Result;
use jsonrpsee::{core::RpcResult, proc_macros::rpc, types::ErrorCode};
use reth_primitives::BlockNumber;
use reth_provider::{BlockReader, StateProviderFactory};
use reth_rpc_eth_types::EthApiError;
use reth_rpc_server_types::result::internal_rpc_err;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, info, warn};

use crate::{archival::ArchivalProvider, config::ArchivalRpcConfig};

/// Enhanced RPC module for archival operations
#[derive(Debug)]
pub struct ArchivalRpcModule<Provider> {
    /// Archival provider
    provider: Arc<ArchivalProvider<Provider>>,
    
    /// Configuration
    config: ArchivalRpcConfig,
    
    /// RPC metrics
    metrics: Arc<parking_lot::RwLock<RpcMetrics>>,
}

/// RPC metrics tracking
#[derive(Debug, Default, Clone)]
struct RpcMetrics {
    /// Total requests
    total_requests: u64,
    
    /// Requests by method
    method_counts: HashMap<String, u64>,
    
    /// Average response times
    avg_response_times: HashMap<String, Duration>,
    
    /// Error counts
    error_counts: HashMap<String, u64>,
    
    /// Active requests
    active_requests: u64,
}

/// Extended archival API trait
#[rpc(server)]
pub trait ArchivalApi {
    /// Get account proof for historical state
    #[method(name = "eth_getHistoricalProof")]
    async fn get_historical_proof(
        &self,
        address: Address,
        keys: Vec<JsonStorageKey>,
        block_number: BlockNumber,
    ) -> RpcResult<EIP1186AccountProofResponse>;
    
    /// Get multiple proofs in parallel
    #[method(name = "eth_getMultipleProofs")]
    async fn get_multiple_proofs(
        &self,
        requests: Vec<ProofRequest>,
    ) -> RpcResult<Vec<EIP1186AccountProofResponse>>;
    
    /// Get account state at specific block
    #[method(name = "eth_getAccountAtBlock")]
    async fn get_account_at_block(
        &self,
        address: Address,
        block_number: BlockNumber,
    ) -> RpcResult<Option<AccountState>>;
    
    /// Get storage value at specific block
    #[method(name = "eth_getStorageAtBlock")]
    async fn get_storage_at_block(
        &self,
        address: Address,
        key: JsonStorageKey,
        block_number: BlockNumber,
    ) -> RpcResult<U256>;
    
    /// Get state changes between blocks
    #[method(name = "eth_getStateChanges")]
    async fn get_state_changes(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
        addresses: Vec<Address>,
    ) -> RpcResult<Vec<StateChange>>;
    
    /// Get historical logs with extended filtering
    #[method(name = "eth_getHistoricalLogs")]
    async fn get_historical_logs(
        &self,
        filter: ExtendedLogFilter,
    ) -> RpcResult<Vec<Log>>;
    
    /// Get archival node statistics
    #[method(name = "archival_getStats")]
    async fn get_archival_stats(&self) -> RpcResult<ArchivalStats>;
    
    /// Get proof generation stats
    #[method(name = "archival_getProofStats")]
    async fn get_proof_stats(&self) -> RpcResult<ProofGenerationStats>;
    
    /// Clear archival caches
    #[method(name = "archival_clearCaches")]
    async fn clear_caches(&self) -> RpcResult<bool>;
    
    /// Optimize archival caches
    #[method(name = "archival_optimizeCaches")]
    async fn optimize_caches(&self) -> RpcResult<bool>;
}

/// Proof request for batch operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofRequest {
    pub address: Address,
    pub storage_keys: Vec<JsonStorageKey>,
    pub block_number: BlockNumber,
}

/// Account state information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    pub balance: U256,
    pub nonce: u64,
    pub code_hash: B256,
    pub storage_root: B256,
}

/// State change information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    pub address: Address,
    pub from_block: BlockNumber,
    pub to_block: BlockNumber,
    pub account_changes: Option<AccountChanges>,
    pub storage_changes: Vec<StorageChange>,
}

/// Account-level changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountChanges {
    pub balance_change: Option<(U256, U256)>,
    pub nonce_change: Option<(u64, u64)>,
    pub code_change: Option<(B256, B256)>,
}

/// Storage-level changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageChange {
    pub key: B256,
    pub old_value: U256,
    pub new_value: U256,
}

/// Extended log filter with archival-specific options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedLogFilter {
    #[serde(flatten)]
    pub base: Filter,
    
    /// Maximum number of blocks to scan
    pub max_blocks: Option<u64>,
    
    /// Include removed logs
    pub include_removed: Option<bool>,
    
    /// Batch size for processing
    pub batch_size: Option<u64>,
}

/// Archival node statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivalStats {
    pub total_blocks_indexed: u64,
    pub cache_hit_rate: f64,
    pub avg_query_time_ms: u64,
    pub memory_usage_mb: u64,
    pub disk_usage_gb: u64,
    pub active_connections: u64,
    pub total_requests: u64,
}

/// Proof generation statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofGenerationStats {
    pub total_proofs_generated: u64,
    pub avg_generation_time_ms: u64,
    pub cache_hit_rate: f64,
    pub parallel_efficiency: f64,
    pub active_workers: u32,
    pub queue_size: u32,
}

impl<Provider> ArchivalRpcModule<Provider>
where
    Provider: StateProviderFactory + BlockReader + Send + Sync + 'static,
{
    /// Create a new archival RPC module
    pub fn new(provider: Arc<ArchivalProvider<Provider>>, config: ArchivalRpcConfig) -> Self {
        Self {
            provider,
            config,
            metrics: Arc::new(parking_lot::RwLock::new(RpcMetrics::default())),
        }
    }
    
    /// Record method call for metrics
    fn record_method_call(&self, method: &str, duration: Duration) {
        let mut metrics = self.metrics.write();
        metrics.total_requests += 1;
        *metrics.method_counts.entry(method.to_string()).or_insert(0) += 1;
        
        // Update average response time
        let current_avg = metrics.avg_response_times.get(method)
            .copied()
            .unwrap_or_default();
        let count = metrics.method_counts.get(method).copied().unwrap_or(1);
        let new_avg = Duration::from_nanos(
            (current_avg.as_nanos() as u64 * (count - 1) + duration.as_nanos() as u64) / count
        );
        metrics.avg_response_times.insert(method.to_string(), new_avg);
    }
    
    /// Record error for metrics
    fn record_error(&self, method: &str) {
        let mut metrics = self.metrics.write();
        *metrics.error_counts.entry(method.to_string()).or_insert(0) += 1;
    }
    
    /// Get current RPC metrics
    pub fn metrics(&self) -> RpcMetrics {
        self.metrics.read().clone()
    }
}

impl<Provider> ArchivalApiServer for ArchivalRpcModule<Provider>
where
    Provider: StateProviderFactory + BlockReader + Send + Sync + 'static,
{
    async fn get_historical_proof(
        &self,
        address: Address,
        keys: Vec<JsonStorageKey>,
        block_number: BlockNumber,
    ) -> RpcResult<EIP1186AccountProofResponse> {
        let start = Instant::now();
        let method = "eth_getHistoricalProof";
        
        match self.get_historical_proof_impl(address, keys, block_number).await {
            Ok(result) => {
                self.record_method_call(method, start.elapsed());
                Ok(result)
            }
            Err(e) => {
                self.record_error(method);
                Err(internal_rpc_err(e.to_string()))
            }
        }
    }
    
    async fn get_multiple_proofs(
        &self,
        requests: Vec<ProofRequest>,
    ) -> RpcResult<Vec<EIP1186AccountProofResponse>> {
        let start = Instant::now();
        let method = "eth_getMultipleProofs";
        
        if requests.len() > 100 {
            self.record_error(method);
            return Err(jsonrpsee::types::ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                "Too many proof requests (max 100)",
                None::<String>,
            ));
        }
        
        match self.get_multiple_proofs_impl(requests).await {
            Ok(result) => {
                self.record_method_call(method, start.elapsed());
                Ok(result)
            }
            Err(e) => {
                self.record_error(method);
                Err(internal_rpc_err(e.to_string()))
            }
        }
    }
    
    async fn get_account_at_block(
        &self,
        address: Address,
        block_number: BlockNumber,
    ) -> RpcResult<Option<AccountState>> {
        let start = Instant::now();
        let method = "eth_getAccountAtBlock";
        
        match self.provider.get_account_at_block(address, block_number).await {
            Ok(account) => {
                self.record_method_call(method, start.elapsed());
                Ok(account.map(|acc| AccountState {
                    balance: acc.balance,
                    nonce: acc.nonce,
                    code_hash: acc.bytecode_hash.unwrap_or_default(),
                    storage_root: B256::default(), // Would need to compute this
                }))
            }
            Err(e) => {
                self.record_error(method);
                Err(internal_rpc_err(e.to_string()))
            }
        }
    }
    
    async fn get_storage_at_block(
        &self,
        address: Address,
        key: JsonStorageKey,
        block_number: BlockNumber,
    ) -> RpcResult<U256> {
        let start = Instant::now();
        let method = "eth_getStorageAtBlock";
        
        match self.provider.get_storage_at_block(address, key.as_b256(), block_number).await {
            Ok(value) => {
                self.record_method_call(method, start.elapsed());
                Ok(value)
            }
            Err(e) => {
                self.record_error(method);
                Err(internal_rpc_err(e.to_string()))
            }
        }
    }
    
    async fn get_state_changes(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
        addresses: Vec<Address>,
    ) -> RpcResult<Vec<StateChange>> {
        let start = Instant::now();
        let method = "eth_getStateChanges";
        
        if to_block <= from_block {
            self.record_error(method);
            return Err(jsonrpsee::types::ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                "to_block must be greater than from_block",
                None::<String>,
            ));
        }
        
        if to_block - from_block > 1000 {
            self.record_error(method);
            return Err(jsonrpsee::types::ErrorObjectOwned::owned(
                ErrorCode::InvalidParams.code(),
                "Block range too large (max 1000)",
                None::<String>,
            ));
        }
        
        match self.get_state_changes_impl(from_block, to_block, addresses).await {
            Ok(result) => {
                self.record_method_call(method, start.elapsed());
                Ok(result)
            }
            Err(e) => {
                self.record_error(method);
                Err(internal_rpc_err(e.to_string()))
            }
        }
    }
    
    async fn get_historical_logs(
        &self,
        filter: ExtendedLogFilter,
    ) -> RpcResult<Vec<Log>> {
        let start = Instant::now();
        let method = "eth_getHistoricalLogs";
        
        // Implementation would go here
        self.record_method_call(method, start.elapsed());
        Ok(vec![])
    }
    
    async fn get_archival_stats(&self) -> RpcResult<ArchivalStats> {
        let start = Instant::now();
        let method = "archival_getStats";
        
        let archival_metrics = self.provider.metrics();
        let rpc_metrics = self.metrics();
        
        let stats = ArchivalStats {
            total_blocks_indexed: 0, // Would need to implement
            cache_hit_rate: archival_metrics.cache_hit_rate,
            avg_query_time_ms: archival_metrics.avg_query_time.as_millis() as u64,
            memory_usage_mb: archival_metrics.memory_usage / 1024 / 1024,
            disk_usage_gb: 0, // Would need to implement
            active_connections: 0, // Would need to track
            total_requests: rpc_metrics.total_requests,
        };
        
        self.record_method_call(method, start.elapsed());
        Ok(stats)
    }
    
    async fn get_proof_stats(&self) -> RpcResult<ProofGenerationStats> {
        let start = Instant::now();
        let method = "archival_getProofStats";
        
        let archival_metrics = self.provider.metrics();
        
        let stats = ProofGenerationStats {
            total_proofs_generated: archival_metrics.proof_stats.total_generated,
            avg_generation_time_ms: archival_metrics.proof_stats.avg_generation_time.as_millis() as u64,
            cache_hit_rate: archival_metrics.proof_stats.cache_hit_rate,
            parallel_efficiency: archival_metrics.proof_stats.parallel_efficiency,
            active_workers: 0, // Would need to implement
            queue_size: 0,     // Would need to implement
        };
        
        self.record_method_call(method, start.elapsed());
        Ok(stats)
    }
    
    async fn clear_caches(&self) -> RpcResult<bool> {
        let start = Instant::now();
        let method = "archival_clearCaches";
        
        self.provider.clear_caches().await;
        
        self.record_method_call(method, start.elapsed());
        Ok(true)
    }
    
    async fn optimize_caches(&self) -> RpcResult<bool> {
        let start = Instant::now();
        let method = "archival_optimizeCaches";
        
        self.provider.optimize_cache().await;
        
        self.record_method_call(method, start.elapsed());
        Ok(true)
    }
}

impl<Provider> ArchivalRpcModule<Provider>
where
    Provider: StateProviderFactory + BlockReader + Send + Sync + 'static,
{
    /// Implementation for get_historical_proof
    async fn get_historical_proof_impl(
        &self,
        address: Address,
        keys: Vec<JsonStorageKey>,
        block_number: BlockNumber,
    ) -> Result<EIP1186AccountProofResponse> {
        let storage_keys: Vec<B256> = keys.iter().map(|k| k.as_b256()).collect();
        let proof = self.provider.generate_proof(address, &storage_keys, block_number).await?;
        Ok(proof.into_eip1186_response(keys))
    }
    
    /// Implementation for get_multiple_proofs
    async fn get_multiple_proofs_impl(
        &self,
        requests: Vec<ProofRequest>,
    ) -> Result<Vec<EIP1186AccountProofResponse>> {
        let proof_requests: Vec<(Address, Vec<B256>, BlockNumber)> = requests
            .iter()
            .map(|req| {
                let storage_keys = req.storage_keys.iter().map(|k| k.as_b256()).collect();
                (req.address, storage_keys, req.block_number)
            })
            .collect();
        
        let proofs = self.provider.generate_multiple_proofs(proof_requests).await?;
        
        let mut responses = Vec::new();
        for (proof, request) in proofs.into_iter().zip(requests.iter()) {
            responses.push(proof.into_eip1186_response(request.storage_keys.clone()));
        }
        
        Ok(responses)
    }
    
    /// Implementation for get_state_changes
    async fn get_state_changes_impl(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
        addresses: Vec<Address>,
    ) -> Result<Vec<StateChange>> {
        let state_range = self.provider.get_state_range(from_block, to_block, &addresses).await?;
        
        let mut changes = Vec::new();
        
        // Compare adjacent states to find changes
        for i in 1..state_range.len() {
            let (prev_block, prev_accounts) = &state_range[i - 1];
            let (curr_block, curr_accounts) = &state_range[i];
            
            for (j, &address) in addresses.iter().enumerate() {
                let prev_account = prev_accounts.get(j).and_then(|(_, acc)| *acc);
                let curr_account = curr_accounts.get(j).and_then(|(_, acc)| *acc);
                
                // Check for account changes
                if prev_account != curr_account {
                    let account_changes = match (prev_account, curr_account) {
                        (Some(prev), Some(curr)) => {
                            let mut changes = AccountChanges {
                                balance_change: None,
                                nonce_change: None,
                                code_change: None,
                            };
                            
                            if prev.balance != curr.balance {
                                changes.balance_change = Some((prev.balance, curr.balance));
                            }
                            
                            if prev.nonce != curr.nonce {
                                changes.nonce_change = Some((prev.nonce, curr.nonce));
                            }
                            
                            Some(changes)
                        }
                        _ => None,
                    };
                    
                    changes.push(StateChange {
                        address,
                        from_block: *prev_block,
                        to_block: *curr_block,
                        account_changes,
                        storage_changes: vec![], // Would need to implement storage comparison
                    });
                }
            }
        }
        
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{archival::ArchivalProvider, config::ArchivalConfig};
    use reth_provider::test_utils::NoopProvider;
    
    fn create_test_module() -> ArchivalRpcModule<NoopProvider> {
        let provider = NoopProvider::default();
        let archival_config = ArchivalConfig::default();
        let archival_provider = Arc::new(ArchivalProvider::new(provider, archival_config));
        let rpc_config = ArchivalRpcConfig::default();
        
        ArchivalRpcModule::new(archival_provider, rpc_config)
    }
    
    #[test]
    fn test_rpc_module_creation() {
        let module = create_test_module();
        let metrics = module.metrics();
        assert_eq!(metrics.total_requests, 0);
    }
    
    #[tokio::test]
    async fn test_get_archival_stats() {
        let module = create_test_module();
        let result = module.get_archival_stats().await;
        assert!(result.is_ok());
    }
}
