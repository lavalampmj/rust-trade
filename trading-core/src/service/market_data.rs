use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::interval;
use tokio::{select, spawn};
use tracing::{debug, error, info, warn};

use super::{ProcessingStats, ServiceError};
use crate::exchange::Exchange;
use crate::live_trading::PaperTradingProcessor;
use crate::metrics;
use trading_common::data::types::TickData;
use trading_common::data::cache::TickDataCache;

/// Market data service that coordinates between exchange and data processing
///
/// Note: This service does NOT persist tick data to the database.
/// Tick data persistence is handled by data-manager with the --persist flag.
/// This service only:
/// - Updates the cache (for strategy context)
/// - Processes paper trading signals
///
/// Tick validation happens at the data-manager ingestion point, not here.
pub struct MarketDataService {
    /// Exchange implementation (IPC from data-manager)
    exchange: Arc<dyn Exchange>,
    /// Cache for tick data (used by paper trading for context)
    cache: Arc<dyn TickDataCache>,
    /// Symbols to monitor
    symbols: Vec<String>,
    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
    /// Processing statistics
    stats: Arc<Mutex<ProcessingStats>>,
    /// Paper trading processor
    paper_trading: Option<Arc<Mutex<PaperTradingProcessor>>>,
}

impl MarketDataService {
    /// Create a new market data service
    pub fn new(
        exchange: Arc<dyn Exchange>,
        cache: Arc<dyn TickDataCache>,
        symbols: Vec<String>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            exchange,
            cache,
            symbols,
            shutdown_tx,
            stats: Arc::new(Mutex::new(ProcessingStats::default())),
            paper_trading: None,
        }
    }

    pub fn with_paper_trading(mut self, paper_trading: Arc<Mutex<PaperTradingProcessor>>) -> Self {
        self.paper_trading = Some(paper_trading);
        self
    }

    pub fn get_shutdown_tx(&self) -> broadcast::Sender<()> {
        self.shutdown_tx.clone()
    }

    /// Start the market data service
    pub async fn start(&self) -> Result<(), ServiceError> {
        if self.symbols.is_empty() {
            return Err(ServiceError::Config("No symbols configured".to_string()));
        }

        info!(
            "Starting market data service for symbols: {:?}",
            self.symbols
        );

        // Create data processing pipeline with larger buffer for backpressure handling
        const CHANNEL_CAPACITY: usize = 10_000;
        let (tick_tx, tick_rx) = mpsc::channel::<TickData>(CHANNEL_CAPACITY);

        info!(
            "Data pipeline initialized with buffer capacity: {} ticks",
            CHANNEL_CAPACITY
        );

        // Start data collection task
        let collection_task = self.start_data_collection(tick_tx).await?;

        // Start data processing task
        let processing_task = self.start_data_processing(tick_rx).await?;

        // Wait for tasks to complete
        let result = tokio::try_join!(collection_task, processing_task);

        match result {
            Ok(_) => {
                info!("Market data service stopped normally");
                Ok(())
            }
            Err(e) => Err(ServiceError::Task(format!("Task failed: {}", e))),
        }
    }

    /// Start data collection from exchange
    async fn start_data_collection(
        &self,
        tick_tx: mpsc::Sender<TickData>,
    ) -> Result<tokio::task::JoinHandle<()>, ServiceError> {
        let exchange = Arc::clone(&self.exchange);
        let symbols = self.symbols.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        let handle = spawn(async move {
            loop {
                // Check for shutdown signal before attempting connection
                if shutdown_rx.try_recv().is_ok() {
                    info!("Data collection shutdown requested before connection attempt");
                    break;
                }

                // Create callback for tick data with backpressure monitoring
                let tick_tx_clone = tick_tx.clone();
                let callback = Box::new(move |tick: TickData| {
                    let tx = tick_tx_clone.clone();

                    // Increment tick received counter
                    metrics::TICKS_RECEIVED_TOTAL.inc();

                    // Check channel capacity for backpressure monitoring
                    let capacity = tx.capacity();
                    let max_capacity = tx.max_capacity();
                    let utilization_pct = if max_capacity > 0 {
                        ((max_capacity - capacity) as f64 / max_capacity as f64) * 100.0
                    } else {
                        0.0
                    };

                    // Update channel metrics
                    metrics::CHANNEL_UTILIZATION.set(utilization_pct);
                    metrics::CHANNEL_BUFFER_SIZE.set((max_capacity - capacity) as i64);

                    // Warn if channel is getting full (>80% utilized)
                    if utilization_pct > 80.0 {
                        warn!(
                            "Processing backpressure detected: channel {}% full ({}/{})",
                            utilization_pct as u32,
                            max_capacity - capacity,
                            max_capacity
                        );
                    }

                    spawn(async move {
                        match tx.try_send(tick.clone()) {
                            Ok(_) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                warn!(
                                    "Channel full! Blocking until space available. Symbol: {}",
                                    tick.symbol
                                );
                                if let Err(e) = tx.send(tick).await {
                                    if !e.to_string().contains("channel closed") {
                                        error!("Failed to send tick data after blocking: {}", e);
                                    }
                                }
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                debug!("Tick channel closed, skipping tick");
                            }
                        }
                    });
                });

                // Start subscription with shutdown signal
                match exchange
                    .subscribe_trades(&symbols, callback, shutdown_rx.resubscribe())
                    .await
                {
                    Ok(()) => {
                        info!("Exchange subscription completed normally");
                        break;
                    }
                    Err(e) => {
                        error!("Exchange subscription failed: {}", e);

                        if shutdown_rx.try_recv().is_ok() {
                            info!("Data collection shutdown requested, canceling retry");
                            break;
                        }

                        warn!("Retrying exchange connection in 5 seconds...");

                        select! {
                            _ = tokio::time::sleep(Duration::from_secs(5)) => {
                                continue;
                            }
                            _ = shutdown_rx.recv() => {
                                info!("Data collection shutdown requested during retry delay");
                                break;
                            }
                        }
                    }
                }
            }

            info!("Data collection stopped");
        });

        Ok(handle)
    }

    /// Start data processing pipeline
    async fn start_data_processing(
        &self,
        mut tick_rx: mpsc::Receiver<TickData>,
    ) -> Result<tokio::task::JoinHandle<()>, ServiceError> {
        let cache = Arc::clone(&self.cache);
        let stats = Arc::clone(&self.stats);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let paper_trading = self.paper_trading.clone();

        let handle = spawn(async move {
            // Add periodic health monitoring (every 30 seconds)
            let mut health_monitor = interval(Duration::from_secs(30));
            let mut last_tick_count = 0u64;

            // Add bar timer for time-based bar closing (every 1 second)
            let mut bar_timer = interval(Duration::from_secs(1));

            loop {
                select! {
                    // Receive new tick data (already validated at data-manager ingestion)
                    tick_opt = tick_rx.recv() => {
                        match tick_opt {
                            Some(tick) => {
                                // Update cache for strategy context
                                if let Err(e) = cache.push_tick(&tick).await {
                                    warn!("Failed to update cache for tick {}: {}", tick.trade_id, e);
                                    let mut s = stats.lock().await;
                                    s.cache_update_failures += 1;
                                    metrics::CACHE_UPDATE_FAILURES_TOTAL.inc();
                                } else {
                                    debug!("Cache updated for symbol: {}", tick.symbol);
                                }

                                // Paper trading processing
                                if let Some(paper_trading_processor) = &paper_trading {
                                    let mut processor = paper_trading_processor.lock().await;
                                    if let Err(e) = processor.process_tick(&tick).await {
                                        warn!("Paper trading processing failed: {}", e);
                                    }
                                }

                                // Update stats and metrics
                                {
                                    let mut s = stats.lock().await;
                                    s.total_ticks_processed += 1;
                                }
                                metrics::TICKS_PROCESSED_TOTAL.inc();
                            }
                            None => {
                                warn!("Tick data channel closed");
                                break;
                            }
                        }
                    }

                    _ = shutdown_rx.recv() => {
                        info!("Processing shutdown requested");
                        break;
                    }

                    _ = health_monitor.tick() => {
                        let current_stats = stats.lock().await;
                        let current_tick_count = current_stats.total_ticks_processed;
                        let ticks_processed_since_last = current_tick_count - last_tick_count;
                        last_tick_count = current_tick_count;

                        info!(
                            "Pipeline Health: {} ticks/30s | Total: {} | Cache failures: {}",
                            ticks_processed_since_last,
                            current_stats.total_ticks_processed,
                            current_stats.cache_update_failures
                        );
                    }

                    _ = bar_timer.tick() => {
                        // Timer-based bar closing for time-based bars
                        if let Some(paper_trading_processor) = &paper_trading {
                            let mut processor = paper_trading_processor.lock().await;
                            if let Err(e) = processor.check_timer().await {
                                warn!("Bar timer check failed: {}", e);
                            }
                        }
                    }
                }
            }

            info!("Data processing pipeline stopped");
        });

        Ok(handle)
    }
}
