//! Node components for the archival node

use eyre::Result;
use reth_consensus::FullConsensus;
use reth_evm::{execute::BlockExecutorProvider, ConfigureEvmFor};
use reth_network::NetworkPrimitives;
use reth_node_api::{FullNodeTypes, NodeTypesWithEngine};
use reth_node_builder::{
    components::{ComponentsBuilder, NodeComponents, NodeComponentsBuilder},
    BuilderContext,
};
use reth_payload_builder::PayloadBuilderHandle;
use reth_primitives::EthPrimitives;
use reth_provider::FullRpcProvider;
use reth_tasks::TaskExecutor;
use reth_transaction_pool::TransactionPool;
use std::{future::Future, sync::Arc};

use crate::{
    archival::ArchivalProvider,
    config::ArchivalNodeConfig,
    rpc::ArchivalRpcModule,
    storage::ArchivalStorageProvider,
    websocket::ArchivalWebSocketServer,
};

/// Components for the archival node
#[derive(Debug)]
pub struct ArchivalNodeComponents<Node: FullNodeTypes> {
    /// Transaction pool
    pub pool: Node::Pool,
    
    /// EVM configuration
    pub evm_config: Node::Evm,
    
    /// Block executor
    pub executor: Node::Executor,
    
    /// Consensus engine
    pub consensus: Node::Consensus,
    
    /// Network handle
    pub network: Node::Network,
    
    /// Payload builder
    pub payload_builder: Node::PayloadBuilder,
    
    /// Payload builder handle
    pub payload_builder_handle: PayloadBuilderHandle<Node::Engine>,
    
    /// Enhanced archival provider
    pub archival_provider: Arc<ArchivalProvider<Node::Provider>>,
    
    /// Archival storage provider
    pub archival_storage: Arc<ArchivalStorageProvider<Node::Provider>>,
    
    /// Enhanced RPC module
    pub archival_rpc: Arc<ArchivalRpcModule<Node::Provider>>,
    
    /// WebSocket server
    pub websocket_server: Arc<ArchivalWebSocketServer>,
}

/// Builder for archival node components
#[derive(Debug)]
pub struct ArchivalComponentsBuilder<Node: FullNodeTypes> {
    /// Configuration
    config: ArchivalNodeConfig,
    
    /// Inner components builder
    inner: ComponentsBuilder<
        Node,
        reth_transaction_pool::EthTransactionPool<Node::Provider, reth_transaction_pool::blobstore::DiskFileBlobStore>,
        reth_payload_builder::EthereumPayloadBuilder,
        reth_network::EthereumNetworkBuilder,
        reth_evm_ethereum::execute::EthExecutorProvider<EthPrimitives>,
        reth_consensus::noop::NoopConsensus,
    >,
}

impl<Node: FullNodeTypes> ArchivalComponentsBuilder<Node> {
    /// Create a new archival components builder
    pub fn new(config: ArchivalNodeConfig) -> Self {
        Self {
            config,
            inner: ComponentsBuilder::default(),
        }
    }
    
    /// Configure with custom transaction pool
    pub fn with_pool<P>(self, pool_builder: P) -> ArchivalComponentsBuilder<Node>
    where
        P: reth_node_builder::components::PoolBuilder<Node>,
    {
        Self {
            config: self.config,
            inner: self.inner.pool(pool_builder),
        }
    }
    
    /// Configure with custom EVM
    pub fn with_evm<E>(self, evm_builder: E) -> ArchivalComponentsBuilder<Node>
    where
        E: reth_node_builder::components::ExecutorBuilder<Node>,
    {
        Self {
            config: self.config,
            inner: self.inner.executor(evm_builder),
        }
    }
    
    /// Configure with custom network
    pub fn with_network<N>(self, network_builder: N) -> ArchivalComponentsBuilder<Node>
    where
        N: reth_node_builder::components::NetworkBuilder<Node>,
    {
        Self {
            config: self.config,
            inner: self.inner.network(network_builder),
        }
    }
}

impl<Node> NodeComponentsBuilder<Node> for ArchivalComponentsBuilder<Node>
where
    Node: FullNodeTypes<Types: NodeTypesWithEngine, Provider: FullRpcProvider> + 'static,
    Node::Pool: TransactionPool + Unpin + 'static,
    Node::Evm: ConfigureEvmFor<Node::Primitives> + 'static,
    Node::Executor: BlockExecutorProvider<Primitives = Node::Primitives> + 'static,
    Node::Consensus: FullConsensus<Node::Primitives> + Clone + Unpin + 'static,
    Node::Network: NetworkPrimitives + Clone + 'static,
    Node::PayloadBuilder: reth_node_builder::components::PayloadBuilderFor<Node::Types> + 'static,
{
    type Components = ArchivalNodeComponents<Node>;
    
    fn build_components(
        self,
        ctx: &BuilderContext<Node>,
    ) -> impl Future<Output = Result<Self::Components>> + Send {
        async move {
            // Build standard components first
            let components = self.inner.build_components(ctx).await?;
            
            // Create archival-specific components
            let archival_provider = Arc::new(ArchivalProvider::new(
                ctx.provider().clone(),
                self.config.archival.clone(),
            ));
            
            let archival_storage = Arc::new(ArchivalStorageProvider::new(
                ctx.provider().clone(),
                self.config.storage.clone(),
            ));
            
            let archival_rpc = Arc::new(ArchivalRpcModule::new(
                Arc::clone(&archival_provider),
                self.config.rpc.clone(),
            ));
            
            let websocket_server = Arc::new(ArchivalWebSocketServer::new(
                self.config.websocket.clone(),
            ));
            
            Ok(ArchivalNodeComponents {
                pool: components.pool,
                evm_config: components.evm_config,
                executor: components.executor,
                consensus: components.consensus,
                network: components.network,
                payload_builder: components.payload_builder,
                payload_builder_handle: components.payload_builder_handle,
                archival_provider,
                archival_storage,
                archival_rpc,
                websocket_server,
            })
        }
    }
}

impl<Node: FullNodeTypes> NodeComponents<Node> for ArchivalNodeComponents<Node> {
    type Pool = Node::Pool;
    type Evm = Node::Evm;
    type Executor = Node::Executor;
    type Consensus = Node::Consensus;
    type Network = Node::Network;
    type PayloadBuilder = Node::PayloadBuilder;
    
    fn pool(&self) -> &Self::Pool {
        &self.pool
    }
    
    fn evm_config(&self) -> &Self::Evm {
        &self.evm_config
    }
    
    fn block_executor(&self) -> &Self::Executor {
        &self.executor
    }
    
    fn consensus(&self) -> &Self::Consensus {
        &self.consensus
    }
    
    fn network(&self) -> &Self::Network {
        &self.network
    }
    
    fn payload_builder(&self) -> &Self::PayloadBuilder {
        &self.payload_builder
    }
    
    fn payload_builder_handle(&self) -> &PayloadBuilderHandle<Node::Engine> {
        &self.payload_builder_handle
    }
}

impl<Node: FullNodeTypes> ArchivalNodeComponents<Node> {
    /// Start all archival-specific services
    pub async fn start_services(&self, executor: &TaskExecutor) -> Result<()> {
        // Start WebSocket server
        {
            let mut ws_server = (*self.websocket_server).clone();
            let executor_clone = executor.clone();
            executor_clone.spawn(async move {
                if let Err(e) = ws_server.start().await {
                    tracing::error!("Failed to start WebSocket server: {}", e);
                }
            });
        }
        
        // Start background maintenance tasks
        self.start_maintenance_tasks(executor).await?;
        
        tracing::info!("Archival node services started successfully");
        Ok(())
    }
    
    /// Start background maintenance tasks
    async fn start_maintenance_tasks(&self, executor: &TaskExecutor) -> Result<()> {
        // Cache optimization task
        {
            let archival_provider = Arc::clone(&self.archival_provider);
            executor.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5 minutes
                loop {
                    interval.tick().await;
                    archival_provider.optimize_cache().await;
                }
            });
        }
        
        // Metrics reporting task
        {
            let archival_provider = Arc::clone(&self.archival_provider);
            let archival_rpc = Arc::clone(&self.archival_rpc);
            executor.spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(60)); // 1 minute
                loop {
                    interval.tick().await;
                    let archival_metrics = archival_provider.metrics();
                    let rpc_metrics = archival_rpc.metrics();
                    
                    tracing::info!(
                        "Archival metrics - Total queries: {}, Cache hit rate: {:.2}%, Avg query time: {}ms",
                        archival_metrics.total_queries,
                        archival_metrics.cache_hit_rate * 100.0,
                        archival_metrics.avg_query_time.as_millis()
                    );
                    
                    tracing::info!(
                        "RPC metrics - Total requests: {}, Active requests: {}",
                        rpc_metrics.total_requests,
                        rpc_metrics.active_requests
                    );
                }
            });
        }
        
        Ok(())
    }
    
    /// Stop all archival services
    pub async fn stop_services(&self) -> Result<()> {
        // Stop WebSocket server
        {
            let mut ws_server = (*self.websocket_server).clone();
            ws_server.stop().await?;
        }
        
        // Clear caches
        self.archival_provider.clear_caches().await;
        self.archival_storage.clear_caches();
        
        tracing::info!("Archival node services stopped successfully");
        Ok(())
    }
    
    /// Get comprehensive system status
    pub async fn get_system_status(&self) -> ArchivalSystemStatus {
        let archival_metrics = self.archival_provider.metrics();
        let rpc_metrics = self.archival_rpc.metrics();
        let storage_metrics = self.archival_storage.metrics();
        let ws_metrics = self.websocket_server.metrics();
        
        ArchivalSystemStatus {
            archival_metrics,
            rpc_metrics,
            storage_metrics,
            websocket_metrics: ws_metrics,
            uptime: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
        }
    }
}

/// Comprehensive system status
#[derive(Debug, Clone)]
pub struct ArchivalSystemStatus {
    pub archival_metrics: crate::archival::ArchivalMetrics,
    pub rpc_metrics: crate::rpc::RpcMetrics,
    pub storage_metrics: crate::storage::StorageMetrics,
    pub websocket_metrics: crate::websocket::WebSocketMetrics,
    pub uptime: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use reth_node_ethereum::EthereumNode;
    use reth_provider::test_utils::NoopProvider;
    
    #[test]
    fn test_archival_components_builder_creation() {
        let config = ArchivalNodeConfig::default();
        let _builder = ArchivalComponentsBuilder::<EthereumNode>::new(config);
    }
    
    #[tokio::test]
    async fn test_components_creation() {
        // This would require a more complex setup with actual providers
        // For now, just test that the types compile correctly
        let config = ArchivalNodeConfig::default();
        let _builder = ArchivalComponentsBuilder::<EthereumNode>::new(config);
    }
}
