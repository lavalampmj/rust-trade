//! Report generation for the integration test harness
//!
//! This module generates formatted reports of test results including
//! tick counts, latency statistics, and per-strategy breakdowns.

use std::fmt::Write;

use crate::metrics::TestResults;

/// Report format options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    /// Pretty box-drawing format for terminal display
    Pretty,
    /// Simple text format
    Simple,
    /// Compact single-line format
    Compact,
}

/// Generate a formatted report from test results
pub fn generate_report(results: &TestResults, format: ReportFormat) -> String {
    match format {
        ReportFormat::Pretty => generate_pretty_report(results),
        ReportFormat::Simple => generate_simple_report(results),
        ReportFormat::Compact => generate_compact_report(results),
    }
}

/// Generate a pretty box-drawing report
fn generate_pretty_report(results: &TestResults) -> String {
    let mut report = String::new();

    let status = if results.passed { "PASSED" } else { "FAILED" };
    // Note: Could add ANSI color codes here for terminal display

    // Header
    writeln!(report, "╔════════════════════════════════════════════════════════════════╗").unwrap();
    writeln!(report, "║            INTEGRATION TEST RESULTS                            ║").unwrap();
    writeln!(report, "╠════════════════════════════════════════════════════════════════╣").unwrap();
    writeln!(report, "║ Status: {:<55}║", status).unwrap();
    writeln!(report, "║ Duration: {:<53}║", format!("{:.1}s", results.test_duration.as_secs_f64())).unwrap();

    // Tick Counts Section
    writeln!(report, "╠════════════════════════════════════════════════════════════════╣").unwrap();
    writeln!(report, "║ TICK COUNTS:                                                   ║").unwrap();
    writeln!(report, "║   Generated:       {:>12}                                ║", format_number(results.ticks_generated)).unwrap();
    writeln!(report, "║   Sent:            {:>12}                                ║", format_number(results.ticks_sent)).unwrap();
    writeln!(report, "║   Received (all):  {:>12}                                ║", format_number(results.ticks_received_total)).unwrap();

    if let Some(persisted) = results.ticks_persisted {
        writeln!(report, "║   Persisted (DB):  {:>12}                                ║", format_number(persisted)).unwrap();
    }

    let loss_rate = results.tick_loss_rate();
    writeln!(report, "║   Loss Rate:       {:>12}                                ║", format!("{:.2}%", loss_rate * 100.0)).unwrap();

    // Latency Section
    writeln!(report, "╠════════════════════════════════════════════════════════════════╣").unwrap();
    writeln!(report, "║ LATENCY (μs):                                                  ║").unwrap();

    let stats = &results.latency_aggregate;
    if stats.count > 0 {
        writeln!(report, "║   Average:             {:>12.1}                           ║", stats.average_us()).unwrap();
        writeln!(report, "║   Median:              {:>12}                           ║", stats.median_us()).unwrap();
        writeln!(report, "║   Std Dev:             {:>12.1}                           ║", stats.std_dev_us()).unwrap();
        writeln!(report, "║   Min:                 {:>12}                           ║", stats.min_us).unwrap();
        writeln!(report, "║   Max:                 {:>12}                           ║", stats.max_us).unwrap();
        writeln!(report, "║   p95:                 {:>12}                           ║", stats.p95_us()).unwrap();
        writeln!(report, "║   p99:                 {:>12}                           ║", stats.p99_us()).unwrap();
        writeln!(report, "║   p99.9:               {:>12}                           ║", stats.p999_us()).unwrap();
    } else {
        writeln!(report, "║   No latency data collected                                  ║").unwrap();
    }

    // Per-Strategy Breakdown
    if !results.strategy_metrics.is_empty() {
        writeln!(report, "╠════════════════════════════════════════════════════════════════╣").unwrap();
        writeln!(report, "║ PER-STRATEGY BREAKDOWN:                                        ║").unwrap();

        for strategy in &results.strategy_metrics {
            let stats = strategy.latency_stats.lock();
            let line = format!(
                "   {}: recv={} avg={:.0}μs p99={}μs",
                strategy.id,
                format_number(strategy.received_count()),
                stats.average_us(),
                stats.p99_us()
            );
            writeln!(report, "║{:<65}║", line).unwrap();
        }
    }

    // Failures Section
    if !results.failures.is_empty() {
        writeln!(report, "╠════════════════════════════════════════════════════════════════╣").unwrap();
        writeln!(report, "║ FAILURES:                                                      ║").unwrap();
        for failure in &results.failures {
            // Truncate long failure messages
            let msg = if failure.len() > 60 {
                format!("{}...", &failure[..57])
            } else {
                failure.clone()
            };
            writeln!(report, "║   - {:<60}║", msg).unwrap();
        }
    }

    // Footer
    writeln!(report, "╚════════════════════════════════════════════════════════════════╝").unwrap();

    report
}

/// Generate a simple text report
fn generate_simple_report(results: &TestResults) -> String {
    let mut report = String::new();

    let status = if results.passed { "PASSED" } else { "FAILED" };

    writeln!(report, "=== INTEGRATION TEST RESULTS ===").unwrap();
    writeln!(report).unwrap();
    writeln!(report, "Status: {}", status).unwrap();
    writeln!(report, "Duration: {:.1}s", results.test_duration.as_secs_f64()).unwrap();
    writeln!(report).unwrap();

    writeln!(report, "TICK COUNTS:").unwrap();
    writeln!(report, "  Generated: {}", format_number(results.ticks_generated)).unwrap();
    writeln!(report, "  Sent: {}", format_number(results.ticks_sent)).unwrap();
    writeln!(report, "  Received: {}", format_number(results.ticks_received_total)).unwrap();
    if let Some(persisted) = results.ticks_persisted {
        writeln!(report, "  Persisted: {}", format_number(persisted)).unwrap();
    }
    writeln!(report, "  Loss Rate: {:.2}%", results.tick_loss_rate() * 100.0).unwrap();
    writeln!(report).unwrap();

    writeln!(report, "LATENCY (μs):").unwrap();
    let stats = &results.latency_aggregate;
    if stats.count > 0 {
        writeln!(report, "  Average: {:.1}", stats.average_us()).unwrap();
        writeln!(report, "  Median: {}", stats.median_us()).unwrap();
        writeln!(report, "  Std Dev: {:.1}", stats.std_dev_us()).unwrap();
        writeln!(report, "  Min: {}", stats.min_us).unwrap();
        writeln!(report, "  Max: {}", stats.max_us).unwrap();
        writeln!(report, "  p95: {}", stats.p95_us()).unwrap();
        writeln!(report, "  p99: {}", stats.p99_us()).unwrap();
        writeln!(report, "  p99.9: {}", stats.p999_us()).unwrap();
    } else {
        writeln!(report, "  No latency data collected").unwrap();
    }
    writeln!(report).unwrap();

    if !results.strategy_metrics.is_empty() {
        writeln!(report, "PER-STRATEGY:").unwrap();
        for strategy in &results.strategy_metrics {
            let stats = strategy.latency_stats.lock();
            writeln!(
                report,
                "  {}: recv={} avg={:.0}μs p99={}μs",
                strategy.id,
                format_number(strategy.received_count()),
                stats.average_us(),
                stats.p99_us()
            ).unwrap();
        }
        writeln!(report).unwrap();
    }

    if !results.failures.is_empty() {
        writeln!(report, "FAILURES:").unwrap();
        for failure in &results.failures {
            writeln!(report, "  - {}", failure).unwrap();
        }
    }

    report
}

/// Generate a compact single-line report
fn generate_compact_report(results: &TestResults) -> String {
    let status = if results.passed { "PASS" } else { "FAIL" };
    let stats = &results.latency_aggregate;

    format!(
        "[{}] duration={:.1}s ticks_sent={} ticks_recv={} loss={:.2}% lat_avg={:.0}μs lat_p99={}μs",
        status,
        results.test_duration.as_secs_f64(),
        format_number(results.ticks_sent),
        format_number(results.ticks_received_total),
        results.tick_loss_rate() * 100.0,
        stats.average_us(),
        stats.p99_us()
    )
}

/// Format a number with thousands separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let chars: Vec<char> = s.chars().rev().collect();
    let mut result = String::new();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }

    result.chars().rev().collect()
}

/// Format duration in human-readable format
#[allow(dead_code)]
fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        format!("{:.0}m {:.0}s", secs / 60.0, secs % 60.0)
    } else {
        format!("{:.0}h {:.0}m", secs / 3600.0, (secs % 3600.0) / 60.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{StrategyMetrics, TestResults};
    use std::sync::Arc;
    use std::time::Duration;

    fn create_test_results() -> TestResults {
        let mut results = TestResults::new();
        results.ticks_generated = 1_500_000;
        results.ticks_sent = 1_500_000;
        results.ticks_received_total = 1_499_850;
        results.ticks_persisted = Some(1_500_000);
        results.test_duration = Duration::from_secs_f64(65.2);
        results.passed = true;

        // Add latency stats
        for lat in [45, 100, 150, 198, 250, 300, 500, 1000, 2000, 5000, 12000] {
            results.latency_aggregate.record(lat);
        }

        // Add strategy metrics
        let strategy1 = Arc::new(StrategyMetrics::new(
            "rust_0".to_string(),
            "rust".to_string(),
            1000,
        ));
        for lat in [100, 150, 200, 250, 980] {
            strategy1.latency_stats.lock().record(lat);
        }
        for _ in 0..150_000 {
            strategy1.ticks_received.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        results.strategy_metrics.push(strategy1);

        let strategy2 = Arc::new(StrategyMetrics::new(
            "python_0".to_string(),
            "python".to_string(),
            1000,
        ));
        for lat in [200, 300, 400, 500, 2100] {
            strategy2.latency_stats.lock().record(lat);
        }
        for _ in 0..149_925 {
            strategy2.ticks_received.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        results.strategy_metrics.push(strategy2);

        results
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(100), "100");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1_000_000), "1,000,000");
        assert_eq!(format_number(1_500_000), "1,500,000");
    }

    #[test]
    fn test_pretty_report() {
        let results = create_test_results();
        let report = generate_report(&results, ReportFormat::Pretty);

        assert!(report.contains("PASSED"));
        assert!(report.contains("65.2s"));
        assert!(report.contains("1,500,000"));
        assert!(report.contains("LATENCY"));
        assert!(report.contains("rust_0"));
        assert!(report.contains("python_0"));
    }

    #[test]
    fn test_simple_report() {
        let results = create_test_results();
        let report = generate_report(&results, ReportFormat::Simple);

        assert!(report.contains("PASSED"));
        assert!(report.contains("Generated: 1,500,000"));
        assert!(report.contains("Loss Rate:"));
    }

    #[test]
    fn test_compact_report() {
        let results = create_test_results();
        let report = generate_report(&results, ReportFormat::Compact);

        assert!(report.starts_with("[PASS]"));
        assert!(report.contains("ticks_sent="));
        assert!(report.contains("lat_avg="));
    }

    #[test]
    fn test_failed_report() {
        let mut results = create_test_results();
        results.passed = false;
        results.failures.push("Tick loss rate exceeded tolerance".to_string());

        let report = generate_report(&results, ReportFormat::Pretty);

        assert!(report.contains("FAILED"));
        assert!(report.contains("FAILURES"));
        assert!(report.contains("Tick loss rate exceeded"));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30.0), "30.0s");
        assert_eq!(format_duration(90.0), "2m 30s");
        assert_eq!(format_duration(3700.0), "1h 2m");
    }
}
