// alerting/mod.rs - Alert system for monitoring critical metrics

mod evaluator;
mod handler;
mod rules;

pub use evaluator::AlertEvaluator;
pub use handler::{Alert, AlertHandler, LogAlertHandler};
pub use rules::{AlertRule, AlertSeverity};

#[cfg(test)]
mod tests;
