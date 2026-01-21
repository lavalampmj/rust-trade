//! IPC Data Source
//!
//! Receives market data from data-manager via shared memory IPC.
//! This provides ultra-low latency data access for strategies.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use data_manager::schema::NormalizedTick;
use data_manager::transport::ipc::SharedMemoryChannel;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use trading_common::data::types::TickData;

/// Error type for IPC data source
#[derive(Debug, thiserror::Error)]
pub enum IpcDataSourceError {
    #[error("Failed to connect to shared memory: {0}")]
    ConnectionError(String),
    #[error("Channel closed")]
    ChannelClosed,
    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),
}

/// Configuration for IPC data source
#[derive(Debug, Clone)]
pub struct IpcDataSourceConfig {
    /// Shared memory path prefix (must match data-manager)
    pub shm_path_prefix: String,
    /// Poll interval when no data available
    pub poll_interval: Duration,
    /// Channel buffer size for tick forwarding
    pub channel_buffer_size: usize,
}

impl Default for IpcDataSourceConfig {
    fn default() -> Self {
        Self {
            shm_path_prefix: "/data_manager_".to_string(),
            poll_interval: Duration::from_micros(100),
            channel_buffer_size: 10000,
        }
    }
}

/// IPC Data Source for receiving market data from data-manager
pub struct IpcDataSource {
    config: IpcDataSourceConfig,
    /// Active consumers by symbol@exchange key
    consumers: HashMap<String, SymbolConsumer>,
    /// Statistics
    stats: Arc<IpcDataSourceStats>,
    /// Shutdown flag
    shutdown: Arc<AtomicBool>,
}

/// Per-symbol consumer
struct SymbolConsumer {
    channel: SharedMemoryChannel,
    /// Symbol (kept for identification/debugging)
    #[allow(dead_code)]
    symbol: String,
    /// Exchange (kept for identification/debugging)
    #[allow(dead_code)]
    exchange: String,
}

/// Statistics for the IPC data source
#[derive(Debug, Default)]
pub struct IpcDataSourceStats {
    pub ticks_received: AtomicU64,
    pub overflows_detected: AtomicU64,
    pub errors: AtomicU64,
}

impl IpcDataSource {
    /// Create a new IPC data source
    pub fn new(config: IpcDataSourceConfig) -> Self {
        Self {
            config,
            consumers: HashMap::new(),
            stats: Arc::new(IpcDataSourceStats::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Subscribe to a symbol from data-manager's shared memory
    pub fn subscribe(&mut self, symbol: &str, exchange: &str) -> Result<(), IpcDataSourceError> {
        let key = format!("{}@{}", symbol, exchange);

        if self.consumers.contains_key(&key) {
            debug!("Already subscribed to {}", key);
            return Ok(());
        }

        let channel = SharedMemoryChannel::open(symbol, exchange, &self.config.shm_path_prefix)
            .map_err(|e| IpcDataSourceError::ConnectionError(e.to_string()))?;

        info!("Connected to IPC channel for {}", key);

        self.consumers.insert(
            key,
            SymbolConsumer {
                channel,
                symbol: symbol.to_string(),
                exchange: exchange.to_string(),
            },
        );

        Ok(())
    }

    /// Unsubscribe from a symbol
    pub fn unsubscribe(&mut self, symbol: &str, exchange: &str) {
        let key = format!("{}@{}", symbol, exchange);
        if self.consumers.remove(&key).is_some() {
            info!("Unsubscribed from IPC channel for {}", key);
        }
    }

    /// Poll for ticks from all subscribed symbols (non-blocking)
    pub fn poll(&mut self) -> Vec<TickData> {
        let mut ticks = Vec::new();

        for (key, consumer) in &self.consumers {
            let mut ipc_consumer = consumer.channel.consumer();

            // Check for overflow
            if ipc_consumer.had_overflow() {
                self.stats.overflows_detected.fetch_add(1, Ordering::Relaxed);
                warn!("Overflow detected for {}", key);
            }

            // Drain available ticks
            while let Ok(Some(normalized_tick)) = ipc_consumer.try_recv() {
                let tick = Self::convert_tick(normalized_tick);
                ticks.push(tick);
                self.stats.ticks_received.fetch_add(1, Ordering::Relaxed);
            }
        }

        ticks
    }

    /// Poll ticks for a specific symbol (non-blocking)
    pub fn poll_symbol(&self, symbol: &str, exchange: &str) -> Result<Vec<TickData>, IpcDataSourceError> {
        let key = format!("{}@{}", symbol, exchange);

        let consumer = self
            .consumers
            .get(&key)
            .ok_or_else(|| IpcDataSourceError::SymbolNotFound(key.clone()))?;

        let mut ipc_consumer = consumer.channel.consumer();
        let mut ticks = Vec::new();

        while let Ok(Some(normalized_tick)) = ipc_consumer.try_recv() {
            let tick = Self::convert_tick(normalized_tick);
            ticks.push(tick);
        }

        Ok(ticks)
    }

    /// Start a background task that forwards ticks to a channel
    pub fn start_streaming(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<(mpsc::Receiver<TickData>, tokio::task::JoinHandle<()>), IpcDataSourceError> {
        let key = format!("{}@{}", symbol, exchange);

        // Verify symbol is subscribed (we'll create a fresh channel for streaming)
        let _consumer = self
            .consumers
            .get(&key)
            .ok_or_else(|| IpcDataSourceError::SymbolNotFound(key.clone()))?;

        let (tx, rx): (mpsc::Sender<TickData>, mpsc::Receiver<TickData>) =
            mpsc::channel(self.config.channel_buffer_size);
        let channel = SharedMemoryChannel::open(symbol, exchange, &self.config.shm_path_prefix)
            .map_err(|e| IpcDataSourceError::ConnectionError(e.to_string()))?;

        let poll_interval = self.config.poll_interval;
        let shutdown = Arc::clone(&self.shutdown);
        let stats = Arc::clone(&self.stats);
        let symbol = symbol.to_string();
        let exchange = exchange.to_string();

        let handle = tokio::spawn(async move {
            let mut ipc_consumer = channel.consumer();
            let mut consecutive_empty = 0u32;

            while !shutdown.load(Ordering::Relaxed) {
                match ipc_consumer.try_recv() {
                    Ok(Some(normalized_tick)) => {
                        let tick = Self::convert_tick(normalized_tick);
                        stats.ticks_received.fetch_add(1, Ordering::Relaxed);
                        consecutive_empty = 0;

                        if tx.send(tick).await.is_err() {
                            debug!("Channel closed for {}@{}", symbol, exchange);
                            break;
                        }
                    }
                    Ok(None) => {
                        consecutive_empty += 1;
                        // Adaptive backoff
                        let sleep_time = if consecutive_empty > 100 {
                            Duration::from_millis(1)
                        } else {
                            poll_interval
                        };
                        tokio::time::sleep(sleep_time).await;
                    }
                    Err(e) => {
                        stats.errors.fetch_add(1, Ordering::Relaxed);
                        error!("Error receiving tick for {}@{}: {}", symbol, exchange, e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }

                // Check for overflow periodically
                if ipc_consumer.had_overflow() {
                    stats.overflows_detected.fetch_add(1, Ordering::Relaxed);
                    warn!("Overflow detected for {}@{}", symbol, exchange);
                }
            }

            info!("Streaming stopped for {}@{}", symbol, exchange);
        });

        Ok((rx, handle))
    }

    /// Signal shutdown
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    /// Get statistics
    pub fn stats(&self) -> &IpcDataSourceStats {
        &self.stats
    }

    /// Get list of subscribed symbols
    pub fn subscribed_symbols(&self) -> Vec<String> {
        self.consumers.keys().cloned().collect()
    }

    /// Convert NormalizedTick to TickData
    fn convert_tick(tick: NormalizedTick) -> TickData {
        use trading_common::data::types::TradeSide;

        TickData::new(
            tick.ts_event,
            tick.symbol,
            tick.price,
            tick.size,
            match tick.side {
                data_manager::schema::TradeSide::Buy => TradeSide::Buy,
                data_manager::schema::TradeSide::Sell => TradeSide::Sell,
            },
            tick.provider_trade_id.unwrap_or_default(),
            tick.is_buyer_maker.unwrap_or(false),
        )
    }
}

impl Drop for IpcDataSource {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use data_manager::schema::TradeSide as DmTradeSide;
    use data_manager::transport::ipc::SharedMemoryConfig;
    use rust_decimal_macros::dec;

    fn create_test_tick(symbol: &str, exchange: &str, seq: i64) -> NormalizedTick {
        NormalizedTick::new(
            Utc::now(),
            symbol.to_string(),
            exchange.to_string(),
            dec!(5025.50),
            dec!(10),
            DmTradeSide::Buy,
            "test".to_string(),
            seq,
        )
    }

    #[test]
    fn test_ipc_data_source_integration() {
        // Use a consistent path prefix for both sides
        let path_prefix = "/test_ipc_int_";

        // Create a shared memory channel (simulates data-manager)
        // Use SharedMemoryChannel::create which generates the name as prefix+symbol_exchange
        let config = SharedMemoryConfig {
            path_prefix: path_prefix.to_string(),
            buffer_entries: 1024,
            entry_size: std::mem::size_of::<data_manager::schema::CompactTick>(),
        };

        let dm_channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
        let mut producer = dm_channel.producer();

        // Send some ticks
        for i in 0..100i64 {
            let tick = create_test_tick("ES", "CME", i);
            producer.send(&tick).unwrap();
        }

        // Create IPC data source (simulates trading-core)
        // Uses the same path prefix so it finds the same shared memory
        let ds_config = IpcDataSourceConfig {
            shm_path_prefix: path_prefix.to_string(),
            ..Default::default()
        };
        let mut data_source = IpcDataSource::new(ds_config);

        // Subscribe
        data_source.subscribe("ES", "CME").unwrap();

        // Poll and verify
        let ticks = data_source.poll();
        assert_eq!(ticks.len(), 100);
        assert_eq!(ticks[0].symbol, "ES");

        // Check stats
        assert_eq!(
            data_source.stats().ticks_received.load(Ordering::Relaxed),
            100
        );

        // Cleanup - construct the same name that create() would use
        let shm_name = format!("{}ES_CME", path_prefix);
        SharedMemoryChannel::unlink(&shm_name).ok();
    }

    #[test]
    fn test_tick_conversion() {
        let normalized = NormalizedTick::new(
            Utc::now(),
            "ES".to_string(),
            "CME".to_string(),
            dec!(5025.50),
            dec!(10),
            DmTradeSide::Buy,
            "test".to_string(),
            42,
        );

        let tick = IpcDataSource::convert_tick(normalized);

        assert_eq!(tick.symbol, "ES");
        assert_eq!(tick.price, dec!(5025.50));
        assert_eq!(tick.quantity, dec!(10));
    }
}
