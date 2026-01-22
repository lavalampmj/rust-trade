//! Instrument types and classifications for multi-asset support.
//!
//! This module defines asset types and instrument classifications:
//! - `AssetClass` - High-level asset classification (Equity, Crypto, FX, etc.)
//! - `InstrumentClass` - Specific instrument type (Spot, Futures, Options, etc.)
//! - `SecurityType` - Combined classification (alias for InstrumentClass)
//! - `OptionType` - Call or Put for options

use serde::{Deserialize, Serialize};
use std::fmt;

/// High-level asset class classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetClass {
    /// Cryptocurrency assets (BTC, ETH, etc.)
    Crypto,
    /// Foreign exchange (currency pairs)
    FX,
    /// Equity/Stock assets
    Equity,
    /// Commodities (gold, oil, etc.)
    Commodity,
    /// Fixed income / bonds
    Bond,
    /// Index-based instruments
    Index,
    /// Alternative/Other assets
    Alternative,
}

impl AssetClass {
    /// Returns true if this is a 24/7 trading market
    pub fn is_24_7(&self) -> bool {
        matches!(self, AssetClass::Crypto)
    }

    /// Returns true if this is a fiat-based market
    pub fn is_fiat_based(&self) -> bool {
        matches!(
            self,
            AssetClass::FX | AssetClass::Equity | AssetClass::Bond | AssetClass::Commodity
        )
    }
}

impl fmt::Display for AssetClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetClass::Crypto => write!(f, "CRYPTO"),
            AssetClass::FX => write!(f, "FX"),
            AssetClass::Equity => write!(f, "EQUITY"),
            AssetClass::Commodity => write!(f, "COMMODITY"),
            AssetClass::Bond => write!(f, "BOND"),
            AssetClass::Index => write!(f, "INDEX"),
            AssetClass::Alternative => write!(f, "ALTERNATIVE"),
        }
    }
}

impl Default for AssetClass {
    fn default() -> Self {
        AssetClass::Crypto
    }
}

/// Specific instrument/security classification.
///
/// Defines the specific type of tradable instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstrumentClass {
    /// Spot/cash market instrument
    Spot,
    /// Linear perpetual futures (settled in quote currency, e.g., USDT)
    PerpetualLinear,
    /// Inverse perpetual futures (settled in base currency, e.g., BTC)
    PerpetualInverse,
    /// Dated futures contract with expiry
    Future,
    /// Options contract
    Option,
    /// Betting/prediction market
    Betting,
    /// Contract for Difference
    CFD,
    /// Warrant
    Warrant,
}

impl InstrumentClass {
    /// Returns true if this is a derivative instrument
    pub fn is_derivative(&self) -> bool {
        !matches!(self, InstrumentClass::Spot)
    }

    /// Returns true if this is a perpetual instrument (no expiry)
    pub fn is_perpetual(&self) -> bool {
        matches!(
            self,
            InstrumentClass::Spot
                | InstrumentClass::PerpetualLinear
                | InstrumentClass::PerpetualInverse
        )
    }

    /// Returns true if this instrument has leverage
    pub fn has_leverage(&self) -> bool {
        matches!(
            self,
            InstrumentClass::PerpetualLinear
                | InstrumentClass::PerpetualInverse
                | InstrumentClass::Future
                | InstrumentClass::Option
                | InstrumentClass::CFD
        )
    }

    /// Returns true if this is a linear instrument (settled in quote currency)
    pub fn is_linear(&self) -> bool {
        matches!(
            self,
            InstrumentClass::Spot | InstrumentClass::PerpetualLinear | InstrumentClass::Future
        )
    }

    /// Returns true if this is an inverse instrument (settled in base currency)
    pub fn is_inverse(&self) -> bool {
        matches!(self, InstrumentClass::PerpetualInverse)
    }
}

impl fmt::Display for InstrumentClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstrumentClass::Spot => write!(f, "SPOT"),
            InstrumentClass::PerpetualLinear => write!(f, "PERPETUAL_LINEAR"),
            InstrumentClass::PerpetualInverse => write!(f, "PERPETUAL_INVERSE"),
            InstrumentClass::Future => write!(f, "FUTURE"),
            InstrumentClass::Option => write!(f, "OPTION"),
            InstrumentClass::Betting => write!(f, "BETTING"),
            InstrumentClass::CFD => write!(f, "CFD"),
            InstrumentClass::Warrant => write!(f, "WARRANT"),
        }
    }
}

impl Default for InstrumentClass {
    fn default() -> Self {
        InstrumentClass::Spot
    }
}

/// Alias for InstrumentClass for compatibility with other trading systems.
pub type SecurityType = InstrumentClass;

/// Option type for options contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OptionType {
    /// Call option - right to buy
    Call,
    /// Put option - right to sell
    Put,
}

impl fmt::Display for OptionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OptionType::Call => write!(f, "CALL"),
            OptionType::Put => write!(f, "PUT"),
        }
    }
}

/// Currency type for settlement and margin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Currency {
    /// Currency code (e.g., "USD", "BTC", "USDT")
    pub code: String,
    /// Number of decimal places for precision
    pub precision: u8,
    /// Currency type classification
    pub currency_type: CurrencyType,
}

impl Currency {
    /// Create a new Currency
    pub fn new(code: impl Into<String>, precision: u8, currency_type: CurrencyType) -> Self {
        Self {
            code: code.into(),
            precision,
            currency_type,
        }
    }

    /// Create a fiat currency
    pub fn fiat(code: impl Into<String>, precision: u8) -> Self {
        Self::new(code, precision, CurrencyType::Fiat)
    }

    /// Create a cryptocurrency
    pub fn crypto(code: impl Into<String>, precision: u8) -> Self {
        Self::new(code, precision, CurrencyType::Crypto)
    }

    /// Common currencies
    pub fn usd() -> Self {
        Self::fiat("USD", 2)
    }

    pub fn usdt() -> Self {
        Self::crypto("USDT", 8)
    }

    pub fn btc() -> Self {
        Self::crypto("BTC", 8)
    }

    pub fn eth() -> Self {
        Self::crypto("ETH", 8)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)
    }
}

/// Currency type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CurrencyType {
    /// Fiat currency (USD, EUR, etc.)
    Fiat,
    /// Cryptocurrency
    Crypto,
    /// Stablecoin (USDT, USDC, etc.)
    Stablecoin,
    /// Commodity-backed (gold, etc.)
    CommodityBacked,
}

impl Default for CurrencyType {
    fn default() -> Self {
        CurrencyType::Crypto
    }
}

impl fmt::Display for CurrencyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CurrencyType::Fiat => write!(f, "FIAT"),
            CurrencyType::Crypto => write!(f, "CRYPTO"),
            CurrencyType::Stablecoin => write!(f, "STABLECOIN"),
            CurrencyType::CommodityBacked => write!(f, "COMMODITY_BACKED"),
        }
    }
}

/// Venue/Exchange identifier with type safety.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Venue {
    /// Venue identifier code (e.g., "BINANCE", "KRAKEN")
    pub code: String,
    /// Venue type
    pub venue_type: VenueType,
}

impl Venue {
    /// Create a new Venue
    pub fn new(code: impl Into<String>, venue_type: VenueType) -> Self {
        Self {
            code: code.into(),
            venue_type,
        }
    }

    /// Create a crypto exchange venue
    pub fn crypto_exchange(code: impl Into<String>) -> Self {
        Self::new(code, VenueType::Exchange)
    }

    /// Common venues
    pub fn binance() -> Self {
        Self::crypto_exchange("BINANCE")
    }

    pub fn binance_futures() -> Self {
        Self::new("BINANCE_FUTURES", VenueType::Exchange)
    }

    pub fn kraken() -> Self {
        Self::crypto_exchange("KRAKEN")
    }

    pub fn coinbase() -> Self {
        Self::crypto_exchange("COINBASE")
    }

    /// Simulated/backtest venue
    pub fn simulated() -> Self {
        Self::new("SIMULATED", VenueType::Simulated)
    }
}

impl fmt::Display for Venue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)
    }
}

impl Default for Venue {
    fn default() -> Self {
        Self::binance()
    }
}

/// Venue type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VenueType {
    /// Centralized exchange
    Exchange,
    /// Decentralized exchange
    DEX,
    /// Over-the-counter
    OTC,
    /// Dark pool
    DarkPool,
    /// Simulated/paper trading
    Simulated,
}

impl Default for VenueType {
    fn default() -> Self {
        VenueType::Exchange
    }
}

impl fmt::Display for VenueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VenueType::Exchange => write!(f, "EXCHANGE"),
            VenueType::DEX => write!(f, "DEX"),
            VenueType::OTC => write!(f, "OTC"),
            VenueType::DarkPool => write!(f, "DARK_POOL"),
            VenueType::Simulated => write!(f, "SIMULATED"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_class_properties() {
        assert!(AssetClass::Crypto.is_24_7());
        assert!(!AssetClass::Equity.is_24_7());
        assert!(AssetClass::FX.is_fiat_based());
        assert!(!AssetClass::Crypto.is_fiat_based());
    }

    #[test]
    fn test_instrument_class_properties() {
        assert!(!InstrumentClass::Spot.is_derivative());
        assert!(InstrumentClass::Future.is_derivative());
        assert!(InstrumentClass::PerpetualLinear.is_perpetual());
        assert!(!InstrumentClass::Future.is_perpetual());
        assert!(InstrumentClass::PerpetualLinear.has_leverage());
        assert!(!InstrumentClass::Spot.has_leverage());
    }

    #[test]
    fn test_currency_creation() {
        let usd = Currency::usd();
        assert_eq!(usd.code, "USD");
        assert_eq!(usd.precision, 2);
        assert_eq!(usd.currency_type, CurrencyType::Fiat);

        let btc = Currency::btc();
        assert_eq!(btc.code, "BTC");
        assert_eq!(btc.precision, 8);
        assert_eq!(btc.currency_type, CurrencyType::Crypto);
    }

    #[test]
    fn test_venue_creation() {
        let binance = Venue::binance();
        assert_eq!(binance.code, "BINANCE");
        assert_eq!(binance.venue_type, VenueType::Exchange);

        let sim = Venue::simulated();
        assert_eq!(sim.venue_type, VenueType::Simulated);
    }

    #[test]
    fn test_security_type_alias() {
        // SecurityType is an alias for InstrumentClass
        let st: SecurityType = InstrumentClass::Spot;
        assert!(!st.is_derivative());
    }
}
