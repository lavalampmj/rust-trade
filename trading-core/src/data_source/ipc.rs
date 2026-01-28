//! IPC Data Source
//!
//! Receives market data from data-manager via shared memory IPC.
//! This provides ultra-low latency data access for strategies.
//!
//! Uses `dbn::TradeMsg` (48 bytes) as the canonical IPC format.
//!
//! Supports:
//! - **Service Registry Discovery**: Automatically finds which data-manager instance
//!   serves a particular symbol when multiple instances are running.
//! - **Dynamic Subscription**: If a symbol's IPC channel doesn't exist, trading-core
//!   can request it from data-manager at runtime via control channel.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use data_manager::transport::ipc::{
    ControlChannelClient, ControlChannelConfig, Registry, RegistryEntry, SharedMemoryChannel,
};
use dbn::TradeMsg;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use trading_common::data::types::TickData;
use trading_common::data::TradeMsgExt;

/// Error type for IPC data source
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
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
#[allow(dead_code)]
pub struct IpcDataSourceConfig {
    /// Shared memory path prefix (must match data-manager)
    /// Only used as fallback when registry is not available
    pub shm_path_prefix: String,
    /// Poll interval when no data available
    pub poll_interval: Duration,
    /// Channel buffer size for tick forwarding
    pub channel_buffer_size: usize,
    /// Enable dynamic subscription via control channel
    pub enable_control_channel: bool,
    /// Timeout for control channel requests
    pub control_channel_timeout: Duration,
    /// Enable service registry discovery for multi-instance support
    pub enable_registry: bool,
}

impl Default for IpcDataSourceConfig {
    fn default() -> Self {
        Self {
            shm_path_prefix: "/data_manager_".to_string(),
            poll_interval: Duration::from_micros(100),
            channel_buffer_size: 10000,
            enable_control_channel: true,
            control_channel_timeout: Duration::from_secs(5),
            enable_registry: true,
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
    /// Control channel client for dynamic subscription requests
    control_client: Option<ControlChannelClient>,
    /// Service registry for multi-instance discovery
    registry: Option<Registry>,
}

/// Per-symbol consumer
struct SymbolConsumer {
    channel: SharedMemoryChannel,
    /// Symbol (needed for TradeMsg to TickData conversion)
    symbol: String,
    /// Exchange (needed for TradeMsg to TickData conversion)
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
        // Try to open the service registry for multi-instance discovery
        let registry = if config.enable_registry {
            match Registry::open() {
                Ok(reg) => {
                    let active = reg.list_healthy();
                    info!(
                        "Connected to service registry ({} healthy instance(s))",
                        active.len()
                    );
                    for (slot, entry) in &active {
                        debug!(
                            "  Slot {}: {} ({}) serving {} symbols",
                            slot,
                            entry.instance_id_str(),
                            entry.provider_str(),
                            entry.symbol_count
                        );
                    }
                    Some(reg)
                }
                Err(e) => {
                    debug!(
                        "Service registry not available: {} (will use default path prefix)",
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        // Try to connect to control channel if enabled
        let control_client = if config.enable_control_channel {
            match ControlChannelClient::open(ControlChannelConfig::default()) {
                Ok(client) => {
                    if client.is_server_active() {
                        info!("Connected to data-manager control channel");
                        Some(client)
                    } else {
                        warn!("Control channel exists but server is not active");
                        None
                    }
                }
                Err(e) => {
                    debug!("Control channel not available: {} (dynamic subscription disabled)", e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            config,
            consumers: HashMap::new(),
            stats: Arc::new(IpcDataSourceStats::default()),
            shutdown: Arc::new(AtomicBool::new(false)),
            control_client,
            registry,
        }
    }

    /// Subscribe to a symbol from data-manager's shared memory
    ///
    /// If the IPC channel doesn't exist and a control channel is available,
    /// this will request data-manager to create the channel dynamically.
    pub fn subscribe(&mut self, symbol: &str, exchange: &str) -> Result<(), IpcDataSourceError> {
        self.subscribe_with_provider(symbol, exchange, "")
    }

    /// Subscribe to a symbol with an explicit provider name
    ///
    /// The provider name is used when requesting dynamic subscription from data-manager.
    ///
    /// Discovery order:
    /// 1. Query service registry to find which data-manager serves this symbol
    /// 2. Try to open IPC channel using discovered prefix (or fallback prefix)
    /// 3. If channel doesn't exist and control channel available, request dynamic creation
    pub fn subscribe_with_provider(
        &mut self,
        symbol: &str,
        exchange: &str,
        provider: &str,
    ) -> Result<(), IpcDataSourceError> {
        let key = format!("{}@{}", symbol, exchange);

        if self.consumers.contains_key(&key) {
            debug!("Already subscribed to {}", key);
            return Ok(());
        }

        // Step 1: Check registry for which instance serves this symbol
        let discovered_entry = self.discover_instance(symbol, provider);

        // Determine the path prefix to use
        let path_prefix = match &discovered_entry {
            Some(entry) => {
                let prefix = entry.channel_prefix_str();
                info!(
                    "Registry: {} served by {} (prefix: {})",
                    symbol,
                    entry.instance_id_str(),
                    prefix
                );
                prefix.to_string()
            }
            None => {
                debug!(
                    "No registry entry for {}, using default prefix: {}",
                    symbol, self.config.shm_path_prefix
                );
                self.config.shm_path_prefix.clone()
            }
        };

        // Step 2: Try to open the IPC channel
        match SharedMemoryChannel::open(symbol, exchange, &path_prefix) {
            Ok(channel) => {
                info!("Connected to IPC channel for {} (prefix: {})", key, path_prefix);
                self.consumers.insert(
                    key,
                    SymbolConsumer {
                        channel,
                        symbol: symbol.to_string(),
                        exchange: exchange.to_string(),
                    },
                );
                return Ok(());
            }
            Err(e) => {
                debug!("IPC channel not found for {} at prefix {}: {}", key, path_prefix, e);
            }
        }

        // Step 3: If we have a control channel, try to request the subscription
        if let Some(ref control_client) = self.control_client {
            info!(
                "Requesting dynamic subscription for {} via control channel",
                key
            );

            match control_client.subscribe(
                symbol,
                exchange,
                provider,
                self.config.control_channel_timeout,
            ) {
                Ok(()) => {
                    info!("Dynamic subscription request succeeded for {}", key);

                    // Small delay to allow channel creation
                    std::thread::sleep(Duration::from_millis(50));

                    // Try to open the channel again
                    // After dynamic creation, refresh the prefix from registry
                    let updated_entry = self.discover_instance(symbol, provider);
                    let updated_prefix = updated_entry
                        .as_ref()
                        .map(|e| e.channel_prefix_str().to_string())
                        .unwrap_or_else(|| self.config.shm_path_prefix.clone());

                    let channel =
                        SharedMemoryChannel::open(symbol, exchange, &updated_prefix)
                            .map_err(|e| {
                                IpcDataSourceError::ConnectionError(format!(
                                    "Channel created but failed to open: {}",
                                    e
                                ))
                            })?;

                    info!("Connected to dynamically created IPC channel for {}", key);
                    self.consumers.insert(
                        key,
                        SymbolConsumer {
                            channel,
                            symbol: symbol.to_string(),
                            exchange: exchange.to_string(),
                        },
                    );
                    return Ok(());
                }
                Err(e) => {
                    // Control channel request failed - symbol likely not subscribed on provider
                    return Err(IpcDataSourceError::ConnectionError(format!(
                        "Dynamic subscription failed for {}: {}",
                        key, e
                    )));
                }
            }
        }

        // No control channel available and direct open failed
        Err(IpcDataSourceError::ConnectionError(format!(
            "IPC channel not found for {} and no control channel available. \
             Ensure data-manager is running with --symbols {} or --ipc-symbols {}",
            key, symbol, symbol
        )))
    }

    /// Discover which data-manager instance serves a symbol
    ///
    /// Returns the registry entry for the best matching instance, or None if not found.
    fn discover_instance(&self, symbol: &str, provider: &str) -> Option<RegistryEntry> {
        let registry = self.registry.as_ref()?;

        // Find instances serving this symbol
        let instances = registry.find_by_symbol(symbol);

        if instances.is_empty() {
            debug!("No registry entries found for symbol {}", symbol);
            return None;
        }

        // If a specific provider is requested, prefer that
        if !provider.is_empty() {
            if let Some((_, entry)) = instances
                .iter()
                .find(|(_, e)| e.provider_str() == provider)
            {
                return Some(entry.clone());
            }
            debug!(
                "No instance found for {} with provider '{}', using first available",
                symbol, provider
            );
        }

        // Return the first healthy instance
        instances.into_iter().next().map(|(_, e)| e)
    }

    /// List all healthy data-manager instances from registry
    pub fn list_instances(&self) -> Vec<(usize, RegistryEntry)> {
        match &self.registry {
            Some(reg) => reg.list_healthy(),
            None => Vec::new(),
        }
    }

    /// Refresh the registry connection
    pub fn refresh_registry(&mut self) {
        if !self.config.enable_registry {
            return;
        }

        match Registry::open() {
            Ok(reg) => {
                let count = reg.list_healthy().len();
                info!("Refreshed registry connection ({} healthy instances)", count);
                self.registry = Some(reg);
            }
            Err(e) => {
                debug!("Failed to refresh registry: {}", e);
            }
        }
    }

    /// Unsubscribe from a symbol
    #[allow(dead_code)]
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
                self.stats
                    .overflows_detected
                    .fetch_add(1, Ordering::Relaxed);
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
    #[allow(dead_code)]
    pub fn poll_symbol(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> Result<Vec<TickData>, IpcDataSourceError> {
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
    #[allow(dead_code)]
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
