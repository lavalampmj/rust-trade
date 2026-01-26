//! Kraken-specific configuration types.
//!
//! This module provides configuration types for Kraken venues,
//! supporting production and sandbox environments.

use serde::{Deserialize, Serialize};

use crate::execution::venue::config::VenueConfig;

/// Kraken rate limit tier.
///
/// Higher tiers have higher rate limits and faster decay rates.
/// See: <https://docs.kraken.com/api/docs/guides/rate-limits>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KrakenTier {
    /// Starter tier: 60 threshold, -1/sec decay
    #[default]
    Starter,
    /// Intermediate tier: 125 threshold, -2.34/sec decay
    Intermediate,
    /// Pro tier: 180 threshold, -3.75/sec decay
    Pro,
}

impl KrakenTier {
    /// Get the rate limit threshold for this tier.
    pub fn threshold(&self) -> u32 {
        match self {
            Self::Starter => 60,
            Self::Intermediate => 125,
            Self::Pro => 180,
        }
    }

    /// Get the decay rate per second for this tier.
    pub fn decay_rate(&self) -> f64 {
        match self {
            Self::Starter => 1.0,
            Self::Intermediate => 2.34,
            Self::Pro => 3.75,
        }
    }

    /// Parse tier from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "starter" => Some(Self::Starter),
            "intermediate" => Some(Self::Intermediate),
            "pro" => Some(Self::Pro),
            _ => None,
        }
    }

    /// Get the display name for the tier.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Starter => "Starter",
            Self::Intermediate => "Intermediate",
            Self::Pro => "Pro",
        }
    }
}

/// Configuration for Kraken venues.
///
/// This extends the base VenueConfig with Kraken-specific settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenVenueConfig {
    /// Base venue configuration
    #[serde(flatten)]
    pub base: VenueConfig,

    /// Whether to use sandbox/demo environment
    #[serde(default)]
    pub sandbox: bool,

    /// Rate limit tier
    #[serde(default)]
    pub tier: KrakenTier,
}

impl Default for KrakenVenueConfig {
    fn default() -> Self {
        Self {
            base: VenueConfig::default(),
            sandbox: false,
            tier: KrakenTier::default(),
        }
    }
}

impl KrakenVenueConfig {
    /// Create a new Kraken Spot production configuration.
    pub fn production() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "kraken_spot".to_string();
        config.base.auth.api_key_env = "KRAKEN_API_KEY".to_string();
        config.base.auth.api_secret_env = "KRAKEN_API_SECRET".to_string();
        config
    }

    /// Create a new Kraken Spot sandbox configuration.
    ///
    /// **WARNING**: Kraken Spot does NOT have a public sandbox like Binance.
    ///
    /// This "sandbox" mode uses PRODUCTION endpoints but is intended for:
    /// - Testing with minimal order sizes
    /// - Using limit orders far from market price
    /// - Verifying API connectivity and authentication
    ///
    /// **Orders placed will execute on REAL production markets!**
    ///
    /// For integration testing, use:
    /// - Small quantities (minimum allowed)
    /// - Limit prices far from market (won't fill)
    /// - GTC orders that can be canceled
    pub fn sandbox() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "kraken_spot".to_string();
        // Use separate env vars so production keys aren't accidentally used for testing
        config.base.auth.api_key_env = "KRAKEN_SANDBOX_API_KEY".to_string();
        config.base.auth.api_secret_env = "KRAKEN_SANDBOX_API_SECRET".to_string();
        config.sandbox = true;
        config
    }

    /// Set the sandbox flag.
    pub fn with_sandbox(mut self, sandbox: bool) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// Set the rate limit tier.
    pub fn with_tier(mut self, tier: KrakenTier) -> Self {
        self.tier = tier;
        self
    }

    /// Get the venue type string.
    pub fn venue_type(&self) -> &str {
        &self.base.venue_type
    }

    /// Check if sandbox is enabled.
    pub fn is_sandbox(&self) -> bool {
        self.sandbox
    }
}

// =============================================================================
// KRAKEN FUTURES CONFIGURATION
// =============================================================================

/// Configuration for Kraken Futures venues.
///
/// This extends the base VenueConfig with Kraken Futures-specific settings.
/// Unlike Spot, Futures has a fully functional demo environment!
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KrakenFuturesVenueConfig {
    /// Base venue configuration
    #[serde(flatten)]
    pub base: VenueConfig,

    /// Whether to use demo environment
    #[serde(default)]
    pub demo: bool,
}

impl Default for KrakenFuturesVenueConfig {
    fn default() -> Self {
        Self {
            base: VenueConfig::default(),
            demo: false,
        }
    }
}

impl KrakenFuturesVenueConfig {
    /// Create a new Kraken Futures production configuration.
    pub fn production() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "kraken_futures".to_string();
        config.base.auth.api_key_env = "KRAKEN_FUTURES_API_KEY".to_string();
        config.base.auth.api_secret_env = "KRAKEN_FUTURES_API_SECRET".to_string();
        config
    }

    /// Create a new Kraken Futures demo configuration.
    ///
    /// **Unlike Spot, Futures HAS a public demo environment!**
    ///
    /// Demo URLs:
    /// - REST: `https://demo-futures.kraken.com`
    /// - WebSocket: `wss://demo-futures.kraken.com/ws/v1`
    ///
    /// Perfect for:
    /// - Testing order flows safely
    /// - Integration testing
    /// - Learning the API
    pub fn demo() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "kraken_futures".to_string();
        config.base.auth.api_key_env = "KRAKEN_FUTURES_DEMO_API_KEY".to_string();
        config.base.auth.api_secret_env = "KRAKEN_FUTURES_DEMO_API_SECRET".to_string();
        config.demo = true;
        config
    }

    /// Set the demo flag.
    pub fn with_demo(mut self, demo: bool) -> Self {
        self.demo = demo;
        self
    }

    /// Get the venue type string.
    pub fn venue_type(&self) -> &str {
        &self.base.venue_type
    }

    /// Check if demo mode is enabled.
    pub fn is_demo(&self) -> bool {
        self.demo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_threshold() {
        assert_eq!(KrakenTier::Starter.threshold(), 60);
        assert_eq!(KrakenTier::Intermediate.threshold(), 125);
        assert_eq!(KrakenTier::Pro.threshold(), 180);
    }

    #[test]
    fn test_tier_decay() {
        assert_eq!(KrakenTier::Starter.decay_rate(), 1.0);
        assert!((KrakenTier::Intermediate.decay_rate() - 2.34).abs() < 0.01);
        assert!((KrakenTier::Pro.decay_rate() - 3.75).abs() < 0.01);
    }

    #[test]
    fn test_tier_from_str() {
        assert_eq!(KrakenTier::from_str("starter"), Some(KrakenTier::Starter));
        assert_eq!(
            KrakenTier::from_str("intermediate"),
            Some(KrakenTier::Intermediate)
        );
        assert_eq!(KrakenTier::from_str("pro"), Some(KrakenTier::Pro));
        assert_eq!(KrakenTier::from_str("invalid"), None);
    }

    #[test]
    fn test_config_production() {
        let config = KrakenVenueConfig::production();
        assert_eq!(config.base.venue_type, "kraken_spot");
        assert!(!config.sandbox);
        assert_eq!(config.base.auth.api_key_env, "KRAKEN_API_KEY");
    }

    #[test]
    fn test_config_sandbox() {
        let config = KrakenVenueConfig::sandbox();
        assert!(config.sandbox);
        assert_eq!(config.base.auth.api_key_env, "KRAKEN_SANDBOX_API_KEY");
    }

    #[test]
    fn test_config_builders() {
        let config = KrakenVenueConfig::production()
            .with_sandbox(true)
            .with_tier(KrakenTier::Pro);

        assert!(config.sandbox);
        assert_eq!(config.tier, KrakenTier::Pro);
    }

    #[test]
    fn test_deserialization() {
        let toml_str = r#"
            enabled = true
            venue_type = "kraken_spot"
            sandbox = false
            tier = "intermediate"

            [rest]
            base_url = "https://api.kraken.com"

            [auth]
            api_key_env = "KRAKEN_API_KEY"
            api_secret_env = "KRAKEN_API_SECRET"
        "#;

        let config: KrakenVenueConfig = toml::from_str(toml_str).unwrap();
        assert!(config.base.enabled);
        assert!(!config.sandbox);
        assert_eq!(config.tier, KrakenTier::Intermediate);
    }

    // =========================================================================
    // FUTURES CONFIG TESTS
    // =========================================================================

    #[test]
    fn test_futures_config_production() {
        let config = KrakenFuturesVenueConfig::production();
        assert_eq!(config.base.venue_type, "kraken_futures");
        assert!(!config.demo);
        assert_eq!(config.base.auth.api_key_env, "KRAKEN_FUTURES_API_KEY");
    }

    #[test]
    fn test_futures_config_demo() {
        let config = KrakenFuturesVenueConfig::demo();
        assert_eq!(config.base.venue_type, "kraken_futures");
        assert!(config.demo);
        assert_eq!(config.base.auth.api_key_env, "KRAKEN_FUTURES_DEMO_API_KEY");
    }

    #[test]
    fn test_futures_config_builders() {
        let config = KrakenFuturesVenueConfig::production().with_demo(true);
        assert!(config.demo);
    }

    #[test]
    fn test_futures_deserialization() {
        let toml_str = r#"
            enabled = true
            venue_type = "kraken_futures"
            demo = true

            [rest]
            base_url = "https://demo-futures.kraken.com"

            [auth]
            api_key_env = "KRAKEN_FUTURES_DEMO_API_KEY"
            api_secret_env = "KRAKEN_FUTURES_DEMO_API_SECRET"
        "#;

        let config: KrakenFuturesVenueConfig = toml::from_str(toml_str).unwrap();
        assert!(config.base.enabled);
        assert!(config.demo);
        assert_eq!(config.base.venue_type, "kraken_futures");
    }
}
