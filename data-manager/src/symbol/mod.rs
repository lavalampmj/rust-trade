//! Symbol universe management
//!
//! This module provides symbol management including:
//! - SymbolSpec: Symbol specification with exchange
//! - SymbolUniverse: Manager for the active symbol set
//! - SymbolRegistry: Database-backed symbol storage
//! - Discovery: Provider-based symbol discovery

mod universe;
mod discovery;
mod registry;

pub use universe::*;
pub use discovery::*;
pub use registry::*;

use crate::config::routing::AssetType;
use serde::{Deserialize, Serialize};

/// Symbol specification with exchange
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolSpec {
    /// Symbol name (e.g., "ES", "BTCUSDT", "AAPL")
    pub symbol: String,
    /// Exchange identifier (e.g., "CME", "BINANCE", "NASDAQ")
    pub exchange: String,
    /// Asset type for routing decisions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_type: Option<AssetType>,
    /// Provider-specific symbol mapping
    #[serde(default)]
    pub provider_mapping: std::collections::HashMap<String, String>,
}

impl std::hash::Hash for SymbolSpec {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Only hash symbol and exchange, not the provider_mapping
        self.symbol.hash(state);
        self.exchange.hash(state);
    }
}

impl SymbolSpec {
    /// Create a new symbol spec
    pub fn new(symbol: impl Into<String>, exchange: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: exchange.into(),
            asset_type: None,
            provider_mapping: std::collections::HashMap::new(),
        }
    }

    /// Create a new symbol spec with asset type
    pub fn with_asset_type(
        symbol: impl Into<String>,
        exchange: impl Into<String>,
        asset_type: AssetType,
    ) -> Self {
        Self {
            symbol: symbol.into(),
            exchange: exchange.into(),
            asset_type: Some(asset_type),
            provider_mapping: std::collections::HashMap::new(),
        }
    }

    /// Create from a full symbol string (e.g., "ES@CME")
    pub fn from_full_symbol(full: &str) -> Option<Self> {
        let parts: Vec<&str> = full.split('@').collect();
        if parts.len() == 2 {
            Some(Self::new(parts[0], parts[1]))
        } else {
            None
        }
    }

    /// Set the asset type for this symbol
    pub fn set_asset_type(mut self, asset_type: AssetType) -> Self {
        self.asset_type = Some(asset_type);
        self
    }

    /// Get the full symbol identifier
    pub fn full_symbol(&self) -> String {
        format!("{}@{}", self.symbol, self.exchange)
    }

    /// Add a provider-specific mapping
    pub fn with_provider_mapping(mut self, provider: &str, provider_symbol: &str) -> Self {
        self.provider_mapping
            .insert(provider.to_string(), provider_symbol.to_string());
        self
    }

    /// Get the symbol for a specific provider
    pub fn symbol_for_provider(&self, provider: &str) -> &str {
        self.provider_mapping
            .get(provider)
            .map(|s| s.as_str())
            .unwrap_or(&self.symbol)
    }
}

impl std::fmt::Display for SymbolSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.full_symbol())
    }
}

/// Symbol status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolStatus {
    /// Symbol is active and can be subscribed to
    Active,
    /// Symbol is inactive but data exists
    Inactive,
    /// Symbol is deprecated and will be removed
    Deprecated,
    /// Symbol is pending verification
    Pending,
}

impl SymbolStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolStatus::Active => "active",
            SymbolStatus::Inactive => "inactive",
            SymbolStatus::Deprecated => "deprecated",
            SymbolStatus::Pending => "pending",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(SymbolStatus::Active),
            "inactive" => Some(SymbolStatus::Inactive),
            "deprecated" => Some(SymbolStatus::Deprecated),
            "pending" => Some(SymbolStatus::Pending),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_spec_creation() {
        let spec = SymbolSpec::new("ES", "CME");
        assert_eq!(spec.symbol, "ES");
        assert_eq!(spec.exchange, "CME");
        assert_eq!(spec.full_symbol(), "ES@CME");
    }

    #[test]
    fn test_symbol_spec_from_full() {
        let spec = SymbolSpec::from_full_symbol("BTCUSDT@BINANCE").unwrap();
        assert_eq!(spec.symbol, "BTCUSDT");
        assert_eq!(spec.exchange, "BINANCE");
    }

    #[test]
    fn test_provider_mapping() {
        let spec = SymbolSpec::new("ES", "CME")
            .with_provider_mapping("databento", "ESH5");

        assert_eq!(spec.symbol_for_provider("databento"), "ESH5");
        assert_eq!(spec.symbol_for_provider("unknown"), "ES");
    }

    #[test]
    fn test_symbol_status() {
        assert_eq!(SymbolStatus::Active.as_str(), "active");
        assert_eq!(SymbolStatus::from_str("inactive"), Some(SymbolStatus::Inactive));
    }
}
