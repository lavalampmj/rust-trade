// Relative Strength Index Transform

use crate::series::{BarsContext, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::source::PriceSource;
use super::Transform;

const HUNDRED: Decimal = Decimal::from_parts(100, 0, 0, false, 0);

/// Relative Strength Index (RSI) transform.
///
/// A momentum oscillator that measures the speed and magnitude of price movements.
/// Values range from 0 to 100. By default uses close prices, but can be configured
/// to use any price source.
///
/// # Interpretation
///
/// - RSI > 70: Overbought (potential reversal down)
/// - RSI < 30: Oversold (potential reversal up)
/// - RSI = 50: Neutral momentum
///
/// # Formula
///
/// RSI = 100 - (100 / (1 + RS))
///
/// where RS = Average Gain / Average Loss over the period.
///
/// # Warmup
///
/// Requires `period + 1` bars to produce the first value
/// (need `period` price changes to calculate initial averages).
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, Rsi, PriceSource};
///
/// // RSI of close prices (default)
/// let mut rsi = Rsi::new(14);
///
/// // RSI of typical price
/// let mut rsi_typical = Rsi::with_source(14, PriceSource::Typical);
///
/// if let Some(value) = rsi.compute(&bars) {
///     if value < dec!(30) {
///         // Oversold - potential buy signal
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Rsi {
    period: usize,
    source: PriceSource,
    name: String,
    output: Series<Decimal>,
    /// Average gain (smoothed)
    avg_gain: Option<Decimal>,
    /// Average loss (smoothed)
    avg_loss: Option<Decimal>,
    /// Previous price for calculating changes
    prev_price: Option<Decimal>,
    /// Gains buffer for initial calculation
    gains: Vec<Decimal>,
    /// Losses buffer for initial calculation
    losses: Vec<Decimal>,
}

impl Rsi {
    /// Create a new RSI transform with the specified period.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Lookback period (typically 14)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new RSI transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Lookback period (typically 14)
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "RSI period must be > 0");
        let name = if source == PriceSource::Close {
            format!("rsi_{}", period)
        } else {
            format!("rsi_{}_{}", period, source)
        };
        Self {
            period,
            source,
            name: name.clone(),
            output: Series::with_warmup(
                &name,
                MaximumBarsLookBack::default(),
                period + 1, // Need period + 1 bars for first RSI
            ),
            avg_gain: None,
            avg_loss: None,
            prev_price: None,
            gains: Vec::with_capacity(period),
            losses: Vec::with_capacity(period),
        }
    }

    /// Get the period of this RSI
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get the price source of this RSI
    pub fn source(&self) -> PriceSource {
        self.source
    }

    /// Get the current average gain
    pub fn avg_gain(&self) -> Option<Decimal> {
        self.avg_gain
    }

    /// Get the current average loss
    pub fn avg_loss(&self) -> Option<Decimal> {
        self.avg_loss
    }

    /// Calculate RSI from average gain and loss
    fn calculate_rsi(avg_gain: Decimal, avg_loss: Decimal) -> Decimal {
        if avg_gain == Decimal::ZERO && avg_loss == Decimal::ZERO {
            // No movement at all -> neutral RSI = 50
            return Decimal::from(50);
        }
        if avg_loss == Decimal::ZERO {
            // All gains, no losses -> RSI = 100
            return HUNDRED;
        }
        if avg_gain == Decimal::ZERO {
            // All losses, no gains -> RSI = 0
            return Decimal::ZERO;
        }

        let rs = avg_gain / avg_loss;
        HUNDRED - (HUNDRED / (Decimal::ONE + rs))
    }
}

impl Transform for Rsi {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period + 1
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        let current_price = self.source.get_current(bars)?;

        // Calculate gain/loss from previous price
        if let Some(prev) = self.prev_price {
            let change = current_price - prev;
            let gain = if change > Decimal::ZERO {
                change
            } else {
                Decimal::ZERO
            };
            let loss = if change < Decimal::ZERO {
                change.abs()
            } else {
                Decimal::ZERO
            };

            match &self.avg_gain {
                None => {
                    // Still collecting initial period data
                    self.gains.push(gain);
                    self.losses.push(loss);

                    if self.gains.len() >= self.period {
                        // Calculate initial averages (SMA)
                        let sum_gain: Decimal = self.gains.iter().sum();
                        let sum_loss: Decimal = self.losses.iter().sum();
                        let period_dec = Decimal::from(self.period);

                        let initial_avg_gain = sum_gain / period_dec;
                        let initial_avg_loss = sum_loss / period_dec;

                        self.avg_gain = Some(initial_avg_gain);
                        self.avg_loss = Some(initial_avg_loss);

                        let rsi = Self::calculate_rsi(initial_avg_gain, initial_avg_loss);
                        self.output.push(rsi);
                        self.prev_price = Some(current_price);
                        return Some(rsi);
                    }
                }
                Some(prev_avg_gain) => {
                    // Wilder's smoothing method (similar to EMA)
                    // AvgGain = (PrevAvgGain * (period - 1) + CurrentGain) / period
                    let period_dec = Decimal::from(self.period);
                    let smoothing = period_dec - Decimal::ONE;

                    let new_avg_gain = (*prev_avg_gain * smoothing + gain) / period_dec;
                    let new_avg_loss = (self.avg_loss.unwrap() * smoothing + loss) / period_dec;

                    self.avg_gain = Some(new_avg_gain);
                    self.avg_loss = Some(new_avg_loss);

                    let rsi = Self::calculate_rsi(new_avg_gain, new_avg_loss);
                    self.output.push(rsi);
                    self.prev_price = Some(current_price);
                    return Some(rsi);
                }
            }
        }

        self.prev_price = Some(current_price);
        None
    }

    fn output(&self) -> &Series<Decimal> {
        &self.output
    }

    fn reset(&mut self) {
        self.output.reset();
        self.avg_gain = None;
        self.avg_loss = None;
        self.prev_price = None;
        self.gains.clear();
        self.losses.clear();
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

    fn compute_incrementally(rsi: &mut Rsi, closes: &[Decimal]) -> Option<Decimal> {
        let mut result = None;
        for i in 1..=closes.len() {
            let ctx = create_bars_with_closes(&closes[..i]);
            result = rsi.compute(&ctx);
        }
        result
    }

    #[test]
    fn test_rsi_warmup_period() {
        let rsi = Rsi::new(14);
        assert_eq!(rsi.warmup_period(), 15);

        let rsi5 = Rsi::new(5);
        assert_eq!(rsi5.warmup_period(), 6);
    }

    #[test]
    fn test_rsi_name() {
        let rsi = Rsi::new(14);
        assert_eq!(rsi.name(), "rsi_14");
    }

    #[test]
    fn test_rsi_insufficient_data() {
        let mut rsi = Rsi::new(5);

        // Need 6 bars for RSI(5), only have 4
        let closes = vec![dec!(100), dec!(101), dec!(102), dec!(103)];
        let result = compute_incrementally(&mut rsi, &closes);

        assert_eq!(result, None);
    }

    #[test]
    fn test_rsi_all_gains() {
        let mut rsi = Rsi::new(3);

        // Monotonically increasing prices (all gains)
        let closes = vec![dec!(100), dec!(101), dec!(102), dec!(103)];
        let result = compute_incrementally(&mut rsi, &closes);

        // All gains, no losses -> RSI = 100
        assert_eq!(result, Some(dec!(100)));
    }

    #[test]
    fn test_rsi_all_losses() {
        let mut rsi = Rsi::new(3);

        // Monotonically decreasing prices (all losses)
        let closes = vec![dec!(103), dec!(102), dec!(101), dec!(100)];
        let result = compute_incrementally(&mut rsi, &closes);

        // All losses, no gains -> RSI = 0
        assert_eq!(result, Some(dec!(0)));
    }

    #[test]
    fn test_rsi_equal_gains_losses() {
        let mut rsi = Rsi::new(4);

        // Alternating gains and losses of equal magnitude
        // Changes: +2, -2, +2, -2 (4 changes for period 4)
        let closes = vec![dec!(100), dec!(102), dec!(100), dec!(102), dec!(100)];
        let result = compute_incrementally(&mut rsi, &closes);

        // Equal gains and losses -> RS = 1 -> RSI = 50
        assert_eq!(result, Some(dec!(50)));
    }

    #[test]
    fn test_rsi_bounded_0_to_100() {
        let mut rsi = Rsi::new(3);

        // Various price sequences
        let sequences = vec![
            vec![dec!(100), dec!(150), dec!(200), dec!(250)], // Strong uptrend
            vec![dec!(250), dec!(200), dec!(150), dec!(100)], // Strong downtrend
            vec![dec!(100), dec!(105), dec!(103), dec!(108)], // Mixed
        ];

        for closes in sequences {
            rsi.reset();
            let result = compute_incrementally(&mut rsi, &closes);
            if let Some(value) = result {
                assert!(
                    value >= dec!(0) && value <= dec!(100),
                    "RSI {} out of bounds for sequence {:?}",
                    value,
                    closes
                );
            }
        }
    }

    #[test]
    fn test_rsi_no_change() {
        let mut rsi = Rsi::new(3);

        // Constant price (no change)
        let closes = vec![dec!(100), dec!(100), dec!(100), dec!(100)];
        let result = compute_incrementally(&mut rsi, &closes);

        // No gains, no losses -> avg_gain = 0, avg_loss = 0
        // When both are zero, RSI is neutral = 50
        assert_eq!(result, Some(dec!(50)));
    }

    #[test]
    fn test_rsi_smoothing() {
        let mut rsi = Rsi::new(3);

        // First RSI value
        let closes1 = vec![dec!(100), dec!(102), dec!(101), dec!(104)];
        let first = compute_incrementally(&mut rsi, &closes1);
        assert!(first.is_some());

        // Second RSI value (uses smoothing)
        let closes2 = vec![dec!(100), dec!(102), dec!(101), dec!(104), dec!(103)];
        rsi.reset();
        let second = compute_incrementally(&mut rsi, &closes2);
        assert!(second.is_some());

        // Values should be different due to different price sequences
        // and smoothing effect
    }

    #[test]
    fn test_rsi_reset() {
        let mut rsi = Rsi::new(3);

        let closes = vec![dec!(100), dec!(101), dec!(102), dec!(103)];
        compute_incrementally(&mut rsi, &closes);

        assert!(rsi.current().is_some());
        assert!(rsi.avg_gain.is_some());
        assert!(rsi.avg_loss.is_some());
        assert!(rsi.prev_price.is_some());

        rsi.reset();

        assert!(rsi.current().is_none());
        assert!(rsi.avg_gain.is_none());
        assert!(rsi.avg_loss.is_none());
        assert!(rsi.prev_price.is_none());
        assert!(rsi.gains.is_empty());
        assert!(rsi.losses.is_empty());
    }

    #[test]
    fn test_rsi_historical_access() {
        let mut rsi = Rsi::new(3);

        // Build up multiple RSI values
        let closes = vec![
            dec!(100),
            dec!(102),
            dec!(101),
            dec!(104), // First RSI
            dec!(103), // Second RSI
            dec!(106), // Third RSI
        ];
        compute_incrementally(&mut rsi, &closes);

        // Should have 3 RSI values
        assert_eq!(rsi.output().count(), 3);
        assert!(rsi.get(0).is_some()); // Most recent
        assert!(rsi.get(1).is_some()); // Previous
        assert!(rsi.get(2).is_some()); // Two bars ago
    }

    #[test]
    fn test_rsi_period_accessor() {
        let rsi = Rsi::new(14);
        assert_eq!(rsi.period(), 14);
    }

    #[test]
    #[should_panic(expected = "RSI period must be > 0")]
    fn test_rsi_zero_period_panics() {
        Rsi::new(0);
    }

    #[test]
    fn test_rsi_is_ready() {
        let rsi = Rsi::new(5);

        let ctx_small = create_bars_with_closes(&[dec!(10), dec!(20), dec!(30)]);
        assert!(!rsi.is_ready(&ctx_small));

        let ctx_exact =
            create_bars_with_closes(&[dec!(10), dec!(20), dec!(30), dec!(40), dec!(50), dec!(60)]);
        assert!(rsi.is_ready(&ctx_exact));
    }

    // ========== Custom Source Tests ==========

    #[test]
    fn test_rsi_source_name() {
        let rsi_close = Rsi::new(14);
        assert_eq!(rsi_close.name(), "rsi_14");

        let rsi_typical = Rsi::with_source(14, PriceSource::Typical);
        assert_eq!(rsi_typical.name(), "rsi_14_typical");
    }

    #[test]
    fn test_rsi_source_accessor() {
        let rsi = Rsi::new(14);
        assert_eq!(rsi.source(), PriceSource::Close);

        let rsi_high = Rsi::with_source(14, PriceSource::High);
        assert_eq!(rsi_high.source(), PriceSource::High);
    }
}
