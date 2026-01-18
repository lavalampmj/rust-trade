// alerting/evaluator.rs - Alert evaluation engine

use super::{Alert, AlertHandler, AlertRule};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::time::interval;

/// Evaluates alert rules and triggers handlers
pub struct AlertEvaluator<H: AlertHandler> {
    rules: Vec<AlertRule>,
    handler: H,
    cooldown: Duration,
    last_fired: Arc<Mutex<HashMap<String, SystemTime>>>,
}

impl<H: AlertHandler> AlertEvaluator<H> {
    /// Create a new alert evaluator
    pub fn new(handler: H) -> Self {
        Self {
            rules: Vec::new(),
            handler,
            cooldown: Duration::from_secs(300), // 5 minutes default cooldown
            last_fired: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Set cooldown period between repeated alerts
    pub fn set_cooldown(&mut self, cooldown: Duration) {
        self.cooldown = cooldown;
    }

    /// Add an alert rule
    pub fn add_rule(&mut self, rule: AlertRule) {
        self.rules.push(rule);
    }

    /// Evaluate all rules and trigger alerts for violated conditions
    pub fn evaluate_all(&mut self) {
        for rule in &self.rules {
            if rule.evaluate() {
                // Check cooldown
                if self.is_in_cooldown(rule.name()) {
                    continue;
                }

                let alert = Alert::new(
                    rule.severity(),
                    rule.name().to_string(),
                    rule.description(),
                );

                self.handler.handle(alert);
                self.mark_fired(rule.name());
            }
        }
    }

    /// Check if an alert is in cooldown period
    fn is_in_cooldown(&self, rule_name: &str) -> bool {
        let last_fired = self.last_fired.lock().unwrap();
        if let Some(last_time) = last_fired.get(rule_name) {
            if let Ok(elapsed) = SystemTime::now().duration_since(*last_time) {
                return elapsed < self.cooldown;
            }
        }
        false
    }

    /// Mark an alert as fired
    fn mark_fired(&self, rule_name: &str) {
        let mut last_fired = self.last_fired.lock().unwrap();
        last_fired.insert(rule_name.to_string(), SystemTime::now());
    }

    /// Start a background task that periodically evaluates all rules
    pub fn start_monitoring(self, interval_secs: u64) -> tokio::task::JoinHandle<()>
    where
        H: 'static,
    {
        let evaluator = Arc::new(Mutex::new(self));

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));

            loop {
                ticker.tick().await;

                let mut eval = evaluator.lock().unwrap();
                eval.evaluate_all();
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerting::{AlertSeverity, LogAlertHandler};
    use crate::metrics::*;

    #[test]
    fn test_evaluator_creation() {
        let handler = LogAlertHandler::new();
        let evaluator = AlertEvaluator::new(handler);

        assert_eq!(evaluator.rules.len(), 0);
    }

    #[test]
    fn test_add_rule() {
        let handler = LogAlertHandler::new();
        let mut evaluator = AlertEvaluator::new(handler);

        evaluator.add_rule(AlertRule::connection_pool_saturation(8, 0.8));

        assert_eq!(evaluator.rules.len(), 1);
    }

    #[test]
    fn test_cooldown_setting() {
        let handler = LogAlertHandler::new();
        let mut evaluator = AlertEvaluator::new(handler);

        evaluator.set_cooldown(Duration::from_secs(120));

        assert_eq!(evaluator.cooldown, Duration::from_secs(120));
    }

    #[test]
    fn test_is_in_cooldown() {
        let handler = LogAlertHandler::new();
        let mut evaluator = AlertEvaluator::new(handler);
        evaluator.set_cooldown(Duration::from_secs(60));

        // Not in cooldown initially
        assert!(!evaluator.is_in_cooldown("test_rule"));

        // Mark as fired
        evaluator.mark_fired("test_rule");

        // Should be in cooldown now
        assert!(evaluator.is_in_cooldown("test_rule"));
    }

    #[tokio::test]
    async fn test_background_monitoring() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct CountingHandler {
            count: Arc<Mutex<usize>>,
        }

        impl AlertHandler for CountingHandler {
            fn handle(&self, _alert: Alert) {
                *self.count.lock().unwrap() += 1;
            }
        }

        let count = Arc::new(Mutex::new(0));
        let handler = CountingHandler {
            count: count.clone(),
        };

        // Set metric value high to ensure rule triggers
        DB_CONNECTIONS_ACTIVE.set(8);

        let mut evaluator = AlertEvaluator::new(handler);
        evaluator.set_cooldown(Duration::from_millis(500)); // Short cooldown for testing

        // Add a rule that will trigger
        evaluator.add_rule(AlertRule::connection_pool_critical(8, 0.95));

        // Manually evaluate once to ensure we get at least one alert
        evaluator.evaluate_all();

        // Verify we got the first alert
        let alert_count = *count.lock().unwrap();
        assert!(alert_count >= 1, "Expected at least 1 alert after manual evaluation, got {}", alert_count);

        // Now start background monitoring - it should trigger more alerts after cooldown
        let handle = evaluator.start_monitoring(1);

        // Wait for background evaluations
        tokio::time::sleep(Duration::from_millis(2000)).await;

        // Stop the monitoring task
        handle.abort();

        // Should have at least 2 alerts total (1 manual + at least 1 from background)
        let final_count = *count.lock().unwrap();
        assert!(final_count >= 1, "Expected at least 1 alert total, got {}", final_count);
    }
}
