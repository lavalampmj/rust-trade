//! Symbol discovery from providers
//!
//! Provides functionality to discover available symbols from data providers.

use std::collections::HashMap;
use tracing::info;

use super::{SymbolSpec, SymbolStatus};
use crate::provider::{DataProvider, ProviderResult};

/// Symbol discovery result
#[derive(Debug, Clone)]
pub struct DiscoveredSymbol {
    pub spec: SymbolSpec,
    pub status: SymbolStatus,
    pub description: Option<String>,
    pub asset_class: Option<String>,
    pub first_available: Option<chrono::DateTime<chrono::Utc>>,
    pub last_available: Option<chrono::DateTime<chrono::Utc>>,
}

impl DiscoveredSymbol {
    pub fn new(spec: SymbolSpec) -> Self {
        Self {
            spec,
            status: SymbolStatus::Active,
            description: None,
            asset_class: None,
            first_available: None,
            last_available: None,
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_asset_class(mut self, asset_class: String) -> Self {
        self.asset_class = Some(asset_class);
        self
    }
}

/// Symbol discovery service
pub struct SymbolDiscovery {
    /// Discovered symbols cache
    cache: HashMap<String, Vec<DiscoveredSymbol>>,
}

impl SymbolDiscovery {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Discover symbols from a provider
    pub async fn discover_from_provider<P: DataProvider>(
        &mut self,
        provider: &P,
        exchange: Option<&str>,
    ) -> ProviderResult<Vec<DiscoveredSymbol>> {
        let provider_name = provider.info().name.clone();
        info!(
            "Discovering symbols from {} (exchange: {:?})",
            provider_name, exchange
        );

        let specs = provider.discover_symbols(exchange).await?;

        let discovered: Vec<DiscoveredSymbol> =
            specs.into_iter().map(DiscoveredSymbol::new).collect();

        // Cache results
        let cache_key = format!("{}:{}", provider_name, exchange.unwrap_or("all"));
        self.cache.insert(cache_key, discovered.clone());

        info!(
            "Discovered {} symbols from {}",
            discovered.len(),
            provider_name
        );
        Ok(discovered)
    }

    /// Get cached discovery results
    pub fn get_cached(
        &self,
        provider: &str,
        exchange: Option<&str>,
    ) -> Option<&Vec<DiscoveredSymbol>> {
        let cache_key = format!("{}:{}", provider, exchange.unwrap_or("all"));
        self.cache.get(&cache_key)
    }

    /// Clear discovery cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Filter discovered symbols by criteria
    pub fn filter_symbols(
        symbols: &[DiscoveredSymbol],
        asset_class: Option<&str>,
        status: Option<SymbolStatus>,
    ) -> Vec<DiscoveredSymbol> {
        symbols
            .iter()
            .filter(|s| {
                let class_match = asset_class
                    .map(|c| s.asset_class.as_deref() == Some(c))
                    .unwrap_or(true);
                let status_match = status.map(|st| s.status == st).unwrap_or(true);
                class_match && status_match
            })
            .cloned()
            .collect()
    }
}

impl Default for SymbolDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Common symbol mappings for known instruments
pub mod mappings {
    use super::SymbolSpec;

    /// Get standard futures symbols for CME
    pub fn cme_futures() -> Vec<SymbolSpec> {
        vec![
            SymbolSpec::new("ES", "CME"),  // E-mini S&P 500
            SymbolSpec::new("NQ", "CME"),  // E-mini Nasdaq 100
            SymbolSpec::new("YM", "CME"),  // E-mini Dow
            SymbolSpec::new("RTY", "CME"), // E-mini Russell 2000
            SymbolSpec::new("CL", "CME"),  // Crude Oil
            SymbolSpec::new("GC", "CME"),  // Gold
            SymbolSpec::new("SI", "CME"),  // Silver
            SymbolSpec::new("ZN", "CME"),  // 10-Year T-Note
            SymbolSpec::new("ZB", "CME"),  // 30-Year T-Bond
            SymbolSpec::new("ZC", "CME"),  // Corn
            SymbolSpec::new("ZS", "CME"),  // Soybeans
            SymbolSpec::new("ZW", "CME"),  // Wheat
            SymbolSpec::new("6E", "CME"),  // Euro FX
            SymbolSpec::new("6J", "CME"),  // Japanese Yen
            SymbolSpec::new("6B", "CME"),  // British Pound
        ]
    }

    /// Get standard crypto symbols for Binance
    pub fn binance_crypto() -> Vec<SymbolSpec> {
        vec![
            SymbolSpec::new("BTCUSDT", "BINANCE"),
            SymbolSpec::new("ETHUSDT", "BINANCE"),
            SymbolSpec::new("BNBUSDT", "BINANCE"),
            SymbolSpec::new("SOLUSDT", "BINANCE"),
            SymbolSpec::new("ADAUSDT", "BINANCE"),
            SymbolSpec::new("DOTUSDT", "BINANCE"),
            SymbolSpec::new("XRPUSDT", "BINANCE"),
            SymbolSpec::new("DOGEUSDT", "BINANCE"),
            SymbolSpec::new("MATICUSDT", "BINANCE"),
            SymbolSpec::new("LINKUSDT", "BINANCE"),
        ]
    }

    /// Get standard equity ETF symbols
    pub fn equity_etfs() -> Vec<SymbolSpec> {
        vec![
            SymbolSpec::new("SPY", "NYSE"),   // S&P 500 ETF
            SymbolSpec::new("QQQ", "NASDAQ"), // Nasdaq 100 ETF
            SymbolSpec::new("IWM", "NYSE"),   // Russell 2000 ETF
            SymbolSpec::new("DIA", "NYSE"),   // Dow Jones ETF
            SymbolSpec::new("VTI", "NYSE"),   // Total Stock Market
            SymbolSpec::new("VOO", "NYSE"),   // Vanguard S&P 500
            SymbolSpec::new("GLD", "NYSE"),   // Gold ETF
            SymbolSpec::new("SLV", "NYSE"),   // Silver ETF
            SymbolSpec::new("TLT", "NASDAQ"), // 20+ Year Treasury
            SymbolSpec::new("VIX", "CBOE"),   // Volatility Index
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovered_symbol() {
        let spec = SymbolSpec::new("ES", "CME");
        let discovered = DiscoveredSymbol::new(spec)
            .with_description("E-mini S&P 500 Futures".to_string())
            .with_asset_class("futures".to_string());

        assert_eq!(discovered.spec.symbol, "ES");
        assert_eq!(
            discovered.description,
            Some("E-mini S&P 500 Futures".to_string())
        );
        assert_eq!(discovered.asset_class, Some("futures".to_string()));
    }

    #[test]
    fn test_filter_symbols() {
        let symbols = vec![
            DiscoveredSymbol::new(SymbolSpec::new("ES", "CME"))
                .with_asset_class("futures".to_string()),
            DiscoveredSymbol::new(SymbolSpec::new("SPY", "NYSE"))
                .with_asset_class("etf".to_string()),
        ];

        let filtered = SymbolDiscovery::filter_symbols(&symbols, Some("futures"), None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].spec.symbol, "ES");
    }

    #[test]
    fn test_cme_futures() {
        let symbols = mappings::cme_futures();
        assert!(symbols.len() >= 10);
        assert!(symbols.iter().any(|s| s.symbol == "ES"));
    }
}
