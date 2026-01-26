//! Symbol registry for canonical symbol mapping across venues.
//!
//! This module provides mappings between canonical symbols (e.g., "BTC/USD")
//! and venue-specific symbols (e.g., "BTCUSDT" on Binance, "PI_XBTUSD" on Kraken Futures).
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::router::{SymbolRegistry, CanonicalSymbol};
//!
//! let mut registry = SymbolRegistry::new();
//!
//! // Register BTC/USD across venues
//! registry.register(
//!     CanonicalSymbol::new("BTC/USD", "BTC", "USD")
//!         .with_venue_symbol("BINANCE", "BTCUSDT")
//!         .with_venue_symbol("BINANCE_FUTURES", "BTCUSDT")
//!         .with_venue_symbol("KRAKEN", "XBT/USD")
//!         .with_venue_symbol("KRAKEN_FUTURES", "PI_XBTUSD")
//! );
//!
//! // Lookup
//! let kraken_symbol = registry.venue_symbol("BTC/USD", "KRAKEN_FUTURES");
//! assert_eq!(kraken_symbol, Some("PI_XBTUSD"));
//!
//! // Reverse lookup
//! let canonical = registry.canonical_symbol("PI_XBTUSD", "KRAKEN_FUTURES");
//! assert_eq!(canonical, Some("BTC/USD"));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A canonical symbol representing the same underlying across venues.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSymbol {
    /// Canonical identifier (e.g., "BTC/USD")
    pub id: String,

    /// Base asset (e.g., "BTC")
    pub base: String,

    /// Quote asset (e.g., "USD", "USDT")
    pub quote: String,

    /// Venue-specific symbol mappings
    /// Key: venue ID (e.g., "BINANCE", "KRAKEN_FUTURES")
    /// Value: venue-specific symbol (e.g., "BTCUSDT", "PI_XBTUSD")
    pub venue_symbols: HashMap<String, VenueSymbolMapping>,
}

impl CanonicalSymbol {
    /// Create a new canonical symbol.
    pub fn new(id: impl Into<String>, base: impl Into<String>, quote: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            base: base.into(),
            quote: quote.into(),
            venue_symbols: HashMap::new(),
        }
    }

    /// Add a venue-specific symbol mapping.
    pub fn with_venue_symbol(mut self, venue: impl Into<String>, symbol: impl Into<String>) -> Self {
        self.venue_symbols.insert(
            venue.into(),
            VenueSymbolMapping {
                symbol: symbol.into(),
                ..Default::default()
            },
        );
        self
    }

    /// Add a venue-specific symbol with full mapping details.
    pub fn with_venue_mapping(mut self, venue: impl Into<String>, mapping: VenueSymbolMapping) -> Self {
        self.venue_symbols.insert(venue.into(), mapping);
        self
    }

    /// Get the symbol for a specific venue.
    pub fn symbol_for_venue(&self, venue: &str) -> Option<&str> {
        self.venue_symbols.get(venue).map(|m| m.symbol.as_str())
    }

    /// Get all venues that support this symbol.
    pub fn available_venues(&self) -> impl Iterator<Item = &str> {
        self.venue_symbols.keys().map(|s| s.as_str())
    }
}

/// Detailed mapping information for a venue-specific symbol.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenueSymbolMapping {
    /// The symbol as used by the venue
    pub symbol: String,

    /// Minimum order quantity
    #[serde(default)]
    pub min_quantity: Option<rust_decimal::Decimal>,

    /// Maximum order quantity
    #[serde(default)]
    pub max_quantity: Option<rust_decimal::Decimal>,

    /// Quantity step size (lot size)
    #[serde(default)]
    pub quantity_step: Option<rust_decimal::Decimal>,

    /// Minimum price
    #[serde(default)]
    pub min_price: Option<rust_decimal::Decimal>,

    /// Price tick size
    #[serde(default)]
    pub price_tick: Option<rust_decimal::Decimal>,

    /// Minimum notional value
    #[serde(default)]
    pub min_notional: Option<rust_decimal::Decimal>,

    /// Whether the symbol is currently tradeable
    #[serde(default = "default_true")]
    pub is_tradeable: bool,
}

fn default_true() -> bool {
    true
}

impl VenueSymbolMapping {
    /// Create a new venue symbol mapping.
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            is_tradeable: true,
            ..Default::default()
        }
    }

    /// Set minimum quantity.
    pub fn with_min_quantity(mut self, qty: rust_decimal::Decimal) -> Self {
        self.min_quantity = Some(qty);
        self
    }

    /// Set quantity step size.
    pub fn with_quantity_step(mut self, step: rust_decimal::Decimal) -> Self {
        self.quantity_step = Some(step);
        self
    }

    /// Set price tick size.
    pub fn with_price_tick(mut self, tick: rust_decimal::Decimal) -> Self {
        self.price_tick = Some(tick);
        self
    }

    /// Set minimum notional.
    pub fn with_min_notional(mut self, notional: rust_decimal::Decimal) -> Self {
        self.min_notional = Some(notional);
        self
    }
}

/// Registry for canonical symbols and their venue mappings.
#[derive(Debug, Clone, Default)]
pub struct SymbolRegistry {
    /// Canonical symbols by ID
    symbols: HashMap<String, CanonicalSymbol>,

    /// Reverse lookup: (venue, venue_symbol) -> canonical_id
    reverse_lookup: HashMap<(String, String), String>,
}

impl SymbolRegistry {
    /// Create a new empty symbol registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with common crypto symbols pre-registered.
    pub fn with_crypto_defaults() -> Self {
        let mut registry = Self::new();

        // BTC/USD
        registry.register(
            CanonicalSymbol::new("BTC/USD", "BTC", "USD")
                .with_venue_symbol("BINANCE", "BTCUSDT")
                .with_venue_symbol("BINANCE_US", "BTCUSD")
                .with_venue_symbol("BINANCE_FUTURES", "BTCUSDT")
                .with_venue_symbol("KRAKEN", "XBT/USD")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_XBTUSD"),
        );

        // ETH/USD
        registry.register(
            CanonicalSymbol::new("ETH/USD", "ETH", "USD")
                .with_venue_symbol("BINANCE", "ETHUSDT")
                .with_venue_symbol("BINANCE_US", "ETHUSD")
                .with_venue_symbol("BINANCE_FUTURES", "ETHUSDT")
                .with_venue_symbol("KRAKEN", "ETH/USD")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_ETHUSD"),
        );

        // SOL/USD
        registry.register(
            CanonicalSymbol::new("SOL/USD", "SOL", "USD")
                .with_venue_symbol("BINANCE", "SOLUSDT")
                .with_venue_symbol("BINANCE_US", "SOLUSD")
                .with_venue_symbol("BINANCE_FUTURES", "SOLUSDT")
                .with_venue_symbol("KRAKEN", "SOL/USD")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_SOLUSD"),
        );

        registry
    }

    /// Register a canonical symbol.
    pub fn register(&mut self, symbol: CanonicalSymbol) {
        // Build reverse lookup entries
        for (venue, mapping) in &symbol.venue_symbols {
            self.reverse_lookup.insert(
                (venue.clone(), mapping.symbol.clone()),
                symbol.id.clone(),
            );
        }

        self.symbols.insert(symbol.id.clone(), symbol);
    }

    /// Get a canonical symbol by ID.
    pub fn get(&self, canonical_id: &str) -> Option<&CanonicalSymbol> {
        self.symbols.get(canonical_id)
    }

    /// Get the venue-specific symbol for a canonical symbol.
    pub fn venue_symbol(&self, canonical_id: &str, venue: &str) -> Option<&str> {
        self.symbols
            .get(canonical_id)
            .and_then(|s| s.symbol_for_venue(venue))
    }

    /// Get the canonical symbol ID from a venue-specific symbol.
    pub fn canonical_symbol(&self, venue_symbol: &str, venue: &str) -> Option<&str> {
        self.reverse_lookup
            .get(&(venue.to_string(), venue_symbol.to_string()))
            .map(|s| s.as_str())
    }

    /// Get all canonical symbols.
    pub fn all_symbols(&self) -> impl Iterator<Item = &CanonicalSymbol> {
        self.symbols.values()
    }

    /// Get all venues that have a mapping for a canonical symbol.
    pub fn venues_for_symbol(&self, canonical_id: &str) -> Vec<&str> {
        self.symbols
            .get(canonical_id)
            .map(|s| s.available_venues().collect())
            .unwrap_or_default()
    }

    /// Check if a canonical symbol is available on a venue.
    pub fn is_available(&self, canonical_id: &str, venue: &str) -> bool {
        self.venue_symbol(canonical_id, venue).is_some()
    }

    /// Get the full mapping for a symbol on a venue.
    pub fn get_mapping(&self, canonical_id: &str, venue: &str) -> Option<&VenueSymbolMapping> {
        self.symbols
            .get(canonical_id)
            .and_then(|s| s.venue_symbols.get(venue))
    }

    /// Convert a venue symbol to the internal framework format.
    ///
    /// This is a convenience method that:
    /// 1. Looks up the canonical symbol
    /// 2. Returns the Binance-style symbol (used internally)
    ///
    /// Returns the original symbol if no mapping found.
    pub fn to_internal_symbol(&self, venue_symbol: &str, venue: &str) -> String {
        if let Some(canonical_id) = self.canonical_symbol(venue_symbol, venue) {
            // Return Binance-style symbol as internal format
            if let Some(binance_symbol) = self.venue_symbol(canonical_id, "BINANCE") {
                return binance_symbol.to_string();
            }
        }
        venue_symbol.to_string()
    }

    /// Convert an internal symbol to venue-specific format.
    ///
    /// This assumes the internal symbol is in Binance format (e.g., "BTCUSDT").
    pub fn to_venue_symbol(&self, internal_symbol: &str, target_venue: &str) -> String {
        // First, try to find the canonical symbol from the internal (Binance) format
        if let Some(canonical_id) = self.canonical_symbol(internal_symbol, "BINANCE") {
            if let Some(venue_symbol) = self.venue_symbol(canonical_id, target_venue) {
                return venue_symbol.to_string();
            }
        }
        internal_symbol.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonical_symbol_creation() {
        let symbol = CanonicalSymbol::new("BTC/USD", "BTC", "USD")
            .with_venue_symbol("BINANCE", "BTCUSDT")
            .with_venue_symbol("KRAKEN", "XBT/USD");

        assert_eq!(symbol.id, "BTC/USD");
        assert_eq!(symbol.base, "BTC");
        assert_eq!(symbol.quote, "USD");
        assert_eq!(symbol.symbol_for_venue("BINANCE"), Some("BTCUSDT"));
        assert_eq!(symbol.symbol_for_venue("KRAKEN"), Some("XBT/USD"));
        assert_eq!(symbol.symbol_for_venue("UNKNOWN"), None);
    }

    #[test]
    fn test_symbol_registry() {
        let mut registry = SymbolRegistry::new();

        registry.register(
            CanonicalSymbol::new("BTC/USD", "BTC", "USD")
                .with_venue_symbol("BINANCE", "BTCUSDT")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_XBTUSD"),
        );

        // Forward lookup
        assert_eq!(
            registry.venue_symbol("BTC/USD", "BINANCE"),
            Some("BTCUSDT")
        );
        assert_eq!(
            registry.venue_symbol("BTC/USD", "KRAKEN_FUTURES"),
            Some("PI_XBTUSD")
        );

        // Reverse lookup
        assert_eq!(
            registry.canonical_symbol("BTCUSDT", "BINANCE"),
            Some("BTC/USD")
        );
        assert_eq!(
            registry.canonical_symbol("PI_XBTUSD", "KRAKEN_FUTURES"),
            Some("BTC/USD")
        );
    }

    #[test]
    fn test_crypto_defaults() {
        let registry = SymbolRegistry::with_crypto_defaults();

        // Check BTC mappings
        assert_eq!(registry.venue_symbol("BTC/USD", "BINANCE"), Some("BTCUSDT"));
        assert_eq!(
            registry.venue_symbol("BTC/USD", "KRAKEN_FUTURES"),
            Some("PI_XBTUSD")
        );

        // Check ETH mappings
        assert_eq!(registry.venue_symbol("ETH/USD", "BINANCE"), Some("ETHUSDT"));
        assert_eq!(
            registry.venue_symbol("ETH/USD", "KRAKEN_FUTURES"),
            Some("PI_ETHUSD")
        );
    }

    #[test]
    fn test_symbol_conversion() {
        let registry = SymbolRegistry::with_crypto_defaults();

        // Venue to internal
        assert_eq!(
            registry.to_internal_symbol("PI_XBTUSD", "KRAKEN_FUTURES"),
            "BTCUSDT"
        );

        // Internal to venue
        assert_eq!(
            registry.to_venue_symbol("BTCUSDT", "KRAKEN_FUTURES"),
            "PI_XBTUSD"
        );
        assert_eq!(
            registry.to_venue_symbol("ETHUSDT", "KRAKEN"),
            "ETH/USD"
        );
    }

    #[test]
    fn test_venues_for_symbol() {
        let registry = SymbolRegistry::with_crypto_defaults();

        let venues = registry.venues_for_symbol("BTC/USD");
        assert!(venues.contains(&"BINANCE"));
        assert!(venues.contains(&"KRAKEN_FUTURES"));
        assert!(venues.len() >= 4); // At least 4 venues for BTC/USD
    }

    #[test]
    fn test_is_available() {
        let registry = SymbolRegistry::with_crypto_defaults();

        assert!(registry.is_available("BTC/USD", "BINANCE"));
        assert!(registry.is_available("BTC/USD", "KRAKEN_FUTURES"));
        assert!(!registry.is_available("BTC/USD", "UNKNOWN_VENUE"));
        assert!(!registry.is_available("UNKNOWN/SYMBOL", "BINANCE"));
    }
}
