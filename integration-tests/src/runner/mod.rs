//! Strategy runner framework for the integration test harness
//!
//! This module provides the infrastructure for running multiple concurrent
//! strategies (both Rust and Python) during integration tests.
//!
//! Each strategy runner subscribes to a specific symbol (e.g., rust_0 -> TEST0000,
//! rust_1 -> TEST0001) to simulate realistic per-symbol strategy execution.

mod python_runner;
mod rust_runner;

pub use python_runner::PythonStrategyRunner;
pub use rust_runner::RustStrategyRunner;

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use data_manager::schema::NormalizedTick;

use crate::config::StrategyConfig;
use crate::metrics::StrategyMetrics;
use crate::strategies::TestTickCounterStrategy;

/// Trait for strategy runners that can process ticks
#[async_trait]
pub trait StrategyRunner: Send + Sync {
    /// Get the strategy identifier
    fn id(&self) -> &str;

    /// Get the strategy type (rust/python)
    fn strategy_type(&self) -> &str;

    /// Get the symbol this runner is subscribed to
    fn subscribed_symbol(&self) -> &str;

    /// Process a single tick (only called if tick.symbol matches subscribed_symbol)
    async fn process_tick(&self, tick: &NormalizedTick);

    /// Get the metrics for this runner
    fn metrics(&self) -> Arc<StrategyMetrics>;

    /// Shutdown the runner
    async fn shutdown(&self);
}

/// Factory for creating strategy runners
pub struct StrategyRunnerFactory;

impl StrategyRunnerFactory {
    /// Create all strategy runners based on configuration and available symbols
    ///
    /// Each runner is assigned a specific symbol to subscribe to:
    /// - rust_0 -> symbols[0] (e.g., TEST0000)
    /// - rust_1 -> symbols[1] (e.g., TEST0001)
    /// - python_0 -> symbols[rust_count] (e.g., TEST0002)
    /// - etc.
    ///
    /// If there are more runners than symbols, runners wrap around to reuse symbols.
    pub fn create_runners(
        config: &StrategyConfig,
        symbols: &[String],
        sample_limit: usize,
    ) -> Vec<Arc<dyn StrategyRunner>> {
        let mut runners: Vec<Arc<dyn StrategyRunner>> = Vec::new();
        let mut symbol_index = 0;

        // Create Rust strategy runners
        for i in 0..config.rust_count {
            let id = format!("rust_{}", i);
            let symbol = if symbols.is_empty() {
                format!("TEST{:04}", i)
            } else {
                symbols[symbol_index % symbols.len()].clone()
            };
            symbol_index += 1;

            let runner = RustStrategyRunner::new(
                id,
                symbol,
                Box::new(TestTickCounterStrategy::new()),
                sample_limit,
            );
            runners.push(Arc::new(runner));
        }

        // Create Python strategy runners (if enabled)
        for i in 0..config.python_count {
            let id = format!("python_{}", i);
            let symbol = if symbols.is_empty() {
                format!("TEST{:04}", config.rust_count + i)
            } else {
                symbols[symbol_index % symbols.len()].clone()
            };
            symbol_index += 1;

            let runner = PythonStrategyRunner::new(id, symbol, sample_limit);
            runners.push(Arc::new(runner));
        }

        runners
    }
}

/// Manager for coordinating multiple strategy runners
///
/// Routes ticks to runners based on their subscribed symbols.
pub struct StrategyRunnerManager {
    runners: Vec<Arc<dyn StrategyRunner>>,
    /// Map from symbol to runners subscribed to that symbol
    symbol_subscriptions: HashMap<String, Vec<Arc<dyn StrategyRunner>>>,
}

impl StrategyRunnerManager {
    /// Create a new manager with the given runners
    pub fn new(runners: Vec<Arc<dyn StrategyRunner>>) -> Self {
        let symbol_subscriptions = Self::build_subscription_map(&runners);
        Self {
            runners,
            symbol_subscriptions,
        }
    }

    /// Build a map from symbol to subscribed runners
    fn build_subscription_map(
        runners: &[Arc<dyn StrategyRunner>],
    ) -> HashMap<String, Vec<Arc<dyn StrategyRunner>>> {
        let mut map: HashMap<String, Vec<Arc<dyn StrategyRunner>>> = HashMap::new();
        for runner in runners {
            map.entry(runner.subscribed_symbol().to_string())
                .or_default()
                .push(runner.clone());
        }
        map
    }

    /// Create a new manager from configuration and symbols
    pub fn from_config(config: &StrategyConfig, symbols: &[String], sample_limit: usize) -> Self {
        let runners = StrategyRunnerFactory::create_runners(config, symbols, sample_limit);
        Self::new(runners)
    }

    /// Get all runners
    pub fn runners(&self) -> &[Arc<dyn StrategyRunner>] {
        &self.runners
    }

    /// Get runners subscribed to a specific symbol
    pub fn runners_for_symbol(&self, symbol: &str) -> Option<&Vec<Arc<dyn StrategyRunner>>> {
        self.symbol_subscriptions.get(symbol)
    }

    /// Route a tick to runners subscribed to its symbol
    ///
    /// Only runners subscribed to `tick.symbol` will receive the tick.
    pub async fn route_tick(&self, tick: &NormalizedTick) {
        if let Some(runners) = self.symbol_subscriptions.get(&tick.symbol) {
            for runner in runners {
                runner.process_tick(tick).await;
            }
        }
    }

    /// Get all metrics
    pub fn all_metrics(&self) -> Vec<Arc<StrategyMetrics>> {
        self.runners.iter().map(|r| r.metrics()).collect()
    }

    /// Shutdown all runners
    pub async fn shutdown_all(&self) {
        for runner in &self.runners {
            runner.shutdown().await;
        }
    }

    /// Get subscription info for logging/debugging
    pub fn subscription_info(&self) -> Vec<(String, String, String)> {
        self.runners
            .iter()
            .map(|r| {
                (
                    r.id().to_string(),
                    r.strategy_type().to_string(),
                    r.subscribed_symbol().to_string(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use data_manager::schema::TradeSide;
    use rust_decimal::Decimal;

    fn create_test_tick(symbol: &str) -> NormalizedTick {
        NormalizedTick::new(
            Utc::now(),
            symbol.to_string(),
            "TEST".to_string(),
            Decimal::from(50000),
            Decimal::from(100),
            TradeSide::Buy,
            "test".to_string(),
            1,
        )
    }

    #[tokio::test]
    async fn test_runner_factory() {
        let config = StrategyConfig {
            rust_count: 3,
            python_count: 2,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        };

        let symbols: Vec<String> = (0..5).map(|i| format!("TEST{:04}", i)).collect();
        let runners = StrategyRunnerFactory::create_runners(&config, &symbols, 1000);

        assert_eq!(runners.len(), 5);

        let rust_count = runners
            .iter()
            .filter(|r| r.strategy_type() == "rust")
            .count();
        let python_count = runners
            .iter()
            .filter(|r| r.strategy_type() == "python")
            .count();

        assert_eq!(rust_count, 3);
        assert_eq!(python_count, 2);

        // Verify symbol assignments
        assert_eq!(runners[0].subscribed_symbol(), "TEST0000"); // rust_0
        assert_eq!(runners[1].subscribed_symbol(), "TEST0001"); // rust_1
        assert_eq!(runners[2].subscribed_symbol(), "TEST0002"); // rust_2
        assert_eq!(runners[3].subscribed_symbol(), "TEST0003"); // python_0
        assert_eq!(runners[4].subscribed_symbol(), "TEST0004"); // python_1
    }

    #[tokio::test]
    async fn test_runner_manager_route_tick() {
        let config = StrategyConfig {
            rust_count: 2,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        };

        let symbols = vec!["TEST0000".to_string(), "TEST0001".to_string()];
        let manager = StrategyRunnerManager::from_config(&config, &symbols, 1000);

        // Route ticks for TEST0000
        let tick0 = create_test_tick("TEST0000");
        for _ in 0..10 {
            manager.route_tick(&tick0).await;
        }

        // Route ticks for TEST0001
        let tick1 = create_test_tick("TEST0001");
        for _ in 0..5 {
            manager.route_tick(&tick1).await;
        }

        // Check only the subscribed runner received each tick
        let runners = manager.runners();
        assert_eq!(runners[0].subscribed_symbol(), "TEST0000");
        assert_eq!(runners[0].metrics().received_count(), 10);

        assert_eq!(runners[1].subscribed_symbol(), "TEST0001");
        assert_eq!(runners[1].metrics().received_count(), 5);
    }

    #[tokio::test]
    async fn test_runner_manager_unsubscribed_symbol() {
        let config = StrategyConfig {
            rust_count: 2,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        };

        let symbols = vec!["TEST0000".to_string(), "TEST0001".to_string()];
        let manager = StrategyRunnerManager::from_config(&config, &symbols, 1000);

        // Route tick for an unsubscribed symbol
        let tick = create_test_tick("TEST9999");
        manager.route_tick(&tick).await;

        // No runner should have received the tick
        for runner in manager.runners() {
            assert_eq!(runner.metrics().received_count(), 0);
        }
    }

    #[tokio::test]
    async fn test_runner_manager_metrics() {
        let config = StrategyConfig {
            rust_count: 2,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        };

        let symbols = vec!["TEST0000".to_string(), "TEST0001".to_string()];
        let manager = StrategyRunnerManager::from_config(&config, &symbols, 1000);

        let metrics = manager.all_metrics();
        assert_eq!(metrics.len(), 2);
    }

    #[tokio::test]
    async fn test_subscription_info() {
        let config = StrategyConfig {
            rust_count: 2,
            python_count: 1,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        };

        let symbols = vec![
            "TEST0000".to_string(),
            "TEST0001".to_string(),
            "TEST0002".to_string(),
        ];
        let manager = StrategyRunnerManager::from_config(&config, &symbols, 1000);

        let info = manager.subscription_info();
        assert_eq!(info.len(), 3);
        assert_eq!(
            info[0],
            (
                "rust_0".to_string(),
                "rust".to_string(),
                "TEST0000".to_string()
            )
        );
        assert_eq!(
            info[1],
            (
                "rust_1".to_string(),
                "rust".to_string(),
                "TEST0001".to_string()
            )
        );
        assert_eq!(
            info[2],
            (
                "python_0".to_string(),
                "python".to_string(),
                "TEST0002".to_string()
            )
        );
    }
}
