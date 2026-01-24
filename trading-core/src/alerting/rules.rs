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
    /// IPC connection is disconnected
    IpcDisconnected,
    /// Excessive IPC reconnection attempts
    IpcReconnectionStorm { max_reconnects: u64 },
    /// Channel backpressure (buffer utilization too high)
    ChannelBackpressure { threshold_percent: f64 },
    /// Cache update failure rate exceeds threshold
    CacheFailureRate { threshold_percent: f64 },
}

impl AlertCondition {
    /// Evaluate the condition against current metrics
    pub fn evaluate(&self) -> bool {
        match self {
            AlertCondition::IpcDisconnected => IPC_CONNECTION_STATUS.get() == 0,

            AlertCondition::IpcReconnectionStorm { max_reconnects } => {
                IPC_RECONNECTS_TOTAL.get() > *max_reconnects
            }

            AlertCondition::ChannelBackpressure { threshold_percent } => {
                CHANNEL_UTILIZATION.get() >= *threshold_percent
            }

            AlertCondition::CacheFailureRate { threshold_percent } => {
                let failures = CACHE_UPDATE_FAILURES_TOTAL.get();
                let total = TICKS_PROCESSED_TOTAL.get();

                if total == 0 {
                    return false; // No ticks processed yet
                }

                let failure_rate = failures as f64 / total as f64;
                failure_rate >= *threshold_percent
            }
        }
    }

    /// Get a human-readable description of the condition
    pub fn description(&self) -> String {
        match self {
            AlertCondition::IpcDisconnected => {
                let reconnects = IPC_RECONNECTS_TOTAL.get();
                format!(
                    "IPC connection lost (total reconnection attempts: {})",
                    reconnects
                )
            }

            AlertCondition::IpcReconnectionStorm { max_reconnects } => {
                let reconnects = IPC_RECONNECTS_TOTAL.get();
                format!(
                    "Excessive IPC reconnection attempts: {} reconnects (threshold: {})",
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

            AlertCondition::CacheFailureRate { threshold_percent } => {
                let failures = CACHE_UPDATE_FAILURES_TOTAL.get();
                let total = TICKS_PROCESSED_TOTAL.get();
                let failure_rate = if total > 0 {
                    (failures as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                format!(
                    "High cache failure rate: {} failures out of {} processed ({:.1}% failure rate, threshold: {:.1}%)",
                    failures, total, failure_rate, threshold_percent * 100.0
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

    /// Create IPC disconnection critical rule
    pub fn ipc_disconnected() -> Self {
        Self::new(
            "ipc_disconnected".to_string(),
            AlertCondition::IpcDisconnected,
            AlertSeverity::Critical,
        )
    }

    /// Create IPC reconnection storm warning rule
    pub fn ipc_reconnection_storm(max_reconnects: u64) -> Self {
        Self::new(
            "ipc_reconnection_storm".to_string(),
            AlertCondition::IpcReconnectionStorm { max_reconnects },
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

    /// Create cache failure rate warning rule
    pub fn cache_failure_rate(threshold_percent: f64) -> Self {
        Self::new(
            "cache_failure_rate".to_string(),
            AlertCondition::CacheFailureRate { threshold_percent },
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
        let rule = AlertRule::ipc_disconnected();
        assert_eq!(rule.name(), "ipc_disconnected");
        assert_eq!(rule.severity(), AlertSeverity::Critical);
    }

    #[test]
    fn test_ipc_reconnection_storm_rule() {
        let rule = AlertRule::ipc_reconnection_storm(10);
        assert_eq!(rule.name(), "ipc_reconnection_storm");
        assert_eq!(rule.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn test_channel_backpressure_rule() {
        let rule = AlertRule::channel_backpressure(80.0);
        assert_eq!(rule.name(), "channel_backpressure");
        assert_eq!(rule.severity(), AlertSeverity::Warning);
    }

    #[test]
    fn test_cache_failure_rate_rule() {
        let rule = AlertRule::cache_failure_rate(0.1);
        assert_eq!(rule.name(), "cache_failure_rate");
        assert_eq!(rule.severity(), AlertSeverity::Warning);
    }
}
