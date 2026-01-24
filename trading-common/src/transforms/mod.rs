// Transform framework for composable indicator calculations
//
// Transforms are stateful - they maintain internal output buffers allowing
// historical access and efficient O(1) per-bar updates.
//
// Key features:
// - Automatic warmup propagation in chained transforms
// - Type-safe composition with TransformExt trait
// - Internal Series<Output> for historical value access

mod atr;
mod change;
mod compose;
mod cross;
mod ema;
mod extremes;
mod registry;
mod rsi;
mod sma;
mod source;

pub use atr::Atr;
pub use change::{Change, RateOfChange};
pub use compose::{EmaOf, HighestOf, LowestOf, SmaOf};
pub use cross::{CrossAbove, CrossBelow};
pub use ema::Ema;
pub use extremes::{Highest, Lowest};
pub use registry::TransformRegistry;
pub use rsi::Rsi;
pub use sma::Sma;
pub use source::PriceSource;

use crate::series::{BarsContext, Series, SeriesValue};
use rust_decimal::Decimal;

/// Core trait for all transforms.
///
/// Transforms convert bar data into output values while maintaining internal state.
/// They can be chained together for composition (e.g., `Rsi::new(14).sma(3)`).
///
/// # Design
///
/// - **Stateful**: Transforms maintain an internal `Series<Output>` buffer
/// - **O(1) per bar**: EMA and similar transforms update efficiently
/// - **Historical access**: `get(bars_ago)` retrieves past values
/// - **Warmup propagation**: Chained transforms automatically sum warmup periods
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, Sma, TransformExt};
///
/// let mut sma = Sma::new(20);
/// let value = sma.compute(&bars);  // Updates internal state, returns Option<Decimal>
/// let prev = sma.get(1);           // Get value from 1 bar ago
/// ```
pub trait Transform: Send + Sync {
    /// The output type produced by this transform
    type Output: SeriesValue + Copy;

    /// Unique name for this transform instance
    fn name(&self) -> &str;

    /// Minimum bars required before this transform produces valid output
    fn warmup_period(&self) -> usize;

    /// Compute the current output value, updating internal state.
    ///
    /// This method should be called once per bar. It:
    /// 1. Reads input data from BarsContext
    /// 2. Computes the output value
    /// 3. Pushes the result to the internal output series
    /// 4. Returns the computed value (or None if insufficient data)
    fn compute(&mut self, bars: &BarsContext) -> Option<Self::Output>;

    /// Access the internal output series (for historical values)
    fn output(&self) -> &Series<Self::Output>;

    /// Check if this transform has enough data to produce valid output
    fn is_ready(&self, bars: &BarsContext) -> bool {
        bars.count() >= self.warmup_period()
    }

    /// Reset internal state for new backtest run
    fn reset(&mut self);

    /// Current value (most recent output)
    fn current(&self) -> Option<Self::Output> {
        self.output().current().copied()
    }

    /// Value N bars ago
    fn get(&self, bars_ago: usize) -> Option<Self::Output> {
        self.output().get(bars_ago).copied()
    }
}

/// Extension trait enabling fluent chaining for Decimal-output transforms.
///
/// This trait provides methods to chain transforms together, creating
/// composite indicators where the output of one transform feeds into another.
///
/// # Warmup Propagation
///
/// When transforms are chained, warmup periods automatically accumulate:
/// - `Rsi::new(14)` has warmup = 15
/// - `Rsi::new(14).sma(3)` has warmup = 15 + 3 = 18
/// - `Rsi::new(14).sma(3).ema(5)` has warmup = 15 + 3 + 5 = 23
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Rsi, Transform, TransformExt};
///
/// // Create a smoothed RSI (RSI with 3-period SMA smoothing)
/// let mut smoothed_rsi = Rsi::new(14).sma(3);
///
/// // Compute on each bar
/// if let Some(value) = smoothed_rsi.compute(&bars) {
///     println!("Smoothed RSI: {}", value);
/// }
///
/// // Deep chaining
/// let complex = Rsi::new(14).sma(3).ema(5).highest(10);
/// ```
pub trait TransformExt: Transform<Output = Decimal> + Sized {
    /// Apply SMA smoothing to this transform's output
    fn sma(self, period: usize) -> SmaOf<Self> {
        SmaOf::new(self, period)
    }

    /// Apply EMA smoothing to this transform's output
    fn ema(self, period: usize) -> EmaOf<Self> {
        EmaOf::new(self, period)
    }

    /// Get highest value over period
    fn highest(self, period: usize) -> HighestOf<Self> {
        HighestOf::new(self, period)
    }

    /// Get lowest value over period
    fn lowest(self, period: usize) -> LowestOf<Self> {
        LowestOf::new(self, period)
    }

    /// Detect cross above threshold (returns bool transform)
    fn cross_above(self, threshold: Decimal) -> CrossAbove<Self> {
        CrossAbove::new(self, threshold)
    }

    /// Detect cross below threshold (returns bool transform)
    fn cross_below(self, threshold: Decimal) -> CrossBelow<Self> {
        CrossBelow::new(self, threshold)
    }
}

// Blanket implementation for all Decimal transforms
impl<T: Transform<Output = Decimal> + Sized> TransformExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarData, OHLCData, Timeframe};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn create_test_bars_context(prices: &[Decimal]) -> BarsContext {
        let mut ctx = BarsContext::new("TEST");
        for price in prices {
            let ohlc = OHLCData::new(
                Utc::now(),
                "TEST".to_string(),
                Timeframe::OneMinute,
                *price,
                *price,
                *price,
                *price,
                dec!(100),
                1,
            );
            ctx.on_bar_update(&BarData::from_ohlc(&ohlc));
        }
        ctx
    }

    #[test]
    fn test_transform_ext_chaining_compiles() {
        // This test verifies that the chaining API compiles correctly
        let _smoothed = Rsi::new(14).sma(3);
        let _double_smooth = Rsi::new(14).sma(3).ema(5);
        let _with_highest = Rsi::new(14).sma(3).highest(10);
        let _with_lowest = Rsi::new(14).sma(3).lowest(10);
        let _cross = Rsi::new(14).cross_above(dec!(70));
    }

    #[test]
    fn test_warmup_propagation() {
        // RSI(14) = 15 bars warmup
        let rsi = Rsi::new(14);
        assert_eq!(rsi.warmup_period(), 15);

        // RSI(14).sma(3) = 15 + 3 = 18 bars
        let smoothed = Rsi::new(14).sma(3);
        assert_eq!(smoothed.warmup_period(), 18);

        // RSI(14).sma(3).ema(5) = 15 + 3 + 5 = 23 bars
        let double_smooth = Rsi::new(14).sma(3).ema(5);
        assert_eq!(double_smooth.warmup_period(), 23);

        // Deep chaining: RSI(14).sma(3).sma(5).highest(10) = 15 + 3 + 5 + 10 = 33
        let complex = Rsi::new(14).sma(3).sma(5).highest(10);
        assert_eq!(complex.warmup_period(), 33);
    }

    #[test]
    fn test_basic_sma_transform() {
        let prices: Vec<Decimal> = (1..=10).map(|i| Decimal::from(i * 10)).collect();

        let mut sma = Sma::new(3);

        // Compute through all bars
        for i in 0..10 {
            // Rebuild context up to current bar
            let partial_prices: Vec<Decimal> = prices[..=i].to_vec();
            let ctx = create_test_bars_context(&partial_prices);
            sma.compute(&ctx);
        }

        // SMA(3) of [80, 90, 100] = 90
        assert_eq!(sma.current(), Some(dec!(90)));
    }

    #[test]
    fn test_transform_reset() {
        let prices: Vec<Decimal> = (1..=5).map(|i| Decimal::from(i * 10)).collect();
        let ctx = create_test_bars_context(&prices);

        let mut sma = Sma::new(3);
        sma.compute(&ctx);

        assert!(sma.current().is_some());

        sma.reset();

        assert!(sma.current().is_none());
        assert_eq!(sma.output().count(), 0);
    }
}
