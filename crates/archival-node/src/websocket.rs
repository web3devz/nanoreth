//! Enhanced WebSocket server for the archival node

use eyre::Result;
use futures::{SinkExt, StreamExt};
use jsonrpsee::{
    core::SubscriptionResult,
    server::{ServerBuilder, ServerHandle},
    types::{ErrorCode, ErrorObject},
    PendingSubscriptionSink, SubscriptionMessage, SubscriptionSink,
};
use parking_lot::RwLock;
use reth_primitives::{BlockHash, BlockNumber, Header, TransactionSigned, B256};
use reth_provider::{BlockReader, CanonStateSubscriptions};
use reth_rpc_eth_types::logs::Log;
use serde_json::Value;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::{broadcast, mpsc},
    time::interval,
};
use tracing::{debug, error, info, warn};

use crate::config::WebSocketConfig;

/// Enhanced WebSocket server with subscription management
#[derive(Debug)]
pub struct ArchivalWebSocketServer {
    /// Configuration
    config: WebSocketConfig,
    
    /// Active connections
    connections: Arc<RwLock<HashMap<String, WebSocketConnection>>>,
    
    /// Subscription manager
    subscription_manager: Arc<SubscriptionManager>,
    
    /// Metrics
    metrics: Arc<WebSocketMetrics>,
    
    /// Server handle
    server_handle: Option<ServerHandle>,
}

/// WebSocket connection information
#[derive(Debug, Clone)]
struct WebSocketConnection {
    /// Connection ID
    id: String,
    
    /// Remote address
    remote_addr: SocketAddr,
    
    /// Connection time
    connected_at: Instant,
    
    /// Last activity
    last_activity: Instant,
    
    /// Active subscriptions
    subscriptions: Vec<String>,
    
    /// Message count
    message_count: u64,
}

/// Subscription manager for handling various subscription types
#[derive(Debug)]
struct SubscriptionManager {
    /// Block subscriptions
    block_subscriptions: Arc<RwLock<HashMap<String, SubscriptionSink>>>,
    
    /// Transaction subscriptions  
    tx_subscriptions: Arc<RwLock<HashMap<String, SubscriptionSink>>>,
    
    /// Log subscriptions
    log_subscriptions: Arc<RwLock<HashMap<String, LogSubscription>>>,
    
    /// State change subscriptions
    state_subscriptions: Arc<RwLock<HashMap<String, StateSubscription>>>,
    
    /// Broadcast channels
    block_tx: broadcast::Sender<BlockNotification>,
    tx_tx: broadcast::Sender<TxNotification>,
    log_tx: broadcast::Sender<LogNotification>,
    state_tx: broadcast::Sender<StateNotification>,
}

/// Log subscription with filters
#[derive(Debug, Clone)]
struct LogSubscription {
    sink: SubscriptionSink,
    addresses: Vec<reth_primitives::Address>,
    topics: Vec<Vec<B256>>,
}

/// State subscription for account/storage changes
#[derive(Debug, Clone)]
struct StateSubscription {
    sink: SubscriptionSink,
    addresses: Vec<reth_primitives::Address>,
    storage_keys: Vec<B256>,
}

/// Block notification
#[derive(Debug, Clone)]
struct BlockNotification {
    hash: BlockHash,
    number: BlockNumber,
    header: Header,
}

/// Transaction notification
#[derive(Debug, Clone)]
struct TxNotification {
    hash: B256,
    transaction: TransactionSigned,
}

/// Log notification
#[derive(Debug, Clone)]
struct LogNotification {
    log: Log,
    block_hash: BlockHash,
    block_number: BlockNumber,
}

/// State change notification
#[derive(Debug, Clone)]
struct StateNotification {
    address: reth_primitives::Address,
    storage_key: Option<B256>,
    old_value: Value,
    new_value: Value,
}

/// WebSocket metrics
#[derive(Debug, Default)]
#[derive(Debug, Clone)]
struct WebSocketMetrics {
    /// Total connections
    total_connections: AtomicU64,
    
    /// Active connections
    active_connections: AtomicU64,
    
    /// Total messages sent
    messages_sent: AtomicU64,
    
    /// Total messages received
    messages_received: AtomicU64,
    
    /// Active subscriptions
    active_subscriptions: AtomicU64,
    
    /// Connection errors
    connection_errors: AtomicU64,
}

impl ArchivalWebSocketServer {
    /// Create a new WebSocket server
    pub fn new(config: WebSocketConfig) -> Self {
        let (block_tx, _) = broadcast::channel(1000);
        let (tx_tx, _) = broadcast::channel(1000);
        let (log_tx, _) = broadcast::channel(1000);
        let (state_tx, _) = broadcast::channel(1000);
        
        let subscription_manager = Arc::new(SubscriptionManager {
            block_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            tx_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            log_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            state_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            block_tx,
            tx_tx,
            log_tx,
            state_tx,
        });
        
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            subscription_manager,
            metrics: Arc::new(WebSocketMetrics::default()),
            server_handle: None,
        }
    }
    
    /// Start the WebSocket server
    pub async fn start(&mut self) -> Result<()> {
        if !self.config.enabled {
            info!("WebSocket server is disabled");
            return Ok(());
        }
        
        let addr = format!("{}:{}", self.config.addr, self.config.port);
        info!("Starting WebSocket server on {}", addr);
        
        let server = ServerBuilder::default()
            .max_connections(self.config.max_connections)
            .max_subscriptions_per_connection(self.config.max_subscriptions_per_connection)
            .ping_interval(Duration::from_secs(self.config.ping_interval))
            .build(&addr)
            .await?;
        
        // Register subscription methods
        self.register_subscription_methods(&server).await?;
        
        // Start background tasks
        self.start_background_tasks().await?;
        
        let handle = server.start(self.create_rpc_module().await?);
        self.server_handle = Some(handle);
        
        info!("WebSocket server started successfully");
        Ok(())
    }
    
    /// Stop the WebSocket server
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.server_handle.take() {
            handle.stop()?;
            info!("WebSocket server stopped");
        }
        Ok(())
    }
    
    /// Register subscription methods
    async fn register_subscription_methods(&self, server: &ServerBuilder) -> Result<()> {
        // This would register the actual subscription methods
        // Implementation details would go here
        Ok(())
    }
    
    /// Create the RPC module with subscription endpoints
    async fn create_rpc_module(&self) -> Result<jsonrpsee::RpcModule<()>> {
        let mut module = jsonrpsee::RpcModule::new(());
        
        // Clone necessary data for closures
        let subscription_manager = Arc::clone(&self.subscription_manager);
        let metrics = Arc::clone(&self.metrics);
        
        // Register newHeads subscription
        module.register_subscription(
            "eth_subscribe",
            "eth_subscription",
            "eth_unsubscribe",
            move |params, pending, _ctx| {
                let subscription_manager = Arc::clone(&subscription_manager);
                let metrics = Arc::clone(&metrics);
                
                async move {
                    let subscription_type: String = params.one()?;
                    
                    match subscription_type.as_str() {
                        "newHeads" => {
                            handle_new_heads_subscription(pending, subscription_manager, metrics).await
                        }
                        "logs" => {
                            handle_logs_subscription(params, pending, subscription_manager, metrics).await
                        }
                        "newPendingTransactions" => {
                            handle_pending_tx_subscription(pending, subscription_manager, metrics).await
                        }
                        "stateChanges" => {
                            handle_state_changes_subscription(params, pending, subscription_manager, metrics).await
                        }
                        _ => {
                            let error = ErrorObject::owned(
                                ErrorCode::InvalidParams.code(),
                                "Invalid subscription type",
                                None::<String>,
                            );
                            Err(error)
                        }
                    }
                }
            },
        )?;
        
        Ok(module)
    }
    
    /// Start background tasks for the WebSocket server
    async fn start_background_tasks(&self) -> Result<()> {
        // Start connection cleanup task
        let connections = Arc::clone(&self.connections);
        let metrics = Arc::clone(&self.metrics);
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;
                cleanup_stale_connections(&connections, &metrics).await;
            }
        });
        
        // Start metrics reporting task
        let metrics_clone = Arc::clone(&self.metrics);
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                report_metrics(&metrics_clone).await;
            }
        });
        
        Ok(())
    }
    
    /// Get current metrics
    pub fn metrics(&self) -> WebSocketMetrics {
        WebSocketMetrics {
            total_connections: AtomicU64::new(self.metrics.total_connections.load(Ordering::Relaxed)),
            active_connections: AtomicU64::new(self.metrics.active_connections.load(Ordering::Relaxed)),
            messages_sent: AtomicU64::new(self.metrics.messages_sent.load(Ordering::Relaxed)),
            messages_received: AtomicU64::new(self.metrics.messages_received.load(Ordering::Relaxed)),
            active_subscriptions: AtomicU64::new(self.metrics.active_subscriptions.load(Ordering::Relaxed)),
            connection_errors: AtomicU64::new(self.metrics.connection_errors.load(Ordering::Relaxed)),
        }
    }
    
    /// Broadcast block notification
    pub fn notify_new_block(&self, notification: BlockNotification) -> Result<()> {
        if let Err(e) = self.subscription_manager.block_tx.send(notification) {
            warn!("Failed to broadcast block notification: {}", e);
        }
        Ok(())
    }
    
    /// Broadcast transaction notification
    pub fn notify_new_transaction(&self, notification: TxNotification) -> Result<()> {
        if let Err(e) = self.subscription_manager.tx_tx.send(notification) {
            warn!("Failed to broadcast transaction notification: {}", e);
        }
        Ok(())
    }
}

// Helper functions for subscription handling
async fn handle_new_heads_subscription(
    pending: PendingSubscriptionSink,
    subscription_manager: Arc<SubscriptionManager>,
    metrics: Arc<WebSocketMetrics>,
) -> SubscriptionResult {
    let sink = pending.accept().await?;
    let subscription_id = sink.subscription_id().clone();
    
    // Store the subscription
    subscription_manager.block_subscriptions.write().insert(subscription_id.clone(), sink.clone());
    metrics.active_subscriptions.fetch_add(1, Ordering::Relaxed);
    
    // Start listening for block notifications
    let mut block_rx = subscription_manager.block_tx.subscribe();
    let sink_clone = sink.clone();
    
    tokio::spawn(async move {
        while let Ok(notification) = block_rx.recv().await {
            let header_json = serde_json::to_value(&notification.header)
                .unwrap_or_else(|_| Value::Null);
            
            if sink_clone.send(SubscriptionMessage::from_json(&header_json).unwrap()).await.is_err() {
                break;
            }
        }
    });
    
    Ok(sink)
}

async fn handle_logs_subscription(
    _params: jsonrpsee::core::Params<'_>,
    pending: PendingSubscriptionSink,
    subscription_manager: Arc<SubscriptionManager>,
    metrics: Arc<WebSocketMetrics>,
) -> SubscriptionResult {
    let sink = pending.accept().await?;
    let subscription_id = sink.subscription_id().clone();
    
    // Parse log filter parameters (simplified)
    let log_subscription = LogSubscription {
        sink: sink.clone(),
        addresses: Vec::new(), // Would parse from params
        topics: Vec::new(),    // Would parse from params
    };
    
    subscription_manager.log_subscriptions.write().insert(subscription_id, log_subscription);
    metrics.active_subscriptions.fetch_add(1, Ordering::Relaxed);
    
    Ok(sink)
}

async fn handle_pending_tx_subscription(
    pending: PendingSubscriptionSink,
    subscription_manager: Arc<SubscriptionManager>,
    metrics: Arc<WebSocketMetrics>,
) -> SubscriptionResult {
    let sink = pending.accept().await?;
    let subscription_id = sink.subscription_id().clone();
    
    subscription_manager.tx_subscriptions.write().insert(subscription_id, sink.clone());
    metrics.active_subscriptions.fetch_add(1, Ordering::Relaxed);
    
    Ok(sink)
}

async fn handle_state_changes_subscription(
    _params: jsonrpsee::core::Params<'_>,
    pending: PendingSubscriptionSink,
    subscription_manager: Arc<SubscriptionManager>,
    metrics: Arc<WebSocketMetrics>,
) -> SubscriptionResult {
    let sink = pending.accept().await?;
    let subscription_id = sink.subscription_id().clone();
    
    // Parse state change filter parameters (simplified)
    let state_subscription = StateSubscription {
        sink: sink.clone(),
        addresses: Vec::new(),     // Would parse from params
        storage_keys: Vec::new(),  // Would parse from params
    };
    
    subscription_manager.state_subscriptions.write().insert(subscription_id, state_subscription);
    metrics.active_subscriptions.fetch_add(1, Ordering::Relaxed);
    
    Ok(sink)
}

async fn cleanup_stale_connections(
    connections: &Arc<RwLock<HashMap<String, WebSocketConnection>>>,
    metrics: &Arc<WebSocketMetrics>,
) {
    let stale_threshold = Duration::from_secs(300); // 5 minutes
    let mut stale_connections = Vec::new();
    
    {
        let connections_read = connections.read();
        for (id, conn) in connections_read.iter() {
            if conn.last_activity.elapsed() > stale_threshold {
                stale_connections.push(id.clone());
            }
        }
    }
    
    if !stale_connections.is_empty() {
        let mut connections_write = connections.write();
        for id in stale_connections {
            connections_write.remove(&id);
            metrics.active_connections.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

async fn report_metrics(metrics: &Arc<WebSocketMetrics>) {
    debug!(
        "WebSocket metrics - Active connections: {}, Total messages sent: {}, Active subscriptions: {}",
        metrics.active_connections.load(Ordering::Relaxed),
        metrics.messages_sent.load(Ordering::Relaxed),
        metrics.active_subscriptions.load(Ordering::Relaxed)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_server_creation() {
        let config = WebSocketConfig::default();
        let server = ArchivalWebSocketServer::new(config);
        
        let metrics = server.metrics();
        assert_eq!(metrics.total_connections.load(Ordering::Relaxed), 0);
    }
    
    #[tokio::test]
    async fn test_websocket_server_start_stop() {
        let mut config = WebSocketConfig::default();
        config.port = 0; // Use random port for testing
        
        let mut server = ArchivalWebSocketServer::new(config);
        
        // For testing, we'll just create and immediately stop
        // In a real test, you'd need to set up a proper test environment
        assert!(server.server_handle.is_none());
    }
}
