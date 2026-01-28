//! Order book data structures for L1/L2/L3 market depth.
//!
//! This module provides order book representation and management:
//! - `BookLevel` - A single price level with size and order count
//! - `OrderBook` - Full order book with bids and asks
//! - `OrderBookDelta` - Incremental book updates
//!
//! # Order Book Levels
//!
//! - **L1 (Top of Book)**: Best bid and ask only
//! - **L2 (Market Depth)**: Multiple price levels, aggregated sizes
//! - **L3 (Full Book)**: Individual orders at each level (not implemented here)
//!
//! # Example
//!
//! ```no_run
//! use trading_common::data::{OrderBook, BookLevel, BookAction};
//! use rust_decimal_macros::dec;
//! use chrono::Utc;
//!
//! let mut book = OrderBook::new("BTCUSDT".to_string());
//!
//! // Add some levels
//! book.update_bid(dec!(50000), dec!(1.5), 5);
//! book.update_bid(dec!(49999), dec!(2.0), 3);
//! book.update_ask(dec!(50001), dec!(1.0), 4);
//! book.update_ask(dec!(50002), dec!(3.0), 7);
//!
//! // Access L1 (top of book)
//! println!("Best bid: {:?}", book.best_bid());
//! println!("Best ask: {:?}", book.best_ask());
//! println!("Spread: {}", book.spread().unwrap());
//!
//! // Access L2 (market depth)
//! println!("Top 5 bids: {:?}", book.bids(5));
//! println!("Top 5 asks: {:?}", book.asks(5));
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

/// A single price level in the order book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookLevel {
    /// Price at this level
    pub price: Decimal,
    /// Total size/quantity at this level
    pub size: Decimal,
    /// Number of orders at this level (if available)
    pub order_count: u32,
}

impl BookLevel {
    /// Create a new book level
    pub fn new(price: Decimal, size: Decimal, order_count: u32) -> Self {
        Self {
            price,
            size,
            order_count,
        }
    }

    /// Create a book level without order count
    pub fn simple(price: Decimal, size: Decimal) -> Self {
        Self {
            price,
            size,
            order_count: 0,
        }
    }

    /// Check if this level has any size
    pub fn is_empty(&self) -> bool {
        self.size.is_zero()
    }

    /// Calculate notional value at this level
    pub fn notional(&self) -> Decimal {
        self.price * self.size
    }
}

impl fmt::Display for BookLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.order_count > 0 {
            write!(f, "{}@{} ({})", self.size, self.price, self.order_count)
        } else {
            write!(f, "{}@{}", self.size, self.price)
        }
    }
}

/// Side of the order book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BookSide {
    /// Bid side (buyers)
    Bid,
    /// Ask side (sellers)
    Ask,
}

impl fmt::Display for BookSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookSide::Bid => write!(f, "BID"),
            BookSide::Ask => write!(f, "ASK"),
        }
    }
}

/// Action type for order book updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BookAction {
    /// Add a new level or update existing
    Add,
    /// Update an existing level
    Update,
    /// Delete a level
    Delete,
    /// Clear all levels on one side
    Clear,
}

impl fmt::Display for BookAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BookAction::Add => write!(f, "ADD"),
            BookAction::Update => write!(f, "UPDATE"),
            BookAction::Delete => write!(f, "DELETE"),
            BookAction::Clear => write!(f, "CLEAR"),
        }
    }
}

/// Order book depth level specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BookDepth {
    /// L1 - Top of book only (best bid/ask)
    L1,
    /// L2 - Market depth with N levels
    L2(usize),
    /// Full book - all levels
    Full,
}

impl Default for BookDepth {
    fn default() -> Self {
        BookDepth::L2(10)
    }
}

/// Wrapper for reverse ordering of Decimal (for asks)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReverseDecimal(Decimal);

impl PartialOrd for ReverseDecimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReverseDecimal {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering
        other.0.cmp(&self.0)
    }
}

/// Full order book with bids and asks.
///
/// Maintains sorted price levels for both sides:
/// - Bids: Sorted descending (highest first)
/// - Asks: Sorted ascending (lowest first)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    /// Symbol for this order book
    pub symbol: String,

    /// Exchange/venue
    #[serde(default)]
    pub exchange: String,

    /// Bid levels (price -> level), sorted descending
    #[serde(with = "btree_decimal_serde")]
    bids: BTreeMap<ReverseDecimal, BookLevel>,

    /// Ask levels (price -> level), sorted ascending
    #[serde(with = "btree_decimal_serde")]
    asks: BTreeMap<ReverseDecimal, BookLevel>,

    /// Last update timestamp
    pub ts_event: DateTime<Utc>,

    /// Sequence number
    pub sequence: u64,
}

// Custom serialization for BTreeMap with ReverseDecimal keys
mod btree_decimal_serde {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(
        map: &BTreeMap<ReverseDecimal, BookLevel>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let vec: Vec<&BookLevel> = map.values().collect();
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<BTreeMap<ReverseDecimal, BookLevel>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<BookLevel> = Vec::deserialize(deserializer)?;
        let mut map = BTreeMap::new();
        for level in vec {
            map.insert(ReverseDecimal(level.price), level);
        }
        Ok(map)
    }
}

impl OrderBook {
    /// Create a new empty order book
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: String::new(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            ts_event: Utc::now(),
            sequence: 0,
        }
    }

    /// Create with exchange specified
    pub fn with_exchange(symbol: impl Into<String>, exchange: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: exchange.into(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            ts_event: Utc::now(),
            sequence: 0,
        }
    }

    /// Update the timestamp and sequence
    pub fn set_timestamp(&mut self, ts: DateTime<Utc>, sequence: u64) {
        self.ts_event = ts;
        self.sequence = sequence;
    }

    // ========================================================================
    // Bid side operations
    // ========================================================================

    /// Update or add a bid level
    pub fn update_bid(&mut self, price: Decimal, size: Decimal, order_count: u32) {
        let key = ReverseDecimal(price);
        if size.is_zero() {
            self.bids.remove(&key);
        } else {
            self.bids
                .insert(key, BookLevel::new(price, size, order_count));
        }
    }

    /// Remove a bid level
    pub fn remove_bid(&mut self, price: Decimal) {
        self.bids.remove(&ReverseDecimal(price));
    }

    /// Clear all bids
    pub fn clear_bids(&mut self) {
        self.bids.clear();
    }

    /// Get the best bid (highest price)
    pub fn best_bid(&self) -> Option<&BookLevel> {
        self.bids.values().next()
    }

    /// Get top N bid levels (sorted by price descending)
    pub fn bids(&self, depth: usize) -> Vec<&BookLevel> {
        self.bids.values().take(depth).collect()
    }

    /// Get all bid levels
    pub fn all_bids(&self) -> Vec<&BookLevel> {
        self.bids.values().collect()
    }

    /// Get number of bid levels
    pub fn bid_depth(&self) -> usize {
        self.bids.len()
    }

    /// Get total bid size across all levels
    pub fn total_bid_size(&self) -> Decimal {
        self.bids.values().map(|l| l.size).sum()
    }

    /// Get total bid notional value
    pub fn total_bid_notional(&self) -> Decimal {
        self.bids.values().map(|l| l.notional()).sum()
    }

    // ========================================================================
    // Ask side operations
    // ========================================================================

    /// Update or add an ask level
    pub fn update_ask(&mut self, price: Decimal, size: Decimal, order_count: u32) {
        // For asks, we want ascending order, so negate the key for BTreeMap
        let key = ReverseDecimal(-price);
        if size.is_zero() {
            self.asks.remove(&key);
        } else {
            self.asks
                .insert(key, BookLevel::new(price, size, order_count));
        }
    }

    /// Remove an ask level
    pub fn remove_ask(&mut self, price: Decimal) {
        self.asks.remove(&ReverseDecimal(-price));
    }

    /// Clear all asks
    pub fn clear_asks(&mut self) {
        self.asks.clear();
    }

    /// Get the best ask (lowest price)
    pub fn best_ask(&self) -> Option<&BookLevel> {
        self.asks.values().next()
    }

    /// Get top N ask levels (sorted by price ascending)
    pub fn asks(&self, depth: usize) -> Vec<&BookLevel> {
        self.asks.values().take(depth).collect()
    }

    /// Get all ask levels
    pub fn all_asks(&self) -> Vec<&BookLevel> {
        self.asks.values().collect()
    }

    /// Get number of ask levels
    pub fn ask_depth(&self) -> usize {
        self.asks.len()
    }

    /// Get total ask size across all levels
    pub fn total_ask_size(&self) -> Decimal {
        self.asks.values().map(|l| l.size).sum()
    }

    /// Get total ask notional value
    pub fn total_ask_notional(&self) -> Decimal {
        self.asks.values().map(|l| l.notional()).sum()
    }

    // ========================================================================
    // Book-wide operations
    // ========================================================================

    /// Clear the entire book
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }

    /// Check if the book is empty
    pub fn is_empty(&self) -> bool {
        self.bids.is_empty() && self.asks.is_empty()
    }

    /// Get the bid-ask spread
    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    /// Get the mid price
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / Decimal::new(2, 0)),
            _ => None,
        }
    }

    /// Get the weighted mid price
    pub fn weighted_mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => {
                let total = bid.size + ask.size;
                if total.is_zero() {
                    self.mid_price()
                } else {
                    Some((bid.price * ask.size + ask.price * bid.size) / total)
                }
            }
            _ => None,
        }
    }

    /// Check if the book is crossed (best bid >= best ask)
    pub fn is_crossed(&self) -> bool {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => bid.price >= ask.price,
            _ => false,
        }
    }

    /// Calculate book imbalance at top of book
    pub fn imbalance(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => {
                let total = bid.size + ask.size;
                if total.is_zero() {
                    Some(Decimal::ZERO)
                } else {
                    Some((bid.size - ask.size) / total)
                }
            }
            _ => None,
        }
    }

    /// Calculate cumulative imbalance up to N levels
    pub fn cumulative_imbalance(&self, depth: usize) -> Option<Decimal> {
        let bid_size: Decimal = self.bids.values().take(depth).map(|l| l.size).sum();
        let ask_size: Decimal = self.asks.values().take(depth).map(|l| l.size).sum();
        let total = bid_size + ask_size;

        if total.is_zero() {
            None
        } else {
            Some((bid_size - ask_size) / total)
        }
    }

    /// Simulate a market buy order, returning average fill price and total cost
    ///
    /// Returns (fill_price, total_cost, filled_qty, remaining_qty)
    pub fn simulate_market_buy(&self, quantity: Decimal) -> (Decimal, Decimal, Decimal, Decimal) {
        let mut remaining = quantity;
        let mut total_cost = Decimal::ZERO;
        let mut filled = Decimal::ZERO;

        for level in self.asks.values() {
            if remaining.is_zero() {
                break;
            }
            let fill_qty = remaining.min(level.size);
            total_cost += fill_qty * level.price;
            filled += fill_qty;
            remaining -= fill_qty;
        }

        let avg_price = if filled.is_zero() {
            Decimal::ZERO
        } else {
            total_cost / filled
        };

        (avg_price, total_cost, filled, remaining)
    }

    /// Simulate a market sell order, returning average fill price and total proceeds
    ///
    /// Returns (fill_price, total_proceeds, filled_qty, remaining_qty)
    pub fn simulate_market_sell(&self, quantity: Decimal) -> (Decimal, Decimal, Decimal, Decimal) {
        let mut remaining = quantity;
        let mut total_proceeds = Decimal::ZERO;
        let mut filled = Decimal::ZERO;

        for level in self.bids.values() {
            if remaining.is_zero() {
                break;
            }
            let fill_qty = remaining.min(level.size);
            total_proceeds += fill_qty * level.price;
            filled += fill_qty;
            remaining -= fill_qty;
        }

        let avg_price = if filled.is_zero() {
            Decimal::ZERO
        } else {
            total_proceeds / filled
        };

        (avg_price, total_proceeds, filled, remaining)
    }

    /// Convert to a QuoteTick (L1 representation)
    pub fn to_quote_tick(&self) -> Option<super::quotes::QuoteTick> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(super::quotes::QuoteTick::with_details(
                self.ts_event,
                self.ts_event,
                self.symbol.clone(),
                self.exchange.clone(),
                bid.price,
                ask.price,
                bid.size,
                ask.size,
                self.sequence,
            )),
            _ => None,
        }
    }
}

impl fmt::Display for OrderBook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "OrderBook {} @ {}", self.symbol, self.ts_event)?;
        writeln!(f, "  Asks ({}):", self.ask_depth())?;
        for level in self.asks(5).iter().rev() {
            writeln!(f, "    {}", level)?;
        }
        if let Some(spread) = self.spread() {
            writeln!(f, "  --- spread: {} ---", spread)?;
        }
        writeln!(f, "  Bids ({}):", self.bid_depth())?;
        for level in self.bids(5) {
            writeln!(f, "    {}", level)?;
        }
        Ok(())
    }
}

/// A single order book delta/update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookDelta {
    /// Symbol
    pub symbol: String,

    /// Exchange
    #[serde(default)]
    pub exchange: String,

    /// Timestamp of the event
    pub ts_event: DateTime<Utc>,

    /// Receive timestamp
    #[serde(default = "Utc::now")]
    pub ts_recv: DateTime<Utc>,

    /// Side being updated
    pub side: BookSide,

    /// Action type
    pub action: BookAction,

    /// Price level
    pub price: Decimal,

    /// New size (0 for delete)
    pub size: Decimal,

    /// Order count at level
    #[serde(default)]
    pub order_count: u32,

    /// Sequence number
    #[serde(default)]
    pub sequence: u64,

    /// Flags (venue-specific)
    #[serde(default)]
    pub flags: u8,

    /// Order ID for L3 (MBO) data
    /// Present when delta represents an individual order action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<u64>,
}

impl OrderBookDelta {
    /// Create a new delta
    pub fn new(
        symbol: impl Into<String>,
        ts_event: DateTime<Utc>,
        side: BookSide,
        action: BookAction,
        price: Decimal,
        size: Decimal,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: String::new(),
            ts_event,
            ts_recv: ts_event,
            side,
            action,
            price,
            size,
            order_count: 0,
            sequence: 0,
            flags: 0,
            order_id: None,
        }
    }

    /// Create an add/update delta
    pub fn update(
        symbol: impl Into<String>,
        ts_event: DateTime<Utc>,
        side: BookSide,
        price: Decimal,
        size: Decimal,
    ) -> Self {
        let action = if size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };
        Self::new(symbol, ts_event, side, action, price, size)
    }

    /// Create a delete delta
    pub fn delete(
        symbol: impl Into<String>,
        ts_event: DateTime<Utc>,
        side: BookSide,
        price: Decimal,
    ) -> Self {
        Self::new(
            symbol,
            ts_event,
            side,
            BookAction::Delete,
            price,
            Decimal::ZERO,
        )
    }

    /// Apply this delta to an order book
    pub fn apply(&self, book: &mut OrderBook) {
        book.ts_event = self.ts_event;
        if self.sequence > book.sequence {
            book.sequence = self.sequence;
        }

        match self.action {
            BookAction::Clear => match self.side {
                BookSide::Bid => book.clear_bids(),
                BookSide::Ask => book.clear_asks(),
            },
            BookAction::Delete => match self.side {
                BookSide::Bid => book.remove_bid(self.price),
                BookSide::Ask => book.remove_ask(self.price),
            },
            BookAction::Add | BookAction::Update => match self.side {
                BookSide::Bid => book.update_bid(self.price, self.size, self.order_count),
                BookSide::Ask => book.update_ask(self.price, self.size, self.order_count),
            },
        }
    }
}

impl fmt::Display for OrderBookDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {}@{}",
            self.symbol, self.side, self.action, self.size, self.price
        )
    }
}

/// A batch of order book deltas (snapshot or incremental).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookDeltas {
    /// Symbol
    pub symbol: String,

    /// Exchange
    #[serde(default)]
    pub exchange: String,

    /// Whether this is a full snapshot
    pub is_snapshot: bool,

    /// Timestamp
    pub ts_event: DateTime<Utc>,

    /// Sequence number
    pub sequence: u64,

    /// Individual deltas
    pub deltas: Vec<OrderBookDelta>,
}

impl OrderBookDeltas {
    /// Create an empty batch
    pub fn new(symbol: impl Into<String>, is_snapshot: bool) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: String::new(),
            is_snapshot,
            ts_event: Utc::now(),
            sequence: 0,
            deltas: Vec::new(),
        }
    }

    /// Create a snapshot batch
    pub fn snapshot(symbol: impl Into<String>) -> Self {
        Self::new(symbol, true)
    }

    /// Create an incremental batch
    pub fn incremental(symbol: impl Into<String>) -> Self {
        Self::new(symbol, false)
    }

    /// Add a delta to the batch
    pub fn add(&mut self, delta: OrderBookDelta) {
        self.deltas.push(delta);
    }

    /// Apply all deltas to an order book
    pub fn apply(&self, book: &mut OrderBook) {
        if self.is_snapshot {
            book.clear();
        }
        for delta in &self.deltas {
            delta.apply(book);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_book_level() {
        let level = BookLevel::new(dec!(100.00), dec!(5.5), 3);
        assert_eq!(level.price, dec!(100.00));
        assert_eq!(level.size, dec!(5.5));
        assert_eq!(level.order_count, 3);
        assert_eq!(level.notional(), dec!(550.00));
        assert!(!level.is_empty());

        let empty = BookLevel::simple(dec!(100), Decimal::ZERO);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_order_book_basic() {
        let mut book = OrderBook::new("BTCUSDT");

        // Add bids
        book.update_bid(dec!(50000), dec!(1.0), 2);
        book.update_bid(dec!(49999), dec!(2.0), 3);
        book.update_bid(dec!(49998), dec!(3.0), 5);

        // Add asks
        book.update_ask(dec!(50001), dec!(1.5), 2);
        book.update_ask(dec!(50002), dec!(2.5), 4);

        // Check best bid/ask
        assert_eq!(book.best_bid().unwrap().price, dec!(50000));
        assert_eq!(book.best_ask().unwrap().price, dec!(50001));

        // Check spread
        assert_eq!(book.spread(), Some(dec!(1)));

        // Check mid price
        assert_eq!(book.mid_price(), Some(dec!(50000.5)));

        // Check depths
        assert_eq!(book.bid_depth(), 3);
        assert_eq!(book.ask_depth(), 2);
    }

    #[test]
    fn test_order_book_ordering() {
        let mut book = OrderBook::new("TEST");

        // Add bids in random order
        book.update_bid(dec!(100), dec!(1), 0);
        book.update_bid(dec!(103), dec!(1), 0);
        book.update_bid(dec!(101), dec!(1), 0);

        // Should be sorted descending
        let bids = book.bids(3);
        assert_eq!(bids[0].price, dec!(103));
        assert_eq!(bids[1].price, dec!(101));
        assert_eq!(bids[2].price, dec!(100));

        // Add asks in random order
        book.update_ask(dec!(105), dec!(1), 0);
        book.update_ask(dec!(104), dec!(1), 0);
        book.update_ask(dec!(106), dec!(1), 0);

        // Should be sorted ascending
        let asks = book.asks(3);
        assert_eq!(asks[0].price, dec!(104));
        assert_eq!(asks[1].price, dec!(105));
        assert_eq!(asks[2].price, dec!(106));
    }

    #[test]
    fn test_order_book_removal() {
        let mut book = OrderBook::new("TEST");

        book.update_bid(dec!(100), dec!(1), 0);
        book.update_bid(dec!(99), dec!(2), 0);
        assert_eq!(book.bid_depth(), 2);

        // Remove by setting size to 0
        book.update_bid(dec!(100), Decimal::ZERO, 0);
        assert_eq!(book.bid_depth(), 1);
        assert_eq!(book.best_bid().unwrap().price, dec!(99));

        // Remove explicitly
        book.remove_bid(dec!(99));
        assert_eq!(book.bid_depth(), 0);
    }

    #[test]
    fn test_order_book_imbalance() {
        let mut book = OrderBook::new("TEST");

        // More bids than asks
        book.update_bid(dec!(100), dec!(10), 0);
        book.update_ask(dec!(101), dec!(5), 0);

        let imbalance = book.imbalance().unwrap();
        // (10 - 5) / (10 + 5) = 5/15 = 0.333...
        assert!(imbalance > dec!(0.33) && imbalance < dec!(0.34));
    }

    #[test]
    fn test_simulate_market_order() {
        let mut book = OrderBook::new("TEST");

        // Setup asks: 1@100, 2@101, 3@102
        book.update_ask(dec!(100), dec!(1), 0);
        book.update_ask(dec!(101), dec!(2), 0);
        book.update_ask(dec!(102), dec!(3), 0);

        // Buy 2 units - should fill 1@100, 1@101
        let (avg_price, total_cost, filled, remaining) = book.simulate_market_buy(dec!(2));
        assert_eq!(filled, dec!(2));
        assert_eq!(remaining, Decimal::ZERO);
        assert_eq!(total_cost, dec!(201)); // 100 + 101
        assert_eq!(avg_price, dec!(100.5));

        // Buy 10 units - not enough liquidity
        let (_, _, filled, remaining) = book.simulate_market_buy(dec!(10));
        assert_eq!(filled, dec!(6)); // 1 + 2 + 3
        assert_eq!(remaining, dec!(4));
    }

    #[test]
    fn test_order_book_delta() {
        let mut book = OrderBook::new("TEST");

        // Apply some deltas
        let delta1 = OrderBookDelta::update("TEST", Utc::now(), BookSide::Bid, dec!(100), dec!(5));
        let delta2 = OrderBookDelta::update("TEST", Utc::now(), BookSide::Ask, dec!(101), dec!(3));

        delta1.apply(&mut book);
        delta2.apply(&mut book);

        assert_eq!(book.best_bid().unwrap().price, dec!(100));
        assert_eq!(book.best_ask().unwrap().price, dec!(101));

        // Delete
        let delta3 = OrderBookDelta::delete("TEST", Utc::now(), BookSide::Bid, dec!(100));
        delta3.apply(&mut book);
        assert!(book.best_bid().is_none());
    }

    #[test]
    fn test_order_book_deltas_snapshot() {
        let mut book = OrderBook::new("TEST");

        // Pre-populate
        book.update_bid(dec!(50), dec!(1), 0);

        // Apply snapshot - should clear first
        let mut deltas = OrderBookDeltas::snapshot("TEST");
        deltas.add(OrderBookDelta::update(
            "TEST",
            Utc::now(),
            BookSide::Bid,
            dec!(100),
            dec!(5),
        ));
        deltas.add(OrderBookDelta::update(
            "TEST",
            Utc::now(),
            BookSide::Ask,
            dec!(101),
            dec!(3),
        ));

        deltas.apply(&mut book);

        // Old data should be gone
        assert_eq!(book.bid_depth(), 1);
        assert_eq!(book.best_bid().unwrap().price, dec!(100));
    }

    #[test]
    fn test_to_quote_tick() {
        let mut book = OrderBook::new("BTCUSDT");
        book.exchange = "BINANCE".to_string();
        book.update_bid(dec!(50000), dec!(1.5), 2);
        book.update_ask(dec!(50001), dec!(2.0), 3);

        let quote = book.to_quote_tick().unwrap();
        assert_eq!(quote.symbol, "BTCUSDT");
        assert_eq!(quote.bid_price, dec!(50000));
        assert_eq!(quote.ask_price, dec!(50001));
        assert_eq!(quote.bid_size, dec!(1.5));
        assert_eq!(quote.ask_size, dec!(2.0));
    }

    #[test]
    fn test_is_crossed() {
        let mut book = OrderBook::new("TEST");

        // Normal book
        book.update_bid(dec!(100), dec!(1), 0);
        book.update_ask(dec!(101), dec!(1), 0);
        assert!(!book.is_crossed());

        // Crossed book
        book.update_bid(dec!(102), dec!(1), 0);
        assert!(book.is_crossed());
    }
}
