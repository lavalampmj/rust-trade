//! Serve command - start the data manager service

use anyhow::Result;
use clap::Args;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::cli::control_handler::{ControlHandler, ControlHandlerState};
use crate::config::Settings;
use crate::provider::binance::{BinanceProvider, BinanceSettings as BinanceProviderSettings};
use crate::provider::kraken::{KrakenMarketType, KrakenProvider, KrakenSettings as KrakenProviderSettings};
use crate::provider::{DataProvider, LiveStreamProvider, LiveSubscription, StreamEvent};
use crate::storage::{MarketDataRepository, TimescaleOperations};
use crate::symbol::SymbolSpec;
use crate::transport::ipc::{
    generate_instance_id, Registry, RegistryEntry, SharedMemoryConfig, SharedMemoryTransport,
    HEARTBEAT_INTERVAL,
};
use crate::transport::Transport;
use trading_common::data::orderbook::{OrderBook, OrderBookDelta};
use trading_common::data::quotes::QuoteTick;
use trading_common::data::types::TickData;

/// Arguments for the serve command
#[derive(Args)]
pub struct ServeArgs {
    /// Enable live data streaming
    #[arg(long)]
    pub live: bool,

    /// Provider to use for live data (binance, kraken, kraken_futures, databento)
    #[arg(long, default_value = "binance")]
    pub provider: String,

    /// Symbols to subscribe to (comma-separated)
    #[arg(long, short)]
    pub symbols: Option<String>,

    /// Enable IPC transport
    #[arg(long)]
    pub ipc: bool,

    /// Symbols to publish via IPC (subset of --symbols)
    /// If not specified, defaults to all --symbols when IPC is enabled
    #[arg(long, value_delimiter = ',')]
    pub ipc_symbols: Option<Vec<String>>,

    /// Persist data to database (enabled by default, use --persist=false to disable)
    #[arg(long, default_value_t = true)]
    pub persist: bool,

    /// Use demo/testnet endpoints (Kraken Futures only)
    #[arg(long)]
    pub demo: bool,

    /// Bind address for API
    #[arg(long, default_value = "0.0.0.0:8081")]
    pub bind: String,

    /// Configuration file path
    #[arg(long, short)]
    pub config: Option<String>,

    /// Unique instance identifier for service registry
    /// If not specified, auto-generates based on provider and PID
    #[arg(long)]
    pub instance_id: Option<String>,
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

    // Generate or use instance ID
    let instance_id = args
        .instance_id
        .clone()
        .unwrap_or_else(|| generate_instance_id(&args.provider));
    info!("  Instance ID: {}", instance_id);

    // Initialize IPC transport if enabled
    let (transport, ipc_path_prefix) = if args.ipc {
        info!("Initializing IPC transport...");
        // Use instance-specific prefix to avoid conflicts between multiple data-managers
        let base_prefix = settings.transport.ipc.shm_path_prefix.clone();
        let instance_prefix = format!("{}{}__", base_prefix, instance_id);

        let config = SharedMemoryConfig {
            path_prefix: instance_prefix.clone(),
            buffer_entries: settings.transport.ipc.ring_buffer_entries,
            entry_size: settings.transport.ipc.entry_size,
        };
        let memory_per_channel = config.buffer_entries * config.entry_size;
        info!(
            "IPC config: path_prefix={}, buffer_entries={}, entry_size={} bytes, memory_per_channel={} KB",
            config.path_prefix,
            config.buffer_entries,
            config.entry_size,
            memory_per_channel / 1024
        );
        let transport = Arc::new(SharedMemoryTransport::new(config));
        info!("IPC transport ready - waiting for symbols to create channels");
        (Some(transport), instance_prefix)
    } else {
        (None, String::new())
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

        // Build IPC symbol set and control handler state
        let exchange = match args.provider.as_str() {
            "kraken" | "kraken_spot" => "KRAKEN",
            "kraken_futures" => "KRAKEN_FUTURES",
            _ => "BINANCE",
        };

        let control_state: Option<Arc<ControlHandlerState>> = if args.ipc {
            let ipc_symbols: HashSet<String> = if let Some(ref ipc_syms) = args.ipc_symbols {
                ipc_syms.iter().cloned().collect()
            } else {
                // Default to all subscribed symbols
                symbols.iter().map(|s| s.symbol.clone()).collect()
            };

            // Validate that IPC symbols are a subset of subscribed symbols
            let subscribed: HashSet<String> = symbols.iter().map(|s| s.symbol.clone()).collect();
            for ipc_sym in &ipc_symbols {
                if !subscribed.contains(ipc_sym) {
                    warn!(
                        "IPC symbol '{}' is not in subscribed symbols, will be ignored",
                        ipc_sym
                    );
                }
            }

            // Filter to only include symbols that are actually subscribed
            let valid_ipc_symbols: HashSet<String> = ipc_symbols
                .intersection(&subscribed)
                .cloned()
                .collect();

            info!(
                "IPC streaming {} of {} symbols: {:?}",
                valid_ipc_symbols.len(),
                symbols.len(),
                valid_ipc_symbols
            );

            // Pre-create IPC channels for the IPC symbols
            if let Some(ref transport) = transport {
                for symbol_spec in symbols.iter().filter(|s| valid_ipc_symbols.contains(&s.symbol)) {
                    if let Err(e) = transport.ensure_channel(&symbol_spec.symbol, &symbol_spec.exchange) {
                        error!("Failed to pre-create IPC channel for {}: {}", symbol_spec.symbol, e);
                    }
                }
                info!(
                    "Pre-created {} IPC channels",
                    transport.channel_count()
                );
            }

            // Create control handler state
            let state = Arc::new(ControlHandlerState::new(
                subscribed,
                valid_ipc_symbols,
                exchange,
            ));

            // Start control channel handler
            if let Some(ref transport) = transport {
                let _control_handle = ControlHandler::spawn(
                    Arc::clone(&state),
                    Arc::clone(transport),
                    shutdown_tx.subscribe(),
                );
                info!("Control channel handler started - dynamic subscriptions enabled");
            }

            Some(state)
        } else {
            None
        };

        // Set up service registry for multi-instance discovery
        let registry_slot: Option<usize> = if args.ipc {
            match Registry::open_or_create() {
                Ok(registry) => {
                    let ipc_symbol_list: Vec<String> = control_state
                        .as_ref()
                        .map(|s| s.ipc_symbols())
                        .unwrap_or_default();

                    let entry = RegistryEntry::new(
                        &instance_id,
                        &args.provider,
                        exchange,
                        &ipc_path_prefix,
                        &ipc_symbol_list,
                    );

                    match registry.register(entry) {
                        Ok(slot) => {
                            info!(
                                "Registered in service registry (slot {}) with {} symbols",
                                slot,
                                ipc_symbol_list.len()
                            );

                            // Start heartbeat background task
                            let registry = Arc::new(registry);
                            let registry_clone = Arc::clone(&registry);
                            let mut heartbeat_shutdown = shutdown_tx.subscribe();
                            tokio::spawn(async move {
                                let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
                                loop {
                                    tokio::select! {
                                        _ = interval.tick() => {
                                            if let Err(e) = registry_clone.heartbeat(slot) {
                                                warn!("Failed to update registry heartbeat: {}", e);
                                            } else {
                                                debug!("Registry heartbeat updated for slot {}", slot);
                                            }
                                        }
                                        _ = heartbeat_shutdown.recv() => {
                                            info!("Stopping registry heartbeat task");
                                            // Deregister on shutdown
                                            if let Err(e) = registry_clone.deregister(slot) {
                                                warn!("Failed to deregister from registry: {}", e);
                                            } else {
                                                info!("Deregistered from service registry (slot {})", slot);
                                            }
                                            break;
                                        }
                                    }
                                }
                            });

                            Some(slot)
                        }
                        Err(e) => {
                            warn!("Failed to register in service registry: {} - continuing without registry", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to open service registry: {} - continuing without registry", e);
                    None
                }
            }
        } else {
            None
        };

        if registry_slot.is_some() {
            info!("Service registry integration enabled");
        }

        // Start the appropriate provider
        match args.provider.as_str() {
            "binance" => {
                run_binance_provider(
                    symbols,
                    repository,
                    transport,
                    control_state,
                    shutdown_tx.subscribe(),
                )
                .await?;
            }
            "kraken" | "kraken_spot" => {
                run_kraken_provider(
                    KrakenMarketType::Spot,
                    false,
                    symbols,
                    repository,
                    transport,
                    control_state,
                    shutdown_tx.subscribe(),
                )
                .await?;
            }
            "kraken_futures" => {
                run_kraken_provider(
                    KrakenMarketType::Futures,
                    args.demo,
                    symbols,
                    repository,
                    transport,
                    control_state,
                    shutdown_tx.subscribe(),
                )
                .await?;
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
    // Determine exchange based on provider
    let exchange = match args.provider.as_str() {
        "kraken" | "kraken_spot" => "KRAKEN",
        "kraken_futures" => "KRAKEN_FUTURES",
        _ => "BINANCE",
    };

    let symbols: Vec<SymbolSpec> = if let Some(ref symbol_str) = args.symbols {
        // Parse from command line
        symbol_str
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| SymbolSpec::new(s, exchange))
            .collect()
    } else {
        // Use default symbols from config based on provider
        match args.provider.as_str() {
            "kraken" | "kraken_spot" | "kraken_futures" => {
                if let Some(ref kraken) = settings.provider.kraken {
                    kraken
                        .default_symbols
                        .iter()
                        .map(|s| SymbolSpec::new(s, exchange))
                        .collect()
                } else {
                    vec![]
                }
            }
            _ => {
                if let Some(ref binance) = settings.provider.binance {
                    binance
                        .default_symbols
                        .iter()
                        .map(|s| SymbolSpec::new(s, exchange))
                        .collect()
                } else {
                    vec![]
                }
            }
        }
    };

    Ok(symbols)
}

/// Run the Binance provider
async fn run_binance_provider(
    symbols: Vec<SymbolSpec>,
    repository: Option<Arc<MarketDataRepository>>,
    transport: Option<Arc<SharedMemoryTransport>>,
    control_state: Option<Arc<ControlHandlerState>>,
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
    let control_state_clone = control_state.clone();

    // Create callback for handling stream events
    let callback = Arc::new(move |event: StreamEvent| {
        match event {
            StreamEvent::Tick(tick) => {
                ticks_received_clone.fetch_add(1, Ordering::Relaxed);

                // Log periodically
                let count = ticks_received_clone.load(Ordering::Relaxed);
                if count % 100 == 0 {
                    info!(
                        "Live stats: {} ticks received | {} persisted | {} IPC",
                        count,
                        ticks_persisted_clone.load(Ordering::Relaxed),
                        ticks_ipc_clone.load(Ordering::Relaxed)
                    );
                }

                // Log IPC status periodically (every 1000 ticks)
                if count % 1000 == 0 {
                    if let Some(ref transport) = transport_clone {
                        let active = transport.active_symbols();
                        let stats = transport.stats();
                        info!(
                            "IPC status: {} active channels, {} msgs sent, {:.1}% avg buffer utilization",
                            active.len(),
                            stats.messages_sent,
                            stats.buffer_utilization * 100.0
                        );
                    }
                }

                // Publish to IPC only if symbol is in the IPC set (check via control state)
                if let Some(ref transport) = transport_clone {
                    let should_publish = control_state_clone
                        .as_ref()
                        .map(|s| s.has_ipc_channel(&tick.symbol))
                        .unwrap_or(false);

                    if should_publish {
                        if let Err(e) = publish_to_ipc(transport, &tick) {
                            warn!("Failed to publish to IPC: {}", e);
                        } else {
                            ticks_ipc_clone.fetch_add(1, Ordering::Relaxed);
                        }
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
            StreamEvent::Quote(_) | StreamEvent::QuoteBatch(_) => {
                debug!("Received quote event (not handled for Binance)");
            }
            StreamEvent::OrderBookSnapshot(_) | StreamEvent::OrderBookUpdate(_) => {
                debug!("Received orderbook event (not handled for Binance)");
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
    provider
        .subscribe(subscription, callback, shutdown_rx)
        .await?;

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
fn publish_to_ipc(transport: &SharedMemoryTransport, tick: &TickData) -> Result<()> {
    let msg = tick.to_trade_msg_with_exchange(&tick.exchange);
    transport.send_msg(&msg, &tick.symbol, &tick.exchange)?;
    Ok(())
}

/// Persist tick to database
async fn persist_tick(repo: &MarketDataRepository, tick: &TickData) -> Result<()> {
    repo.insert_tick(tick).await?;
    Ok(())
}

/// Persist quote to database
async fn persist_quote(repo: &MarketDataRepository, quote: &QuoteTick) -> Result<()> {
    repo.insert_quote(quote).await?;
    Ok(())
}

/// Persist orderbook snapshot to database
async fn persist_orderbook_snapshot(repo: &MarketDataRepository, book: &OrderBook) -> Result<()> {
    repo.insert_orderbook_snapshot(book).await?;
    Ok(())
}

/// Persist orderbook delta to database
async fn persist_orderbook_delta(repo: &MarketDataRepository, delta: &OrderBookDelta) -> Result<()> {
    repo.insert_orderbook_delta(delta).await?;
    Ok(())
}

/// Run the Kraken provider
async fn run_kraken_provider(
    market_type: KrakenMarketType,
    demo: bool,
    symbols: Vec<SymbolSpec>,
    repository: Option<Arc<MarketDataRepository>>,
    transport: Option<Arc<SharedMemoryTransport>>,
    control_state: Option<Arc<ControlHandlerState>>,
    shutdown_rx: broadcast::Receiver<()>,
) -> Result<()> {
    // Create provider settings
    let provider_settings = KrakenProviderSettings {
        market_type,
        demo,
        ..Default::default()
    };
    let mut provider = KrakenProvider::with_settings(provider_settings);

    // Connect to provider
    provider.connect().await?;
    info!(
        "Kraken {} provider connected (demo: {})",
        market_type.as_str(),
        demo
    );

    // Statistics
    let ticks_received = Arc::new(AtomicU64::new(0));
    let ticks_persisted = Arc::new(AtomicU64::new(0));
    let ticks_ipc = Arc::new(AtomicU64::new(0));
    let quotes_received = Arc::new(AtomicU64::new(0));
    let quotes_persisted = Arc::new(AtomicU64::new(0));
    let orderbook_received = Arc::new(AtomicU64::new(0));
    let orderbook_persisted = Arc::new(AtomicU64::new(0));

    // Clone for callback
    let ticks_received_clone = Arc::clone(&ticks_received);
    let ticks_persisted_clone = Arc::clone(&ticks_persisted);
    let ticks_ipc_clone = Arc::clone(&ticks_ipc);
    let quotes_received_clone = Arc::clone(&quotes_received);
    let quotes_persisted_clone = Arc::clone(&quotes_persisted);
    let orderbook_received_clone = Arc::clone(&orderbook_received);
    let orderbook_persisted_clone = Arc::clone(&orderbook_persisted);
    let repository_clone = repository.clone();
    let transport_clone = transport.clone();
    let control_state_clone = control_state.clone();

    // Create callback for handling stream events
    let callback = Arc::new(move |event: StreamEvent| {
        match event {
            StreamEvent::Tick(tick) => {
                ticks_received_clone.fetch_add(1, Ordering::Relaxed);

                // Log periodically
                let count = ticks_received_clone.load(Ordering::Relaxed);
                if count % 100 == 0 {
                    info!(
                        "Live stats: {} ticks received | {} persisted | {} IPC",
                        count,
                        ticks_persisted_clone.load(Ordering::Relaxed),
                        ticks_ipc_clone.load(Ordering::Relaxed)
                    );
                }

                // Log IPC status periodically (every 1000 ticks)
                if count % 1000 == 0 {
                    if let Some(ref transport) = transport_clone {
                        let active = transport.active_symbols();
                        let stats = transport.stats();
                        info!(
                            "IPC status: {} active channels, {} msgs sent, {:.1}% avg buffer utilization",
                            active.len(),
                            stats.messages_sent,
                            stats.buffer_utilization * 100.0
                        );
                    }
                }

                // Publish to IPC only if symbol is in the IPC set (check via control state)
                if let Some(ref transport) = transport_clone {
                    let should_publish = control_state_clone
                        .as_ref()
                        .map(|s| s.has_ipc_channel(&tick.symbol))
                        .unwrap_or(false);

                    if should_publish {
                        if let Err(e) = publish_to_ipc(transport, &tick) {
                            warn!("Failed to publish to IPC: {}", e);
                        } else {
                            ticks_ipc_clone.fetch_add(1, Ordering::Relaxed);
                        }
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
            StreamEvent::Quote(quote) => {
                quotes_received_clone.fetch_add(1, Ordering::Relaxed);

                // Log periodically
                let count = quotes_received_clone.load(Ordering::Relaxed);
                if count % 100 == 0 {
                    info!(
                        "Quote stats: {} received | {} persisted",
                        count,
                        quotes_persisted_clone.load(Ordering::Relaxed)
                    );
                }

                // Persist to database
                if let Some(ref repo) = repository_clone {
                    let repo_clone = Arc::clone(repo);
                    let quote_clone = quote.clone();
                    let persisted_clone = Arc::clone(&quotes_persisted_clone);
                    tokio::spawn(async move {
                        if let Err(e) = persist_quote(&repo_clone, &quote_clone).await {
                            warn!("Failed to persist quote: {}", e);
                        } else {
                            persisted_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            }
            StreamEvent::QuoteBatch(quotes) => {
                let batch_size = quotes.len() as u64;
                quotes_received_clone.fetch_add(batch_size, Ordering::Relaxed);
                debug!("Received batch of {} quotes", batch_size);
            }
            StreamEvent::OrderBookSnapshot(book) => {
                orderbook_received_clone.fetch_add(1, Ordering::Relaxed);
                debug!(
                    "Received orderbook snapshot for {} with {} bids, {} asks",
                    book.symbol,
                    book.bids(100).len(),
                    book.asks(100).len()
                );

                // Persist to database
                if let Some(ref repo) = repository_clone {
                    let repo_clone = Arc::clone(repo);
                    let book_clone = book.clone();
                    let persisted_clone = Arc::clone(&orderbook_persisted_clone);
                    tokio::spawn(async move {
                        if let Err(e) = persist_orderbook_snapshot(&repo_clone, &book_clone).await {
                            warn!("Failed to persist orderbook snapshot: {}", e);
                        } else {
                            persisted_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
            }
            StreamEvent::OrderBookUpdate(delta) => {
                orderbook_received_clone.fetch_add(1, Ordering::Relaxed);

                // Persist to database
                if let Some(ref repo) = repository_clone {
                    let repo_clone = Arc::clone(repo);
                    let delta_clone = delta.clone();
                    let persisted_clone = Arc::clone(&orderbook_persisted_clone);
                    tokio::spawn(async move {
                        if let Err(e) = persist_orderbook_delta(&repo_clone, &delta_clone).await {
                            warn!("Failed to persist orderbook delta: {}", e);
                        } else {
                            persisted_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }
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
    info!("Starting Kraken live data stream...");
    provider
        .subscribe(subscription, callback, shutdown_rx)
        .await?;

    // Final statistics
    info!(
        "Final stats: Ticks: {} recv / {} persist / {} ipc | Quotes: {} recv / {} persist | OrderBook: {} recv / {} persist",
        ticks_received.load(Ordering::Relaxed),
        ticks_persisted.load(Ordering::Relaxed),
        ticks_ipc.load(Ordering::Relaxed),
        quotes_received.load(Ordering::Relaxed),
        quotes_persisted.load(Ordering::Relaxed),
        orderbook_received.load(Ordering::Relaxed),
        orderbook_persisted.load(Ordering::Relaxed)
    );

    Ok(())
}
