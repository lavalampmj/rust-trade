// alerting/mod.rs - Alert system for monitoring critical metrics

mod rules;
mod evaluator;
mod handler;

pub use rules::{AlertRule, AlertCondition, AlertSeverity};
pub use evaluator::AlertEvaluator;
pub use handler::{AlertHandler, LogAlertHandler, Alert};

#[cfg(test)]
mod tests;
