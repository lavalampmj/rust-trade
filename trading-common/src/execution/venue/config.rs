//! Configuration types for execution venues.
//!
//! These types are designed to be deserialized from TOML configuration files.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Base configuration for any execution venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueConfig {
    /// Whether this venue is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Venue type identifier (e.g., "binance_spot", "kraken_spot")
    pub venue_type: String,
    /// REST API configuration
    #[serde(default)]
    pub rest: RestConfig,
    /// WebSocket stream configuration
    #[serde(default)]
    pub stream: StreamConfig,
    /// Rate limiting configuration
    #[serde(default)]
    pub rate_limits: RateLimitConfig,
    /// Authentication configuration
    #[serde(default)]
    pub auth: AuthConfig,
}

fn default_enabled() -> bool {
    true
}

impl Default for VenueConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            venue_type: String::new(),
            rest: RestConfig::default(),
            stream: StreamConfig::default(),
            rate_limits: RateLimitConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

/// REST API configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestConfig {
    /// Base URL for the REST API
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// Request timeout in milliseconds
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// Receive window in milliseconds (for timestamp validation)
    #[serde(default = "default_recv_window_ms")]
    pub recv_window_ms: u64,
    /// Maximum retries for transient failures
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,
}

fn default_base_url() -> String {
    String::new()
}

fn default_timeout_ms() -> u64 {
    10_000
}

fn default_recv_window_ms() -> u64 {
    5_000
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_delay_ms() -> u64 {
    100
}

impl Default for RestConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            timeout_ms: default_timeout_ms(),
            recv_window_ms: default_recv_window_ms(),
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
        }
    }
}

impl RestConfig {
    /// Returns the request timeout as a Duration.
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    /// Returns the retry delay as a Duration.
    pub fn retry_delay(&self) -> Duration {
        Duration::from_millis(self.retry_delay_ms)
    }
}

/// WebSocket stream configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// WebSocket URL
    #[serde(default = "default_ws_url")]
    pub ws_url: String,
    /// Keepalive interval in minutes
    #[serde(default = "default_keepalive_interval_mins")]
    pub keepalive_interval_mins: u64,
    /// Maximum reconnection attempts
    #[serde(default = "default_reconnect_max_attempts")]
    pub reconnect_max_attempts: u32,
    /// Initial reconnection delay in milliseconds
    #[serde(default = "default_reconnect_initial_delay_ms")]
    pub reconnect_initial_delay_ms: u64,
    /// Maximum reconnection delay in milliseconds
    #[serde(default = "default_reconnect_max_delay_ms")]
    pub reconnect_max_delay_ms: u64,
    /// Ping interval in seconds (0 to disable)
    #[serde(default = "default_ping_interval_secs")]
    pub ping_interval_secs: u64,
    /// Pong timeout in seconds
    #[serde(default = "default_pong_timeout_secs")]
    pub pong_timeout_secs: u64,
}

fn default_ws_url() -> String {
    String::new()
}

fn default_keepalive_interval_mins() -> u64 {
    30
}

fn default_reconnect_max_attempts() -> u32 {
    10
}

fn default_reconnect_initial_delay_ms() -> u64 {
    1_000
}

fn default_reconnect_max_delay_ms() -> u64 {
    60_000
}

fn default_ping_interval_secs() -> u64 {
    30
}

fn default_pong_timeout_secs() -> u64 {
    10
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            ws_url: default_ws_url(),
            keepalive_interval_mins: default_keepalive_interval_mins(),
            reconnect_max_attempts: default_reconnect_max_attempts(),
            reconnect_initial_delay_ms: default_reconnect_initial_delay_ms(),
            reconnect_max_delay_ms: default_reconnect_max_delay_ms(),
            ping_interval_secs: default_ping_interval_secs(),
            pong_timeout_secs: default_pong_timeout_secs(),
        }
    }
}

impl StreamConfig {
    /// Returns the keepalive interval as a Duration.
    pub fn keepalive_interval(&self) -> Duration {
        Duration::from_secs(self.keepalive_interval_mins * 60)
    }

    /// Returns the initial reconnection delay as a Duration.
    pub fn reconnect_initial_delay(&self) -> Duration {
        Duration::from_millis(self.reconnect_initial_delay_ms)
    }

    /// Returns the maximum reconnection delay as a Duration.
    pub fn reconnect_max_delay(&self) -> Duration {
        Duration::from_millis(self.reconnect_max_delay_ms)
    }

    /// Returns the ping interval as a Duration.
    pub fn ping_interval(&self) -> Duration {
        Duration::from_secs(self.ping_interval_secs)
    }

    /// Returns the pong timeout as a Duration.
    pub fn pong_timeout(&self) -> Duration {
        Duration::from_secs(self.pong_timeout_secs)
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum orders per second
    #[serde(default = "default_orders_per_second")]
    pub orders_per_second: u32,
    /// Maximum orders per day
    #[serde(default = "default_orders_per_day")]
    pub orders_per_day: u32,
    /// Maximum request weight per minute
    #[serde(default = "default_request_weight_per_minute")]
    pub request_weight_per_minute: u32,
    /// Buffer factor for rate limits (e.g., 0.9 = use only 90% of limit)
    #[serde(default = "default_buffer_factor")]
    pub buffer_factor: f64,
}

fn default_orders_per_second() -> u32 {
    10
}

fn default_orders_per_day() -> u32 {
    200_000
}

fn default_request_weight_per_minute() -> u32 {
    1200
}

fn default_buffer_factor() -> f64 {
    0.9
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            orders_per_second: default_orders_per_second(),
            orders_per_day: default_orders_per_day(),
            request_weight_per_minute: default_request_weight_per_minute(),
            buffer_factor: default_buffer_factor(),
        }
    }
}

impl RateLimitConfig {
    /// Returns the effective orders per second (with buffer).
    pub fn effective_orders_per_second(&self) -> u32 {
        ((self.orders_per_second as f64) * self.buffer_factor) as u32
    }

    /// Returns the effective request weight per minute (with buffer).
    pub fn effective_request_weight_per_minute(&self) -> u32 {
        ((self.request_weight_per_minute as f64) * self.buffer_factor) as u32
    }
}

/// Authentication configuration.
///
/// API keys are loaded from environment variables for security.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    /// Environment variable name for API key
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Environment variable name for API secret
    #[serde(default = "default_api_secret_env")]
    pub api_secret_env: String,
    /// Optional passphrase environment variable (for some venues)
    pub passphrase_env: Option<String>,
}

fn default_api_key_env() -> String {
    "API_KEY".to_string()
}

fn default_api_secret_env() -> String {
    "API_SECRET".to_string()
}

impl AuthConfig {
    /// Create a new auth config with environment variable names.
    pub fn new(api_key_env: impl Into<String>, api_secret_env: impl Into<String>) -> Self {
        Self {
            api_key_env: api_key_env.into(),
            api_secret_env: api_secret_env.into(),
            passphrase_env: None,
        }
    }

    /// Add a passphrase environment variable.
    pub fn with_passphrase(mut self, env: impl Into<String>) -> Self {
        self.passphrase_env = Some(env.into());
        self
    }

    /// Load API key from environment.
    pub fn load_api_key(&self) -> Option<String> {
        std::env::var(&self.api_key_env).ok()
    }

    /// Load API secret from environment.
    pub fn load_api_secret(&self) -> Option<String> {
        std::env::var(&self.api_secret_env).ok()
    }

    /// Load passphrase from environment.
    pub fn load_passphrase(&self) -> Option<String> {
        self.passphrase_env
            .as_ref()
            .and_then(|env| std::env::var(env).ok())
    }

    /// Returns true if API credentials are available in environment.
    pub fn has_credentials(&self) -> bool {
        self.load_api_key().is_some() && self.load_api_secret().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rest_config_defaults() {
        let config = RestConfig::default();
        assert_eq!(config.timeout_ms, 10_000);
        assert_eq!(config.recv_window_ms, 5_000);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_rest_config_duration() {
        let config = RestConfig {
            timeout_ms: 5000,
            ..Default::default()
        };
        assert_eq!(config.timeout(), Duration::from_millis(5000));
    }

    #[test]
    fn test_stream_config_defaults() {
        let config = StreamConfig::default();
        assert_eq!(config.keepalive_interval_mins, 30);
        assert_eq!(config.reconnect_max_attempts, 10);
    }

    #[test]
    fn test_stream_config_duration() {
        let config = StreamConfig {
            keepalive_interval_mins: 15,
            ..Default::default()
        };
        assert_eq!(config.keepalive_interval(), Duration::from_secs(15 * 60));
    }

    #[test]
    fn test_rate_limit_config_buffer() {
        let config = RateLimitConfig {
            orders_per_second: 10,
            buffer_factor: 0.8,
            ..Default::default()
        };
        assert_eq!(config.effective_orders_per_second(), 8);
    }

    #[test]
    fn test_auth_config() {
        let config = AuthConfig::new("TEST_API_KEY", "TEST_API_SECRET")
            .with_passphrase("TEST_PASSPHRASE");
        assert_eq!(config.api_key_env, "TEST_API_KEY");
        assert_eq!(config.api_secret_env, "TEST_API_SECRET");
        assert_eq!(config.passphrase_env, Some("TEST_PASSPHRASE".to_string()));
    }

    #[test]
    fn test_venue_config_deserialization() {
        let toml_str = r#"
            enabled = true
            venue_type = "binance_spot"

            [rest]
            base_url = "https://api.binance.us"
            timeout_ms = 5000

            [stream]
            ws_url = "wss://stream.binance.us:9443/ws"
            keepalive_interval_mins = 20

            [rate_limits]
            orders_per_second = 10

            [auth]
            api_key_env = "BINANCE_API_KEY"
            api_secret_env = "BINANCE_API_SECRET"
        "#;

        let config: VenueConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.venue_type, "binance_spot");
        assert_eq!(config.rest.base_url, "https://api.binance.us");
        assert_eq!(config.rest.timeout_ms, 5000);
        assert_eq!(config.stream.keepalive_interval_mins, 20);
        assert_eq!(config.auth.api_key_env, "BINANCE_API_KEY");
    }
}
