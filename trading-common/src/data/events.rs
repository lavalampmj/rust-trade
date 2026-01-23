//! Market data events for strategy event-driven processing.
//!
//! This module provides market data event types that can be used by strategies
//! to react to various market data updates including ticks, quotes, and order book changes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
}
