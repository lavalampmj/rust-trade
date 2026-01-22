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
use trading_common::data::types::TickData;

/// Provider error types
#[derive(Error, Debug)]
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
