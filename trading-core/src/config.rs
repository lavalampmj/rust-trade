use config::{Config, ConfigError, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Database {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub max_lifetime: u64,
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
    pub fn to_rate_limiter_config(&self) -> crate::exchange::rate_limiter::ReconnectionRateLimiterConfig {
        use crate::exchange::rate_limiter::{ReconnectionRateLimiterConfig as RLConfig, ReconnectionWindow};
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

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub database: Database,
    pub cache: Cache,
    pub symbols: Vec<String>,
    pub paper_trading: PaperTrading,
    #[serde(default)]
    pub validation: Validation,
    #[serde(default)]
    pub reconnection_rate_limit: ReconnectionRateLimitConfig,
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
