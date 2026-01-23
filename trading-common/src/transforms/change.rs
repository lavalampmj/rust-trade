// Change and Rate of Change Transforms

use crate::series::{BarsContext, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::source::PriceSource;
use super::Transform;

const HUNDRED: Decimal = Decimal::from_parts(100, 0, 0, false, 0);

/// Change transform (momentum).
///
/// Calculates the difference between current price and price N bars ago.
/// By default uses close prices, but can be configured to use any price source.
///
/// # Formula
///
/// Change = Price[0] - Price[N]
///
/// where N is the period (default 1 for simple bar-to-bar change).
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, Change, PriceSource};
///
/// // Change in close prices (default)
/// let mut change = Change::new(1);
///
/// // Change in high prices
/// let mut change_high = Change::with_source(1, PriceSource::High);
///
/// if let Some(delta) = change.compute(&bars) {
///     if delta > dec!(0) {
///         // Price increased from previous bar
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Change {
    period: usize,
    source: PriceSource,
    name: String,
    output: Series<Decimal>,
}

impl Change {
    /// Create a new Change transform.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars back to compare (typically 1)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new Change transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars back to compare (typically 1)
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "Change period must be > 0");
        let name = if source == PriceSource::Close {
            format!("change_{}", period)
        } else {
            format!("change_{}_{}", period, source)
        };
        Self {
            period,
            source,
            name: name.clone(),
            output: Series::with_warmup(
                &name,
                MaximumBarsLookBack::default(),
                period + 1, // Need period + 1 bars to calculate change
            ),
        }
    }

    /// Create a Change transform for bar-to-bar change (period = 1)
    pub fn default_period() -> Self {
        Self::new(1)
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

impl Transform for Change {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period + 1
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        let current = self.source.get(bars, 0)?;
        let previous = self.source.get(bars, self.period)?;

        let change = current - previous;
        self.output.push(change);
        Some(change)
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.output.reset();
    }
}

/// Rate of Change (ROC) transform.
///
/// Calculates the percentage change between current price and price N bars ago.
/// By default uses close prices, but can be configured to use any price source.
///
/// # Formula
///
/// ROC = ((Price[0] - Price[N]) / Price[N]) * 100
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, RateOfChange, PriceSource};
///
/// // ROC of close prices (default)
/// let mut roc = RateOfChange::new(12);
///
/// // ROC of typical price
/// let mut roc_typical = RateOfChange::with_source(12, PriceSource::Typical);
///
/// if let Some(pct) = roc.compute(&bars) {
///     if pct > dec!(5) {
///         // Price increased by more than 5% over 12 bars
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RateOfChange {
    period: usize,
    source: PriceSource,
    name: String,
    output: Series<Decimal>,
}

impl RateOfChange {
    /// Create a new Rate of Change transform.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars back to compare
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new Rate of Change transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars back to compare
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "RateOfChange period must be > 0");
        let name = if source == PriceSource::Close {
            format!("roc_{}", period)
        } else {
            format!("roc_{}_{}", period, source)
        };
        Self {
            period,
            source,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), period + 1),
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

impl Transform for RateOfChange {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period + 1
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        let current = self.source.get(bars, 0)?;
        let previous = self.source.get(bars, self.period)?;

        // Avoid division by zero
        if previous == Decimal::ZERO {
            return None;
        }

        let roc = (current - previous) / previous * HUNDRED;
        self.output.push(roc);
        Some(roc)
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

    // ========== Change Tests ==========

    #[test]
    fn test_change_basic() {
        let closes = vec![dec!(100), dec!(105)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(1);
        let value = change.compute(&ctx);

        // Change = 105 - 100 = 5
        assert_eq!(value, Some(dec!(5)));
    }

    #[test]
    fn test_change_negative() {
        let closes = vec![dec!(100), dec!(90)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(1);
        let value = change.compute(&ctx);

        // Change = 90 - 100 = -10
        assert_eq!(value, Some(dec!(-10)));
    }

    #[test]
    fn test_change_no_change() {
        let closes = vec![dec!(100), dec!(100)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(1);
        let value = change.compute(&ctx);

        assert_eq!(value, Some(dec!(0)));
    }

    #[test]
    fn test_change_longer_period() {
        let closes = vec![dec!(100), dec!(110), dec!(105), dec!(120)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(3);
        let value = change.compute(&ctx);

        // Change from 3 bars ago: 120 - 100 = 20
        assert_eq!(value, Some(dec!(20)));
    }

    #[test]
    fn test_change_insufficient_data() {
        let closes = vec![dec!(100)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(1);
        assert_eq!(change.compute(&ctx), None);
    }

    #[test]
    fn test_change_warmup_period() {
        let change = Change::new(1);
        assert_eq!(change.warmup_period(), 2);

        let change5 = Change::new(5);
        assert_eq!(change5.warmup_period(), 6);
    }

    #[test]
    fn test_change_name() {
        let change = Change::new(1);
        assert_eq!(change.name(), "change_1");
    }

    #[test]
    fn test_change_reset() {
        let closes = vec![dec!(100), dec!(110)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(1);
        change.compute(&ctx);

        assert!(change.current().is_some());

        change.reset();

        assert!(change.current().is_none());
    }

    #[test]
    fn test_change_default_period() {
        let change = Change::default_period();
        assert_eq!(change.period(), 1);
    }

    #[test]
    #[should_panic(expected = "Change period must be > 0")]
    fn test_change_zero_period_panics() {
        Change::new(0);
    }

    // ========== Rate of Change Tests ==========

    #[test]
    fn test_roc_basic() {
        let closes = vec![dec!(100), dec!(110)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(1);
        let value = roc.compute(&ctx);

        // ROC = ((110 - 100) / 100) * 100 = 10%
        assert_eq!(value, Some(dec!(10)));
    }

    #[test]
    fn test_roc_negative() {
        let closes = vec![dec!(100), dec!(80)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(1);
        let value = roc.compute(&ctx);

        // ROC = ((80 - 100) / 100) * 100 = -20%
        assert_eq!(value, Some(dec!(-20)));
    }

    #[test]
    fn test_roc_no_change() {
        let closes = vec![dec!(100), dec!(100)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(1);
        let value = roc.compute(&ctx);

        assert_eq!(value, Some(dec!(0)));
    }

    #[test]
    fn test_roc_longer_period() {
        let closes = vec![dec!(100), dec!(110), dec!(105), dec!(150)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(3);
        let value = roc.compute(&ctx);

        // ROC from 3 bars ago: ((150 - 100) / 100) * 100 = 50%
        assert_eq!(value, Some(dec!(50)));
    }

    #[test]
    fn test_roc_insufficient_data() {
        let closes = vec![dec!(100)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(1);
        assert_eq!(roc.compute(&ctx), None);
    }

    #[test]
    fn test_roc_division_by_zero() {
        // Previous price is zero - should return None
        let closes = vec![dec!(0), dec!(100)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(1);
        assert_eq!(roc.compute(&ctx), None);
    }

    #[test]
    fn test_roc_warmup_period() {
        let roc = RateOfChange::new(1);
        assert_eq!(roc.warmup_period(), 2);

        let roc12 = RateOfChange::new(12);
        assert_eq!(roc12.warmup_period(), 13);
    }

    #[test]
    fn test_roc_name() {
        let roc = RateOfChange::new(12);
        assert_eq!(roc.name(), "roc_12");
    }

    #[test]
    fn test_roc_reset() {
        let closes = vec![dec!(100), dec!(110)];
        let ctx = create_bars_with_closes(&closes);

        let mut roc = RateOfChange::new(1);
        roc.compute(&ctx);

        assert!(roc.current().is_some());

        roc.reset();

        assert!(roc.current().is_none());
    }

    #[test]
    fn test_roc_historical_access() {
        let mut roc = RateOfChange::new(1);

        for n in 2..=5 {
            let closes: Vec<Decimal> = (1..=n).map(|i| Decimal::from(i * 100)).collect();
            let ctx = create_bars_with_closes(&closes);
            roc.compute(&ctx);
        }

        assert_eq!(roc.output().count(), 4);
        assert!(roc.get(0).is_some());
    }

    #[test]
    #[should_panic(expected = "RateOfChange period must be > 0")]
    fn test_roc_zero_period_panics() {
        RateOfChange::new(0);
    }

    // ========== Combined Tests ==========

    #[test]
    fn test_change_and_roc_relationship() {
        // Change and ROC should be related: ROC = (Change / Previous) * 100
        let closes = vec![dec!(100), dec!(125)];
        let ctx = create_bars_with_closes(&closes);

        let mut change = Change::new(1);
        let mut roc = RateOfChange::new(1);

        let change_val = change.compute(&ctx).unwrap();
        let roc_val = roc.compute(&ctx).unwrap();

        // Change = 25, ROC = 25%
        assert_eq!(change_val, dec!(25));
        assert_eq!(roc_val, dec!(25));

        // Verify relationship: ROC = (Change / Previous) * 100
        let expected_roc = (change_val / dec!(100)) * dec!(100);
        assert_eq!(roc_val, expected_roc);
    }

    // ========== Custom Source Tests ==========

    #[test]
    fn test_change_source_name() {
        let c_close = Change::new(1);
        assert_eq!(c_close.name(), "change_1");

        let c_high = Change::with_source(1, PriceSource::High);
        assert_eq!(c_high.name(), "change_1_high");
    }

    #[test]
    fn test_change_source_accessor() {
        let c = Change::new(1);
        assert_eq!(c.source(), PriceSource::Close);

        let c_high = Change::with_source(1, PriceSource::High);
        assert_eq!(c_high.source(), PriceSource::High);
    }

    #[test]
    fn test_roc_source_name() {
        let r_close = RateOfChange::new(12);
        assert_eq!(r_close.name(), "roc_12");

        let r_typical = RateOfChange::with_source(12, PriceSource::Typical);
        assert_eq!(r_typical.name(), "roc_12_typical");
    }

    #[test]
    fn test_roc_source_accessor() {
        let r = RateOfChange::new(12);
        assert_eq!(r.source(), PriceSource::Close);

        let r_high = RateOfChange::with_source(12, PriceSource::High);
        assert_eq!(r_high.source(), PriceSource::High);
    }
}
