// BarsContext - OHLCV series wrapper for strategy access
// Provides synchronized access to price data with built-in indicator helpers

use super::{DecimalSeriesExt, MaximumBarsLookBack, Series, SeriesValue};
use crate::data::types::BarData;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::any::Any;
use std::collections::HashMap;

/// Wrapper providing synchronized OHLCV series access for strategies
///
/// BarsContext maintains OHLCV price series that update together on each bar,
/// ensuring alignment across all series including custom indicators.
///
/// # Example
/// ```text
/// // In strategy on_bar_data:
/// fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
///     // Access OHLCV with reverse indexing
///     let current_close = bars.close[0];      // Most recent
///     let prev_close = bars.close[1];         // Previous bar
///     let close_10_bars_ago = bars.close[10]; // 10 bars ago
///
///     // Built-in helpers
///     let sma20 = bars.sma(20);
///     let highest = bars.highest_high(14);
///
///     Signal::Hold
/// }
/// ```
#[derive(Debug)]
pub struct BarsContext {
    /// Open prices
    pub open: Series<Decimal>,
    /// High prices
    pub high: Series<Decimal>,
    /// Low prices
    pub low: Series<Decimal>,
    /// Close prices
    pub close: Series<Decimal>,
    /// Volume
    pub volume: Series<Decimal>,
    /// Bar timestamps
    pub time: Series<DateTime<Utc>>,
    /// Trade count per bar
    pub trade_count: Series<u64>,

    /// Current bar index
    current_bar: u64,

    /// Symbol being processed
    symbol: String,

    /// Custom series storage (type-erased for flexibility)
    custom_series: HashMap<String, Box<dyn Any + Send + Sync>>,

    /// Max lookback for OHLCV series
    max_lookback: MaximumBarsLookBack,
}

impl BarsContext {
    /// Create a new BarsContext with default lookback (256 bars)
    pub fn new(symbol: &str) -> Self {
        Self::with_lookback(symbol, MaximumBarsLookBack::default())
    }

    /// Create a new BarsContext with custom lookback
    pub fn with_lookback(symbol: &str, max_lookback: MaximumBarsLookBack) -> Self {
        Self {
            open: Series::with_lookback("Open", max_lookback),
            high: Series::with_lookback("High", max_lookback),
            low: Series::with_lookback("Low", max_lookback),
            close: Series::with_lookback("Close", max_lookback),
            volume: Series::with_lookback("Volume", max_lookback),
            time: Series::with_lookback("Time", max_lookback),
            trade_count: Series::with_lookback("TradeCount", max_lookback),
            current_bar: 0,
            symbol: symbol.to_string(),
            custom_series: HashMap::new(),
            max_lookback,
        }
    }

    /// Update all OHLCV series with new bar data
    ///
    /// Called by the backtest engine before invoking strategy.on_bar_data()
    /// This ensures all series are synchronized at the same bar index.
    pub fn on_bar_update(&mut self, bar_data: &BarData) {
        let ohlc = &bar_data.ohlc_bar;

        self.open.push(ohlc.open);
        self.high.push(ohlc.high);
        self.low.push(ohlc.low);
        self.close.push(ohlc.close);
        self.volume.push(ohlc.volume);
        self.time.push(ohlc.timestamp);
        self.trade_count.push(ohlc.trade_count);

        self.current_bar += 1;

        // Update symbol if this is the first bar
        if self.current_bar == 1 {
            self.symbol = ohlc.symbol.clone();
        }
    }

    /// Get current bar index (0-based, increments with each bar)
    pub fn current_bar(&self) -> u64 {
        self.current_bar
    }

    /// Get the symbol being processed
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Get number of bars available
    pub fn count(&self) -> usize {
        self.close.count()
    }

    /// Check if we have enough data for given lookback
    pub fn has_bars(&self, lookback: usize) -> bool {
        self.close.count() >= lookback
    }

    // ========================================================================
    // Warmup / Ready State
    // ========================================================================

    /// Check if BarsContext has at least 1 bar (basic readiness)
    ///
    /// Returns true when we have at least one bar of data.
    /// For more specific readiness checks, use `is_ready_for(lookback)`.
    pub fn is_ready(&self) -> bool {
        self.count() > 0
    }

    /// Check if we have enough bars for a specific lookback period
    ///
    /// Use this to check if a specific indicator calculation will succeed.
    /// This is the primary method strategies should use to check warmup status.
    ///
    /// # Example
    /// ```text
    /// fn is_ready(&self, bars: &BarsContext) -> bool {
    ///     // Ready when we have enough data for long SMA
    ///     bars.is_ready_for(self.long_period)
    /// }
    /// ```
    pub fn is_ready_for(&self, lookback: usize) -> bool {
        self.count() >= lookback
    }

    /// Calculate SMA and return (value, is_ready) tuple
    ///
    /// Useful when you want to check both the value and whether it's valid
    /// in a single call.
    pub fn sma_with_ready(&self, period: usize) -> (Option<Decimal>, bool) {
        let value = self.close.sma(period);
        let is_ready = self.count() >= period;
        (value, is_ready)
    }

    /// Get highest high with ready status
    pub fn highest_high_with_ready(&self, period: usize) -> (Option<Decimal>, bool) {
        let value = self.high.highest(period);
        let is_ready = self.count() >= period;
        (value, is_ready)
    }

    /// Get lowest low with ready status
    pub fn lowest_low_with_ready(&self, period: usize) -> (Option<Decimal>, bool) {
        let value = self.low.lowest(period);
        let is_ready = self.count() >= period;
        (value, is_ready)
    }

    /// Reset all series (for new backtest run)
    pub fn reset(&mut self) {
        self.open.reset();
        self.high.reset();
        self.low.reset();
        self.close.reset();
        self.volume.reset();
        self.time.reset();
        self.trade_count.reset();
        self.current_bar = 0;
        self.custom_series.clear();
    }

    // ========================================================================
    // Convenience indicator methods
    // ========================================================================

    /// Calculate Simple Moving Average of close prices
    pub fn sma(&self, period: usize) -> Option<Decimal> {
        self.close.sma(period)
    }

    /// Calculate SMA of a specific series (open, high, low, volume)
    pub fn sma_of(&self, series: &Series<Decimal>, period: usize) -> Option<Decimal> {
        series.sma(period)
    }

    /// Get highest high over last N bars
    pub fn highest_high(&self, period: usize) -> Option<Decimal> {
        self.high.highest(period)
    }

    /// Get lowest low over last N bars
    pub fn lowest_low(&self, period: usize) -> Option<Decimal> {
        self.low.lowest(period)
    }

    /// Get highest close over last N bars
    pub fn highest_close(&self, period: usize) -> Option<Decimal> {
        self.close.highest(period)
    }

    /// Get lowest close over last N bars
    pub fn lowest_close(&self, period: usize) -> Option<Decimal> {
        self.close.lowest(period)
    }

    /// Calculate price range (high - low) of current bar
    pub fn range(&self) -> Option<Decimal> {
        if self.high.is_empty() || self.low.is_empty() {
            return None;
        }
        Some(self.high[0] - self.low[0])
    }

    /// Calculate average true range (simplified: uses high-low)
    pub fn average_range(&self, period: usize) -> Option<Decimal> {
        if self.count() < period || period == 0 {
            return None;
        }

        let sum: Decimal = (0..period)
            .filter_map(|i| {
                let h = self.high.get(i)?;
                let l = self.low.get(i)?;
                Some(*h - *l)
            })
            .sum();

        Some(sum / Decimal::from(period))
    }

    /// Check if current bar is an up bar (close > open)
    pub fn is_up_bar(&self) -> bool {
        if self.close.is_empty() || self.open.is_empty() {
            return false;
        }
        self.close[0] > self.open[0]
    }

    /// Check if current bar is a down bar (close < open)
    pub fn is_down_bar(&self) -> bool {
        if self.close.is_empty() || self.open.is_empty() {
            return false;
        }
        self.close[0] < self.open[0]
    }

    /// Get price change from previous close
    pub fn change(&self) -> Option<Decimal> {
        if self.close.count() < 2 {
            return None;
        }
        Some(self.close[0] - self.close[1])
    }

    /// Get percentage change from previous close
    pub fn percent_change(&self) -> Option<Decimal> {
        if self.close.count() < 2 {
            return None;
        }
        let prev = self.close[1];
        if prev == Decimal::ZERO {
            return None;
        }
        Some((self.close[0] - prev) / prev * Decimal::from(100))
    }

    // ========================================================================
    // Custom series management
    // ========================================================================

    /// Register a new custom series
    ///
    /// Custom series maintain alignment with OHLCV - when you push to a custom series,
    /// it aligns with the current bar, so `my_series[10]` corresponds to `close[10]`.
    ///
    /// # Example
    /// ```text
    /// bars.register_series::<Decimal>("MyIndicator");
    /// bars.push_to_series("MyIndicator", calculated_value);
    /// let value_10_bars_ago = bars.get_series::<Decimal>("MyIndicator").unwrap()[10];
    /// // This aligns with bars.close[10]
    /// ```
    pub fn register_series<T: SeriesValue>(&mut self, name: &str) {
        let series: Series<T> = Series::with_lookback(name, self.max_lookback);
        self.custom_series
            .insert(name.to_string(), Box::new(series));
    }

    /// Get a reference to a custom series
    pub fn get_series<T: SeriesValue>(&self, name: &str) -> Option<&Series<T>> {
        self.custom_series
            .get(name)
            .and_then(|boxed| boxed.downcast_ref::<Series<T>>())
    }

    /// Get a mutable reference to a custom series
    pub fn get_series_mut<T: SeriesValue>(&mut self, name: &str) -> Option<&mut Series<T>> {
        self.custom_series
            .get_mut(name)
            .and_then(|boxed| boxed.downcast_mut::<Series<T>>())
    }

    /// Push a value to a custom series
    ///
    /// Note: You should call this once per bar to maintain alignment.
    pub fn push_to_series<T: SeriesValue>(&mut self, name: &str, value: T) -> bool {
        if let Some(series) = self.get_series_mut::<T>(name) {
            series.push(value);
            true
        } else {
            false
        }
    }

    /// Check if a custom series exists
    pub fn has_series(&self, name: &str) -> bool {
        self.custom_series.contains_key(name)
    }

    /// Get list of registered custom series names
    pub fn custom_series_names(&self) -> Vec<String> {
        self.custom_series.keys().cloned().collect()
    }
}

impl Clone for BarsContext {
    fn clone(&self) -> Self {
        // Clone only the built-in series; custom series are not cloneable
        // This is intentional - custom series should be re-registered in new context
        Self {
            open: self.open.clone(),
            high: self.high.clone(),
            low: self.low.clone(),
            close: self.close.clone(),
            volume: self.volume.clone(),
            time: self.time.clone(),
            trade_count: self.trade_count.clone(),
            current_bar: self.current_bar,
            symbol: self.symbol.clone(),
            custom_series: HashMap::new(), // Custom series not cloned
            max_lookback: self.max_lookback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{OHLCData, Timeframe};
    use std::str::FromStr;

    fn create_bar_data(
        open: &str,
        high: &str,
        low: &str,
        close: &str,
        volume: &str,
    ) -> BarData {
        let ohlc = OHLCData::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            Timeframe::OneMinute,
            Decimal::from_str(open).unwrap(),
            Decimal::from_str(high).unwrap(),
            Decimal::from_str(low).unwrap(),
            Decimal::from_str(close).unwrap(),
            Decimal::from_str(volume).unwrap(),
            10,
        );
        BarData::from_ohlc(&ohlc)
    }

    #[test]
    fn test_bars_context_new() {
        let ctx = BarsContext::new("BTCUSDT");
        assert_eq!(ctx.symbol(), "BTCUSDT");
        assert_eq!(ctx.current_bar(), 0);
        assert_eq!(ctx.count(), 0);
    }

    #[test]
    fn test_bars_context_on_bar_update() {
        let mut ctx = BarsContext::new("BTCUSDT");

        let bar1 = create_bar_data("50000", "50500", "49500", "50200", "100");
        let bar2 = create_bar_data("50200", "50800", "50100", "50600", "150");

        ctx.on_bar_update(&bar1);
        assert_eq!(ctx.count(), 1);
        assert_eq!(ctx.close[0], Decimal::from_str("50200").unwrap());

        ctx.on_bar_update(&bar2);
        assert_eq!(ctx.count(), 2);
        assert_eq!(ctx.close[0], Decimal::from_str("50600").unwrap());
        assert_eq!(ctx.close[1], Decimal::from_str("50200").unwrap());
    }

    #[test]
    fn test_bars_context_sma() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Add bars with closes: 10, 20, 30, 40, 50
        for close in &["10", "20", "30", "40", "50"] {
            let bar = create_bar_data(close, close, close, close, "100");
            ctx.on_bar_update(&bar);
        }

        // SMA(3) of close: (50 + 40 + 30) / 3 = 40
        assert_eq!(ctx.sma(3).unwrap(), Decimal::from(40));

        // SMA(5) of close: (10 + 20 + 30 + 40 + 50) / 5 = 30
        assert_eq!(ctx.sma(5).unwrap(), Decimal::from(30));
    }

    #[test]
    fn test_bars_context_highest_lowest() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Add bars
        ctx.on_bar_update(&create_bar_data("100", "110", "90", "105", "100"));
        ctx.on_bar_update(&create_bar_data("105", "120", "100", "115", "100"));
        ctx.on_bar_update(&create_bar_data("115", "125", "110", "120", "100"));

        // Highest high of last 3: max(125, 120, 110) = 125
        assert_eq!(ctx.highest_high(3).unwrap(), Decimal::from(125));

        // Lowest low of last 3: min(110, 100, 90) = 90
        assert_eq!(ctx.lowest_low(3).unwrap(), Decimal::from(90));
    }

    #[test]
    fn test_bars_context_up_down_bar() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Up bar: close > open
        ctx.on_bar_update(&create_bar_data("100", "110", "95", "108", "100"));
        assert!(ctx.is_up_bar());
        assert!(!ctx.is_down_bar());

        // Down bar: close < open
        ctx.on_bar_update(&create_bar_data("108", "110", "95", "100", "100"));
        assert!(!ctx.is_up_bar());
        assert!(ctx.is_down_bar());
    }

    #[test]
    fn test_bars_context_change() {
        let mut ctx = BarsContext::new("BTCUSDT");

        ctx.on_bar_update(&create_bar_data("100", "110", "95", "105", "100"));
        assert!(ctx.change().is_none()); // Need 2 bars

        ctx.on_bar_update(&create_bar_data("105", "115", "100", "112", "100"));
        assert_eq!(ctx.change().unwrap(), Decimal::from(7)); // 112 - 105

        let pct = ctx.percent_change().unwrap();
        // (112 - 105) / 105 * 100 ≈ 6.67
        assert!(pct > Decimal::from_str("6.6").unwrap());
        assert!(pct < Decimal::from_str("6.7").unwrap());
    }

    #[test]
    fn test_bars_context_custom_series() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Register custom series
        ctx.register_series::<Decimal>("MyIndicator");
        assert!(ctx.has_series("MyIndicator"));

        // Push values
        ctx.push_to_series("MyIndicator", Decimal::from(100));
        ctx.push_to_series("MyIndicator", Decimal::from(200));
        ctx.push_to_series("MyIndicator", Decimal::from(300));

        // Access via get_series
        let series = ctx.get_series::<Decimal>("MyIndicator").unwrap();
        assert_eq!(series[0], Decimal::from(300));
        assert_eq!(series[1], Decimal::from(200));
        assert_eq!(series[2], Decimal::from(100));
    }

    #[test]
    fn test_bars_context_custom_series_alignment() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Register indicator series
        ctx.register_series::<Decimal>("MySMA");

        // Simulate processing bars and calculating indicator
        for close in &["100", "110", "120", "130", "140"] {
            // First, update OHLCV
            ctx.on_bar_update(&create_bar_data(close, close, close, close, "50"));

            // Then calculate and push indicator value
            // (For this test, just push close value as indicator)
            ctx.push_to_series("MySMA", Decimal::from_str(close).unwrap());
        }

        // Both should have same count
        assert_eq!(ctx.close.count(), 5);
        let sma_series = ctx.get_series::<Decimal>("MySMA").unwrap();
        assert_eq!(sma_series.count(), 5);

        // And values should align
        assert_eq!(ctx.close[2], Decimal::from(120));
        assert_eq!(sma_series[2], Decimal::from(120));
    }

    #[test]
    fn test_bars_context_reset() {
        let mut ctx = BarsContext::new("BTCUSDT");

        ctx.on_bar_update(&create_bar_data("100", "110", "95", "105", "100"));
        ctx.register_series::<Decimal>("Custom");
        ctx.push_to_series("Custom", Decimal::from(50));

        assert_eq!(ctx.count(), 1);
        assert!(ctx.has_series("Custom"));

        ctx.reset();

        assert_eq!(ctx.count(), 0);
        assert_eq!(ctx.current_bar(), 0);
        assert!(!ctx.has_series("Custom")); // Custom series cleared
    }

    #[test]
    fn test_bars_context_with_custom_lookback() {
        let mut ctx = BarsContext::with_lookback("BTCUSDT", MaximumBarsLookBack::Fixed(3));

        // Add 5 bars
        for i in 1..=5 {
            let close = format!("{}", i * 100);
            ctx.on_bar_update(&create_bar_data(&close, &close, &close, &close, "50"));
        }

        // Should only have 3 bars due to eviction
        assert_eq!(ctx.count(), 3);
        assert_eq!(ctx.close[0], Decimal::from(500)); // Most recent
        assert_eq!(ctx.close[2], Decimal::from(300)); // Oldest remaining
    }

    #[test]
    fn test_bars_context_average_range() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Add bars with specific high-low ranges
        ctx.on_bar_update(&create_bar_data("100", "110", "90", "100", "50")); // range: 20
        ctx.on_bar_update(&create_bar_data("100", "120", "100", "110", "50")); // range: 20
        ctx.on_bar_update(&create_bar_data("110", "130", "100", "120", "50")); // range: 30

        // Average range of last 3: (30 + 20 + 20) / 3 ≈ 23.33
        let avg = ctx.average_range(3).unwrap();
        assert!(avg > Decimal::from(23));
        assert!(avg < Decimal::from(24));
    }

    #[test]
    fn test_bars_context_has_bars() {
        let mut ctx = BarsContext::new("BTCUSDT");

        assert!(!ctx.has_bars(1));

        ctx.on_bar_update(&create_bar_data("100", "110", "90", "100", "50"));
        assert!(ctx.has_bars(1));
        assert!(!ctx.has_bars(2));

        ctx.on_bar_update(&create_bar_data("100", "110", "90", "100", "50"));
        assert!(ctx.has_bars(2));
    }

    #[test]
    fn test_bars_context_is_ready() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Empty context is not ready
        assert!(!ctx.is_ready());

        // After adding one bar, is_ready returns true
        ctx.on_bar_update(&create_bar_data("100", "110", "90", "100", "50"));
        assert!(ctx.is_ready());
    }

    #[test]
    fn test_bars_context_is_ready_for() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Not ready for any lookback when empty
        assert!(!ctx.is_ready_for(1));
        assert!(!ctx.is_ready_for(5));

        // Add bars one by one
        for i in 1..=5 {
            ctx.on_bar_update(&create_bar_data("100", "110", "90", "100", "50"));
            assert!(ctx.is_ready_for(i));
            assert!(!ctx.is_ready_for(i + 1));
        }
    }

    #[test]
    fn test_bars_context_sma_with_ready() {
        let mut ctx = BarsContext::new("BTCUSDT");

        // Add bars
        for close in &["10", "20", "30", "40", "50"] {
            ctx.on_bar_update(&create_bar_data(close, close, close, close, "100"));
        }

        // SMA(3) should be ready
        let (sma, is_ready) = ctx.sma_with_ready(3);
        assert!(is_ready);
        assert!(sma.is_some());
        assert_eq!(sma.unwrap(), Decimal::from(40));

        // SMA(10) should not be ready
        let (sma, is_ready) = ctx.sma_with_ready(10);
        assert!(!is_ready);
        assert!(sma.is_none());
    }

    #[test]
    fn test_bars_context_highest_lowest_with_ready() {
        let mut ctx = BarsContext::new("BTCUSDT");

        ctx.on_bar_update(&create_bar_data("100", "110", "90", "105", "100"));
        ctx.on_bar_update(&create_bar_data("105", "120", "100", "115", "100"));
        ctx.on_bar_update(&create_bar_data("115", "125", "110", "120", "100"));

        // Highest high should be ready for period 3
        let (highest, is_ready) = ctx.highest_high_with_ready(3);
        assert!(is_ready);
        assert_eq!(highest.unwrap(), Decimal::from(125));

        // Lowest low should be ready for period 3
        let (lowest, is_ready) = ctx.lowest_low_with_ready(3);
        assert!(is_ready);
        assert_eq!(lowest.unwrap(), Decimal::from(90));

        // Not ready for period 5
        let (_, is_ready) = ctx.highest_high_with_ready(5);
        assert!(!is_ready);
    }
}
