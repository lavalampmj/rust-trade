//! Provider routing implementation.
//!
//! This module provides the `ProviderRouter` that resolves symbols to providers
//! using a three-tier resolution strategy:
//! 1. Symbol-specific routing (highest priority)
//! 2. Asset type + exchange fallback
//! 3. Global default

use crate::config::routing::{
    AssetType, FeedType, ResolvedRoute, RouteResolutionSource, RoutingConfig,
};
use crate::symbol::SymbolSpec;
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Errors that can occur during route resolution.
#[derive(Debug, Error)]
pub enum RoutingError {
    #[error("No routing found for symbol {symbol}@{exchange}")]
    NoRouteFound { symbol: String, exchange: String },

    #[error("Invalid routing configuration: {0}")]
    InvalidConfig(String),

    #[error("Lock poisoned: {0}")]
    LockPoisoned(String),
}

/// Result type for routing operations.
pub type RoutingResult<T> = Result<T, RoutingError>;

/// Trait for provider routing resolution.
pub trait ProviderRouting: Send + Sync {
    /// Resolve routing for a symbol with a specific feed type.
    fn resolve(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
        feed_type: FeedType,
    ) -> RoutingResult<ResolvedRoute>;

    /// Resolve historical data routing for a symbol.
    fn resolve_historical(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> RoutingResult<ResolvedRoute> {
        self.resolve(symbol, asset_type, FeedType::Historical)
    }

    /// Resolve realtime data routing for a symbol.
    fn resolve_realtime(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> RoutingResult<ResolvedRoute> {
        self.resolve(symbol, asset_type, FeedType::Realtime)
    }

    /// Get the Databento dataset for a symbol.
    fn get_databento_dataset(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> Option<String>;
}

/// Provider router implementation using the three-tier resolution strategy.
#[derive(Debug, Clone)]
pub struct ProviderRouter {
    config: RoutingConfig,
}

impl ProviderRouter {
    /// Create a new provider router with the given configuration.
    pub fn new(config: RoutingConfig) -> Self {
        Self { config }
    }

    /// Create a provider router with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(RoutingConfig::default())
    }

    /// Get the underlying routing configuration.
    pub fn config(&self) -> &RoutingConfig {
        &self.config
    }

    /// Update the routing configuration.
    pub fn set_config(&mut self, config: RoutingConfig) {
        self.config = config;
    }

    /// Resolve using the three-tier strategy.
    fn resolve_internal(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
        feed_type: FeedType,
    ) -> ResolvedRoute {
        // Tier 1: Symbol-specific routing
        if let Some(symbol_routing) = self
            .config
            .find_symbol_routing(&symbol.symbol, &symbol.exchange)
        {
            let route = symbol_routing.route_for_feed(feed_type);
            let provider_symbol = symbol_routing.symbol_for_provider(&route.provider);

            return ResolvedRoute::new(
                &route.provider,
                route.dataset.clone(),
                provider_symbol,
                RouteResolutionSource::SymbolSpecific,
            );
        }

        // Tier 2: Asset type + exchange fallback
        if let Some(asset_type) = asset_type {
            if let Some(asset_routing) = self.config.get_asset_type_routing(asset_type) {
                let provider = asset_routing.provider_for_feed(feed_type);

                // Get dataset if using databento
                let dataset = if provider == "databento" {
                    Some(
                        self.config
                            .databento
                            .get_dataset(asset_type, &symbol.exchange)
                            .to_string(),
                    )
                } else {
                    None
                };

                // Use provider-specific symbol if available, otherwise use base symbol
                let provider_symbol = symbol.symbol_for_provider(provider);

                return ResolvedRoute::new(
                    provider,
                    dataset,
                    provider_symbol,
                    RouteResolutionSource::AssetTypeExchange,
                );
            }
        }

        // Tier 3: Global default
        let provider = &self.config.default_provider;
        let dataset = if provider == "databento" {
            Some(self.config.databento.default_dataset.clone())
        } else {
            None
        };

        // Use provider-specific symbol if available, otherwise use base symbol
        let provider_symbol = symbol.symbol_for_provider(provider);

        ResolvedRoute::new(
            provider,
            dataset,
            provider_symbol,
            RouteResolutionSource::GlobalDefault,
        )
    }
}

impl ProviderRouting for ProviderRouter {
    fn resolve(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
        feed_type: FeedType,
    ) -> RoutingResult<ResolvedRoute> {
        Ok(self.resolve_internal(symbol, asset_type, feed_type))
    }

    fn get_databento_dataset(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> Option<String> {
        // Check symbol-specific routing first
        if let Some(symbol_routing) = self
            .config
            .find_symbol_routing(&symbol.symbol, &symbol.exchange)
        {
            // Use historical route for dataset (they should be the same for Databento)
            return symbol_routing.historical.dataset.clone();
        }

        // Asset type fallback
        if let Some(asset_type) = asset_type {
            return Some(
                self.config
                    .databento
                    .get_dataset(asset_type, &symbol.exchange)
                    .to_string(),
            );
        }

        // Global default
        Some(self.config.databento.default_dataset.clone())
    }
}

/// Thread-safe wrapper for ProviderRouter that allows runtime updates.
#[derive(Debug, Clone)]
pub struct SharedProviderRouter {
    inner: Arc<RwLock<ProviderRouter>>,
}

impl SharedProviderRouter {
    /// Create a new shared provider router with the given configuration.
    pub fn new(config: RoutingConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ProviderRouter::new(config))),
        }
    }

    /// Create a shared provider router with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(RoutingConfig::default())
    }

    /// Update the routing configuration.
    pub fn update_config(&self, config: RoutingConfig) -> RoutingResult<()> {
        let mut router = self
            .inner
            .write()
            .map_err(|e| RoutingError::LockPoisoned(e.to_string()))?;
        router.set_config(config);
        Ok(())
    }

    /// Get a clone of the current routing configuration.
    pub fn config(&self) -> RoutingResult<RoutingConfig> {
        let router = self
            .inner
            .read()
            .map_err(|e| RoutingError::LockPoisoned(e.to_string()))?;
        Ok(router.config().clone())
    }
}

impl ProviderRouting for SharedProviderRouter {
    fn resolve(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
        feed_type: FeedType,
    ) -> RoutingResult<ResolvedRoute> {
        let router = self
            .inner
            .read()
            .map_err(|e| RoutingError::LockPoisoned(e.to_string()))?;
        router.resolve(symbol, asset_type, feed_type)
    }

    fn get_databento_dataset(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> Option<String> {
        let router = self.inner.read().ok()?;
        router.get_databento_dataset(symbol, asset_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::routing::{AssetTypeRouting, ProviderRoute, SymbolRouting};

    fn create_test_config() -> RoutingConfig {
        RoutingConfig::default()
            .add_symbol_routing(
                SymbolRouting::new(
                    "ES",
                    "CME",
                    AssetType::Futures,
                    ProviderRoute::with_dataset("databento", "GLBX.MDP3"),
                    ProviderRoute::with_dataset("databento", "GLBX.MDP3"),
                )
                .with_provider_symbol("databento", "ES.FUT"),
            )
            .add_symbol_routing(SymbolRouting::new(
                "AAPL",
                "NASDAQ",
                AssetType::Equity,
                ProviderRoute::with_dataset("databento", "XNAS.ITCH"),
                ProviderRoute::with_dataset("databento", "XNAS.ITCH"),
            ))
    }

    #[test]
    fn test_symbol_specific_resolution() {
        let router = ProviderRouter::new(create_test_config());
        let symbol = SymbolSpec::new("ES", "CME");

        let route = router
            .resolve_historical(&symbol, Some(AssetType::Futures))
            .unwrap();

        assert_eq!(route.provider, "databento");
        assert_eq!(route.dataset, Some("GLBX.MDP3".to_string()));
        assert_eq!(route.provider_symbol, "ES.FUT");
        assert_eq!(
            route.resolution_source,
            RouteResolutionSource::SymbolSpecific
        );
    }

    #[test]
    fn test_asset_type_fallback() {
        let router = ProviderRouter::new(create_test_config());
        // NQ is not in symbol-specific routing, should fall back to asset type
        let symbol = SymbolSpec::new("NQ", "CME");

        let route = router
            .resolve_historical(&symbol, Some(AssetType::Futures))
            .unwrap();

        assert_eq!(route.provider, "databento");
        assert_eq!(route.dataset, Some("GLBX.MDP3".to_string()));
        assert_eq!(route.provider_symbol, "NQ"); // No provider mapping
        assert_eq!(
            route.resolution_source,
            RouteResolutionSource::AssetTypeExchange
        );
    }

    #[test]
    fn test_global_default_fallback() {
        let router = ProviderRouter::new(create_test_config());
        // Unknown symbol with no asset type
        let symbol = SymbolSpec::new("UNKNOWN", "UNKNOWN_EXCHANGE");

        let route = router.resolve_historical(&symbol, None).unwrap();

        assert_eq!(route.provider, "databento");
        assert_eq!(route.dataset, Some("GLBX.MDP3".to_string())); // Default dataset
        assert_eq!(route.provider_symbol, "UNKNOWN");
        assert_eq!(
            route.resolution_source,
            RouteResolutionSource::GlobalDefault
        );
    }

    #[test]
    fn test_realtime_vs_historical() {
        let mut config = RoutingConfig::default();

        // Add asset type with different providers for historical vs realtime
        config.asset_types.insert(
            "crypto".to_string(),
            AssetTypeRouting::new("databento", "binance").with_default_exchange("BINANCE"),
        );

        let router = ProviderRouter::new(config);
        let symbol = SymbolSpec::new("BTCUSDT", "BINANCE");

        let historical = router
            .resolve_historical(&symbol, Some(AssetType::Crypto))
            .unwrap();
        let realtime = router
            .resolve_realtime(&symbol, Some(AssetType::Crypto))
            .unwrap();

        assert_eq!(historical.provider, "databento");
        assert_eq!(realtime.provider, "binance");
    }

    #[test]
    fn test_databento_dataset_resolution() {
        let router = ProviderRouter::new(create_test_config());

        // Symbol-specific
        let es_symbol = SymbolSpec::new("ES", "CME");
        assert_eq!(
            router.get_databento_dataset(&es_symbol, Some(AssetType::Futures)),
            Some("GLBX.MDP3".to_string())
        );

        // Asset type fallback
        let nq_symbol = SymbolSpec::new("NQ", "CME");
        assert_eq!(
            router.get_databento_dataset(&nq_symbol, Some(AssetType::Futures)),
            Some("GLBX.MDP3".to_string())
        );

        // Equity at NASDAQ
        let aapl_symbol = SymbolSpec::new("MSFT", "NASDAQ");
        assert_eq!(
            router.get_databento_dataset(&aapl_symbol, Some(AssetType::Equity)),
            Some("XNAS.ITCH".to_string())
        );

        // Global default (no asset type)
        let unknown_symbol = SymbolSpec::new("UNKNOWN", "UNKNOWN");
        assert_eq!(
            router.get_databento_dataset(&unknown_symbol, None),
            Some("GLBX.MDP3".to_string())
        );
    }

    #[test]
    fn test_case_insensitive_symbol_lookup() {
        let router = ProviderRouter::new(create_test_config());

        // Lowercase should match
        let symbol_lower = SymbolSpec::new("es", "cme");
        let route = router
            .resolve_historical(&symbol_lower, Some(AssetType::Futures))
            .unwrap();

        assert_eq!(
            route.resolution_source,
            RouteResolutionSource::SymbolSpecific
        );
        assert_eq!(route.provider_symbol, "ES.FUT");
    }

    #[test]
    fn test_provider_symbol_mapping() {
        let config = RoutingConfig::default().add_symbol_routing(
            SymbolRouting::new(
                "AAPL",
                "NASDAQ",
                AssetType::Equity,
                ProviderRoute::with_dataset("databento", "XNAS.ITCH"),
                ProviderRoute::new("binance"),
            )
            .with_provider_symbol("databento", "AAPL.NMS")
            .with_provider_symbol("binance", "AAPLUSDT"),
        );

        let router = ProviderRouter::new(config);
        let symbol = SymbolSpec::new("AAPL", "NASDAQ");

        let historical = router
            .resolve_historical(&symbol, Some(AssetType::Equity))
            .unwrap();
        assert_eq!(historical.provider_symbol, "AAPL.NMS");

        let realtime = router
            .resolve_realtime(&symbol, Some(AssetType::Equity))
            .unwrap();
        assert_eq!(realtime.provider_symbol, "AAPLUSDT");
    }

    #[test]
    fn test_shared_router_update() {
        let shared_router = SharedProviderRouter::with_defaults();

        // Initial config should have default provider
        let initial_config = shared_router.config().unwrap();
        assert_eq!(initial_config.default_provider, "databento");

        // Update config
        let mut new_config = initial_config.clone();
        new_config.default_provider = "binance".to_string();
        shared_router.update_config(new_config).unwrap();

        // Verify update
        let updated_config = shared_router.config().unwrap();
        assert_eq!(updated_config.default_provider, "binance");
    }

    #[test]
    fn test_shared_router_routing() {
        let config = create_test_config();
        let shared_router = SharedProviderRouter::new(config);

        let symbol = SymbolSpec::new("ES", "CME");
        let route = shared_router
            .resolve_historical(&symbol, Some(AssetType::Futures))
            .unwrap();

        assert_eq!(route.provider, "databento");
        assert_eq!(route.provider_symbol, "ES.FUT");
    }

    #[test]
    fn test_different_exchanges_same_asset_type() {
        let router = ProviderRouter::new(RoutingConfig::default());

        // Equity at NASDAQ
        let nasdaq_symbol = SymbolSpec::new("AAPL", "NASDAQ");
        let nasdaq_route = router
            .resolve_historical(&nasdaq_symbol, Some(AssetType::Equity))
            .unwrap();
        assert_eq!(nasdaq_route.dataset, Some("XNAS.ITCH".to_string()));

        // Equity at NYSE
        let nyse_symbol = SymbolSpec::new("IBM", "NYSE");
        let nyse_route = router
            .resolve_historical(&nyse_symbol, Some(AssetType::Equity))
            .unwrap();
        assert_eq!(nyse_route.dataset, Some("XNYS.TRADES".to_string()));
    }
}
