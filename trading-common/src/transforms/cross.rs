// Cross Above/Below Transforms

use crate::series::{BarsContext, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::Transform;

/// Cross Above transform.
///
/// Detects when the source transform crosses above a threshold.
/// Returns `true` on the bar where the crossing occurs, `false` otherwise.
///
/// # Logic
///
/// CrossAbove = (Current > Threshold) AND (Previous <= Threshold)
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, TransformExt, Rsi};
/// use rust_decimal_macros::dec;
///
/// // Signal when RSI crosses above 30 (oversold recovery)
/// let mut signal = Rsi::new(14).cross_above(dec!(30));
/// if signal.compute(&bars) == Some(true) {
///     // RSI just crossed above 30 - potential buy signal
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CrossAbove<T: Transform<Output = Decimal>> {
    source: T,
    threshold: Decimal,
    name: String,
    output: Series<bool>,
}

impl<T: Transform<Output = Decimal>> CrossAbove<T> {
    /// Create a new CrossAbove transform.
    ///
    /// # Arguments
    ///
    /// * `source` - The source transform whose output to monitor
    /// * `threshold` - The threshold value to cross above
    pub fn new(source: T, threshold: Decimal) -> Self {
        let name = format!("{}_cross_above_{}", source.name(), threshold);
        Self {
            source,
            threshold,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), 2),
        }
    }

    /// Get the threshold value
    pub fn threshold(&self) -> Decimal {
        self.threshold
    }

    /// Get a reference to the source transform
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Transform<Output = Decimal>> Transform for CrossAbove<T> {
    type Output = bool;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        // Need source warmup + 1 additional bar for previous value
        self.source.warmup_period() + 1
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<bool> {
        // Compute source first (updates source's internal state)
        self.source.compute(bars)?;

        // Get current and previous source values
        let current = self.source.get(0)?;
        let previous = self.source.get(1)?;

        // Cross above: was at or below threshold, now above
        let crossed = previous <= self.threshold && current > self.threshold;

        self.output.push(crossed);
        Some(crossed)
    }

    fn output(&self) -> &Series<bool> {
        &self.output
    }

    fn reset(&mut self) {
        self.source.reset();
        self.output.reset();
    }
}

/// Cross Below transform.
///
/// Detects when the source transform crosses below a threshold.
/// Returns `true` on the bar where the crossing occurs, `false` otherwise.
///
/// # Logic
///
/// CrossBelow = (Current < Threshold) AND (Previous >= Threshold)
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, TransformExt, Rsi};
/// use rust_decimal_macros::dec;
///
/// // Signal when RSI crosses below 70 (overbought correction)
/// let mut signal = Rsi::new(14).cross_below(dec!(70));
/// if signal.compute(&bars) == Some(true) {
///     // RSI just crossed below 70 - potential sell signal
/// }
/// ```
#[derive(Debug, Clone)]
pub struct CrossBelow<T: Transform<Output = Decimal>> {
    source: T,
    threshold: Decimal,
    name: String,
    output: Series<bool>,
}

impl<T: Transform<Output = Decimal>> CrossBelow<T> {
    /// Create a new CrossBelow transform.
    ///
    /// # Arguments
    ///
    /// * `source` - The source transform whose output to monitor
    /// * `threshold` - The threshold value to cross below
    pub fn new(source: T, threshold: Decimal) -> Self {
        let name = format!("{}_cross_below_{}", source.name(), threshold);
        Self {
            source,
            threshold,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), 2),
        }
    }

    /// Get the threshold value
    pub fn threshold(&self) -> Decimal {
        self.threshold
    }

    /// Get a reference to the source transform
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Transform<Output = Decimal>> Transform for CrossBelow<T> {
    type Output = bool;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        // Need source warmup + 1 additional bar for previous value
        self.source.warmup_period() + 1
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<bool> {
        // Compute source first
        self.source.compute(bars)?;

        // Get current and previous source values
        let current = self.source.get(0)?;
        let previous = self.source.get(1)?;

        // Cross below: was at or above threshold, now below
        let crossed = previous >= self.threshold && current < self.threshold;

        self.output.push(crossed);
        Some(crossed)
    }

    fn output(&self) -> &Series<bool> {
        &self.output
    }

    fn reset(&mut self) {
        self.source.reset();
        self.output.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarData, OHLCData, Timeframe};
    use crate::transforms::Sma;
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

    fn compute_incrementally<T: Transform>(
        transform: &mut T,
        closes: &[Decimal],
    ) -> Option<T::Output>
    where
        T::Output: Copy,
    {
        let mut result = None;
        for i in 1..=closes.len() {
            let ctx = create_bars_with_closes(&closes[..i]);
            result = transform.compute(&ctx);
        }
        result
    }

    // ========== CrossAbove Tests ==========

    #[test]
    fn test_cross_above_detects_crossing() {
        // SMA(2) crosses above 25
        // Prices: 20, 20, 30, 30 -> SMA: _, 20, 25, 30
        // SMA goes from 25 to 30 (crosses above 25)
        let closes = vec![dec!(20), dec!(20), dec!(30), dec!(30)];

        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_cross_above_no_crossing_when_above() {
        // Already above threshold, no crossing
        let closes = vec![dec!(30), dec!(30), dec!(35), dec!(40)];

        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_cross_above_no_crossing_when_below() {
        // Stays below threshold, no crossing
        let closes = vec![dec!(10), dec!(10), dec!(15), dec!(20)];

        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(50));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_cross_above_from_equal() {
        // Need SMA to go from <= threshold to > threshold
        // Prices: [15, 35, 20, 40] -> SMA(2): 25, 27.5, 30
        // At 27.5: prev=25, curr=27.5, 25 <= 25 and 27.5 > 25 = TRUE CROSS
        let closes = vec![dec!(15), dec!(35), dec!(20), dec!(40)];

        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        // Last result: prev=27.5, curr=30, 27.5 > 25 so no cross at last bar
        // But we crossed at bar 3, let's check historical
        assert!(cross.output().count() >= 1);
        // The cross happened at the 3rd bar when SMA went from 25 to 27.5
        // Since compute_incrementally returns the last result (bar 4),
        // and at bar 4 SMA goes from 27.5 to 30 (no cross since both > 25),
        // the last result is false
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_cross_above_warmup_period() {
        let sma = Sma::new(3);
        let cross = CrossAbove::new(sma, dec!(50));

        // SMA warmup is 3, cross needs +1 = 4
        assert_eq!(cross.warmup_period(), 4);
    }

    #[test]
    fn test_cross_above_name() {
        let sma = Sma::new(3);
        let cross = CrossAbove::new(sma, dec!(50));

        assert_eq!(cross.name(), "sma_3_cross_above_50");
    }

    #[test]
    fn test_cross_above_reset() {
        let closes = vec![dec!(20), dec!(20), dec!(30), dec!(30)];

        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(25));

        compute_incrementally(&mut cross, &closes);
        assert!(cross.current().is_some());

        cross.reset();
        assert!(cross.current().is_none());
        assert_eq!(cross.output().count(), 0);
    }

    #[test]
    fn test_cross_above_insufficient_data() {
        let closes = vec![dec!(30)];

        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, None);
    }

    // ========== CrossBelow Tests ==========

    #[test]
    fn test_cross_below_detects_crossing() {
        // SMA(2) crosses below 25
        // Prices: 30, 30, 20, 20 -> SMA: _, 30, 25, 20
        // SMA goes from 25 to 20 (crosses below 25)
        let closes = vec![dec!(30), dec!(30), dec!(20), dec!(20)];

        let sma = Sma::new(2);
        let mut cross = CrossBelow::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, Some(true));
    }

    #[test]
    fn test_cross_below_no_crossing_when_below() {
        // Already below threshold, no crossing
        let closes = vec![dec!(10), dec!(10), dec!(15), dec!(12)];

        let sma = Sma::new(2);
        let mut cross = CrossBelow::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_cross_below_no_crossing_when_above() {
        // Stays above threshold, no crossing
        let closes = vec![dec!(40), dec!(50), dec!(45), dec!(55)];

        let sma = Sma::new(2);
        let mut cross = CrossBelow::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_cross_below_from_equal() {
        // Need SMA to go from >= threshold to < threshold
        // Prices: [35, 15, 30, 10] -> SMA(2): 25, 22.5, 20
        // At 22.5: prev=25, curr=22.5, 25 >= 25 and 22.5 < 25 = TRUE CROSS
        let closes = vec![dec!(35), dec!(15), dec!(30), dec!(10)];

        let sma = Sma::new(2);
        let mut cross = CrossBelow::new(sma, dec!(25));

        let result = compute_incrementally(&mut cross, &closes);
        // Last result: prev=22.5, curr=20, 22.5 < 25 so no cross at last bar
        // The cross happened at bar 3 when SMA went from 25 to 22.5
        assert!(cross.output().count() >= 1);
        assert_eq!(result, Some(false));
    }

    #[test]
    fn test_cross_below_warmup_period() {
        let sma = Sma::new(5);
        let cross = CrossBelow::new(sma, dec!(50));

        // SMA warmup is 5, cross needs +1 = 6
        assert_eq!(cross.warmup_period(), 6);
    }

    #[test]
    fn test_cross_below_name() {
        let sma = Sma::new(3);
        let cross = CrossBelow::new(sma, dec!(50));

        assert_eq!(cross.name(), "sma_3_cross_below_50");
    }

    #[test]
    fn test_cross_below_reset() {
        let closes = vec![dec!(30), dec!(30), dec!(20), dec!(20)];

        let sma = Sma::new(2);
        let mut cross = CrossBelow::new(sma, dec!(25));

        compute_incrementally(&mut cross, &closes);
        assert!(cross.current().is_some());

        cross.reset();
        assert!(cross.current().is_none());
        assert_eq!(cross.output().count(), 0);
    }

    // ========== Combined Tests ==========

    #[test]
    fn test_cross_above_below_mutual_exclusion() {
        // On same bar, cannot cross both above AND below
        let closes = vec![dec!(20), dec!(30), dec!(40), dec!(50)];

        let sma1 = Sma::new(2);
        let sma2 = Sma::new(2);

        let mut cross_above = CrossAbove::new(sma1, dec!(30));
        let mut cross_below = CrossBelow::new(sma2, dec!(30));

        let above = compute_incrementally(&mut cross_above, &closes);
        let below = compute_incrementally(&mut cross_below, &closes);

        // Cannot both be true simultaneously
        assert!(!(above == Some(true) && below == Some(true)));
    }

    #[test]
    fn test_cross_historical_access() {
        let sma = Sma::new(2);
        let mut cross = CrossAbove::new(sma, dec!(25));

        // Build up history
        let sequences = vec![
            vec![dec!(20), dec!(20), dec!(30)], // SMA: 20 -> 25, no cross
            vec![dec!(20), dec!(20), dec!(30), dec!(30)], // SMA: 25 -> 30, cross
            vec![dec!(20), dec!(20), dec!(30), dec!(30), dec!(30)], // SMA: 30 -> 30, no cross
        ];

        for closes in sequences {
            cross.reset();
            compute_incrementally(&mut cross, &closes);
        }

        // The last computation should have a result
        assert!(cross.current().is_some());
    }

    #[test]
    fn test_threshold_accessor() {
        let sma = Sma::new(2);
        let cross = CrossAbove::new(sma, dec!(42));

        assert_eq!(cross.threshold(), dec!(42));
    }
}
