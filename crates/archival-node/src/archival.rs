//! Archival provider for historical state access

use alloy_primitives::{Address, B256, U256};
use eyre::Result;
use futures::Future;
use parking_lot::RwLock;
use reth_primitives::{Account, BlockNumber, Header, StorageEntry};
use reth_provider::{
    AccountReader, BlockHashReader, BlockNumReader, BlockReader, HeaderProvider, StateProvider,
    StateProviderBox, StateProviderFactory,
};
use reth_storage_api::{StateProofProvider, StateRootProvider};
use reth_trie::{AccountProof, HashedPostState, MultiProof, MultiProofTargets, TrieInput};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock as AsyncRwLock;
use tracing::{debug, info, warn};

use crate::config::ArchivalConfig;

/// Enhanced archival provider with comprehensive historical state access
#[derive(Debug)]
pub struct ArchivalProvider<Provider> {
    /// Inner provider
    inner: Provider,
    
    /// Configuration
    config: ArchivalConfig,
    
    /// Historical state cache
    state_cache: Arc<AsyncRwLock<HistoricalStateCache>>,
    
    /// Proof generation service
    proof_service: Arc<ProofGenerationService>,
    
    /// Metrics collector
    metrics: Arc<RwLock<ArchivalMetrics>>,
}

/// Cache for historical states
#[derive(Debug)]
struct HistoricalStateCache {
    /// Cached block states by number
    states: BTreeMap<BlockNumber, CachedBlockState>,
    
    /// Account cache by (block_number, address)
    accounts: HashMap<(BlockNumber, Address), Option<Account>>,
    
    /// Storage cache by (block_number, address, key)
    storage: HashMap<(BlockNumber, Address, B256), U256>,
    
    /// Maximum cache size
    max_size: usize,
    
    /// Cache statistics
    stats: CacheStats,
}

/// Cached block state information
#[derive(Debug, Clone)]
struct CachedBlockState {
    /// Block number
    number: BlockNumber,
    
    /// State root
    state_root: B256,
    
    /// Block header
    header: Header,
    
    /// Last access time
    last_access: Instant,
    
    /// Access count
    access_count: u64,
}

/// Cache statistics
#[derive(Debug, Default, Clone)]
struct CacheStats {
    /// Total hits
    hits: u64,
    
    /// Total misses
    misses: u64,
    
    /// Evictions
    evictions: u64,
    
    /// Memory usage estimate (bytes)
    memory_usage: u64,
}

/// Proof generation service with parallel processing
#[derive(Debug)]
struct ProofGenerationService {
    /// Worker pool for proof generation
    workers: Arc<AsyncRwLock<Vec<ProofWorker>>>,
    
    /// Proof cache
    proof_cache: Arc<AsyncRwLock<ProofCache>>,
    
    /// Configuration
    config: ArchivalConfig,
}

/// Individual proof worker
#[derive(Debug)]
struct ProofWorker {
    /// Worker ID
    id: usize,
    
    /// Current task count
    active_tasks: u32,
    
    /// Total processed proofs
    total_proofs: u64,
    
    /// Average processing time
    avg_time: Duration,
}

/// Proof cache for recently generated proofs
#[derive(Debug)]
struct ProofCache {
    /// Cached proofs by key
    proofs: HashMap<ProofCacheKey, CachedProof>,
    
    /// LRU order
    lru: Vec<ProofCacheKey>,
    
    /// Maximum cache size
    max_size: usize,
}

/// Key for proof cache
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ProofCacheKey {
    block_number: BlockNumber,
    address: Address,
    storage_keys: Vec<B256>,
}

/// Cached proof with metadata
#[derive(Debug, Clone)]
struct CachedProof {
    /// The actual proof
    proof: AccountProof,
    
    /// Creation time
    created_at: Instant,
    
    /// Last access time
    last_access: Instant,
    
    /// Access count
    access_count: u64,
    
    /// Generation time
    generation_time: Duration,
}

/// Archival provider metrics
#[derive(Debug, Default, Clone)]
struct ArchivalMetrics {
    /// Total state queries
    total_queries: u64,
    
    /// Cache hit rate
    cache_hit_rate: f64,
    
    /// Average query time
    avg_query_time: Duration,
    
    /// Proof generation stats
    proof_stats: ProofStats,
    
    /// Memory usage
    memory_usage: u64,
}

/// Proof generation statistics
#[derive(Debug, Default, Clone)]
struct ProofStats {
    /// Total proofs generated
    total_generated: u64,
    
    /// Average generation time
    avg_generation_time: Duration,
    
    /// Cache hit rate for proofs
    cache_hit_rate: f64,
    
    /// Parallel efficiency
    parallel_efficiency: f64,
}

impl<Provider> ArchivalProvider<Provider>
where
    Provider: StateProviderFactory + BlockReader + Send + Sync + 'static,
{
    /// Create a new archival provider
    pub fn new(inner: Provider, config: ArchivalConfig) -> Self {
        let cache_size = config.memory_cache_blocks as usize;
        
        let state_cache = Arc::new(AsyncRwLock::new(HistoricalStateCache::new(cache_size)));
        
        let proof_service = Arc::new(ProofGenerationService::new(config.clone()));
        
        Self {
            inner,
            config,
            state_cache,
            proof_service,
            metrics: Arc::new(RwLock::new(ArchivalMetrics::default())),
        }
    }
    
    /// Get account at specific block with caching
    pub async fn get_account_at_block(
        &self,
        address: Address,
        block_number: BlockNumber,
    ) -> Result<Option<Account>> {
        let start_time = Instant::now();
        
        // Check cache first
        {
            let cache = self.state_cache.read().await;
            if let Some(account) = cache.accounts.get(&(block_number, address)) {
                self.update_cache_stats(true, start_time.elapsed()).await;
                return Ok(*account);
            }
        }
        
        // Cache miss - fetch from provider
        let state_provider = self.inner.state_by_block_number(block_number)?;
        let account = state_provider.basic_account(&address)?;
        
        // Update cache
        {
            let mut cache = self.state_cache.write().await;
            cache.insert_account(block_number, address, account);
        }
        
        self.update_cache_stats(false, start_time.elapsed()).await;
        Ok(account)
    }
    
    /// Get storage value at specific block with caching
    pub async fn get_storage_at_block(
        &self,
        address: Address,
        key: B256,
        block_number: BlockNumber,
    ) -> Result<U256> {
        let start_time = Instant::now();
        
        // Check cache first
        {
            let cache = self.state_cache.read().await;
            if let Some(value) = cache.storage.get(&(block_number, address, key)) {
                self.update_cache_stats(true, start_time.elapsed()).await;
                return Ok(*value);
            }
        }
        
        // Cache miss - fetch from provider
        let state_provider = self.inner.state_by_block_number(block_number)?;
        let value = state_provider.storage(address, key)?.unwrap_or_default();
        
        // Update cache
        {
            let mut cache = self.state_cache.write().await;
            cache.insert_storage(block_number, address, key, value);
        }
        
        self.update_cache_stats(false, start_time.elapsed()).await;
        Ok(value)
    }
    
    /// Generate proof with enhanced caching and parallel processing
    pub async fn generate_proof(
        &self,
        address: Address,
        storage_keys: &[B256],
        block_number: BlockNumber,
    ) -> Result<AccountProof> {
        self.proof_service.generate_proof(&self.inner, address, storage_keys, block_number).await
    }
    
    /// Get multiple proofs in parallel
    pub async fn generate_multiple_proofs(
        &self,
        requests: Vec<(Address, Vec<B256>, BlockNumber)>,
    ) -> Result<Vec<AccountProof>> {
        let mut tasks = Vec::new();
        
        for (address, storage_keys, block_number) in requests {
            let proof_service = Arc::clone(&self.proof_service);
            let inner = &self.inner;
            
            let task = tokio::spawn(async move {
                proof_service.generate_proof(inner, address, &storage_keys, block_number).await
            });
            
            tasks.push(task);
        }
        
        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await??);
        }
        
        Ok(results)
    }
    
    /// Get historical state range
    pub async fn get_state_range(
        &self,
        start_block: BlockNumber,
        end_block: BlockNumber,
        addresses: &[Address],
    ) -> Result<Vec<(BlockNumber, Vec<(Address, Option<Account>)>)>> {
        let mut results = Vec::new();
        
        for block_num in start_block..=end_block {
            let mut block_accounts = Vec::new();
            
            for &address in addresses {
                let account = self.get_account_at_block(address, block_num).await?;
                block_accounts.push((address, account));
            }
            
            results.push((block_num, block_accounts));
        }
        
        Ok(results)
    }
    
    /// Update cache statistics
    async fn update_cache_stats(&self, hit: bool, query_time: Duration) {
        let mut metrics = self.metrics.write();
        metrics.total_queries += 1;
        
        if hit {
            let cache = self.state_cache.read().await;
            let total_requests = cache.stats.hits + cache.stats.misses + 1;
            metrics.cache_hit_rate = (cache.stats.hits + 1) as f64 / total_requests as f64;
        } else {
            let cache = self.state_cache.read().await;
            let total_requests = cache.stats.hits + cache.stats.misses + 1;
            metrics.cache_hit_rate = cache.stats.hits as f64 / total_requests as f64;
        }
        
        // Update average query time
        let current_avg_nanos = metrics.avg_query_time.as_nanos() as f64;
        let new_time_nanos = query_time.as_nanos() as f64;
        let new_avg_nanos = (current_avg_nanos * (metrics.total_queries - 1) as f64 + new_time_nanos) / metrics.total_queries as f64;
        metrics.avg_query_time = Duration::from_nanos(new_avg_nanos as u64);
    }
    
    /// Get current metrics
    pub fn metrics(&self) -> ArchivalMetrics {
        self.metrics.read().clone()
    }
    
    /// Clear all caches
    pub async fn clear_caches(&self) {
        let mut cache = self.state_cache.write().await;
        cache.clear();
        
        let mut proof_cache = self.proof_service.proof_cache.write().await;
        proof_cache.clear();
        
        info!("Archival provider caches cleared");
    }
    
    /// Optimize cache by removing least recently used entries
    pub async fn optimize_cache(&self) {
        let mut cache = self.state_cache.write().await;
        cache.optimize();
        
        let mut proof_cache = self.proof_service.proof_cache.write().await;
        proof_cache.optimize();
        
        debug!("Archival provider caches optimized");
    }
}

impl HistoricalStateCache {
    fn new(max_size: usize) -> Self {
        Self {
            states: BTreeMap::new(),
            accounts: HashMap::new(),
            storage: HashMap::new(),
            max_size,
            stats: CacheStats::default(),
        }
    }
    
    fn insert_account(&mut self, block_number: BlockNumber, address: Address, account: Option<Account>) {
        self.accounts.insert((block_number, address), account);
        self.stats.misses += 1;
        self.maybe_evict();
    }
    
    fn insert_storage(&mut self, block_number: BlockNumber, address: Address, key: B256, value: U256) {
        self.storage.insert((block_number, address, key), value);
        self.stats.misses += 1;
        self.maybe_evict();
    }
    
    fn maybe_evict(&mut self) {
        if self.accounts.len() > self.max_size {
            // Simple eviction strategy - remove oldest entries
            let mut keys_to_remove = Vec::new();
            for key in self.accounts.keys().take(self.max_size / 4) {
                keys_to_remove.push(*key);
            }
            
            for key in keys_to_remove {
                self.accounts.remove(&key);
                self.stats.evictions += 1;
            }
        }
    }
    
    fn clear(&mut self) {
        self.states.clear();
        self.accounts.clear();
        self.storage.clear();
        self.stats = CacheStats::default();
    }
    
    fn optimize(&mut self) {
        // Remove entries older than 1 hour
        let cutoff = Instant::now() - Duration::from_secs(3600);
        
        self.states.retain(|_, state| state.last_access > cutoff);
        
        // For accounts and storage, we'd need more sophisticated tracking
        // This is a simplified version
        let accounts_len = self.accounts.len();
        if accounts_len > self.max_size * 3 / 4 {
            self.maybe_evict();
        }
    }
}

impl ProofGenerationService {
    fn new(config: ArchivalConfig) -> Self {
        let workers = (0..config.proof_workers)
            .map(|id| ProofWorker {
                id,
                active_tasks: 0,
                total_proofs: 0,
                avg_time: Duration::default(),
            })
            .collect();
        
        Self {
            workers: Arc::new(AsyncRwLock::new(workers)),
            proof_cache: Arc::new(AsyncRwLock::new(ProofCache::new(1000))),
            config,
        }
    }
    
    async fn generate_proof<Provider>(
        &self,
        provider: &Provider,
        address: Address,
        storage_keys: &[B256],
        block_number: BlockNumber,
    ) -> Result<AccountProof>
    where
        Provider: StateProviderFactory + Send + Sync,
    {
        let cache_key = ProofCacheKey {
            block_number,
            address,
            storage_keys: storage_keys.to_vec(),
        };
        
        // Check cache first
        {
            let mut cache = self.proof_cache.write().await;
            if let Some(cached) = cache.get_mut(&cache_key) {
                return Ok(cached.proof.clone());
            }
        }
        
        // Generate new proof
        let start_time = Instant::now();
        let state_provider = provider.state_by_block_number(block_number)?;
        let proof = state_provider.proof(Default::default(), address, storage_keys)?;
        let generation_time = start_time.elapsed();
        
        // Cache the result
        {
            let mut cache = self.proof_cache.write().await;
            cache.insert(cache_key, CachedProof {
                proof: proof.clone(),
                created_at: Instant::now(),
                last_access: Instant::now(),
                access_count: 1,
                generation_time,
            });
        }
        
        Ok(proof)
    }
}

impl ProofCache {
    fn new(max_size: usize) -> Self {
        Self {
            proofs: HashMap::new(),
            lru: Vec::new(),
            max_size,
        }
    }
    
    fn get_mut(&mut self, key: &ProofCacheKey) -> Option<&mut CachedProof> {
        if let Some(proof) = self.proofs.get_mut(key) {
            proof.last_access = Instant::now();
            proof.access_count += 1;
            
            // Update LRU
            if let Some(pos) = self.lru.iter().position(|k| k == key) {
                self.lru.remove(pos);
            }
            self.lru.push(key.clone());
            
            Some(proof)
        } else {
            None
        }
    }
    
    fn insert(&mut self, key: ProofCacheKey, proof: CachedProof) {
        // Evict if necessary
        while self.proofs.len() >= self.max_size && !self.lru.is_empty() {
            let oldest = self.lru.remove(0);
            self.proofs.remove(&oldest);
        }
        
        self.proofs.insert(key.clone(), proof);
        self.lru.push(key);
    }
    
    fn clear(&mut self) {
        self.proofs.clear();
        self.lru.clear();
    }
    
    fn optimize(&mut self) {
        // Remove proofs older than 5 minutes
        let cutoff = Instant::now() - Duration::from_secs(300);
        
        let mut keys_to_remove = Vec::new();
        for (key, proof) in &self.proofs {
            if proof.created_at < cutoff {
                keys_to_remove.push(key.clone());
            }
        }
        
        for key in keys_to_remove {
            self.proofs.remove(&key);
            if let Some(pos) = self.lru.iter().position(|k| k == &key) {
                self.lru.remove(pos);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_provider::test_utils::NoopProvider;

    #[test]
    fn test_historical_state_cache() {
        let mut cache = HistoricalStateCache::new(10);
        
        cache.insert_account(1, Address::default(), None);
        assert_eq!(cache.accounts.len(), 1);
        assert_eq!(cache.stats.misses, 1);
    }
    
    #[test]
    fn test_proof_cache() {
        let mut cache = ProofCache::new(5);
        
        let key = ProofCacheKey {
            block_number: 1,
            address: Address::default(),
            storage_keys: vec![B256::default()],
        };
        
        let proof = CachedProof {
            proof: AccountProof::new(Address::default()),
            created_at: Instant::now(),
            last_access: Instant::now(),
            access_count: 1,
            generation_time: Duration::from_millis(10),
        };
        
        cache.insert(key.clone(), proof);
        assert!(cache.get_mut(&key).is_some());
    }
    
    #[tokio::test]
    async fn test_archival_provider_creation() {
        let provider = NoopProvider::default();
        let config = ArchivalConfig::default();
        
        let archival_provider = ArchivalProvider::new(provider, config);
        let metrics = archival_provider.metrics();
        
        assert_eq!(metrics.total_queries, 0);
        assert_eq!(metrics.cache_hit_rate, 0.0);
    }
}
