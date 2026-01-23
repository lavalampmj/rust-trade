// Highest/Lowest Transforms (Rolling Extremes)

use crate::series::{BarsContext, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::source::PriceSource;
use super::Transform;

/// Highest value transform.
///
/// Calculates the highest price over the last N bars.
/// By default uses close prices, but can be configured to use any price source.
/// Also known as "Highest High" when applied to high prices.
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, Highest, PriceSource};
///
/// // Highest close (default)
/// let mut highest = Highest::new(20);
///
/// // Highest high (classic Donchian channel)
/// let mut hh = Highest::with_source(20, PriceSource::High);
///
/// if let Some(resistance) = highest.compute(&bars) {
///     // resistance is the highest price of last 20 bars
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Highest {
    period: usize,
    source: PriceSource,
    name: String,
    output: Series<Decimal>,
}

impl Highest {
    /// Create a new Highest transform with the specified period.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Lookback period
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new Highest transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Lookback period
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "Highest period must be > 0");
        let name = if source == PriceSource::Close {
            format!("highest_{}", period)
        } else {
            format!("highest_{}_{}", period, source)
        };
        Self {
            period,
            source,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), period),
        }
    }

    /// Get the period
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get the price source
    pub fn source(&self) -> PriceSource {
        self.source
    }
}

impl Transform for Highest {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        let value = self.source.highest(bars, self.period)?;
        self.output.push(value);
        Some(value)
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.output.reset();
    }
}

/// Lowest value transform.
///
/// Calculates the lowest price over the last N bars.
/// By default uses close prices, but can be configured to use any price source.
/// Also known as "Lowest Low" when applied to low prices.
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, Lowest, PriceSource};
///
/// // Lowest close (default)
/// let mut lowest = Lowest::new(20);
///
/// // Lowest low (classic Donchian channel)
/// let mut ll = Lowest::with_source(20, PriceSource::Low);
///
/// if let Some(support) = lowest.compute(&bars) {
///     // support is the lowest price of last 20 bars
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Lowest {
    period: usize,
    source: PriceSource,
    name: String,
    output: Series<Decimal>,
}

impl Lowest {
    /// Create a new Lowest transform with the specified period.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Lookback period
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new Lowest transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Lookback period
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "Lowest period must be > 0");
        let name = if source == PriceSource::Close {
            format!("lowest_{}", period)
        } else {
            format!("lowest_{}_{}", period, source)
        };
        Self {
            period,
            source,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), period),
        }
    }

    /// Get the period
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get the price source
    pub fn source(&self) -> PriceSource {
        self.source
    }
}

impl Transform for Lowest {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        let value = self.source.lowest(bars, self.period)?;
        self.output.push(value);
        Some(value)
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.output.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarData, OHLCData, Timeframe};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn create_bars_with_closes(closes: &[Decimal]) -> BarsContext {
        let mut ctx = BarsContext::new("TEST");
        for close in closes {
            let ohlc = OHLCData::new(
                Utc::now(),
                "TEST".to_string(),
                Timeframe::OneMinute,
                *close,
                *close,
                *close,
                *close,
                dec!(100),
                1,
            );
            ctx.on_bar_update(&BarData::from_ohlc(&ohlc));
        }
        ctx
    }

    // ========== Highest Tests ==========

    #[test]
    fn test_highest_basic() {
        let closes = vec![dec!(10), dec!(50), dec!(30), dec!(20), dec!(40)];
        let ctx = create_bars_with_closes(&closes);

        let mut highest = Highest::new(3);
        let value = highest.compute(&ctx);

        // Highest of last 3 [30, 20, 40] = 40
        assert_eq!(value, Some(dec!(40)));
    }

    #[test]
    fn test_highest_insufficient_data() {
        let closes = vec![dec!(10), dec!(20)];
        let ctx = create_bars_with_closes(&closes);

        let mut highest = Highest::new(5);
        assert_eq!(highest.compute(&ctx), None);
    }

    #[test]
    fn test_highest_exact_period() {
        let closes = vec![dec!(30), dec!(10), dec!(50)];
        let ctx = create_bars_with_closes(&closes);

        let mut highest = Highest::new(3);
        let value = highest.compute(&ctx);

        // Highest of [30, 10, 50] = 50
        assert_eq!(value, Some(dec!(50)));
    }

    #[test]
    fn test_highest_warmup_period() {
        let highest = Highest::new(20);
        assert_eq!(highest.warmup_period(), 20);
    }

    #[test]
    fn test_highest_name() {
        let highest = Highest::new(14);
        assert_eq!(highest.name(), "highest_14");
    }

    #[test]
    fn test_highest_reset() {
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut highest = Highest::new(3);
        highest.compute(&ctx);

        assert!(highest.current().is_some());

        highest.reset();

        assert!(highest.current().is_none());
        assert_eq!(highest.output().count(), 0);
    }

    #[test]
    fn test_highest_historical_access() {
        let mut highest = Highest::new(3);

        for n in 3..=6 {
            let closes: Vec<Decimal> = (1..=n).map(|i| Decimal::from(i * 10)).collect();
            let ctx = create_bars_with_closes(&closes);
            highest.compute(&ctx);
        }

        assert_eq!(highest.output().count(), 4);
        // Most recent: highest of [40, 50, 60] = 60
        assert_eq!(highest.get(0), Some(dec!(60)));
    }

    #[test]
    #[should_panic(expected = "Highest period must be > 0")]
    fn test_highest_zero_period_panics() {
        Highest::new(0);
    }

    // ========== Lowest Tests ==========

    #[test]
    fn test_lowest_basic() {
        let closes = vec![dec!(10), dec!(50), dec!(30), dec!(20), dec!(40)];
        let ctx = create_bars_with_closes(&closes);

        let mut lowest = Lowest::new(3);
        let value = lowest.compute(&ctx);

        // Lowest of last 3 [30, 20, 40] = 20
        assert_eq!(value, Some(dec!(20)));
    }

    #[test]
    fn test_lowest_insufficient_data() {
        let closes = vec![dec!(10), dec!(20)];
        let ctx = create_bars_with_closes(&closes);

        let mut lowest = Lowest::new(5);
        assert_eq!(lowest.compute(&ctx), None);
    }

    #[test]
    fn test_lowest_exact_period() {
        let closes = vec![dec!(30), dec!(10), dec!(50)];
        let ctx = create_bars_with_closes(&closes);

        let mut lowest = Lowest::new(3);
        let value = lowest.compute(&ctx);

        // Lowest of [30, 10, 50] = 10
        assert_eq!(value, Some(dec!(10)));
    }

    #[test]
    fn test_lowest_warmup_period() {
        let lowest = Lowest::new(20);
        assert_eq!(lowest.warmup_period(), 20);
    }

    #[test]
    fn test_lowest_name() {
        let lowest = Lowest::new(14);
        assert_eq!(lowest.name(), "lowest_14");
    }

    #[test]
    fn test_lowest_reset() {
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut lowest = Lowest::new(3);
        lowest.compute(&ctx);

        assert!(lowest.current().is_some());

        lowest.reset();

        assert!(lowest.current().is_none());
        assert_eq!(lowest.output().count(), 0);
    }

    #[test]
    fn test_lowest_historical_access() {
        let mut lowest = Lowest::new(3);

        for n in 3..=6 {
            let closes: Vec<Decimal> = (1..=n).map(|i| Decimal::from(i * 10)).collect();
            let ctx = create_bars_with_closes(&closes);
            lowest.compute(&ctx);
        }

        assert_eq!(lowest.output().count(), 4);
        // Most recent: lowest of [40, 50, 60] = 40
        assert_eq!(lowest.get(0), Some(dec!(40)));
    }

    #[test]
    #[should_panic(expected = "Lowest period must be > 0")]
    fn test_lowest_zero_period_panics() {
        Lowest::new(0);
    }

    // ========== Combined Tests ==========

    #[test]
    fn test_highest_lowest_same_data() {
        let closes = vec![dec!(100), dec!(100), dec!(100)];
        let ctx = create_bars_with_closes(&closes);

        let mut highest = Highest::new(3);
        let mut lowest = Lowest::new(3);

        let h = highest.compute(&ctx);
        let l = lowest.compute(&ctx);

        // All values same, so highest = lowest = 100
        assert_eq!(h, Some(dec!(100)));
        assert_eq!(l, Some(dec!(100)));
    }

    #[test]
    fn test_donchian_channel_simulation() {
        // Donchian channel uses highest high and lowest low
        let closes = vec![dec!(95), dec!(105), dec!(100), dec!(110), dec!(90)];
        let ctx = create_bars_with_closes(&closes);

        let mut upper = Highest::new(5);
        let mut lower = Lowest::new(5);

        let upper_band = upper.compute(&ctx).unwrap();
        let lower_band = lower.compute(&ctx).unwrap();

        assert_eq!(upper_band, dec!(110));
        assert_eq!(lower_band, dec!(90));

        // Channel width
        let width = upper_band - lower_band;
        assert_eq!(width, dec!(20));
    }

    // ========== Custom Source Tests ==========

    fn create_bars_with_ohlc(bars: &[(Decimal, Decimal, Decimal, Decimal)]) -> BarsContext {
        let mut ctx = BarsContext::new("TEST");
        for (open, high, low, close) in bars {
            let ohlc = OHLCData::new(
                Utc::now(),
                "TEST".to_string(),
                Timeframe::OneMinute,
                *open,
                *high,
                *low,
                *close,
                dec!(100),
                1,
            );
            ctx.on_bar_update(&BarData::from_ohlc(&ohlc));
        }
        ctx
    }

    #[test]
    fn test_highest_high_source() {
        // True Donchian upper band uses highs
        let bars = vec![
            (dec!(100), dec!(110), dec!(95), dec!(105)),   // H=110
            (dec!(105), dec!(115), dec!(100), dec!(110)),  // H=115
            (dec!(110), dec!(120), dec!(105), dec!(115)),  // H=120
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut hh = Highest::with_source(3, PriceSource::High);
        let value = hh.compute(&ctx);

        // Highest of highs [110, 115, 120] = 120
        assert_eq!(value, Some(dec!(120)));
    }

    #[test]
    fn test_lowest_low_source() {
        // True Donchian lower band uses lows
        let bars = vec![
            (dec!(100), dec!(110), dec!(95), dec!(105)),   // L=95
            (dec!(105), dec!(115), dec!(100), dec!(110)),  // L=100
            (dec!(110), dec!(120), dec!(105), dec!(115)),  // L=105
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut ll = Lowest::with_source(3, PriceSource::Low);
        let value = ll.compute(&ctx);

        // Lowest of lows [95, 100, 105] = 95
        assert_eq!(value, Some(dec!(95)));
    }

    #[test]
    fn test_highest_source_name() {
        let h_close = Highest::new(14);
        assert_eq!(h_close.name(), "highest_14");

        let h_high = Highest::with_source(14, PriceSource::High);
        assert_eq!(h_high.name(), "highest_14_high");
    }

    #[test]
    fn test_lowest_source_name() {
        let l_close = Lowest::new(14);
        assert_eq!(l_close.name(), "lowest_14");

        let l_low = Lowest::with_source(14, PriceSource::Low);
        assert_eq!(l_low.name(), "lowest_14_low");
    }

    #[test]
    fn test_highest_source_accessor() {
        let h = Highest::new(14);
        assert_eq!(h.source(), PriceSource::Close);

        let h_high = Highest::with_source(14, PriceSource::High);
        assert_eq!(h_high.source(), PriceSource::High);
    }

    #[test]
    fn test_lowest_source_accessor() {
        let l = Lowest::new(14);
        assert_eq!(l.source(), PriceSource::Close);

        let l_low = Lowest::with_source(14, PriceSource::Low);
        assert_eq!(l_low.source(), PriceSource::Low);
    }

    #[test]
    fn test_true_donchian_channel() {
        // Proper Donchian channel: highest high and lowest low
        let bars = vec![
            (dec!(100), dec!(110), dec!(90), dec!(100)),   // H=110, L=90
            (dec!(100), dec!(115), dec!(95), dec!(105)),   // H=115, L=95
            (dec!(105), dec!(120), dec!(100), dec!(115)),  // H=120, L=100
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut upper = Highest::with_source(3, PriceSource::High);
        let mut lower = Lowest::with_source(3, PriceSource::Low);

        let upper_band = upper.compute(&ctx).unwrap();
        let lower_band = lower.compute(&ctx).unwrap();

        assert_eq!(upper_band, dec!(120)); // Highest high
        assert_eq!(lower_band, dec!(90));  // Lowest low
    }
}
