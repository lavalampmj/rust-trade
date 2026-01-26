//! Symbol registry for canonical symbol mapping across venues.
//!
//! This module provides mappings between canonical symbols and venue-specific
//! raw symbols, aligned with [Databento's symbology conventions](https://databento.com/docs/standards-and-conventions/symbology).
//!
//! # Databento Symbology Alignment
//!
//! | DBT Type | Our Mapping | Example |
//! |----------|-------------|---------|
//! | `raw_symbol` | `VenueSymbolMapping.raw_symbol` | `BTCUSDT`, `ESH6`, `PI_XBTUSD` |
//! | `parent` | `CanonicalSymbol.id` | `BTC.SPOT`, `ES.FUT` |
//! | `continuous` | Supported via `ContinuousSymbol` | `ES.c.0`, `CL.v.0` |
//!
//! # Design Principles
//!
//! 1. **Canonical symbols** use DBT `parent` format: `{asset}.{instrument_class}`
//! 2. **Venue symbols** are the exchange's `raw_symbol` (e.g., `BTCUSDT`, `XBT/USD`)
//! 3. **Conversion happens at venue boundary** - router uses canonical, venues convert
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::router::{
//!     SymbolRegistry, CanonicalSymbol, InstrumentClass,
//! };
//!
//! let mut registry = SymbolRegistry::new();
//!
//! // Register BTC spot across venues using DBT parent format
//! registry.register(
//!     CanonicalSymbol::new("BTC.SPOT", "BTC", "USD", InstrumentClass::Spot)
//!         .with_venue_symbol("BINANCE", "BTCUSDT")
//!         .with_venue_symbol("BINANCE_US", "BTCUSD")
//!         .with_venue_symbol("KRAKEN", "XBT/USD")
//! );
//!
//! // Register BTC perpetual futures
//! registry.register(
//!     CanonicalSymbol::new("BTC.PERP", "BTC", "USD", InstrumentClass::Perpetual)
//!         .with_venue_symbol("BINANCE_FUTURES", "BTCUSDT")
//!         .with_venue_symbol("KRAKEN_FUTURES", "PI_XBTUSD")
//! );
//!
//! // Lookup: canonical -> venue raw_symbol
//! let kraken_symbol = registry.to_raw_symbol("BTC.PERP", "KRAKEN_FUTURES");
//! assert_eq!(kraken_symbol, Some("PI_XBTUSD"));
//!
//! // Reverse lookup: venue raw_symbol -> canonical
//! let canonical = registry.to_canonical("PI_XBTUSD", "KRAKEN_FUTURES");
//! assert_eq!(canonical, Some("BTC.PERP"));
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Instrument class enumeration aligned with Databento's `instrument_class` field.
///
/// Single character codes match Databento conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentClass {
    /// 'S' - Spot/Cash market
    Spot,
    /// 'F' - Future (dated expiry)
    Future,
    /// 'P' - Perpetual swap (no expiry)
    Perpetual,
    /// 'O' - Option
    Option,
    /// 'X' - Spread/Strategy
    Spread,
}

impl InstrumentClass {
    /// Get the single character code (Databento convention).
    pub fn as_char(&self) -> char {
        match self {
            Self::Spot => 'S',
            Self::Future => 'F',
            Self::Perpetual => 'P',
            Self::Option => 'O',
            Self::Spread => 'X',
        }
    }

    /// Get the suffix used in parent symbol format.
    pub fn as_suffix(&self) -> &'static str {
        match self {
            Self::Spot => "SPOT",
            Self::Future => "FUT",
            Self::Perpetual => "PERP",
            Self::Option => "OPT",
            Self::Spread => "SPRD",
        }
    }

    /// Parse from suffix string.
    pub fn from_suffix(suffix: &str) -> Option<Self> {
        match suffix.to_uppercase().as_str() {
            "SPOT" | "S" => Some(Self::Spot),
            "FUT" | "F" | "FUTURE" => Some(Self::Future),
            "PERP" | "P" | "PERPETUAL" => Some(Self::Perpetual),
            "OPT" | "O" | "OPTION" => Some(Self::Option),
            "SPRD" | "X" | "SPREAD" => Some(Self::Spread),
            _ => None,
        }
    }
}

impl std::fmt::Display for InstrumentClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_suffix())
    }
}

/// Continuous contract roll method (Databento notation).
///
/// Used for continuous contract symbols like `ES.c.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContinuousRollMethod {
    /// 'c' - Calendar: Roll by expiration date order
    Calendar,
    /// 'v' - Volume: Roll by highest trading volume
    Volume,
    /// 'n' - Open Interest: Roll by highest open interest
    OpenInterest,
}

impl ContinuousRollMethod {
    /// Get the single character code (Databento convention).
    pub fn as_char(&self) -> char {
        match self {
            Self::Calendar => 'c',
            Self::Volume => 'v',
            Self::OpenInterest => 'n',
        }
    }

    /// Parse from character.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'c' | 'C' => Some(Self::Calendar),
            'v' | 'V' => Some(Self::Volume),
            'n' | 'N' => Some(Self::OpenInterest),
            _ => None,
        }
    }

    /// Generate Databento-compatible continuous symbol.
    ///
    /// # Example
    /// ```ignore
    /// let method = ContinuousRollMethod::Calendar;
    /// assert_eq!(method.continuous_symbol("ES", 0), "ES.c.0");
    /// ```
    pub fn continuous_symbol(&self, base_asset: &str, rank: u8) -> String {
        format!("{}.{}.{}", base_asset, self.as_char(), rank)
    }
}

/// A canonical symbol representing the same underlying across venues.
///
/// Uses Databento's `parent` format: `{asset}.{instrument_class}`
///
/// # Examples
/// - `BTC.SPOT` - Bitcoin spot market
/// - `BTC.PERP` - Bitcoin perpetual futures
/// - `ES.FUT` - E-mini S&P 500 futures
/// - `AAPL.SPOT` - Apple stock (spot)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSymbol {
    /// Canonical identifier in DBT parent format (e.g., "BTC.SPOT", "ES.FUT")
    pub id: String,

    /// Base asset (e.g., "BTC", "ES", "AAPL")
    pub asset: String,

    /// Quote/settlement currency (e.g., "USD", "USDT", "EUR")
    pub quote_currency: String,

    /// Instrument class
    pub instrument_class: InstrumentClass,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,

    /// Venue-specific symbol mappings
    /// Key: venue ID (e.g., "BINANCE", "KRAKEN_FUTURES")
    /// Value: venue-specific raw_symbol mapping
    pub venue_mappings: HashMap<String, VenueSymbolMapping>,
}

impl CanonicalSymbol {
    /// Create a new canonical symbol with DBT parent format.
    ///
    /// # Arguments
    /// * `id` - Canonical ID in format `{asset}.{class}` (e.g., "BTC.SPOT")
    /// * `asset` - Base asset (e.g., "BTC")
    /// * `quote_currency` - Quote currency (e.g., "USD")
    /// * `instrument_class` - Type of instrument
    pub fn new(
        id: impl Into<String>,
        asset: impl Into<String>,
        quote_currency: impl Into<String>,
        instrument_class: InstrumentClass,
    ) -> Self {
        Self {
            id: id.into(),
            asset: asset.into(),
            quote_currency: quote_currency.into(),
            instrument_class,
            description: None,
            venue_mappings: HashMap::new(),
        }
    }

    /// Create a canonical symbol from asset and class, auto-generating the ID.
    ///
    /// # Example
    /// ```ignore
    /// let symbol = CanonicalSymbol::from_asset("BTC", "USD", InstrumentClass::Spot);
    /// assert_eq!(symbol.id, "BTC.SPOT");
    /// ```
    pub fn from_asset(
        asset: impl Into<String>,
        quote_currency: impl Into<String>,
        instrument_class: InstrumentClass,
    ) -> Self {
        let asset = asset.into();
        let id = format!("{}.{}", asset, instrument_class.as_suffix());
        Self {
            id,
            asset,
            quote_currency: quote_currency.into(),
            instrument_class,
            description: None,
            venue_mappings: HashMap::new(),
        }
    }

    /// Parse a canonical ID string into components.
    ///
    /// # Example
    /// ```ignore
    /// let (asset, class) = CanonicalSymbol::parse_id("BTC.SPOT")?;
    /// assert_eq!(asset, "BTC");
    /// assert_eq!(class, InstrumentClass::Spot);
    /// ```
    pub fn parse_id(id: &str) -> Option<(String, InstrumentClass)> {
        let parts: Vec<&str> = id.split('.').collect();
        if parts.len() != 2 {
            return None;
        }
        let asset = parts[0].to_string();
        let class = InstrumentClass::from_suffix(parts[1])?;
        Some((asset, class))
    }

    /// Add a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add a venue-specific raw_symbol mapping.
    pub fn with_venue_symbol(
        mut self,
        venue: impl Into<String>,
        raw_symbol: impl Into<String>,
    ) -> Self {
        self.venue_mappings.insert(
            venue.into(),
            VenueSymbolMapping {
                raw_symbol: raw_symbol.into(),
                ..Default::default()
            },
        );
        self
    }

    /// Add a venue-specific symbol with full mapping details.
    pub fn with_venue_mapping(
        mut self,
        venue: impl Into<String>,
        mapping: VenueSymbolMapping,
    ) -> Self {
        self.venue_mappings.insert(venue.into(), mapping);
        self
    }

    /// Get the raw_symbol for a specific venue.
    pub fn raw_symbol_for_venue(&self, venue: &str) -> Option<&str> {
        self.venue_mappings.get(venue).map(|m| m.raw_symbol.as_str())
    }

    /// Get all venues that support this symbol.
    pub fn available_venues(&self) -> impl Iterator<Item = &str> {
        self.venue_mappings.keys().map(|s| s.as_str())
    }
}

/// Detailed mapping information for a venue-specific symbol.
///
/// The `raw_symbol` field aligns with Databento's `raw_symbol` type -
/// it's the ticker as used by the exchange.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VenueSymbolMapping {
    /// The raw symbol as used by the venue (Databento: raw_symbol)
    /// Examples: "BTCUSDT", "XBT/USD", "ESH6", "PI_XBTUSD"
    pub raw_symbol: String,

    /// Venue's numeric instrument ID if available (Databento: instrument_id)
    #[serde(default)]
    pub instrument_id: Option<u64>,

    /// Venue's dataset identifier (Databento: dataset)
    /// Examples: "GLBX.MDP3", "XNAS.ITCH"
    #[serde(default)]
    pub dataset: Option<String>,

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

    /// Price tick size (Databento: min_price_increment)
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
    /// Create a new venue symbol mapping with raw_symbol.
    pub fn new(raw_symbol: impl Into<String>) -> Self {
        Self {
            raw_symbol: raw_symbol.into(),
            is_tradeable: true,
            ..Default::default()
        }
    }

    /// Set the instrument ID.
    pub fn with_instrument_id(mut self, id: u64) -> Self {
        self.instrument_id = Some(id);
        self
    }

    /// Set the dataset.
    pub fn with_dataset(mut self, dataset: impl Into<String>) -> Self {
        self.dataset = Some(dataset.into());
        self
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

/// Continuous contract symbol representation.
///
/// Aligns with Databento's continuous contract notation: `{asset}.{rule}.{rank}`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuousSymbol {
    /// Full continuous symbol (e.g., "ES.c.0")
    pub symbol: String,

    /// Base asset (e.g., "ES")
    pub asset: String,

    /// Roll method
    pub roll_method: ContinuousRollMethod,

    /// Rank in the roll chain (0 = front month)
    pub rank: u8,

    /// Currently mapped underlying canonical symbol
    pub current_underlying: Option<String>,
}

impl ContinuousSymbol {
    /// Create a new continuous symbol.
    pub fn new(asset: impl Into<String>, roll_method: ContinuousRollMethod, rank: u8) -> Self {
        let asset = asset.into();
        let symbol = roll_method.continuous_symbol(&asset, rank);
        Self {
            symbol,
            asset,
            roll_method,
            rank,
            current_underlying: None,
        }
    }

    /// Parse a continuous symbol string.
    ///
    /// # Example
    /// ```ignore
    /// let cont = ContinuousSymbol::parse("ES.c.0")?;
    /// assert_eq!(cont.asset, "ES");
    /// assert_eq!(cont.roll_method, ContinuousRollMethod::Calendar);
    /// assert_eq!(cont.rank, 0);
    /// ```
    pub fn parse(symbol: &str) -> Option<Self> {
        let parts: Vec<&str> = symbol.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        let asset = parts[0].to_string();
        let roll_method = ContinuousRollMethod::from_char(parts[1].chars().next()?)?;
        let rank = parts[2].parse::<u8>().ok()?;

        Some(Self {
            symbol: symbol.to_string(),
            asset,
            roll_method,
            rank,
            current_underlying: None,
        })
    }

    /// Set the current underlying contract.
    pub fn with_underlying(mut self, canonical_id: impl Into<String>) -> Self {
        self.current_underlying = Some(canonical_id.into());
        self
    }
}

/// Registry for canonical symbols and their venue mappings.
///
/// The registry provides bidirectional mapping:
/// - Canonical ID (DBT parent format) → Venue raw_symbol
/// - Venue raw_symbol → Canonical ID
///
/// # Symbol Conversion Flow
///
/// ```text
/// Strategy Layer:     Uses canonical IDs (e.g., "BTC.SPOT")
///       ↓
/// VenueRouter:        Routes by canonical ID
///       ↓
/// SymbolRegistry:     Converts canonical → raw_symbol
///       ↓
/// Venue Layer:        Uses raw_symbol (e.g., "BTCUSDT", "XBT/USD")
/// ```
#[derive(Debug, Clone, Default)]
pub struct SymbolRegistry {
    /// Canonical symbols by ID (DBT parent format)
    symbols: HashMap<String, CanonicalSymbol>,

    /// Reverse lookup: (venue, raw_symbol) -> canonical_id
    reverse_lookup: HashMap<(String, String), String>,

    /// Continuous contract mappings
    continuous_contracts: HashMap<String, ContinuousSymbol>,
}

impl SymbolRegistry {
    /// Create a new empty symbol registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry with common crypto symbols pre-registered.
    ///
    /// Uses DBT parent format for canonical IDs.
    pub fn with_crypto_defaults() -> Self {
        let mut registry = Self::new();

        // BTC Spot
        registry.register(
            CanonicalSymbol::from_asset("BTC", "USD", InstrumentClass::Spot)
                .with_description("Bitcoin Spot")
                .with_venue_symbol("BINANCE", "BTCUSDT")
                .with_venue_symbol("BINANCE_US", "BTCUSD")
                .with_venue_symbol("KRAKEN", "XBT/USD"),
        );

        // BTC Perpetual
        registry.register(
            CanonicalSymbol::from_asset("BTC", "USD", InstrumentClass::Perpetual)
                .with_description("Bitcoin Perpetual Futures")
                .with_venue_symbol("BINANCE_FUTURES", "BTCUSDT")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_XBTUSD"),
        );

        // ETH Spot
        registry.register(
            CanonicalSymbol::from_asset("ETH", "USD", InstrumentClass::Spot)
                .with_description("Ethereum Spot")
                .with_venue_symbol("BINANCE", "ETHUSDT")
                .with_venue_symbol("BINANCE_US", "ETHUSD")
                .with_venue_symbol("KRAKEN", "ETH/USD"),
        );

        // ETH Perpetual
        registry.register(
            CanonicalSymbol::from_asset("ETH", "USD", InstrumentClass::Perpetual)
                .with_description("Ethereum Perpetual Futures")
                .with_venue_symbol("BINANCE_FUTURES", "ETHUSDT")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_ETHUSD"),
        );

        // SOL Spot
        registry.register(
            CanonicalSymbol::from_asset("SOL", "USD", InstrumentClass::Spot)
                .with_description("Solana Spot")
                .with_venue_symbol("BINANCE", "SOLUSDT")
                .with_venue_symbol("BINANCE_US", "SOLUSD")
                .with_venue_symbol("KRAKEN", "SOL/USD"),
        );

        // SOL Perpetual
        registry.register(
            CanonicalSymbol::from_asset("SOL", "USD", InstrumentClass::Perpetual)
                .with_description("Solana Perpetual Futures")
                .with_venue_symbol("BINANCE_FUTURES", "SOLUSDT")
                .with_venue_symbol("KRAKEN_FUTURES", "PI_SOLUSD"),
        );

        registry
    }

    /// Register a canonical symbol.
    pub fn register(&mut self, symbol: CanonicalSymbol) {
        // Build reverse lookup entries
        for (venue, mapping) in &symbol.venue_mappings {
            self.reverse_lookup.insert(
                (venue.clone(), mapping.raw_symbol.clone()),
                symbol.id.clone(),
            );
        }

        self.symbols.insert(symbol.id.clone(), symbol);
    }

    /// Register a continuous contract.
    pub fn register_continuous(&mut self, continuous: ContinuousSymbol) {
        self.continuous_contracts
            .insert(continuous.symbol.clone(), continuous);
    }

    /// Get a canonical symbol by ID.
    pub fn get(&self, canonical_id: &str) -> Option<&CanonicalSymbol> {
        self.symbols.get(canonical_id)
    }

    /// Get a continuous contract by symbol.
    pub fn get_continuous(&self, symbol: &str) -> Option<&ContinuousSymbol> {
        self.continuous_contracts.get(symbol)
    }

    /// Convert canonical ID to venue raw_symbol.
    ///
    /// This is the primary conversion method - used when sending orders to venues.
    ///
    /// # Example
    /// ```ignore
    /// let raw = registry.to_raw_symbol("BTC.SPOT", "BINANCE");
    /// assert_eq!(raw, Some("BTCUSDT"));
    /// ```
    pub fn to_raw_symbol(&self, canonical_id: &str, venue: &str) -> Option<&str> {
        self.symbols
            .get(canonical_id)
            .and_then(|s| s.raw_symbol_for_venue(venue))
    }

    /// Convert venue raw_symbol to canonical ID.
    ///
    /// This is the reverse conversion - used when receiving data from venues.
    ///
    /// # Example
    /// ```ignore
    /// let canonical = registry.to_canonical("PI_XBTUSD", "KRAKEN_FUTURES");
    /// assert_eq!(canonical, Some("BTC.PERP"));
    /// ```
    pub fn to_canonical(&self, raw_symbol: &str, venue: &str) -> Option<&str> {
        self.reverse_lookup
            .get(&(venue.to_string(), raw_symbol.to_string()))
            .map(|s| s.as_str())
    }

    /// Get all canonical symbols.
    pub fn all_symbols(&self) -> impl Iterator<Item = &CanonicalSymbol> {
        self.symbols.values()
    }

    /// Get all continuous contracts.
    pub fn all_continuous(&self) -> impl Iterator<Item = &ContinuousSymbol> {
        self.continuous_contracts.values()
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
        self.to_raw_symbol(canonical_id, venue).is_some()
    }

    /// Get the full mapping for a symbol on a venue.
    pub fn get_mapping(&self, canonical_id: &str, venue: &str) -> Option<&VenueSymbolMapping> {
        self.symbols
            .get(canonical_id)
            .and_then(|s| s.venue_mappings.get(venue))
    }

    /// Find canonical symbols by asset.
    ///
    /// # Example
    /// ```ignore
    /// let btc_symbols = registry.find_by_asset("BTC");
    /// // Returns: ["BTC.SPOT", "BTC.PERP", ...]
    /// ```
    pub fn find_by_asset(&self, asset: &str) -> Vec<&str> {
        self.symbols
            .values()
            .filter(|s| s.asset.eq_ignore_ascii_case(asset))
            .map(|s| s.id.as_str())
            .collect()
    }

    /// Find canonical symbols by instrument class.
    pub fn find_by_class(&self, class: InstrumentClass) -> Vec<&str> {
        self.symbols
            .values()
            .filter(|s| s.instrument_class == class)
            .map(|s| s.id.as_str())
            .collect()
    }

    /// Resolve a continuous contract to its current underlying.
    ///
    /// Returns the canonical ID of the underlying contract.
    pub fn resolve_continuous(&self, continuous_symbol: &str) -> Option<&str> {
        self.continuous_contracts
            .get(continuous_symbol)
            .and_then(|c| c.current_underlying.as_deref())
    }

    // === Legacy compatibility methods (deprecated) ===

    /// Get the venue-specific symbol for a canonical symbol.
    ///
    /// **Deprecated**: Use `to_raw_symbol` instead.
    #[deprecated(since = "0.2.0", note = "Use to_raw_symbol instead")]
    pub fn venue_symbol(&self, canonical_id: &str, venue: &str) -> Option<&str> {
        self.to_raw_symbol(canonical_id, venue)
    }

    /// Get the canonical symbol ID from a venue-specific symbol.
    ///
    /// **Deprecated**: Use `to_canonical` instead.
    #[deprecated(since = "0.2.0", note = "Use to_canonical instead")]
    pub fn canonical_symbol(&self, venue_symbol: &str, venue: &str) -> Option<&str> {
        self.to_canonical(venue_symbol, venue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instrument_class() {
        assert_eq!(InstrumentClass::Spot.as_char(), 'S');
        assert_eq!(InstrumentClass::Future.as_suffix(), "FUT");
        assert_eq!(InstrumentClass::from_suffix("PERP"), Some(InstrumentClass::Perpetual));
        assert_eq!(InstrumentClass::from_suffix("invalid"), None);
    }

    #[test]
    fn test_continuous_roll_method() {
        assert_eq!(ContinuousRollMethod::Calendar.as_char(), 'c');
        assert_eq!(ContinuousRollMethod::Volume.as_char(), 'v');
        assert_eq!(ContinuousRollMethod::OpenInterest.as_char(), 'n');

        assert_eq!(
            ContinuousRollMethod::Calendar.continuous_symbol("ES", 0),
            "ES.c.0"
        );
        assert_eq!(
            ContinuousRollMethod::Volume.continuous_symbol("CL", 1),
            "CL.v.1"
        );
    }

    #[test]
    fn test_canonical_symbol_creation() {
        let symbol = CanonicalSymbol::from_asset("BTC", "USD", InstrumentClass::Spot)
            .with_venue_symbol("BINANCE", "BTCUSDT")
            .with_venue_symbol("KRAKEN", "XBT/USD");

        assert_eq!(symbol.id, "BTC.SPOT");
        assert_eq!(symbol.asset, "BTC");
        assert_eq!(symbol.quote_currency, "USD");
        assert_eq!(symbol.instrument_class, InstrumentClass::Spot);
        assert_eq!(symbol.raw_symbol_for_venue("BINANCE"), Some("BTCUSDT"));
        assert_eq!(symbol.raw_symbol_for_venue("KRAKEN"), Some("XBT/USD"));
        assert_eq!(symbol.raw_symbol_for_venue("UNKNOWN"), None);
    }

    #[test]
    fn test_canonical_symbol_parse_id() {
        let (asset, class) = CanonicalSymbol::parse_id("BTC.SPOT").unwrap();
        assert_eq!(asset, "BTC");
        assert_eq!(class, InstrumentClass::Spot);

        let (asset, class) = CanonicalSymbol::parse_id("ES.FUT").unwrap();
        assert_eq!(asset, "ES");
        assert_eq!(class, InstrumentClass::Future);

        assert!(CanonicalSymbol::parse_id("INVALID").is_none());
        assert!(CanonicalSymbol::parse_id("BTC.UNKNOWN").is_none());
    }

    #[test]
    fn test_continuous_symbol_parse() {
        let cont = ContinuousSymbol::parse("ES.c.0").unwrap();
        assert_eq!(cont.asset, "ES");
        assert_eq!(cont.roll_method, ContinuousRollMethod::Calendar);
        assert_eq!(cont.rank, 0);

        let cont = ContinuousSymbol::parse("CL.v.1").unwrap();
        assert_eq!(cont.asset, "CL");
        assert_eq!(cont.roll_method, ContinuousRollMethod::Volume);
        assert_eq!(cont.rank, 1);

        let cont = ContinuousSymbol::parse("GC.n.2").unwrap();
        assert_eq!(cont.asset, "GC");
        assert_eq!(cont.roll_method, ContinuousRollMethod::OpenInterest);
        assert_eq!(cont.rank, 2);

        assert!(ContinuousSymbol::parse("INVALID").is_none());
        assert!(ContinuousSymbol::parse("ES.x.0").is_none()); // Invalid roll method
    }

    #[test]
    fn test_symbol_registry() {
        let mut registry = SymbolRegistry::new();

        registry.register(
            CanonicalSymbol::from_asset("BTC", "USD", InstrumentClass::Spot)
                .with_venue_symbol("BINANCE", "BTCUSDT")
                .with_venue_symbol("KRAKEN", "XBT/USD"),
        );

        registry.register(
            CanonicalSymbol::from_asset("BTC", "USD", InstrumentClass::Perpetual)
                .with_venue_symbol("KRAKEN_FUTURES", "PI_XBTUSD"),
        );

        // Forward lookup
        assert_eq!(registry.to_raw_symbol("BTC.SPOT", "BINANCE"), Some("BTCUSDT"));
        assert_eq!(registry.to_raw_symbol("BTC.SPOT", "KRAKEN"), Some("XBT/USD"));
        assert_eq!(
            registry.to_raw_symbol("BTC.PERP", "KRAKEN_FUTURES"),
            Some("PI_XBTUSD")
        );

        // Reverse lookup
        assert_eq!(registry.to_canonical("BTCUSDT", "BINANCE"), Some("BTC.SPOT"));
        assert_eq!(registry.to_canonical("XBT/USD", "KRAKEN"), Some("BTC.SPOT"));
        assert_eq!(
            registry.to_canonical("PI_XBTUSD", "KRAKEN_FUTURES"),
            Some("BTC.PERP")
        );
    }

    #[test]
    fn test_crypto_defaults() {
        let registry = SymbolRegistry::with_crypto_defaults();

        // Check BTC spot mappings
        assert_eq!(registry.to_raw_symbol("BTC.SPOT", "BINANCE"), Some("BTCUSDT"));
        assert_eq!(registry.to_raw_symbol("BTC.SPOT", "KRAKEN"), Some("XBT/USD"));

        // Check BTC perpetual mappings
        assert_eq!(
            registry.to_raw_symbol("BTC.PERP", "BINANCE_FUTURES"),
            Some("BTCUSDT")
        );
        assert_eq!(
            registry.to_raw_symbol("BTC.PERP", "KRAKEN_FUTURES"),
            Some("PI_XBTUSD")
        );

        // Check ETH mappings
        assert_eq!(registry.to_raw_symbol("ETH.SPOT", "BINANCE"), Some("ETHUSDT"));
        assert_eq!(
            registry.to_raw_symbol("ETH.PERP", "KRAKEN_FUTURES"),
            Some("PI_ETHUSD")
        );

        // Reverse lookups
        assert_eq!(registry.to_canonical("BTCUSDT", "BINANCE"), Some("BTC.SPOT"));
        assert_eq!(
            registry.to_canonical("PI_XBTUSD", "KRAKEN_FUTURES"),
            Some("BTC.PERP")
        );
    }

    #[test]
    fn test_find_by_asset() {
        let registry = SymbolRegistry::with_crypto_defaults();

        let btc_symbols = registry.find_by_asset("BTC");
        assert!(btc_symbols.contains(&"BTC.SPOT"));
        assert!(btc_symbols.contains(&"BTC.PERP"));
        assert_eq!(btc_symbols.len(), 2);

        let eth_symbols = registry.find_by_asset("ETH");
        assert!(eth_symbols.contains(&"ETH.SPOT"));
        assert!(eth_symbols.contains(&"ETH.PERP"));
    }

    #[test]
    fn test_find_by_class() {
        let registry = SymbolRegistry::with_crypto_defaults();

        let spots = registry.find_by_class(InstrumentClass::Spot);
        assert!(spots.contains(&"BTC.SPOT"));
        assert!(spots.contains(&"ETH.SPOT"));
        assert!(spots.contains(&"SOL.SPOT"));

        let perps = registry.find_by_class(InstrumentClass::Perpetual);
        assert!(perps.contains(&"BTC.PERP"));
        assert!(perps.contains(&"ETH.PERP"));
        assert!(perps.contains(&"SOL.PERP"));
    }

    #[test]
    fn test_venues_for_symbol() {
        let registry = SymbolRegistry::with_crypto_defaults();

        let venues = registry.venues_for_symbol("BTC.SPOT");
        assert!(venues.contains(&"BINANCE"));
        assert!(venues.contains(&"BINANCE_US"));
        assert!(venues.contains(&"KRAKEN"));
        assert_eq!(venues.len(), 3);

        let venues = registry.venues_for_symbol("BTC.PERP");
        assert!(venues.contains(&"BINANCE_FUTURES"));
        assert!(venues.contains(&"KRAKEN_FUTURES"));
        assert_eq!(venues.len(), 2);
    }

    #[test]
    fn test_is_available() {
        let registry = SymbolRegistry::with_crypto_defaults();

        assert!(registry.is_available("BTC.SPOT", "BINANCE"));
        assert!(registry.is_available("BTC.PERP", "KRAKEN_FUTURES"));
        assert!(!registry.is_available("BTC.SPOT", "KRAKEN_FUTURES")); // Spot not on futures venue
        assert!(!registry.is_available("BTC.PERP", "BINANCE")); // Perp not on spot venue
        assert!(!registry.is_available("UNKNOWN.SPOT", "BINANCE"));
    }

    #[test]
    fn test_continuous_contract_registration() {
        let mut registry = SymbolRegistry::new();

        // Register underlying
        registry.register(
            CanonicalSymbol::new("ESH6.FUT", "ES", "USD", InstrumentClass::Future)
                .with_venue_symbol("CME", "ESH6"),
        );

        // Register continuous contract
        let continuous =
            ContinuousSymbol::new("ES", ContinuousRollMethod::Calendar, 0).with_underlying("ESH6.FUT");
        registry.register_continuous(continuous);

        // Lookup continuous
        let cont = registry.get_continuous("ES.c.0").unwrap();
        assert_eq!(cont.asset, "ES");
        assert_eq!(cont.roll_method, ContinuousRollMethod::Calendar);
        assert_eq!(cont.rank, 0);

        // Resolve to underlying
        assert_eq!(registry.resolve_continuous("ES.c.0"), Some("ESH6.FUT"));
    }
}
