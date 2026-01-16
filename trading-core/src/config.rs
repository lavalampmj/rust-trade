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

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub database: Database,
    pub cache: Cache,
    pub symbols: Vec<String>,
    pub paper_trading: PaperTrading,
    #[serde(default)]
    pub validation: Validation,
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
