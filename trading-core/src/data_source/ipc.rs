//! IPC Data Source
//!
//! Receives market data from data-manager via shared memory IPC.
//! This provides ultra-low latency data access for strategies.
//!
//! Uses `dbn::TradeMsg` (48 bytes) as the canonical IPC format.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dbn::TradeMsg;
use data_manager::transport::ipc::SharedMemoryChannel;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use trading_common::data::types::TickData;
use trading_common::data::TradeMsgExt;

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
    /// Symbol (needed for TradeMsg to TickData conversion)
    symbol: String,
    /// Exchange (needed for TradeMsg to TickData conversion)
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

            // Drain available ticks (now TradeMsg)
            while let Ok(Some(trade_msg)) = ipc_consumer.try_recv() {
                let tick = Self::convert_trade_msg(&trade_msg, &consumer.symbol);
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

        while let Ok(Some(trade_msg)) = ipc_consumer.try_recv() {
            let tick = Self::convert_trade_msg(&trade_msg, symbol);
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
                    Ok(Some(trade_msg)) => {
                        let tick = convert_trade_msg_static(&trade_msg, &symbol);
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

    /// Convert TradeMsg to TickData
    ///
    /// Requires the symbol name since TradeMsg only contains instrument_id
    fn convert_trade_msg(msg: &TradeMsg, symbol: &str) -> TickData {
        convert_trade_msg_static(msg, symbol)
    }
}

/// Convert TradeMsg to TickData (static function for async contexts)
fn convert_trade_msg_static(msg: &TradeMsg, symbol: &str) -> TickData {
    use trading_common::data::types::TradeSide;
    use trading_common::data::TradeSideCompat;

    let side = match msg.trade_side() {
        TradeSideCompat::Buy => TradeSide::Buy,
        TradeSideCompat::Sell => TradeSide::Sell,
        TradeSideCompat::None => TradeSide::Sell, // Default to Sell for unknown
    };

    TickData::new(
        msg.timestamp(),
        symbol.to_string(),
        msg.price_decimal(),
        msg.size_decimal(),
        side,
        msg.sequence.to_string(),
        msg.side as u8 as char == 'A', // Ask side = buyer is maker
    )
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
    use data_manager::transport::ipc::SharedMemoryConfig;
    use rust_decimal_macros::dec;
    use trading_common::data::{create_trade_msg_from_decimals, TradeSideCompat};

    fn create_test_msg(symbol: &str, exchange: &str, seq: u32) -> TradeMsg {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap() as u64;
        create_trade_msg_from_decimals(
            now_nanos,
            now_nanos,
            symbol,
            exchange,
            dec!(5025.50),
            dec!(10),
            TradeSideCompat::Buy,
            seq,
        )
    }

    #[test]
    fn test_ipc_data_source_integration() {
        // Use a consistent path prefix for both sides
        let path_prefix = "/test_ipc_int_";

        // Create a shared memory channel (simulates data-manager)
        let config = SharedMemoryConfig {
            path_prefix: path_prefix.to_string(),
            buffer_entries: 1024,
            entry_size: std::mem::size_of::<TradeMsg>(),
        };

        let dm_channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
        let mut producer = dm_channel.producer();

        // Send some TradeMsg (now 48 bytes each)
        for i in 0..100u32 {
            let msg = create_test_msg("ES", "CME", i);
            producer.send(&msg).unwrap();
        }

        // Create IPC data source (simulates trading-core)
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

        // Cleanup
        let shm_name = format!("{}ES_CME", path_prefix);
        SharedMemoryChannel::unlink(&shm_name).ok();
    }

    #[test]
    fn test_trade_msg_conversion() {
        let msg = create_test_msg("ES", "CME", 42);
        let tick = convert_trade_msg_static(&msg, "ES");

        assert_eq!(tick.symbol, "ES");
        // Price should be close to 5025.50 (may have small rounding)
        assert!((tick.price - dec!(5025.50)).abs() < dec!(0.01));
        assert_eq!(tick.quantity, dec!(10));
        assert_eq!(tick.trade_id, "42");
    }
}
