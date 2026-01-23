// Price Source Selection for Transforms
//
// Allows transforms to read from different OHLCV series instead of
// being hardcoded to close prices.

use crate::series::{BarsContext, Series};
use rust_decimal::Decimal;

/// Selects which price series a transform should read from.
///
/// By default, most transforms use `Close` prices, but this enum
/// allows selecting any OHLCV component.
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Sma, PriceSource};
///
/// // SMA of close prices (default)
/// let sma_close = Sma::new(20);
///
/// // SMA of high prices
/// let sma_high = Sma::with_source(20, PriceSource::High);
///
/// // SMA of typical price (custom)
/// let sma_typical = Sma::with_source(20, PriceSource::Typical);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PriceSource {
    /// Open price
    Open,
    /// High price
    High,
    /// Low price
    Low,
    /// Close price (default for most indicators)
    #[default]
    Close,
    /// Volume
    Volume,
    /// Typical Price: (High + Low + Close) / 3
    Typical,
    /// Weighted Close: (High + Low + Close + Close) / 4
    WeightedClose,
    /// Median Price: (High + Low) / 2
    Median,
}

impl PriceSource {
    /// Get the value from BarsContext for the current bar
    pub fn get_current(&self, bars: &BarsContext) -> Option<Decimal> {
        match self {
            PriceSource::Open => bars.open.current().copied(),
            PriceSource::High => bars.high.current().copied(),
            PriceSource::Low => bars.low.current().copied(),
            PriceSource::Close => bars.close.current().copied(),
            PriceSource::Volume => bars.volume.current().copied(),
            PriceSource::Typical => {
                let h = bars.high.current()?;
                let l = bars.low.current()?;
                let c = bars.close.current()?;
                Some((*h + *l + *c) / Decimal::from(3))
            }
            PriceSource::WeightedClose => {
                let h = bars.high.current()?;
                let l = bars.low.current()?;
                let c = bars.close.current()?;
                Some((*h + *l + *c + *c) / Decimal::from(4))
            }
            PriceSource::Median => {
                let h = bars.high.current()?;
                let l = bars.low.current()?;
                Some((*h + *l) / Decimal::from(2))
            }
        }
    }

    /// Get the value at bars_ago offset
    pub fn get(&self, bars: &BarsContext, bars_ago: usize) -> Option<Decimal> {
        match self {
            PriceSource::Open => bars.open.get(bars_ago).copied(),
            PriceSource::High => bars.high.get(bars_ago).copied(),
            PriceSource::Low => bars.low.get(bars_ago).copied(),
            PriceSource::Close => bars.close.get(bars_ago).copied(),
            PriceSource::Volume => bars.volume.get(bars_ago).copied(),
            PriceSource::Typical => {
                let h = bars.high.get(bars_ago)?;
                let l = bars.low.get(bars_ago)?;
                let c = bars.close.get(bars_ago)?;
                Some((*h + *l + *c) / Decimal::from(3))
            }
            PriceSource::WeightedClose => {
                let h = bars.high.get(bars_ago)?;
                let l = bars.low.get(bars_ago)?;
                let c = bars.close.get(bars_ago)?;
                Some((*h + *l + *c + *c) / Decimal::from(4))
            }
            PriceSource::Median => {
                let h = bars.high.get(bars_ago)?;
                let l = bars.low.get(bars_ago)?;
                Some((*h + *l) / Decimal::from(2))
            }
        }
    }

    /// Get the underlying series reference (only for simple sources)
    ///
    /// Returns None for computed sources like Typical, WeightedClose, Median
    pub fn get_series<'a>(&self, bars: &'a BarsContext) -> Option<&'a Series<Decimal>> {
        match self {
            PriceSource::Open => Some(&bars.open),
            PriceSource::High => Some(&bars.high),
            PriceSource::Low => Some(&bars.low),
            PriceSource::Close => Some(&bars.close),
            PriceSource::Volume => Some(&bars.volume),
            // Computed sources don't have a direct series
            PriceSource::Typical | PriceSource::WeightedClose | PriceSource::Median => None,
        }
    }

    /// Calculate sum over a period (for SMA calculation)
    pub fn sum(&self, bars: &BarsContext, period: usize) -> Option<Decimal> {
        if bars.count() < period {
            return None;
        }

        let mut sum = Decimal::ZERO;
        for i in 0..period {
            sum += self.get(bars, i)?;
        }
        Some(sum)
    }

    /// Calculate SMA over a period
    pub fn sma(&self, bars: &BarsContext, period: usize) -> Option<Decimal> {
        let sum = self.sum(bars, period)?;
        Some(sum / Decimal::from(period))
    }

    /// Get highest value over a period
    pub fn highest(&self, bars: &BarsContext, period: usize) -> Option<Decimal> {
        if bars.count() < period {
            return None;
        }

        let mut max = self.get(bars, 0)?;
        for i in 1..period {
            let val = self.get(bars, i)?;
            if val > max {
                max = val;
            }
        }
        Some(max)
    }

    /// Get lowest value over a period
    pub fn lowest(&self, bars: &BarsContext, period: usize) -> Option<Decimal> {
        if bars.count() < period {
            return None;
        }

        let mut min = self.get(bars, 0)?;
        for i in 1..period {
            let val = self.get(bars, i)?;
            if val < min {
                min = val;
            }
        }
        Some(min)
    }

    /// Get short name for display
    pub fn as_str(&self) -> &'static str {
        match self {
            PriceSource::Open => "open",
            PriceSource::High => "high",
            PriceSource::Low => "low",
            PriceSource::Close => "close",
            PriceSource::Volume => "volume",
            PriceSource::Typical => "typical",
            PriceSource::WeightedClose => "wclose",
            PriceSource::Median => "median",
        }
    }
}

impl std::fmt::Display for PriceSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarData, OHLCData, Timeframe};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn create_bar(open: Decimal, high: Decimal, low: Decimal, close: Decimal) -> BarData {
        let ohlc = OHLCData::new(
            Utc::now(),
            "TEST".to_string(),
            Timeframe::OneMinute,
            open,
            high,
            low,
            close,
            dec!(100),
            1,
        );
        BarData::from_ohlc(&ohlc)
    }

    fn create_test_bars() -> BarsContext {
        let mut ctx = BarsContext::new("TEST");
        // Bar 1: O=100, H=110, L=90, C=105
        ctx.on_bar_update(&create_bar(dec!(100), dec!(110), dec!(90), dec!(105)));
        // Bar 2: O=105, H=120, L=100, C=115
        ctx.on_bar_update(&create_bar(dec!(105), dec!(120), dec!(100), dec!(115)));
        // Bar 3: O=115, H=125, L=110, C=120
        ctx.on_bar_update(&create_bar(dec!(115), dec!(125), dec!(110), dec!(120)));
        ctx
    }

    #[test]
    fn test_price_source_basic() {
        let bars = create_test_bars();

        assert_eq!(PriceSource::Open.get_current(&bars), Some(dec!(115)));
        assert_eq!(PriceSource::High.get_current(&bars), Some(dec!(125)));
        assert_eq!(PriceSource::Low.get_current(&bars), Some(dec!(110)));
        assert_eq!(PriceSource::Close.get_current(&bars), Some(dec!(120)));
    }

    #[test]
    fn test_price_source_typical() {
        let bars = create_test_bars();

        // Typical = (H + L + C) / 3 = (125 + 110 + 120) / 3 = 118.333...
        let typical = PriceSource::Typical.get_current(&bars).unwrap();
        assert!(typical > dec!(118) && typical < dec!(119));
    }

    #[test]
    fn test_price_source_median() {
        let bars = create_test_bars();

        // Median = (H + L) / 2 = (125 + 110) / 2 = 117.5
        let median = PriceSource::Median.get_current(&bars).unwrap();
        assert_eq!(median, dec!(117.5));
    }

    #[test]
    fn test_price_source_weighted_close() {
        let bars = create_test_bars();

        // WeightedClose = (H + L + C + C) / 4 = (125 + 110 + 120 + 120) / 4 = 118.75
        let wclose = PriceSource::WeightedClose.get_current(&bars).unwrap();
        assert_eq!(wclose, dec!(118.75));
    }

    #[test]
    fn test_price_source_bars_ago() {
        let bars = create_test_bars();

        // Current bar (bars_ago=0)
        assert_eq!(PriceSource::Close.get(&bars, 0), Some(dec!(120)));
        // Previous bar (bars_ago=1)
        assert_eq!(PriceSource::Close.get(&bars, 1), Some(dec!(115)));
        // Two bars ago (bars_ago=2)
        assert_eq!(PriceSource::Close.get(&bars, 2), Some(dec!(105)));
    }

    #[test]
    fn test_price_source_sma() {
        let bars = create_test_bars();

        // SMA(3) of close = (120 + 115 + 105) / 3 = 113.333...
        let sma = PriceSource::Close.sma(&bars, 3).unwrap();
        assert!(sma > dec!(113) && sma < dec!(114));
    }

    #[test]
    fn test_price_source_highest_lowest() {
        let bars = create_test_bars();

        // Highest high over 3 bars = max(125, 120, 110) = 125
        assert_eq!(PriceSource::High.highest(&bars, 3), Some(dec!(125)));

        // Lowest low over 3 bars = min(110, 100, 90) = 90
        assert_eq!(PriceSource::Low.lowest(&bars, 3), Some(dec!(90)));
    }

    #[test]
    fn test_price_source_get_series() {
        let bars = create_test_bars();

        // Simple sources return the series
        assert!(PriceSource::Close.get_series(&bars).is_some());
        assert!(PriceSource::Open.get_series(&bars).is_some());

        // Computed sources return None
        assert!(PriceSource::Typical.get_series(&bars).is_none());
        assert!(PriceSource::Median.get_series(&bars).is_none());
    }

    #[test]
    fn test_price_source_default() {
        let source = PriceSource::default();
        assert_eq!(source, PriceSource::Close);
    }

    #[test]
    fn test_price_source_display() {
        assert_eq!(PriceSource::Close.to_string(), "close");
        assert_eq!(PriceSource::Typical.to_string(), "typical");
    }
}
