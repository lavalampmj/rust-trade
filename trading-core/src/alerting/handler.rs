// alerting/handler.rs - Alert handler implementations

use super::AlertSeverity;
use std::time::SystemTime;

/// An alert that has been triggered
#[derive(Debug, Clone)]
pub struct Alert {
    pub severity: AlertSeverity,
    pub metric_name: String,
    pub message: String,
    pub timestamp: SystemTime,
}

impl Alert {
    /// Create a new alert
    pub fn new(severity: AlertSeverity, metric_name: String, message: String) -> Self {
        Self {
            severity,
            metric_name,
            message,
            timestamp: SystemTime::now(),
        }
    }
}

/// Trait for handling alerts
pub trait AlertHandler: Send + Sync {
    /// Handle an alert
    fn handle(&self, alert: Alert);
}

/// Alert handler that logs to standard output
#[derive(Clone)]
pub struct LogAlertHandler;

impl LogAlertHandler {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LogAlertHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertHandler for LogAlertHandler {
    fn handle(&self, alert: Alert) {
        match alert.severity {
            AlertSeverity::Info => {
                tracing::info!(
                    metric = %alert.metric_name,
                    message = %alert.message,
                    "[ALERT:INFO]"
                );
            }
            AlertSeverity::Warning => {
                tracing::warn!(
                    metric = %alert.metric_name,
                    message = %alert.message,
                    "[ALERT:WARNING]"
                );
            }
            AlertSeverity::Critical => {
                tracing::error!(
                    metric = %alert.metric_name,
                    message = %alert.message,
                    "[ALERT:CRITICAL]"
                );
            }
        }
    }
}

/// Composite handler that sends alerts to multiple handlers
pub struct MultiAlertHandler {
    handlers: Vec<Box<dyn AlertHandler>>,
}

impl MultiAlertHandler {
    /// Create a new multi-handler
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Add a handler to the chain
    pub fn add_handler<H: AlertHandler + 'static>(mut self, handler: H) -> Self {
        self.handlers.push(Box::new(handler));
        self
    }
}

impl Default for MultiAlertHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertHandler for MultiAlertHandler {
    fn handle(&self, alert: Alert) {
        for handler in &self.handlers {
            handler.handle(alert.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct CountingHandler {
        count: Arc<Mutex<usize>>,
    }

    impl CountingHandler {
        fn new() -> Self {
            Self {
                count: Arc::new(Mutex::new(0)),
            }
        }

        fn get_count(&self) -> usize {
            *self.count.lock().unwrap()
        }
    }

    impl AlertHandler for CountingHandler {
        fn handle(&self, _alert: Alert) {
            *self.count.lock().unwrap() += 1;
        }
    }

    #[test]
    fn test_log_handler_does_not_panic() {
        let handler = LogAlertHandler::new();
        let alert = Alert::new(
            AlertSeverity::Warning,
            "test_metric".to_string(),
            "Test alert".to_string(),
        );

        handler.handle(alert);
    }

    #[test]
    fn test_multi_handler_calls_all_handlers() {
        let handler1 = CountingHandler::new();
        let handler2 = CountingHandler::new();
        let handler1_clone = handler1.clone();
        let handler2_clone = handler2.clone();

        let multi = MultiAlertHandler::new()
            .add_handler(handler1)
            .add_handler(handler2);

        let alert = Alert::new(
            AlertSeverity::Info,
            "test".to_string(),
            "Test".to_string(),
        );

        multi.handle(alert);

        assert_eq!(handler1_clone.get_count(), 1);
        assert_eq!(handler2_clone.get_count(), 1);
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert::new(
            AlertSeverity::Critical,
            "db_pool".to_string(),
            "Pool saturated".to_string(),
        );

        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert_eq!(alert.metric_name, "db_pool");
        assert_eq!(alert.message, "Pool saturated");
    }
}
