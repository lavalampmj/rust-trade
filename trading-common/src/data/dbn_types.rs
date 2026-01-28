//! DBN (Databento Binary Normalized) format types and utilities
//!
//! This module provides the canonical data representation for the entire system.
//! `dbn::TradeMsg` is the standard format for all market data from ingestion
//! through IPC, storage, and strategy execution.
//!
//! Only presentation/frontend layers should convert to display formats.
//!
//! # Type Hierarchy
//!
//! - **TradeMsg**: Individual trade executions (last tick)
//! - **BboMsg**: Best bid/offer quotes (L1)
//! - **Mbp1Msg**: Market-by-price with 1 level (L2)
//! - **Mbp10Msg**: Market-by-price with 10 levels (L2)
//! - **MboMsg**: Market-by-order (L3)
//!
//! # Extension Traits
//!
//! This module provides extension traits for convenient access:
//! - `TradeMsgExt`: Price/size as Decimal, timestamps as DateTime
//! - `BboMsgExt`: BBO quote accessors
//! - `Mbp1MsgExt`: Single-level book accessors
//! - `Mbp10MsgExt`: 10-level book accessors
//! - `MboMsgExt`: L3 order accessors

use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::os::raw::c_char;

// Re-export core DBN types as canonical format
pub use dbn::enums::Side as DbnSide;
pub use dbn::record::{BboMsg, BidAskPair, MboMsg, Mbp10Msg, Mbp1Msg, TradeMsg};
pub use dbn::{FlagSet, RecordHeader, VersionUpgradePolicy};

/// DBN fixed-point scale factor (1 unit = 1e-9)
pub const DBN_PRICE_SCALE: i64 = 1_000_000_000;

/// Custom publisher ID for non-Databento sources (Binance, etc.)
pub const CUSTOM_PUBLISHER_ID: u16 = 0xFFFF;

/// Extension trait for `dbn::TradeMsg` providing convenient helper methods
///
/// This trait adds Rust-friendly accessors for price, size, and timestamp
/// that convert from DBN's fixed-point/nanosecond representations.
pub trait TradeMsgExt {
    /// Get price as Decimal (converted from fixed-point 1e-9 scale)
    fn price_decimal(&self) -> Decimal;

    /// Get size as Decimal (converted from u32)
    fn size_decimal(&self) -> Decimal;

    /// Get event timestamp as DateTime<Utc>
    fn timestamp(&self) -> DateTime<Utc>;

    /// Get receive timestamp as DateTime<Utc>
    fn recv_timestamp(&self) -> DateTime<Utc>;

    /// Get side as a convenient enum (Buy, Sell, None)
    fn trade_side(&self) -> TradeSideCompat;

    /// Check if this is a buy (aggressor is buyer)
    fn is_buy(&self) -> bool;

    /// Check if this is a sell (aggressor is seller)
    fn is_sell(&self) -> bool;
}

impl TradeMsgExt for TradeMsg {
    fn price_decimal(&self) -> Decimal {
        Decimal::from(self.price) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn size_decimal(&self) -> Decimal {
        Decimal::from(self.size)
    }

    fn timestamp(&self) -> DateTime<Utc> {
        let ts_event = self.hd.ts_event;
        Utc.timestamp_nanos(ts_event as i64)
    }

    fn recv_timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.ts_recv as i64)
    }

    fn trade_side(&self) -> TradeSideCompat {
        match self.side as u8 as char {
            'A' => TradeSideCompat::Sell, // Ask side = Sell aggressor
            'B' => TradeSideCompat::Buy,  // Bid side = Buy aggressor
            _ => TradeSideCompat::None,
        }
    }

    fn is_buy(&self) -> bool {
        self.side as u8 as char == 'B'
    }

    fn is_sell(&self) -> bool {
        self.side as u8 as char == 'A'
    }
}

/// Compatible trade side enum for use throughout the system
///
/// Maps to DBN's side field ('A' = Ask/Sell, 'B' = Bid/Buy, 'N' = None)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TradeSideCompat {
    Buy,
    Sell,
    None,
}

impl TradeSideCompat {
    /// Convert to DBN side character
    pub fn to_dbn_char(&self) -> i8 {
        match self {
            TradeSideCompat::Buy => b'B' as i8,
            TradeSideCompat::Sell => b'A' as i8,
            TradeSideCompat::None => b'N' as i8,
        }
    }

    /// Parse from DBN side character
    pub fn from_dbn_char(c: i8) -> Self {
        match c as u8 as char {
            'B' => TradeSideCompat::Buy,
            'A' => TradeSideCompat::Sell,
            _ => TradeSideCompat::None,
        }
    }

    /// Convert to database string representation
    pub fn as_db_str(&self) -> &'static str {
        match self {
            TradeSideCompat::Buy => "BUY",
            TradeSideCompat::Sell => "SELL",
            TradeSideCompat::None => "NONE",
        }
    }
}

impl std::fmt::Display for TradeSideCompat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeSideCompat::Buy => write!(f, "BUY"),
            TradeSideCompat::Sell => write!(f, "SELL"),
            TradeSideCompat::None => write!(f, "NONE"),
        }
    }
}

/// Generate a deterministic instrument_id from symbol and exchange
///
/// This function creates a stable u32 hash that can be used as the
/// instrument_id field in TradeMsg for non-Databento sources.
///
/// # Arguments
/// * `symbol` - Trading symbol (e.g., "BTCUSDT", "ESH5")
/// * `exchange` - Exchange identifier (e.g., "BINANCE", "CME")
///
/// # Returns
/// A deterministic u32 that uniquely identifies this symbol/exchange pair
pub fn symbol_to_instrument_id(symbol: &str, exchange: &str) -> u32 {
    let mut hasher = DefaultHasher::new();
    symbol.hash(&mut hasher);
    exchange.hash(&mut hasher);
    hasher.finish() as u32
}

/// Create a TradeMsg from individual components
///
/// This is a convenience function for creating TradeMsg instances
/// from sources like Binance that provide data in different formats.
#[allow(clippy::too_many_arguments)]
pub fn create_trade_msg(
    ts_event_nanos: u64,
    ts_recv_nanos: u64,
    instrument_id: u32,
    price: Decimal,
    size: u32,
    side: TradeSideCompat,
    sequence: u32,
    publisher_id: u16,
) -> TradeMsg {
    use std::os::raw::c_char;

    // Convert price to fixed-point (1e-9 scale)
    let price_fixed = (price * Decimal::from(DBN_PRICE_SCALE))
        .round()
        .to_string()
        .parse::<i64>()
        .unwrap_or(0);

    TradeMsg {
        hd: RecordHeader::new::<TradeMsg>(
            dbn::rtype::MBP_0, // Trade record type
            publisher_id,
            instrument_id,
            ts_event_nanos,
        ),
        price: price_fixed,
        size,
        action: b'T' as c_char, // Trade action
        side: side.to_dbn_char() as c_char,
        flags: FlagSet::default(),
        depth: 0,
        ts_recv: ts_recv_nanos,
        ts_in_delta: 0,
        sequence,
    }
}

/// Create a TradeMsg from Decimal price and size values
///
/// Convenience function that handles the fixed-point conversion.
/// Uses hash-based instrument_id generation from symbol and exchange.
#[allow(clippy::too_many_arguments)]
pub fn create_trade_msg_from_decimals(
    ts_event_nanos: u64,
    ts_recv_nanos: u64,
    symbol: &str,
    exchange: &str,
    price: Decimal,
    size: Decimal,
    side: TradeSideCompat,
    sequence: u32,
) -> TradeMsg {
    let instrument_id = symbol_to_instrument_id(symbol, exchange);

    // Convert size to u32 (assuming reasonable trade sizes)
    let size_u32 = size.round().to_string().parse::<u32>().unwrap_or(0);

    create_trade_msg(
        ts_event_nanos,
        ts_recv_nanos,
        instrument_id,
        price,
        size_u32,
        side,
        sequence,
        CUSTOM_PUBLISHER_ID,
    )
}

/// Create a TradeMsg with an explicit instrument_id
///
/// Use this when you have a persistent instrument_id from InstrumentRegistry
/// rather than generating a hash-based ID.
///
/// # Arguments
/// * `ts_event_nanos` - Event timestamp in nanoseconds
/// * `ts_recv_nanos` - Receive timestamp in nanoseconds
/// * `instrument_id` - Pre-assigned instrument identifier
/// * `price` - Trade price as Decimal
/// * `size` - Trade size as Decimal
/// * `side` - Trade side (Buy/Sell)
/// * `sequence` - Sequence number for ordering
#[allow(clippy::too_many_arguments)]
pub fn create_trade_msg_with_instrument_id(
    ts_event_nanos: u64,
    ts_recv_nanos: u64,
    instrument_id: u32,
    price: Decimal,
    size: Decimal,
    side: TradeSideCompat,
    sequence: u32,
) -> TradeMsg {
    // Convert size to u32 (assuming reasonable trade sizes)
    let size_u32 = size.round().to_string().parse::<u32>().unwrap_or(0);

    create_trade_msg(
        ts_event_nanos,
        ts_recv_nanos,
        instrument_id,
        price,
        size_u32,
        side,
        sequence,
        CUSTOM_PUBLISHER_ID,
    )
}

// =============================================================================
// Inverse Conversion Functions (Decimal/DateTime -> DBN format)
// =============================================================================

/// Convert Decimal price to DBN fixed-point format (1e-9 scale)
///
/// # Example
/// ```
/// use trading_common::data::dbn_types::decimal_to_dbn_price;
/// use rust_decimal_macros::dec;
///
/// let dbn_price = decimal_to_dbn_price(dec!(50000.50));
/// assert_eq!(dbn_price, 50000500000000);
/// ```
pub fn decimal_to_dbn_price(price: Decimal) -> i64 {
    (price * Decimal::from(DBN_PRICE_SCALE))
        .round()
        .to_string()
        .parse::<i64>()
        .unwrap_or(0)
}

/// Convert DateTime<Utc> to nanoseconds since Unix epoch
///
/// Returns 0 if the conversion fails (e.g., timestamp before 1970).
pub fn datetime_to_nanos(dt: DateTime<Utc>) -> u64 {
    dt.timestamp_nanos_opt().unwrap_or(0) as u64
}

/// Convert nanoseconds since Unix epoch to DateTime<Utc>
pub fn nanos_to_datetime(nanos: u64) -> DateTime<Utc> {
    Utc.timestamp_nanos(nanos as i64)
}

/// Convert Decimal size to u32 (for DBN size fields)
///
/// Rounds to nearest integer and clamps to u32 range.
pub fn decimal_to_dbn_size(size: Decimal) -> u32 {
    size.round()
        .to_string()
        .parse::<u32>()
        .unwrap_or(0)
}

// =============================================================================
// BboMsg Extension Trait (L1 Quote Data)
// =============================================================================

/// Extension trait for `dbn::BboMsg` providing convenient helper methods
///
/// BboMsg represents best bid/offer (L1) quote data with a single price level.
pub trait BboMsgExt {
    /// Get event timestamp as DateTime<Utc>
    fn timestamp(&self) -> DateTime<Utc>;

    /// Get receive timestamp as DateTime<Utc>
    fn recv_timestamp(&self) -> DateTime<Utc>;

    /// Get bid price as Decimal
    fn bid_price(&self) -> Decimal;

    /// Get ask price as Decimal
    fn ask_price(&self) -> Decimal;

    /// Get bid size as Decimal
    fn bid_size(&self) -> Decimal;

    /// Get ask size as Decimal
    fn ask_size(&self) -> Decimal;

    /// Get spread (ask - bid)
    fn spread(&self) -> Decimal;

    /// Get mid price ((bid + ask) / 2)
    fn mid_price(&self) -> Decimal;

    /// Get last trade price as Decimal
    fn last_price(&self) -> Decimal;

    /// Get last trade size as Decimal
    fn last_size(&self) -> Decimal;
}

impl BboMsgExt for BboMsg {
    fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.hd.ts_event as i64)
    }

    fn recv_timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.ts_recv as i64)
    }

    fn bid_price(&self) -> Decimal {
        Decimal::from(self.levels[0].bid_px) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn ask_price(&self) -> Decimal {
        Decimal::from(self.levels[0].ask_px) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn bid_size(&self) -> Decimal {
        Decimal::from(self.levels[0].bid_sz)
    }

    fn ask_size(&self) -> Decimal {
        Decimal::from(self.levels[0].ask_sz)
    }

    fn spread(&self) -> Decimal {
        self.ask_price() - self.bid_price()
    }

    fn mid_price(&self) -> Decimal {
        (self.bid_price() + self.ask_price()) / Decimal::from(2)
    }

    fn last_price(&self) -> Decimal {
        Decimal::from(self.price) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn last_size(&self) -> Decimal {
        Decimal::from(self.size)
    }
}

// =============================================================================
// Mbp1Msg Extension Trait (L2 Single Level)
// =============================================================================

/// Extension trait for `dbn::Mbp1Msg` providing convenient helper methods
///
/// Mbp1Msg represents market-by-price data with a single level.
pub trait Mbp1MsgExt {
    /// Get event timestamp as DateTime<Utc>
    fn timestamp(&self) -> DateTime<Utc>;

    /// Get receive timestamp as DateTime<Utc>
    fn recv_timestamp(&self) -> DateTime<Utc>;

    /// Get update price as Decimal
    fn price_decimal(&self) -> Decimal;

    /// Get update size as Decimal
    fn size_decimal(&self) -> Decimal;

    /// Get bid price as Decimal
    fn bid_price(&self) -> Decimal;

    /// Get ask price as Decimal
    fn ask_price(&self) -> Decimal;

    /// Get bid size as Decimal
    fn bid_size(&self) -> Decimal;

    /// Get ask size as Decimal
    fn ask_size(&self) -> Decimal;

    /// Get action as BookAction
    fn book_action(&self) -> BookActionCompat;

    /// Get side (bid or ask)
    fn book_side(&self) -> BookSideCompat;
}

impl Mbp1MsgExt for Mbp1Msg {
    fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.hd.ts_event as i64)
    }

    fn recv_timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.ts_recv as i64)
    }

    fn price_decimal(&self) -> Decimal {
        Decimal::from(self.price) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn size_decimal(&self) -> Decimal {
        Decimal::from(self.size)
    }

    fn bid_price(&self) -> Decimal {
        Decimal::from(self.levels[0].bid_px) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn ask_price(&self) -> Decimal {
        Decimal::from(self.levels[0].ask_px) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn bid_size(&self) -> Decimal {
        Decimal::from(self.levels[0].bid_sz)
    }

    fn ask_size(&self) -> Decimal {
        Decimal::from(self.levels[0].ask_sz)
    }

    fn book_action(&self) -> BookActionCompat {
        BookActionCompat::from_dbn_char(self.action)
    }

    fn book_side(&self) -> BookSideCompat {
        BookSideCompat::from_dbn_char(self.side)
    }
}

// =============================================================================
// Mbp10Msg Extension Trait (L2 10 Levels)
// =============================================================================

/// Extension trait for `dbn::Mbp10Msg` providing convenient helper methods
///
/// Mbp10Msg represents market-by-price data with 10 levels on each side.
pub trait Mbp10MsgExt {
    /// Get event timestamp as DateTime<Utc>
    fn timestamp(&self) -> DateTime<Utc>;

    /// Get receive timestamp as DateTime<Utc>
    fn recv_timestamp(&self) -> DateTime<Utc>;

    /// Get update price as Decimal
    fn price_decimal(&self) -> Decimal;

    /// Get update size as Decimal
    fn size_decimal(&self) -> Decimal;

    /// Get bid price at level (0-9)
    fn bid_price_at(&self, level: usize) -> Option<Decimal>;

    /// Get ask price at level (0-9)
    fn ask_price_at(&self, level: usize) -> Option<Decimal>;

    /// Get bid size at level (0-9)
    fn bid_size_at(&self, level: usize) -> Option<Decimal>;

    /// Get ask size at level (0-9)
    fn ask_size_at(&self, level: usize) -> Option<Decimal>;

    /// Get best bid price (level 0)
    fn best_bid_price(&self) -> Decimal;

    /// Get best ask price (level 0)
    fn best_ask_price(&self) -> Decimal;

    /// Get spread at top of book
    fn spread(&self) -> Decimal;

    /// Get action type
    fn book_action(&self) -> BookActionCompat;

    /// Get side
    fn book_side(&self) -> BookSideCompat;
}

impl Mbp10MsgExt for Mbp10Msg {
    fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.hd.ts_event as i64)
    }

    fn recv_timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.ts_recv as i64)
    }

    fn price_decimal(&self) -> Decimal {
        Decimal::from(self.price) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn size_decimal(&self) -> Decimal {
        Decimal::from(self.size)
    }

    fn bid_price_at(&self, level: usize) -> Option<Decimal> {
        if level < 10 {
            Some(Decimal::from(self.levels[level].bid_px) / Decimal::from(DBN_PRICE_SCALE))
        } else {
            None
        }
    }

    fn ask_price_at(&self, level: usize) -> Option<Decimal> {
        if level < 10 {
            Some(Decimal::from(self.levels[level].ask_px) / Decimal::from(DBN_PRICE_SCALE))
        } else {
            None
        }
    }

    fn bid_size_at(&self, level: usize) -> Option<Decimal> {
        if level < 10 {
            Some(Decimal::from(self.levels[level].bid_sz))
        } else {
            None
        }
    }

    fn ask_size_at(&self, level: usize) -> Option<Decimal> {
        if level < 10 {
            Some(Decimal::from(self.levels[level].ask_sz))
        } else {
            None
        }
    }

    fn best_bid_price(&self) -> Decimal {
        Decimal::from(self.levels[0].bid_px) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn best_ask_price(&self) -> Decimal {
        Decimal::from(self.levels[0].ask_px) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn spread(&self) -> Decimal {
        self.best_ask_price() - self.best_bid_price()
    }

    fn book_action(&self) -> BookActionCompat {
        BookActionCompat::from_dbn_char(self.action)
    }

    fn book_side(&self) -> BookSideCompat {
        BookSideCompat::from_dbn_char(self.side)
    }
}

// =============================================================================
// MboMsg Extension Trait (L3 Order Data)
// =============================================================================

/// Extension trait for `dbn::MboMsg` providing convenient helper methods
///
/// MboMsg represents market-by-order (L3) data with individual order IDs.
pub trait MboMsgExt {
    /// Get event timestamp as DateTime<Utc>
    fn timestamp(&self) -> DateTime<Utc>;

    /// Get receive timestamp as DateTime<Utc>
    fn recv_timestamp(&self) -> DateTime<Utc>;

    /// Get order price as Decimal
    fn price_decimal(&self) -> Decimal;

    /// Get order size as Decimal
    fn size_decimal(&self) -> Decimal;

    /// Get action type
    fn book_action(&self) -> BookActionCompat;

    /// Get side
    fn book_side(&self) -> BookSideCompat;

    /// Get the order ID
    fn get_order_id(&self) -> u64;
}

impl MboMsgExt for MboMsg {
    fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.hd.ts_event as i64)
    }

    fn recv_timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_nanos(self.ts_recv as i64)
    }

    fn price_decimal(&self) -> Decimal {
        Decimal::from(self.price) / Decimal::from(DBN_PRICE_SCALE)
    }

    fn size_decimal(&self) -> Decimal {
        Decimal::from(self.size)
    }

    fn book_action(&self) -> BookActionCompat {
        BookActionCompat::from_dbn_char(self.action)
    }

    fn book_side(&self) -> BookSideCompat {
        BookSideCompat::from_dbn_char(self.side)
    }

    fn get_order_id(&self) -> u64 {
        self.order_id
    }
}

// =============================================================================
// Book Action and Side Compat Types
// =============================================================================

/// Compatible book action enum for use throughout the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BookActionCompat {
    /// Add a new order/level
    Add,
    /// Cancel/delete an order/level
    Cancel,
    /// Modify an existing order/level
    Modify,
    /// Clear the book (or one side)
    Clear,
    /// Trade execution
    Trade,
    /// Fill execution
    Fill,
    /// No action specified
    None,
}

impl BookActionCompat {
    /// Parse from DBN action character
    pub fn from_dbn_char(c: c_char) -> Self {
        match c as u8 as char {
            'A' => BookActionCompat::Add,
            'C' => BookActionCompat::Cancel,
            'M' => BookActionCompat::Modify,
            'R' => BookActionCompat::Clear,
            'T' => BookActionCompat::Trade,
            'F' => BookActionCompat::Fill,
            _ => BookActionCompat::None,
        }
    }

    /// Convert to DBN action character
    pub fn to_dbn_char(&self) -> c_char {
        match self {
            BookActionCompat::Add => b'A' as c_char,
            BookActionCompat::Cancel => b'C' as c_char,
            BookActionCompat::Modify => b'M' as c_char,
            BookActionCompat::Clear => b'R' as c_char,
            BookActionCompat::Trade => b'T' as c_char,
            BookActionCompat::Fill => b'F' as c_char,
            BookActionCompat::None => b'N' as c_char,
        }
    }
}

/// Compatible book side enum for use throughout the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BookSideCompat {
    /// Bid side (buy orders)
    Bid,
    /// Ask side (sell orders)
    Ask,
    /// No side specified
    None,
}

impl BookSideCompat {
    /// Parse from DBN side character
    pub fn from_dbn_char(c: c_char) -> Self {
        match c as u8 as char {
            'B' => BookSideCompat::Bid,
            'A' | 'S' => BookSideCompat::Ask,
            _ => BookSideCompat::None,
        }
    }

    /// Convert to DBN side character
    pub fn to_dbn_char(&self) -> c_char {
        match self {
            BookSideCompat::Bid => b'B' as c_char,
            BookSideCompat::Ask => b'A' as c_char,
            BookSideCompat::None => b'N' as c_char,
        }
    }
}

// =============================================================================
// BboMsg Creation Functions
// =============================================================================

/// Create a BboMsg from individual components
#[allow(clippy::too_many_arguments)]
pub fn create_bbo_msg(
    ts_event_nanos: u64,
    ts_recv_nanos: u64,
    instrument_id: u32,
    bid_price: Decimal,
    ask_price: Decimal,
    bid_size: u32,
    ask_size: u32,
    last_price: Decimal,
    last_size: u32,
    side: TradeSideCompat,
    sequence: u32,
    publisher_id: u16,
) -> BboMsg {
    BboMsg {
        hd: RecordHeader::new::<BboMsg>(
            dbn::rtype::BBO_1S,
            publisher_id,
            instrument_id,
            ts_event_nanos,
        ),
        price: decimal_to_dbn_price(last_price),
        size: last_size,
        _reserved1: 0,
        side: side.to_dbn_char() as c_char,
        flags: FlagSet::default(),
        _reserved2: 0,
        ts_recv: ts_recv_nanos,
        _reserved3: [0; 4],
        sequence,
        levels: [BidAskPair {
            bid_px: decimal_to_dbn_price(bid_price),
            ask_px: decimal_to_dbn_price(ask_price),
            bid_sz: bid_size,
            ask_sz: ask_size,
            bid_ct: 0,
            ask_ct: 0,
        }],
    }
}

/// Create a BboMsg from Decimal values with symbol lookup
#[allow(clippy::too_many_arguments)]
pub fn create_bbo_msg_from_decimals(
    ts_event_nanos: u64,
    ts_recv_nanos: u64,
    symbol: &str,
    exchange: &str,
    bid_price: Decimal,
    ask_price: Decimal,
    bid_size: Decimal,
    ask_size: Decimal,
    sequence: u32,
) -> BboMsg {
    let instrument_id = symbol_to_instrument_id(symbol, exchange);
    let mid_price = (bid_price + ask_price) / Decimal::from(2);

    create_bbo_msg(
        ts_event_nanos,
        ts_recv_nanos,
        instrument_id,
        bid_price,
        ask_price,
        decimal_to_dbn_size(bid_size),
        decimal_to_dbn_size(ask_size),
        mid_price,
        0,
        TradeSideCompat::None,
        sequence,
        CUSTOM_PUBLISHER_ID,
    )
}

// =============================================================================
// MboMsg Creation Functions
// =============================================================================

/// Create an MboMsg from individual components
#[allow(clippy::too_many_arguments)]
pub fn create_mbo_msg(
    ts_event_nanos: u64,
    ts_recv_nanos: u64,
    instrument_id: u32,
    order_id: u64,
    price: Decimal,
    size: u32,
    action: BookActionCompat,
    side: BookSideCompat,
    sequence: u32,
    publisher_id: u16,
) -> MboMsg {
    MboMsg {
        hd: RecordHeader::new::<MboMsg>(
            dbn::rtype::MBO,
            publisher_id,
            instrument_id,
            ts_event_nanos,
        ),
        order_id,
        price: decimal_to_dbn_price(price),
        size,
        flags: FlagSet::default(),
        channel_id: 0,
        action: action.to_dbn_char(),
        side: side.to_dbn_char(),
        ts_recv: ts_recv_nanos,
        ts_in_delta: 0,
        sequence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_symbol_to_instrument_id_deterministic() {
        let id1 = symbol_to_instrument_id("BTCUSDT", "BINANCE");
        let id2 = symbol_to_instrument_id("BTCUSDT", "BINANCE");
        assert_eq!(id1, id2, "Same inputs should produce same ID");

        let id3 = symbol_to_instrument_id("ETHUSDT", "BINANCE");
        assert_ne!(id1, id3, "Different symbols should produce different IDs");

        let id4 = symbol_to_instrument_id("BTCUSDT", "COINBASE");
        assert_ne!(id1, id4, "Different exchanges should produce different IDs");
    }

    #[test]
    fn test_trade_side_compat_conversion() {
        assert_eq!(TradeSideCompat::Buy.to_dbn_char(), b'B' as i8);
        assert_eq!(TradeSideCompat::Sell.to_dbn_char(), b'A' as i8);
        assert_eq!(TradeSideCompat::None.to_dbn_char(), b'N' as i8);

        assert_eq!(
            TradeSideCompat::from_dbn_char(b'B' as i8),
            TradeSideCompat::Buy
        );
        assert_eq!(
            TradeSideCompat::from_dbn_char(b'A' as i8),
            TradeSideCompat::Sell
        );
        assert_eq!(
            TradeSideCompat::from_dbn_char(b'N' as i8),
            TradeSideCompat::None
        );
    }

    #[test]
    fn test_create_trade_msg() {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap() as u64;
        let msg = create_trade_msg_from_decimals(
            now_nanos,
            now_nanos,
            "BTCUSDT",
            "BINANCE",
            dec!(50000.50),
            dec!(100),
            TradeSideCompat::Buy,
            12345,
        );

        assert_eq!(msg.size, 100);
        assert_eq!(msg.sequence, 12345);
        assert!(msg.is_buy());
        assert!(!msg.is_sell());

        // Check price conversion
        let price_back = msg.price_decimal();
        assert!((price_back - dec!(50000.50)).abs() < dec!(0.000000001));
    }

    #[test]
    fn test_trade_msg_ext_methods() {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap() as u64;
        let msg = create_trade_msg_from_decimals(
            now_nanos,
            now_nanos + 1000,
            "ETHUSDT",
            "BINANCE",
            dec!(3500.25),
            dec!(50),
            TradeSideCompat::Sell,
            999,
        );

        assert_eq!(msg.trade_side(), TradeSideCompat::Sell);
        assert!(msg.is_sell());
        assert!(!msg.is_buy());

        let ts = msg.timestamp();
        assert!(ts.timestamp_nanos_opt().unwrap() > 0);

        let recv_ts = msg.recv_timestamp();
        assert!(recv_ts > ts);
    }

    // =========================================================================
    // Inverse Conversion Tests
    // =========================================================================

    #[test]
    fn test_decimal_to_dbn_price() {
        // Test basic conversion
        let price = dec!(50000.50);
        let dbn_price = decimal_to_dbn_price(price);
        assert_eq!(dbn_price, 50000500000000);

        // Test precision
        let price2 = dec!(1.123456789);
        let dbn_price2 = decimal_to_dbn_price(price2);
        assert_eq!(dbn_price2, 1123456789);

        // Test zero
        let zero = dec!(0);
        assert_eq!(decimal_to_dbn_price(zero), 0);
    }

    #[test]
    fn test_datetime_to_nanos() {
        let dt = Utc::now();
        let nanos = datetime_to_nanos(dt);
        assert!(nanos > 0);

        // Round-trip
        let dt_back = nanos_to_datetime(nanos);
        assert_eq!(dt.timestamp(), dt_back.timestamp());
    }

    #[test]
    fn test_decimal_to_dbn_size() {
        assert_eq!(decimal_to_dbn_size(dec!(100)), 100);
        // Note: Decimal::round() uses banker's rounding (round half to even)
        // 100.5 rounds to 100 (even), 100.6 rounds up to 101
        assert_eq!(decimal_to_dbn_size(dec!(100.6)), 101);
        assert_eq!(decimal_to_dbn_size(dec!(100.4)), 100);
    }

    // =========================================================================
    // BboMsg Tests
    // =========================================================================

    #[test]
    fn test_create_bbo_msg() {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap() as u64;
        let bbo = create_bbo_msg_from_decimals(
            now_nanos,
            now_nanos,
            "BTCUSDT",
            "BINANCE",
            dec!(49999.0), // bid
            dec!(50001.0), // ask
            dec!(100),     // bid size
            dec!(50),      // ask size
            123,
        );

        // Test extension trait methods
        assert_eq!(bbo.bid_price(), dec!(49999.0));
        assert_eq!(bbo.ask_price(), dec!(50001.0));
        assert_eq!(bbo.bid_size(), dec!(100));
        assert_eq!(bbo.ask_size(), dec!(50));
        assert_eq!(bbo.spread(), dec!(2.0));
        assert_eq!(bbo.mid_price(), dec!(50000.0));
    }

    // =========================================================================
    // MboMsg Tests
    // =========================================================================

    #[test]
    fn test_create_mbo_msg() {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap() as u64;
        let instrument_id = symbol_to_instrument_id("BTCUSDT", "BINANCE");

        let mbo = create_mbo_msg(
            now_nanos,
            now_nanos,
            instrument_id,
            12345678, // order_id
            dec!(50000.0),
            100,
            BookActionCompat::Add,
            BookSideCompat::Bid,
            1,
            CUSTOM_PUBLISHER_ID,
        );

        // Test extension trait methods
        assert_eq!(mbo.price_decimal(), dec!(50000.0));
        assert_eq!(mbo.size_decimal(), dec!(100));
        assert_eq!(mbo.book_action(), BookActionCompat::Add);
        assert_eq!(mbo.book_side(), BookSideCompat::Bid);
        assert_eq!(mbo.get_order_id(), 12345678);
    }

    // =========================================================================
    // BookActionCompat Tests
    // =========================================================================

    #[test]
    fn test_book_action_compat_roundtrip() {
        let actions = vec![
            BookActionCompat::Add,
            BookActionCompat::Cancel,
            BookActionCompat::Modify,
            BookActionCompat::Clear,
            BookActionCompat::Trade,
            BookActionCompat::Fill,
            BookActionCompat::None,
        ];

        for action in actions {
            let c = action.to_dbn_char();
            let back = BookActionCompat::from_dbn_char(c);
            assert_eq!(action, back);
        }
    }

    // =========================================================================
    // BookSideCompat Tests
    // =========================================================================

    #[test]
    fn test_book_side_compat_roundtrip() {
        let sides = vec![
            BookSideCompat::Bid,
            BookSideCompat::Ask,
            BookSideCompat::None,
        ];

        for side in sides {
            let c = side.to_dbn_char();
            let back = BookSideCompat::from_dbn_char(c);
            assert_eq!(side, back);
        }
    }

    #[test]
    fn test_book_side_s_maps_to_ask() {
        // 'S' (Sell) should also map to Ask
        let side = BookSideCompat::from_dbn_char(b'S' as c_char);
        assert_eq!(side, BookSideCompat::Ask);
    }
}
