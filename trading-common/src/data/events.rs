//! Market data events for strategy event-driven processing.
//!
//! This module provides market data event types that can be used by strategies
//! to react to various market data updates including ticks, quotes, and order book changes.
//!
//! # DBN-Native Events
//!
//! For high-performance strategies, use `MarketDataEventDbn` which wraps native
//! DBN types (TradeMsg, BboMsg, Mbp10Msg, MboMsg) without intermediate conversions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::dbn_types::{BboMsg, MboMsg, Mbp10Msg, TradeMsg};
use super::orderbook::{OrderBook, OrderBookDelta};
use super::quotes::QuoteTick;
use super::types::TickData;
use crate::orders::InstrumentId;

/// Type of market data update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MarketDataType {
    /// Trade tick (last price)
    Last,
    /// Level 1 best bid/offer quote
    Quote,
    /// Full order book snapshot
    BookSnapshot,
    /// Incremental order book update
    BookDelta,
    /// Futures mark price
    MarkPrice,
    /// Index price
    IndexPrice,
    /// Perpetual funding rate
    FundingRate,
}

impl std::fmt::Display for MarketDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarketDataType::Last => write!(f, "LAST"),
            MarketDataType::Quote => write!(f, "QUOTE"),
            MarketDataType::BookSnapshot => write!(f, "BOOK_SNAPSHOT"),
            MarketDataType::BookDelta => write!(f, "BOOK_DELTA"),
            MarketDataType::MarkPrice => write!(f, "MARK_PRICE"),
            MarketDataType::IndexPrice => write!(f, "INDEX_PRICE"),
            MarketDataType::FundingRate => write!(f, "FUNDING_RATE"),
        }
    }
}

/// Market data event for strategy processing.
///
/// Contains one of several types of market data updates that a strategy
/// can subscribe to and react to. Not all fields will be populated -
/// check the `data_type` field to determine which optional fields are valid.
///
/// # Example
///
/// ```ignore
/// use trading_common::data::events::{MarketDataEvent, MarketDataType};
///
/// fn on_marketdata_update(&mut self, event: &MarketDataEvent) {
///     match event.data_type {
///         MarketDataType::Last => {
///             if let Some(tick) = &event.tick {
///                 println!("Last price: {}", tick.price);
///             }
///         }
///         MarketDataType::Quote => {
///             if let Some(quote) = &event.quote {
///                 println!("Spread: {}", quote.ask_price - quote.bid_price);
///             }
///         }
///         _ => {}
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataEvent {
    /// Type of market data update
    pub data_type: MarketDataType,

    /// Instrument this event applies to
    pub instrument_id: InstrumentId,

    /// Event timestamp (when the data was generated at venue)
    pub ts_event: DateTime<Utc>,

    /// Trade tick data (populated for Last type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick: Option<TickData>,

    /// Quote tick data (populated for Quote type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quote: Option<QuoteTick>,

    /// Full order book snapshot (populated for BookSnapshot type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_snapshot: Option<OrderBook>,

    /// Incremental order book update (populated for BookDelta type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_delta: Option<OrderBookDelta>,
}

impl MarketDataEvent {
    /// Create a new Last (trade tick) event
    pub fn last(instrument_id: InstrumentId, tick: TickData) -> Self {
        Self {
            data_type: MarketDataType::Last,
            instrument_id,
            ts_event: tick.timestamp,
            tick: Some(tick),
            quote: None,
            book_snapshot: None,
            book_delta: None,
        }
    }

    /// Create a new Quote event
    pub fn quote(instrument_id: InstrumentId, quote: QuoteTick) -> Self {
        Self {
            data_type: MarketDataType::Quote,
            instrument_id,
            ts_event: quote.ts_event,
            tick: None,
            quote: Some(quote),
            book_snapshot: None,
            book_delta: None,
        }
    }

    /// Create a new BookSnapshot event
    pub fn book_snapshot(
        instrument_id: InstrumentId,
        book: OrderBook,
        ts_event: DateTime<Utc>,
    ) -> Self {
        Self {
            data_type: MarketDataType::BookSnapshot,
            instrument_id,
            ts_event,
            tick: None,
            quote: None,
            book_snapshot: Some(book),
            book_delta: None,
        }
    }

    /// Create a new BookDelta event
    pub fn book_delta(
        instrument_id: InstrumentId,
        delta: OrderBookDelta,
        ts_event: DateTime<Utc>,
    ) -> Self {
        Self {
            data_type: MarketDataType::BookDelta,
            instrument_id,
            ts_event,
            tick: None,
            quote: None,
            book_snapshot: None,
            book_delta: Some(delta),
        }
    }

    /// Get the symbol from the instrument ID
    pub fn symbol(&self) -> &str {
        &self.instrument_id.symbol
    }
}

// =============================================================================
// DBN-Native Market Data Events
// =============================================================================

/// DBN-native market data event for high-performance strategy processing.
///
/// This enum provides direct access to DBN record types without intermediate
/// conversions, enabling maximum performance for strategies that need to process
/// large volumes of market data.
///
/// # Data Levels
///
/// | Variant | DBN Type | Level | Description |
/// |---------|----------|-------|-------------|
/// | `Trade` | `TradeMsg` | L0 | Individual trade executions |
/// | `Quote` | `BboMsg` | L1 | Best bid/offer quotes |
/// | `BookSnapshot` | `Mbp10Msg` | L2 | 10-level order book snapshot |
/// | `BookUpdate` | `MboMsg` | L3 | Individual order events |
///
/// # Example
///
/// ```ignore
/// use trading_common::data::events::MarketDataEventDbn;
/// use trading_common::data::{TradeMsgExt, BboMsgExt};
///
/// fn process_event(event: &MarketDataEventDbn) {
///     match event {
///         MarketDataEventDbn::Trade(msg) => {
///             println!("Trade: {} @ {}", msg.size_decimal(), msg.price_decimal());
///         }
///         MarketDataEventDbn::Quote(msg) => {
///             println!("Spread: {}", msg.spread());
///         }
///         _ => {}
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub enum MarketDataEventDbn {
    /// Trade execution (L0 - tick data)
    Trade(TradeMsg),

    /// Best bid/offer quote (L1)
    Quote(BboMsg),

    /// 10-level order book snapshot (L2)
    BookSnapshot(Mbp10Msg),

    /// Individual order event (L3)
    BookUpdate(MboMsg),
}

impl MarketDataEventDbn {
    /// Get the instrument_id from the underlying message
    pub fn instrument_id(&self) -> u32 {
        match self {
            MarketDataEventDbn::Trade(msg) => msg.hd.instrument_id,
            MarketDataEventDbn::Quote(msg) => msg.hd.instrument_id,
            MarketDataEventDbn::BookSnapshot(msg) => msg.hd.instrument_id,
            MarketDataEventDbn::BookUpdate(msg) => msg.hd.instrument_id,
        }
    }

    /// Get the event timestamp in nanoseconds
    pub fn ts_event(&self) -> u64 {
        match self {
            MarketDataEventDbn::Trade(msg) => msg.hd.ts_event,
            MarketDataEventDbn::Quote(msg) => msg.hd.ts_event,
            MarketDataEventDbn::BookSnapshot(msg) => msg.hd.ts_event,
            MarketDataEventDbn::BookUpdate(msg) => msg.hd.ts_event,
        }
    }

    /// Check if this is a trade event
    pub fn is_trade(&self) -> bool {
        matches!(self, MarketDataEventDbn::Trade(_))
    }

    /// Check if this is a quote event
    pub fn is_quote(&self) -> bool {
        matches!(self, MarketDataEventDbn::Quote(_))
    }

    /// Check if this is a book event (snapshot or update)
    pub fn is_book(&self) -> bool {
        matches!(
            self,
            MarketDataEventDbn::BookSnapshot(_) | MarketDataEventDbn::BookUpdate(_)
        )
    }

    /// Get the data level (0=trade, 1=L1 quote, 2=L2 book, 3=L3 order)
    pub fn level(&self) -> u8 {
        match self {
            MarketDataEventDbn::Trade(_) => 0,
            MarketDataEventDbn::Quote(_) => 1,
            MarketDataEventDbn::BookSnapshot(_) => 2,
            MarketDataEventDbn::BookUpdate(_) => 3,
        }
    }

    /// Create from TradeMsg
    pub fn trade(msg: TradeMsg) -> Self {
        MarketDataEventDbn::Trade(msg)
    }

    /// Create from BboMsg
    pub fn quote(msg: BboMsg) -> Self {
        MarketDataEventDbn::Quote(msg)
    }

    /// Create from Mbp10Msg
    pub fn book_snapshot(msg: Mbp10Msg) -> Self {
        MarketDataEventDbn::BookSnapshot(msg)
    }

    /// Create from MboMsg
    pub fn book_update(msg: MboMsg) -> Self {
        MarketDataEventDbn::BookUpdate(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::TradeSide;
    use rust_decimal_macros::dec;

    fn create_test_tick() -> TickData {
        TickData {
            symbol: "BTCUSDT".to_string(),
            timestamp: Utc::now(),
            ts_recv: Utc::now(),
            exchange: "BINANCE".to_string(),
            price: dec!(50000.0),
            quantity: dec!(1.0),
            side: TradeSide::Buy,
            provider: "BINANCE".to_string(),
            trade_id: "123".to_string(),
            is_buyer_maker: false,
            sequence: 1,
            raw_dbn: None,
        }
    }

    fn create_test_quote() -> QuoteTick {
        QuoteTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(49999.0),
            dec!(50001.0),
            dec!(1.0),
            dec!(1.0),
        )
    }

    #[test]
    fn test_market_data_type_display() {
        assert_eq!(MarketDataType::Last.to_string(), "LAST");
        assert_eq!(MarketDataType::Quote.to_string(), "QUOTE");
        assert_eq!(MarketDataType::BookSnapshot.to_string(), "BOOK_SNAPSHOT");
        assert_eq!(MarketDataType::BookDelta.to_string(), "BOOK_DELTA");
        assert_eq!(MarketDataType::MarkPrice.to_string(), "MARK_PRICE");
        assert_eq!(MarketDataType::IndexPrice.to_string(), "INDEX_PRICE");
        assert_eq!(MarketDataType::FundingRate.to_string(), "FUNDING_RATE");
    }

    #[test]
    fn test_market_data_event_last() {
        let tick = create_test_tick();
        let instrument_id = InstrumentId::new("BTCUSDT", "BINANCE");
        let event = MarketDataEvent::last(instrument_id.clone(), tick.clone());

        assert_eq!(event.data_type, MarketDataType::Last);
        assert_eq!(event.instrument_id, instrument_id);
        assert!(event.tick.is_some());
        assert!(event.quote.is_none());
        assert!(event.book_snapshot.is_none());
        assert!(event.book_delta.is_none());
        assert_eq!(event.symbol(), "BTCUSDT");
    }

    #[test]
    fn test_market_data_event_quote() {
        let quote = create_test_quote();
        let instrument_id = InstrumentId::new("BTCUSDT", "BINANCE");
        let event = MarketDataEvent::quote(instrument_id.clone(), quote);

        assert_eq!(event.data_type, MarketDataType::Quote);
        assert!(event.tick.is_none());
        assert!(event.quote.is_some());
        assert!(event.book_snapshot.is_none());
    }

    // ========================================================================
    // MarketDataEventDbn Tests
    // ========================================================================

    #[test]
    fn test_market_data_event_dbn_trade() {
        use crate::data::{create_trade_msg_from_decimals, TradeSideCompat};

        let trade = create_trade_msg_from_decimals(
            1_000_000_000, // ts_event: 1 second
            1_000_000_100, // ts_recv
            "BTCUSDT",
            "BINANCE",
            dec!(50000.0),
            dec!(1.5),
            TradeSideCompat::Buy,
            1,
        );

        let event = MarketDataEventDbn::trade(trade);

        assert!(event.is_trade());
        assert!(!event.is_quote());
        assert!(!event.is_book());
        assert_eq!(event.level(), 0);
        assert_eq!(event.ts_event(), 1_000_000_000);
    }

    #[test]
    fn test_market_data_event_dbn_quote() {
        use crate::data::create_bbo_msg_from_decimals;

        let quote = create_bbo_msg_from_decimals(
            2_000_000_000, // ts_event
            2_000_000_100, // ts_recv
            "ETHUSDT",
            "BINANCE",
            dec!(3000.0),  // bid_price
            dec!(3001.0),  // ask_price
            dec!(10.0),    // bid_size
            dec!(15.0),    // ask_size
            1,             // sequence
        );

        let event = MarketDataEventDbn::quote(quote);

        assert!(!event.is_trade());
        assert!(event.is_quote());
        assert!(!event.is_book());
        assert_eq!(event.level(), 1);
        assert_eq!(event.ts_event(), 2_000_000_000);
    }

    #[test]
    fn test_market_data_event_dbn_levels() {
        use crate::data::{create_bbo_msg_from_decimals, create_trade_msg_from_decimals, TradeSideCompat};

        let trade = create_trade_msg_from_decimals(
            1_000_000_000,
            1_000_000_100,
            "BTCUSDT",
            "BINANCE",
            dec!(50000.0),
            dec!(1.5),
            TradeSideCompat::Buy,
            1,
        );
        let quote = create_bbo_msg_from_decimals(
            2_000_000_000,
            2_000_000_100,
            "BTCUSDT",
            "BINANCE",
            dec!(50000.0),
            dec!(50001.0),
            dec!(10.0),
            dec!(10.0),
            1,
        );

        // Check level assignments
        assert_eq!(MarketDataEventDbn::trade(trade.clone()).level(), 0);
        assert_eq!(MarketDataEventDbn::quote(quote.clone()).level(), 1);
        // L2 and L3 would need Mbp10Msg and MboMsg which require more setup
    }

    #[test]
    fn test_market_data_event_dbn_instrument_id() {
        use crate::data::{create_trade_msg_from_decimals, symbol_to_instrument_id, TradeSideCompat};

        let trade = create_trade_msg_from_decimals(
            1_000_000_000,
            1_000_000_100,
            "BTCUSDT",
            "BINANCE",
            dec!(50000.0),
            dec!(1.5),
            TradeSideCompat::Buy,
            1,
        );

        let event = MarketDataEventDbn::trade(trade);
        let expected_id = symbol_to_instrument_id("BTCUSDT", "BINANCE");

        assert_eq!(event.instrument_id(), expected_id);
    }
}
