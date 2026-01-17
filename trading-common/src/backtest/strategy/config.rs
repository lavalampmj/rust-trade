use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StrategiesConfig {
    pub python_dir: PathBuf,
    #[serde(default = "default_hot_reload")]
    pub hot_reload: bool,
    #[serde(default)]
    pub python: Vec<PythonStrategyConfigEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ResourceLimits {
    /// Maximum execution time per tick/OHLC call in milliseconds
    #[serde(default = "default_max_execution_time_ms")]
    pub max_execution_time_ms: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_execution_time_ms: default_max_execution_time_ms(),
        }
    }
}

fn default_max_execution_time_ms() -> u64 {
    10 // 10ms default timeout
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PythonStrategyConfigEntry {
    pub id: String,
    pub file: String,
    pub class_name: String,
    pub description: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub limits: ResourceLimits,
}

fn default_enabled() -> bool {
    true
}

fn default_hot_reload() -> bool {
    false
}

impl StrategiesConfig {
    /// Load strategies configuration from TOML config file
    pub fn load(config_path: &str) -> Result<Self, String> {
        // Read the entire config file
        let content = std::fs::read_to_string(config_path)
            .map_err(|e| format!("Failed to read config file '{}': {}", config_path, e))?;

        // Parse as TOML
        let config_value: toml::Value = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse TOML: {}", e))?;

        // Extract strategies section
        let strategies_table = config_value
            .get("strategies")
            .ok_or_else(|| "No [strategies] section found in config".to_string())?;

        // Deserialize to StrategiesConfig
        let mut strategies_config: StrategiesConfig = strategies_table.clone().try_into()
            .map_err(|e| format!("Failed to deserialize strategies config: {}", e))?;

        // Resolve python_dir relative to config file's project root if it's a relative path
        // The config file is typically at "config/development.toml" relative to project root
        // So we go up from config dir to get to project root
        if strategies_config.python_dir.is_relative() {
            if let Some(config_parent) = std::path::Path::new(config_path).parent() {
                // If config is in a subdirectory (like "config/"), go up to project root
                let project_root = if config_parent.ends_with("config") {
                    config_parent.parent().unwrap_or(config_parent)
                } else {
                    config_parent
                };
                strategies_config.python_dir = project_root.join(&strategies_config.python_dir);
            }
        }

        Ok(strategies_config)
    }

    /// Get full path for a strategy file
    pub fn get_strategy_path(&self, file: &str) -> PathBuf {
        self.python_dir.join(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_strategy_path() {
        let config = StrategiesConfig {
            python_dir: PathBuf::from("strategies"),
            hot_reload: true,
            python: vec![],
        };

        let path = config.get_strategy_path("example_sma.py");
        assert_eq!(path, PathBuf::from("strategies/example_sma.py"));
    }
}
