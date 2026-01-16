use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::time::{interval, sleep};
use tokio::{select, spawn};
use tracing::{debug, error, info, warn};

use super::{BatchConfig, BatchStats, ServiceError};
use crate::exchange::Exchange;
use crate::live_trading::PaperTradingProcessor;
use trading_common::data::types::TickData;
use trading_common::data::{cache::TickDataCache, repository::TickDataRepository};

/// Market data service that coordinates between exchange and data storage
pub struct MarketDataService {
    /// Exchange implementation
    exchange: Arc<dyn Exchange>,
    /// Data repository (wrapped in Arc for sharing across tasks)
    repository: Arc<TickDataRepository>,
    /// Symbols to monitor
    symbols: Vec<String>,
    /// Batch processing configuration
    batch_config: BatchConfig,
    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
    /// Processing statistics
    stats: Arc<Mutex<BatchStats>>,
    /// Paper trading processor
    paper_trading: Option<Arc<Mutex<PaperTradingProcessor>>>,
}

impl MarketDataService {
    /// Create a new market data service
    pub fn new(
        exchange: Arc<dyn Exchange>,
        repository: Arc<TickDataRepository>,
        symbols: Vec<String>,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(16);

        Self {
            exchange,
            repository,
            symbols,
            batch_config: BatchConfig::default(),
            shutdown_tx,
            stats: Arc::new(Mutex::new(BatchStats::default())),
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
        // Increased from 1000 to 10000 to handle burst traffic from multiple symbols
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

                    // Check channel capacity for backpressure monitoring
                    let capacity = tx.capacity();
                    let max_capacity = tx.max_capacity();
                    let utilization_pct = if max_capacity > 0 {
                        ((max_capacity - capacity) as f64 / max_capacity as f64) * 100.0
                    } else {
                        0.0
                    };

                    // Warn if channel is getting full (>80% utilized)
                    if utilization_pct > 80.0 {
                        warn!(
                            "Processing backpressure detected: channel {}% full ({}/{}), data processing may be falling behind",
                            utilization_pct as u32,
                            max_capacity - capacity,
                            max_capacity
                        );
                    } else if utilization_pct > 50.0 {
                        debug!(
                            "Channel utilization: {}% ({}/{})",
                            utilization_pct as u32,
                            max_capacity - capacity,
                            max_capacity
                        );
                    }

                    spawn(async move {
                        // Use try_send for non-blocking check, fallback to send if needed
                        match tx.try_send(tick.clone()) {
                            Ok(_) => {
                                // Successfully sent without blocking
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                // Channel is full, warn and use blocking send
                                warn!(
                                    "Channel full! Blocking until space available. Symbol: {}, Price: {}",
                                    tick.symbol, tick.price
                                );

                                if let Err(e) = tx.send(tick).await {
                                    if !e.to_string().contains("channel closed") {
                                        error!("Failed to send tick data after blocking: {}", e);
                                    }
                                }
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                // Channel closed, likely during shutdown
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
                        break; // Normal completion, exit loop
                    }
                    Err(e) => {
                        error!("Exchange subscription failed: {}", e);

                        // Check if shutdown was requested before attempting retry
                        if shutdown_rx.try_recv().is_ok() {
                            info!("Data collection shutdown requested, canceling retry");
                            break;
                        }

                        warn!("Retrying exchange connection in 5 seconds...");

                        select! {
                            _ = sleep(Duration::from_secs(5)) => {
                                continue; // Retry connection
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
        let repository = Arc::clone(&self.repository);
        let batch_config = self.batch_config.clone();
        let stats = Arc::clone(&self.stats);
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let paper_trading = self.paper_trading.clone();

        let handle = spawn(async move {
            let mut batch_buffer = Vec::with_capacity(batch_config.max_batch_size);
            let mut last_flush = Instant::now();
            let mut flush_timer = interval(Duration::from_secs(batch_config.max_batch_time));

            // Add periodic health monitoring (every 30 seconds)
            let mut health_monitor = interval(Duration::from_secs(30));
            let mut last_tick_count = 0u64;

            loop {
                select! {
                    // Receive new tick data
                    tick_opt = tick_rx.recv() => {
                        match tick_opt {
                            Some(tick) => {
                                // Update cache immediately
                                Self::update_cache_async(&repository, &tick, &stats).await;

                                // Paper transaction processing
                                if let Some(paper_trading_processor) = &paper_trading {
                                    let mut processor = paper_trading_processor.lock().await;
                                    if let Err(e) = processor.process_tick(&tick).await {
                                        warn!("Paper trading processing failed: {}", e);
                                    }
                                }

                                // Add to batch buffer
                                batch_buffer.push(tick);

                                // Update stats
                                {
                                    let mut s = stats.lock().await;
                                    s.total_ticks_processed += 1;
                                }

                                // Check if the batch is full
                                if batch_buffer.len() >= batch_config.max_batch_size {
                                    Self::flush_batch_with_retry(
                                        &repository,
                                        &mut batch_buffer,
                                        &batch_config,
                                        &stats,
                                    ).await;
                                    last_flush = Instant::now();
                                }
                            }
                            None => {
                                warn!("Tick data channel closed");
                                break;
                            }
                        }
                    }

                    _ = flush_timer.tick() => {
                        if !batch_buffer.is_empty() && last_flush.elapsed() >= Duration::from_secs(batch_config.max_batch_time) {
                            debug!("Time-based batch flush triggered (batch size: {})", batch_buffer.len());
                            Self::flush_batch_with_retry(
                                &repository,
                                &mut batch_buffer,
                                &batch_config,
                                &stats,
                            ).await;
                            last_flush = Instant::now();
                        }
                    }

                    _ = shutdown_rx.recv() => {
                        info!("Processing shutdown requested, flushing remaining data");
                        if !batch_buffer.is_empty() {
                            Self::flush_batch_with_retry(
                                &repository,
                                &mut batch_buffer,
                                &batch_config,
                                &stats,
                            ).await;
                        }
                        break;
                    }

                    _ = health_monitor.tick() => {
                        // Periodic health check and metrics logging
                        let current_stats = stats.lock().await;
                        let current_tick_count = current_stats.total_ticks_processed;
                        let ticks_processed_since_last = current_tick_count - last_tick_count;
                        last_tick_count = current_tick_count;

                        info!(
                            "Pipeline Health: {} ticks/30s | Total: {} | Batches: {} | Failed: {} | Retries: {} | Buffer: {}/{}",
                            ticks_processed_since_last,
                            current_stats.total_ticks_processed,
                            current_stats.total_batches_flushed,
                            current_stats.total_failed_batches,
                            current_stats.total_retry_attempts,
                            batch_buffer.len(),
                            batch_config.max_batch_size
                        );

                        // Warn if batch buffer is getting full
                        let buffer_utilization = (batch_buffer.len() as f64 / batch_config.max_batch_size as f64) * 100.0;
                        if buffer_utilization > 80.0 {
                            warn!(
                                "Batch buffer {}% full ({}/{}), database writes may be slow",
                                buffer_utilization as u32,
                                batch_buffer.len(),
                                batch_config.max_batch_size
                            );
                        }
                    }
                }
            }

            info!("Data processing pipeline stopped");
        });

        Ok(handle)
    }

    /// Update cache asynchronously (non-blocking)
    async fn update_cache_async(
        repository: &TickDataRepository,
        tick: &TickData,
        stats: &Arc<Mutex<BatchStats>>,
    ) {
        if let Err(e) = repository.get_cache().push_tick(tick).await {
            warn!("Failed to update cache for tick {}: {}", tick.trade_id, e);

            // Update failure stats
            {
                let mut s = stats.lock().await;
                s.cache_update_failures += 1;
            }
        } else {
            debug!("Cache updated for symbol: {}", tick.symbol);
        }
    }

    /// Flush batch to database with retry logic
    async fn flush_batch_with_retry(
        repository: &TickDataRepository,
        batch_buffer: &mut Vec<TickData>,
        config: &BatchConfig,
        stats: &Arc<Mutex<BatchStats>>,
    ) {
        if batch_buffer.is_empty() {
            return;
        }

        let batch_size = batch_buffer.len();
        let mut attempt = 0;

        loop {
            match repository.batch_insert(batch_buffer.clone()).await {
                Ok(inserted_count) => {
                    info!(
                        "Successfully flushed batch: {} ticks inserted",
                        inserted_count
                    );

                    // Update success stats
                    {
                        let mut s = stats.lock().await;
                        s.total_batches_flushed += 1;
                        s.last_flush_time = Some(chrono::Utc::now());
                    }

                    batch_buffer.clear();
                    break;
                }
                Err(e) => {
                    attempt += 1;
                    error!(
                        "Batch insert failed (attempt {}/{}): {}",
                        attempt, config.max_retry_attempts, e
                    );

                    // Update retry stats
                    {
                        let mut s = stats.lock().await;
                        s.total_retry_attempts += 1;
                    }

                    if attempt >= config.max_retry_attempts {
                        error!(
                            "Batch insert failed after {} attempts, discarding {} ticks",
                            config.max_retry_attempts, batch_size
                        );

                        // Update failure stats
                        {
                            let mut s = stats.lock().await;
                            s.total_failed_batches += 1;
                        }

                        batch_buffer.clear();
                        break;
                    }

                    // Wait before retry
                    warn!("Retrying batch insert in {}ms...", config.retry_delay_ms);
                    sleep(Duration::from_millis(config.retry_delay_ms)).await;
                }
            }
        }
    }
}
