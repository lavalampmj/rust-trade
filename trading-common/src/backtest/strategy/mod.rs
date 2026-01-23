pub(crate) mod base;
mod config;
pub mod dispatcher;
pub mod position;
mod python_bridge;
mod python_loader;
mod rsi;
mod sma;
pub mod state;

#[cfg(test)]
mod security_tests;

pub use base::Strategy;
pub use config::StrategiesConfig;
pub use dispatcher::StrategyEventDispatcher;
pub use position::PositionEvent;
pub use python_loader::{
    calculate_file_hash, HotReloadConfig, PythonStrategyRegistry, ReloadMetrics, ReloadStats,
    StrategyConfig,
};
use rsi::RsiStrategy;
use sma::SmaStrategy;
pub use state::{StrategyState, StrategyStateEvent};

use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::OnceLock;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StrategyType {
    Rust,
    Python,
}

#[derive(Debug, Clone)]
pub struct StrategyInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub strategy_type: StrategyType,
}

// Global Python registry singleton
static PYTHON_REGISTRY: OnceLock<Arc<RwLock<PythonStrategyRegistry>>> = OnceLock::new();

/// Initialize Python strategy system
pub fn initialize_python_strategies(config_path: &str) -> Result<(), String> {
    let config = StrategiesConfig::load(config_path)?;

    let strategy_dir = config.python_dir.clone();

    // Create HotReloadConfig from settings
    let hot_reload_config = HotReloadConfig {
        debounce_ms: config.hot_reload_config.debounce_ms,
        skip_hash_verification: config.hot_reload_config.skip_hash_verification,
    };

    let mut registry =
        PythonStrategyRegistry::with_hot_reload_config(strategy_dir, hot_reload_config)?;

    // Register all Python strategies from config
    for entry in &config.python {
        let full_path = config.get_strategy_path(&entry.file);
        registry.register(StrategyConfig {
            id: entry.id.clone(),
            file_path: full_path,
            class_name: entry.class_name.clone(),
            description: entry.description.clone(),
            enabled: entry.enabled,
            sha256: entry.sha256.clone(),
        })?;
    }

    // Enable hot-reload if configured
    if config.hot_reload {
        registry.enable_hot_reload()?;
    }

    PYTHON_REGISTRY
        .set(Arc::new(RwLock::new(registry)))
        .map_err(|_| "Python registry already initialized")?;

    Ok(())
}

pub fn create_strategy(strategy_id: &str) -> Result<Box<dyn Strategy>, String> {
    // Try Python registry first
    if let Some(registry) = PYTHON_REGISTRY.get() {
        let reg = registry.read();
        match reg.get_strategy(strategy_id) {
            Ok(strategy) => return Ok(strategy),
            Err(e) => {
                // Only fall through if strategy is not a Python strategy
                if e.contains("not registered") || e == "Unknown strategy" {
                    // Not a Python strategy, continue to Rust strategies
                } else {
                    // Python strategy exists but failed to load
                    return Err(e);
                }
            }
        }
    }

    // Fall back to Rust strategies
    match strategy_id {
        "sma" => Ok(Box::new(SmaStrategy::new())),
        "rsi" => Ok(Box::new(RsiStrategy::new())),
        _ => Err(format!("Unknown strategy: {}", strategy_id)),
    }
}

pub fn list_strategies() -> Vec<StrategyInfo> {
    let mut strategies = Vec::new();

    // Add Python strategies
    if let Some(registry) = PYTHON_REGISTRY.get() {
        let reg = registry.read();
        for config in reg.list_strategies() {
            strategies.push(StrategyInfo {
                id: config.id,
                name: config.class_name,
                description: config.description,
                strategy_type: StrategyType::Python,
            });
        }
    }

    // Add Rust strategies
    strategies.push(StrategyInfo {
        id: "sma".to_string(),
        name: "Simple Moving Average (Rust)".to_string(),
        description: "Built-in Rust SMA implementation".to_string(),
        strategy_type: StrategyType::Rust,
    });

    strategies.push(StrategyInfo {
        id: "rsi".to_string(),
        name: "RSI Strategy (Rust)".to_string(),
        description: "Built-in Rust RSI implementation".to_string(),
        strategy_type: StrategyType::Rust,
    });

    strategies
}

pub fn get_strategy_info(strategy_id: &str) -> Option<StrategyInfo> {
    list_strategies()
        .into_iter()
        .find(|info| info.id == strategy_id)
}

/// Reload a Python strategy (invalidate cache)
pub fn reload_python_strategy(strategy_id: &str) -> Result<(), String> {
    let registry = PYTHON_REGISTRY
        .get()
        .ok_or("Python registry not initialized")?;

    let reg = registry.read();
    reg.reload_strategy(strategy_id);
    Ok(())
}
