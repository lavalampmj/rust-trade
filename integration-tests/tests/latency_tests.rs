//! Latency Measurement Validation Tests
//!
//! These tests validate the latency extraction and statistics collection
//! functionality.

use integration_tests::{extract_latency_us, LatencyStats, MetricsCollector, MetricsConfig};
use std::sync::Arc;
use std::time::Duration;

/// Test basic latency extraction
#[test]
fn test_latency_extraction_basic() {
    // Create a ts_in_delta value representing "1000 microseconds ago"
    let now_us = chrono::Utc::now().timestamp_micros();
    let sent_us = now_us - 1000; // 1ms ago
    let ts_in_delta = (sent_us % 0x7FFF_FFFF) as i32;

    let latency = extract_latency_us(ts_in_delta);

    // Should return a reasonable latency
    assert!(latency.is_some());
    let lat = latency.unwrap();

    // Should be around 1000us (allowing for timing variance)
    assert!(
        lat < 100_000, // Less than 100ms
        "Latency should be reasonable: {}us",
        lat
    );
}

/// Test latency extraction with zero/negative input
#[test]
fn test_latency_extraction_invalid() {
    assert!(extract_latency_us(0).is_none());
    assert!(extract_latency_us(-1).is_none());
    assert!(extract_latency_us(-1000).is_none());
}

/// Test latency extraction wraparound
#[test]
fn test_latency_extraction_wraparound() {
    // Simulate a wraparound scenario
    // This is tricky to test deterministically, but we can verify the logic
    let max_val = 0x7FFF_FFFF;
    let sent_us = max_val - 1000; // Near the max
    let ts_in_delta = sent_us as i32;

    // Current time should have wrapped around
    // This test mainly verifies the function doesn't panic
    let _ = extract_latency_us(ts_in_delta);
}

/// Test LatencyStats basic operations
#[test]
fn test_latency_stats_basic() {
    let mut stats = LatencyStats::new(1000);

    // Record some samples
    stats.record(100);
    stats.record(200);
    stats.record(300);

    assert_eq!(stats.count, 3);
    assert_eq!(stats.min_us, 100);
    assert_eq!(stats.max_us, 300);
    assert!((stats.average_us() - 200.0).abs() < 0.01);
}

/// Test LatencyStats median calculation
#[test]
fn test_latency_stats_median() {
    let mut stats = LatencyStats::new(1000);

    // Odd number of samples
    stats.record(100);
    stats.record(200);
    stats.record(300);
    assert_eq!(stats.median_us(), 200);

    // Even number of samples
    stats.record(400);
    // With 4 samples [100, 200, 300, 400], median should be ~250
    let median = stats.median_us();
    assert!(median >= 200 && median <= 300);
}

/// Test LatencyStats percentiles
#[test]
fn test_latency_stats_percentiles() {
    let mut stats = LatencyStats::new(1000);

    // Record 100 samples: 1, 2, 3, ..., 100
    for i in 1..=100 {
        stats.record(i);
    }

    // p50 should be around 50
    let p50 = stats.percentile_us(50.0);
    assert!(p50 >= 49 && p50 <= 51, "p50 = {}", p50);

    // p95 should be around 95
    let p95 = stats.p95_us();
    assert!(p95 >= 94 && p95 <= 96, "p95 = {}", p95);

    // p99 should be around 99
    let p99 = stats.p99_us();
    assert!(p99 >= 98 && p99 <= 100, "p99 = {}", p99);
}

/// Test LatencyStats standard deviation
#[test]
fn test_latency_stats_std_dev() {
    let mut stats = LatencyStats::new(1000);

    // All same values -> std dev should be ~0
    for _ in 0..100 {
        stats.record(50);
    }
    assert!(
        stats.std_dev_us() < 0.01,
        "Std dev of constant values should be ~0: {}",
        stats.std_dev_us()
    );

    // Mixed values should have non-zero std dev
    let mut stats2 = LatencyStats::new(1000);
    stats2.record(10);
    stats2.record(20);
    stats2.record(30);
    assert!(
        stats2.std_dev_us() > 0.0,
        "Std dev should be non-zero for varying values"
    );
}

/// Test LatencyStats merge operation
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

/// Test LatencyStats with empty merge
#[test]
fn test_latency_stats_merge_empty() {
    let mut stats1 = LatencyStats::new(1000);
    stats1.record(100);

    let stats2 = LatencyStats::new(1000); // Empty

    stats1.merge(&stats2);

    // Should be unchanged
    assert_eq!(stats1.count, 1);
    assert_eq!(stats1.average_us(), 100.0);
}

/// Test LatencyStats sample limit
#[test]
fn test_latency_stats_sample_limit() {
    let limit = 100;
    let mut stats = LatencyStats::new(limit);

    // Record more than the limit
    for i in 0..200 {
        stats.record(i);
    }

    assert_eq!(stats.count, 200);
    // Internal samples should be capped
    // (We can't directly access samples, but percentiles should still work)
    let _ = stats.percentile_us(50.0);
}

/// Test MetricsCollector basic usage
#[test]
fn test_metrics_collector() {
    let config = MetricsConfig {
        latency_sample_limit: 1000,
        tick_loss_tolerance: 0.01,
        max_avg_latency_us: 5000,
        max_p99_latency_us: 50000,
    };

    let collector = MetricsCollector::new(config);

    let strategy1 = collector.register_strategy("s1".to_string(), "rust".to_string());
    let strategy2 = collector.register_strategy("s2".to_string(), "python".to_string());

    collector.start();

    // Record ticks
    strategy1.record_tick(Some(100));
    strategy1.record_tick(Some(200));
    strategy2.record_tick(Some(150));

    std::thread::sleep(Duration::from_millis(10));
    collector.stop();

    let duration = collector.duration();
    assert!(duration.as_millis() >= 10);

    let results = collector.build_results(100, 100, None);
    assert_eq!(results.strategy_metrics.len(), 2);
}

/// Test results validation - passing case
#[test]
fn test_results_validation_pass() {
    let config = MetricsConfig {
        latency_sample_limit: 1000,
        tick_loss_tolerance: 0.1, // 10%
        max_avg_latency_us: 1000,
        max_p99_latency_us: 5000,
    };

    let collector = MetricsCollector::new(config);
    let strategy = collector.register_strategy("test".to_string(), "rust".to_string());

    // Record ticks with good latency
    for _ in 0..100 {
        strategy.record_tick(Some(100)); // 100us latency
    }

    let results = collector.build_results(100, 100, None);

    assert!(results.passed, "Test should pass: {:?}", results.failures);
    assert!(results.failures.is_empty());
}

/// Test results validation - failing due to high latency
#[test]
fn test_results_validation_fail_latency() {
    let config = MetricsConfig {
        latency_sample_limit: 1000,
        tick_loss_tolerance: 0.1,
        max_avg_latency_us: 100, // Very low threshold
        max_p99_latency_us: 500,
    };

    let collector = MetricsCollector::new(config);
    let strategy = collector.register_strategy("test".to_string(), "rust".to_string());

    // Record ticks with high latency
    for _ in 0..100 {
        strategy.record_tick(Some(1000)); // 1000us latency (exceeds 100us threshold)
    }

    let results = collector.build_results(100, 100, None);

    assert!(!results.passed, "Test should fail due to high latency");
    assert!(!results.failures.is_empty());
    assert!(
        results.failures.iter().any(|f| f.contains("latency")),
        "Should mention latency in failure"
    );
}

/// Test results validation - failing due to tick loss
#[test]
fn test_results_validation_fail_loss() {
    let config = MetricsConfig {
        latency_sample_limit: 1000,
        tick_loss_tolerance: 0.01, // 1%
        max_avg_latency_us: 10000,
        max_p99_latency_us: 50000,
    };

    let collector = MetricsCollector::new(config);
    let strategy = collector.register_strategy("test".to_string(), "rust".to_string());

    // Record only 50 ticks out of 100 expected
    for _ in 0..50 {
        strategy.record_tick(Some(100));
    }

    let results = collector.build_results(100, 100, None);

    assert!(
        !results.passed,
        "Test should fail due to tick loss: {:?}",
        results.failures
    );
    assert!(
        results.failures.iter().any(|f| f.contains("loss")),
        "Should mention loss in failure"
    );
}

/// Test tick loss rate calculation
#[test]
fn test_tick_loss_rate() {
    let config = MetricsConfig::default();
    let collector = MetricsCollector::new(config);
    let strategy = collector.register_strategy("test".to_string(), "rust".to_string());

    // Record 90 ticks
    for _ in 0..90 {
        strategy.record_tick(None);
    }

    // 100 sent, 90 received = 10% loss
    let results = collector.build_results(100, 100, None);

    let loss_rate = results.tick_loss_rate();
    assert!(
        (loss_rate - 0.1).abs() < 0.01,
        "Loss rate should be ~10%: {:.2}%",
        loss_rate * 100.0
    );
}

/// Test Welford's algorithm accuracy
#[test]
fn test_welford_accuracy() {
    let mut stats = LatencyStats::new(100_000);

    // Record many samples with known statistics
    let samples: Vec<u64> = (1..=1000).collect();
    for &s in &samples {
        stats.record(s);
    }

    // Mean should be 500.5
    let expected_mean = 500.5;
    assert!(
        (stats.average_us() - expected_mean).abs() < 0.1,
        "Mean should be ~500.5: {}",
        stats.average_us()
    );

    // Variance of 1..n is n(n-1)/12
    // For n=1000: 1000*999/12 = 83250
    // Std dev = sqrt(83250) ≈ 288.5
    let expected_std = 288.5;
    assert!(
        (stats.std_dev_us() - expected_std).abs() < 1.0,
        "Std dev should be ~288.5: {}",
        stats.std_dev_us()
    );
}

/// Test strategy metrics throughput calculation
#[tokio::test]
async fn test_strategy_throughput() {
    use integration_tests::StrategyMetrics;

    let metrics = Arc::new(StrategyMetrics::new(
        "test".to_string(),
        "rust".to_string(),
        1000,
    ));

    // Record ticks over time
    for _ in 0..100 {
        metrics.record_tick(None);
        tokio::time::sleep(Duration::from_micros(100)).await;
    }

    let throughput = metrics.throughput();

    // With 100 ticks over ~10ms + overhead, expect ~500-10,000 tps
    // The actual value depends on system scheduler and overhead
    assert!(
        throughput > 100.0 && throughput < 100_000.0,
        "Throughput should be reasonable: {} tps",
        throughput
    );
}

/// Test concurrent latency recording
#[tokio::test]
async fn test_concurrent_latency_recording() {
    use integration_tests::StrategyMetrics;

    let metrics = Arc::new(StrategyMetrics::new(
        "test".to_string(),
        "rust".to_string(),
        10000,
    ));

    let handles: Vec<_> = (0..10)
        .map(|_| {
            let m = metrics.clone();
            tokio::spawn(async move {
                for i in 0..100 {
                    m.record_tick(Some(100 + i));
                }
            })
        })
        .collect();

    for h in handles {
        h.await.unwrap();
    }

    // Should have recorded all ticks
    assert_eq!(metrics.received_count(), 1000);
    assert_eq!(metrics.processed_count(), 1000);

    // Latency stats should be valid
    let stats = metrics.latency_stats.lock();
    assert_eq!(stats.count, 1000);
    assert!(stats.min_us >= 100);
    assert!(stats.max_us < 200);
}
