// Exponential Moving Average Transform

use crate::series::{BarsContext, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::source::PriceSource;
use super::Transform;

/// Exponential Moving Average transform.
///
/// Calculates the exponentially weighted moving average of prices.
/// Gives more weight to recent prices compared to SMA.
/// By default uses close prices, but can be configured to use any price source.
///
/// # Formula
///
/// EMA_today = (Price_today * k) + (EMA_yesterday * (1 - k))
///
/// where k = 2 / (period + 1) is the smoothing factor.
///
/// # Initialization
///
/// The first EMA value is set to the SMA of the first `period` prices.
///
/// # Example
///
/// ```ignore
/// use trading_common::transforms::{Transform, Ema, PriceSource};
///
/// // EMA of close prices (default)
/// let mut ema = Ema::new(20);
///
/// // EMA of high prices
/// let mut ema_high = Ema::with_source(20, PriceSource::High);
/// ```
#[derive(Debug, Clone)]
pub struct Ema {
    period: usize,
    source: PriceSource,
    multiplier: Decimal,
    name: String,
    output: Series<Decimal>,
    /// Previous EMA value for incremental calculation
    prev_ema: Option<Decimal>,
}

impl Ema {
    /// Create a new EMA transform with the specified period.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars for EMA calculation (must be > 0)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new EMA transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars for EMA calculation (must be > 0)
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "EMA period must be > 0");

        // Multiplier: k = 2 / (period + 1)
        let multiplier = Decimal::TWO / Decimal::from(period + 1);

        let name = if source == PriceSource::Close {
            format!("ema_{}", period)
        } else {
            format!("ema_{}_{}", period, source)
        };

        Self {
            period,
            source,
            multiplier,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), period),
            prev_ema: None,
        }
    }

    /// Get the period of this EMA
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get the price source of this EMA
    pub fn source(&self) -> PriceSource {
        self.source
    }

    /// Get the smoothing multiplier
    pub fn multiplier(&self) -> Decimal {
        self.multiplier
    }
}

impl Transform for Ema {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        if bars.count() < self.period {
            return None;
        }

        let current_price = self.source.get_current(bars)?;

        let ema_value = match self.prev_ema {
            Some(prev) => {
                // EMA = (Price * k) + (PrevEMA * (1 - k))
                (current_price * self.multiplier) + (prev * (Decimal::ONE - self.multiplier))
            }
            None => {
                // Initialize with SMA
                self.source.sma(bars, self.period)?
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
        self.output.reset();
        self.prev_ema = None;
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

    #[test]
    fn test_ema_first_value_is_sma() {
        // First EMA value should equal SMA
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut ema = Ema::new(3);
        let value = ema.compute(&ctx);

        // SMA of [10, 20, 30] = 20
        assert_eq!(value, Some(dec!(20)));
    }

    #[test]
    fn test_ema_subsequent_values() {
        let mut ema = Ema::new(3);

        // First compute: initializes with SMA
        let ctx1 = create_bars_with_closes(&[dec!(10), dec!(20), dec!(30)]);
        let val1 = ema.compute(&ctx1);
        assert_eq!(val1, Some(dec!(20))); // SMA = 20

        // Second compute: EMA calculation
        // k = 2 / (3 + 1) = 0.5
        // EMA = (40 * 0.5) + (20 * 0.5) = 20 + 10 = 30
        let ctx2 = create_bars_with_closes(&[dec!(10), dec!(20), dec!(30), dec!(40)]);
        let val2 = ema.compute(&ctx2);
        assert_eq!(val2, Some(dec!(30)));
    }

    #[test]
    fn test_ema_insufficient_data() {
        let closes = vec![dec!(10), dec!(20)];
        let ctx = create_bars_with_closes(&closes);

        let mut ema = Ema::new(5);
        let value = ema.compute(&ctx);

        assert_eq!(value, None);
    }

    #[test]
    fn test_ema_multiplier_calculation() {
        // Period 3: k = 2/(3+1) = 0.5
        let ema3 = Ema::new(3);
        assert_eq!(ema3.multiplier(), dec!(0.5));

        // Period 9: k = 2/(9+1) = 0.2
        let ema9 = Ema::new(9);
        assert_eq!(ema9.multiplier(), dec!(0.2));

        // Period 19: k = 2/(19+1) = 0.1
        let ema19 = Ema::new(19);
        assert_eq!(ema19.multiplier(), dec!(0.1));
    }

    #[test]
    fn test_ema_warmup_period() {
        let ema = Ema::new(20);
        assert_eq!(ema.warmup_period(), 20);
    }

    #[test]
    fn test_ema_name() {
        let ema = Ema::new(14);
        assert_eq!(ema.name(), "ema_14");
    }

    #[test]
    fn test_ema_reset() {
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut ema = Ema::new(3);
        ema.compute(&ctx);

        assert!(ema.current().is_some());
        assert!(ema.prev_ema.is_some());

        ema.reset();

        assert!(ema.current().is_none());
        assert!(ema.prev_ema.is_none());
        assert_eq!(ema.output().count(), 0);
    }

    #[test]
    fn test_ema_convergence_to_price() {
        // EMA should converge toward recent prices over time
        let mut ema = Ema::new(3);
        let constant_price = dec!(100);

        // Feed many bars at constant price
        for n in 3..=20 {
            let closes: Vec<Decimal> = vec![constant_price; n];
            let ctx = create_bars_with_closes(&closes);
            ema.compute(&ctx);
        }

        // EMA should be very close to 100
        let current = ema.current().unwrap();
        assert!((current - constant_price).abs() < dec!(0.01));
    }

    #[test]
    fn test_ema_weights_recent_prices_more() {
        let mut ema = Ema::new(3);

        // Start with 3 bars at 100
        let ctx1 = create_bars_with_closes(&[dec!(100), dec!(100), dec!(100)]);
        ema.compute(&ctx1); // EMA = 100

        // Add a spike to 200
        let ctx2 = create_bars_with_closes(&[dec!(100), dec!(100), dec!(100), dec!(200)]);
        let ema_after_spike = ema.compute(&ctx2).unwrap();

        // EMA should move toward 200, but not reach it
        // k = 0.5, so EMA = (200 * 0.5) + (100 * 0.5) = 150
        assert_eq!(ema_after_spike, dec!(150));
    }

    #[test]
    fn test_ema_is_ready() {
        let ema = Ema::new(5);

        let ctx_small = create_bars_with_closes(&[dec!(10), dec!(20)]);
        assert!(!ema.is_ready(&ctx_small));

        let ctx_exact =
            create_bars_with_closes(&[dec!(10), dec!(20), dec!(30), dec!(40), dec!(50)]);
        assert!(ema.is_ready(&ctx_exact));
    }

    #[test]
    #[should_panic(expected = "EMA period must be > 0")]
    fn test_ema_zero_period_panics() {
        Ema::new(0);
    }

    #[test]
    fn test_ema_period_accessor() {
        let ema = Ema::new(14);
        assert_eq!(ema.period(), 14);
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
    fn test_ema_with_high_source() {
        let bars = vec![
            (dec!(100), dec!(110), dec!(90), dec!(105)),
            (dec!(105), dec!(120), dec!(95), dec!(115)),
            (dec!(115), dec!(130), dec!(100), dec!(125)),
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut ema_high = Ema::with_source(3, PriceSource::High);
        let value = ema_high.compute(&ctx);

        // First EMA is SMA of highs [110, 120, 130] = 120
        assert_eq!(value, Some(dec!(120)));
    }

    #[test]
    fn test_ema_source_name() {
        let ema_close = Ema::new(14);
        assert_eq!(ema_close.name(), "ema_14");

        let ema_high = Ema::with_source(14, PriceSource::High);
        assert_eq!(ema_high.name(), "ema_14_high");
    }

    #[test]
    fn test_ema_source_accessor() {
        let ema = Ema::new(14);
        assert_eq!(ema.source(), PriceSource::Close);

        let ema_high = Ema::with_source(14, PriceSource::High);
        assert_eq!(ema_high.source(), PriceSource::High);
    }
}
