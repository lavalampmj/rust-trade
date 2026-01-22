//! Configuration structs for the integration test harness
//!
//! This module defines the configuration structures that control test behavior,
//! including data generation profiles, emulator settings, and strategy configurations.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

use crate::transport::{TransportConfig, TransportMode, WebSocketConfig};

/// Volume profile for test data generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VolumeProfile {
    /// ~1000 ticks/sec/symbol (clustered bursts)
    Heavy,
    /// ~250 ticks/sec/symbol (Poisson distribution)
    Normal,
    /// ~25 ticks/sec/symbol (uniform distribution)
    Lite,
}

impl VolumeProfile {
    /// Get the target ticks per second per symbol for this profile
    pub fn ticks_per_second_per_symbol(&self) -> u64 {
        match self {
            VolumeProfile::Heavy => 1000,
            VolumeProfile::Normal => 250,
            VolumeProfile::Lite => 25,
        }
    }

    /// Get the total expected ticks for a given symbol count and time window
    pub fn expected_total_ticks(&self, symbol_count: usize, time_window_secs: u64) -> u64 {
        self.ticks_per_second_per_symbol() * symbol_count as u64 * time_window_secs
    }
}

impl Default for VolumeProfile {
    fn default() -> Self {
        VolumeProfile::Normal
    }
}

/// Configuration for the test data generator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataGenConfig {
    /// Number of symbols to generate (generates TEST0000..TEST{N-1})
    pub symbol_count: usize,
    /// Time window in seconds for the generated data
    pub time_window_secs: u64,
    /// Volume profile (Heavy/Normal/Lite)
    pub profile: VolumeProfile,
    /// Random seed for deterministic reproducibility
    pub seed: u64,
    /// Exchange name for generated data
    pub exchange: String,
    /// Base price for random walk (default: 50000.00)
    #[serde(default = "default_base_price")]
    pub base_price: f64,
}

fn default_base_price() -> f64 {
    50000.0
}

impl Default for DataGenConfig {
    fn default() -> Self {
        Self {
            symbol_count: 10,
            time_window_secs: 60,
            profile: VolumeProfile::Normal,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        }
    }
}

impl DataGenConfig {
    /// Calculate the expected number of ticks to be generated
    pub fn expected_tick_count(&self) -> u64 {
        self.profile
            .expected_total_ticks(self.symbol_count, self.time_window_secs)
    }

    /// Generate symbol names (TEST0000, TEST0001, etc.)
    pub fn generate_symbol_names(&self) -> Vec<String> {
        (0..self.symbol_count)
            .map(|i| format!("TEST{:04}", i))
            .collect()
    }
}

/// Configuration for the data emulator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmulatorConfig {
    /// Replay speed multiplier (1.0 = real-time)
    #[serde(default = "default_replay_speed")]
    pub replay_speed: f64,
    /// Whether to embed send timestamp in ts_in_delta field
    #[serde(default = "default_true")]
    pub embed_send_time: bool,
    /// Minimum inter-tick delay in microseconds
    #[serde(default = "default_min_delay_us")]
    pub min_delay_us: u64,
    /// Transport configuration (Direct or WebSocket)
    #[serde(default)]
    pub transport: TransportConfig,
}

fn default_replay_speed() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_min_delay_us() -> u64 {
    10
}

impl Default for EmulatorConfig {
    fn default() -> Self {
        Self {
            replay_speed: 1.0,
            embed_send_time: true,
            min_delay_us: 10,
            transport: TransportConfig::default(),
        }
    }
}

impl EmulatorConfig {
    /// Set WebSocket port (works with default WebSocket transport)
    pub fn with_port(mut self, port: u16) -> Self {
        self.transport.websocket.port = port;
        self
    }

    /// Create config with WebSocket transport on specified port
    pub fn with_websocket(mut self, port: u16) -> Self {
        self.transport = TransportConfig {
            mode: TransportMode::WebSocket,
            websocket: WebSocketConfig {
                port,
                ..Default::default()
            },
        };
        self
    }

    /// Create config with Direct transport (in-memory, for baseline measurements)
    pub fn with_direct(mut self) -> Self {
        self.transport = TransportConfig {
            mode: TransportMode::Direct,
            ..Default::default()
        };
        self
    }
}

/// Configuration for strategy runners
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Number of Rust strategy instances to run
    pub rust_count: usize,
    /// Number of Python strategy instances to run
    pub python_count: usize,
    /// Strategy type to use
    pub strategy_type: String,
    /// Whether to enable latency tracking
    #[serde(default = "default_true")]
    pub track_latency: bool,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            rust_count: 5,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        }
    }
}

/// Configuration for metrics collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Maximum number of latency samples to keep for percentile calculations
    pub latency_sample_limit: usize,
    /// Maximum acceptable tick loss rate (0.001 = 0.1%)
    pub tick_loss_tolerance: f64,
    /// Maximum acceptable average latency in microseconds
    #[serde(default = "default_max_avg_latency_us")]
    pub max_avg_latency_us: u64,
    /// Maximum acceptable p99 latency in microseconds
    #[serde(default = "default_max_p99_latency_us")]
    pub max_p99_latency_us: u64,
}

fn default_max_avg_latency_us() -> u64 {
    1000 // 1ms
}

fn default_max_p99_latency_us() -> u64 {
    10000 // 10ms
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            latency_sample_limit: 100_000,
            tick_loss_tolerance: 0.001,
            max_avg_latency_us: 1000,
            max_p99_latency_us: 10000,
        }
    }
}

/// Configuration for the overall test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    /// Test timeout in seconds
    pub timeout_secs: u64,
    /// Settling time after replay completes (to allow pipeline to drain)
    pub settling_time_secs: u64,
    /// Database URL for verification
    #[serde(default)]
    pub database_url: Option<String>,
    /// Whether to verify database persistence
    #[serde(default)]
    pub verify_db_persistence: bool,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            settling_time_secs: 5,
            database_url: None,
            verify_db_persistence: false,
        }
    }
}

/// Complete integration test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationTestConfig {
    /// Data generation configuration
    pub data_gen: DataGenConfig,
    /// Emulator configuration
    pub emulator: EmulatorConfig,
    /// Strategy configuration
    pub strategies: StrategyConfig,
    /// Metrics collection configuration
    pub metrics: MetricsConfig,
    /// Test execution configuration
    pub test: TestConfig,
}

impl Default for IntegrationTestConfig {
    fn default() -> Self {
        Self {
            data_gen: DataGenConfig::default(),
            emulator: EmulatorConfig::default(),
            strategies: StrategyConfig::default(),
            metrics: MetricsConfig::default(),
            test: TestConfig::default(),
        }
    }
}

impl IntegrationTestConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            ConfigError::IoError(format!("Failed to read config file: {}", e))
        })?;
        Self::from_str(&content)
    }

    /// Parse configuration from a TOML string
    pub fn from_str(content: &str) -> Result<Self, ConfigError> {
        toml::from_str(content)
            .map_err(|e| ConfigError::ParseError(format!("Failed to parse TOML: {}", e)))
    }

    /// Get the test duration as a Duration type
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.test.timeout_secs)
    }

    /// Get the settling time as a Duration type
    pub fn settling_time(&self) -> Duration {
        Duration::from_secs(self.test.settling_time_secs)
    }

    /// Get the path to a config file, checking multiple locations
    fn find_config_file(name: &str) -> Option<std::path::PathBuf> {
        // Try relative to CARGO_MANIFEST_DIR (for tests)
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let path = std::path::PathBuf::from(&manifest_dir).join("config").join(name);
            if path.exists() {
                return Some(path);
            }
        }

        // Try relative to current directory
        let path = std::path::PathBuf::from("config").join(name);
        if path.exists() {
            return Some(path);
        }

        // Try integration-tests/config (when running from workspace root)
        let path = std::path::PathBuf::from("integration-tests/config").join(name);
        if path.exists() {
            return Some(path);
        }

        None
    }

    /// Create a lite configuration for quick tests
    /// Loads from config/lite.toml if available
    pub fn lite() -> Self {
        if let Some(path) = Self::find_config_file("lite.toml") {
            if let Ok(config) = Self::from_file(&path) {
                println!("Loaded config from {}", path.display());
                return config;
            }
        }
        // Fallback to defaults if file not found
        Self::lite_defaults()
    }

    /// Default lite configuration (used when TOML file not found)
    fn lite_defaults() -> Self {
        Self {
            data_gen: DataGenConfig {
                symbol_count: 5,
                time_window_secs: 10,
                profile: VolumeProfile::Lite,
                seed: 12345,
                exchange: "TEST".to_string(),
                base_price: 50000.0,
            },
            emulator: EmulatorConfig::default().with_port(19100),
            strategies: StrategyConfig {
                rust_count: 2,
                python_count: 2,
                strategy_type: "tick_counter".to_string(),
                track_latency: true,
            },
            metrics: MetricsConfig::default(),
            test: TestConfig {
                timeout_secs: 30,
                settling_time_secs: 2,
                database_url: None,
                verify_db_persistence: false,
            },
        }
    }

    /// Create a normal configuration for standard stress tests
    /// Loads from config/default.toml if available
    pub fn normal() -> Self {
        if let Some(path) = Self::find_config_file("default.toml") {
            if let Ok(config) = Self::from_file(&path) {
                println!("Loaded config from {}", path.display());
                return config;
            }
        }
        // Fallback to defaults if file not found
        Self::normal_defaults()
    }

    /// Default normal configuration (used when TOML file not found)
    fn normal_defaults() -> Self {
        Self {
            data_gen: DataGenConfig {
                symbol_count: 10,
                time_window_secs: 60,
                profile: VolumeProfile::Normal,
                seed: 12345,
                exchange: "TEST".to_string(),
                base_price: 50000.0,
            },
            emulator: EmulatorConfig::default().with_port(19200),
            strategies: StrategyConfig {
                rust_count: 5,
                python_count: 5,
                strategy_type: "tick_counter".to_string(),
                track_latency: true,
            },
            metrics: MetricsConfig::default(),
            test: TestConfig {
                timeout_secs: 120,
                settling_time_secs: 5,
                database_url: None,
                verify_db_persistence: false,
            },
        }
    }

    /// Create a heavy configuration for full stress tests
    /// Loads from config/heavy.toml if available
    pub fn heavy() -> Self {
        if let Some(path) = Self::find_config_file("heavy.toml") {
            if let Ok(config) = Self::from_file(&path) {
                println!("Loaded config from {}", path.display());
                return config;
            }
        }
        // Fallback to defaults if file not found
        Self::heavy_defaults()
    }

    /// Default heavy configuration (used when TOML file not found)
    fn heavy_defaults() -> Self {
        Self {
            data_gen: DataGenConfig {
                symbol_count: 100,
                time_window_secs: 60,
                profile: VolumeProfile::Heavy,
                seed: 12345,
                exchange: "TEST".to_string(),
                base_price: 50000.0,
            },
            emulator: EmulatorConfig::default().with_port(19300),
            strategies: StrategyConfig {
                rust_count: 50,
                python_count: 50,
                strategy_type: "tick_counter".to_string(),
                track_latency: true,
            },
            metrics: MetricsConfig {
                latency_sample_limit: 100_000,
                tick_loss_tolerance: 0.05,
                max_avg_latency_us: 5000,
                max_p99_latency_us: 50000,
            },
            test: TestConfig {
                timeout_secs: 300,
                settling_time_secs: 10,
                database_url: None,
                verify_db_persistence: false,
            },
        }
    }
}

/// Configuration errors
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_profile_ticks() {
        assert_eq!(VolumeProfile::Heavy.ticks_per_second_per_symbol(), 1000);
        assert_eq!(VolumeProfile::Normal.ticks_per_second_per_symbol(), 250);
        assert_eq!(VolumeProfile::Lite.ticks_per_second_per_symbol(), 25);
    }

    #[test]
    fn test_expected_total_ticks() {
        let profile = VolumeProfile::Normal;
        // 10 symbols * 60 seconds * 250 ticks/sec = 150,000
        assert_eq!(profile.expected_total_ticks(10, 60), 150_000);
    }

    #[test]
    fn test_symbol_name_generation() {
        let config = DataGenConfig {
            symbol_count: 5,
            ..Default::default()
        };
        let symbols = config.generate_symbol_names();
        assert_eq!(symbols.len(), 5);
        assert_eq!(symbols[0], "TEST0000");
        assert_eq!(symbols[4], "TEST0004");
    }

    #[test]
    fn test_config_deserialization() {
        let toml_str = r#"
[data_gen]
symbol_count = 15
time_window_secs = 30
profile = "Heavy"
seed = 99999
exchange = "TESTEX"

[emulator]
replay_speed = 2.0

[strategies]
rust_count = 3
python_count = 2
strategy_type = "custom"

[metrics]
latency_sample_limit = 50000
tick_loss_tolerance = 0.002

[test]
timeout_secs = 60
settling_time_secs = 3
"#;
        let config = IntegrationTestConfig::from_str(toml_str).unwrap();
        assert_eq!(config.data_gen.symbol_count, 15);
        assert_eq!(config.data_gen.profile, VolumeProfile::Heavy);
        assert_eq!(config.emulator.replay_speed, 2.0);
        assert_eq!(config.strategies.rust_count, 3);
        assert_eq!(config.strategies.python_count, 2);
    }

    #[test]
    fn test_preset_configs() {
        let lite = IntegrationTestConfig::lite();
        assert_eq!(lite.data_gen.symbol_count, 5);
        assert_eq!(lite.data_gen.profile, VolumeProfile::Lite);

        let normal = IntegrationTestConfig::normal();
        assert_eq!(normal.data_gen.symbol_count, 10);
        assert_eq!(normal.data_gen.profile, VolumeProfile::Normal);

        let heavy = IntegrationTestConfig::heavy();
        assert_eq!(heavy.data_gen.symbol_count, 100);
        assert_eq!(heavy.data_gen.profile, VolumeProfile::Heavy);
    }
}
