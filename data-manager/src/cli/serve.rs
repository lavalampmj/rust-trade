//! Serve command - start the data manager service

use anyhow::Result;
use clap::Args;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::config::Settings;
use crate::provider::binance::{BinanceProvider, BinanceSettings as BinanceProviderSettings};
use crate::provider::{DataProvider, LiveStreamProvider, LiveSubscription, StreamEvent};
use crate::schema::NormalizedTick;
use crate::storage::{MarketDataRepository, TimescaleOperations};
use crate::symbol::SymbolSpec;
use crate::transport::ipc::{SharedMemoryConfig, SharedMemoryTransport};
use crate::transport::Transport;

/// Arguments for the serve command
#[derive(Args)]
pub struct ServeArgs {
    /// Enable live data streaming
    #[arg(long)]
    pub live: bool,

    /// Provider to use for live data (binance, databento)
    #[arg(long, default_value = "binance")]
    pub provider: String,

    /// Symbols to subscribe to (comma-separated)
    #[arg(long, short)]
    pub symbols: Option<String>,

    /// Enable IPC transport
    #[arg(long)]
    pub ipc: bool,

    /// Persist data to database
    #[arg(long)]
    pub persist: bool,

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
    info!("  Provider: {}", args.provider);
    info!("  IPC transport: {}", args.ipc);
    info!("  Persist to DB: {}", args.persist);
    info!("  Bind address: {}", args.bind);

    // Load configuration
    let settings = Settings::default_settings();

    // Initialize database if persistence is enabled
    let repository = if args.persist {
        info!("Connecting to database...");
        let repo = MarketDataRepository::from_settings(&settings.database).await?;
        let timescale = TimescaleOperations::new(repo.pool().clone());
        timescale.run_migrations().await?;
        info!("Database connected and migrations applied");
        Some(Arc::new(repo))
    } else {
        None
    };

    // Initialize IPC transport if enabled
    let transport = if args.ipc {
        info!("Initializing IPC transport...");
        let config = SharedMemoryConfig {
            path_prefix: settings.transport.ipc.shm_path_prefix.clone(),
            buffer_entries: settings.transport.ipc.ring_buffer_entries,
            entry_size: settings.transport.ipc.entry_size,
        };
        Some(Arc::new(SharedMemoryTransport::new(config)))
    } else {
        None
    };

    // Set up shutdown handling
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

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
        // Determine symbols to subscribe to
        let symbols = parse_symbols(&args, &settings)?;

        if symbols.is_empty() {
            error!("No symbols specified for live streaming");
            return Err(anyhow::anyhow!("No symbols specified"));
        }

        info!("Subscribing to {} symbols: {:?}", symbols.len(), symbols);

        // Start the appropriate provider
        match args.provider.as_str() {
            "binance" => {
                run_binance_provider(symbols, repository, transport, shutdown_tx.subscribe()).await?;
            }
            "databento" => {
                warn!("Databento live streaming not yet implemented");
                // Wait for shutdown
                let mut shutdown_rx = shutdown_tx.subscribe();
                shutdown_rx.recv().await?;
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown provider: {}", args.provider));
            }
        }
    } else {
        info!("Data manager service started (no live streaming)");
        info!("Press Ctrl+C to stop");

        // Wait for shutdown
        let mut shutdown_rx = shutdown_tx.subscribe();
        shutdown_rx.recv().await?;
    }

    info!("Shutting down...");
    info!("Data manager service stopped");
    Ok(())
}

/// Parse symbols from command line or config
fn parse_symbols(args: &ServeArgs, settings: &Settings) -> Result<Vec<SymbolSpec>> {
    let symbols: Vec<SymbolSpec> = if let Some(ref symbol_str) = args.symbols {
        // Parse from command line
        symbol_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| SymbolSpec::new(s, "BINANCE"))
            .collect()
    } else if let Some(ref binance) = settings.provider.binance {
        // Use default symbols from config
        binance
            .default_symbols
            .iter()
            .map(|s| SymbolSpec::new(s, "BINANCE"))
            .collect()
    } else {
        vec![]
    };

    Ok(symbols)
}

/// Run the Binance provider
async fn run_binance_provider(
    symbols: Vec<SymbolSpec>,
    repository: Option<Arc<MarketDataRepository>>,
    transport: Option<Arc<SharedMemoryTransport>>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    // Create provider settings
    let provider_settings = BinanceProviderSettings::default();
    let mut provider = BinanceProvider::with_settings(provider_settings);

    // Connect to provider
    provider.connect().await?;
    info!("Binance provider connected");

    // Statistics
    let ticks_received = Arc::new(AtomicU64::new(0));
    let ticks_persisted = Arc::new(AtomicU64::new(0));
    let ticks_ipc = Arc::new(AtomicU64::new(0));

    // Clone for callback
    let ticks_received_clone = Arc::clone(&ticks_received);
    let ticks_persisted_clone = Arc::clone(&ticks_persisted);
    let ticks_ipc_clone = Arc::clone(&ticks_ipc);
    let repository_clone = repository.clone();
    let transport_clone = transport.clone();

    // Create callback for handling stream events
    let callback = Arc::new(move |event: StreamEvent| {
        match event {
            StreamEvent::Tick(tick) => {
                ticks_received_clone.fetch_add(1, Ordering::Relaxed);

                // Log periodically
                let count = ticks_received_clone.load(Ordering::Relaxed);
                if count % 100 == 0 {
                    debug!(
                        "Received {} ticks | Persisted: {} | IPC: {}",
                        count,
                        ticks_persisted_clone.load(Ordering::Relaxed),
                        ticks_ipc_clone.load(Ordering::Relaxed)
                    );
                }

                // Publish to IPC
                if let Some(ref transport) = transport_clone {
                    if let Err(e) = publish_to_ipc(transport, &tick) {
                        warn!("Failed to publish to IPC: {}", e);
                    } else {
                        ticks_ipc_clone.fetch_add(1, Ordering::Relaxed);
                    }
                }

                // Persist to database (async - fire and forget for now)
                if let Some(ref repo) = repository_clone {
                    let repo_clone = Arc::clone(repo);
                    let tick_clone = tick.clone();
                    let persisted_clone = Arc::clone(&ticks_persisted_clone);
                    tokio::spawn(async move {
                        if let Err(e) = persist_tick(&repo_clone, &tick_clone).await {
                            warn!("Failed to persist tick: {}", e);
                        } else {
                            persisted_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            }
            StreamEvent::TickBatch(ticks) => {
                let batch_size = ticks.len() as u64;
                ticks_received_clone.fetch_add(batch_size, Ordering::Relaxed);
                debug!("Received batch of {} ticks", batch_size);
            }
            StreamEvent::Status(status) => {
                info!("Connection status: {:?}", status);
            }
            StreamEvent::Error(err) => {
                error!("Stream error: {}", err);
            }
        }
    });

    // Create subscription
    let subscription = LiveSubscription::trades(symbols);

    // Start streaming (this will block until shutdown)
    info!("Starting live data stream...");
    provider.subscribe(subscription, callback, shutdown_rx).await?;

    // Final statistics
    info!(
        "Final stats: Received: {} | Persisted: {} | IPC: {}",
        ticks_received.load(Ordering::Relaxed),
        ticks_persisted.load(Ordering::Relaxed),
        ticks_ipc.load(Ordering::Relaxed)
    );

    Ok(())
}

/// Publish tick to IPC transport
fn publish_to_ipc(transport: &SharedMemoryTransport, tick: &NormalizedTick) -> Result<()> {
    let msg = tick.to_trade_msg();
    transport.send_msg(&msg, &tick.symbol, &tick.exchange)?;
    Ok(())
}

/// Persist tick to database
async fn persist_tick(repo: &MarketDataRepository, tick: &NormalizedTick) -> Result<()> {
    repo.insert_tick(tick).await?;
    Ok(())
}
