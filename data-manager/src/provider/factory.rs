//! Provider factory for creating providers based on routing decisions.
//!
//! This module provides a factory that creates the appropriate provider
//! based on the resolved route from the ProviderRouter.

use crate::config::{
    AssetType, FeedType, ProviderRouter, ProviderRouting, ResolvedRoute, RoutingConfig,
};
use crate::provider::databento::DatabentoClient;
use crate::provider::kraken::KrakenProvider;
use crate::provider::{DataProvider, HistoricalDataProvider, LiveStreamProvider, ProviderError};
use crate::symbol::SymbolSpec;

/// Provider factory for creating providers based on routing decisions.
#[derive(Debug, Clone)]
pub struct ProviderFactory {
    router: ProviderRouter,
    databento_api_key: Option<String>,
}

impl ProviderFactory {
    /// Create a new provider factory with default routing configuration.
    pub fn new() -> Self {
        Self {
            router: ProviderRouter::with_defaults(),
            databento_api_key: std::env::var("DATABENTO_API_KEY").ok(),
        }
    }

    /// Create a provider factory with custom routing configuration.
    pub fn with_routing(config: RoutingConfig) -> Self {
        Self {
            router: ProviderRouter::new(config),
            databento_api_key: std::env::var("DATABENTO_API_KEY").ok(),
        }
    }

    /// Set the Databento API key.
    pub fn with_databento_key(mut self, api_key: String) -> Self {
        self.databento_api_key = Some(api_key);
        self
    }

    /// Get the underlying router.
    pub fn router(&self) -> &ProviderRouter {
        &self.router
    }

    /// Resolve routing for a symbol.
    pub fn resolve(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
        feed_type: FeedType,
    ) -> ResolvedRoute {
        self.router
            .resolve(symbol, asset_type, feed_type)
            .expect("Routing resolution should never fail with defaults")
    }

    /// Resolve routing for historical data.
    pub fn resolve_historical(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> ResolvedRoute {
        self.resolve(symbol, asset_type, FeedType::Historical)
    }

    /// Resolve routing for realtime data.
    pub fn resolve_realtime(
        &self,
        symbol: &SymbolSpec,
        asset_type: Option<AssetType>,
    ) -> ResolvedRoute {
        self.resolve(symbol, asset_type, FeedType::Realtime)
    }

    /// Create a historical data provider based on the resolved route.
    ///
    /// # Arguments
    /// * `route` - The resolved route from the router
    ///
    /// # Returns
    /// A boxed historical data provider, or an error if the provider cannot be created.
    pub async fn create_historical_provider(
        &self,
        route: &ResolvedRoute,
    ) -> Result<Box<dyn HistoricalDataProvider>, ProviderError> {
        match route.provider.as_str() {
            "databento" => {
                let api_key = self.databento_api_key.as_ref().ok_or_else(|| {
                    ProviderError::Configuration(
                        "DATABENTO_API_KEY environment variable not set".to_string(),
                    )
                })?;

                let mut client = DatabentoClient::from_api_key(api_key.clone());
                client.connect().await?;
                Ok(Box::new(client))
            }
            provider => Err(ProviderError::Configuration(format!(
                "Provider '{}' does not support historical data. Use 'databento' for historical data.",
                provider
            ))),
        }
    }

    /// Create a live streaming provider based on the resolved route.
    ///
    /// # Arguments
    /// * `route` - The resolved route from the router
    ///
    /// # Returns
    /// A boxed live stream provider, or an error if the provider cannot be created.
    pub async fn create_live_provider(
        &self,
        route: &ResolvedRoute,
    ) -> Result<Box<dyn LiveStreamProvider>, ProviderError> {
        match route.provider.as_str() {
            "databento" => {
                let api_key = self.databento_api_key.as_ref().ok_or_else(|| {
                    ProviderError::Configuration(
                        "DATABENTO_API_KEY environment variable not set".to_string(),
                    )
                })?;

                let mut client = DatabentoClient::from_api_key(api_key.clone());
                client.connect().await?;
                Ok(Box::new(client))
            }
            "kraken" => {
                let provider = KrakenProvider::spot();
                Ok(Box::new(provider))
            }
            "kraken_futures" => {
                // Default to production, can be overridden via config
                let provider = KrakenProvider::futures(false);
                Ok(Box::new(provider))
            }
            "binance" => {
                // Binance provider would be created here
                // For now, return an error as it's not fully implemented yet
                Err(ProviderError::Configuration(
                    "Binance live provider not yet implemented via factory".to_string(),
                ))
            }
            provider => Err(ProviderError::Configuration(format!(
                "Unknown provider: '{}'. Supported: databento, kraken, kraken_futures",
                provider
            ))),
        }
    }

    /// Get the dataset for a symbol from the resolved route or router configuration.
    pub fn get_dataset(&self, route: &ResolvedRoute, asset_type: Option<AssetType>) -> String {
        // Use route's dataset if specified
        if let Some(dataset) = &route.dataset {
            return dataset.clone();
        }

        // Fall back to router's databento configuration
        if let Some(asset_type) = asset_type {
            return self
                .router
                .config()
                .databento
                .get_dataset(asset_type, "")
                .to_string();
        }

        // Ultimate fallback
        self.router.config().databento.default_dataset.clone()
    }
}

impl Default for ProviderFactory {
    fn default() -> Self {
        Self::new()
    }
}

/// Infer asset type from symbol and exchange.
///
/// This is a heuristic-based inference and may not be accurate for all cases.
/// Use explicit `--asset-type` when possible.
pub fn infer_asset_type(symbol: &str, exchange: &str) -> Option<AssetType> {
    let exchange_lower = exchange.to_lowercase();
    let symbol_upper = symbol.to_uppercase();

    // Kraken futures prefixes (check symbol first for Kraken)
    let kraken_futures_prefixes = ["PI_", "PF_", "FI_", "FF_", "PV_"];

    // Check futures patterns FIRST for any exchange
    // This handles Kraken perpetuals (PI_XBTUSD) and traditional futures (ESH5)
    let futures_roots = [
        "ES", "NQ", "YM", "RTY", "CL", "GC", "SI", "HG", "ZB", "ZN", "ZF", "ZT", "ZC", "ZS",
        "ZW", "LE", "HE", "6E", "6J", "6B", "6A", "6C",
    ];

    // Check for Kraken futures prefixes first
    for prefix in &kraken_futures_prefixes {
        if symbol_upper.starts_with(prefix) {
            return Some(AssetType::Futures);
        }
    }

    // Check for traditional futures contract patterns
    for root in &futures_roots {
        if symbol_upper.starts_with(root) {
            return Some(AssetType::Futures);
        }
    }

    // Exchange-based inference (now that futures are ruled out)
    match exchange_lower.as_str() {
        // Crypto exchanges (spot)
        "binance" | "kraken" | "coinbase" | "ftx" | "okx" | "bybit" | "huobi" | "kucoin"
        | "gemini" | "bitstamp" | "bitfinex" => Some(AssetType::Crypto),

        // Futures exchanges
        "cme" | "cbot" | "nymex" | "comex" | "ice" | "eurex" | "sgx" | "hkex" => {
            Some(AssetType::Futures)
        }

        // Equity exchanges
        "nasdaq" | "nyse" | "arca" | "bats" | "iex" | "amex" | "xnas" | "xnys" => {
            Some(AssetType::Equity)
        }

        // Options exchanges
        "cboe" | "phlx" | "ise" | "miax" | "box" => {
            // Could be equity or index options - check symbol pattern
            if symbol_upper.starts_with("SPX")
                || symbol_upper.starts_with("VIX")
                || symbol_upper.starts_with("NDX")
            {
                Some(AssetType::IndexOptions)
            } else {
                Some(AssetType::EquityOptions)
            }
        }

        // FX (typically OTC but some exchanges)
        "oanda" | "lmax" | "currenex" => Some(AssetType::Fx),

        _ => None,
    }
    .or_else(|| {
        // Symbol-based inference as final fallback

        // Crypto patterns (contains common crypto suffixes)
        if symbol_upper.ends_with("USDT")
            || symbol_upper.ends_with("USD")
            || symbol_upper.ends_with("BTC")
            || symbol_upper.ends_with("ETH")
            || symbol_upper.starts_with("BTC")
            || symbol_upper.starts_with("ETH")
            || symbol_upper.starts_with("XBT")
        {
            // Check if it looks like a futures contract (has month/year code)
            if symbol_upper.len() > 6
                && symbol_upper
                    .chars()
                    .rev()
                    .take(2)
                    .all(|c| c.is_ascii_digit())
            {
                // Could be a futures contract
                return Some(AssetType::Futures);
            }
            return Some(AssetType::Crypto);
        }

        // FX patterns (EURUSD, USDJPY, etc.)
        let fx_pairs = [
            "EURUSD", "USDJPY", "GBPUSD", "USDCHF", "AUDUSD", "USDCAD", "NZDUSD", "EURGBP",
            "EURJPY", "GBPJPY",
        ];
        for pair in &fx_pairs {
            if symbol_upper.contains(pair) || symbol_upper == *pair {
                return Some(AssetType::Fx);
            }
        }

        // Default: assume equity if 1-5 character symbol (typical stock ticker)
        if symbol.len() >= 1 && symbol.len() <= 5 && symbol.chars().all(|c| c.is_alphabetic()) {
            return Some(AssetType::Equity);
        }

        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_asset_type_crypto() {
        assert_eq!(
            infer_asset_type("BTCUSDT", "binance"),
            Some(AssetType::Crypto)
        );
        assert_eq!(
            infer_asset_type("ETHUSD", "kraken"),
            Some(AssetType::Crypto)
        );
        assert_eq!(
            infer_asset_type("XBTUSD", "kraken"),
            Some(AssetType::Crypto)
        );
    }

    #[test]
    fn test_infer_asset_type_futures() {
        assert_eq!(infer_asset_type("ESH5", "CME"), Some(AssetType::Futures));
        assert_eq!(infer_asset_type("NQM4", "CME"), Some(AssetType::Futures));
        assert_eq!(infer_asset_type("CLZ4", "NYMEX"), Some(AssetType::Futures));
        assert_eq!(
            infer_asset_type("PI_XBTUSD", "kraken"),
            Some(AssetType::Futures)
        );
    }

    #[test]
    fn test_infer_asset_type_equity() {
        assert_eq!(infer_asset_type("AAPL", "NASDAQ"), Some(AssetType::Equity));
        assert_eq!(infer_asset_type("MSFT", "NASDAQ"), Some(AssetType::Equity));
        assert_eq!(infer_asset_type("IBM", "NYSE"), Some(AssetType::Equity));
    }

    #[test]
    fn test_infer_asset_type_from_exchange() {
        // Exchange takes precedence
        assert_eq!(
            infer_asset_type("UNKNOWN", "binance"),
            Some(AssetType::Crypto)
        );
        assert_eq!(
            infer_asset_type("UNKNOWN", "CME"),
            Some(AssetType::Futures)
        );
        assert_eq!(
            infer_asset_type("UNKNOWN", "NASDAQ"),
            Some(AssetType::Equity)
        );
    }

    #[test]
    fn test_provider_factory_resolve() {
        let factory = ProviderFactory::new();
        let symbol = SymbolSpec::new("ESH5", "CME");

        let route = factory.resolve_historical(&symbol, Some(AssetType::Futures));
        assert_eq!(route.provider, "databento");
        assert!(route.dataset.is_some());
    }
}
