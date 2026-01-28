//! Data venue types.
//!
//! Types for data streaming and historical data retrieval.

use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::data::orderbook::{OrderBook, OrderBookDelta};
use crate::data::quotes::QuoteTick;
use crate::data::types::TickData;
use crate::data::{BboMsg, MboMsg, Mbp10Msg, TradeMsg};

/// Data types supported by venues.
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

/// Historical data request.
#[derive(Debug, Clone)]
pub struct HistoricalRequest {
    /// Symbols to fetch (canonical format)
    pub symbols: Vec<String>,
    /// Start time (inclusive)
    pub start: DateTime<Utc>,
    /// End time (exclusive)
    pub end: DateTime<Utc>,
    /// Data type to fetch
    pub data_type: DataType,
    /// Venue-specific dataset (e.g., "GLBX.MDP3" for CME via Databento)
    pub dataset: Option<String>,
    /// Maximum number of records (optional limit)
    pub limit: Option<usize>,
}

impl HistoricalRequest {
    /// Create a new historical request for trades.
    pub fn trades(symbols: Vec<String>, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            symbols,
            start,
            end,
            data_type: DataType::Trades,
            dataset: None,
            limit: None,
        }
    }

    /// Create a new historical request for OHLC.
    pub fn ohlc(symbols: Vec<String>, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        Self {
            symbols,
            start,
            end,
            data_type: DataType::OHLC,
            dataset: None,
            limit: None,
        }
    }

    /// Set the dataset.
    pub fn with_dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }

    /// Set the limit.
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Live subscription request.
#[derive(Debug, Clone)]
pub struct LiveSubscription {
    /// Symbols to subscribe to (canonical format)
    pub symbols: Vec<String>,
    /// Data type to subscribe to
    pub data_type: DataType,
    /// Venue-specific dataset
    pub dataset: Option<String>,
}

impl LiveSubscription {
    /// Create a new live subscription for trades.
    pub fn trades(symbols: Vec<String>) -> Self {
        Self {
            symbols,
            data_type: DataType::Trades,
            dataset: None,
        }
    }

    /// Create a new live subscription for L1 quotes (BBO).
    pub fn quotes(symbols: Vec<String>) -> Self {
        Self {
            symbols,
            data_type: DataType::Quotes,
            dataset: None,
        }
    }

    /// Create a new live subscription for L2 order book.
    pub fn orderbook(symbols: Vec<String>) -> Self {
        Self {
            symbols,
            data_type: DataType::OrderBook,
            dataset: None,
        }
    }

    /// Set the dataset.
    pub fn with_dataset(mut self, dataset: String) -> Self {
        self.dataset = Some(dataset);
        self
    }
}

/// Connection status for live streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataConnectionStatus {
    Connected,
    Disconnected,
    Reconnecting,
}

/// Stream event callback for live data.
pub type StreamCallback = Arc<dyn Fn(StreamEvent) + Send + Sync>;

/// Events from a live data stream.
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
    Status(DataConnectionStatus),
    /// Error occurred
    Error(String),
}

/// DBN-native stream event callback for live data.
///
/// Use this for DBN-native pipelines that avoid intermediate type conversions.
pub type StreamCallbackDbn = Arc<dyn Fn(StreamEventDbn) + Send + Sync>;

/// DBN-native events from a live data stream.
///
/// This enum provides DBN-native variants for efficient processing without
/// intermediate type conversions.
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
    BookSnapshot(Mbp10Msg),
    /// L3 order book update (DBN MboMsg)
    BookUpdate(MboMsg),
    /// Connection status change
    Status(DataConnectionStatus),
    /// Error occurred during streaming
    Error(String),
}

impl StreamEventDbn {
    /// Check if this is a trade event.
    pub fn is_trade(&self) -> bool {
        matches!(
            self,
            StreamEventDbn::Trade(_) | StreamEventDbn::TradeBatch(_)
        )
    }

    /// Check if this is a quote event.
    pub fn is_quote(&self) -> bool {
        matches!(
            self,
            StreamEventDbn::Quote(_) | StreamEventDbn::QuoteBatch(_)
        )
    }

    /// Check if this is a book event.
    pub fn is_book(&self) -> bool {
        matches!(
            self,
            StreamEventDbn::BookSnapshot(_) | StreamEventDbn::BookUpdate(_)
        )
    }

    /// Check if this is a status or error event.
    pub fn is_control(&self) -> bool {
        matches!(self, StreamEventDbn::Status(_) | StreamEventDbn::Error(_))
    }

    /// Get the instrument_id if this is a data event.
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

    /// Convert to legacy StreamEvent using the provided symbol resolver.
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

/// Information about data availability.
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

/// Status of a live subscription.
#[derive(Debug, Clone)]
pub struct SubscriptionStatus {
    /// Currently subscribed symbols
    pub symbols: Vec<String>,
    /// Connection status
    pub connection: DataConnectionStatus,
    /// Number of messages received
    pub messages_received: u64,
    /// Last message timestamp
    pub last_message: Option<DateTime<Utc>>,
}

impl Default for SubscriptionStatus {
    fn default() -> Self {
        Self {
            symbols: vec![],
            connection: DataConnectionStatus::Disconnected,
            messages_received: 0,
            last_message: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_historical_request() {
        let start = Utc::now();
        let end = start + chrono::Duration::days(1);
        let req = HistoricalRequest::trades(vec!["BTCUSD".into()], start, end)
            .with_dataset("GLBX.MDP3".into())
            .with_limit(1000);

        assert_eq!(req.data_type, DataType::Trades);
        assert_eq!(req.dataset, Some("GLBX.MDP3".into()));
        assert_eq!(req.limit, Some(1000));
    }

    #[test]
    fn test_live_subscription() {
        let sub = LiveSubscription::quotes(vec!["BTCUSD".into(), "ETHUSD".into()]);
        assert_eq!(sub.data_type, DataType::Quotes);
        assert_eq!(sub.symbols.len(), 2);
    }

    #[test]
    fn test_stream_event_dbn() {
        let event = StreamEventDbn::Status(DataConnectionStatus::Connected);
        assert!(event.is_control());
        assert!(!event.is_trade());
        assert_eq!(event.instrument_id(), None);
    }
}
