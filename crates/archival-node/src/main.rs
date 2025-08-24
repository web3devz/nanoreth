//! Main binary for the archival node

use clap::Parser;
use eyre::Result;
use reth_archival_node::{cli::ArchivalNodeArgs, node::ArchivalNode};
use reth_tracing::{FileWorkerGuard, RethTracer, Tracer};
use std::io;
use tokio::signal;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = ArchivalNodeArgs::parse();
    
    // Initialize tracing
    init_tracing(&args)?;
    
    // Print banner
    print_banner();
    
    // Validate arguments
    if let Err(e) = args.validate() {
        error!("Invalid configuration: {}", e);
        std::process::exit(1);
    }
    
    // Create and launch the archival node
    let node = match ArchivalNode::from_args(args).await {
        Ok(node) => node,
        Err(e) => {
            error!("Failed to create archival node: {}", e);
            std::process::exit(1);
        }
    };
    
    let node = match node.launch().await {
        Ok(node) => node,
        Err(e) => {
            error!("Failed to launch archival node: {}", e);
            std::process::exit(1);
        }
    };
    
    info!("Archival node is now running");
    
    // Set up signal handling
    let shutdown_signal = setup_signal_handlers();
    
    // Wait for shutdown signal
    tokio::select! {
        _ = shutdown_signal => {
            info!("Shutdown signal received");
        }
        result = node.wait_for_shutdown() => {
            if let Err(e) = result {
                error!("Node shutdown with error: {}", e);
            }
        }
    }
    
    // Graceful shutdown
    info!("Initiating graceful shutdown...");
    if let Err(e) = node.stop().await {
        error!("Error during shutdown: {}", e);
        std::process::exit(1);
    }
    
    info!("Archival node shut down successfully");
    Ok(())
}

/// Initialize tracing/logging
fn init_tracing(args: &ArchivalNodeArgs) -> Result<()> {
    let mut tracer = RethTracer::new();
    
    // Set log level based on args
    if let Some(ref filter) = args.logs.log_filter {
        tracer = tracer.with_filter(filter);
    }
    
    // Enable JSON formatting if requested
    if args.logs.log_json {
        tracer = tracer.with_json();
    }
    
    // Initialize the tracer
    let _guard = tracer.init()?;
    
    Ok(())
}

/// Print the application banner
fn print_banner() {
    println!(
        r#"
    ╔═══════════════════════════════════════════════════════════╗
    ║                                                           ║
    ║                   Archival Ethereum Node                  ║
    ║                                                           ║
    ║          Enhanced with modular architecture and          ║
    ║              comprehensive archival support              ║
    ║                                                           ║
    ║  Features:                                                ║
    ║  • Full historical state access                          ║
    ║  • Enhanced eth_getProof support                         ║
    ║  • WebSocket subscriptions                               ║
    ║  • Parallel proof generation                             ║
    ║  • Intelligent caching                                   ║
    ║                                                           ║
    ║  Version: {}                                         ║
    ╚═══════════════════════════════════════════════════════════╝
        "#,
        env!("CARGO_PKG_VERSION")
    );
}

/// Set up signal handlers for graceful shutdown
async fn setup_signal_handlers() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}

/// Handle application errors
fn handle_error(error: eyre::Error) -> ! {
    error!("Fatal error: {}", error);
    
    // Print the error chain
    let mut source = error.source();
    while let Some(err) = source {
        error!("  Caused by: {}", err);
        source = err.source();
    }
    
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_banner_print() {
        // This just tests that the banner function doesn't panic
        print_banner();
    }
    
    #[tokio::test]
    async fn test_signal_setup() {
        // Test that signal setup doesn't panic
        tokio::select! {
            _ = setup_signal_handlers() => {},
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
        }
    }
}
