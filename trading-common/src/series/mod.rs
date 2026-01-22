// Series<T> abstraction for reverse-indexed time series data
// NinjaTrader-style access: series[0] = most recent, series[1] = previous

pub mod bars_context;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::VecDeque;
use std::ops::Index;

/// Trait for types that can be stored in a Series
///
/// Implemented for common trading data types: Decimal, f64, i64, bool, DateTime<Utc>
pub trait SeriesValue: Clone + Default + Send + Sync + 'static {}

// Implement SeriesValue for common types
impl SeriesValue for Decimal {}
impl SeriesValue for f64 {}
impl SeriesValue for i64 {}
impl SeriesValue for i32 {}
impl SeriesValue for u64 {}
impl SeriesValue for u32 {}
impl SeriesValue for bool {}
impl SeriesValue for DateTime<Utc> {}
impl<T: SeriesValue> SeriesValue for Option<T> {}

/// Controls memory management for series data
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaximumBarsLookBack {
    /// Fixed lookback - FIFO eviction when exceeded (default: 256)
    Fixed(usize),
    /// Infinite lookback - no eviction, grows unbounded
    Infinite,
}

impl Default for MaximumBarsLookBack {
    fn default() -> Self {
        MaximumBarsLookBack::Fixed(256)
    }
}

/// Reverse-indexed time series data structure
///
/// Provides NinjaTrader-style access where:
/// - `series[0]` returns the most recent value
/// - `series[1]` returns the previous value
/// - `series[n]` returns the value n bars ago
///
/// # Warmup System
///
/// Each series tracks a `warmup_period` that indicates how many samples are
/// needed before the series is considered "ready" for use. This allows
/// indicators to propagate their ready state up the call chain.
///
/// ```text
/// let mut series = Series::<Decimal>::with_warmup("sma", MaximumBarsLookBack::default(), 20);
/// series.push(value);
/// if series.is_ready() {
///     // Safe to use - guaranteed to have enough samples
/// }
/// ```
///
/// # Example
/// ```text
/// let mut series = Series::<Decimal>::new("close");
/// series.push(Decimal::from(100));
/// series.push(Decimal::from(101));
/// series.push(Decimal::from(102));
///
/// assert_eq!(series[0], Decimal::from(102)); // Most recent
/// assert_eq!(series[1], Decimal::from(101)); // Previous
/// assert_eq!(series[2], Decimal::from(100)); // Two bars ago
/// ```
#[derive(Debug, Clone)]
pub struct Series<T: SeriesValue> {
    /// Internal storage using VecDeque for O(1) front removal
    /// Data is stored in chronological order: oldest at front, newest at back
    data: VecDeque<T>,

    /// Maximum lookback configuration
    max_lookback: MaximumBarsLookBack,

    /// Current bar index (incremented with each push)
    current_bar_index: u64,

    /// Series name for debugging and identification
    name: String,

    /// Number of samples needed before series is ready (default: 1)
    warmup_period: usize,
}

impl<T: SeriesValue> Series<T> {
    /// Create a new series with default lookback (256 bars) and warmup period of 1
    pub fn new(name: &str) -> Self {
        Self::with_lookback(name, MaximumBarsLookBack::default())
    }

    /// Create a new series with custom lookback and default warmup period of 1
    pub fn with_lookback(name: &str, max_lookback: MaximumBarsLookBack) -> Self {
        Self::with_warmup(name, max_lookback, 1)
    }

    /// Create a new series with custom lookback and warmup period
    ///
    /// # Arguments
    /// - `name`: Series name for identification
    /// - `max_lookback`: Maximum lookback configuration (FIFO eviction)
    /// - `warmup_period`: Number of samples needed before is_ready() returns true
    pub fn with_warmup(name: &str, max_lookback: MaximumBarsLookBack, warmup_period: usize) -> Self {
        let capacity = match max_lookback {
            MaximumBarsLookBack::Fixed(n) => n,
            MaximumBarsLookBack::Infinite => 256, // Initial capacity
        };

        Self {
            data: VecDeque::with_capacity(capacity),
            max_lookback,
            current_bar_index: 0,
            name: name.to_string(),
            warmup_period,
        }
    }

    /// Check if series has enough samples to be ready
    ///
    /// Returns true when `count() >= warmup_period`. Each indicator/series
    /// knows its own readiness state.
    ///
    /// # Example
    /// ```text
    /// let mut series = Series::<Decimal>::with_warmup("sma", MaximumBarsLookBack::default(), 20);
    /// for _ in 0..19 {
    ///     series.push(value);
    ///     assert!(!series.is_ready()); // Not ready yet
    /// }
    /// series.push(value);
    /// assert!(series.is_ready()); // Now ready with 20 samples
    /// ```
    pub fn is_ready(&self) -> bool {
        self.count() >= self.warmup_period
    }

    /// Get the warmup period (samples needed before is_ready returns true)
    pub fn warmup_period(&self) -> usize {
        self.warmup_period
    }

    /// Set the warmup period (for dynamic indicators)
    ///
    /// Use this when the warmup period needs to be changed after construction,
    /// for example when indicator parameters are set dynamically.
    pub fn set_warmup_period(&mut self, period: usize) {
        self.warmup_period = period;
    }

    /// Push a new value to the series
    ///
    /// This should be called once per bar to maintain alignment with other series.
    /// Old values are automatically evicted based on max_lookback configuration.
    pub fn push(&mut self, value: T) {
        // Add new value at the back (newest)
        self.data.push_back(value);
        self.current_bar_index += 1;

        // Evict old values if needed
        if let MaximumBarsLookBack::Fixed(max) = self.max_lookback {
            while self.data.len() > max {
                self.data.pop_front();
            }
        }
    }

    /// Get value at bars_ago index (0 = most recent)
    ///
    /// Returns None if bars_ago exceeds available data
    pub fn get(&self, bars_ago: usize) -> Option<&T> {
        if self.data.is_empty() {
            return None;
        }

        // Convert reverse index to forward index
        // data[len-1] is newest (bars_ago=0)
        // data[len-2] is previous (bars_ago=1)
        let forward_index = self.data.len().checked_sub(1 + bars_ago)?;
        self.data.get(forward_index)
    }

    /// Get mutable value at bars_ago index
    pub fn get_mut(&mut self, bars_ago: usize) -> Option<&mut T> {
        if self.data.is_empty() {
            return None;
        }

        let forward_index = self.data.len().checked_sub(1 + bars_ago)?;
        self.data.get_mut(forward_index)
    }

    /// Check if data is available at bars_ago index
    pub fn has_data(&self, bars_ago: usize) -> bool {
        self.get(bars_ago).is_some()
    }

    /// Get the number of available data points
    pub fn count(&self) -> usize {
        self.data.len()
    }

    /// Check if series is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get the current bar index
    pub fn current_bar(&self) -> u64 {
        self.current_bar_index
    }

    /// Get series name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reset the series, clearing all data but preserving warmup_period
    pub fn reset(&mut self) {
        self.data.clear();
        self.current_bar_index = 0;
        // Note: warmup_period is preserved through reset
    }

    /// Iterate over values from oldest to newest
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.data.iter()
    }

    /// Iterate over values from newest to oldest (reverse index order)
    pub fn iter_reverse(&self) -> impl Iterator<Item = &T> {
        self.data.iter().rev()
    }

    /// Get most recent value (bars_ago = 0)
    pub fn current(&self) -> Option<&T> {
        self.get(0)
    }

    /// Get previous value (bars_ago = 1)
    pub fn previous(&self) -> Option<&T> {
        self.get(1)
    }

    /// Take the N most recent values (in reverse order: newest first)
    pub fn take(&self, n: usize) -> Vec<&T> {
        self.data.iter().rev().take(n).collect()
    }

    /// Get max lookback configuration
    pub fn max_lookback(&self) -> MaximumBarsLookBack {
        self.max_lookback
    }
}

/// Implement Index trait for convenient series[n] syntax
///
/// Panics if index is out of bounds (consistent with NinjaTrader behavior)
impl<T: SeriesValue> Index<usize> for Series<T> {
    type Output = T;

    fn index(&self, bars_ago: usize) -> &Self::Output {
        self.get(bars_ago).unwrap_or_else(|| {
            panic!(
                "Series '{}' index out of bounds: bars_ago={} but only {} bars available",
                self.name,
                bars_ago,
                self.count()
            )
        })
    }
}

/// Extension trait for Decimal series with common calculations
pub trait DecimalSeriesExt {
    /// Calculate Simple Moving Average over last N values
    fn sma(&self, period: usize) -> Option<Decimal>;

    /// Get highest value over last N bars
    fn highest(&self, period: usize) -> Option<Decimal>;

    /// Get lowest value over last N bars
    fn lowest(&self, period: usize) -> Option<Decimal>;

    /// Calculate sum of last N values
    fn sum(&self, period: usize) -> Option<Decimal>;
}

impl DecimalSeriesExt for Series<Decimal> {
    fn sma(&self, period: usize) -> Option<Decimal> {
        if self.count() < period || period == 0 {
            return None;
        }

        let sum: Decimal = (0..period).filter_map(|i| self.get(i)).cloned().sum();

        Some(sum / Decimal::from(period))
    }

    fn highest(&self, period: usize) -> Option<Decimal> {
        if self.count() < period || period == 0 {
            return None;
        }

        (0..period).filter_map(|i| self.get(i)).cloned().max()
    }

    fn lowest(&self, period: usize) -> Option<Decimal> {
        if self.count() < period || period == 0 {
            return None;
        }

        (0..period).filter_map(|i| self.get(i)).cloned().min()
    }

    fn sum(&self, period: usize) -> Option<Decimal> {
        if self.count() < period || period == 0 {
            return None;
        }

        Some((0..period).filter_map(|i| self.get(i)).cloned().sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_series_new() {
        let series: Series<Decimal> = Series::new("test");
        assert_eq!(series.name(), "test");
        assert_eq!(series.count(), 0);
        assert!(series.is_empty());
        assert_eq!(series.max_lookback(), MaximumBarsLookBack::Fixed(256));
    }

    #[test]
    fn test_series_push_and_get() {
        let mut series: Series<Decimal> = Series::new("close");

        series.push(Decimal::from(100));
        series.push(Decimal::from(101));
        series.push(Decimal::from(102));

        assert_eq!(series.count(), 3);
        assert_eq!(*series.get(0).unwrap(), Decimal::from(102)); // Most recent
        assert_eq!(*series.get(1).unwrap(), Decimal::from(101)); // Previous
        assert_eq!(*series.get(2).unwrap(), Decimal::from(100)); // Two bars ago
        assert!(series.get(3).is_none()); // Out of bounds
    }

    #[test]
    fn test_series_index_operator() {
        let mut series: Series<Decimal> = Series::new("close");

        series.push(Decimal::from(100));
        series.push(Decimal::from(101));
        series.push(Decimal::from(102));

        assert_eq!(series[0], Decimal::from(102));
        assert_eq!(series[1], Decimal::from(101));
        assert_eq!(series[2], Decimal::from(100));
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_series_index_out_of_bounds() {
        let series: Series<Decimal> = Series::new("close");
        let _ = series[0]; // Should panic
    }

    #[test]
    fn test_series_fifo_eviction() {
        let mut series: Series<Decimal> = Series::with_lookback("test", MaximumBarsLookBack::Fixed(3));

        series.push(Decimal::from(1));
        series.push(Decimal::from(2));
        series.push(Decimal::from(3));
        assert_eq!(series.count(), 3);

        series.push(Decimal::from(4)); // Should evict 1
        assert_eq!(series.count(), 3);
        assert_eq!(series[0], Decimal::from(4));
        assert_eq!(series[2], Decimal::from(2)); // Oldest remaining
    }

    #[test]
    fn test_series_infinite_lookback() {
        let mut series: Series<Decimal> =
            Series::with_lookback("test", MaximumBarsLookBack::Infinite);

        for i in 0..1000 {
            series.push(Decimal::from(i));
        }

        assert_eq!(series.count(), 1000);
        assert_eq!(series[0], Decimal::from(999));
        assert_eq!(series[999], Decimal::from(0));
    }

    #[test]
    fn test_series_current_and_previous() {
        let mut series: Series<Decimal> = Series::new("test");

        series.push(Decimal::from(100));
        assert_eq!(*series.current().unwrap(), Decimal::from(100));
        assert!(series.previous().is_none());

        series.push(Decimal::from(101));
        assert_eq!(*series.current().unwrap(), Decimal::from(101));
        assert_eq!(*series.previous().unwrap(), Decimal::from(100));
    }

    #[test]
    fn test_series_reset() {
        let mut series: Series<Decimal> = Series::new("test");

        series.push(Decimal::from(100));
        series.push(Decimal::from(101));
        assert_eq!(series.count(), 2);

        series.reset();
        assert_eq!(series.count(), 0);
        assert!(series.is_empty());
    }

    #[test]
    fn test_series_take() {
        let mut series: Series<Decimal> = Series::new("test");

        series.push(Decimal::from(100));
        series.push(Decimal::from(101));
        series.push(Decimal::from(102));

        let recent: Vec<&Decimal> = series.take(2);
        assert_eq!(recent.len(), 2);
        assert_eq!(*recent[0], Decimal::from(102)); // Newest first
        assert_eq!(*recent[1], Decimal::from(101));
    }

    #[test]
    fn test_sma_calculation() {
        let mut series: Series<Decimal> = Series::new("close");

        series.push(Decimal::from_str("10").unwrap());
        series.push(Decimal::from_str("20").unwrap());
        series.push(Decimal::from_str("30").unwrap());
        series.push(Decimal::from_str("40").unwrap());
        series.push(Decimal::from_str("50").unwrap());

        // SMA(3) = (50 + 40 + 30) / 3 = 40
        let sma = series.sma(3).unwrap();
        assert_eq!(sma, Decimal::from(40));

        // SMA(5) = (10 + 20 + 30 + 40 + 50) / 5 = 30
        let sma5 = series.sma(5).unwrap();
        assert_eq!(sma5, Decimal::from(30));

        // Not enough data for SMA(6)
        assert!(series.sma(6).is_none());
    }

    #[test]
    fn test_highest_lowest() {
        let mut series: Series<Decimal> = Series::new("high");

        series.push(Decimal::from(10));
        series.push(Decimal::from(50));
        series.push(Decimal::from(30));
        series.push(Decimal::from(20));
        series.push(Decimal::from(40));

        // Highest of last 3: 40, 20, 30 -> 40
        assert_eq!(series.highest(3).unwrap(), Decimal::from(40));

        // Lowest of last 3: 40, 20, 30 -> 20
        assert_eq!(series.lowest(3).unwrap(), Decimal::from(20));

        // Highest of all 5
        assert_eq!(series.highest(5).unwrap(), Decimal::from(50));

        // Lowest of all 5
        assert_eq!(series.lowest(5).unwrap(), Decimal::from(10));
    }

    #[test]
    fn test_series_has_data() {
        let mut series: Series<Decimal> = Series::new("test");

        assert!(!series.has_data(0));

        series.push(Decimal::from(100));
        assert!(series.has_data(0));
        assert!(!series.has_data(1));

        series.push(Decimal::from(101));
        assert!(series.has_data(0));
        assert!(series.has_data(1));
        assert!(!series.has_data(2));
    }

    #[test]
    fn test_series_with_optional_values() {
        let mut series: Series<Option<Decimal>> = Series::new("optional");

        series.push(Some(Decimal::from(100)));
        series.push(None);
        series.push(Some(Decimal::from(102)));

        assert_eq!(series[0], Some(Decimal::from(102)));
        assert_eq!(series[1], None);
        assert_eq!(series[2], Some(Decimal::from(100)));
    }

    #[test]
    fn test_series_iter() {
        let mut series: Series<Decimal> = Series::new("test");

        series.push(Decimal::from(1));
        series.push(Decimal::from(2));
        series.push(Decimal::from(3));

        // Forward iteration: oldest to newest
        let forward: Vec<Decimal> = series.iter().cloned().collect();
        assert_eq!(forward, vec![Decimal::from(1), Decimal::from(2), Decimal::from(3)]);

        // Reverse iteration: newest to oldest
        let reverse: Vec<Decimal> = series.iter_reverse().cloned().collect();
        assert_eq!(reverse, vec![Decimal::from(3), Decimal::from(2), Decimal::from(1)]);
    }

    #[test]
    fn test_series_is_ready_default() {
        let mut series: Series<Decimal> = Series::new("test");

        // Default warmup_period is 1
        assert_eq!(series.warmup_period(), 1);
        assert!(!series.is_ready()); // Empty series not ready

        series.push(Decimal::from(100));
        assert!(series.is_ready()); // Ready with 1 sample
    }

    #[test]
    fn test_series_is_ready_with_warmup() {
        let mut series: Series<Decimal> = Series::with_warmup(
            "sma",
            MaximumBarsLookBack::default(),
            5, // Need 5 samples
        );

        assert_eq!(series.warmup_period(), 5);

        // Push 4 values - not ready yet
        for i in 1..=4 {
            series.push(Decimal::from(i));
            assert!(!series.is_ready(), "Should not be ready with {} samples", i);
        }

        // Push 5th value - now ready
        series.push(Decimal::from(5));
        assert!(series.is_ready());
        assert_eq!(series.count(), 5);
    }

    #[test]
    fn test_series_set_warmup_period() {
        let mut series: Series<Decimal> = Series::new("test");

        // Initially warmup is 1
        assert_eq!(series.warmup_period(), 1);

        // Change warmup period
        series.set_warmup_period(10);
        assert_eq!(series.warmup_period(), 10);

        // Push some values
        for i in 1..=9 {
            series.push(Decimal::from(i));
            assert!(!series.is_ready());
        }

        series.push(Decimal::from(10));
        assert!(series.is_ready());
    }

    #[test]
    fn test_series_reset_preserves_warmup() {
        let mut series: Series<Decimal> = Series::with_warmup(
            "test",
            MaximumBarsLookBack::default(),
            5,
        );

        // Push some values and verify ready
        for i in 1..=5 {
            series.push(Decimal::from(i));
        }
        assert!(series.is_ready());

        // Reset should clear data but preserve warmup_period
        series.reset();
        assert_eq!(series.count(), 0);
        assert!(!series.is_ready());
        assert_eq!(series.warmup_period(), 5); // Preserved

        // Refill and verify
        for i in 1..=5 {
            series.push(Decimal::from(i));
        }
        assert!(series.is_ready());
    }

    #[test]
    fn test_series_is_ready_zero_warmup() {
        // Zero warmup means always ready (edge case)
        let series: Series<Decimal> = Series::with_warmup(
            "test",
            MaximumBarsLookBack::default(),
            0,
        );

        assert!(series.is_ready()); // Ready even when empty
    }
}
