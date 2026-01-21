//! Subscription filters
//!
//! Provides filtering capabilities for subscriptions, allowing consumers
//! to receive only data matching specific criteria.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::schema::{NormalizedTick, TradeSide};
use crate::symbol::SymbolSpec;

/// Filter for subscription data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionFilter {
    /// Symbol filters
    pub symbols: Option<SymbolFilter>,
    /// Price filters
    pub price: Option<PriceFilter>,
    /// Size filters
    pub size: Option<SizeFilter>,
    /// Side filter
    pub side: Option<TradeSide>,
    /// Time filter
    pub time: Option<TimeFilter>,
}

impl SubscriptionFilter {
    /// Create a new empty filter (matches everything)
    pub fn new() -> Self {
        Self {
            symbols: None,
            price: None,
            size: None,
            side: None,
            time: None,
        }
    }

    /// Filter by symbols
    pub fn with_symbols(mut self, symbols: Vec<SymbolSpec>) -> Self {
        self.symbols = Some(SymbolFilter::Include(symbols));
        self
    }

    /// Exclude symbols
    pub fn excluding_symbols(mut self, symbols: Vec<SymbolSpec>) -> Self {
        self.symbols = Some(SymbolFilter::Exclude(symbols));
        self
    }

    /// Filter by minimum price
    pub fn with_min_price(mut self, min: Decimal) -> Self {
        self.price = Some(PriceFilter::Min(min));
        self
    }

    /// Filter by price range
    pub fn with_price_range(mut self, min: Decimal, max: Decimal) -> Self {
        self.price = Some(PriceFilter::Range { min, max });
        self
    }

    /// Filter by minimum size
    pub fn with_min_size(mut self, min: Decimal) -> Self {
        self.size = Some(SizeFilter::Min(min));
        self
    }

    /// Filter by trade side
    pub fn with_side(mut self, side: TradeSide) -> Self {
        self.side = Some(side);
        self
    }

    /// Check if a tick matches this filter
    pub fn matches(&self, tick: &NormalizedTick) -> bool {
        // Check symbol filter
        if let Some(ref symbol_filter) = self.symbols {
            let spec = SymbolSpec::new(&tick.symbol, &tick.exchange);
            match symbol_filter {
                SymbolFilter::Include(symbols) => {
                    if !symbols.contains(&spec) {
                        return false;
                    }
                }
                SymbolFilter::Exclude(symbols) => {
                    if symbols.contains(&spec) {
                        return false;
                    }
                }
            }
        }

        // Check price filter
        if let Some(ref price_filter) = self.price {
            match price_filter {
                PriceFilter::Min(min) => {
                    if tick.price < *min {
                        return false;
                    }
                }
                PriceFilter::Max(max) => {
                    if tick.price > *max {
                        return false;
                    }
                }
                PriceFilter::Range { min, max } => {
                    if tick.price < *min || tick.price > *max {
                        return false;
                    }
                }
            }
        }

        // Check size filter
        if let Some(ref size_filter) = self.size {
            match size_filter {
                SizeFilter::Min(min) => {
                    if tick.size < *min {
                        return false;
                    }
                }
                SizeFilter::Max(max) => {
                    if tick.size > *max {
                        return false;
                    }
                }
                SizeFilter::Range { min, max } => {
                    if tick.size < *min || tick.size > *max {
                        return false;
                    }
                }
            }
        }

        // Check side filter
        if let Some(side) = self.side {
            if tick.side != side {
                return false;
            }
        }

        // Check time filter
        if let Some(ref time_filter) = self.time {
            match time_filter {
                TimeFilter::After(ts) => {
                    if tick.ts_event < *ts {
                        return false;
                    }
                }
                TimeFilter::Before(ts) => {
                    if tick.ts_event > *ts {
                        return false;
                    }
                }
                TimeFilter::Range { start, end } => {
                    if tick.ts_event < *start || tick.ts_event > *end {
                        return false;
                    }
                }
            }
        }

        true
    }
}

impl Default for SubscriptionFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Symbol filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SymbolFilter {
    /// Include only these symbols
    Include(Vec<SymbolSpec>),
    /// Exclude these symbols
    Exclude(Vec<SymbolSpec>),
}

/// Price filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PriceFilter {
    /// Minimum price
    Min(Decimal),
    /// Maximum price
    Max(Decimal),
    /// Price range
    Range { min: Decimal, max: Decimal },
}

/// Size filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SizeFilter {
    /// Minimum size
    Min(Decimal),
    /// Maximum size
    Max(Decimal),
    /// Size range
    Range { min: Decimal, max: Decimal },
}

/// Time filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimeFilter {
    /// After timestamp
    After(DateTime<Utc>),
    /// Before timestamp
    Before(DateTime<Utc>),
    /// Time range
    Range {
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn create_test_tick(price: Decimal, size: Decimal, side: TradeSide) -> NormalizedTick {
        NormalizedTick::new(
            Utc::now(),
            "ES".to_string(),
            "CME".to_string(),
            price,
            size,
            side,
            "test".to_string(),
            1,
        )
    }

    #[test]
    fn test_empty_filter() {
        let filter = SubscriptionFilter::new();
        let tick = create_test_tick(dec!(5000), dec!(10), TradeSide::Buy);
        assert!(filter.matches(&tick));
    }

    #[test]
    fn test_price_filter() {
        let filter = SubscriptionFilter::new().with_min_price(dec!(5000));

        let tick1 = create_test_tick(dec!(5000), dec!(10), TradeSide::Buy);
        let tick2 = create_test_tick(dec!(4999), dec!(10), TradeSide::Buy);

        assert!(filter.matches(&tick1));
        assert!(!filter.matches(&tick2));
    }

    #[test]
    fn test_size_filter() {
        let filter = SubscriptionFilter::new().with_min_size(dec!(100));

        let tick1 = create_test_tick(dec!(5000), dec!(100), TradeSide::Buy);
        let tick2 = create_test_tick(dec!(5000), dec!(50), TradeSide::Buy);

        assert!(filter.matches(&tick1));
        assert!(!filter.matches(&tick2));
    }

    #[test]
    fn test_side_filter() {
        let filter = SubscriptionFilter::new().with_side(TradeSide::Buy);

        let tick1 = create_test_tick(dec!(5000), dec!(10), TradeSide::Buy);
        let tick2 = create_test_tick(dec!(5000), dec!(10), TradeSide::Sell);

        assert!(filter.matches(&tick1));
        assert!(!filter.matches(&tick2));
    }

    #[test]
    fn test_combined_filters() {
        let filter = SubscriptionFilter::new()
            .with_min_price(dec!(5000))
            .with_min_size(dec!(100))
            .with_side(TradeSide::Buy);

        let tick1 = create_test_tick(dec!(5100), dec!(150), TradeSide::Buy);
        let tick2 = create_test_tick(dec!(4900), dec!(150), TradeSide::Buy); // price too low
        let tick3 = create_test_tick(dec!(5100), dec!(50), TradeSide::Buy); // size too low
        let tick4 = create_test_tick(dec!(5100), dec!(150), TradeSide::Sell); // wrong side

        assert!(filter.matches(&tick1));
        assert!(!filter.matches(&tick2));
        assert!(!filter.matches(&tick3));
        assert!(!filter.matches(&tick4));
    }
}
