//! Configuration for the VenueRouter.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Configuration for VenueRouter behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueRouterConfig {
    /// Default venue to use when order has no venue specified (or "DEFAULT")
    #[serde(default)]
    pub default_venue: Option<String>,

    /// Routing rules for specific symbols
    #[serde(default)]
    pub route_rules: HashMap<String, RouteRule>,

    /// Whether to require all venues connected before accepting orders
    #[serde(default)]
    pub require_all_connected: bool,

    /// Connection timeout per venue
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,

    /// Whether to automatically reconnect disconnected venues
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,

    /// Delay between reconnection attempts
    #[serde(default = "default_reconnect_delay_ms")]
    pub reconnect_delay_ms: u64,

    /// Maximum reconnection attempts before giving up
    #[serde(default = "default_max_reconnect_attempts")]
    pub max_reconnect_attempts: u32,

    /// Whether to aggregate execution streams from all venues
    #[serde(default = "default_aggregate_streams")]
    pub aggregate_execution_streams: bool,

    /// Timeout for order operations (submit, cancel, modify)
    #[serde(default = "default_order_timeout_ms")]
    pub order_timeout_ms: u64,
}

fn default_connect_timeout_ms() -> u64 {
    30000
}
fn default_auto_reconnect() -> bool {
    true
}
fn default_reconnect_delay_ms() -> u64 {
    5000
}
fn default_max_reconnect_attempts() -> u32 {
    10
}
fn default_aggregate_streams() -> bool {
    true
}
fn default_order_timeout_ms() -> u64 {
    10000
}

impl Default for VenueRouterConfig {
    fn default() -> Self {
        Self {
            default_venue: None,
            route_rules: HashMap::new(),
            require_all_connected: false,
            connect_timeout_ms: default_connect_timeout_ms(),
            auto_reconnect: default_auto_reconnect(),
            reconnect_delay_ms: default_reconnect_delay_ms(),
            max_reconnect_attempts: default_max_reconnect_attempts(),
            aggregate_execution_streams: default_aggregate_streams(),
            order_timeout_ms: default_order_timeout_ms(),
        }
    }
}

impl VenueRouterConfig {
    /// Create a new config with default venue.
    pub fn with_default_venue(venue: impl Into<String>) -> Self {
        Self {
            default_venue: Some(venue.into()),
            ..Default::default()
        }
    }

    /// Set the default venue.
    pub fn default_venue(mut self, venue: impl Into<String>) -> Self {
        self.default_venue = Some(venue.into());
        self
    }

    /// Add a routing rule for a symbol.
    pub fn add_route_rule(mut self, symbol: impl Into<String>, rule: RouteRule) -> Self {
        self.route_rules.insert(symbol.into(), rule);
        self
    }

    /// Require all venues to be connected.
    pub fn require_all_connected(mut self) -> Self {
        self.require_all_connected = true;
        self
    }

    /// Disable automatic reconnection.
    pub fn disable_auto_reconnect(mut self) -> Self {
        self.auto_reconnect = false;
        self
    }

    /// Set connection timeout.
    pub fn connect_timeout(mut self, duration: Duration) -> Self {
        self.connect_timeout_ms = duration.as_millis() as u64;
        self
    }

    /// Set order operation timeout.
    pub fn order_timeout(mut self, duration: Duration) -> Self {
        self.order_timeout_ms = duration.as_millis() as u64;
        self
    }

    /// Get connection timeout as Duration.
    pub fn connect_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.connect_timeout_ms)
    }

    /// Get reconnect delay as Duration.
    pub fn reconnect_delay_duration(&self) -> Duration {
        Duration::from_millis(self.reconnect_delay_ms)
    }

    /// Get order timeout as Duration.
    pub fn order_timeout_duration(&self) -> Duration {
        Duration::from_millis(self.order_timeout_ms)
    }
}

/// Routing rule for a specific symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRule {
    /// Primary venue for this symbol
    pub primary_venue: String,

    /// Fallback venues in order of preference
    #[serde(default)]
    pub fallback_venues: Vec<String>,

    /// Routing strategy
    #[serde(default)]
    pub strategy: RouteStrategy,
}

impl RouteRule {
    /// Create a new route rule with primary venue.
    pub fn new(primary: impl Into<String>) -> Self {
        Self {
            primary_venue: primary.into(),
            fallback_venues: Vec::new(),
            strategy: RouteStrategy::default(),
        }
    }

    /// Add a fallback venue.
    pub fn with_fallback(mut self, venue: impl Into<String>) -> Self {
        self.fallback_venues.push(venue.into());
        self
    }

    /// Set routing strategy.
    pub fn with_strategy(mut self, strategy: RouteStrategy) -> Self {
        self.strategy = strategy;
        self
    }
}

/// Strategy for choosing venue when multiple are available.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum RouteStrategy {
    /// Always use primary venue (fallback only if primary unavailable)
    #[default]
    PrimaryOnly,

    /// Round-robin between primary and fallbacks
    RoundRobin,

    /// Choose venue with lowest fees (requires fee info)
    LowestFee,

    /// Choose venue with best liquidity (requires order book)
    BestLiquidity,

    /// Choose venue with lowest latency (requires latency tracking)
    LowestLatency,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = VenueRouterConfig::default();
        assert!(config.default_venue.is_none());
        assert!(config.route_rules.is_empty());
        assert!(!config.require_all_connected);
        assert!(config.auto_reconnect);
    }

    #[test]
    fn test_builder_pattern() {
        let config = VenueRouterConfig::with_default_venue("BINANCE")
            .add_route_rule(
                "BTCUSDT",
                RouteRule::new("BINANCE").with_fallback("KRAKEN"),
            )
            .require_all_connected()
            .connect_timeout(Duration::from_secs(60));

        assert_eq!(config.default_venue, Some("BINANCE".to_string()));
        assert!(config.route_rules.contains_key("BTCUSDT"));
        assert!(config.require_all_connected);
        assert_eq!(config.connect_timeout_ms, 60000);
    }

    #[test]
    fn test_route_rule() {
        let rule = RouteRule::new("BINANCE")
            .with_fallback("KRAKEN")
            .with_fallback("COINBASE")
            .with_strategy(RouteStrategy::RoundRobin);

        assert_eq!(rule.primary_venue, "BINANCE");
        assert_eq!(rule.fallback_venues.len(), 2);
        assert_eq!(rule.strategy, RouteStrategy::RoundRobin);
    }
}
