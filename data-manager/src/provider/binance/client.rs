//! Binance data provider implementation
//!
//! Implements the LiveStreamProvider trait for Binance WebSocket streams.
//!
//! # Instrument Registration
//!
//! When an `InstrumentRegistry` is provided, the provider will automatically
//! register all subscribed symbols and use persistent instrument_ids in the
//! normalized data.

use async_trait::async_trait;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::instruments::InstrumentRegistry;
use crate::provider::{
    ConnectionStatus, DataProvider, DataType, LiveStreamProvider, LiveSubscription, ProviderError,
    ProviderResult, StreamCallback, StreamEvent, SubscriptionStatus, VenueCapabilities,
    VenueConnection, VenueConnectionStatus, VenueInfo,
};
use crate::symbol::SymbolSpec;

use super::normalizer::BinanceNormalizer;
use super::symbol::to_binance;
use super::types::{BinanceStreamMessage, BinanceSubscribeMessage, BinanceTradeMessage};

/// Default Binance US WebSocket URL
const DEFAULT_WS_URL: &str = "wss://stream.binance.us:9443/stream";

/// Initial reconnection delay
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Maximum reconnection delay
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// Maximum reconnection attempts before giving up
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Binance provider settings
#[derive(Debug, Clone)]
pub struct BinanceSettings {
    /// WebSocket URL
    pub ws_url: String,
    /// Maximum reconnection attempts per window
    pub rate_limit_attempts: u32,
    /// Rate limit window in seconds
    pub rate_limit_window_secs: u64,
}

impl Default for BinanceSettings {
    fn default() -> Self {
        Self {
            ws_url: DEFAULT_WS_URL.to_string(),
            rate_limit_attempts: 5,
            rate_limit_window_secs: 60,
        }
    }
}

/// Binance data provider
pub struct BinanceProvider {
    /// Venue information
    info: VenueInfo,
    /// WebSocket URL
    ws_url: String,
    /// Connection status
    connected: Arc<AtomicBool>,
    /// Messages received count
    messages_received: Arc<AtomicU64>,
    /// Current subscription status
    subscription_status: SubscriptionStatus,
    /// Normalizer for converting messages
    normalizer: Arc<BinanceNormalizer>,
    /// Rate limiter for reconnections
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// Rate limiter max attempts (for logging)
    rate_limit_max_attempts: u32,
    /// Rate limit window duration
    rate_limit_window: Duration,
    /// Optional instrument registry for persistent IDs
    registry: Option<Arc<InstrumentRegistry>>,
}

impl BinanceProvider {
    /// Create a new Binance provider with default settings
    pub fn new() -> Self {
        Self::with_settings(BinanceSettings::default())
    }

    /// Create a new Binance provider with custom settings
    pub fn with_settings(settings: BinanceSettings) -> Self {
        let info = VenueInfo::data_provider("binance", "Binance")
            .with_version("1.0")
            .with_exchanges(vec!["BINANCE".to_string()])
            .with_capabilities(VenueCapabilities {
                supports_live_streaming: true,
                supports_historical: false,
                supports_quotes: false, // Only trades for now
                supports_orderbook: false,
                ..VenueCapabilities::default()
            });

        let quota = Quota::per_minute(
            NonZeroU32::new(settings.rate_limit_attempts).expect("rate_limit_attempts must be > 0"),
        );

        Self {
            info,
            ws_url: settings.ws_url,
            connected: Arc::new(AtomicBool::new(false)),
            messages_received: Arc::new(AtomicU64::new(0)),
            subscription_status: SubscriptionStatus::default(),
            normalizer: Arc::new(BinanceNormalizer::new()),
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            rate_limit_max_attempts: settings.rate_limit_attempts,
            rate_limit_window: Duration::from_secs(settings.rate_limit_window_secs),
            registry: None,
        }
    }

    /// Create a new Binance provider with an instrument registry
    ///
    /// When a registry is provided, all subscribed symbols will be registered
    /// and assigned persistent instrument_ids.
    pub fn with_registry(settings: BinanceSettings, registry: Arc<InstrumentRegistry>) -> Self {
        let mut provider = Self::with_settings(settings);
        provider.registry = Some(registry.clone());
        provider.normalizer = Arc::new(BinanceNormalizer::with_registry(registry));
        provider
    }

    /// Set the instrument registry
    ///
    /// Call this before subscribing to enable persistent instrument_ids.
    pub fn set_registry(&mut self, registry: Arc<InstrumentRegistry>) {
        self.registry = Some(registry.clone());
        self.normalizer = Arc::new(BinanceNormalizer::with_registry(registry));
    }

    /// Parse WebSocket message and extract trade data
    fn parse_trade_message(&self, text: &str) -> Result<BinanceTradeMessage, ProviderError> {
        // First try to parse as stream message (combined streams format)
        if let Ok(stream_msg) = serde_json::from_str::<BinanceStreamMessage>(text) {
            return Ok(stream_msg.data);
        }

        // Fallback: try to parse as direct trade message
        if let Ok(trade_msg) = serde_json::from_str::<BinanceTradeMessage>(text) {
            return Ok(trade_msg);
        }

        // Check if it's a subscription confirmation or other control message
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            if value.get("result").is_some() || value.get("id").is_some() {
                debug!("Received subscription confirmation: {}", text);
                return Err(ProviderError::Internal(
                    "Control message, not trade data".to_string(),
                ));
            }
        }

        Err(ProviderError::Parse(format!(
            "Unable to parse message: {}",
            text
        )))
    }

    /// Build WebSocket subscription streams for Binance
    ///
    /// Converts canonical symbols (e.g., `BTCUSD`) to Binance format (e.g., `btcusdt@trade`)
    fn build_trade_streams(symbols: &[String]) -> Result<Vec<String>, ProviderError> {
        if symbols.is_empty() {
            return Err(ProviderError::Configuration(
                "No symbols provided".to_string(),
            ));
        }

        let mut streams = Vec::with_capacity(symbols.len());

        for symbol in symbols {
            // Convert canonical symbol to Binance format
            let binance_symbol = to_binance(symbol)?;
            streams.push(format!("{}@trade", binance_symbol.to_lowercase()));
        }

        Ok(streams)
    }

    /// Handle WebSocket connection with reconnection logic
    async fn handle_websocket_connection(
        &self,
        symbols: &[SymbolSpec],
        callback: StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        let symbol_names: Vec<String> = symbols.iter().map(|s| s.symbol.clone()).collect();
        let streams = Self::build_trade_streams(&symbol_names)?;

        info!(
            "Connecting to Binance WebSocket with {} streams",
            streams.len()
        );

        let mut reconnect_attempts = 0;
        let mut current_delay = INITIAL_RECONNECT_DELAY;

        loop {
            // Check for shutdown signal before each connection attempt
            if shutdown_rx.try_recv().is_ok() {
                info!("Shutdown signal received, stopping WebSocket connection attempts");
                return Ok(());
            }

            match self
                .connect_and_subscribe(&streams, &callback, shutdown_rx.resubscribe())
                .await
            {
                Ok(()) => {
                    info!("WebSocket connection ended normally");
                    return Ok(());
                }
                Err(e) => {
                    reconnect_attempts += 1;
                    self.connected.store(false, Ordering::Release);
                    callback(StreamEvent::Status(ConnectionStatus::Reconnecting));

                    error!(
                        "WebSocket connection failed (attempt {}/{}): {}",
                        reconnect_attempts, MAX_RECONNECT_ATTEMPTS, e
                    );

                    if reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                        callback(StreamEvent::Error(format!(
                            "Max reconnection attempts ({}) exceeded",
                            MAX_RECONNECT_ATTEMPTS
                        )));
                        return Err(ProviderError::Connection(format!(
                            "Max reconnection attempts ({}) exceeded",
                            MAX_RECONNECT_ATTEMPTS
                        )));
                    }

                    // Check rate limiter before attempting reconnection
                    if self.rate_limiter.check().is_err() {
                        error!(
                            "Reconnection rate limit exceeded ({} attempts per window). Waiting {:?} before retry...",
                            self.rate_limit_max_attempts,
                            self.rate_limit_window
                        );

                        tokio::select! {
                            _ = sleep(self.rate_limit_window) => {
                                warn!("Rate limit window reset, will retry connection");
                            }
                            _ = shutdown_rx.recv() => {
                                info!("Shutdown signal received during rate limit wait");
                                return Ok(());
                            }
                        }
                    }

                    // Exponential backoff
                    warn!(
                        "Attempting to reconnect in {:?}... (attempt {}/{})",
                        current_delay, reconnect_attempts, MAX_RECONNECT_ATTEMPTS
                    );

                    tokio::select! {
                        _ = sleep(current_delay) => {
                            current_delay = std::cmp::min(current_delay * 2, MAX_RECONNECT_DELAY);
                            continue;
                        }
                        _ = shutdown_rx.recv() => {
                            info!("Shutdown signal received during reconnect delay");
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Connect to WebSocket and handle subscription
    async fn connect_and_subscribe(
        &self,
        streams: &[String],
        callback: &StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        // Establish WebSocket connection
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .map_err(|e| ProviderError::Connection(format!("Failed to connect: {}", e)))?;

        debug!("WebSocket connected to {}", self.ws_url);
        self.connected.store(true, Ordering::Release);
        callback(StreamEvent::Status(ConnectionStatus::Connected));

        let (mut write, mut read) = ws_stream.split();

        // Send subscription message
        let subscribe_msg = BinanceSubscribeMessage::new(streams.to_vec());
        let subscribe_json = serde_json::to_string(&subscribe_msg).map_err(|e| {
            ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
        })?;

        write
            .send(Message::Text(subscribe_json))
            .await
            .map_err(|e| {
                ProviderError::Connection(format!("Failed to send subscription: {}", e))
            })?;

        info!("Subscription sent for {} streams", streams.len());

        // Message processing loop
        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            match self.parse_trade_message(&text) {
                                Ok(trade_msg) => {
                                    match self.normalizer.normalize(trade_msg) {
                                        Ok(tick) => {
                                            self.messages_received.fetch_add(1, Ordering::Relaxed);
                                            callback(StreamEvent::Tick(tick));
                                        }
                                        Err(e) => {
                                            warn!("Normalization error: {}", e);
                                            callback(StreamEvent::Error(e.to_string()));
                                        }
                                    }
                                }
                                Err(ProviderError::Internal(_)) => {
                                    // Control message, ignore
                                }
                                Err(e) => {
                                    warn!("Parse error: {}", e);
                                }
                            }
                        }
                        Some(Ok(Message::Ping(ping))) => {
                            if let Err(e) = write.send(Message::Pong(ping)).await {
                                warn!("Failed to send pong: {}", e);
                            }
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("WebSocket closed by server");
                            self.connected.store(false, Ordering::Release);
                            callback(StreamEvent::Status(ConnectionStatus::Disconnected));
                            break;
                        }
                        Some(Err(e)) => {
                            self.connected.store(false, Ordering::Release);
                            callback(StreamEvent::Status(ConnectionStatus::Disconnected));
                            return Err(ProviderError::Connection(e.to_string()));
                        }
                        None => {
                            info!("WebSocket stream ended");
                            self.connected.store(false, Ordering::Release);
                            callback(StreamEvent::Status(ConnectionStatus::Disconnected));
                            break;
                        }
                        _ => continue,
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received, closing WebSocket gracefully");
                    self.connected.store(false, Ordering::Release);
                    callback(StreamEvent::Status(ConnectionStatus::Disconnected));

                    // Send Close frame to server
                    if let Err(e) = write.send(Message::Close(None)).await {
                        warn!("Failed to send close frame: {}", e);
                    }
                    break;
                }
            }
        }

        Ok(())
    }
}

impl Default for BinanceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
#[async_trait]
impl VenueConnection for BinanceProvider {
    fn info(&self) -> &VenueInfo {
        &self.info
    }

    async fn connect(&mut self) -> trading_common::venue::VenueResult<()> {
        // Connection happens lazily during subscribe
        // This just marks us as "ready to connect"
        info!("Binance provider initialized");
        Ok(())
    }

    async fn disconnect(&mut self) -> trading_common::venue::VenueResult<()> {
        self.connected.store(false, Ordering::Release);
        self.subscription_status.connection = ConnectionStatus::Disconnected;
        info!("Binance provider disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn connection_status(&self) -> VenueConnectionStatus {
        if self.connected.load(Ordering::Acquire) {
            VenueConnectionStatus::Connected
        } else {
            VenueConnectionStatus::Disconnected
        }
    }
}

#[async_trait]
impl DataProvider for BinanceProvider {
    async fn discover_symbols(&self, _exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>> {
        // Binance has many symbols - return common crypto pairs in canonical (DBT) format
        Ok(vec![
            SymbolSpec::new("BTCUSD", "BINANCE"),
            SymbolSpec::new("ETHUSD", "BINANCE"),
            SymbolSpec::new("SOLUSD", "BINANCE"),
            SymbolSpec::new("BNBUSD", "BINANCE"),
            SymbolSpec::new("XRPUSD", "BINANCE"),
            SymbolSpec::new("ADAUSD", "BINANCE"),
            SymbolSpec::new("DOGEUSD", "BINANCE"),
            SymbolSpec::new("AVAXUSD", "BINANCE"),
            SymbolSpec::new("DOTUSD", "BINANCE"),
            SymbolSpec::new("MATICUSD", "BINANCE"),
        ])
    }
}

#[async_trait]
impl LiveStreamProvider for BinanceProvider {
    async fn subscribe(
        &mut self,
        subscription: LiveSubscription,
        callback: StreamCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        if subscription.symbols.is_empty() {
            return Err(ProviderError::Subscription(
                "No symbols provided".to_string(),
            ));
        }

        info!(
            "Starting Binance trade subscription for symbols: {:?}",
            subscription.symbols
        );

        // Pre-register symbols with the instrument registry if available
        if self.normalizer.has_registry() {
            let symbol_names: Vec<String> =
                subscription.symbols.iter().map(|s| s.symbol.clone()).collect();

            match self.normalizer.register_symbols(&symbol_names).await {
                Ok(ids) => {
                    info!(
                        "Registered {} symbols with instrument registry",
                        ids.len()
                    );
                    for (symbol, id) in &ids {
                        debug!("  {} -> instrument_id {}", symbol, id);
                    }
                }
                Err(e) => {
                    warn!("Failed to register symbols with registry: {}", e);
                    // Continue anyway - normalizer will fall back to hash-based IDs
                }
            }
        }

        self.subscription_status.symbols = subscription.symbols.clone();
        self.subscription_status.connection = ConnectionStatus::Disconnected;

        // Start the WebSocket connection (runs until shutdown)
        self.handle_websocket_connection(&subscription.symbols, callback, shutdown_rx)
            .await
    }

    async fn unsubscribe(&mut self, symbols: &[SymbolSpec]) -> ProviderResult<()> {
        self.subscription_status
            .symbols
            .retain(|s| !symbols.contains(s));
        info!("Unsubscribed from {:?}", symbols);
        Ok(())
    }

    fn subscription_status(&self) -> SubscriptionStatus {
        SubscriptionStatus {
            symbols: self.subscription_status.symbols.clone(),
            connection: if self.connected.load(Ordering::Acquire) {
                ConnectionStatus::Connected
            } else {
                ConnectionStatus::Disconnected
            },
            messages_received: self.messages_received.load(Ordering::Relaxed),
            last_message: Some(Utc::now()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creation() {
        let provider = BinanceProvider::new();
        assert_eq!(provider.info().name, "binance");
        assert!(!provider.is_connected());
    }

    #[test]
    fn test_custom_settings() {
        let settings = BinanceSettings {
            ws_url: "wss://custom.url:9443/stream".to_string(),
            rate_limit_attempts: 10,
            rate_limit_window_secs: 120,
        };

        let provider = BinanceProvider::with_settings(settings);
        assert_eq!(provider.ws_url, "wss://custom.url:9443/stream");
        assert_eq!(provider.rate_limit_max_attempts, 10);
    }

    #[test]
    fn test_parse_control_message() {
        let provider = BinanceProvider::new();

        let control_msg = r#"{"result": null, "id": 1}"#;
        let result = provider.parse_trade_message(control_msg);
        assert!(matches!(result, Err(ProviderError::Internal(_))));
    }

    #[tokio::test]
    async fn test_discover_symbols() {
        let provider = BinanceProvider::new();
        let symbols = provider.discover_symbols(None).await.unwrap();
        assert!(!symbols.is_empty());
        // Should return canonical symbols (BTCUSD, not BTCUSDT)
        assert!(symbols.iter().any(|s| s.symbol == "BTCUSD"));
        assert!(!symbols.iter().any(|s| s.symbol == "BTCUSDT"));
    }

    #[test]
    fn test_build_trade_streams() {
        // Canonical symbols should be converted to Binance format for WebSocket
        let symbols = vec!["BTCUSD".to_string(), "ETHUSD".to_string()];
        let streams = BinanceProvider::build_trade_streams(&symbols).unwrap();

        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0], "btcusdt@trade");
        assert_eq!(streams[1], "ethusdt@trade");
    }

    #[test]
    fn test_build_trade_streams_passthrough() {
        // Already in Binance format should also work
        let symbols = vec!["BTCUSDT".to_string(), "ETHBUSD".to_string()];
        let streams = BinanceProvider::build_trade_streams(&symbols).unwrap();

        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0], "btcusdt@trade");
        assert_eq!(streams[1], "ethbusd@trade");
    }
}
