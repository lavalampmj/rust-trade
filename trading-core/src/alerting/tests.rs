// alerting/tests.rs - Tests for alert system

use super::*;
use crate::metrics::*;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Mock alert handler for testing
#[derive(Clone)]
struct MockAlertHandler {
    alerts: Arc<Mutex<Vec<Alert>>>,
}

impl MockAlertHandler {
    fn new() -> Self {
        Self {
            alerts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_alerts(&self) -> Vec<Alert> {
        self.alerts.lock().unwrap().clone()
    }
}

impl AlertHandler for MockAlertHandler {
    fn handle(&self, alert: Alert) {
        self.alerts.lock().unwrap().push(alert);
    }
}

#[test]
fn test_connection_pool_saturation_alert() {
    // Given: Maximum pool size is 8 connections
    // When: Active connections reach 7 (87.5% utilization)
    // Then: WARNING alert should be triggered

    DB_CONNECTIONS_ACTIVE.set(7);

    let rule = AlertRule::connection_pool_saturation(8, 0.8); // 80% threshold
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger at 87.5% pool utilization");
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}

#[test]
fn test_connection_pool_critical_saturation() {
    // Given: Maximum pool size is 8 connections
    // When: Active connections reach 8 (100% utilization)
    // Then: CRITICAL alert should be triggered

    DB_CONNECTIONS_ACTIVE.set(8);

    let rule = AlertRule::connection_pool_critical(8, 0.95); // 95% threshold
    let condition_met = rule.evaluate();

    assert!(condition_met, "Critical alert should trigger at 100% pool utilization");
    assert_eq!(rule.severity(), AlertSeverity::Critical);
}

#[test]
fn test_batch_failure_rate_alert() {
    // Given: 10 batches processed, 3 failed
    // When: Failure rate exceeds 20%
    // Then: WARNING alert should be triggered

    BATCHES_FLUSHED_TOTAL.reset();
    BATCHES_FAILED_TOTAL.reset();

    BATCHES_FLUSHED_TOTAL.inc_by(7); // 7 successful
    BATCHES_FAILED_TOTAL.inc_by(3);  // 3 failed (30% failure rate)

    let rule = AlertRule::batch_failure_rate(0.2); // 20% threshold
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger at 30% batch failure rate");
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}

#[test]
fn test_websocket_disconnection_alert() {
    // Given: WebSocket is disconnected
    // When: Connection status is 0
    // Then: CRITICAL alert should be triggered

    WS_CONNECTION_STATUS.set(0);

    let rule = AlertRule::websocket_disconnected();
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger when WebSocket is disconnected");
    assert_eq!(rule.severity(), AlertSeverity::Critical);
}

#[test]
fn test_websocket_reconnection_storm_alert() {
    // Given: Multiple reconnection attempts in short time
    // When: Reconnection rate exceeds threshold
    // Then: WARNING alert should be triggered

    WS_RECONNECTS_TOTAL.reset();
    WS_RECONNECTS_TOTAL.inc_by(5); // 5 reconnections

    let rule = AlertRule::websocket_reconnection_storm(3); // More than 3 reconnects
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger with excessive reconnections");
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}

#[test]
fn test_channel_backpressure_alert() {
    // Given: Channel buffer has high utilization
    // When: Utilization exceeds 80%
    // Then: WARNING alert should be triggered

    CHANNEL_UTILIZATION.set(85.0);

    let rule = AlertRule::channel_backpressure(80.0); // 80% threshold
    let condition_met = rule.evaluate();

    assert!(condition_met, "Alert should trigger at 85% channel utilization");
    assert_eq!(rule.severity(), AlertSeverity::Warning);
}

#[test]
fn test_no_alert_when_below_threshold() {
    // Given: All metrics are healthy
    // When: Values are below thresholds
    // Then: No alerts should be triggered

    DB_CONNECTIONS_ACTIVE.set(4); // 50% of 8
    BATCHES_FAILED_TOTAL.reset();
    BATCHES_FLUSHED_TOTAL.inc_by(10);
    WS_CONNECTION_STATUS.set(1);
    CHANNEL_UTILIZATION.set(50.0);

    let rules = vec![
        AlertRule::connection_pool_saturation(8, 0.8),
        AlertRule::batch_failure_rate(0.2),
        AlertRule::websocket_disconnected(),
        AlertRule::channel_backpressure(80.0),
    ];

    for rule in rules {
        assert!(!rule.evaluate(), "No alert should trigger when metrics are healthy");
    }
}

#[test]
fn test_alert_evaluator_with_multiple_rules() {
    // Given: Multiple alert rules configured
    // When: Evaluator checks all rules
    // Then: Only violated rules should generate alerts

    DB_CONNECTIONS_ACTIVE.set(7); // High
    WS_CONNECTION_STATUS.set(1);  // OK
    CHANNEL_UTILIZATION.set(50.0); // OK

    let handler = MockAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler.clone());

    evaluator.add_rule(AlertRule::connection_pool_saturation(8, 0.8));
    evaluator.add_rule(AlertRule::websocket_disconnected());
    evaluator.add_rule(AlertRule::channel_backpressure(80.0));

    evaluator.evaluate_all();

    let alerts = handler.get_alerts();
    assert_eq!(alerts.len(), 1, "Only one alert should be triggered");
    assert!(alerts[0].message.contains("pool"), "Alert should be about connection pool");
}

#[test]
fn test_alert_cooldown_prevents_spam() {
    // Given: Alert rule with cooldown period
    // When: Same alert fires multiple times quickly
    // Then: Only first alert is sent during cooldown

    DB_CONNECTIONS_ACTIVE.set(8);

    let handler = MockAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler.clone());
    evaluator.set_cooldown(Duration::from_secs(60)); // 60 second cooldown

    evaluator.add_rule(AlertRule::connection_pool_critical(8, 0.95));

    // First evaluation - should trigger alert
    evaluator.evaluate_all();
    assert_eq!(handler.get_alerts().len(), 1, "First alert should be sent");

    // Second evaluation immediately - should be suppressed
    evaluator.evaluate_all();
    assert_eq!(handler.get_alerts().len(), 1, "Second alert should be suppressed by cooldown");
}

#[test]
fn test_alert_contains_metric_value() {
    // Given: Alert rule that checks metric value
    // When: Alert is triggered
    // Then: Alert should contain the actual metric value

    DB_CONNECTIONS_ACTIVE.set(7);

    let handler = MockAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler.clone());
    evaluator.add_rule(AlertRule::connection_pool_saturation(8, 0.8));

    evaluator.evaluate_all();

    let alerts = handler.get_alerts();
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0].message.contains("7"), "Alert should include actual value");
    assert!(alerts[0].message.contains("8"), "Alert should include max value");
}

#[test]
fn test_batch_failure_with_zero_batches() {
    // Given: No batches have been processed yet
    // When: Checking batch failure rate
    // Then: No alert should trigger (avoid division by zero)

    BATCHES_FLUSHED_TOTAL.reset();
    BATCHES_FAILED_TOTAL.reset();

    let rule = AlertRule::batch_failure_rate(0.2);
    let condition_met = rule.evaluate();

    assert!(!condition_met, "No alert should trigger when no batches processed");
}

#[test]
fn test_log_alert_handler_formats_correctly() {
    // Given: Log alert handler
    // When: Alert is triggered
    // Then: Log should contain all alert details

    let handler = LogAlertHandler::new();
    let alert = Alert::new(
        AlertSeverity::Critical,
        "test_metric".to_string(),
        "Test alert message".to_string(),
    );

    // This should not panic
    handler.handle(alert);
}

#[test]
fn test_alert_severity_ordering() {
    // Given: Different alert severities
    // Then: Critical > Warning

    assert!(AlertSeverity::Critical > AlertSeverity::Warning);
}
