//! Serve command - start the data manager service

use anyhow::Result;
use clap::Args;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;

use crate::config::Settings;
use crate::storage::{MarketDataRepository, TimescaleOperations};
use crate::subscription::SubscriptionManager;
use crate::symbol::SymbolUniverse;
use crate::transport::ipc::SharedMemoryTransport;

/// Arguments for the serve command
#[derive(Args)]
pub struct ServeArgs {
    /// Enable live data streaming
    #[arg(long)]
    pub live: bool,

    /// Enable IPC transport
    #[arg(long)]
    pub ipc: bool,

    /// Bind address for API
    #[arg(long, default_value = "0.0.0.0:8081")]
    pub bind: String,

    /// Configuration file path
    #[arg(long, short)]
    pub config: Option<String>,
}

/// Execute the serve command
pub async fn execute(args: ServeArgs) -> Result<()> {
    info!("Starting data manager service");
    info!("  Live streaming: {}", args.live);
    info!("  IPC transport: {}", args.ipc);
    info!("  Bind address: {}", args.bind);

    // Load configuration
    let settings = Settings::default_settings();

    // Initialize database
    info!("Connecting to database...");
    let repository = MarketDataRepository::from_settings(&settings.database).await?;

    // Run migrations
    let timescale = TimescaleOperations::new(repository.pool().clone());
    timescale.run_migrations().await?;

    // Initialize symbol universe
    let _universe = SymbolUniverse::new();

    // Initialize subscription manager
    let _subscription_manager = Arc::new(SubscriptionManager::new());

    // Initialize transport
    let _transport = if args.ipc {
        info!("Initializing IPC transport...");
        Some(Arc::new(SharedMemoryTransport::new(
            crate::transport::ipc::SharedMemoryConfig::default(),
        )))
    } else {
        None
    };

    // Set up shutdown handling
    let (shutdown_tx, _) = broadcast::channel(1);

    // Handle Ctrl+C
    let shutdown_tx_clone = shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl+c");
        info!("Received shutdown signal");
        let _ = shutdown_tx_clone.send(());
    });

    // Start live streaming if enabled
    if args.live {
        info!("Live streaming enabled but no provider configured");
        // In production, would start the live streaming task here
        // using the configured provider
    }

    info!("Data manager service started");
    info!("Press Ctrl+C to stop");

    // Wait for shutdown
    let mut shutdown_rx = shutdown_tx.subscribe();
    shutdown_rx.recv().await?;

    info!("Shutting down...");

    // Cleanup
    // - Stop live streams
    // - Flush pending data
    // - Close database connections

    info!("Data manager service stopped");
    Ok(())
}
