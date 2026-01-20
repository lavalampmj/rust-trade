// alerting/rules.rs - Alert rule definitions and conditions

use crate::metrics::*;
use std::fmt;

/// Severity levels for alerts
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Warning = 1,
    Critical = 2,
}

impl fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlertSeverity::Warning => write!(f, "WARNING"),
            AlertSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Condition that determines if an alert should fire
pub enum AlertCondition {
    /// Connection pool utilization exceeds threshold (current/max >= threshold)
    ConnectionPoolUtilization {
        max_connections: i64,
        threshold_percent: f64,
    },
    /// Batch failure rate exceeds threshold
    BatchFailureRate {
        threshold_percent: f64,
    },
    /// WebSocket is disconnected
    WebSocketDisconnected,
    /// Excessive WebSocket reconnection attempts
    WebSocketReconnectionStorm {
        max_reconnects: u64,
    },
    /// Channel backpressure (buffer utilization too high)
    ChannelBackpressure {
        threshold_percent: f64,
    },
}

impl AlertCondition {
    /// Evaluate the condition against current metrics
    pub fn evaluate(&self) -> bool {
        match self {
            AlertCondition::ConnectionPoolUtilization {
                max_connections,
                threshold_percent,
            } => {
                let active = DB_CONNECTIONS_ACTIVE.get();
                let utilization = active as f64 / *max_connections as f64;
                utilization >= *threshold_percent
            }

            AlertCondition::BatchFailureRate { threshold_percent } => {
                let failed = BATCHES_FAILED_TOTAL.get();
                let flushed = BATCHES_FLUSHED_TOTAL.get();
                let total = failed + flushed;

                if total == 0 {
                    return false; // No batches processed yet
                }

                let failure_rate = failed as f64 / total as f64;
                failure_rate >= *threshold_percent
            }

            AlertCondition::WebSocketDisconnected => {
                WS_CONNECTION_STATUS.get() == 0
            }

            AlertCondition::WebSocketReconnectionStorm { max_reconnects } => {
                WS_RECONNECTS_TOTAL.get() > *max_reconnects
            }

            AlertCondition::ChannelBackpressure { threshold_percent } => {
                CHANNEL_UTILIZATION.get() >= *threshold_percent
            }
        }
    }

    /// Get a human-readable description of the condition
    pub fn description(&self) -> String {
        match self {
            AlertCondition::ConnectionPoolUtilization {
                max_connections,
                threshold_percent,
            } => {
                let active = DB_CONNECTIONS_ACTIVE.get();
                let utilization = (active as f64 / *max_connections as f64) * 100.0;
                format!(
                    "Database connection pool saturation: {} of {} connections active ({:.1}% utilization, threshold: {:.1}%)",
                    active, max_connections, utilization, threshold_percent * 100.0
                )
            }

            AlertCondition::BatchFailureRate { threshold_percent } => {
                let failed = BATCHES_FAILED_TOTAL.get();
                let flushed = BATCHES_FLUSHED_TOTAL.get();
                let total = failed + flushed;
                let failure_rate = if total > 0 {
                    (failed as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                format!(
                    "High batch failure rate: {} failures out of {} total batches ({:.1}% failure rate, threshold: {:.1}%)",
                    failed, total, failure_rate, threshold_percent * 100.0
                )
            }

            AlertCondition::WebSocketDisconnected => {
                let disconnections = WS_DISCONNECTIONS_TOTAL.get();
                format!(
                    "WebSocket connection lost (total disconnections: {})",
                    disconnections
                )
            }

            AlertCondition::WebSocketReconnectionStorm { max_reconnects } => {
                let reconnects = WS_RECONNECTS_TOTAL.get();
                format!(
                    "Excessive WebSocket reconnection attempts: {} reconnects (threshold: {})",
                    reconnects, max_reconnects
                )
            }

            AlertCondition::ChannelBackpressure { threshold_percent } => {
                let utilization = CHANNEL_UTILIZATION.get();
                let buffer_size = CHANNEL_BUFFER_SIZE.get();
                format!(
                    "Channel backpressure detected: {:.1}% utilization ({} ticks in buffer, threshold: {:.1}%)",
                    utilization, buffer_size, threshold_percent
                )
            }
        }
    }
}

/// Alert rule combining condition, severity, and metadata
pub struct AlertRule {
    name: String,
    condition: AlertCondition,
    severity: AlertSeverity,
}

impl AlertRule {
    /// Create a new alert rule
    pub fn new(name: String, condition: AlertCondition, severity: AlertSeverity) -> Self {
        Self {
            name,
            condition,
            severity,
        }
    }

    /// Create connection pool saturation warning rule (80%+ utilization)
    pub fn connection_pool_saturation(max_connections: i64, threshold_percent: f64) -> Self {
        Self::new(
            "connection_pool_saturation".to_string(),
            AlertCondition::ConnectionPoolUtilization {
                max_connections,
                threshold_percent,
            },
            AlertSeverity::Warning,
        )
    }

    /// Create connection pool critical rule (95%+ utilization)
    pub fn connection_pool_critical(max_connections: i64, threshold_percent: f64) -> Self {
        Self::new(
            "connection_pool_critical".to_string(),
            AlertCondition::ConnectionPoolUtilization {
                max_connections,
                threshold_percent,
            },
            AlertSeverity::Critical,
        )
    }

    /// Create batch failure rate warning rule
    pub fn batch_failure_rate(threshold_percent: f64) -> Self {
        Self::new(
            "batch_failure_rate".to_string(),
            AlertCondition::BatchFailureRate { threshold_percent },
            AlertSeverity::Warning,
        )
    }

    /// Create WebSocket disconnection critical rule
    pub fn websocket_disconnected() -> Self {
        Self::new(
            "websocket_disconnected".to_string(),
            AlertCondition::WebSocketDisconnected,
            AlertSeverity::Critical,
        )
    }

    /// Create WebSocket reconnection storm warning rule
    pub fn websocket_reconnection_storm(max_reconnects: u64) -> Self {
        Self::new(
            "websocket_reconnection_storm".to_string(),
            AlertCondition::WebSocketReconnectionStorm { max_reconnects },
            AlertSeverity::Warning,
        )
    }

    /// Create channel backpressure warning rule
    pub fn channel_backpressure(threshold_percent: f64) -> Self {
        Self::new(
            "channel_backpressure".to_string(),
            AlertCondition::ChannelBackpressure { threshold_percent },
            AlertSeverity::Warning,
        )
    }

    /// Evaluate the rule's condition
    pub fn evaluate(&self) -> bool {
        self.condition.evaluate()
    }

    /// Get the rule's name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the rule's severity
    pub fn severity(&self) -> AlertSeverity {
        self.severity
    }

    /// Get a description of the alert condition
    pub fn description(&self) -> String {
        self.condition.description()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alert_severity_display() {
        assert_eq!(AlertSeverity::Warning.to_string(), "WARNING");
        assert_eq!(AlertSeverity::Critical.to_string(), "CRITICAL");
    }

    #[test]
    fn test_alert_severity_ordering() {
        assert!(AlertSeverity::Critical > AlertSeverity::Warning);
    }

    #[test]
    fn test_alert_rule_creation() {
        let rule = AlertRule::connection_pool_saturation(8, 0.8);
        assert_eq!(rule.name(), "connection_pool_saturation");
        assert_eq!(rule.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn test_alert_description_includes_metrics() {
        DB_CONNECTIONS_ACTIVE.set(7);
        let rule = AlertRule::connection_pool_saturation(8, 0.8);
        let description = rule.description();

        assert!(description.contains("7"), "Should include active connections");
        assert!(description.contains("8"), "Should include max connections");
    }
}
