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
    /// Validation settings
    #[serde(default)]
    pub validation: ValidationSettings,
    /// Scheduler settings
    #[serde(default)]
    pub scheduler: SchedulerSettings,
    /// Subscription settings
    #[serde(default)]
    pub subscription: SubscriptionSettings,
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
    /// Kraken configuration
    pub kraken: Option<KrakenSettings>,
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
    /// Reconnection settings
    #[serde(default)]
    pub reconnection: BinanceReconnectionSettings,
}

/// Binance reconnection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceReconnectionSettings {
    /// Initial reconnection delay in seconds
    #[serde(default = "default_initial_reconnect_delay")]
    pub initial_delay_secs: u64,
    /// Maximum reconnection delay in seconds
    #[serde(default = "default_max_reconnect_delay")]
    pub max_delay_secs: u64,
    /// Maximum reconnection attempts before giving up
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_attempts: u32,
}

fn default_initial_reconnect_delay() -> u64 {
    1
}

fn default_max_reconnect_delay() -> u64 {
    60
}

fn default_max_reconnect_attempts() -> u32 {
    10
}

impl Default for BinanceReconnectionSettings {
    fn default() -> Self {
        Self {
            initial_delay_secs: default_initial_reconnect_delay(),
            max_delay_secs: default_max_reconnect_delay(),
            max_attempts: default_max_reconnect_attempts(),
        }
    }
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
            default_symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
            reconnection: BinanceReconnectionSettings::default(),
        }
    }
}

/// Kraken provider settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenSettings {
    /// Enable Kraken provider
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Market type: "spot" or "futures"
    #[serde(default = "default_kraken_market_type")]
    pub market_type: String,
    /// Use demo/testnet endpoints (Futures only - Spot has no public testnet)
    #[serde(default)]
    pub demo: bool,
    /// Maximum reconnection attempts per window
    #[serde(default = "default_rate_limit_attempts")]
    pub rate_limit_attempts: u32,
    /// Rate limit window in seconds
    #[serde(default = "default_rate_limit_window")]
    pub rate_limit_window_secs: u64,
    /// Default symbols to subscribe to
    #[serde(default = "default_kraken_symbols")]
    pub default_symbols: Vec<String>,
}

fn default_kraken_market_type() -> String {
    "spot".to_string()
}

fn default_kraken_symbols() -> Vec<String> {
    vec!["XBT/USD".to_string(), "ETH/USD".to_string()]
}

impl Default for KrakenSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            market_type: default_kraken_market_type(),
            demo: false,
            rate_limit_attempts: default_rate_limit_attempts(),
            rate_limit_window_secs: default_rate_limit_window(),
            default_symbols: default_kraken_symbols(),
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
    /// Default lookback period for historical data (days)
    #[serde(default = "default_databento_lookback")]
    pub default_lookback_days: u32,
    /// Reconnection settings
    #[serde(default)]
    pub reconnection: DatabentoReconnectionSettings,
}

/// Databento reconnection settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabentoReconnectionSettings {
    /// Maximum reconnection attempts
    #[serde(default = "default_databento_max_attempts")]
    pub max_attempts: u32,
    /// Base delay in milliseconds
    #[serde(default = "default_databento_base_delay")]
    pub base_delay_ms: u64,
    /// Maximum delay in milliseconds
    #[serde(default = "default_databento_max_delay")]
    pub max_delay_ms: u64,
}

fn default_databento_lookback() -> u32 {
    30
}

fn default_databento_max_attempts() -> u32 {
    10
}

fn default_databento_base_delay() -> u64 {
    1000
}

fn default_databento_max_delay() -> u64 {
    30000
}

impl Default for DatabentoReconnectionSettings {
    fn default() -> Self {
        Self {
            max_attempts: default_databento_max_attempts(),
            base_delay_ms: default_databento_base_delay(),
            max_delay_ms: default_databento_max_delay(),
        }
    }
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

// =============================================================================
// VALIDATION CONFIGURATION
// =============================================================================

/// Validation settings for incoming tick data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSettings {
    /// Enable tick data validation
    #[serde(default = "default_validation_enabled")]
    pub enabled: bool,
    /// Maximum price (reject ticks above this value)
    #[serde(default = "default_max_price")]
    pub max_price: String,
    /// Maximum price change per tick (percentage)
    #[serde(default = "default_max_price_change")]
    pub max_price_change_percent: String,
    /// Maximum symbol length
    #[serde(default = "default_max_symbol_length")]
    pub max_symbol_length: usize,
    /// Maximum exchange name length
    #[serde(default = "default_max_exchange_length")]
    pub max_exchange_length: usize,
    /// Maximum trade ID length
    #[serde(default = "default_max_trade_id_length")]
    pub max_trade_id_length: usize,
    /// Maximum timestamp age in hours
    #[serde(default = "default_max_timestamp_age")]
    pub max_timestamp_age_hours: u64,
    /// Maximum timestamp future tolerance in seconds
    #[serde(default = "default_max_timestamp_future")]
    pub max_timestamp_future_secs: u64,
    /// Require trade ID on all ticks
    #[serde(default)]
    pub require_trade_id: bool,
    /// Symbol-specific price change thresholds
    #[serde(default)]
    pub symbol_overrides: std::collections::HashMap<String, String>,
}

fn default_validation_enabled() -> bool {
    true
}

fn default_max_price() -> String {
    "10000000".to_string() // $10M
}

fn default_max_price_change() -> String {
    "50".to_string() // 50%
}

fn default_max_symbol_length() -> usize {
    20
}

fn default_max_exchange_length() -> usize {
    20
}

fn default_max_trade_id_length() -> usize {
    100
}

fn default_max_timestamp_age() -> u64 {
    24 // hours
}

fn default_max_timestamp_future() -> u64 {
    60 // seconds
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            enabled: default_validation_enabled(),
            max_price: default_max_price(),
            max_price_change_percent: default_max_price_change(),
            max_symbol_length: default_max_symbol_length(),
            max_exchange_length: default_max_exchange_length(),
            max_trade_id_length: default_max_trade_id_length(),
            max_timestamp_age_hours: default_max_timestamp_age(),
            max_timestamp_future_secs: default_max_timestamp_future(),
            require_trade_id: false,
            symbol_overrides: std::collections::HashMap::new(),
        }
    }
}

// =============================================================================
// SCHEDULER CONFIGURATION
// =============================================================================

/// Scheduler settings for job management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerSettings {
    /// Maximum job history entries to retain
    #[serde(default = "default_max_job_history")]
    pub max_job_history: usize,
    /// Job cleanup interval in seconds
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_secs: u64,
}

fn default_max_job_history() -> usize {
    100
}

fn default_cleanup_interval() -> u64 {
    3600 // 1 hour
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            max_job_history: default_max_job_history(),
            cleanup_interval_secs: default_cleanup_interval(),
        }
    }
}

// =============================================================================
// SUBSCRIPTION CONFIGURATION
// =============================================================================

/// Subscription settings for market data subscriptions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionSettings {
    /// Default snapshot depth for order book
    #[serde(default = "default_snapshot_depth")]
    pub snapshot_depth: usize,
    /// Default backfill duration in minutes
    #[serde(default = "default_backfill_duration")]
    pub default_backfill_duration_minutes: u32,
    /// Default backfill max records
    #[serde(default = "default_backfill_max_records")]
    pub default_backfill_max_records: usize,
}

fn default_snapshot_depth() -> usize {
    100
}

fn default_backfill_duration() -> u32 {
    5
}

fn default_backfill_max_records() -> usize {
    10000
}

impl Default for SubscriptionSettings {
    fn default() -> Self {
        Self {
            snapshot_depth: default_snapshot_depth(),
            default_backfill_duration_minutes: default_backfill_duration(),
            default_backfill_max_records: default_backfill_max_records(),
        }
    }
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
        std::env::var("DATA_MANAGER_CONFIG_DIR").unwrap_or_else(|_| "config".into())
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
                kraken: Some(KrakenSettings::default()),
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
            validation: ValidationSettings::default(),
            scheduler: SchedulerSettings::default(),
            subscription: SubscriptionSettings::default(),
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
