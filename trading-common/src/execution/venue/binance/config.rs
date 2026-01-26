//! Binance-specific configuration types.
//!
//! This module provides configuration types for Binance venues,
//! supporting both Binance.US and Binance.com, and spot/futures markets.

use serde::{Deserialize, Serialize};

use crate::execution::venue::config::VenueConfig;

/// Binance platform identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BinancePlatform {
    /// Binance.com (International)
    #[default]
    Com,
    /// Binance.US
    Us,
}

impl BinancePlatform {
    /// Parse platform from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "com" | "international" | "binance.com" => Some(Self::Com),
            "us" | "binance.us" => Some(Self::Us),
            _ => None,
        }
    }

    /// Get the display name for the platform.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Com => "Binance.com",
            Self::Us => "Binance.US",
        }
    }
}

/// Binance market type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BinanceMarketType {
    /// Spot market
    #[default]
    Spot,
    /// USDT-M Futures (linear perpetual)
    UsdtFutures,
    /// COIN-M Futures (inverse perpetual) - not yet implemented
    CoinFutures,
}

impl BinanceMarketType {
    /// Parse market type from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "spot" => Some(Self::Spot),
            "usdt_futures" | "futures" | "linear" | "usdtm" => Some(Self::UsdtFutures),
            "coin_futures" | "inverse" | "coinm" => Some(Self::CoinFutures),
            _ => None,
        }
    }

    /// Check if this is a futures market.
    pub fn is_futures(&self) -> bool {
        matches!(self, Self::UsdtFutures | Self::CoinFutures)
    }
}

/// Configuration for Binance venues.
///
/// This extends the base VenueConfig with Binance-specific settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceVenueConfig {
    /// Base venue configuration
    #[serde(flatten)]
    pub base: VenueConfig,

    /// Binance platform (com or us)
    #[serde(default)]
    pub platform: BinancePlatform,

    /// Market type (spot or futures)
    #[serde(default)]
    pub market_type: BinanceMarketType,

    /// Whether to use testnet
    #[serde(default)]
    pub testnet: bool,
}

impl Default for BinanceVenueConfig {
    fn default() -> Self {
        Self {
            base: VenueConfig::default(),
            platform: BinancePlatform::default(),
            market_type: BinanceMarketType::default(),
            testnet: false,
        }
    }
}

impl BinanceVenueConfig {
    /// Create a new Binance Spot configuration for Binance.com.
    pub fn spot_com() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "binance_spot".to_string();
        config.platform = BinancePlatform::Com;
        config.market_type = BinanceMarketType::Spot;
        config
    }

    /// Create a new Binance Spot configuration for Binance.US.
    pub fn spot_us() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "binance_spot".to_string();
        config.platform = BinancePlatform::Us;
        config.market_type = BinanceMarketType::Spot;
        config
    }

    /// Create a new Binance USDT-M Futures configuration.
    pub fn usdt_futures() -> Self {
        let mut config = Self::default();
        config.base.venue_type = "binance_futures".to_string();
        config.platform = BinancePlatform::Com;
        config.market_type = BinanceMarketType::UsdtFutures;
        config
    }

    /// Set the testnet flag.
    pub fn with_testnet(mut self, testnet: bool) -> Self {
        self.testnet = testnet;
        self
    }

    /// Get the venue type string.
    pub fn venue_type(&self) -> &str {
        &self.base.venue_type
    }

    /// Check if testnet is enabled.
    pub fn is_testnet(&self) -> bool {
        self.testnet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_from_str() {
        assert_eq!(BinancePlatform::from_str("com"), Some(BinancePlatform::Com));
        assert_eq!(BinancePlatform::from_str("us"), Some(BinancePlatform::Us));
        assert_eq!(
            BinancePlatform::from_str("binance.com"),
            Some(BinancePlatform::Com)
        );
        assert_eq!(BinancePlatform::from_str("invalid"), None);
    }

    #[test]
    fn test_market_type_from_str() {
        assert_eq!(
            BinanceMarketType::from_str("spot"),
            Some(BinanceMarketType::Spot)
        );
        assert_eq!(
            BinanceMarketType::from_str("futures"),
            Some(BinanceMarketType::UsdtFutures)
        );
        assert!(BinanceMarketType::UsdtFutures.is_futures());
        assert!(!BinanceMarketType::Spot.is_futures());
    }

    #[test]
    fn test_config_builders() {
        let spot_com = BinanceVenueConfig::spot_com();
        assert_eq!(spot_com.platform, BinancePlatform::Com);
        assert_eq!(spot_com.market_type, BinanceMarketType::Spot);

        let spot_us = BinanceVenueConfig::spot_us();
        assert_eq!(spot_us.platform, BinancePlatform::Us);

        let futures = BinanceVenueConfig::usdt_futures();
        assert_eq!(futures.market_type, BinanceMarketType::UsdtFutures);
    }

    #[test]
    fn test_deserialization() {
        let toml_str = r#"
            enabled = true
            venue_type = "binance_spot"
            platform = "com"
            market_type = "spot"
            testnet = false

            [rest]
            base_url = "https://api.binance.com"

            [auth]
            api_key_env = "BINANCE_API_KEY"
            api_secret_env = "BINANCE_API_SECRET"
        "#;

        let config: BinanceVenueConfig = toml::from_str(toml_str).unwrap();
        assert!(config.base.enabled);
        assert_eq!(config.platform, BinancePlatform::Com);
        assert_eq!(config.market_type, BinanceMarketType::Spot);
        assert!(!config.testnet);
    }
}
