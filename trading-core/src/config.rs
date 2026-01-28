//! Configuration types for trading-core.
//!
//! These structs are deserialized from TOML config files.
//! Fields may not be directly read but are needed for config parsing.

#![allow(dead_code)]

use config::{Config, ConfigError, File};
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct Database {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub max_lifetime: u64,
    /// Connection acquire timeout in seconds
    #[serde(default = "default_acquire_timeout_secs")]
    pub acquire_timeout_secs: u64,
    /// Idle connection timeout in seconds
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
}

fn default_acquire_timeout_secs() -> u64 {
    30
}

fn default_idle_timeout_secs() -> u64 {
    600
}

#[derive(Debug, Deserialize)]
pub struct MemoryCache {
    pub max_ticks_per_symbol: usize,
    pub ttl_seconds: u64,
}

#[derive(Debug, Deserialize)]
pub struct RedisCache {
    pub url: String,
    pub ttl_seconds: u64,
    pub max_ticks_per_symbol: usize,
}

#[derive(Debug, Deserialize)]
pub struct Cache {
    pub memory: MemoryCache,
    pub redis: RedisCache,
}

#[derive(Debug, Deserialize)]
pub struct PaperTrading {
    pub enabled: bool,
    pub strategy: String,
    pub initial_capital: f64,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Validation {
    #[serde(default = "default_validation_enabled")]
    pub enabled: bool,
    #[serde(default = "default_max_price_change_pct")]
    pub max_price_change_pct: f64,
    #[serde(default = "default_timestamp_tolerance_minutes")]
    pub timestamp_tolerance_minutes: i64,
    #[serde(default = "default_max_past_days")]
    pub max_past_days: i64,
}

impl Default for Validation {
    fn default() -> Self {
        Self {
            enabled: default_validation_enabled(),
            max_price_change_pct: default_max_price_change_pct(),
            timestamp_tolerance_minutes: default_timestamp_tolerance_minutes(),
            max_past_days: default_max_past_days(),
        }
    }
}

fn default_validation_enabled() -> bool {
    true
}
fn default_max_price_change_pct() -> f64 {
    10.0
}
fn default_timestamp_tolerance_minutes() -> i64 {
    5
}
fn default_max_past_days() -> i64 {
    3650
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct ReconnectionRateLimitConfig {
    /// Maximum number of reconnection attempts allowed in the time window
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_attempts: u32,
    /// Time window in seconds (60 = per minute, 3600 = per hour)
    #[serde(default = "default_reconnect_window_secs")]
    pub window_secs: u64,
    /// Optional custom wait duration in seconds when limit is exceeded
    #[serde(default)]
    pub wait_on_limit_secs: Option<u64>,
}

impl Default for ReconnectionRateLimitConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_reconnect_attempts(),
            window_secs: default_reconnect_window_secs(),
            wait_on_limit_secs: None,
        }
    }
}

impl ReconnectionRateLimitConfig {
    /// Convert to rate limiter config for exchange use
    #[allow(dead_code)]
    pub fn to_rate_limiter_config(
        &self,
    ) -> crate::exchange::rate_limiter::ReconnectionRateLimiterConfig {
        use crate::exchange::rate_limiter::{
            ReconnectionRateLimiterConfig as RLConfig, ReconnectionWindow,
        };
        use std::time::Duration;

        let window = if self.window_secs == 60 {
            ReconnectionWindow::PerMinute
        } else if self.window_secs == 3600 {
            ReconnectionWindow::PerHour
        } else {
            ReconnectionWindow::Custom(Duration::from_secs(self.window_secs))
        };

        RLConfig {
            max_attempts: self.max_attempts,
            window,
            wait_on_limit_exceeded: self.wait_on_limit_secs.map(Duration::from_secs),
        }
    }
}

fn default_max_reconnect_attempts() -> u32 {
    5
}

fn default_reconnect_window_secs() -> u64 {
    60 // Per minute
}

#[derive(Debug, Deserialize, Clone)]
pub struct AlertingConfig {
    /// Enable alerting system
    #[serde(default = "default_alerting_enabled")]
    pub enabled: bool,
    /// Evaluation interval in seconds
    #[serde(default = "default_alert_interval_secs")]
    pub interval_secs: u64,
    /// Cooldown period between repeated alerts in seconds
    #[serde(default = "default_alert_cooldown_secs")]
    pub cooldown_secs: u64,
    /// Channel backpressure threshold (0.0-100.0)
    #[serde(default = "default_channel_backpressure_threshold")]
    pub channel_backpressure_threshold: f64,
    /// IPC reconnection storm threshold (number of reconnects)
    #[serde(default = "default_reconnection_storm_threshold")]
    pub reconnection_storm_threshold: u64,
    /// Cache failure rate threshold (0.0-1.0)
    #[serde(default)]
    pub cache_failure_threshold: Option<f64>,
}

impl Default for AlertingConfig {
    fn default() -> Self {
        Self {
            enabled: default_alerting_enabled(),
            interval_secs: default_alert_interval_secs(),
            cooldown_secs: default_alert_cooldown_secs(),
            channel_backpressure_threshold: default_channel_backpressure_threshold(),
            reconnection_storm_threshold: default_reconnection_storm_threshold(),
            cache_failure_threshold: Some(default_cache_failure_threshold()),
        }
    }
}

fn default_alerting_enabled() -> bool {
    true
}
fn default_alert_interval_secs() -> u64 {
    30 // Check every 30 seconds
}
fn default_alert_cooldown_secs() -> u64 {
    300 // 5 minutes
}
fn default_channel_backpressure_threshold() -> f64 {
    80.0 // 80%
}
fn default_reconnection_storm_threshold() -> u64 {
    10 // More than 10 reconnects
}
fn default_cache_failure_threshold() -> f64 {
    0.1 // 10%
}

// =============================================================================
// FEE CONFIGURATION
// =============================================================================

/// Fee rate configuration (maker/taker rates as decimal strings)
#[derive(Debug, Deserialize, Clone)]
pub struct FeeRateConfig {
    /// Maker fee rate (e.g., "0.001" for 0.1%)
    #[serde(default = "default_maker_rate")]
    pub maker: String,
    /// Taker fee rate (e.g., "0.001" for 0.1%)
    #[serde(default = "default_taker_rate")]
    pub taker: String,
}

fn default_maker_rate() -> String {
    "0.001".to_string()
}

fn default_taker_rate() -> String {
    "0.001".to_string()
}

impl Default for FeeRateConfig {
    fn default() -> Self {
        Self {
            maker: default_maker_rate(),
            taker: default_taker_rate(),
        }
    }
}

impl FeeRateConfig {
    pub fn maker_decimal(&self) -> Decimal {
        Decimal::from_str(&self.maker).unwrap_or(Decimal::new(1, 3))
    }

    pub fn taker_decimal(&self) -> Decimal {
        Decimal::from_str(&self.taker).unwrap_or(Decimal::new(1, 3))
    }
}

/// Fee tier configuration for tiered fee structures
#[derive(Debug, Deserialize, Clone)]
pub struct FeeTierConfig {
    /// Tier name (e.g., "VIP0", "VIP1")
    pub name: String,
    /// Minimum 30-day volume threshold
    pub min_volume: String,
    /// Maker rate
    pub maker: String,
    /// Taker rate
    pub taker: String,
}

/// Complete fee configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct FeeConfig {
    /// Default fee model to use: "zero", "binance_spot", "binance_futures", "custom"
    #[serde(default = "default_fee_model")]
    pub default_model: String,
    /// Binance spot fees
    #[serde(default)]
    pub binance_spot: FeeRateConfig,
    /// Binance spot fees with BNB discount
    #[serde(default = "default_binance_spot_bnb")]
    pub binance_spot_bnb: FeeRateConfig,
    /// Binance futures fees
    #[serde(default = "default_binance_futures")]
    pub binance_futures: FeeRateConfig,
    /// Custom fee rates (used when default_model = "custom")
    #[serde(default)]
    pub custom: FeeRateConfig,
    /// Tiered fee structure
    #[serde(default)]
    pub tiered: Vec<FeeTierConfig>,
}

fn default_fee_model() -> String {
    "zero".to_string()
}

fn default_binance_spot_bnb() -> FeeRateConfig {
    FeeRateConfig {
        maker: "0.00075".to_string(),
        taker: "0.00075".to_string(),
    }
}

fn default_binance_futures() -> FeeRateConfig {
    FeeRateConfig {
        maker: "0.0002".to_string(),
        taker: "0.0004".to_string(),
    }
}

// =============================================================================
// LATENCY CONFIGURATION
// =============================================================================

/// Latency model configuration for order execution simulation
#[derive(Debug, Deserialize, Clone)]
pub struct LatencyConfig {
    /// Latency model: "none", "fixed", "variable"
    #[serde(default = "default_latency_model")]
    pub model: String,
    /// Insert (order submission) latency in milliseconds
    #[serde(default = "default_insert_latency_ms")]
    pub insert_ms: u64,
    /// Update (order modification) latency in milliseconds
    #[serde(default = "default_update_latency_ms")]
    pub update_ms: u64,
    /// Delete (order cancellation) latency in milliseconds
    #[serde(default = "default_delete_latency_ms")]
    pub delete_ms: u64,
    /// Jitter for variable latency model (max additional delay in ms)
    #[serde(default)]
    pub jitter_ms: u64,
    /// Random seed for variable latency (0 = random seed)
    #[serde(default)]
    pub seed: u64,
}

fn default_latency_model() -> String {
    "fixed".to_string()
}

fn default_insert_latency_ms() -> u64 {
    50
}

fn default_update_latency_ms() -> u64 {
    50
}

fn default_delete_latency_ms() -> u64 {
    30
}

impl Default for LatencyConfig {
    fn default() -> Self {
        Self {
            model: default_latency_model(),
            insert_ms: default_insert_latency_ms(),
            update_ms: default_update_latency_ms(),
            delete_ms: default_delete_latency_ms(),
            jitter_ms: 0,
            seed: 0,
        }
    }
}

// =============================================================================
// SERVICE CONFIGURATION
// =============================================================================

/// Market data service configuration
#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    /// Channel capacity for tick processing pipeline
    #[serde(default = "default_channel_capacity")]
    pub channel_capacity: usize,
    /// Health monitor interval in seconds
    #[serde(default = "default_health_monitor_interval_secs")]
    pub health_monitor_interval_secs: u64,
    /// Bar timer interval in seconds (for OHLC generation)
    #[serde(default = "default_bar_timer_interval_secs")]
    pub bar_timer_interval_secs: u64,
    /// Reconnection delay on IPC failure in seconds
    #[serde(default = "default_reconnection_delay_secs")]
    pub reconnection_delay_secs: u64,
}

fn default_channel_capacity() -> usize {
    10_000
}

fn default_health_monitor_interval_secs() -> u64 {
    30
}

fn default_bar_timer_interval_secs() -> u64 {
    1
}

fn default_reconnection_delay_secs() -> u64 {
    5
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            channel_capacity: default_channel_capacity(),
            health_monitor_interval_secs: default_health_monitor_interval_secs(),
            bar_timer_interval_secs: default_bar_timer_interval_secs(),
            reconnection_delay_secs: default_reconnection_delay_secs(),
        }
    }
}

// =============================================================================
// IPC DATA SOURCE CONFIGURATION
// =============================================================================

/// IPC data source configuration
#[derive(Debug, Deserialize, Clone)]
pub struct IpcConfig {
    /// Shared memory path prefix (must match data-manager)
    #[serde(default = "default_shm_path_prefix")]
    pub shm_path_prefix: String,
    /// Poll interval in microseconds when no data available
    #[serde(default = "default_poll_interval_us")]
    pub poll_interval_us: u64,
    /// Channel buffer size for tick forwarding
    #[serde(default = "default_ipc_channel_buffer_size")]
    pub channel_buffer_size: usize,
    /// Stats logging interval in seconds
    #[serde(default = "default_stats_interval_secs")]
    pub stats_interval_secs: u64,
}

fn default_shm_path_prefix() -> String {
    "/data_manager_".to_string()
}

fn default_poll_interval_us() -> u64 {
    100
}

fn default_ipc_channel_buffer_size() -> usize {
    10_000
}

fn default_stats_interval_secs() -> u64 {
    30
}

impl Default for IpcConfig {
    fn default() -> Self {
        Self {
            shm_path_prefix: default_shm_path_prefix(),
            poll_interval_us: default_poll_interval_us(),
            channel_buffer_size: default_ipc_channel_buffer_size(),
            stats_interval_secs: default_stats_interval_secs(),
        }
    }
}

// =============================================================================
// METRICS CONFIGURATION
// =============================================================================

/// Metrics server configuration
#[derive(Debug, Deserialize, Clone)]
pub struct MetricsConfig {
    /// Bind address for metrics HTTP server
    #[serde(default = "default_metrics_bind_address")]
    pub bind_address: String,
    /// Tick interval for metrics updates in seconds
    #[serde(default = "default_metrics_tick_interval_secs")]
    pub tick_interval_secs: u64,
}

fn default_metrics_bind_address() -> String {
    "0.0.0.0:9090".to_string()
}

fn default_metrics_tick_interval_secs() -> u64 {
    1
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            bind_address: default_metrics_bind_address(),
            tick_interval_secs: default_metrics_tick_interval_secs(),
        }
    }
}

// =============================================================================
// STRATEGY CONFIGURATION
// =============================================================================

/// RSI strategy configuration
#[derive(Debug, Deserialize, Clone)]
pub struct RsiStrategyConfig {
    /// RSI calculation period
    #[serde(default = "default_rsi_period")]
    pub period: usize,
    /// Oversold threshold (buy signal)
    #[serde(default = "default_rsi_oversold")]
    pub oversold: u32,
    /// Overbought threshold (sell signal)
    #[serde(default = "default_rsi_overbought")]
    pub overbought: u32,
    /// Default order quantity
    #[serde(default = "default_order_quantity")]
    pub order_quantity: String,
    /// Lookback buffer multiplier (period * multiplier + buffer)
    #[serde(default = "default_lookback_multiplier")]
    pub lookback_multiplier: f64,
    /// Additional lookback buffer
    #[serde(default = "default_lookback_buffer")]
    pub lookback_buffer: usize,
}

fn default_rsi_period() -> usize {
    14
}

fn default_rsi_oversold() -> u32 {
    30
}

fn default_rsi_overbought() -> u32 {
    70
}

fn default_order_quantity() -> String {
    "100".to_string()
}

fn default_lookback_multiplier() -> f64 {
    2.0
}

fn default_lookback_buffer() -> usize {
    10
}

impl Default for RsiStrategyConfig {
    fn default() -> Self {
        Self {
            period: default_rsi_period(),
            oversold: default_rsi_oversold(),
            overbought: default_rsi_overbought(),
            order_quantity: default_order_quantity(),
            lookback_multiplier: default_lookback_multiplier(),
            lookback_buffer: default_lookback_buffer(),
        }
    }
}

/// SMA strategy configuration
#[derive(Debug, Deserialize, Clone)]
pub struct SmaStrategyConfig {
    /// Short period for fast MA
    #[serde(default = "default_sma_short_period")]
    pub short_period: usize,
    /// Long period for slow MA
    #[serde(default = "default_sma_long_period")]
    pub long_period: usize,
    /// Default order quantity
    #[serde(default = "default_order_quantity")]
    pub order_quantity: String,
    /// Lookback buffer multiplier (long_period * multiplier)
    #[serde(default = "default_lookback_multiplier")]
    pub lookback_multiplier: f64,
}

fn default_sma_short_period() -> usize {
    5
}

fn default_sma_long_period() -> usize {
    20
}

impl Default for SmaStrategyConfig {
    fn default() -> Self {
        Self {
            short_period: default_sma_short_period(),
            long_period: default_sma_long_period(),
            order_quantity: default_order_quantity(),
            lookback_multiplier: default_lookback_multiplier(),
        }
    }
}

/// Built-in strategies configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct BuiltinStrategiesConfig {
    /// RSI strategy defaults
    #[serde(default)]
    pub rsi: RsiStrategyConfig,
    /// SMA strategy defaults
    #[serde(default)]
    pub sma: SmaStrategyConfig,
}

// =============================================================================
// BACKTEST CONFIGURATION
// =============================================================================

/// Backtest engine configuration
#[derive(Debug, Deserialize, Clone)]
pub struct BacktestConfig {
    /// Default commission rate (e.g., "0.001" for 0.1%)
    #[serde(default = "default_commission_rate")]
    pub default_commission_rate: String,
    /// Default number of records for interactive backtest
    #[serde(default = "default_backtest_records")]
    pub default_records: i64,
    /// Default initial capital for interactive backtest
    #[serde(default = "default_backtest_capital")]
    pub default_capital: String,
    /// Minimum records required for backtest
    #[serde(default = "default_minimum_records")]
    pub minimum_records: i64,
    /// Metrics calculation configuration
    #[serde(default)]
    pub metrics: BacktestMetricsConfig,
}

/// Backtest metrics calculation configuration
#[derive(Debug, Deserialize, Clone)]
pub struct BacktestMetricsConfig {
    /// Square root calculation tolerance for Newton's method
    #[serde(default = "default_sqrt_tolerance")]
    pub sqrt_tolerance: String,
    /// Maximum iterations for square root calculation
    #[serde(default = "default_sqrt_max_iterations")]
    pub sqrt_max_iterations: u32,
}

fn default_sqrt_tolerance() -> String {
    "0.000001".to_string()
}

fn default_sqrt_max_iterations() -> u32 {
    50
}

impl Default for BacktestMetricsConfig {
    fn default() -> Self {
        Self {
            sqrt_tolerance: default_sqrt_tolerance(),
            sqrt_max_iterations: default_sqrt_max_iterations(),
        }
    }
}

fn default_commission_rate() -> String {
    "0.001".to_string()
}

fn default_backtest_records() -> i64 {
    10000
}

fn default_backtest_capital() -> String {
    "10000".to_string()
}

fn default_minimum_records() -> i64 {
    100
}

impl Default for BacktestConfig {
    fn default() -> Self {
        Self {
            default_commission_rate: default_commission_rate(),
            default_records: default_backtest_records(),
            default_capital: default_backtest_capital(),
            minimum_records: default_minimum_records(),
            metrics: BacktestMetricsConfig::default(),
        }
    }
}

// =============================================================================
// REPOSITORY CONFIGURATION
// =============================================================================

/// Repository query configuration
#[derive(Debug, Deserialize, Clone)]
pub struct RepositoryConfig {
    /// Default query limit for database queries
    #[serde(default = "default_query_limit")]
    pub default_query_limit: u32,
    /// Maximum query limit for database queries
    #[serde(default = "default_max_query_limit")]
    pub max_query_limit: u32,
    /// Maximum batch size for bulk inserts
    #[serde(default = "default_max_batch_size")]
    pub max_batch_size: usize,
}

fn default_query_limit() -> u32 {
    1000
}

fn default_max_query_limit() -> u32 {
    10000
}

fn default_max_batch_size() -> usize {
    1000
}

impl Default for RepositoryConfig {
    fn default() -> Self {
        Self {
            default_query_limit: default_query_limit(),
            max_query_limit: default_max_query_limit(),
            max_batch_size: default_max_batch_size(),
        }
    }
}

// =============================================================================
// TAURI DESKTOP APP CONFIGURATION
// =============================================================================

/// Tauri desktop app configuration
#[derive(Debug, Deserialize, Clone)]
pub struct TauriConfig {
    /// Default query limit for historical data requests
    #[serde(default = "default_tauri_query_limit")]
    pub default_query_limit: i64,
    /// Maximum query limit for historical data requests
    #[serde(default = "default_tauri_max_query_limit")]
    pub max_query_limit: i64,
}

fn default_tauri_query_limit() -> i64 {
    1000
}

fn default_tauri_max_query_limit() -> i64 {
    10000
}

impl Default for TauriConfig {
    fn default() -> Self {
        Self {
            default_query_limit: default_tauri_query_limit(),
            max_query_limit: default_tauri_max_query_limit(),
        }
    }
}

// =============================================================================
// EXECUTION CONFIGURATION (combines fee + latency)
// =============================================================================

/// Execution simulation configuration
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ExecutionConfig {
    /// Fee configuration
    #[serde(default)]
    pub fees: FeeConfig,
    /// Latency configuration
    #[serde(default)]
    pub latency: LatencyConfig,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Settings {
    pub database: Database,
    pub cache: Cache,
    pub symbols: Vec<String>,
    pub paper_trading: PaperTrading,
    #[serde(default)]
    pub validation: Validation,
    #[serde(default)]
    pub reconnection_rate_limit: ReconnectionRateLimitConfig,
    #[serde(default)]
    pub alerting: AlertingConfig,
    /// Execution simulation configuration (fees + latency)
    #[serde(default)]
    pub execution: ExecutionConfig,
    /// Market data service configuration
    #[serde(default)]
    pub service: ServiceConfig,
    /// IPC data source configuration
    #[serde(default)]
    pub ipc: IpcConfig,
    /// Metrics server configuration
    #[serde(default)]
    pub metrics: MetricsConfig,
    /// Built-in strategies configuration
    #[serde(default)]
    pub builtin_strategies: BuiltinStrategiesConfig,
    /// Backtest configuration
    #[serde(default)]
    pub backtest: BacktestConfig,
    /// Repository configuration
    #[serde(default)]
    pub repository: RepositoryConfig,
    /// Tauri desktop app configuration
    #[serde(default)]
    pub tauri: TauriConfig,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let run_mode = std::env::var("RUN_MODE").unwrap_or_else(|_| "development".into());

        let mut builder = Config::builder()
            .add_source(File::with_name(&format!("../config/{}", run_mode)).required(true));

        if let Ok(database_url) = std::env::var("DATABASE_URL") {
            builder = builder.set_override("database.url", database_url)?;
        }

        if let Ok(redis_url) = std::env::var("REDIS_URL") {
            builder = builder.set_override("cache.redis.url", redis_url)?;
        }

        let s = builder.build()?;
        s.try_deserialize()
    }
}
