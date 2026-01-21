//! Strategy runner framework for the integration test harness
//!
//! This module provides the infrastructure for running multiple concurrent
//! strategies (both Rust and Python) during integration tests.

mod rust_runner;
mod python_runner;

pub use rust_runner::{RustStrategyRunner, TestTickCounterStrategy};
pub use python_runner::PythonStrategyRunner;

use async_trait::async_trait;
use std::sync::Arc;

use data_manager::schema::NormalizedTick;

use crate::config::StrategyConfig;
use crate::metrics::StrategyMetrics;

/// Trait for strategy runners that can process ticks
#[async_trait]
pub trait StrategyRunner: Send + Sync {
    /// Get the strategy identifier
    fn id(&self) -> &str;

    /// Get the strategy type (rust/python)
    fn strategy_type(&self) -> &str;

    /// Process a single tick
    async fn process_tick(&self, tick: &NormalizedTick);

    /// Get the metrics for this runner
    fn metrics(&self) -> Arc<StrategyMetrics>;

    /// Shutdown the runner
    async fn shutdown(&self);
}

/// Factory for creating strategy runners
pub struct StrategyRunnerFactory;

impl StrategyRunnerFactory {
    /// Create all strategy runners based on configuration
    pub fn create_runners(
        config: &StrategyConfig,
        sample_limit: usize,
    ) -> Vec<Arc<dyn StrategyRunner>> {
        let mut runners: Vec<Arc<dyn StrategyRunner>> = Vec::new();

        // Create Rust strategy runners
        for i in 0..config.rust_count {
            let id = format!("rust_{}", i);
            let runner = RustStrategyRunner::new(
                id,
                Box::new(TestTickCounterStrategy::new()),
                sample_limit,
            );
            runners.push(Arc::new(runner));
        }

        // Create Python strategy runners (if enabled)
        for i in 0..config.python_count {
            let id = format!("python_{}", i);
            let runner = PythonStrategyRunner::new(id, sample_limit);
            runners.push(Arc::new(runner));
        }

        runners
    }
}

/// Manager for coordinating multiple strategy runners
pub struct StrategyRunnerManager {
    runners: Vec<Arc<dyn StrategyRunner>>,
}

impl StrategyRunnerManager {
    /// Create a new manager with the given runners
    pub fn new(runners: Vec<Arc<dyn StrategyRunner>>) -> Self {
        Self { runners }
    }

    /// Create a new manager from configuration
    pub fn from_config(config: &StrategyConfig, sample_limit: usize) -> Self {
        let runners = StrategyRunnerFactory::create_runners(config, sample_limit);
        Self { runners }
    }

    /// Get all runners
    pub fn runners(&self) -> &[Arc<dyn StrategyRunner>] {
        &self.runners
    }

    /// Broadcast a tick to all runners
    pub async fn broadcast_tick(&self, tick: &NormalizedTick) {
        for runner in &self.runners {
            runner.process_tick(tick).await;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use data_manager::schema::TradeSide;
    use rust_decimal::Decimal;

    fn create_test_tick() -> NormalizedTick {
        NormalizedTick::new(
            Utc::now(),
            "TEST0000".to_string(),
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

        let runners = StrategyRunnerFactory::create_runners(&config, 1000);

        assert_eq!(runners.len(), 5);

        let rust_count = runners.iter().filter(|r| r.strategy_type() == "rust").count();
        let python_count = runners.iter().filter(|r| r.strategy_type() == "python").count();

        assert_eq!(rust_count, 3);
        assert_eq!(python_count, 2);
    }

    #[tokio::test]
    async fn test_runner_manager_broadcast() {
        let config = StrategyConfig {
            rust_count: 2,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        };

        let manager = StrategyRunnerManager::from_config(&config, 1000);
        let tick = create_test_tick();

        // Broadcast multiple ticks
        for _ in 0..10 {
            manager.broadcast_tick(&tick).await;
        }

        // Check all runners received ticks
        for runner in manager.runners() {
            assert_eq!(runner.metrics().received_count(), 10);
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

        let manager = StrategyRunnerManager::from_config(&config, 1000);

        let metrics = manager.all_metrics();
        assert_eq!(metrics.len(), 2);
    }
}
