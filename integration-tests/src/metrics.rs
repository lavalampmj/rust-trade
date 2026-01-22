//! Metrics collection for the integration test harness
//!
//! This module provides latency statistics, tick counting, and overall
//! test results aggregation.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::config::MetricsConfig;

/// Latency statistics with streaming computation
///
/// Uses Welford's algorithm for online mean and variance calculation
/// to avoid storing all samples when under memory pressure.
#[derive(Debug, Clone)]
pub struct LatencyStats {
    /// Number of samples
    pub count: u64,
    /// Sum of all samples in microseconds
    pub sum_us: u64,
    /// Sum of squares for variance calculation
    pub sum_sq_us: f64,
    /// Minimum latency
    pub min_us: u64,
    /// Maximum latency
    pub max_us: u64,
    /// Running mean (Welford's algorithm)
    pub mean: f64,
    /// Running M2 for variance (Welford's algorithm)
    pub m2: f64,
    /// Samples for percentile calculation (capped)
    samples: Vec<u64>,
    /// Maximum number of samples to keep
    sample_limit: usize,
}

impl Default for LatencyStats {
    fn default() -> Self {
        Self::new(100_000)
    }
}

impl LatencyStats {
    /// Create new latency stats with given sample limit
    pub fn new(sample_limit: usize) -> Self {
        Self {
            count: 0,
            sum_us: 0,
            sum_sq_us: 0.0,
            min_us: u64::MAX,
            max_us: 0,
            mean: 0.0,
            m2: 0.0,
            samples: Vec::with_capacity(sample_limit.min(100_000)),
            sample_limit,
        }
    }

    /// Record a new latency sample
    pub fn record(&mut self, latency_us: u64) {
        self.count += 1;
        self.sum_us += latency_us;

        // Update min/max
        self.min_us = self.min_us.min(latency_us);
        self.max_us = self.max_us.max(latency_us);

        // Welford's online algorithm for mean and variance
        let delta = latency_us as f64 - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = latency_us as f64 - self.mean;
        self.m2 += delta * delta2;

        // Store sample for percentile calculation (with limit)
        if self.samples.len() < self.sample_limit {
            self.samples.push(latency_us);
        } else {
            // Reservoir sampling to maintain representative sample
            let idx = (self.count % self.sample_limit as u64) as usize;
            if idx < self.samples.len() {
                self.samples[idx] = latency_us;
            }
        }
    }

    /// Get average latency in microseconds
    pub fn average_us(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.mean
        }
    }

    /// Get variance in microseconds squared
    pub fn variance(&self) -> f64 {
        if self.count < 2 {
            0.0
        } else {
            self.m2 / (self.count - 1) as f64
        }
    }

    /// Get standard deviation in microseconds
    pub fn std_dev_us(&self) -> f64 {
        self.variance().sqrt()
    }

    /// Get median latency in microseconds
    pub fn median_us(&self) -> u64 {
        self.percentile_us(50.0)
    }

    /// Get percentile latency in microseconds
    ///
    /// Uses sorted sample data for percentile calculation.
    pub fn percentile_us(&self, p: f64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }

        let mut sorted = self.samples.clone();
        sorted.sort_unstable();

        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Get p95 latency
    pub fn p95_us(&self) -> u64 {
        self.percentile_us(95.0)
    }

    /// Get p99 latency
    pub fn p99_us(&self) -> u64 {
        self.percentile_us(99.0)
    }

    /// Get p99.9 latency
    pub fn p999_us(&self) -> u64 {
        self.percentile_us(99.9)
    }

    /// Merge another LatencyStats into this one
    pub fn merge(&mut self, other: &LatencyStats) {
        if other.count == 0 {
            return;
        }

        // Merge counts and sums
        let total_count = self.count + other.count;

        if self.count == 0 {
            // Just copy from other
            *self = other.clone();
            return;
        }

        // Parallel algorithm for combining means and variances
        let delta = other.mean - self.mean;
        self.mean = (self.count as f64 * self.mean + other.count as f64 * other.mean)
            / total_count as f64;
        self.m2 += other.m2 + delta * delta * self.count as f64 * other.count as f64
            / total_count as f64;

        self.count = total_count;
        self.sum_us += other.sum_us;
        self.min_us = self.min_us.min(other.min_us);
        self.max_us = self.max_us.max(other.max_us);

        // Merge samples (keeping within limit)
        let remaining = self.sample_limit.saturating_sub(self.samples.len());
        self.samples.extend(
            other.samples.iter()
                .take(remaining)
                .copied()
        );
    }
}

/// Per-strategy metrics
#[derive(Debug)]
pub struct StrategyMetrics {
    /// Strategy identifier
    pub id: String,
    /// Whether this is a Rust or Python strategy
    pub strategy_type: String,
    /// Number of ticks received
    pub ticks_received: AtomicU64,
    /// Number of ticks successfully processed
    pub ticks_processed: AtomicU64,
    /// Number of errors during processing
    pub errors: AtomicU64,
    /// Latency statistics
    pub latency_stats: Mutex<LatencyStats>,
    /// First tick receive time
    pub first_tick_time: Mutex<Option<Instant>>,
    /// Last tick receive time
    pub last_tick_time: Mutex<Option<Instant>>,
}

impl StrategyMetrics {
    /// Create new strategy metrics
    pub fn new(id: String, strategy_type: String, sample_limit: usize) -> Self {
        Self {
            id,
            strategy_type,
            ticks_received: AtomicU64::new(0),
            ticks_processed: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            latency_stats: Mutex::new(LatencyStats::new(sample_limit)),
            first_tick_time: Mutex::new(None),
            last_tick_time: Mutex::new(None),
        }
    }

    /// Record a tick reception with optional latency
    pub fn record_tick(&self, latency_us: Option<u64>) {
        let now = Instant::now();

        self.ticks_received.fetch_add(1, Ordering::SeqCst);

        // Update timestamps
        {
            let mut first = self.first_tick_time.lock();
            if first.is_none() {
                *first = Some(now);
            }
        }
        {
            let mut last = self.last_tick_time.lock();
            *last = Some(now);
        }

        // Record latency if available
        if let Some(lat) = latency_us {
            self.latency_stats.lock().record(lat);
        }

        self.ticks_processed.fetch_add(1, Ordering::SeqCst);
    }

    /// Record an error
    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }

    /// Get ticks received count
    pub fn received_count(&self) -> u64 {
        self.ticks_received.load(Ordering::SeqCst)
    }

    /// Get ticks processed count
    pub fn processed_count(&self) -> u64 {
        self.ticks_processed.load(Ordering::SeqCst)
    }

    /// Get error count
    pub fn error_count(&self) -> u64 {
        self.errors.load(Ordering::SeqCst)
    }

    /// Get throughput (ticks per second)
    pub fn throughput(&self) -> f64 {
        let first = self.first_tick_time.lock();
        let last = self.last_tick_time.lock();

        match (*first, *last) {
            (Some(f), Some(l)) => {
                let duration = l.duration_since(f);
                if duration.as_secs_f64() > 0.0 {
                    self.received_count() as f64 / duration.as_secs_f64()
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }
}

/// Overall test results
#[derive(Debug)]
pub struct TestResults {
    /// Number of ticks generated
    pub ticks_generated: u64,
    /// Number of ticks sent by emulator
    pub ticks_sent: u64,
    /// Total ticks received across all strategies
    pub ticks_received_total: u64,
    /// Ticks persisted to database (if verified)
    pub ticks_persisted: Option<u64>,
    /// Per-strategy metrics
    pub strategy_metrics: Vec<Arc<StrategyMetrics>>,
    /// Aggregate latency statistics
    pub latency_aggregate: LatencyStats,
    /// Total test duration
    pub test_duration: Duration,
    /// Whether the test passed
    pub passed: bool,
    /// List of failure reasons
    pub failures: Vec<String>,
}

impl TestResults {
    /// Create new test results
    pub fn new() -> Self {
        Self {
            ticks_generated: 0,
            ticks_sent: 0,
            ticks_received_total: 0,
            ticks_persisted: None,
            strategy_metrics: Vec::new(),
            latency_aggregate: LatencyStats::default(),
            test_duration: Duration::ZERO,
            passed: true,
            failures: Vec::new(),
        }
    }

    /// Calculate tick loss rate
    ///
    /// With per-symbol subscriptions, each strategy receives ticks only for its
    /// subscribed symbol. Loss rate = 1 - (total_received / total_sent).
    pub fn tick_loss_rate(&self) -> f64 {
        if self.ticks_sent == 0 {
            return 0.0;
        }

        1.0 - (self.ticks_received_total as f64 / self.ticks_sent as f64)
    }

    /// Validate results against config
    pub fn validate(&mut self, config: &MetricsConfig) {
        self.failures.clear();
        self.passed = true;

        // Check tick loss rate
        let loss_rate = self.tick_loss_rate();
        if loss_rate > config.tick_loss_tolerance {
            self.passed = false;
            self.failures.push(format!(
                "Tick loss rate {:.2}% exceeds tolerance {:.2}%",
                loss_rate * 100.0,
                config.tick_loss_tolerance * 100.0
            ));
        }

        // Check average latency
        let avg_lat = self.latency_aggregate.average_us();
        if avg_lat > config.max_avg_latency_us as f64 {
            self.passed = false;
            self.failures.push(format!(
                "Average latency {:.1}μs exceeds max {}μs",
                avg_lat,
                config.max_avg_latency_us
            ));
        }

        // Check p99 latency
        let p99_lat = self.latency_aggregate.p99_us();
        if p99_lat > config.max_p99_latency_us {
            self.passed = false;
            self.failures.push(format!(
                "P99 latency {}μs exceeds max {}μs",
                p99_lat,
                config.max_p99_latency_us
            ));
        }
    }

    /// Aggregate metrics from all strategies
    pub fn aggregate_metrics(&mut self) {
        self.ticks_received_total = 0;
        self.latency_aggregate = LatencyStats::default();

        for strategy in &self.strategy_metrics {
            self.ticks_received_total += strategy.received_count();

            let stats = strategy.latency_stats.lock();
            self.latency_aggregate.merge(&stats);
        }
    }
}

impl Default for TestResults {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe metrics collector
#[derive(Debug)]
pub struct MetricsCollector {
    /// Configuration
    config: MetricsConfig,
    /// Strategy metrics (keyed by strategy ID)
    strategies: Mutex<HashMap<String, Arc<StrategyMetrics>>>,
    /// Test start time
    start_time: Mutex<Option<Instant>>,
    /// Test end time
    end_time: Mutex<Option<Instant>>,
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new(config: MetricsConfig) -> Self {
        Self {
            config,
            strategies: Mutex::new(HashMap::new()),
            start_time: Mutex::new(None),
            end_time: Mutex::new(None),
        }
    }

    /// Register a strategy and return its metrics handle
    pub fn register_strategy(&self, id: String, strategy_type: String) -> Arc<StrategyMetrics> {
        let metrics = Arc::new(StrategyMetrics::new(
            id.clone(),
            strategy_type,
            self.config.latency_sample_limit,
        ));
        self.strategies.lock().insert(id, metrics.clone());
        metrics
    }

    /// Mark test start
    pub fn start(&self) {
        *self.start_time.lock() = Some(Instant::now());
    }

    /// Mark test end
    pub fn stop(&self) {
        *self.end_time.lock() = Some(Instant::now());
    }

    /// Get test duration
    pub fn duration(&self) -> Duration {
        let start = *self.start_time.lock();
        let end = *self.end_time.lock();

        match (start, end) {
            (Some(s), Some(e)) => e.duration_since(s),
            (Some(s), None) => Instant::now().duration_since(s),
            _ => Duration::ZERO,
        }
    }

    /// Build test results using internally registered metrics
    pub fn build_results(
        &self,
        ticks_generated: u64,
        ticks_sent: u64,
        ticks_persisted: Option<u64>,
    ) -> TestResults {
        let strategy_metrics = self.strategies.lock().values().cloned().collect();
        self.build_results_with_metrics(ticks_generated, ticks_sent, ticks_persisted, strategy_metrics)
    }

    /// Build test results with externally provided strategy metrics
    ///
    /// Use this when strategy runners maintain their own metrics instead of
    /// using metrics registered with the collector.
    pub fn build_results_with_metrics(
        &self,
        ticks_generated: u64,
        ticks_sent: u64,
        ticks_persisted: Option<u64>,
        strategy_metrics: Vec<Arc<StrategyMetrics>>,
    ) -> TestResults {
        let mut results = TestResults {
            ticks_generated,
            ticks_sent,
            ticks_received_total: 0,
            ticks_persisted,
            strategy_metrics,
            latency_aggregate: LatencyStats::new(self.config.latency_sample_limit),
            test_duration: self.duration(),
            passed: true,
            failures: Vec::new(),
        };

        results.aggregate_metrics();
        results.validate(&self.config);

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_stats_basic() {
        let mut stats = LatencyStats::new(1000);

        stats.record(100);
        stats.record(200);
        stats.record(300);

        assert_eq!(stats.count, 3);
        assert_eq!(stats.min_us, 100);
        assert_eq!(stats.max_us, 300);
        assert!((stats.average_us() - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_latency_stats_percentiles() {
        let mut stats = LatencyStats::new(1000);

        // Record values 1-100
        for i in 1..=100 {
            stats.record(i);
        }

        // Median of 1-100 is ~50.5, so allow 50 or 51
        let median = stats.median_us();
        assert!(median >= 50 && median <= 51, "median = {}", median);
        assert!(stats.p95_us() >= 94 && stats.p95_us() <= 96);
        assert!(stats.p99_us() >= 98 && stats.p99_us() <= 100);
    }

    #[test]
    fn test_latency_stats_std_dev() {
        let mut stats = LatencyStats::new(1000);

        // All same values -> std dev should be 0
        for _ in 0..100 {
            stats.record(50);
        }

        assert!(stats.std_dev_us() < 0.01);

        // Mixed values
        let mut stats2 = LatencyStats::new(1000);
        stats2.record(10);
        stats2.record(20);
        stats2.record(30);

        assert!(stats2.std_dev_us() > 0.0);
    }

    #[test]
    fn test_latency_stats_merge() {
        let mut stats1 = LatencyStats::new(1000);
        stats1.record(100);
        stats1.record(200);

        let mut stats2 = LatencyStats::new(1000);
        stats2.record(300);
        stats2.record(400);

        stats1.merge(&stats2);

        assert_eq!(stats1.count, 4);
        assert_eq!(stats1.min_us, 100);
        assert_eq!(stats1.max_us, 400);
        assert!((stats1.average_us() - 250.0).abs() < 0.01);
    }

    #[test]
    fn test_strategy_metrics() {
        let metrics = StrategyMetrics::new(
            "test_strategy".to_string(),
            "rust".to_string(),
            1000,
        );

        metrics.record_tick(Some(100));
        metrics.record_tick(Some(200));
        metrics.record_tick(None);

        assert_eq!(metrics.received_count(), 3);
        assert_eq!(metrics.processed_count(), 3);

        let stats = metrics.latency_stats.lock();
        assert_eq!(stats.count, 2); // Only 2 had latency
    }

    #[test]
    fn test_test_results_validation() {
        let config = MetricsConfig {
            latency_sample_limit: 1000,
            tick_loss_tolerance: 0.01,
            max_avg_latency_us: 1000,
            max_p99_latency_us: 5000,
        };

        let mut results = TestResults::new();
        results.ticks_sent = 1000;
        results.ticks_received_total = 950; // 5% loss (exceeds 1% tolerance)
        results.latency_aggregate.record(100);
        results.latency_aggregate.record(200);

        // Create mock strategy with proper metrics
        let strategy = Arc::new(StrategyMetrics::new(
            "test".to_string(),
            "rust".to_string(),
            1000,
        ));
        for _ in 0..950 {
            strategy.record_tick(Some(150));
        }
        results.strategy_metrics.push(strategy);

        results.aggregate_metrics();
        results.validate(&config);

        assert!(!results.passed);
        assert!(!results.failures.is_empty());
    }

    #[test]
    fn test_metrics_collector() {
        let config = MetricsConfig::default();
        let collector = MetricsCollector::new(config);

        let strategy1 = collector.register_strategy("s1".to_string(), "rust".to_string());
        let strategy2 = collector.register_strategy("s2".to_string(), "python".to_string());

        collector.start();

        strategy1.record_tick(Some(100));
        strategy1.record_tick(Some(200));
        strategy2.record_tick(Some(150));

        collector.stop();

        let results = collector.build_results(1000, 1000, None);

        assert_eq!(results.strategy_metrics.len(), 2);
        assert!(results.test_duration > Duration::ZERO);
    }
}
