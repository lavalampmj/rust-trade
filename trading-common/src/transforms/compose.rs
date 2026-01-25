// Transform Composition - Transforms applied to other transforms' output
//
// This module enables indicator chaining like: Rsi::new(14).sma(3)
// where SMA is computed on the RSI output, not on price.

use crate::series::{BarsContext, DecimalSeriesExt, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::Transform;

/// SMA applied to another transform's output.
///
/// Computes Simple Moving Average of the source transform's values.
///
/// # Warmup Propagation
///
/// Warmup = source.warmup_period() + period
///
/// # Example
///
/// ```no_run
/// use trading_common::transforms::{Transform, TransformExt, Rsi};
///
/// // Smoothed RSI: 3-period SMA of RSI(14)
/// let mut smoothed = Rsi::new(14).sma(3);
/// // warmup = 15 + 3 = 18 bars
/// ```
#[derive(Debug, Clone)]
pub struct SmaOf<T: Transform<Output = Decimal>> {
    source: T,
    period: usize,
    name: String,
    output: Series<Decimal>,
}

impl<T: Transform<Output = Decimal>> SmaOf<T> {
    /// Create a new SmaOf composition.
    ///
    /// # Arguments
    ///
    /// * `source` - The source transform
    /// * `period` - SMA period
    pub fn new(source: T, period: usize) -> Self {
        assert!(period > 0, "SMA period must be > 0");
        let name = format!("sma_{}_of_{}", period, source.name());
        let warmup = source.warmup_period() + period;

        Self {
            source,
            period,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), warmup),
        }
    }

    /// Get the SMA period
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get a reference to the source transform
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Transform<Output = Decimal>> Transform for SmaOf<T> {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.source.warmup_period() + self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        // First compute source (updates source's internal state)
        self.source.compute(bars)?;

        // Then compute SMA of source's output series
        let value = self.source.output().sma(self.period)?;
        self.output.push(value);
        Some(value)
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.source.reset();
        self.output.reset();
    }
}

/// EMA applied to another transform's output.
///
/// Computes Exponential Moving Average of the source transform's values.
///
/// # Warmup Propagation
///
/// Warmup = source.warmup_period() + period
///
/// # Example
///
/// ```no_run
/// use trading_common::transforms::{Transform, TransformExt, Rsi};
///
/// // Smoothed RSI with EMA: 3-period EMA of RSI(14)
/// let mut smoothed = Rsi::new(14).ema(3);
/// // warmup = 15 + 3 = 18 bars
/// ```
#[derive(Debug, Clone)]
pub struct EmaOf<T: Transform<Output = Decimal>> {
    source: T,
    period: usize,
    multiplier: Decimal,
    name: String,
    output: Series<Decimal>,
    prev_ema: Option<Decimal>,
}

impl<T: Transform<Output = Decimal>> EmaOf<T> {
    /// Create a new EmaOf composition.
    ///
    /// # Arguments
    ///
    /// * `source` - The source transform
    /// * `period` - EMA period
    pub fn new(source: T, period: usize) -> Self {
        assert!(period > 0, "EMA period must be > 0");
        let name = format!("ema_{}_of_{}", period, source.name());
        let warmup = source.warmup_period() + period;
        let multiplier = Decimal::TWO / Decimal::from(period + 1);

        Self {
            source,
            period,
            multiplier,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), warmup),
            prev_ema: None,
        }
    }

    /// Get the EMA period
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get a reference to the source transform
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Transform<Output = Decimal>> Transform for EmaOf<T> {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.source.warmup_period() + self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        // First compute source
        self.source.compute(bars)?;

        // Need enough source values for EMA
        if self.source.output().count() < self.period {
            return None;
        }

        let current_value = *self.source.output().current()?;

        let ema_value = match self.prev_ema {
            Some(prev) => {
                // EMA = (Value * k) + (PrevEMA * (1 - k))
                (current_value * self.multiplier) + (prev * (Decimal::ONE - self.multiplier))
            }
            None => {
                // Initialize with SMA of source values
                self.source.output().sma(self.period)?
            }
        };

        self.prev_ema = Some(ema_value);
        self.output.push(ema_value);
        Some(ema_value)
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.source.reset();
        self.output.reset();
        self.prev_ema = None;
    }
}

/// Highest value applied to another transform's output.
///
/// Computes the highest value of the source transform over the last N values.
///
/// # Warmup Propagation
///
/// Warmup = source.warmup_period() + period
///
/// # Example
///
/// ```no_run
/// use trading_common::transforms::{Transform, TransformExt, Rsi};
///
/// // Highest RSI over 10 bars
/// let mut highest_rsi = Rsi::new(14).highest(10);
/// ```
#[derive(Debug, Clone)]
pub struct HighestOf<T: Transform<Output = Decimal>> {
    source: T,
    period: usize,
    name: String,
    output: Series<Decimal>,
}

impl<T: Transform<Output = Decimal>> HighestOf<T> {
    /// Create a new HighestOf composition.
    ///
    /// # Arguments
    ///
    /// * `source` - The source transform
    /// * `period` - Lookback period
    pub fn new(source: T, period: usize) -> Self {
        assert!(period > 0, "Highest period must be > 0");
        let name = format!("highest_{}_of_{}", period, source.name());
        let warmup = source.warmup_period() + period;

        Self {
            source,
            period,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), warmup),
        }
    }

    /// Get the period
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get a reference to the source transform
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Transform<Output = Decimal>> Transform for HighestOf<T> {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.source.warmup_period() + self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        // First compute source
        self.source.compute(bars)?;

        // Compute highest of source's output
        let value = self.source.output().highest(self.period)?;
        self.output.push(value);
        Some(value)
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.source.reset();
        self.output.reset();
    }
}

/// Lowest value applied to another transform's output.
///
/// Computes the lowest value of the source transform over the last N values.
///
/// # Warmup Propagation
///
/// Warmup = source.warmup_period() + period
///
/// # Example
///
/// ```no_run
/// use trading_common::transforms::{Transform, TransformExt, Rsi};
///
/// // Lowest RSI over 10 bars
/// let mut lowest_rsi = Rsi::new(14).lowest(10);
/// ```
#[derive(Debug, Clone)]
pub struct LowestOf<T: Transform<Output = Decimal>> {
    source: T,
    period: usize,
    name: String,
    output: Series<Decimal>,
}

impl<T: Transform<Output = Decimal>> LowestOf<T> {
    /// Create a new LowestOf composition.
    ///
    /// # Arguments
    ///
    /// * `source` - The source transform
    /// * `period` - Lookback period
    pub fn new(source: T, period: usize) -> Self {
        assert!(period > 0, "Lowest period must be > 0");
        let name = format!("lowest_{}_of_{}", period, source.name());
        let warmup = source.warmup_period() + period;

        Self {
            source,
            period,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), warmup),
        }
    }

    /// Get the period
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get a reference to the source transform
    pub fn source(&self) -> &T {
        &self.source
    }
}

impl<T: Transform<Output = Decimal>> Transform for LowestOf<T> {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.source.warmup_period() + self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        // First compute source
        self.source.compute(bars)?;

        // Compute lowest of source's output
        let value = self.source.output().lowest(self.period)?;
        self.output.push(value);
        Some(value)
    }

    fn output(&self) -> &Series<Decimal> {
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
    use crate::transforms::{Rsi, Sma, TransformExt};
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

    // ========== SmaOf Tests ==========

    #[test]
    fn test_sma_of_warmup_propagation() {
        // SMA(3) of SMA(2) = 2 + 3 = 5
        let sma2 = Sma::new(2);
        let sma_of = SmaOf::new(sma2, 3);
        assert_eq!(sma_of.warmup_period(), 5);
    }

    #[test]
    fn test_sma_of_sma() {
        // Double smoothing: SMA(2) of SMA(2)
        // Prices: 10, 20, 30, 40 -> SMA(2): 15, 25, 35
        // SMA(2) of [15, 25, 35] -> 20, 30
        let closes: Vec<Decimal> = (1..=5).map(|i| Decimal::from(i * 10)).collect();

        let sma = Sma::new(2);
        let mut double_sma = SmaOf::new(sma, 2);

        let result = compute_incrementally(&mut double_sma, &closes);
        assert!(result.is_some());
    }

    #[test]
    fn test_sma_of_name() {
        let sma = Sma::new(3);
        let sma_of = SmaOf::new(sma, 5);
        assert_eq!(sma_of.name(), "sma_5_of_sma_3");
    }

    #[test]
    fn test_sma_of_reset() {
        let closes: Vec<Decimal> = (1..=10).map(|i| Decimal::from(i * 10)).collect();

        let sma = Sma::new(2);
        let mut sma_of = SmaOf::new(sma, 3);

        compute_incrementally(&mut sma_of, &closes);
        assert!(sma_of.current().is_some());

        sma_of.reset();
        assert!(sma_of.current().is_none());
        assert_eq!(sma_of.output().count(), 0);
    }

    // ========== EmaOf Tests ==========

    #[test]
    fn test_ema_of_warmup_propagation() {
        // EMA(3) of SMA(2) = 2 + 3 = 5
        let sma = Sma::new(2);
        let ema_of = EmaOf::new(sma, 3);
        assert_eq!(ema_of.warmup_period(), 5);
    }

    #[test]
    fn test_ema_of_sma() {
        let closes: Vec<Decimal> = (1..=10).map(|i| Decimal::from(i * 10)).collect();

        let sma = Sma::new(2);
        let mut ema_of = EmaOf::new(sma, 3);

        let result = compute_incrementally(&mut ema_of, &closes);
        assert!(result.is_some());
    }

    #[test]
    fn test_ema_of_name() {
        let sma = Sma::new(3);
        let ema_of = EmaOf::new(sma, 5);
        assert_eq!(ema_of.name(), "ema_5_of_sma_3");
    }

    #[test]
    fn test_ema_of_reset() {
        let closes: Vec<Decimal> = (1..=10).map(|i| Decimal::from(i * 10)).collect();

        let sma = Sma::new(2);
        let mut ema_of = EmaOf::new(sma, 3);

        compute_incrementally(&mut ema_of, &closes);
        assert!(ema_of.current().is_some());
        assert!(ema_of.prev_ema.is_some());

        ema_of.reset();
        assert!(ema_of.current().is_none());
        assert!(ema_of.prev_ema.is_none());
    }

    // ========== HighestOf Tests ==========

    #[test]
    fn test_highest_of_warmup_propagation() {
        // Highest(5) of SMA(3) = 3 + 5 = 8
        let sma = Sma::new(3);
        let highest_of = HighestOf::new(sma, 5);
        assert_eq!(highest_of.warmup_period(), 8);
    }

    #[test]
    fn test_highest_of_sma() {
        // Prices creating varying SMA values
        let closes = vec![
            dec!(10),
            dec!(20),
            dec!(30), // SMA(2) = 25
            dec!(50), // SMA(2) = 40
            dec!(40), // SMA(2) = 45
            dec!(20), // SMA(2) = 30
        ];

        let sma = Sma::new(2);
        let mut highest_of = HighestOf::new(sma, 3);

        let result = compute_incrementally(&mut highest_of, &closes);
        // Highest of SMA values [40, 45, 30] = 45
        assert_eq!(result, Some(dec!(45)));
    }

    #[test]
    fn test_highest_of_name() {
        let sma = Sma::new(3);
        let highest_of = HighestOf::new(sma, 10);
        assert_eq!(highest_of.name(), "highest_10_of_sma_3");
    }

    // ========== LowestOf Tests ==========

    #[test]
    fn test_lowest_of_warmup_propagation() {
        // Lowest(5) of SMA(3) = 3 + 5 = 8
        let sma = Sma::new(3);
        let lowest_of = LowestOf::new(sma, 5);
        assert_eq!(lowest_of.warmup_period(), 8);
    }

    #[test]
    fn test_lowest_of_sma() {
        let closes = vec![
            dec!(10),
            dec!(20),
            dec!(30), // SMA(2) = 25
            dec!(50), // SMA(2) = 40
            dec!(40), // SMA(2) = 45
            dec!(20), // SMA(2) = 30
        ];

        let sma = Sma::new(2);
        let mut lowest_of = LowestOf::new(sma, 3);

        let result = compute_incrementally(&mut lowest_of, &closes);
        // Lowest of SMA values [40, 45, 30] = 30
        assert_eq!(result, Some(dec!(30)));
    }

    #[test]
    fn test_lowest_of_name() {
        let sma = Sma::new(3);
        let lowest_of = LowestOf::new(sma, 10);
        assert_eq!(lowest_of.name(), "lowest_10_of_sma_3");
    }

    // ========== Fluent API Tests ==========

    #[test]
    fn test_fluent_api_sma() {
        let mut smoothed = Sma::new(3).sma(2);
        assert_eq!(smoothed.warmup_period(), 5);

        let closes: Vec<Decimal> = (1..=10).map(|i| Decimal::from(i * 10)).collect();
        let result = compute_incrementally(&mut smoothed, &closes);
        assert!(result.is_some());
    }

    #[test]
    fn test_fluent_api_ema() {
        let smoothed = Sma::new(3).ema(2);
        assert_eq!(smoothed.warmup_period(), 5);
    }

    #[test]
    fn test_fluent_api_highest() {
        let highest = Sma::new(3).highest(5);
        assert_eq!(highest.warmup_period(), 8);
    }

    #[test]
    fn test_fluent_api_lowest() {
        let lowest = Sma::new(3).lowest(5);
        assert_eq!(lowest.warmup_period(), 8);
    }

    // ========== Deep Chaining Tests ==========

    #[test]
    fn test_deep_chaining_warmup() {
        // SMA(3).sma(2).ema(4).highest(5) = 3 + 2 + 4 + 5 = 14
        let complex = Sma::new(3).sma(2).ema(4).highest(5);
        assert_eq!(complex.warmup_period(), 14);
    }

    #[test]
    fn test_rsi_sma_chaining() {
        // RSI(5).sma(3) = 6 + 3 = 9
        let smoothed_rsi = Rsi::new(5).sma(3);
        assert_eq!(smoothed_rsi.warmup_period(), 9);
    }

    #[test]
    fn test_triple_chain() {
        // SMA(2).sma(2).sma(2) = 2 + 2 + 2 = 6
        let triple = Sma::new(2).sma(2).sma(2);
        assert_eq!(triple.warmup_period(), 6);

        let closes: Vec<Decimal> = (1..=10).map(|i| Decimal::from(i * 10)).collect();
        let mut triple = Sma::new(2).sma(2).sma(2);
        let result = compute_incrementally(&mut triple, &closes);
        assert!(result.is_some());
    }

    #[test]
    fn test_complex_strategy_chain() {
        // Realistic strategy setup: RSI(14).sma(3).highest(10)
        // Warmup = 15 + 3 + 10 = 28
        let complex = Rsi::new(14).sma(3).highest(10);
        assert_eq!(complex.warmup_period(), 28);
    }
}
