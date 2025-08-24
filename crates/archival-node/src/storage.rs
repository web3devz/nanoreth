//! Archival storage provider implementation

use alloy_primitives::{Address, B256, U256};
use eyre::Result;
use parking_lot::RwLock;
use reth_db_api::transaction::DbTx;
use reth_primitives::{Account, BlockNumber, Header, StorageEntry};
use reth_storage_api::{
    AccountReader, BlockHashReader, BlockNumReader, BlockReader, HeaderProvider, StateProvider,
    StateProviderBox, StateProviderFactory, StateRootProvider,
    StateProofProvider, BlockIdReader,
};
use reth_storage_api::StateProofProvider;
use reth_trie::{AccountProof, HashedPostState, MultiProof, MultiProofTargets, TrieInput};
use reth_trie_common::AccountProof as TrieAccountProof;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::config::ArchivalStorageConfig;

/// Enhanced archival storage provider with optimized historical state access
#[derive(Debug, Clone)]
pub struct ArchivalStorageProvider<Provider> {
    /// Inner provider
    inner: Provider,
    
    /// Configuration
    config: ArchivalStorageConfig,
    
    /// State cache for recently accessed states
    state_cache: Arc<RwLock<StateCache>>,
    
    /// Proof cache for recently generated proofs
    proof_cache: Arc<RwLock<ProofCache>>,
    
    /// Metrics
    metrics: Arc<RwLock<StorageMetrics>>,
}

/// Cache for state data
#[derive(Debug)]
struct StateCache {
    /// Cached state providers by block number
    states: HashMap<BlockNumber, CachedState>,
    
    /// Maximum cache size
    max_size: usize,
    
    /// LRU tracking
    access_order: Vec<BlockNumber>,
}

/// Cached state information
#[derive(Debug, Clone)]
struct CachedState {
    /// State root
    state_root: B256,
    
    /// Cached accounts
    accounts: HashMap<Address, Option<Account>>,
    
    /// Cached storage
    storage: HashMap<(Address, B256), U256>,
    
    /// Last access time
    last_access: Instant,
}

/// Cache for proofs
#[derive(Debug)]
struct ProofCache {
    /// Cached proofs by key
    proofs: HashMap<ProofKey, CachedProof>,
    
    /// Maximum cache size
    max_size: usize,
    
    /// LRU tracking
    access_order: Vec<ProofKey>,
}

/// Key for proof cache
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ProofKey {
    block_number: BlockNumber,
    address: Address,
    storage_keys: Vec<B256>,
}

/// Cached proof
#[derive(Debug, Clone)]
struct CachedProof {
    proof: AccountProof,
    created_at: Instant,
    access_count: u64,
}

/// Storage metrics
#[derive(Debug, Default)]
#[derive(Debug, Clone)]
struct StorageMetrics {
    /// Cache hits
    cache_hits: u64,
    
    /// Cache misses
    cache_misses: u64,
    
    /// Proof generations
    proof_generations: u64,
    
    /// Average proof generation time
    avg_proof_time: Duration,
    
    /// Total queries
    total_queries: u64,
}

impl<Provider> ArchivalStorageProvider<Provider>
where
    Provider: StateProviderFactory + Send + Sync + 'static,
{
    /// Create a new archival storage provider
    pub fn new(inner: Provider, config: ArchivalStorageConfig) -> Self {
        let cache_size = (config.max_db_size_gb * 1024 * 1024 * 1024 / 1000) as usize; // Rough estimate
        
        Self {
            inner,
            config,
            state_cache: Arc::new(RwLock::new(StateCache::new(cache_size))),
            proof_cache: Arc::new(RwLock::new(ProofCache::new(cache_size / 10))),
            metrics: Arc::new(RwLock::new(StorageMetrics::default())),
        }
    }
    
    /// Get cached state or create new one
    fn get_or_create_state(&self, block_number: BlockNumber) -> Result<CachedState> {
        // Check cache first
        {
            let mut cache = self.state_cache.write();
            if let Some(state) = cache.get_mut(block_number) {
                self.metrics.write().cache_hits += 1;
                return Ok(state.clone());
            }
        }
        
        // Cache miss - load from provider
        self.metrics.write().cache_misses += 1;
        let state_provider = self.inner.state_by_block_number_or_tag(block_number.into())?;
        let state_root = state_provider.state_root(Default::default())?;
        
        let cached_state = CachedState {
            state_root,
            accounts: HashMap::new(),
            storage: HashMap::new(),
            last_access: Instant::now(),
        };
        
        // Insert into cache
        {
            let mut cache = self.state_cache.write();
            cache.insert(block_number, cached_state.clone());
        }
        
        Ok(cached_state)
    }
    
    /// Generate proof with caching
    fn generate_proof_cached(
        &self,
        block_number: BlockNumber,
        address: Address,
        storage_keys: &[B256],
    ) -> Result<AccountProof> {
        let proof_key = ProofKey {
            block_number,
            address,
            storage_keys: storage_keys.to_vec(),
        };
        
        // Check proof cache
        {
            let mut cache = self.proof_cache.write();
            if let Some(cached) = cache.get_mut(&proof_key) {
                // Check if proof is still valid (not too old)
                if cached.created_at.elapsed() < Duration::from_secs(300) {
                    cached.access_count += 1;
                    return Ok(cached.proof.clone());
                }
            }
        }
        
        // Generate new proof
        let start = Instant::now();
        let state_provider = self.inner.state_by_block_number_or_tag(block_number.into())?;
        let proof = state_provider.proof(Default::default(), address, storage_keys)?;
        let generation_time = start.elapsed();
        
        // Update metrics
        {
            let mut metrics = self.metrics.write();
            metrics.proof_generations += 1;
            metrics.avg_proof_time = Duration::from_nanos(
                (metrics.avg_proof_time.as_nanos() as u64 + generation_time.as_nanos() as u64) / 2,
            );
        }
        
        // Cache the proof
        {
            let mut cache = self.proof_cache.write();
            cache.insert(proof_key, CachedProof {
                proof: proof.clone(),
                created_at: Instant::now(),
                access_count: 1,
            });
        }
        
        Ok(proof)
    }
    
    /// Get storage metrics
    pub fn metrics(&self) -> StorageMetrics {
        (*self.metrics.read()).clone()
    }
    
    /// Clear caches
    pub fn clear_caches(&self) {
        self.state_cache.write().clear();
        self.proof_cache.write().clear();
    }
}

impl StateCache {
    fn new(max_size: usize) -> Self {
        Self {
            states: HashMap::new(),
            max_size,
            access_order: Vec::new(),
        }
    }
    
    fn get_mut(&mut self, block_number: BlockNumber) -> Option<&mut CachedState> {
        if let Some(state) = self.states.get_mut(&block_number) {
            state.last_access = Instant::now();
            // Move to end of access order
            if let Some(pos) = self.access_order.iter().position(|&x| x == block_number) {
                self.access_order.remove(pos);
            }
            self.access_order.push(block_number);
            Some(state)
        } else {
            None
        }
    }
    
    fn insert(&mut self, block_number: BlockNumber, state: CachedState) {
        // Evict old entries if necessary
        while self.states.len() >= self.max_size && !self.access_order.is_empty() {
            let oldest = self.access_order.remove(0);
            self.states.remove(&oldest);
        }
        
        self.states.insert(block_number, state);
        self.access_order.push(block_number);
    }
    
    fn clear(&mut self) {
        self.states.clear();
        self.access_order.clear();
    }
}

impl ProofCache {
    fn new(max_size: usize) -> Self {
        Self {
            proofs: HashMap::new(),
            max_size,
            access_order: Vec::new(),
        }
    }
    
    fn get_mut(&mut self, key: &ProofKey) -> Option<&mut CachedProof> {
        if let Some(proof) = self.proofs.get_mut(key) {
            // Move to end of access order
            if let Some(pos) = self.access_order.iter().position(|x| x == key) {
                self.access_order.remove(pos);
            }
            self.access_order.push(key.clone());
            Some(proof)
        } else {
            None
        }
    }
    
    fn insert(&mut self, key: ProofKey, proof: CachedProof) {
        // Evict old entries if necessary
        while self.proofs.len() >= self.max_size && !self.access_order.is_empty() {
            let oldest = self.access_order.remove(0);
            self.proofs.remove(&oldest);
        }
        
        self.proofs.insert(key.clone(), proof);
        self.access_order.push(key);
    }
    
    fn clear(&mut self) {
        self.proofs.clear();
        self.access_order.clear();
    }
}

// Implement required traits for ArchivalStorageProvider
impl<Provider> StateProviderFactory for ArchivalStorageProvider<Provider>
where
    Provider: StateProviderFactory + Send + Sync,
{
    fn latest(&self) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.latest()
    }
    
    fn state_by_block_number_or_tag(
        &self,
        number_or_tag: alloy_rpc_types_eth::BlockNumberOrTag,
    ) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.state_by_block_number_or_tag(number_or_tag)
    }

    fn history_by_block_number(
        &self,
        block_number: BlockNumber,
    ) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.history_by_block_number(block_number)
    }

    fn history_by_block_hash(
        &self,
        block_hash: alloy_primitives::FixedBytes<32>,
    ) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.history_by_block_hash(block_hash)
    }
    
    fn state_by_block_hash(
        &self,
        block_hash: B256,
    ) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.state_by_block_hash(block_hash)
    }
    
    fn state_by_block_id(
        &self,
        block_id: alloy_eips::BlockId,
    ) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.state_by_block_id(block_id)
    }
    
    fn pending(&self) -> Result<StateProviderBox, reth_storage_errors::provider::ProviderError> {
        self.inner.pending()
    }
    
    fn pending_state_by_hash(
        &self,
        block_hash: B256,
    ) -> Result<Option<StateProviderBox>, reth_storage_errors::provider::ProviderError> {
        self.inner.pending_state_by_hash(block_hash)
    }
}

// Implement BlockIdReader by delegating to inner provider
impl<Provider> BlockIdReader for ArchivalStorageProvider<Provider>
where
    Provider: BlockIdReader + Send + Sync,
{
}

// Implement BlockNumReader by delegating to inner provider  
impl<Provider> BlockNumReader for ArchivalStorageProvider<Provider>
where
    Provider: BlockNumReader + Send + Sync,
{
    fn chain_info(&self) -> Result<reth_storage_api::ChainInfo, reth_storage_errors::provider::ProviderError> {
        self.inner.chain_info()
    }

    fn best_block_number(&self) -> Result<reth_primitives::BlockNumber, reth_storage_errors::provider::ProviderError> {
        self.inner.best_block_number()
    }

    fn last_block_number(&self) -> Result<reth_primitives::BlockNumber, reth_storage_errors::provider::ProviderError> {
        self.inner.last_block_number()
    }

    fn block_number(&self, hash: alloy_primitives::FixedBytes<32>) -> Result<Option<reth_primitives::BlockNumber>, reth_storage_errors::provider::ProviderError> {
        self.inner.block_number(hash)
    }
}

// Implement BlockHashReader by delegating to inner provider
impl<Provider> BlockHashReader for ArchivalStorageProvider<Provider>
where
    Provider: BlockHashReader + Send + Sync,
{
    fn block_hash(&self, number: u64) -> Result<Option<alloy_primitives::FixedBytes<32>>, reth_storage_errors::provider::ProviderError> {
        self.inner.block_hash(number)
    }

    fn canonical_hashes_range(&self, start: reth_primitives::BlockNumber, end: reth_primitives::BlockNumber) -> Result<Vec<alloy_primitives::FixedBytes<32>>, reth_storage_errors::provider::ProviderError> {
        self.inner.canonical_hashes_range(start, end)
    }
}

impl<Provider> StateProofProvider for ArchivalStorageProvider<Provider>
where
    Provider: StateProofProvider + StateProviderFactory + Send + Sync,
{
    fn proof(
        &self,
        input: TrieInput,
        address: Address,
        slots: &[B256],
    ) -> Result<AccountProof, reth_storage_errors::provider::ProviderError> {
        // For archival provider, we enhance proof generation with caching
        // Note: This is a simplified implementation - delegate to inner provider for now
        self.inner.proof(input, address, slots)
    }
    
    fn multiproof(
        &self,
        input: TrieInput,
        targets: MultiProofTargets,
    ) -> Result<MultiProof, reth_storage_errors::provider::ProviderError> {
        self.inner.multiproof(input, targets)
    }
    
    fn witness(
        &self,
        input: TrieInput,
        target: HashedPostState,
    ) -> Result<alloy_primitives::map::B256Map<alloy_primitives::Bytes>, reth_storage_errors::provider::ProviderError> {
        self.inner.witness(input, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_provider::test_utils::NoopProvider;

    #[test]
    fn test_state_cache() {
        let mut cache = StateCache::new(2);
        let state1 = CachedState {
            state_root: B256::default(),
            accounts: HashMap::new(),
            storage: HashMap::new(),
            last_access: Instant::now(),
        };
        
        cache.insert(1, state1.clone());
        cache.insert(2, state1.clone());
        assert_eq!(cache.states.len(), 2);
        
        // This should evict block 1
        cache.insert(3, state1);
        assert_eq!(cache.states.len(), 2);
        assert!(!cache.states.contains_key(&1));
    }
    
    #[test]
    fn test_archival_storage_provider_creation() {
        let provider = NoopProvider::default();
        let config = ArchivalStorageConfig::default();
        let archival_provider = ArchivalStorageProvider::new(provider, config);
        
        let metrics = archival_provider.metrics();
        assert_eq!(metrics.cache_hits, 0);
        assert_eq!(metrics.cache_misses, 0);
    }
}
