//! Quote tick data types for bid/ask market data.
//!
//! This module provides QuoteTick for representing best bid/ask (L1) quotes
//! from exchanges.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A single quote tick representing the best bid and ask (L1 data).
///
/// QuoteTick captures the top-of-book state at a point in time, including:
/// - Best bid price and size
/// - Best ask price and size
/// - Timestamps for event and receipt
///
/// # Example
///
/// ```ignore
/// use trading_common::data::QuoteTick;
/// use rust_decimal_macros::dec;
/// use chrono::Utc;
///
/// let quote = QuoteTick::new(
///     Utc::now(),
///     "BTCUSDT".to_string(),
///     dec!(50000.00),  // bid price
///     dec!(50001.00),  // ask price
///     dec!(1.5),       // bid size
///     dec!(2.0),       // ask size
/// );
///
/// println!("Spread: {}", quote.spread());
/// println!("Mid price: {}", quote.mid_price());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuoteTick {
    /// Event timestamp (when the quote was generated at the venue)
    pub ts_event: DateTime<Utc>,

    /// Receive timestamp (when we received the quote)
    #[serde(default = "Utc::now")]
    pub ts_recv: DateTime<Utc>,

    /// Trading symbol (e.g., "BTCUSDT")
    pub symbol: String,

    /// Exchange/venue identifier
    #[serde(default = "default_exchange")]
    pub exchange: String,

    /// Best bid price
    pub bid_price: Decimal,

    /// Best ask price
    pub ask_price: Decimal,

    /// Best bid size/quantity
    pub bid_size: Decimal,

    /// Best ask size/quantity
    pub ask_size: Decimal,

    /// Sequence number for ordering
    #[serde(default)]
    pub sequence: u64,
}

fn default_exchange() -> String {
    "UNKNOWN".to_string()
}

impl QuoteTick {
    /// Create a new QuoteTick with required fields
    pub fn new(
        ts_event: DateTime<Utc>,
        symbol: String,
        bid_price: Decimal,
        ask_price: Decimal,
        bid_size: Decimal,
        ask_size: Decimal,
    ) -> Self {
        Self {
            ts_event,
            ts_recv: ts_event,
            symbol,
            exchange: "UNKNOWN".to_string(),
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence: 0,
        }
    }

    /// Create a QuoteTick with all fields
    pub fn with_details(
        ts_event: DateTime<Utc>,
        ts_recv: DateTime<Utc>,
        symbol: String,
        exchange: String,
        bid_price: Decimal,
        ask_price: Decimal,
        bid_size: Decimal,
        ask_size: Decimal,
        sequence: u64,
    ) -> Self {
        Self {
            ts_event,
            ts_recv,
            symbol,
            exchange,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence,
        }
    }

    /// Calculate the bid-ask spread
    pub fn spread(&self) -> Decimal {
        self.ask_price - self.bid_price
    }

    /// Calculate the spread in basis points (spread / mid_price * 10000)
    pub fn spread_bps(&self) -> Decimal {
        let mid = self.mid_price();
        if mid.is_zero() {
            Decimal::ZERO
        } else {
            (self.spread() / mid) * Decimal::new(10000, 0)
        }
    }

    /// Calculate the mid price (average of bid and ask)
    pub fn mid_price(&self) -> Decimal {
        (self.bid_price + self.ask_price) / Decimal::new(2, 0)
    }

    /// Calculate the weighted mid price based on sizes
    ///
    /// Weighted towards the side with more liquidity
    pub fn weighted_mid_price(&self) -> Decimal {
        let total_size = self.bid_size + self.ask_size;
        if total_size.is_zero() {
            self.mid_price()
        } else {
            // Weight inversely - more ask size means price closer to bid
            (self.bid_price * self.ask_size + self.ask_price * self.bid_size) / total_size
        }
    }

    /// Check if the quote is valid (bid < ask, positive sizes)
    pub fn is_valid(&self) -> bool {
        self.bid_price > Decimal::ZERO
            && self.ask_price > Decimal::ZERO
            && self.bid_price < self.ask_price
            && self.bid_size >= Decimal::ZERO
            && self.ask_size >= Decimal::ZERO
    }

    /// Check if the quote is crossed (bid >= ask) - indicates stale data
    pub fn is_crossed(&self) -> bool {
        self.bid_price >= self.ask_price
    }

    /// Check if either side has zero liquidity
    pub fn has_zero_liquidity(&self) -> bool {
        self.bid_size.is_zero() || self.ask_size.is_zero()
    }

    /// Get total liquidity (bid + ask size)
    pub fn total_liquidity(&self) -> Decimal {
        self.bid_size + self.ask_size
    }

    /// Calculate the imbalance ratio: (bid_size - ask_size) / (bid_size + ask_size)
    ///
    /// Returns a value between -1 and 1:
    /// - Positive: More bids than asks (buying pressure)
    /// - Negative: More asks than bids (selling pressure)
    /// - Zero: Balanced
    pub fn imbalance(&self) -> Decimal {
        let total = self.total_liquidity();
        if total.is_zero() {
            Decimal::ZERO
        } else {
            (self.bid_size - self.ask_size) / total
        }
    }

    /// Create a synthetic quote from a trade price (for testing)
    pub fn from_trade_price(
        ts_event: DateTime<Utc>,
        symbol: String,
        price: Decimal,
        spread: Decimal,
    ) -> Self {
        let half_spread = spread / Decimal::new(2, 0);
        Self::new(
            ts_event,
            symbol,
            price - half_spread,
            price + half_spread,
            Decimal::ONE,
            Decimal::ONE,
        )
    }
}

impl fmt::Display for QuoteTick {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} bid: {}@{} ask: {}@{} spread: {}",
            self.ts_event.format("%H:%M:%S%.3f"),
            self.symbol,
            self.bid_size,
            self.bid_price,
            self.ask_size,
            self.ask_price,
            self.spread()
        )
    }
}

/// Aggregated quote statistics over a period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteStats {
    /// Symbol
    pub symbol: String,
    /// Number of quotes processed
    pub count: u64,
    /// Average spread
    pub avg_spread: Decimal,
    /// Minimum spread observed
    pub min_spread: Decimal,
    /// Maximum spread observed
    pub max_spread: Decimal,
    /// Average bid size
    pub avg_bid_size: Decimal,
    /// Average ask size
    pub avg_ask_size: Decimal,
    /// Time-weighted average price (TWAP)
    pub twap: Decimal,
    /// Volume-weighted average price (VWAP) if volume available
    pub vwap: Option<Decimal>,
}

impl QuoteStats {
    /// Create empty stats for a symbol
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            count: 0,
            avg_spread: Decimal::ZERO,
            min_spread: Decimal::MAX,
            max_spread: Decimal::ZERO,
            avg_bid_size: Decimal::ZERO,
            avg_ask_size: Decimal::ZERO,
            twap: Decimal::ZERO,
            vwap: None,
        }
    }
}

/// Builder for accumulating quote statistics.
#[derive(Debug, Clone)]
pub struct QuoteStatsBuilder {
    symbol: String,
    count: u64,
    spread_sum: Decimal,
    min_spread: Decimal,
    max_spread: Decimal,
    bid_size_sum: Decimal,
    ask_size_sum: Decimal,
    mid_price_sum: Decimal,
}

impl QuoteStatsBuilder {
    /// Create a new stats builder
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            count: 0,
            spread_sum: Decimal::ZERO,
            min_spread: Decimal::MAX,
            max_spread: Decimal::ZERO,
            bid_size_sum: Decimal::ZERO,
            ask_size_sum: Decimal::ZERO,
            mid_price_sum: Decimal::ZERO,
        }
    }

    /// Add a quote to the statistics
    pub fn add(&mut self, quote: &QuoteTick) {
        self.count += 1;
        let spread = quote.spread();
        self.spread_sum += spread;
        self.min_spread = self.min_spread.min(spread);
        self.max_spread = self.max_spread.max(spread);
        self.bid_size_sum += quote.bid_size;
        self.ask_size_sum += quote.ask_size;
        self.mid_price_sum += quote.mid_price();
    }

    /// Build the final statistics
    pub fn build(self) -> QuoteStats {
        let count_dec = Decimal::from(self.count);
        QuoteStats {
            symbol: self.symbol,
            count: self.count,
            avg_spread: if self.count > 0 {
                self.spread_sum / count_dec
            } else {
                Decimal::ZERO
            },
            min_spread: if self.count > 0 {
                self.min_spread
            } else {
                Decimal::ZERO
            },
            max_spread: self.max_spread,
            avg_bid_size: if self.count > 0 {
                self.bid_size_sum / count_dec
            } else {
                Decimal::ZERO
            },
            avg_ask_size: if self.count > 0 {
                self.ask_size_sum / count_dec
            } else {
                Decimal::ZERO
            },
            twap: if self.count > 0 {
                self.mid_price_sum / count_dec
            } else {
                Decimal::ZERO
            },
            vwap: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_quote_tick_creation() {
        let quote = QuoteTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(50000.00),
            dec!(50001.00),
            dec!(1.5),
            dec!(2.0),
        );

        assert_eq!(quote.symbol, "BTCUSDT");
        assert_eq!(quote.bid_price, dec!(50000.00));
        assert_eq!(quote.ask_price, dec!(50001.00));
        assert!(quote.is_valid());
    }

    #[test]
    fn test_spread_calculations() {
        let quote = QuoteTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(50000.00),
            dec!(50010.00),
            dec!(1.0),
            dec!(1.0),
        );

        assert_eq!(quote.spread(), dec!(10.00));
        assert_eq!(quote.mid_price(), dec!(50005.00));
        // spread_bps = 10 / 50005 * 10000 ≈ 2
        assert!(quote.spread_bps() > dec!(1.9) && quote.spread_bps() < dec!(2.1));
    }

    #[test]
    fn test_weighted_mid_price() {
        let quote = QuoteTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(100.00),
            dec!(102.00),
            dec!(10.0), // Large bid
            dec!(2.0),  // Small ask
        );

        // More bid size pushes weighted mid closer to ask
        let weighted = quote.weighted_mid_price();
        let mid = quote.mid_price();
        assert!(weighted > mid);
    }

    #[test]
    fn test_imbalance() {
        // More bids
        let quote1 = QuoteTick::new(
            Utc::now(),
            "TEST".to_string(),
            dec!(100),
            dec!(101),
            dec!(10),
            dec!(5),
        );
        assert!(quote1.imbalance() > Decimal::ZERO);

        // More asks
        let quote2 = QuoteTick::new(
            Utc::now(),
            "TEST".to_string(),
            dec!(100),
            dec!(101),
            dec!(5),
            dec!(10),
        );
        assert!(quote2.imbalance() < Decimal::ZERO);

        // Balanced
        let quote3 = QuoteTick::new(
            Utc::now(),
            "TEST".to_string(),
            dec!(100),
            dec!(101),
            dec!(5),
            dec!(5),
        );
        assert_eq!(quote3.imbalance(), Decimal::ZERO);
    }

    #[test]
    fn test_invalid_quotes() {
        // Crossed quote
        let crossed = QuoteTick::new(
            Utc::now(),
            "TEST".to_string(),
            dec!(101),
            dec!(100),
            dec!(1),
            dec!(1),
        );
        assert!(!crossed.is_valid());
        assert!(crossed.is_crossed());

        // Zero liquidity
        let no_liquidity = QuoteTick::new(
            Utc::now(),
            "TEST".to_string(),
            dec!(100),
            dec!(101),
            dec!(0),
            dec!(1),
        );
        assert!(no_liquidity.has_zero_liquidity());
    }

    #[test]
    fn test_from_trade_price() {
        let quote = QuoteTick::from_trade_price(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(50000),
            dec!(10), // 10 spread
        );

        assert_eq!(quote.bid_price, dec!(49995));
        assert_eq!(quote.ask_price, dec!(50005));
        assert_eq!(quote.spread(), dec!(10));
    }

    #[test]
    fn test_quote_stats_builder() {
        let mut builder = QuoteStatsBuilder::new("BTCUSDT");

        builder.add(&QuoteTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(100),
            dec!(101),
            dec!(10),
            dec!(5),
        ));
        builder.add(&QuoteTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            dec!(100),
            dec!(102),
            dec!(8),
            dec!(6),
        ));

        let stats = builder.build();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.min_spread, dec!(1));
        assert_eq!(stats.max_spread, dec!(2));
        assert_eq!(stats.avg_spread, dec!(1.5));
    }
}
