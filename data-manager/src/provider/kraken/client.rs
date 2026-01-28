//! Kraken data provider implementation
//!
//! Implements the LiveStreamProvider trait for Kraken WebSocket streams.
//! Supports both Spot (V2) and Futures (V1) markets.

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

use crate::provider::{
    ConnectionStatus, DataProvider, DataType, LiveStreamProvider, LiveSubscription, ProviderError,
    ProviderInfo, ProviderResult, StreamCallback, StreamEvent, SubscriptionStatus,
};
use crate::symbol::SymbolSpec;

use super::normalizer::KrakenNormalizer;
use super::symbol::{get_exchange_name, to_kraken_futures, to_kraken_spot};
use super::types::{
    KrakenFuturesBookSnapshotMessage, KrakenFuturesBookUpdateMessage, KrakenFuturesResponse,
    KrakenFuturesSubscribeMessage, KrakenFuturesTickerMessage, KrakenFuturesTradeMessage,
    KrakenSpotBookMessage, KrakenSpotResponse, KrakenSpotSubscribeMessage, KrakenSpotTickerMessage,
    KrakenSpotTradeMessage,
};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Kraken Spot V2 WebSocket URL (production)
const KRAKEN_SPOT_WS_URL: &str = "wss://ws.kraken.com/v2";

/// Kraken Futures V1 WebSocket URL (production)
const KRAKEN_FUTURES_WS_URL: &str = "wss://futures.kraken.com/ws/v1";

/// Kraken Futures V1 WebSocket URL (demo/testnet)
const KRAKEN_FUTURES_DEMO_WS_URL: &str = "wss://demo-futures.kraken.com/ws/v1";

/// Initial reconnection delay
const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Maximum reconnection delay
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// Maximum reconnection attempts before giving up
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

// =============================================================================
// MARKET TYPE
// =============================================================================

/// Kraken market type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KrakenMarketType {
    /// Spot market (V2 WebSocket API)
    Spot,
    /// Futures market (V1 WebSocket API)
    Futures,
}

impl KrakenMarketType {
    /// Get a human-readable name for this market type
    pub fn as_str(&self) -> &'static str {
        match self {
            KrakenMarketType::Spot => "spot",
            KrakenMarketType::Futures => "futures",
        }
    }
}

// =============================================================================
// SETTINGS
// =============================================================================

/// Kraken provider settings
#[derive(Debug, Clone)]
pub struct KrakenSettings {
    /// Market type (Spot or Futures)
    pub market_type: KrakenMarketType,
    /// Use demo/testnet endpoints (Futures only - Spot has no public testnet)
    pub demo: bool,
    /// Maximum reconnection attempts per window
    pub rate_limit_attempts: u32,
    /// Rate limit window in seconds
    pub rate_limit_window_secs: u64,
}

impl Default for KrakenSettings {
    fn default() -> Self {
        Self {
            market_type: KrakenMarketType::Spot,
            demo: false,
            rate_limit_attempts: 5,
            rate_limit_window_secs: 60,
        }
    }
}

impl KrakenSettings {
    /// Create settings for Spot market
    pub fn spot() -> Self {
        Self {
            market_type: KrakenMarketType::Spot,
            ..Default::default()
        }
    }

    /// Create settings for Futures market
    pub fn futures(demo: bool) -> Self {
        Self {
            market_type: KrakenMarketType::Futures,
            demo,
            ..Default::default()
        }
    }
}

// =============================================================================
// PROVIDER
// =============================================================================

/// Kraken data provider
pub struct KrakenProvider {
    /// Provider information
    info: ProviderInfo,
    /// Market type
    market_type: KrakenMarketType,
    /// Use demo endpoints (Futures only)
    demo: bool,
    /// Connection status
    connected: Arc<AtomicBool>,
    /// Messages received count
    messages_received: Arc<AtomicU64>,
    /// Current subscription status
    subscription_status: SubscriptionStatus,
    /// Normalizer for converting messages
    normalizer: Arc<KrakenNormalizer>,
    /// Rate limiter for reconnections
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    /// Rate limiter max attempts (for logging)
    rate_limit_max_attempts: u32,
    /// Rate limit window duration
    rate_limit_window: Duration,
}

impl KrakenProvider {
    /// Create a new Kraken provider with default Spot settings
    pub fn new() -> Self {
        Self::with_settings(KrakenSettings::default())
    }

    /// Create a new Kraken Spot provider
    pub fn spot() -> Self {
        Self::with_settings(KrakenSettings::spot())
    }

    /// Create a new Kraken Futures provider
    pub fn futures(demo: bool) -> Self {
        Self::with_settings(KrakenSettings::futures(demo))
    }

    /// Create a new Kraken provider with custom settings
    pub fn with_settings(settings: KrakenSettings) -> Self {
        let (name, display_name) = match settings.market_type {
            KrakenMarketType::Spot => ("kraken", "Kraken Spot"),
            KrakenMarketType::Futures => {
                if settings.demo {
                    ("kraken_futures_demo", "Kraken Futures (Demo)")
                } else {
                    ("kraken_futures", "Kraken Futures")
                }
            }
        };

        let exchange = get_exchange_name(settings.market_type == KrakenMarketType::Futures);

        let info = ProviderInfo {
            name: name.to_string(),
            display_name: display_name.to_string(),
            version: "1.0".to_string(),
            supported_exchanges: vec![exchange.to_string()],
            supported_data_types: vec![DataType::Trades, DataType::Quotes, DataType::OrderBook],
            supports_historical: false,
            supports_live: true,
        };

        let normalizer = match settings.market_type {
            KrakenMarketType::Spot => KrakenNormalizer::spot(),
            KrakenMarketType::Futures => KrakenNormalizer::futures(),
        };

        let quota = Quota::per_minute(
            NonZeroU32::new(settings.rate_limit_attempts).expect("rate_limit_attempts must be > 0"),
        );

        Self {
            info,
            market_type: settings.market_type,
            demo: settings.demo,
            connected: Arc::new(AtomicBool::new(false)),
            messages_received: Arc::new(AtomicU64::new(0)),
            subscription_status: SubscriptionStatus::default(),
            normalizer: Arc::new(normalizer),
            rate_limiter: Arc::new(RateLimiter::direct(quota)),
            rate_limit_max_attempts: settings.rate_limit_attempts,
            rate_limit_window: Duration::from_secs(settings.rate_limit_window_secs),
        }
    }

    /// Get the WebSocket URL for this provider
    fn get_ws_url(&self) -> &'static str {
        match self.market_type {
            KrakenMarketType::Spot => KRAKEN_SPOT_WS_URL,
            KrakenMarketType::Futures => {
                if self.demo {
                    KRAKEN_FUTURES_DEMO_WS_URL
                } else {
                    KRAKEN_FUTURES_WS_URL
                }
            }
        }
    }

    /// Convert symbols to Kraken format
    fn convert_symbols(&self, symbols: &[SymbolSpec]) -> Result<Vec<String>, ProviderError> {
        symbols
            .iter()
            .map(|s| match self.market_type {
                KrakenMarketType::Spot => to_kraken_spot(&s.symbol),
                KrakenMarketType::Futures => to_kraken_futures(&s.symbol),
            })
            .collect()
    }

    /// Handle WebSocket connection with reconnection logic
    async fn handle_websocket_connection(
        &self,
        symbols: &[SymbolSpec],
        data_type: DataType,
        callback: StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        let kraken_symbols = self.convert_symbols(symbols)?;

        let data_type_str = match data_type {
            DataType::Trades => "trades",
            DataType::Quotes => "ticker",
            DataType::OrderBook => "book",
            DataType::OHLC => "ohlc",
        };

        info!(
            "Connecting to Kraken {} WebSocket for {} with {} symbols: {:?}",
            self.market_type.as_str(),
            data_type_str,
            kraken_symbols.len(),
            kraken_symbols
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
                .connect_and_subscribe(&kraken_symbols, data_type, &callback, shutdown_rx.resubscribe())
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
        kraken_symbols: &[String],
        data_type: DataType,
        callback: &StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        let ws_url = self.get_ws_url();

        // Establish WebSocket connection
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .map_err(|e| ProviderError::Connection(format!("Failed to connect to {}: {}", ws_url, e)))?;

        debug!("WebSocket connected to {}", ws_url);
        self.connected.store(true, Ordering::Release);
        callback(StreamEvent::Status(ConnectionStatus::Connected));

        let (mut write, mut read) = ws_stream.split();

        // Send subscription message based on market type and data type
        let subscribe_json = self.build_subscription_message(kraken_symbols, data_type)?;

        write
            .send(Message::Text(subscribe_json))
            .await
            .map_err(|e| {
                ProviderError::Connection(format!("Failed to send subscription: {}", e))
            })?;

        let data_type_str = match data_type {
            DataType::Trades => "trades",
            DataType::Quotes => "ticker",
            DataType::OrderBook => "book",
            DataType::OHLC => "ohlc",
        };

        info!(
            "Subscription sent for {} {} {} symbols",
            kraken_symbols.len(),
            self.market_type.as_str(),
            data_type_str
        );

        // Message processing loop
        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            self.process_message(&text, data_type, callback);
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

    /// Build subscription message based on market type and data type
    fn build_subscription_message(
        &self,
        kraken_symbols: &[String],
        data_type: DataType,
    ) -> ProviderResult<String> {
        match self.market_type {
            KrakenMarketType::Spot => match data_type {
                DataType::Trades => {
                    let msg = KrakenSpotSubscribeMessage::trades(kraken_symbols.to_vec());
                    serde_json::to_string(&msg).map_err(|e| {
                        ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
                    })
                }
                DataType::Quotes => {
                    let msg = KrakenSpotSubscribeMessage::ticker(kraken_symbols.to_vec());
                    serde_json::to_string(&msg).map_err(|e| {
                        ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
                    })
                }
                DataType::OrderBook => {
                    // Default depth of 10 levels
                    let msg = KrakenSpotSubscribeMessage::book(kraken_symbols.to_vec(), 10);
                    serde_json::to_string(&msg).map_err(|e| {
                        ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
                    })
                }
                DataType::OHLC => Err(ProviderError::DataNotAvailable(
                    "OHLC streaming not supported for Kraken Spot".to_string(),
                )),
            },
            KrakenMarketType::Futures => match data_type {
                DataType::Trades => {
                    let msg = KrakenFuturesSubscribeMessage::trades(kraken_symbols.to_vec());
                    serde_json::to_string(&msg).map_err(|e| {
                        ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
                    })
                }
                DataType::Quotes => {
                    let msg = KrakenFuturesSubscribeMessage::ticker(kraken_symbols.to_vec());
                    serde_json::to_string(&msg).map_err(|e| {
                        ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
                    })
                }
                DataType::OrderBook => {
                    let msg = KrakenFuturesSubscribeMessage::book(kraken_symbols.to_vec());
                    serde_json::to_string(&msg).map_err(|e| {
                        ProviderError::Internal(format!("Failed to serialize subscription: {}", e))
                    })
                }
                DataType::OHLC => Err(ProviderError::DataNotAvailable(
                    "OHLC streaming not supported for Kraken Futures".to_string(),
                )),
            },
        }
    }

    /// Process a WebSocket message
    fn process_message(&self, text: &str, data_type: DataType, callback: &StreamCallback) {
        match self.market_type {
            KrakenMarketType::Spot => self.process_spot_message(text, data_type, callback),
            KrakenMarketType::Futures => self.process_futures_message(text, data_type, callback),
        }
    }

    /// Process a Kraken Spot V2 message
    fn process_spot_message(&self, text: &str, data_type: DataType, callback: &StreamCallback) {
        match data_type {
            DataType::Trades => self.process_spot_trade_message(text, callback),
            DataType::Quotes => self.process_spot_ticker_message(text, callback),
            DataType::OrderBook => self.process_spot_book_message(text, callback),
            _ => {}
        }

        // Also check for subscription responses
        if let Ok(response) = serde_json::from_str::<KrakenSpotResponse>(text) {
            if let Some(success) = response.success {
                if success {
                    debug!("Spot subscription confirmed");
                } else if let Some(error) = response.error {
                    warn!("Spot subscription error: {}", error);
                    callback(StreamEvent::Error(error));
                }
            }
        }
    }

    /// Process a Kraken Spot trade message
    fn process_spot_trade_message(&self, text: &str, callback: &StreamCallback) {
        if let Ok(trade_msg) = serde_json::from_str::<KrakenSpotTradeMessage>(text) {
            if trade_msg.channel == "trade" && trade_msg.msg_type == "update" {
                for trade in trade_msg.data {
                    match self.normalizer.normalize_spot(trade) {
                        Ok(tick) => {
                            self.messages_received.fetch_add(1, Ordering::Relaxed);
                            callback(StreamEvent::Tick(tick));
                        }
                        Err(e) => {
                            warn!("Spot trade normalization error: {}", e);
                            callback(StreamEvent::Error(e.to_string()));
                        }
                    }
                }
            }
        }
    }

    /// Process a Kraken Spot ticker message (L1)
    fn process_spot_ticker_message(&self, text: &str, callback: &StreamCallback) {
        if let Ok(ticker_msg) = serde_json::from_str::<KrakenSpotTickerMessage>(text) {
            if ticker_msg.channel == "ticker" {
                for ticker in ticker_msg.data {
                    match self.normalizer.normalize_spot_ticker(ticker, None) {
                        Ok(quote) => {
                            self.messages_received.fetch_add(1, Ordering::Relaxed);
                            callback(StreamEvent::Quote(quote));
                        }
                        Err(e) => {
                            warn!("Spot ticker normalization error: {}", e);
                            callback(StreamEvent::Error(e.to_string()));
                        }
                    }
                }
            }
        }
    }

    /// Process a Kraken Spot book message (L2)
    fn process_spot_book_message(&self, text: &str, callback: &StreamCallback) {
        if let Ok(book_msg) = serde_json::from_str::<KrakenSpotBookMessage>(text) {
            if book_msg.channel == "book" {
                for book_data in book_msg.data {
                    if book_msg.msg_type == "snapshot" {
                        // Full snapshot
                        match self.normalizer.normalize_spot_book_snapshot(book_data) {
                            Ok(order_book) => {
                                self.messages_received.fetch_add(1, Ordering::Relaxed);
                                callback(StreamEvent::OrderBookSnapshot(order_book));
                            }
                            Err(e) => {
                                warn!("Spot book snapshot normalization error: {}", e);
                                callback(StreamEvent::Error(e.to_string()));
                            }
                        }
                    } else {
                        // Incremental update
                        match self.normalizer.normalize_spot_book_update(&book_data) {
                            Ok(deltas) => {
                                for delta in deltas {
                                    self.messages_received.fetch_add(1, Ordering::Relaxed);
                                    callback(StreamEvent::OrderBookUpdate(delta));
                                }
                            }
                            Err(e) => {
                                warn!("Spot book update normalization error: {}", e);
                                callback(StreamEvent::Error(e.to_string()));
                            }
                        }
                    }
                }
            }
        }
    }

    /// Process a Kraken Futures V1 message
    fn process_futures_message(&self, text: &str, data_type: DataType, callback: &StreamCallback) {
        match data_type {
            DataType::Trades => self.process_futures_trade_message(text, callback),
            DataType::Quotes => self.process_futures_ticker_message(text, callback),
            DataType::OrderBook => self.process_futures_book_message(text, callback),
            _ => {}
        }

        // Check for subscription responses
        if let Ok(response) = serde_json::from_str::<KrakenFuturesResponse>(text) {
            if let Some(event) = &response.event {
                match event.as_str() {
                    "subscribed" => {
                        debug!("Futures subscription confirmed: {:?}", response.product_ids);
                    }
                    "error" => {
                        if let Some(error) = &response.error {
                            warn!("Futures subscription error: {}", error);
                            callback(StreamEvent::Error(error.clone()));
                        }
                    }
                    "info" => {
                        if let Some(msg) = &response.message {
                            debug!("Futures info: {}", msg);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Check for heartbeat
        if text.contains("\"feed\":\"heartbeat\"") {
            debug!("Futures heartbeat received");
        }
    }

    /// Process a Kraken Futures trade message
    fn process_futures_trade_message(&self, text: &str, callback: &StreamCallback) {
        if let Ok(trade_msg) = serde_json::from_str::<KrakenFuturesTradeMessage>(text) {
            if trade_msg.feed == "trade" {
                match self.normalizer.normalize_futures(trade_msg) {
                    Ok(tick) => {
                        self.messages_received.fetch_add(1, Ordering::Relaxed);
                        callback(StreamEvent::Tick(tick));
                    }
                    Err(e) => {
                        warn!("Futures trade normalization error: {}", e);
                        callback(StreamEvent::Error(e.to_string()));
                    }
                }
            }
        }
    }

    /// Process a Kraken Futures ticker message (L1)
    fn process_futures_ticker_message(&self, text: &str, callback: &StreamCallback) {
        if let Ok(ticker_msg) = serde_json::from_str::<KrakenFuturesTickerMessage>(text) {
            if ticker_msg.feed == "ticker" {
                match self.normalizer.normalize_futures_ticker(ticker_msg) {
                    Ok(quote) => {
                        self.messages_received.fetch_add(1, Ordering::Relaxed);
                        callback(StreamEvent::Quote(quote));
                    }
                    Err(e) => {
                        warn!("Futures ticker normalization error: {}", e);
                        callback(StreamEvent::Error(e.to_string()));
                    }
                }
            }
        }
    }

    /// Process a Kraken Futures book message (L2)
    fn process_futures_book_message(&self, text: &str, callback: &StreamCallback) {
        // Try to parse as book snapshot
        if let Ok(snapshot) = serde_json::from_str::<KrakenFuturesBookSnapshotMessage>(text) {
            if snapshot.feed == "book_snapshot" {
                match self.normalizer.normalize_futures_book_snapshot(snapshot) {
                    Ok(order_book) => {
                        self.messages_received.fetch_add(1, Ordering::Relaxed);
                        callback(StreamEvent::OrderBookSnapshot(order_book));
                    }
                    Err(e) => {
                        warn!("Futures book snapshot normalization error: {}", e);
                        callback(StreamEvent::Error(e.to_string()));
                    }
                }
                return;
            }
        }

        // Try to parse as book update
        if let Ok(update) = serde_json::from_str::<KrakenFuturesBookUpdateMessage>(text) {
            if update.feed == "book" {
                match self.normalizer.normalize_futures_book_update(update) {
                    Ok(delta) => {
                        self.messages_received.fetch_add(1, Ordering::Relaxed);
                        callback(StreamEvent::OrderBookUpdate(delta));
                    }
                    Err(e) => {
                        warn!("Futures book update normalization error: {}", e);
                        callback(StreamEvent::Error(e.to_string()));
                    }
                }
            }
        }
    }
}

impl Default for KrakenProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DataProvider for KrakenProvider {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    async fn connect(&mut self) -> ProviderResult<()> {
        // Connection happens lazily during subscribe
        info!(
            "Kraken {} provider initialized (demo: {})",
            self.market_type.as_str(),
            self.demo
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> ProviderResult<()> {
        self.connected.store(false, Ordering::Release);
        self.subscription_status.connection = ConnectionStatus::Disconnected;
        info!("Kraken {} provider disconnected", self.market_type.as_str());
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    async fn discover_symbols(&self, _exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>> {
        let exchange = get_exchange_name(self.market_type == KrakenMarketType::Futures);

        // Return common Kraken trading pairs
        match self.market_type {
            KrakenMarketType::Spot => Ok(vec![
                SymbolSpec::new("XBT/USD", exchange),
                SymbolSpec::new("ETH/USD", exchange),
                SymbolSpec::new("SOL/USD", exchange),
                SymbolSpec::new("XRP/USD", exchange),
                SymbolSpec::new("ADA/USD", exchange),
                SymbolSpec::new("DOGE/USD", exchange),
                SymbolSpec::new("DOT/USD", exchange),
                SymbolSpec::new("AVAX/USD", exchange),
                SymbolSpec::new("MATIC/USD", exchange),
                SymbolSpec::new("LINK/USD", exchange),
            ]),
            KrakenMarketType::Futures => Ok(vec![
                SymbolSpec::new("PI_XBTUSD", exchange),
                SymbolSpec::new("PI_ETHUSD", exchange),
                SymbolSpec::new("PI_SOLUSD", exchange),
                SymbolSpec::new("PI_XRPUSD", exchange),
                SymbolSpec::new("PI_ADAUSD", exchange),
                SymbolSpec::new("PI_DOGEUSD", exchange),
                SymbolSpec::new("PI_DOTUSD", exchange),
                SymbolSpec::new("PI_AVAXUSD", exchange),
                SymbolSpec::new("PI_MATICUSD", exchange),
                SymbolSpec::new("PI_LINKUSD", exchange),
            ]),
        }
    }
}

#[async_trait]
impl LiveStreamProvider for KrakenProvider {
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

        let data_type_str = match subscription.data_type {
            DataType::Trades => "trades",
            DataType::Quotes => "ticker",
            DataType::OrderBook => "order book",
            DataType::OHLC => "ohlc",
        };

        info!(
            "Starting Kraken {} {} subscription for symbols: {:?}",
            self.market_type.as_str(),
            data_type_str,
            subscription.symbols
        );

        self.subscription_status.symbols = subscription.symbols.clone();
        self.subscription_status.connection = ConnectionStatus::Disconnected;

        // Start the WebSocket connection (runs until shutdown)
        self.handle_websocket_connection(
            &subscription.symbols,
            subscription.data_type,
            callback,
            shutdown_rx,
        )
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
    fn test_provider_creation_spot() {
        let provider = KrakenProvider::spot();
        assert_eq!(provider.info().name, "kraken");
        assert!(!provider.is_connected());
        assert_eq!(provider.market_type, KrakenMarketType::Spot);
    }

    #[test]
    fn test_provider_creation_futures() {
        let provider = KrakenProvider::futures(false);
        assert_eq!(provider.info().name, "kraken_futures");
        assert!(!provider.is_connected());
        assert_eq!(provider.market_type, KrakenMarketType::Futures);
    }

    #[test]
    fn test_provider_creation_futures_demo() {
        let provider = KrakenProvider::futures(true);
        assert_eq!(provider.info().name, "kraken_futures_demo");
        assert!(provider.demo);
    }

    #[test]
    fn test_ws_url_spot() {
        let provider = KrakenProvider::spot();
        assert_eq!(provider.get_ws_url(), KRAKEN_SPOT_WS_URL);
    }

    #[test]
    fn test_ws_url_futures() {
        let provider = KrakenProvider::futures(false);
        assert_eq!(provider.get_ws_url(), KRAKEN_FUTURES_WS_URL);
    }

    #[test]
    fn test_ws_url_futures_demo() {
        let provider = KrakenProvider::futures(true);
        assert_eq!(provider.get_ws_url(), KRAKEN_FUTURES_DEMO_WS_URL);
    }

    #[test]
    fn test_convert_symbols_spot() {
        let provider = KrakenProvider::spot();
        let symbols = vec![
            SymbolSpec::new("BTCUSD", "KRAKEN"),
            SymbolSpec::new("ETHUSD", "KRAKEN"),
        ];
        let kraken_symbols = provider.convert_symbols(&symbols).unwrap();
        // V2 WebSocket API uses standard BTC, not XBT
        assert_eq!(kraken_symbols, vec!["BTC/USD", "ETH/USD"]);
    }

    #[test]
    fn test_convert_symbols_futures() {
        let provider = KrakenProvider::futures(false);
        let symbols = vec![
            SymbolSpec::new("BTCUSD", "KRAKEN_FUTURES"),
            SymbolSpec::new("ETHUSD", "KRAKEN_FUTURES"),
        ];
        let kraken_symbols = provider.convert_symbols(&symbols).unwrap();
        assert_eq!(kraken_symbols, vec!["PI_XBTUSD", "PI_ETHUSD"]);
    }

    #[tokio::test]
    async fn test_discover_symbols_spot() {
        let provider = KrakenProvider::spot();
        let symbols = provider.discover_symbols(None).await.unwrap();
        assert!(!symbols.is_empty());
        // Discover returns raw Kraken symbols which may use XBT
        // The conversion to BTC happens in to_kraken_spot()
        assert!(symbols.iter().any(|s| s.symbol == "XBT/USD" || s.symbol == "BTC/USD"));
    }

    #[tokio::test]
    async fn test_discover_symbols_futures() {
        let provider = KrakenProvider::futures(false);
        let symbols = provider.discover_symbols(None).await.unwrap();
        assert!(!symbols.is_empty());
        assert!(symbols.iter().any(|s| s.symbol == "PI_XBTUSD"));
    }

    #[test]
    fn test_custom_settings() {
        let settings = KrakenSettings {
            market_type: KrakenMarketType::Futures,
            demo: true,
            rate_limit_attempts: 10,
            rate_limit_window_secs: 120,
        };

        let provider = KrakenProvider::with_settings(settings);
        assert_eq!(provider.market_type, KrakenMarketType::Futures);
        assert!(provider.demo);
        assert_eq!(provider.rate_limit_max_attempts, 10);
    }
}
