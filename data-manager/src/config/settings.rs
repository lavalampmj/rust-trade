//! Application settings and configuration

use crate::config::routing::RoutingConfig;
use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use trading_common::data::backfill_config::BackfillConfig;

/// Main application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Database configuration
    pub database: DatabaseSettings,
    /// Provider configurations
    pub provider: ProviderSettings,
    /// IPC/Transport configuration
    pub transport: TransportSettings,
    /// Symbol management configuration
    pub symbol: SymbolSettings,
    /// Storage settings
    pub storage: StorageSettings,
    /// Backfill configuration
    #[serde(default)]
    pub backfill: BackfillConfig,
    /// Provider routing configuration
    #[serde(default)]
    pub routing: RoutingConfig,
}

/// Database connection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSettings {
    /// PostgreSQL connection URL
    pub url: String,
    /// Maximum number of connections in the pool
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    /// Minimum number of connections in the pool
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
}

fn default_max_connections() -> u32 {
    10
}

fn default_min_connections() -> u32 {
    2
}

/// Provider-specific settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    /// Databento configuration
    pub databento: Option<DatabentoSettings>,
    /// Binance configuration
    pub binance: Option<BinanceSettings>,
}

/// Binance provider settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceSettings {
    /// Enable Binance provider
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// WebSocket URL
    #[serde(default = "default_binance_ws_url")]
    pub ws_url: String,
    /// Maximum reconnection attempts per window
    #[serde(default = "default_rate_limit_attempts")]
    pub rate_limit_attempts: u32,
    /// Rate limit window in seconds
    #[serde(default = "default_rate_limit_window")]
    pub rate_limit_window_secs: u64,
    /// Default symbols to subscribe to
    #[serde(default)]
    pub default_symbols: Vec<String>,
}

fn default_binance_ws_url() -> String {
    "wss://stream.binance.us:9443/stream".to_string()
}

fn default_rate_limit_attempts() -> u32 {
    5
}

fn default_rate_limit_window() -> u64 {
    60
}

impl Default for BinanceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            ws_url: default_binance_ws_url(),
            rate_limit_attempts: default_rate_limit_attempts(),
            rate_limit_window_secs: default_rate_limit_window(),
            default_symbols: vec![
                "BTCUSDT".to_string(),
                "ETHUSDT".to_string(),
            ],
        }
    }
}

/// Databento provider settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabentoSettings {
    /// API key for authentication
    pub api_key: String,
    /// Default dataset (e.g., "GLBX.MDP3" for CME Globex)
    #[serde(default = "default_databento_dataset")]
    pub default_dataset: String,
}

fn default_databento_dataset() -> String {
    "GLBX.MDP3".to_string()
}

/// Transport layer settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportSettings {
    /// IPC configuration
    pub ipc: IpcSettings,
    /// gRPC configuration (for future use)
    pub grpc: Option<GrpcSettings>,
}

/// IPC (shared memory) settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcSettings {
    /// Enable IPC transport
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Base path for shared memory segments
    #[serde(default = "default_shm_path")]
    pub shm_path_prefix: String,
    /// Number of entries per ring buffer
    #[serde(default = "default_ring_buffer_size")]
    pub ring_buffer_entries: usize,
    /// Size of each entry in bytes
    #[serde(default = "default_entry_size")]
    pub entry_size: usize,
}

fn default_true() -> bool {
    true
}

fn default_shm_path() -> String {
    "/data_manager_".to_string()
}

fn default_ring_buffer_size() -> usize {
    65536 // 64K entries
}

fn default_entry_size() -> usize {
    256 // bytes per entry
}

/// gRPC transport settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrpcSettings {
    /// Enable gRPC transport
    #[serde(default)]
    pub enabled: bool,
    /// Bind address
    #[serde(default = "default_grpc_bind")]
    pub bind_address: String,
}

fn default_grpc_bind() -> String {
    "0.0.0.0:50051".to_string()
}

/// Symbol management settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSettings {
    /// Default symbols to track
    #[serde(default)]
    pub default_symbols: Vec<String>,
    /// Enable automatic discovery from providers
    #[serde(default)]
    pub auto_discover: bool,
}

/// Storage settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    /// Compression policy (days after which to compress)
    #[serde(default = "default_compression_days")]
    pub compression_after_days: u32,
    /// Batch insert size
    #[serde(default = "default_batch_size")]
    pub batch_insert_size: usize,
}

fn default_compression_days() -> u32 {
    7
}

fn default_batch_size() -> usize {
    1000
}

impl Settings {
    /// Load settings from configuration files and environment
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_with_prefix("DATA_MANAGER")
    }

    /// Load settings with a custom environment variable prefix
    pub fn load_with_prefix(env_prefix: &str) -> Result<Self, ConfigError> {
        let run_mode = std::env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let config_dir = Self::config_dir();

        let s = Config::builder()
            // Start with default configuration
            .add_source(File::with_name(&format!("{}/default", config_dir)).required(false))
            // Add environment-specific configuration
            .add_source(File::with_name(&format!("{}/{}", config_dir, run_mode)).required(false))
            // Add local overrides (not checked into git)
            .add_source(File::with_name(&format!("{}/local", config_dir)).required(false))
            // Add environment variables (e.g., DATA_MANAGER__DATABASE__URL)
            .add_source(
                Environment::with_prefix(env_prefix)
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        s.try_deserialize()
    }

    /// Get the configuration directory path
    fn config_dir() -> String {
        std::env::var("DATA_MANAGER_CONFIG_DIR")
            .unwrap_or_else(|_| "config".into())
    }

    /// Create default settings (useful for testing)
    pub fn default_settings() -> Self {
        Settings {
            database: DatabaseSettings {
                url: std::env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "postgresql://localhost/data_manager".into()),
                max_connections: 10,
                min_connections: 2,
            },
            provider: ProviderSettings {
                databento: None,
                binance: Some(BinanceSettings::default()),
            },
            transport: TransportSettings {
                ipc: IpcSettings {
                    enabled: true,
                    shm_path_prefix: "/data_manager_".to_string(),
                    ring_buffer_entries: 65536,
                    entry_size: 256,
                },
                grpc: None,
            },
            symbol: SymbolSettings {
                default_symbols: vec![],
                auto_discover: false,
            },
            storage: StorageSettings {
                compression_after_days: 7,
                batch_insert_size: 1000,
            },
            backfill: BackfillConfig::default(),
            routing: RoutingConfig::default(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::default_settings()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default_settings();
        assert_eq!(settings.database.max_connections, 10);
        assert_eq!(settings.transport.ipc.ring_buffer_entries, 65536);
    }
}
