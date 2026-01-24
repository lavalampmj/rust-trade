//! Provider routing configuration types.
//!
//! This module defines the types for mapping symbols to data providers,
//! supporting a three-tier resolution strategy:
//! 1. Symbol-specific routing (highest priority)
//! 2. Asset type + exchange fallback
//! 3. Global default

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Asset types based on Databento symbology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Futures,
    Equity,
    Fx,
    Crypto,
    EquityOptions,
    IndexOptions,
}

impl AssetType {
    /// Convert asset type to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetType::Futures => "futures",
            AssetType::Equity => "equity",
            AssetType::Fx => "fx",
            AssetType::Crypto => "crypto",
            AssetType::EquityOptions => "equity_options",
            AssetType::IndexOptions => "index_options",
        }
    }
}

impl FromStr for AssetType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "futures" | "future" | "fut" => Ok(AssetType::Futures),
            "equity" | "equities" | "stock" | "stocks" => Ok(AssetType::Equity),
            "fx" | "forex" | "currency" => Ok(AssetType::Fx),
            "crypto" | "cryptocurrency" => Ok(AssetType::Crypto),
            "equity_options" | "equity-options" | "stock_options" => Ok(AssetType::EquityOptions),
            "index_options" | "index-options" => Ok(AssetType::IndexOptions),
            _ => Err(format!("Unknown asset type: {}", s)),
        }
    }
}

impl fmt::Display for AssetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Feed type for routing decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedType {
    Historical,
    Realtime,
}

impl fmt::Display for FeedType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FeedType::Historical => write!(f, "historical"),
            FeedType::Realtime => write!(f, "realtime"),
        }
    }
}

/// Provider route configuration for a specific feed type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRoute {
    /// Provider name (e.g., "databento", "binance").
    pub provider: String,
    /// Optional dataset identifier (e.g., "GLBX.MDP3" for Databento).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<String>,
}

impl ProviderRoute {
    /// Create a new provider route.
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            dataset: None,
        }
    }

    /// Create a provider route with a dataset.
    pub fn with_dataset(provider: impl Into<String>, dataset: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            dataset: Some(dataset.into()),
        }
    }
}

/// Symbol-specific routing configuration (highest priority).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRouting {
    /// Symbol identifier (e.g., "ES", "AAPL").
    pub symbol: String,
    /// Exchange identifier (e.g., "CME", "NASDAQ").
    pub exchange: String,
    /// Asset type for this symbol.
    pub asset_type: AssetType,
    /// Historical data provider route.
    pub historical: ProviderRoute,
    /// Realtime data provider route.
    pub realtime: ProviderRoute,
    /// Provider-specific symbol mappings (provider name -> provider symbol).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub provider_symbols: HashMap<String, String>,
}

impl SymbolRouting {
    /// Create a new symbol routing configuration.
    pub fn new(
        symbol: impl Into<String>,
        exchange: impl Into<String>,
        asset_type: AssetType,
        historical: ProviderRoute,
        realtime: ProviderRoute,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: exchange.into(),
            asset_type,
            historical,
            realtime,
            provider_symbols: HashMap::new(),
        }
    }

    /// Add a provider-specific symbol mapping.
    pub fn with_provider_symbol(
        mut self,
        provider: impl Into<String>,
        provider_symbol: impl Into<String>,
    ) -> Self {
        self.provider_symbols
            .insert(provider.into(), provider_symbol.into());
        self
    }

    /// Get the provider-specific symbol, or fall back to the base symbol.
    pub fn symbol_for_provider(&self, provider: &str) -> &str {
        self.provider_symbols
            .get(provider)
            .map(String::as_str)
            .unwrap_or(&self.symbol)
    }

    /// Get the route for a specific feed type.
    pub fn route_for_feed(&self, feed_type: FeedType) -> &ProviderRoute {
        match feed_type {
            FeedType::Historical => &self.historical,
            FeedType::Realtime => &self.realtime,
        }
    }
}

/// Asset type routing configuration (fallback tier).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetTypeRouting {
    /// Historical data provider name.
    #[serde(default = "default_provider")]
    pub historical: String,
    /// Realtime data provider name.
    #[serde(default = "default_provider")]
    pub realtime: String,
    /// Default exchange for this asset type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_exchange: Option<String>,
}

fn default_provider() -> String {
    "databento".to_string()
}

impl Default for AssetTypeRouting {
    fn default() -> Self {
        Self {
            historical: default_provider(),
            realtime: default_provider(),
            default_exchange: None,
        }
    }
}

impl AssetTypeRouting {
    /// Create a new asset type routing configuration.
    pub fn new(historical: impl Into<String>, realtime: impl Into<String>) -> Self {
        Self {
            historical: historical.into(),
            realtime: realtime.into(),
            default_exchange: None,
        }
    }

    /// Set the default exchange for this asset type.
    pub fn with_default_exchange(mut self, exchange: impl Into<String>) -> Self {
        self.default_exchange = Some(exchange.into());
        self
    }

    /// Get the provider for a specific feed type.
    pub fn provider_for_feed(&self, feed_type: FeedType) -> &str {
        match feed_type {
            FeedType::Historical => &self.historical,
            FeedType::Realtime => &self.realtime,
        }
    }
}

/// Databento-specific routing configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabentoRouting {
    /// Dataset mappings: "asset_type@exchange" -> dataset name.
    #[serde(default)]
    pub datasets: HashMap<String, String>,
    /// Default dataset when no specific mapping exists.
    #[serde(default = "default_databento_dataset")]
    pub default_dataset: String,
}

fn default_databento_dataset() -> String {
    "GLBX.MDP3".to_string()
}

impl Default for DatabentoRouting {
    fn default() -> Self {
        let mut datasets = HashMap::new();
        // Default Databento dataset mappings
        datasets.insert("futures@CME".to_string(), "GLBX.MDP3".to_string());
        datasets.insert("futures@CBOT".to_string(), "GLBX.MDP3".to_string());
        datasets.insert("futures@NYMEX".to_string(), "GLBX.MDP3".to_string());
        datasets.insert("futures@COMEX".to_string(), "GLBX.MDP3".to_string());
        datasets.insert("equity@NASDAQ".to_string(), "XNAS.ITCH".to_string());
        datasets.insert("equity@NYSE".to_string(), "XNYS.TRADES".to_string());

        Self {
            datasets,
            default_dataset: default_databento_dataset(),
        }
    }
}

impl DatabentoRouting {
    /// Get the dataset for a given asset type and exchange.
    pub fn get_dataset(&self, asset_type: AssetType, exchange: &str) -> &str {
        let key = format!("{}@{}", asset_type.as_str(), exchange.to_uppercase());
        self.datasets
            .get(&key)
            .map(String::as_str)
            .unwrap_or(&self.default_dataset)
    }

    /// Add a dataset mapping.
    pub fn add_dataset(
        mut self,
        asset_type: AssetType,
        exchange: impl Into<String>,
        dataset: impl Into<String>,
    ) -> Self {
        let key = format!("{}@{}", asset_type.as_str(), exchange.into().to_uppercase());
        self.datasets.insert(key, dataset.into());
        self
    }
}

/// Main routing configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Default provider when no routing rules match.
    #[serde(default = "default_provider")]
    pub default_provider: String,

    /// Databento-specific routing configuration.
    #[serde(default)]
    pub databento: DatabentoRouting,

    /// Asset type routing rules (fallback tier).
    #[serde(default)]
    pub asset_types: HashMap<String, AssetTypeRouting>,

    /// Symbol-specific routing rules (highest priority).
    #[serde(default)]
    pub symbols: Vec<SymbolRouting>,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        let mut asset_types = HashMap::new();

        // Default asset type routings
        asset_types.insert(
            "futures".to_string(),
            AssetTypeRouting::new("databento", "databento").with_default_exchange("CME"),
        );
        asset_types.insert(
            "equity".to_string(),
            AssetTypeRouting::new("databento", "databento").with_default_exchange("NASDAQ"),
        );
        asset_types.insert(
            "crypto".to_string(),
            AssetTypeRouting::new("databento", "binance").with_default_exchange("BINANCE"),
        );
        asset_types.insert(
            "fx".to_string(),
            AssetTypeRouting::new("databento", "databento"),
        );

        Self {
            default_provider: default_provider(),
            databento: DatabentoRouting::default(),
            asset_types,
            symbols: Vec::new(),
        }
    }
}

impl RoutingConfig {
    /// Find symbol-specific routing for a given symbol and exchange.
    pub fn find_symbol_routing(&self, symbol: &str, exchange: &str) -> Option<&SymbolRouting> {
        self.symbols.iter().find(|r| {
            r.symbol.eq_ignore_ascii_case(symbol) && r.exchange.eq_ignore_ascii_case(exchange)
        })
    }

    /// Get asset type routing configuration.
    pub fn get_asset_type_routing(&self, asset_type: AssetType) -> Option<&AssetTypeRouting> {
        self.asset_types.get(asset_type.as_str())
    }

    /// Add a symbol-specific routing rule.
    pub fn add_symbol_routing(mut self, routing: SymbolRouting) -> Self {
        self.symbols.push(routing);
        self
    }

    /// Add an asset type routing rule.
    pub fn add_asset_type_routing(
        mut self,
        asset_type: AssetType,
        routing: AssetTypeRouting,
    ) -> Self {
        self.asset_types
            .insert(asset_type.as_str().to_string(), routing);
        self
    }
}

/// Source of route resolution for debugging/logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteResolutionSource {
    /// Resolved from symbol-specific routing.
    SymbolSpecific,
    /// Resolved from asset type + exchange fallback.
    AssetTypeExchange,
    /// Resolved from global default.
    GlobalDefault,
}

impl fmt::Display for RouteResolutionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteResolutionSource::SymbolSpecific => write!(f, "symbol_specific"),
            RouteResolutionSource::AssetTypeExchange => write!(f, "asset_type_exchange"),
            RouteResolutionSource::GlobalDefault => write!(f, "global_default"),
        }
    }
}

/// Resolved route result containing all information needed to fetch data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoute {
    /// Provider name to use.
    pub provider: String,
    /// Dataset identifier (if applicable).
    pub dataset: Option<String>,
    /// Provider-specific symbol to use.
    pub provider_symbol: String,
    /// How this route was resolved.
    pub resolution_source: RouteResolutionSource,
}

impl ResolvedRoute {
    /// Create a new resolved route.
    pub fn new(
        provider: impl Into<String>,
        dataset: Option<String>,
        provider_symbol: impl Into<String>,
        resolution_source: RouteResolutionSource,
    ) -> Self {
        Self {
            provider: provider.into(),
            dataset,
            provider_symbol: provider_symbol.into(),
            resolution_source,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_type_from_str() {
        assert_eq!(AssetType::from_str("futures").unwrap(), AssetType::Futures);
        assert_eq!(AssetType::from_str("FUTURES").unwrap(), AssetType::Futures);
        assert_eq!(AssetType::from_str("fut").unwrap(), AssetType::Futures);
        assert_eq!(AssetType::from_str("equity").unwrap(), AssetType::Equity);
        assert_eq!(AssetType::from_str("stock").unwrap(), AssetType::Equity);
        assert_eq!(AssetType::from_str("crypto").unwrap(), AssetType::Crypto);
        assert_eq!(AssetType::from_str("fx").unwrap(), AssetType::Fx);
        assert_eq!(
            AssetType::from_str("equity_options").unwrap(),
            AssetType::EquityOptions
        );
        assert!(AssetType::from_str("unknown").is_err());
    }

    #[test]
    fn test_asset_type_as_str() {
        assert_eq!(AssetType::Futures.as_str(), "futures");
        assert_eq!(AssetType::Equity.as_str(), "equity");
        assert_eq!(AssetType::Crypto.as_str(), "crypto");
    }

    #[test]
    fn test_provider_route() {
        let route = ProviderRoute::new("databento");
        assert_eq!(route.provider, "databento");
        assert!(route.dataset.is_none());

        let route_with_dataset = ProviderRoute::with_dataset("databento", "GLBX.MDP3");
        assert_eq!(route_with_dataset.provider, "databento");
        assert_eq!(route_with_dataset.dataset, Some("GLBX.MDP3".to_string()));
    }

    #[test]
    fn test_symbol_routing() {
        let routing = SymbolRouting::new(
            "ES",
            "CME",
            AssetType::Futures,
            ProviderRoute::with_dataset("databento", "GLBX.MDP3"),
            ProviderRoute::with_dataset("databento", "GLBX.MDP3"),
        )
        .with_provider_symbol("databento", "ES.FUT");

        assert_eq!(routing.symbol, "ES");
        assert_eq!(routing.exchange, "CME");
        assert_eq!(routing.asset_type, AssetType::Futures);
        assert_eq!(routing.symbol_for_provider("databento"), "ES.FUT");
        assert_eq!(routing.symbol_for_provider("unknown"), "ES");
    }

    #[test]
    fn test_databento_routing() {
        let routing = DatabentoRouting::default();
        assert_eq!(routing.get_dataset(AssetType::Futures, "CME"), "GLBX.MDP3");
        assert_eq!(
            routing.get_dataset(AssetType::Equity, "NASDAQ"),
            "XNAS.ITCH"
        );
        assert_eq!(
            routing.get_dataset(AssetType::Equity, "NYSE"),
            "XNYS.TRADES"
        );
        // Unknown exchange falls back to default
        assert_eq!(
            routing.get_dataset(AssetType::Futures, "UNKNOWN"),
            "GLBX.MDP3"
        );
    }

    #[test]
    fn test_routing_config_find_symbol() {
        let config = RoutingConfig::default().add_symbol_routing(SymbolRouting::new(
            "ES",
            "CME",
            AssetType::Futures,
            ProviderRoute::with_dataset("databento", "GLBX.MDP3"),
            ProviderRoute::with_dataset("databento", "GLBX.MDP3"),
        ));

        assert!(config.find_symbol_routing("ES", "CME").is_some());
        assert!(config.find_symbol_routing("es", "cme").is_some()); // case insensitive
        assert!(config.find_symbol_routing("NQ", "CME").is_none());
    }

    #[test]
    fn test_routing_config_default() {
        let config = RoutingConfig::default();
        assert_eq!(config.default_provider, "databento");
        assert!(config.get_asset_type_routing(AssetType::Futures).is_some());
        assert!(config.get_asset_type_routing(AssetType::Equity).is_some());
        assert!(config.get_asset_type_routing(AssetType::Crypto).is_some());
    }
}
