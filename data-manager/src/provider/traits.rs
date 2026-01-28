//! Provider trait definitions
//!
//! These traits define the interface for data providers. Each provider
//! (Databento, Polygon, etc.) implements these traits to provide
//! historical and live data.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::schema::NormalizedOHLC;
use crate::symbol::SymbolSpec;
use trading_common::data::orderbook::{OrderBook, OrderBookDelta};
use trading_common::data::quotes::QuoteTick;
use trading_common::data::types::TickData;

// DBN types for native streaming
use trading_common::data::{BboMsg, MboMsg, Mbp10Msg, TradeMsg};

use trading_common::error::{ErrorCategory, ErrorClassification};

/// Provider error types
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ProviderError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Request error: {0}")]
    Request(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Data not available: {0}")]
    DataNotAvailable(String),

    #[error("Provider not connected")]
    NotConnected,

    #[error("Subscription error: {0}")]
    Subscription(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Configuration error: {0}")]
    Configuration(String),
}

impl ErrorClassification for ProviderError {
    fn category(&self) -> ErrorCategory {
        match self {
            ProviderError::Connection(_) => ErrorCategory::Transient,
            ProviderError::Authentication(_) => ErrorCategory::Configuration,
            ProviderError::Request(_) => ErrorCategory::Transient,
            ProviderError::Parse(_) => ErrorCategory::Permanent,
            ProviderError::RateLimit(_) => ErrorCategory::ResourceExhausted,
            ProviderError::SymbolNotFound(_) => ErrorCategory::Permanent,
            ProviderError::DataNotAvailable(_) => ErrorCategory::Permanent,
            ProviderError::NotConnected => ErrorCategory::Transient,
            ProviderError::Subscription(_) => ErrorCategory::Transient,
            ProviderError::Internal(_) => ErrorCategory::Internal,
            ProviderError::Configuration(_) => ErrorCategory::Configuration,
        }
    }

    fn suggested_retry_delay(&self) -> Option<std::time::Duration> {
        match self {
            ProviderError::Connection(_) => Some(std::time::Duration::from_secs(2)),
            ProviderError::Request(_) => Some(std::time::Duration::from_millis(500)),
            ProviderError::RateLimit(_) => Some(std::time::Duration::from_secs(60)),
            ProviderError::NotConnected => Some(std::time::Duration::from_secs(1)),
            ProviderError::Subscription(_) => Some(std::time::Duration::from_millis(500)),
            _ => None,
        }
    }
}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// Information about a data provider
#[derive(Debug, Clone)]
pub struct ProviderInfo {
    /// Provider name (e.g., "databento", "polygon")
    pub name: String,
    /// Provider display name
    pub display_name: String,
    /// Provider version
    pub version: String,
    /// Supported exchanges
    pub supported_exchanges: Vec<String>,
    /// Supported data types
    pub supported_data_types: Vec<DataType>,
    /// Whether historical data is available
    pub supports_historical: bool,
    /// Whether live streaming is available
    pub supports_live: bool,
}

/// Data types supported by providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    /// Trade/tick data
    Trades,
    /// Level 1 quotes (BBO)
    Quotes,
    /// Level 2 order book
    OrderBook,
    /// OHLC bars
    OHLC,
}

/// Historical data request
#[derive(Debug, Clone)]
pub struct HistoricalRequest {
    /// Symbols to fetch
    pub symbols: Vec<SymbolSpec>,
    /// Start time (inclusive)
    pub start: DateTime<Utc>,
    /// End time (exclusive)
    pub end: DateTime<Utc>,
    /// Data type to fetch
    pub data_type: DataType,
    /// Provider-specific dataset (e.g., "GLBX.MDP3" for CME via Databento)
    pub dataset: Option<String>,
    /// Maximum number of records (optional limit)
    pub limit: Option<usize>,
}

impl HistoricalRequest {
    /// Create a new historical request for trades
    pub fn trades(symbols: Vec<SymbolSpec>, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            symbols,
            start,
            end,
            data_type: DataType::Trades,
            dataset: None,
            limit: None,
        }
    }

    /// Create a new historical request for OHLC
    pub fn ohlc(symbols: Vec<SymbolSpec>, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            symbols,
            start,
            end,
            data_type: DataType::OHLC,
            dataset: None,
            limit: None,
        }
    }

    /// Set the dataset
    pub fn with_dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }

    /// Set the limit
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Live subscription request
#[derive(Debug, Clone)]
pub struct LiveSubscription {
    /// Symbols to subscribe to
    pub symbols: Vec<SymbolSpec>,
    /// Data type to subscribe to
    pub data_type: DataType,
    /// Provider-specific dataset
    pub dataset: Option<String>,
}

impl LiveSubscription {
    /// Create a new live subscription for trades
    pub fn trades(symbols: Vec<SymbolSpec>) -> Self {
        Self {
            symbols,
            data_type: DataType::Trades,
            dataset: None,
        }
    }

    /// Create a new live subscription for L1 quotes (BBO)
    pub fn quotes(symbols: Vec<SymbolSpec>) -> Self {
        Self {
            symbols,
            data_type: DataType::Quotes,
            dataset: None,
        }
    }

    /// Create a new live subscription for L2 order book
    pub fn orderbook(symbols: Vec<SymbolSpec>) -> Self {
        Self {
            symbols,
            data_type: DataType::OrderBook,
            dataset: None,
        }
    }

    /// Set the dataset
    pub fn with_dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }
}

/// Stream event callback for live data
pub type StreamCallback = Arc<dyn Fn(StreamEvent) + Send + Sync>;

/// Events from a live data stream
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// New tick/trade data
    Tick(TickData),
    /// Batch of ticks
    TickBatch(Vec<TickData>),
    /// L1 quote (best bid/ask)
    Quote(QuoteTick),
    /// Batch of L1 quotes
    QuoteBatch(Vec<QuoteTick>),
    /// L2 order book snapshot
    OrderBookSnapshot(OrderBook),
    /// L2 order book delta (incremental update)
    OrderBookUpdate(OrderBookDelta),
    /// Connection status change
    Status(ConnectionStatus),
    /// Error occurred
    Error(String),
}

/// Connection status for live streams
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Reconnecting,
}

// =============================================================================
// DBN-Native Stream Events
// =============================================================================

/// DBN-native stream event callback for live data
///
/// Use this for DBN-native pipelines that avoid intermediate type conversions.
pub type StreamCallbackDbn = Arc<dyn Fn(StreamEventDbn) + Send + Sync>;

/// DBN-native events from a live data stream
///
/// This enum provides DBN-native variants for efficient processing without
/// intermediate type conversions. Use `StreamEventDbn` for new code when
/// DBN compatibility is desired.
///
/// # Type Mapping
///
/// | Variant | DBN Type | Description |
/// |---------|----------|-------------|
/// | `Trade` | `TradeMsg` | Individual trade execution |
/// | `TradeBatch` | `Vec<TradeMsg>` | Batch of trades |
/// | `Quote` | `BboMsg` | Best bid/offer (L1) |
/// | `QuoteBatch` | `Vec<BboMsg>` | Batch of quotes |
/// | `BookSnapshot` | `Mbp10Msg` | 10-level order book snapshot |
/// | `BookUpdate` | `MboMsg` | L3 order book update |
/// | `Status` | `ConnectionStatus` | Connection status change |
/// | `Error` | `String` | Error message |
#[derive(Debug, Clone)]
pub enum StreamEventDbn {
    /// Individual trade execution (DBN TradeMsg)
    Trade(TradeMsg),

    /// Batch of trades for efficient processing
    TradeBatch(Vec<TradeMsg>),

    /// Best bid/offer quote (DBN BboMsg, L1)
    Quote(BboMsg),

    /// Batch of BBO quotes
    QuoteBatch(Vec<BboMsg>),

    /// 10-level order book snapshot (DBN Mbp10Msg, L2)
    ///
    /// Note: This uses Mbp10Msg for snapshots which provides 10 levels.
    /// For different depth requirements, convert from the raw levels.
    BookSnapshot(Mbp10Msg),

    /// L3 order book update (DBN MboMsg)
    ///
    /// Individual order-level updates for maintaining order book state.
    BookUpdate(MboMsg),

    /// Connection status change
    Status(ConnectionStatus),

    /// Error occurred during streaming
    Error(String),
}

impl StreamEventDbn {
    /// Check if this is a trade event
    pub fn is_trade(&self) -> bool {
        matches!(self, StreamEventDbn::Trade(_) | StreamEventDbn::TradeBatch(_))
    }

    /// Check if this is a quote event
    pub fn is_quote(&self) -> bool {
        matches!(self, StreamEventDbn::Quote(_) | StreamEventDbn::QuoteBatch(_))
    }

    /// Check if this is a book event
    pub fn is_book(&self) -> bool {
        matches!(
            self,
            StreamEventDbn::BookSnapshot(_) | StreamEventDbn::BookUpdate(_)
        )
    }

    /// Check if this is a status or error event
    pub fn is_control(&self) -> bool {
        matches!(self, StreamEventDbn::Status(_) | StreamEventDbn::Error(_))
    }

    /// Get the instrument_id if this is a data event
    pub fn instrument_id(&self) -> Option<u32> {
        match self {
            StreamEventDbn::Trade(msg) => Some(msg.hd.instrument_id),
            StreamEventDbn::TradeBatch(msgs) => msgs.first().map(|m| m.hd.instrument_id),
            StreamEventDbn::Quote(msg) => Some(msg.hd.instrument_id),
            StreamEventDbn::QuoteBatch(msgs) => msgs.first().map(|m| m.hd.instrument_id),
            StreamEventDbn::BookSnapshot(msg) => Some(msg.hd.instrument_id),
            StreamEventDbn::BookUpdate(msg) => Some(msg.hd.instrument_id),
            StreamEventDbn::Status(_) | StreamEventDbn::Error(_) => None,
        }
    }
}

/// Convert StreamEventDbn to legacy StreamEvent
///
/// This allows gradual migration from TickData-based code to DBN-native code.
/// Requires an InstrumentRegistry to resolve instrument_id to symbol.
impl StreamEventDbn {
    /// Convert to legacy StreamEvent using the provided symbol resolver
    ///
    /// # Arguments
    /// * `symbol_resolver` - Function that resolves instrument_id to (symbol, exchange)
    ///
    /// Returns None for control events that don't need symbol resolution.
    pub fn to_stream_event<F>(&self, symbol_resolver: F) -> Option<StreamEvent>
    where
        F: Fn(u32) -> Option<(String, String)>,
    {
        match self {
            StreamEventDbn::Trade(msg) => {
                let (symbol, _exchange) = symbol_resolver(msg.hd.instrument_id)?;
                let tick = TickData::from_trade_msg(msg, symbol);
                Some(StreamEvent::Tick(tick))
            }
            StreamEventDbn::TradeBatch(msgs) => {
                let ticks: Vec<_> = msgs
                    .iter()
                    .filter_map(|msg| {
                        let (symbol, _exchange) = symbol_resolver(msg.hd.instrument_id)?;
                        Some(TickData::from_trade_msg(msg, symbol))
                    })
                    .collect();
                if ticks.is_empty() {
                    None
                } else {
                    Some(StreamEvent::TickBatch(ticks))
                }
            }
            StreamEventDbn::Status(status) => Some(StreamEvent::Status(*status)),
            StreamEventDbn::Error(msg) => Some(StreamEvent::Error(msg.clone())),
            // Quote and book conversions would require QuoteTick/OrderBook adapters
            _ => None,
        }
    }
}

/// Base trait for all data providers
#[async_trait]
pub trait DataProvider: Send + Sync {
    /// Get provider information
    fn info(&self) -> &ProviderInfo;

    /// Connect to the provider
    async fn connect(&mut self) -> ProviderResult<()>;

    /// Disconnect from the provider
    async fn disconnect(&mut self) -> ProviderResult<()>;

    /// Check if connected
    fn is_connected(&self) -> bool;

    /// Discover available symbols from the provider
    async fn discover_symbols(&self, exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>>;
}

/// Trait for historical data providers
#[async_trait]
pub trait HistoricalDataProvider: DataProvider {
    /// Fetch tick/trade data
    ///
    /// Returns a boxed iterator over results to support streaming large datasets.
    async fn fetch_ticks(
        &self,
        request: &HistoricalRequest,
    ) -> ProviderResult<Box<dyn Iterator<Item = ProviderResult<TickData>> + Send>>;

    /// Fetch OHLC bar data
    async fn fetch_ohlc(&self, request: &HistoricalRequest) -> ProviderResult<Vec<NormalizedOHLC>>;

    /// Check data availability for a symbol
    async fn check_availability(
        &self,
        symbol: &SymbolSpec,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> ProviderResult<DataAvailability>;
}

/// Information about data availability
#[derive(Debug, Clone)]
pub struct DataAvailability {
    /// Whether data is available
    pub available: bool,
    /// Actual start time of available data
    pub actual_start: Option<DateTime<Utc>>,
    /// Actual end time of available data
    pub actual_end: Option<DateTime<Utc>>,
    /// Estimated number of records
    pub estimated_records: Option<u64>,
}

/// Trait for live streaming providers
#[async_trait]
pub trait LiveStreamProvider: DataProvider {
    /// Subscribe to live data
    ///
    /// The callback will be invoked for each stream event.
    /// The shutdown receiver signals when to stop.
    async fn subscribe(
        &mut self,
        subscription: LiveSubscription,
        callback: StreamCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()>;

    /// Unsubscribe from symbols
    async fn unsubscribe(&mut self, symbols: &[SymbolSpec]) -> ProviderResult<()>;

    /// Get current subscription status
    fn subscription_status(&self) -> SubscriptionStatus;
}

/// Status of a live subscription
#[derive(Debug, Clone)]
pub struct SubscriptionStatus {
    /// Currently subscribed symbols
    pub symbols: Vec<SymbolSpec>,
    /// Connection status
    pub connection: ConnectionStatus,
    /// Number of messages received
    pub messages_received: u64,
    /// Last message timestamp
    pub last_message: Option<DateTime<Utc>>,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self {
            symbols: vec![],
            connection: ConnectionStatus::Disconnected,
            messages_received: 0,
            last_message: None,
        }
    }
}
