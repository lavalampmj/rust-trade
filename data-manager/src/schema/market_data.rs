//! Normalized market data types
//!
//! These types represent the canonical format for all market data in the system.
//! Provider-specific data is normalized to these types before storage or distribution.
//!
//! For IPC transport, NormalizedTick can be converted to dbn::TradeMsg (48 bytes).

use chrono::{DateTime, Utc};
use dbn::TradeMsg;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use trading_common::data::{create_trade_msg_from_decimals, TradeSideCompat};

/// Trade side (buy or sell)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

impl TradeSide {
    /// Convert to single character representation ('B' or 'S')
    pub fn as_char(&self) -> char {
        match self {
            TradeSide::Buy => 'B',
            TradeSide::Sell => 'S',
        }
    }

    /// Parse from single character
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'B' | 'b' => Some(TradeSide::Buy),
            'S' | 's' => Some(TradeSide::Sell),
            _ => None,
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "BUY" | "B" => Some(TradeSide::Buy),
            "SELL" | "S" => Some(TradeSide::Sell),
            _ => None,
        }
    }
}

impl std::fmt::Display for TradeSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TradeSide::Buy => write!(f, "BUY"),
            TradeSide::Sell => write!(f, "SELL"),
        }
    }
}

impl From<TradeSide> for TradeSideCompat {
    fn from(side: TradeSide) -> Self {
        match side {
            TradeSide::Buy => TradeSideCompat::Buy,
            TradeSide::Sell => TradeSideCompat::Sell,
        }
    }
}

/// Normalized tick (trade) data
///
/// This is the canonical representation of a single trade event.
/// All provider-specific trade data is normalized to this format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedTick {
    /// Event timestamp (when the trade occurred at the exchange)
    pub ts_event: DateTime<Utc>,
    /// Receive timestamp (when we received the data)
    pub ts_recv: DateTime<Utc>,
    /// Symbol in Databento format (canonical symbology)
    pub symbol: String,
    /// Exchange identifier
    pub exchange: String,
    /// Trade price
    pub price: Decimal,
    /// Trade size/quantity
    pub size: Decimal,
    /// Trade side (aggressor side)
    pub side: TradeSide,
    /// Data provider identifier
    pub provider: String,
    /// Provider-specific trade ID (optional)
    pub provider_trade_id: Option<String>,
    /// Whether the buyer is the market maker
    pub is_buyer_maker: Option<bool>,
    /// Raw DBN message bytes for archival (optional)
    pub raw_dbn: Option<Vec<u8>>,
    /// Sequence number for ordering
    pub sequence: i64,
}

impl NormalizedTick {
    /// Create a new NormalizedTick
    pub fn new(
        ts_event: DateTime<Utc>,
        symbol: String,
        exchange: String,
        price: Decimal,
        size: Decimal,
        side: TradeSide,
        provider: String,
        sequence: i64,
    ) -> Self {
        Self {
            ts_event,
            ts_recv: Utc::now(),
            symbol,
            exchange,
            price,
            size,
            side,
            provider,
            provider_trade_id: None,
            is_buyer_maker: None,
            raw_dbn: None,
            sequence,
        }
    }

    /// Create with full details
    pub fn with_details(
        ts_event: DateTime<Utc>,
        ts_recv: DateTime<Utc>,
        symbol: String,
        exchange: String,
        price: Decimal,
        size: Decimal,
        side: TradeSide,
        provider: String,
        provider_trade_id: Option<String>,
        is_buyer_maker: Option<bool>,
        sequence: i64,
    ) -> Self {
        Self {
            ts_event,
            ts_recv,
            symbol,
            exchange,
            price,
            size,
            side,
            provider,
            provider_trade_id,
            is_buyer_maker,
            raw_dbn: None,
            sequence,
        }
    }

    /// Get the full symbol identifier (symbol@exchange)
    pub fn full_symbol(&self) -> String {
        format!("{}@{}", self.symbol, self.exchange)
    }

    /// Convert to DBN TradeMsg format for IPC transport
    ///
    /// TradeMsg is a compact 48-byte representation suitable for
    /// high-performance shared memory communication.
    pub fn to_trade_msg(&self) -> TradeMsg {
        let ts_event_nanos = self.ts_event.timestamp_nanos_opt().unwrap_or(0) as u64;
        let ts_recv_nanos = self.ts_recv.timestamp_nanos_opt().unwrap_or(0) as u64;

        create_trade_msg_from_decimals(
            ts_event_nanos,
            ts_recv_nanos,
            &self.symbol,
            &self.exchange,
            self.price,
            self.size,
            self.side.into(),
            self.sequence as u32,
        )
    }
}

/// Normalized OHLC bar data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedOHLC {
    /// Bar timestamp (start of the period)
    pub timestamp: DateTime<Utc>,
    /// Symbol in canonical format
    pub symbol: String,
    /// Exchange identifier
    pub exchange: String,
    /// Timeframe string (e.g., "1m", "5m", "1h", "1d")
    pub timeframe: String,
    /// Open price
    pub open: Decimal,
    /// High price
    pub high: Decimal,
    /// Low price
    pub low: Decimal,
    /// Close price
    pub close: Decimal,
    /// Total volume traded
    pub volume: Decimal,
    /// Number of trades in the bar
    pub trade_count: u64,
    /// Data provider identifier
    pub provider: String,
}

impl NormalizedOHLC {
    /// Create a new NormalizedOHLC
    pub fn new(
        timestamp: DateTime<Utc>,
        symbol: String,
        exchange: String,
        timeframe: String,
        open: Decimal,
        high: Decimal,
        low: Decimal,
        close: Decimal,
        volume: Decimal,
        trade_count: u64,
        provider: String,
    ) -> Self {
        Self {
            timestamp,
            symbol,
            exchange,
            timeframe,
            open,
            high,
            low,
            close,
            volume,
            trade_count,
            provider,
        }
    }

    /// Get the full symbol identifier (symbol@exchange)
    pub fn full_symbol(&self) -> String {
        format!("{}@{}", self.symbol, self.exchange)
    }
}

/// Compact binary representation of a tick for IPC transport
///
/// Fixed-size structure for lock-free ring buffer transmission.
/// Total size: 128 bytes (fits in 2 cache lines)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CompactTick {
    /// Sequence number (8 bytes)
    pub sequence: i64,
    /// Event timestamp as Unix nanoseconds (8 bytes)
    pub ts_event_nanos: i64,
    /// Receive timestamp as Unix nanoseconds (8 bytes)
    pub ts_recv_nanos: i64,
    /// Price as fixed-point integer (price * 10^8) (8 bytes)
    pub price_fixed: i64,
    /// Size as fixed-point integer (size * 10^8) (8 bytes)
    pub size_fixed: i64,
    /// Symbol hash for fast filtering (8 bytes)
    pub symbol_hash: u64,
    /// Side: 0 = Buy, 1 = Sell (1 byte)
    pub side: u8,
    /// Flags (1 byte): bit 0 = is_buyer_maker, bits 1-7 reserved
    pub flags: u8,
    /// Reserved for alignment (14 bytes to reach 128 bytes total)
    _reserved: [u8; 14],
    /// Symbol name (32 bytes, null-terminated)
    pub symbol: [u8; 32],
    /// Exchange name (16 bytes, null-terminated)
    pub exchange: [u8; 16],
    /// Provider name (16 bytes, null-terminated)
    pub provider: [u8; 16],
}

// SAFETY: CompactTick is repr(C), all fields are Pod-compatible, and there is no padding
unsafe impl bytemuck::Zeroable for CompactTick {}
unsafe impl bytemuck::Pod for CompactTick {}

impl CompactTick {
    /// Price scale factor for fixed-point conversion (10^8)
    pub const PRICE_SCALE: i64 = 100_000_000;

    /// Create a new CompactTick from a NormalizedTick
    pub fn from_normalized(tick: &NormalizedTick) -> Self {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;

        let mut hasher = DefaultHasher::new();
        tick.symbol.hash(&mut hasher);
        tick.exchange.hash(&mut hasher);
        let symbol_hash = hasher.finish();

        let mut symbol = [0u8; 32];
        let symbol_bytes = tick.symbol.as_bytes();
        let len = symbol_bytes.len().min(31);
        symbol[..len].copy_from_slice(&symbol_bytes[..len]);

        let mut exchange = [0u8; 16];
        let exchange_bytes = tick.exchange.as_bytes();
        let len = exchange_bytes.len().min(15);
        exchange[..len].copy_from_slice(&exchange_bytes[..len]);

        let mut provider = [0u8; 16];
        let provider_bytes = tick.provider.as_bytes();
        let len = provider_bytes.len().min(15);
        provider[..len].copy_from_slice(&provider_bytes[..len]);

        // Convert price and size to fixed-point
        use rust_decimal::prelude::ToPrimitive;
        let price_fixed = tick.price
            .checked_mul(Decimal::from(Self::PRICE_SCALE))
            .and_then(|d| d.trunc().to_i64())
            .unwrap_or(0);
        let size_fixed = tick.size
            .checked_mul(Decimal::from(Self::PRICE_SCALE))
            .and_then(|d| d.trunc().to_i64())
            .unwrap_or(0);

        let mut flags = 0u8;
        if tick.is_buyer_maker.unwrap_or(false) {
            flags |= 0x01;
        }

        CompactTick {
            sequence: tick.sequence,
            ts_event_nanos: tick.ts_event.timestamp_nanos_opt().unwrap_or(0),
            ts_recv_nanos: tick.ts_recv.timestamp_nanos_opt().unwrap_or(0),
            price_fixed,
            size_fixed,
            symbol_hash,
            side: if tick.side == TradeSide::Buy { 0 } else { 1 },
            flags,
            _reserved: [0; 14],
            symbol,
            exchange,
            provider,
        }
    }

    /// Convert back to NormalizedTick
    pub fn to_normalized(&self) -> NormalizedTick {
        use rust_decimal::prelude::FromPrimitive;

        let symbol = String::from_utf8_lossy(&self.symbol)
            .trim_end_matches('\0')
            .to_string();
        let exchange = String::from_utf8_lossy(&self.exchange)
            .trim_end_matches('\0')
            .to_string();
        let provider = String::from_utf8_lossy(&self.provider)
            .trim_end_matches('\0')
            .to_string();

        let price = Decimal::from_i64(self.price_fixed)
            .map(|d| d / Decimal::from(Self::PRICE_SCALE))
            .unwrap_or(Decimal::ZERO);
        let size = Decimal::from_i64(self.size_fixed)
            .map(|d| d / Decimal::from(Self::PRICE_SCALE))
            .unwrap_or(Decimal::ZERO);

        let ts_event = DateTime::from_timestamp_nanos(self.ts_event_nanos);
        let ts_recv = DateTime::from_timestamp_nanos(self.ts_recv_nanos);

        NormalizedTick {
            ts_event,
            ts_recv,
            symbol,
            exchange,
            price,
            size,
            side: if self.side == 0 { TradeSide::Buy } else { TradeSide::Sell },
            provider,
            provider_trade_id: None,
            is_buyer_maker: Some(self.flags & 0x01 != 0),
            raw_dbn: None,
            sequence: self.sequence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_trade_side_conversions() {
        assert_eq!(TradeSide::Buy.as_char(), 'B');
        assert_eq!(TradeSide::Sell.as_char(), 'S');
        assert_eq!(TradeSide::from_char('B'), Some(TradeSide::Buy));
        assert_eq!(TradeSide::from_char('s'), Some(TradeSide::Sell));
        assert_eq!(TradeSide::from_str("buy"), Some(TradeSide::Buy));
        assert_eq!(TradeSide::from_str("SELL"), Some(TradeSide::Sell));
    }

    #[test]
    fn test_normalized_tick_creation() {
        let tick = NormalizedTick::new(
            Utc::now(),
            "ESH5".to_string(),
            "CME".to_string(),
            dec!(5025.50),
            dec!(10),
            TradeSide::Buy,
            "databento".to_string(),
            1,
        );

        assert_eq!(tick.symbol, "ESH5");
        assert_eq!(tick.exchange, "CME");
        assert_eq!(tick.full_symbol(), "ESH5@CME");
    }

    #[test]
    fn test_compact_tick_roundtrip() {
        let tick = NormalizedTick::with_details(
            Utc::now(),
            Utc::now(),
            "ESH5".to_string(),
            "CME".to_string(),
            dec!(5025.50),
            dec!(10.5),
            TradeSide::Sell,
            "databento".to_string(),
            Some("trade123".to_string()),
            Some(true),
            42,
        );

        let compact = CompactTick::from_normalized(&tick);
        let restored = compact.to_normalized();

        assert_eq!(restored.symbol, tick.symbol);
        assert_eq!(restored.exchange, tick.exchange);
        assert_eq!(restored.side, tick.side);
        assert_eq!(restored.sequence, tick.sequence);
        // Price/size may have slight precision differences due to fixed-point conversion
        assert!((restored.price - tick.price).abs() < dec!(0.00000001));
    }

    #[test]
    fn test_compact_tick_size() {
        // Ensure CompactTick is the expected size (128 bytes)
        assert_eq!(std::mem::size_of::<CompactTick>(), 128);
    }
}
