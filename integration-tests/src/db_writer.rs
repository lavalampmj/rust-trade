//! Database writer for integration tests
//!
//! This module provides a component that writes ticks to TimescaleDB
//! during integration tests, enabling verification of database persistence.

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use data_manager::schema::NormalizedTick;

/// Database writer errors
#[derive(Error, Debug)]
pub enum DbWriterError {
    #[error("Database connection error: {0}")]
    Connection(String),

    #[error("Write error: {0}")]
    Write(#[from] sqlx::Error),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Writer not started")]
    NotStarted,
}

pub type DbWriterResult<T> = Result<T, DbWriterError>;

/// Configuration for the database writer
#[derive(Debug, Clone)]
pub struct DbWriterConfig {
    /// Database URL
    pub database_url: String,
    /// Batch size for inserts
    pub batch_size: usize,
    /// Flush interval in milliseconds
    pub flush_interval_ms: u64,
    /// Channel buffer size
    pub channel_buffer: usize,
}

impl Default for DbWriterConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            batch_size: 1000,
            flush_interval_ms: 100,
            channel_buffer: 10000,
        }
    }
}

impl DbWriterConfig {
    /// Create config with database URL
    pub fn new(database_url: String) -> Self {
        Self {
            database_url,
            ..Default::default()
        }
    }

    /// Set batch size
    pub fn with_batch_size(mut self, size: usize) -> Self {
        self.batch_size = size;
        self
    }

    /// Set flush interval
    pub fn with_flush_interval_ms(mut self, ms: u64) -> Self {
        self.flush_interval_ms = ms;
        self
    }
}

/// Metrics collected by the database writer
#[derive(Debug, Clone, Default)]
pub struct DbWriterMetrics {
    /// Total ticks received
    pub ticks_received: Arc<AtomicU64>,
    /// Total ticks written to database
    pub ticks_written: Arc<AtomicU64>,
    /// Total ticks dropped (errors)
    pub ticks_dropped: Arc<AtomicU64>,
    /// Number of batch writes
    pub batch_writes: Arc<AtomicU64>,
    /// Start time
    pub start_time: Arc<Mutex<Option<DateTime<Utc>>>>,
    /// End time
    pub end_time: Arc<Mutex<Option<DateTime<Utc>>>>,
}

impl DbWriterMetrics {
    /// Get received count
    pub fn received_count(&self) -> u64 {
        self.ticks_received.load(Ordering::SeqCst)
    }

    /// Get written count
    pub fn written_count(&self) -> u64 {
        self.ticks_written.load(Ordering::SeqCst)
    }

    /// Get dropped count
    pub fn dropped_count(&self) -> u64 {
        self.ticks_dropped.load(Ordering::SeqCst)
    }

    /// Get batch write count
    pub fn batch_count(&self) -> u64 {
        self.batch_writes.load(Ordering::SeqCst)
    }
}

/// Database writer that persists ticks during tests
///
/// Uses a background task with batching for efficient writes.
pub struct DbWriter {
    config: DbWriterConfig,
    pool: Option<PgPool>,
    sender: Option<mpsc::Sender<NormalizedTick>>,
    metrics: DbWriterMetrics,
    running: Arc<AtomicBool>,
    write_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl DbWriter {
    /// Create a new database writer
    pub fn new(config: DbWriterConfig) -> Self {
        Self {
            config,
            pool: None,
            sender: None,
            metrics: DbWriterMetrics::default(),
            running: Arc::new(AtomicBool::new(false)),
            write_handle: Mutex::new(None),
        }
    }

    /// Connect to the database
    pub async fn connect(&mut self) -> DbWriterResult<()> {
        info!("Connecting to database for tick persistence...");

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .min_connections(1)
            .acquire_timeout(Duration::from_secs(30))
            .connect(&self.config.database_url)
            .await
            .map_err(|e| DbWriterError::Connection(e.to_string()))?;

        self.pool = Some(pool);
        info!("Database writer connected");
        Ok(())
    }

    /// Start the background writer task
    pub fn start(&mut self) -> DbWriterResult<mpsc::Sender<NormalizedTick>> {
        let pool = self
            .pool
            .clone()
            .ok_or_else(|| DbWriterError::Connection("Not connected".into()))?;

        let (tx, rx) = mpsc::channel(self.config.channel_buffer);
        self.sender = Some(tx.clone());

        let metrics = self.metrics.clone();
        let running = self.running.clone();
        let batch_size = self.config.batch_size;
        let flush_interval = Duration::from_millis(self.config.flush_interval_ms);

        running.store(true, Ordering::SeqCst);
        *metrics.start_time.lock() = Some(Utc::now());

        let handle = tokio::spawn(async move {
            write_loop(pool, rx, metrics, running, batch_size, flush_interval).await;
        });

        *self.write_handle.lock() = Some(handle);

        info!("Database writer started");
        Ok(tx)
    }

    /// Send a tick to be written
    pub async fn write_tick(&self, tick: NormalizedTick) -> DbWriterResult<()> {
        let sender = self
            .sender
            .as_ref()
            .ok_or(DbWriterError::NotStarted)?;

        self.metrics.ticks_received.fetch_add(1, Ordering::SeqCst);

        sender
            .send(tick)
            .await
            .map_err(|e| DbWriterError::Channel(e.to_string()))?;

        Ok(())
    }

    /// Get a sender for sending ticks from callbacks
    pub fn get_sender(&self) -> Option<mpsc::Sender<NormalizedTick>> {
        self.sender.clone()
    }

    /// Stop the writer and flush remaining ticks
    pub async fn stop(&mut self) -> DbWriterResult<()> {
        info!("Stopping database writer...");

        self.running.store(false, Ordering::SeqCst);

        // Drop sender to signal end
        self.sender.take();

        // Wait for write task to complete
        if let Some(handle) = self.write_handle.lock().take() {
            let _ = handle.await;
        }

        *self.metrics.end_time.lock() = Some(Utc::now());

        info!(
            "Database writer stopped: {} received, {} written, {} dropped",
            self.metrics.received_count(),
            self.metrics.written_count(),
            self.metrics.dropped_count()
        );

        Ok(())
    }

    /// Get the metrics
    pub fn metrics(&self) -> &DbWriterMetrics {
        &self.metrics
    }

    /// Close the database connection
    pub async fn close(mut self) {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
    }
}

/// Background write loop
async fn write_loop(
    pool: PgPool,
    mut rx: mpsc::Receiver<NormalizedTick>,
    metrics: DbWriterMetrics,
    running: Arc<AtomicBool>,
    batch_size: usize,
    flush_interval: Duration,
) {
    let mut batch: Vec<NormalizedTick> = Vec::with_capacity(batch_size);
    let mut last_flush = tokio::time::Instant::now();

    loop {
        // Try to receive with timeout for periodic flushing
        let timeout_duration = flush_interval.saturating_sub(last_flush.elapsed());

        tokio::select! {
            tick_opt = rx.recv() => {
                match tick_opt {
                    Some(tick) => {
                        batch.push(tick);

                        // Flush if batch is full
                        if batch.len() >= batch_size {
                            flush_batch(&pool, &mut batch, &metrics).await;
                            last_flush = tokio::time::Instant::now();
                        }
                    }
                    None => {
                        // Channel closed, flush remaining and exit
                        if !batch.is_empty() {
                            flush_batch(&pool, &mut batch, &metrics).await;
                        }
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(timeout_duration) => {
                // Periodic flush
                if !batch.is_empty() {
                    flush_batch(&pool, &mut batch, &metrics).await;
                    last_flush = tokio::time::Instant::now();
                }

                // Check if we should exit
                if !running.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
    }

    debug!("Write loop exited");
}

/// Flush a batch of ticks to the database
async fn flush_batch(pool: &PgPool, batch: &mut Vec<NormalizedTick>, metrics: &DbWriterMetrics) {
    if batch.is_empty() {
        return;
    }

    let batch_len = batch.len();
    debug!("Flushing batch of {} ticks", batch_len);

    match insert_batch(pool, batch).await {
        Ok(written) => {
            metrics.ticks_written.fetch_add(written as u64, Ordering::SeqCst);
            metrics.batch_writes.fetch_add(1, Ordering::SeqCst);

            if written < batch_len {
                // Some were duplicates (ON CONFLICT DO NOTHING)
                let duplicates = batch_len - written;
                debug!("{} duplicate ticks skipped", duplicates);
            }
        }
        Err(e) => {
            error!("Failed to write batch: {}", e);
            metrics.ticks_dropped.fetch_add(batch_len as u64, Ordering::SeqCst);
        }
    }

    batch.clear();
}

/// Insert a batch of ticks using bulk insert
async fn insert_batch(pool: &PgPool, ticks: &[NormalizedTick]) -> DbWriterResult<usize> {
    if ticks.is_empty() {
        return Ok(0);
    }

    // Build bulk insert query
    let mut query = String::from(
        r#"
        INSERT INTO market_ticks (
            ts_event, ts_recv, symbol, exchange, price, size, side,
            provider, provider_trade_id, is_buyer_maker, sequence
        ) VALUES
        "#,
    );

    let mut param_count = 1;

    for (i, _tick) in ticks.iter().enumerate() {
        if i > 0 {
            query.push_str(", ");
        }

        query.push_str(&format!(
            "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
            param_count,
            param_count + 1,
            param_count + 2,
            param_count + 3,
            param_count + 4,
            param_count + 5,
            param_count + 6,
            param_count + 7,
            param_count + 8,
            param_count + 9,
            param_count + 10,
        ));
        param_count += 11;
    }

    query.push_str(" ON CONFLICT DO NOTHING");

    // Build and execute query
    let mut sqlx_query = sqlx::query(&query);

    for tick in ticks {
        sqlx_query = sqlx_query
            .bind(tick.ts_event)
            .bind(tick.ts_recv)
            .bind(&tick.symbol)
            .bind(&tick.exchange)
            .bind(tick.price)
            .bind(tick.size)
            .bind(tick.side.as_char().to_string())
            .bind(&tick.provider)
            .bind(&tick.provider_trade_id)
            .bind(tick.is_buyer_maker)
            .bind(tick.sequence);
    }

    let result = sqlx_query.execute(pool).await?;
    Ok(result.rows_affected() as usize)
}

/// Create a callback function that sends ticks to both strategy runners and database
///
/// This is a helper to create a dual-purpose callback for testing.
pub fn create_db_callback(
    db_sender: mpsc::Sender<NormalizedTick>,
) -> impl Fn(NormalizedTick) + Send + Sync + 'static {
    move |tick: NormalizedTick| {
        let sender = db_sender.clone();
        tokio::spawn(async move {
            if let Err(e) = sender.send(tick).await {
                warn!("Failed to send tick to db writer: {}", e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = DbWriterConfig::new("postgres://localhost/test".to_string())
            .with_batch_size(500)
            .with_flush_interval_ms(50);

        assert_eq!(config.batch_size, 500);
        assert_eq!(config.flush_interval_ms, 50);
    }

    #[test]
    fn test_metrics_default() {
        let metrics = DbWriterMetrics::default();
        assert_eq!(metrics.received_count(), 0);
        assert_eq!(metrics.written_count(), 0);
        assert_eq!(metrics.dropped_count(), 0);
    }
}
