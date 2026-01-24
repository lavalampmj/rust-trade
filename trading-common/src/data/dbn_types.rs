//! DBN (Databento Binary Normalized) format types and utilities
//!
//! This module provides the canonical data representation for the entire system.
//! `dbn::TradeMsg` is the standard format for all market data from ingestion
//! through IPC, storage, and strategy execution.
//!
//! Only presentation/frontend layers should convert to display formats.

use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

// Re-export core DBN types as canonical format
pub use dbn::enums::Side as DbnSide;
pub use dbn::record::TradeMsg;
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
/// Convenience function that handles the fixed-point conversion
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
}
