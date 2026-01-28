//! Test data emulator implementing the LiveStreamProvider trait
//!
//! This module provides a `TestDataEmulator` that replays pre-generated test data
//! through the data pipeline, simulating a live data feed. It implements the
//! `LiveStreamProvider` trait from data-manager.
//!
//! # Transport Modes
//!
//! The emulator supports two transport modes:
//!
//! - **Direct**: In-memory callback (default, lowest latency)
//! - **WebSocket**: Network transport for realistic latency simulation
//!
//! Configure via `EmulatorConfig::transport.mode`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dbn::record::TradeMsg;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

use data_manager::provider::{
    ConnectionStatus, DataProvider, DataType, LiveStreamProvider, LiveSubscription, ProviderError,
    ProviderResult, StreamCallback, StreamEvent, SubscriptionStatus, VenueCapabilities,
    VenueConnection, VenueConnectionStatus, VenueInfo,
};
use data_manager::schema::{NormalizedTick, TradeSide};
use data_manager::symbol::SymbolSpec;
use trading_common::data::dbn_types::{TradeMsgExt, TradeSideCompat};

use crate::config::EmulatorConfig;
use crate::generator::TestDataBundle;
use crate::transport::{create_transport, TickTransport, TransportMode};

/// Metrics collected during emulation
#[derive(Debug, Clone, Default)]
pub struct EmulatorMetrics {
    /// Total ticks sent
    pub ticks_sent: Arc<AtomicU64>,
    /// Total ticks dropped (due to errors)
    pub ticks_dropped: Arc<AtomicU64>,
    /// Start time of replay
    pub replay_start: Option<DateTime<Utc>>,
    /// End time of replay
    pub replay_end: Option<DateTime<Utc>>,
}

impl EmulatorMetrics {
    /// Get the number of ticks sent
    pub fn sent_count(&self) -> u64 {
        self.ticks_sent.load(Ordering::SeqCst)
    }

    /// Get the number of ticks dropped
    pub fn dropped_count(&self) -> u64 {
        self.ticks_dropped.load(Ordering::SeqCst)
    }

    /// Increment sent counter
    pub fn inc_sent(&self) {
        self.ticks_sent.fetch_add(1, Ordering::SeqCst);
    }

    /// Increment dropped counter
    pub fn inc_dropped(&self) {
        self.ticks_dropped.fetch_add(1, Ordering::SeqCst);
    }
}

/// Test data emulator that implements LiveStreamProvider
///
/// Replays pre-generated test data through the data pipeline, rewriting
/// timestamps to current system time and embedding send timestamps in
/// the ts_in_delta field for latency measurement.
pub struct TestDataEmulator {
    /// Pre-generated test data bundle
    bundle: TestDataBundle,
    /// Configuration for the emulator
    config: EmulatorConfig,
    /// Venue info
    info: VenueInfo,
    /// Connection status
    connected: AtomicBool,
    /// Metrics collection
    metrics: EmulatorMetrics,
    /// Mapping from instrument_id to symbol name
    instrument_to_symbol: HashMap<u32, String>,
    /// Exchange name
    exchange: String,
}

impl TestDataEmulator {
    /// Create a new emulator with the given test data and configuration
    pub fn new(bundle: TestDataBundle, config: EmulatorConfig) -> Self {
        // Build instrument_id -> symbol mapping
        let mut instrument_to_symbol = HashMap::new();
        for symbol in &bundle.metadata.symbols {
            let instrument_id = trading_common::data::dbn_types::symbol_to_instrument_id(
                symbol,
                &bundle.metadata.exchange,
            );
            instrument_to_symbol.insert(instrument_id, symbol.clone());
        }

        let info = VenueInfo::data_provider("test_emulator", "Test Data Emulator")
            .with_version("1.0.0")
            .with_exchanges(vec![bundle.metadata.exchange.clone()])
            .with_capabilities(VenueCapabilities {
                supports_live_streaming: true,
                supports_historical: false,
                ..VenueCapabilities::default()
            });

        Self {
            exchange: bundle.metadata.exchange.clone(),
            bundle,
            config,
            info,
            connected: AtomicBool::new(false),
            metrics: EmulatorMetrics::default(),
            instrument_to_symbol,
        }
    }

    /// Create a new emulator with default configuration
    pub fn with_default_config(bundle: TestDataBundle) -> Self {
        Self::new(bundle, EmulatorConfig::default())
    }

    /// Get the metrics collected during emulation
    pub fn metrics(&self) -> &EmulatorMetrics {
        &self.metrics
    }

    /// Get a clone of the metrics for external tracking
    pub fn metrics_clone(&self) -> EmulatorMetrics {
        self.metrics.clone()
    }

    /// Convert a TradeMsg to NormalizedTick with timestamp rewriting
    ///
    /// This function:
    /// 1. Rewrites ts_event and ts_recv to current system time
    /// 2. Embeds the send time (microseconds) in the ts_in_delta field
    fn trade_msg_to_normalized_tick(&self, msg: &TradeMsg, symbol: &str) -> (NormalizedTick, i32) {
        let now = Utc::now();
        let now_micros = now.timestamp_micros();

        // Embed send time in ts_in_delta (lower 31 bits, microseconds)
        // This allows receivers to compute latency
        let ts_in_delta = (now_micros % 0x7FFF_FFFF) as i32;

        // Convert price from fixed-point
        let price = msg.price_decimal();
        let size = msg.size_decimal();

        // Convert side
        let side = match msg.trade_side() {
            TradeSideCompat::Buy => TradeSide::Buy,
            TradeSideCompat::Sell => TradeSide::Sell,
            TradeSideCompat::None => TradeSide::Buy, // Default to buy if unknown
        };

        let tick = NormalizedTick::with_details(
            now,
            now,
            symbol.to_string(),
            self.exchange.clone(),
            price,
            size,
            side,
            "test_emulator".to_string(),
            Some(format!("test_{}", msg.sequence)),
            Some(msg.is_buy()),
            msg.sequence as i64,
        );

        (tick, ts_in_delta)
    }

    /// Calculate inter-tick delay based on original timestamps and replay speed
    fn calculate_delay(&self, prev_ts: u64, curr_ts: u64) -> Duration {
        if self.config.replay_speed <= 0.0 || prev_ts >= curr_ts {
            return Duration::from_micros(self.config.min_delay_us);
        }

        let diff_nanos = curr_ts - prev_ts;
        let adjusted_nanos = (diff_nanos as f64 / self.config.replay_speed) as u64;
        let delay_micros = (adjusted_nanos / 1000).max(self.config.min_delay_us);

        Duration::from_micros(delay_micros)
    }

    /// Replay all ticks through the transport layer
    async fn replay_ticks(
        &self,
        transport: &dyn TickTransport,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        let ticks = &self.bundle.ticks;
        if ticks.is_empty() {
            return Ok(());
        }

        let mut prev_ts = ticks[0].hd.ts_event;

        // Minimum sleep threshold in microseconds - sleeping for less than this
        // is counterproductive due to tokio timer granularity (~1-10ms overhead)
        const MIN_SLEEP_THRESHOLD_US: u64 = 1000; // 1ms

        // Yield every N ticks when delay is below threshold to prevent overwhelming
        // the async runtime (especially important for WebSocket transport)
        const YIELD_INTERVAL: usize = 10;
        let mut tick_count = 0;

        for tick in ticks.iter() {
            // Check for shutdown
            if shutdown_rx.try_recv().is_ok() {
                tracing::info!("Emulator received shutdown signal");
                break;
            }

            // Calculate and apply inter-tick delay
            let delay = self.calculate_delay(prev_ts, tick.hd.ts_event);
            if delay.as_micros() >= MIN_SLEEP_THRESHOLD_US as u128 {
                sleep(delay).await;
            } else {
                // For small delays, yield periodically to let other tasks run
                // This is critical for WebSocket transport to avoid overwhelming it
                tick_count += 1;
                if tick_count % YIELD_INTERVAL == 0 {
                    tokio::task::yield_now().await;
                }
            }
            prev_ts = tick.hd.ts_event;

            // Get symbol name from instrument_id
            let symbol = match self.instrument_to_symbol.get(&tick.hd.instrument_id) {
                Some(s) => s.clone(),
                None => {
                    self.metrics.inc_dropped();
                    continue;
                }
            };

            // Convert to NormalizedTick with timestamp rewriting, then to TickData
            let (normalized, _ts_in_delta) = self.trade_msg_to_normalized_tick(tick, &symbol);

            // Send through transport layer (convert NormalizedTick to TickData)
            if transport
                .send(StreamEvent::Tick(normalized.into()))
                .await
                .is_ok()
            {
                self.metrics.inc_sent();
            } else {
                self.metrics.inc_dropped();
            }
        }

        Ok(())
    }

    /// Get the transport mode being used
    pub fn transport_mode(&self) -> TransportMode {
        self.config.transport.mode
    }
}

#[async_trait]
#[async_trait]
impl VenueConnection for TestDataEmulator {
    fn info(&self) -> &VenueInfo {
        &self.info
    }

    async fn connect(&mut self) -> trading_common::venue::VenueResult<()> {
        self.connected.store(true, Ordering::SeqCst);
        tracing::info!("Test emulator connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> trading_common::venue::VenueResult<()> {
        self.connected.store(false, Ordering::SeqCst);
        tracing::info!("Test emulator disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn connection_status(&self) -> VenueConnectionStatus {
        if self.connected.load(Ordering::SeqCst) {
            VenueConnectionStatus::Connected
        } else {
            VenueConnectionStatus::Disconnected
        }
    }
}

#[async_trait]
impl DataProvider for TestDataEmulator {
    async fn discover_symbols(&self, _exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>> {
        Ok(self
            .bundle
            .metadata
            .symbols
            .iter()
            .map(|s| SymbolSpec::new(s.clone(), self.exchange.clone()))
            .collect())
    }
}

#[async_trait]
impl LiveStreamProvider for TestDataEmulator {
    async fn subscribe(
        &mut self,
        _subscription: LiveSubscription,
        callback: StreamCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        if !self.is_connected() {
            return Err(ProviderError::NotConnected);
        }

        let transport_mode = self.config.transport.mode;
        tracing::info!(
            "Starting emulator replay: {} ticks, {} symbols, transport={:?}",
            self.bundle.ticks.len(),
            self.bundle.metadata.symbols.len(),
            transport_mode
        );

        // Create and start transport
        let mut transport = create_transport(
            &self.config.transport,
            callback.clone(),
            shutdown_rx.resubscribe(),
        );
        transport.start().await?;

        // Notify connection status through transport
        transport
            .send(StreamEvent::Status(ConnectionStatus::Connected))
            .await?;

        // Replay ticks through transport
        self.replay_ticks(transport.as_ref(), shutdown_rx).await?;

        tracing::info!(
            "Emulator replay complete: {} sent, {} dropped, transport_sent={}",
            self.metrics.sent_count(),
            self.metrics.dropped_count(),
            transport.sent_count()
        );

        // Notify completion through transport
        let _ = transport
            .send(StreamEvent::Status(ConnectionStatus::Disconnected))
            .await;

        // Stop transport
        transport.stop().await?;

        Ok(())
    }

    async fn unsubscribe(&mut self, _symbols: &[SymbolSpec]) -> ProviderResult<()> {
        // No-op for emulator
        Ok(())
    }

    fn subscription_status(&self) -> SubscriptionStatus {
        SubscriptionStatus {
            symbols: self
                .bundle
                .metadata
                .symbols
                .iter()
                .map(|s| SymbolSpec::new(s.clone(), self.exchange.clone()))
                .collect(),
            connection: if self.is_connected() {
                ConnectionStatus::Connected
            } else {
                ConnectionStatus::Disconnected
            },
            messages_received: self.metrics.sent_count(),
            last_message: None,
        }
    }
}

/// Helper function to extract latency from ts_in_delta field
///
/// The emulator embeds the send time (microseconds modulo 2^31) in ts_in_delta.
/// This function calculates the latency by comparing with current time.
pub fn extract_latency_us(ts_in_delta: i32) -> Option<u64> {
    if ts_in_delta <= 0 {
        return None;
    }

    let sent_us = ts_in_delta as u64;
    let recv_us = (Utc::now().timestamp_micros() % 0x7FFF_FFFF) as u64;

    // Handle wraparound
    let latency = if recv_us >= sent_us {
        recv_us - sent_us
    } else {
        // Wraparound occurred
        (0x7FFF_FFFF - sent_us) + recv_us
    };

    // Sanity check: latency should be less than ~30 minutes (1.8B microseconds)
    // If it's larger, we probably have a timing issue
    if latency < 1_800_000_000 {
        Some(latency)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DataGenConfig, VolumeProfile};
    use crate::generator::TestDataGenerator;
    use std::sync::atomic::AtomicU64;
    use std::time::Instant;

    fn create_test_bundle(symbol_count: usize, time_window_secs: u64) -> TestDataBundle {
        let config = DataGenConfig {
            symbol_count,
            time_window_secs,
            profile: VolumeProfile::Lite,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };
        let mut gen = TestDataGenerator::new(config);
        gen.generate()
    }

    #[tokio::test]
    async fn test_emulator_creation() {
        let bundle = create_test_bundle(3, 5);
        let emulator = TestDataEmulator::with_default_config(bundle.clone());

        assert_eq!(emulator.info().name, "test_emulator");
        assert!(!emulator.is_connected());
        assert_eq!(emulator.metrics.sent_count(), 0);
    }

    #[tokio::test]
    async fn test_emulator_connect_disconnect() {
        let bundle = create_test_bundle(2, 5);
        let mut emulator = TestDataEmulator::with_default_config(bundle);

        assert!(!emulator.is_connected());

        emulator.connect().await.unwrap();
        assert!(emulator.is_connected());

        emulator.disconnect().await.unwrap();
        assert!(!emulator.is_connected());
    }

    #[tokio::test]
    async fn test_emulator_discover_symbols() {
        let bundle = create_test_bundle(5, 5);
        let emulator = TestDataEmulator::with_default_config(bundle);

        let symbols = emulator.discover_symbols(None).await.unwrap();
        assert_eq!(symbols.len(), 5);
        assert!(symbols.iter().any(|s| s.symbol == "TEST0000"));
        assert!(symbols.iter().any(|s| s.symbol == "TEST0004"));
    }

    #[tokio::test]
    async fn test_emulator_replay() {
        let bundle = create_test_bundle(2, 2);
        let expected_ticks = bundle.ticks.len();

        // Use Direct transport for reliable unit testing (WebSocket has timing issues in tests)
        let config = EmulatorConfig {
            replay_speed: 100.0, // Speed up for testing
            embed_send_time: true,
            min_delay_us: 1,
            transport: crate::transport::TransportConfig {
                mode: crate::transport::TransportMode::Direct,
                websocket: Default::default(),
            },
        };
        let mut emulator = TestDataEmulator::new(bundle, config);

        let received = Arc::new(AtomicU64::new(0));
        let received_clone = received.clone();

        let callback: StreamCallback = Arc::new(move |event| {
            if matches!(event, StreamEvent::Tick(_)) {
                received_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

        emulator.connect().await.unwrap();

        let start = Instant::now();
        emulator
            .subscribe(LiveSubscription::trades(vec![]), callback, shutdown_rx)
            .await
            .unwrap();
        let duration = start.elapsed();

        assert_eq!(received.load(Ordering::SeqCst), expected_ticks as u64);
        assert_eq!(emulator.metrics.sent_count(), expected_ticks as u64);

        // With 100x speed, should complete very quickly
        assert!(
            duration.as_secs() < 5,
            "Replay took too long: {:?}",
            duration
        );
    }

    #[tokio::test]
    async fn test_emulator_shutdown() {
        let bundle = create_test_bundle(3, 10); // Longer time window
                                                // Use Direct transport for reliable unit testing
        let config = EmulatorConfig {
            replay_speed: 1.0, // Real-time (will be slow)
            embed_send_time: true,
            min_delay_us: 1000, // 1ms minimum
            transport: crate::transport::TransportConfig {
                mode: crate::transport::TransportMode::Direct,
                websocket: Default::default(),
            },
        };
        let mut emulator = TestDataEmulator::new(bundle.clone(), config);

        let received = Arc::new(AtomicU64::new(0));
        let received_clone = received.clone();

        let callback: StreamCallback = Arc::new(move |event| {
            if matches!(event, StreamEvent::Tick(_)) {
                received_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        emulator.connect().await.unwrap();

        // Spawn replay in background and send shutdown after 100ms
        let emulator_future =
            emulator.subscribe(LiveSubscription::trades(vec![]), callback, shutdown_rx);

        let shutdown_future = async {
            sleep(Duration::from_millis(100)).await;
            shutdown_tx.send(()).ok();
        };

        let _ = tokio::join!(emulator_future, shutdown_future);

        // Should have received some but not all ticks
        let received_count = received.load(Ordering::SeqCst);
        assert!(received_count > 0);
        assert!(received_count < bundle.ticks.len() as u64);
    }

    #[test]
    fn test_latency_extraction() {
        // Test normal case
        let now_us = Utc::now().timestamp_micros();
        let ts_in_delta = ((now_us - 100) % 0x7FFF_FFFF) as i32;
        let latency = extract_latency_us(ts_in_delta);
        assert!(latency.is_some());
        assert!(latency.unwrap() < 10000); // Should be less than 10ms given timing

        // Test invalid input
        assert!(extract_latency_us(0).is_none());
        assert!(extract_latency_us(-1).is_none());
    }
}
