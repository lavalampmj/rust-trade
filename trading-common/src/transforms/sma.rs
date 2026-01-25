// Simple Moving Average Transform

use crate::series::{BarsContext, MaximumBarsLookBack, Series};
use rust_decimal::Decimal;

use super::source::PriceSource;
use super::Transform;

/// Simple Moving Average transform.
///
/// Calculates the arithmetic mean of prices over the last N bars.
/// By default uses close prices, but can be configured to use any price source.
///
/// # Formula
///
/// SMA = (P1 + P2 + ... + Pn) / n
///
/// where P1..Pn are the last n prices from the selected source.
///
/// # Example
///
/// ```no_run
/// use trading_common::transforms::{Transform, Sma, PriceSource};
///
/// // SMA of close prices (default)
/// let mut sma = Sma::new(20);
///
/// // SMA of high prices
/// let mut sma_high = Sma::with_source(20, PriceSource::High);
///
/// // SMA of typical price (H+L+C)/3
/// let mut sma_typical = Sma::with_source(20, PriceSource::Typical);
/// ```
#[derive(Debug, Clone)]
pub struct Sma {
    period: usize,
    source: PriceSource,
    name: String,
    output: Series<Decimal>,
}

impl Sma {
    /// Create a new SMA transform with the specified period.
    /// Uses close prices by default.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars to average (must be > 0)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn new(period: usize) -> Self {
        Self::with_source(period, PriceSource::Close)
    }

    /// Create a new SMA transform with a custom price source.
    ///
    /// # Arguments
    ///
    /// * `period` - Number of bars to average (must be > 0)
    /// * `source` - The price source to use (Open, High, Low, Close, etc.)
    ///
    /// # Panics
    ///
    /// Panics if period is 0.
    pub fn with_source(period: usize, source: PriceSource) -> Self {
        assert!(period > 0, "SMA period must be > 0");
        let name = if source == PriceSource::Close {
            format!("sma_{}", period)
        } else {
            format!("sma_{}_{}", period, source)
        };
        Self {
            period,
            source,
            name: name.clone(),
            output: Series::with_warmup(&name, MaximumBarsLookBack::default(), period),
        }
    }

    /// Get the period of this SMA
    pub fn period(&self) -> usize {
        self.period
    }

    /// Get the price source of this SMA
    pub fn source(&self) -> PriceSource {
        self.source
    }
}

impl Transform for Sma {
    type Output = Decimal;

    fn name(&self) -> &str {
        &self.name
    }

    fn warmup_period(&self) -> usize {
        self.period
    }

    fn compute(&mut self, bars: &BarsContext) -> Option<Decimal> {
        let value = self.source.sma(bars, self.period)?;
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
    fn test_sma_basic_calculation() {
        let closes = vec![dec!(10), dec!(20), dec!(30), dec!(40), dec!(50)];
        let ctx = create_bars_with_closes(&closes);

        let mut sma = Sma::new(3);
        let value = sma.compute(&ctx);

        // SMA(3) of [30, 40, 50] = (30 + 40 + 50) / 3 = 40
        assert_eq!(value, Some(dec!(40)));
    }

    #[test]
    fn test_sma_insufficient_data() {
        let closes = vec![dec!(10), dec!(20)];
        let ctx = create_bars_with_closes(&closes);

        let mut sma = Sma::new(5);
        let value = sma.compute(&ctx);

        assert_eq!(value, None);
    }

    #[test]
    fn test_sma_exact_period_data() {
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut sma = Sma::new(3);
        let value = sma.compute(&ctx);

        // SMA(3) of [10, 20, 30] = 20
        assert_eq!(value, Some(dec!(20)));
    }

    #[test]
    fn test_sma_warmup_period() {
        let sma = Sma::new(20);
        assert_eq!(sma.warmup_period(), 20);
    }

    #[test]
    fn test_sma_name() {
        let sma = Sma::new(14);
        assert_eq!(sma.name(), "sma_14");
    }

    #[test]
    fn test_sma_historical_access() {
        let mut sma = Sma::new(3);

        // Build up history by computing multiple times
        for n in 3..=6 {
            let closes: Vec<Decimal> = (1..=n).map(|i| Decimal::from(i * 10)).collect();
            let ctx = create_bars_with_closes(&closes);
            sma.compute(&ctx);
        }

        // Should have 4 values in output
        assert_eq!(sma.output().count(), 4);

        // Most recent: SMA of [40, 50, 60] = 50
        assert_eq!(sma.get(0), Some(dec!(50)));
    }

    #[test]
    fn test_sma_reset() {
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut sma = Sma::new(3);
        sma.compute(&ctx);

        assert!(sma.current().is_some());
        assert_eq!(sma.output().count(), 1);

        sma.reset();

        assert!(sma.current().is_none());
        assert_eq!(sma.output().count(), 0);
    }

    #[test]
    fn test_sma_is_ready() {
        let sma = Sma::new(5);

        let ctx_small = create_bars_with_closes(&[dec!(10), dec!(20)]);
        assert!(!sma.is_ready(&ctx_small));

        let ctx_exact =
            create_bars_with_closes(&[dec!(10), dec!(20), dec!(30), dec!(40), dec!(50)]);
        assert!(sma.is_ready(&ctx_exact));

        let ctx_large =
            create_bars_with_closes(&[dec!(10), dec!(20), dec!(30), dec!(40), dec!(50), dec!(60)]);
        assert!(sma.is_ready(&ctx_large));
    }

    #[test]
    #[should_panic(expected = "SMA period must be > 0")]
    fn test_sma_zero_period_panics() {
        Sma::new(0);
    }

    #[test]
    fn test_sma_period_accessor() {
        let sma = Sma::new(14);
        assert_eq!(sma.period(), 14);
    }

    // ========== Custom Source Tests ==========

    #[test]
    fn test_sma_with_high_source() {
        // Bars with different O/H/L/C values
        let bars = vec![
            (dec!(100), dec!(110), dec!(90), dec!(105)),
            (dec!(105), dec!(120), dec!(95), dec!(115)),
            (dec!(115), dec!(130), dec!(100), dec!(125)),
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut sma_high = Sma::with_source(3, PriceSource::High);
        let value = sma_high.compute(&ctx);

        // SMA(3) of highs [110, 120, 130] = 120
        assert_eq!(value, Some(dec!(120)));
    }

    #[test]
    fn test_sma_with_low_source() {
        let bars = vec![
            (dec!(100), dec!(110), dec!(90), dec!(105)),
            (dec!(105), dec!(120), dec!(95), dec!(115)),
            (dec!(115), dec!(130), dec!(100), dec!(125)),
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut sma_low = Sma::with_source(3, PriceSource::Low);
        let value = sma_low.compute(&ctx);

        // SMA(3) of lows [90, 95, 100] = 95
        assert_eq!(value, Some(dec!(95)));
    }

    #[test]
    fn test_sma_with_typical_price() {
        let bars = vec![
            (dec!(100), dec!(110), dec!(90), dec!(100)), // Typical = (110+90+100)/3 = 100
            (dec!(100), dec!(120), dec!(90), dec!(105)), // Typical = (120+90+105)/3 = 105
            (dec!(105), dec!(130), dec!(100), dec!(115)), // Typical = (130+100+115)/3 = 115
        ];
        let ctx = create_bars_with_ohlc(&bars);

        let mut sma_typical = Sma::with_source(3, PriceSource::Typical);
        let value = sma_typical.compute(&ctx);

        // SMA(3) of typical [100, 105, 115] = (100+105+115)/3 = 106.666...
        let expected = (dec!(100) + dec!(105) + dec!(115)) / dec!(3);
        assert_eq!(value, Some(expected));
    }

    #[test]
    fn test_sma_source_name() {
        let sma_close = Sma::new(14);
        assert_eq!(sma_close.name(), "sma_14");

        let sma_high = Sma::with_source(14, PriceSource::High);
        assert_eq!(sma_high.name(), "sma_14_high");

        let sma_typical = Sma::with_source(14, PriceSource::Typical);
        assert_eq!(sma_typical.name(), "sma_14_typical");
    }

    #[test]
    fn test_sma_source_accessor() {
        let sma = Sma::new(14);
        assert_eq!(sma.source(), PriceSource::Close);

        let sma_high = Sma::with_source(14, PriceSource::High);
        assert_eq!(sma_high.source(), PriceSource::High);
    }

    #[test]
    fn test_sma_default_is_close() {
        let closes = vec![dec!(10), dec!(20), dec!(30)];
        let ctx = create_bars_with_closes(&closes);

        let mut sma_new = Sma::new(3);
        let mut sma_explicit = Sma::with_source(3, PriceSource::Close);

        let val1 = sma_new.compute(&ctx);
        let val2 = sma_explicit.compute(&ctx);

        assert_eq!(val1, val2);
    }
}
