//! Symbol definition types aligned with Databento's InstrumentDefMsg schema.
//!
//! This module provides comprehensive symbol metadata for all tradeable instruments,
//! including trading specifications, venue configuration, and provider mappings.
//!
//! # Databento Alignment
//!
//! The structures in this module align with Databento's symbology conventions:
//! - `raw_symbol`: Publisher's original symbol (e.g., "ESH6", "BTCUSDT")
//! - `instrument_id`: Numeric publisher ID
//! - Provider mappings for multi-data-source support
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::{
//!     SymbolDefinition, SymbolInfo, VenueConfig, TradingSpecs,
//! };
//! use rust_decimal_macros::dec;
//!
//! let btcusdt = SymbolDefinition {
//!     id: InstrumentId::new("BTCUSDT", "BINANCE"),
//!     raw_symbol: "BTCUSDT".to_string(),
//!     info: SymbolInfo { ... },
//!     venue_config: VenueConfig { ... },
//!     trading_specs: TradingSpecs::default(),
//!     ..Default::default()
//! };
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use super::session::SessionSchedule;
use super::contract::ContractSpec;
use super::types::{AssetClass, Currency, InstrumentClass, Venue, VenueType};
use crate::orders::{InstrumentId, OrderType};

/// Complete definition of a tradeable symbol.
///
/// This is the central struct containing all metadata needed to trade a symbol,
/// aligned with Databento's InstrumentDefMsg schema (70 fields).
///
/// # Fields
///
/// - `id`: Unique identifier in "{symbol}.{venue}" format
/// - `instrument_id`: Numeric ID from the publisher
/// - `raw_symbol`: Publisher's original ticker symbol
/// - `info`: Basic symbol information (asset class, currencies)
/// - `venue_config`: Venue-specific configuration
/// - `trading_specs`: Trading constraints and fees
/// - `session_schedule`: Market hours (None for 24/7 markets)
/// - `contract_spec`: Derivative contract details (futures/options only)
/// - `provider_mappings`: Symbol mappings per data provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefinition {
    /// Unique identifier: "{raw_symbol}.{venue}"
    /// Format matches Databento: symbol + MIC code
    pub id: InstrumentId,

    /// Numeric instrument ID (Databento: instrument_id)
    /// Unique numeric identifier assigned by publisher
    #[serde(default)]
    pub instrument_id: u64,

    /// Raw symbol as used by the publisher (Databento: raw_symbol)
    /// Examples: "ESH6", "AAPL", "BTCUSDT"
    pub raw_symbol: String,

    /// Basic symbol information
    pub info: SymbolInfo,

    /// Venue-specific configuration
    pub venue_config: VenueConfig,

    /// Trading specifications
    pub trading_specs: TradingSpecs,

    /// Session schedule (None for 24/7 markets like crypto)
    #[serde(default)]
    pub session_schedule: Option<SessionSchedule>,

    /// Contract specifications (futures/options only)
    #[serde(default)]
    pub contract_spec: Option<ContractSpec>,

    /// Provider-specific symbol mappings
    /// Key: provider name (e.g., "databento", "binance")
    /// Value: provider-specific symbol info
    #[serde(default)]
    pub provider_mappings: HashMap<String, ProviderSymbol>,

    /// Symbol status
    #[serde(default)]
    pub status: SymbolStatus,

    /// Timestamp when data was received (Databento: ts_recv)
    #[serde(default = "Utc::now")]
    pub ts_recv: DateTime<Utc>,

    /// Creation timestamp
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

impl SymbolDefinition {
    /// Create a new SymbolDefinition with minimal required fields
    pub fn new(
        symbol: impl Into<String>,
        venue: impl Into<String>,
        info: SymbolInfo,
        venue_config: VenueConfig,
        trading_specs: TradingSpecs,
    ) -> Self {
        let symbol = symbol.into();
        let venue = venue.into();
        let raw_symbol = symbol.clone();

        Self {
            id: InstrumentId::new(&symbol, &venue),
            instrument_id: 0,
            raw_symbol,
            info,
            venue_config,
            trading_specs,
            session_schedule: None,
            contract_spec: None,
            provider_mappings: HashMap::new(),
            status: SymbolStatus::Active,
            ts_recv: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Check if this is a 24/7 market (no session schedule)
    pub fn is_24_7(&self) -> bool {
        self.session_schedule.is_none() || self.info.asset_class.is_24_7()
    }

    /// Check if this is a derivative instrument
    pub fn is_derivative(&self) -> bool {
        self.contract_spec.is_some()
    }

    /// Get the symbol for a specific provider
    pub fn get_provider_symbol(&self, provider: &str) -> Option<&str> {
        self.provider_mappings
            .get(provider)
            .map(|p| p.symbol.as_str())
    }

    /// Add a provider mapping
    pub fn with_provider_mapping(mut self, provider: impl Into<String>, symbol: ProviderSymbol) -> Self {
        self.provider_mappings.insert(provider.into(), symbol);
        self
    }

    /// Add a session schedule
    pub fn with_session_schedule(mut self, schedule: SessionSchedule) -> Self {
        self.session_schedule = Some(schedule);
        self
    }

    /// Add a contract specification
    pub fn with_contract_spec(mut self, spec: ContractSpec) -> Self {
        self.contract_spec = Some(spec);
        self
    }
}

impl fmt::Display for SymbolDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} {} on {})",
            self.id, self.info.asset_class, self.info.instrument_class, self.venue_config.venue
        )
    }
}

/// Provider-specific symbol mapping.
///
/// Maps our canonical symbol to provider-specific identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSymbol {
    /// Symbol as used by provider
    pub symbol: String,

    /// Provider's dataset identifier (e.g., "GLBX.MDP3")
    #[serde(default)]
    pub dataset: Option<String>,

    /// Provider's instrument ID if numeric
    #[serde(default)]
    pub instrument_id: Option<u64>,

    /// Additional provider-specific metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl ProviderSymbol {
    /// Create a simple provider symbol mapping
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            dataset: None,
            instrument_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Create with dataset (Databento format)
    pub fn with_dataset(mut self, dataset: impl Into<String>) -> Self {
        self.dataset = Some(dataset.into());
        self
    }

    /// Create with instrument ID
    pub fn with_instrument_id(mut self, id: u64) -> Self {
        self.instrument_id = Some(id);
        self
    }
}

/// Symbol status enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SymbolStatus {
    /// Symbol is actively trading
    #[default]
    Active,
    /// Symbol is temporarily halted
    Halted,
    /// Symbol is suspended
    Suspended,
    /// Symbol has been delisted
    Delisted,
    /// Symbol is pending listing
    Pending,
    /// Symbol is expired (for derivatives)
    Expired,
}

impl fmt::Display for SymbolStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolStatus::Active => write!(f, "ACTIVE"),
            SymbolStatus::Halted => write!(f, "HALTED"),
            SymbolStatus::Suspended => write!(f, "SUSPENDED"),
            SymbolStatus::Delisted => write!(f, "DELISTED"),
            SymbolStatus::Pending => write!(f, "PENDING"),
            SymbolStatus::Expired => write!(f, "EXPIRED"),
        }
    }
}

/// Basic symbol information.
///
/// Aligns with Databento InstrumentDefMsg fields for asset classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInfo {
    /// Product root symbol (Databento: asset[11])
    /// Examples: "ES" for E-mini S&P, "CL" for Crude Oil, "BTC" for Bitcoin
    pub asset: String,

    /// Asset class
    pub asset_class: AssetClass,

    /// Instrument class (Databento: instrument_class char)
    /// 'F' = Future, 'O' = Option, 'S' = Spot, 'P' = Perpetual
    pub instrument_class: InstrumentClass,

    /// Security type string (Databento: security_type[7])
    /// Examples: "FUT", "OPT", "SPOT", "PERP"
    #[serde(default)]
    pub security_type: String,

    /// CFI code (Databento: cfi[7]) - ISO 10962 classification
    /// Examples: "FXXXXX" for futures, "OCASPS" for options
    #[serde(default)]
    pub cfi_code: Option<String>,

    /// Base currency/asset
    pub base_currency: Currency,

    /// Quote/settlement currency (Databento: currency[4])
    /// ISO 4217 code: "USD", "EUR", "BTC"
    pub quote_currency: Currency,

    /// Settlement currency if different (Databento: settl_currency[4])
    #[serde(default)]
    pub settlement_currency: Option<Currency>,

    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,

    /// ISIN if available (ISO 6166)
    #[serde(default)]
    pub isin: Option<String>,

    /// Security sub-type (Databento: secsubtype[6])
    #[serde(default)]
    pub security_subtype: Option<String>,

    /// Market segment ID (Databento: market_segment_id)
    #[serde(default)]
    pub market_segment_id: Option<u32>,

    /// Product group (Databento: group[21])
    /// Grouping for related products
    #[serde(default)]
    pub group: Option<String>,
}

impl SymbolInfo {
    /// Create a new SymbolInfo with required fields
    pub fn new(
        asset: impl Into<String>,
        asset_class: AssetClass,
        instrument_class: InstrumentClass,
        base_currency: Currency,
        quote_currency: Currency,
    ) -> Self {
        let asset = asset.into();
        let security_type = match instrument_class {
            InstrumentClass::Spot => "SPOT".to_string(),
            InstrumentClass::Future => "FUT".to_string(),
            InstrumentClass::Option => "OPT".to_string(),
            InstrumentClass::PerpetualLinear | InstrumentClass::PerpetualInverse => "PERP".to_string(),
            _ => "OTHER".to_string(),
        };

        Self {
            asset,
            asset_class,
            instrument_class,
            security_type,
            cfi_code: None,
            base_currency,
            quote_currency,
            settlement_currency: None,
            description: None,
            isin: None,
            security_subtype: None,
            market_segment_id: None,
            group: None,
        }
    }

    /// Create for a crypto spot pair
    pub fn crypto_spot(base: impl Into<String>, quote: impl Into<String>) -> Self {
        let base_str: String = base.into();
        let quote_str: String = quote.into();
        Self::new(
            base_str.clone(),
            AssetClass::Crypto,
            InstrumentClass::Spot,
            Currency::crypto(&base_str, 8),
            Currency::crypto(&quote_str, 8),
        )
    }

    /// Create for a crypto perpetual
    pub fn crypto_perpetual(base: impl Into<String>, quote: impl Into<String>, linear: bool) -> Self {
        let base_str: String = base.into();
        let quote_str: String = quote.into();
        let instrument_class = if linear {
            InstrumentClass::PerpetualLinear
        } else {
            InstrumentClass::PerpetualInverse
        };
        Self::new(
            base_str.clone(),
            AssetClass::Crypto,
            instrument_class,
            Currency::crypto(&base_str, 8),
            Currency::crypto(&quote_str, 8),
        )
    }

    /// Add a description
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

/// Venue-specific configuration.
///
/// Aligns with Databento's publisher/dataset model for venue identification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenueConfig {
    /// Venue identifier
    pub venue: Venue,

    /// Market Identifier Code (ISO 10383) (Databento: exchange[5])
    /// Examples: "XCME", "XNAS", "XNYS", "GLBX"
    pub mic_code: String,

    /// Dataset identifier (Databento format: "{VENUE}.{FEED}")
    /// Examples: "GLBX.MDP3", "XNAS.ITCH", "IFEU.IMPACT"
    #[serde(default)]
    pub dataset: Option<String>,

    /// Publisher ID (Databento: publisher_id)
    /// Unique ID for the data publisher
    #[serde(default)]
    pub publisher_id: Option<u16>,

    /// Country code (ISO 3166-1 alpha-2)
    #[serde(default)]
    pub country: Option<String>,

    /// Primary listing flag
    #[serde(default = "default_true")]
    pub is_primary_listing: bool,

    /// Venue-specific symbol (may differ from canonical)
    pub venue_symbol: String,

    /// Channel ID for market data (Databento: channel_id)
    #[serde(default)]
    pub channel_id: Option<u16>,

    /// API rate limits for this venue
    #[serde(default)]
    pub rate_limits: Option<RateLimits>,
}

fn default_true() -> bool {
    true
}

impl VenueConfig {
    /// Create a new VenueConfig with required fields
    pub fn new(venue: Venue, mic_code: impl Into<String>, venue_symbol: impl Into<String>) -> Self {
        Self {
            venue,
            mic_code: mic_code.into(),
            dataset: None,
            publisher_id: None,
            country: None,
            is_primary_listing: true,
            venue_symbol: venue_symbol.into(),
            channel_id: None,
            rate_limits: None,
        }
    }

    /// Create for Binance
    pub fn binance(symbol: impl Into<String>) -> Self {
        Self::new(Venue::binance(), mic_codes::BINANCE, symbol)
    }

    /// Create for Binance Futures
    pub fn binance_futures(symbol: impl Into<String>) -> Self {
        Self::new(Venue::binance_futures(), mic_codes::BINANCE, symbol)
    }

    /// Create for CME Globex
    pub fn cme_globex(symbol: impl Into<String>) -> Self {
        Self::new(
            Venue::new("GLBX", VenueType::Exchange),
            mic_codes::CME_GLOBEX,
            symbol,
        )
        .with_dataset("GLBX.MDP3")
        .with_country("US")
    }

    /// Add dataset identifier
    pub fn with_dataset(mut self, dataset: impl Into<String>) -> Self {
        self.dataset = Some(dataset.into());
        self
    }

    /// Add country code
    pub fn with_country(mut self, country: impl Into<String>) -> Self {
        self.country = Some(country.into());
        self
    }

    /// Add publisher ID
    pub fn with_publisher_id(mut self, id: u16) -> Self {
        self.publisher_id = Some(id);
        self
    }
}

/// Standard MIC codes for common venues.
pub mod mic_codes {
    /// CME Globex
    pub const CME_GLOBEX: &str = "GLBX";
    /// Chicago Mercantile Exchange
    pub const CME: &str = "XCME";
    /// Chicago Board of Trade
    pub const CBOT: &str = "XCBT";
    /// New York Mercantile Exchange
    pub const NYMEX: &str = "XNYM";
    /// COMEX (Metals)
    pub const COMEX: &str = "XCEC";
    /// NASDAQ
    pub const NASDAQ: &str = "XNAS";
    /// New York Stock Exchange
    pub const NYSE: &str = "XNYS";
    /// NYSE Arca
    pub const NYSE_ARCA: &str = "ARCX";
    /// ICE Futures Europe
    pub const ICE_EU: &str = "IFEU";
    /// ICE Futures US
    pub const ICE_US: &str = "IFUS";
    /// Binance (crypto venues use informal codes)
    pub const BINANCE: &str = "BINC";
    /// Coinbase
    pub const COINBASE: &str = "COIN";
    /// Kraken
    pub const KRAKEN: &str = "KRKN";
}

/// API rate limits for a venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimits {
    /// Maximum requests per second
    #[serde(default)]
    pub requests_per_second: Option<u32>,
    /// Maximum requests per minute
    #[serde(default)]
    pub requests_per_minute: Option<u32>,
    /// Maximum orders per second
    #[serde(default)]
    pub orders_per_second: Option<u32>,
    /// Maximum orders per minute
    #[serde(default)]
    pub orders_per_minute: Option<u32>,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            requests_per_second: None,
            requests_per_minute: Some(1200),
            orders_per_second: Some(10),
            orders_per_minute: Some(100),
        }
    }
}

/// Trading specifications and constraints.
///
/// Aligns with Databento InstrumentDefMsg pricing fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingSpecs {
    /// Minimum price increment (Databento: min_price_increment)
    /// The tick size in price units
    pub min_price_increment: Decimal,

    /// Display factor for price conversion (Databento: display_factor)
    /// Price = raw_price * display_factor
    #[serde(default = "default_one")]
    pub display_factor: Decimal,

    /// Price display format (Databento: price_display_format)
    /// 0 = decimal, 1 = fractional 1/2, 2 = fractional 1/4, etc.
    #[serde(default)]
    pub price_display_format: u8,

    /// Variable tick size rule (Databento: tick_rule)
    /// References VTT (Variable Tick Table) code
    #[serde(default)]
    pub tick_rule: Option<u8>,

    /// Minimum lot size for regular orders (Databento: min_lot_size)
    pub min_lot_size: Decimal,

    /// Minimum lot size for block trades (Databento: min_lot_size_block)
    #[serde(default)]
    pub min_lot_size_block: Option<Decimal>,

    /// Minimum lot size for round lot (Databento: min_lot_size_round_lot)
    #[serde(default)]
    pub min_lot_size_round_lot: Option<Decimal>,

    /// Maximum trade volume per order (Databento: max_trade_vol)
    #[serde(default)]
    pub max_trade_vol: Option<u64>,

    /// Minimum trade volume per order (Databento: min_trade_vol)
    #[serde(default)]
    pub min_trade_vol: Option<u64>,

    /// High price limit (Databento: high_limit_price)
    #[serde(default)]
    pub high_limit_price: Option<Decimal>,

    /// Low price limit (Databento: low_limit_price)
    #[serde(default)]
    pub low_limit_price: Option<Decimal>,

    /// Maximum price variation (Databento: max_price_variation)
    #[serde(default)]
    pub max_price_variation: Option<Decimal>,

    /// Market depth implied (Databento: market_depth_implied)
    #[serde(default)]
    pub market_depth_implied: Option<i32>,

    /// Market depth (Databento: market_depth)
    #[serde(default)]
    pub market_depth: Option<i32>,

    /// Match algorithm (Databento: match_algorithm)
    /// 'F' = FIFO, 'P' = Pro-rata, 'A' = Allocation
    #[serde(default)]
    pub match_algorithm: MatchAlgorithm,

    /// Maker fee (negative = rebate)
    #[serde(default)]
    pub maker_fee: Decimal,

    /// Taker fee
    #[serde(default)]
    pub taker_fee: Decimal,

    /// Margin requirement (if applicable)
    #[serde(default)]
    pub margin_requirement: Option<MarginRequirement>,

    /// Supported order types
    #[serde(default = "default_order_types")]
    pub supported_order_types: Vec<OrderType>,

    /// Instrument attributes (Databento: inst_attrib_value)
    /// Bitmask of instrument attributes
    #[serde(default)]
    pub inst_attrib_value: Option<i32>,

    /// Supports short selling
    #[serde(default = "default_true")]
    pub shortable: bool,

    /// User-defined instrument flag (Databento: user_defined_instrument)
    #[serde(default)]
    pub user_defined_instrument: bool,

    /// Minimum notional value for orders
    #[serde(default)]
    pub min_notional: Option<Decimal>,

    /// Price precision (decimal places)
    #[serde(default = "default_precision")]
    pub price_precision: u8,

    /// Quantity precision (decimal places)
    #[serde(default = "default_precision")]
    pub quantity_precision: u8,
}

fn default_one() -> Decimal {
    Decimal::ONE
}

fn default_order_types() -> Vec<OrderType> {
    vec![OrderType::Market, OrderType::Limit]
}

fn default_precision() -> u8 {
    8
}

impl TradingSpecs {
    /// Create new TradingSpecs with required fields
    pub fn new(min_price_increment: Decimal, min_lot_size: Decimal) -> Self {
        Self {
            min_price_increment,
            display_factor: Decimal::ONE,
            price_display_format: 0,
            tick_rule: None,
            min_lot_size,
            min_lot_size_block: None,
            min_lot_size_round_lot: None,
            max_trade_vol: None,
            min_trade_vol: None,
            high_limit_price: None,
            low_limit_price: None,
            max_price_variation: None,
            market_depth_implied: None,
            market_depth: None,
            match_algorithm: MatchAlgorithm::default(),
            maker_fee: Decimal::ZERO,
            taker_fee: Decimal::ZERO,
            margin_requirement: None,
            supported_order_types: default_order_types(),
            inst_attrib_value: None,
            shortable: true,
            user_defined_instrument: false,
            min_notional: None,
            price_precision: 8,
            quantity_precision: 8,
        }
    }

    /// Create for crypto with common defaults
    pub fn crypto(tick_size: Decimal, lot_size: Decimal, price_prec: u8, qty_prec: u8) -> Self {
        Self {
            min_price_increment: tick_size,
            min_lot_size: lot_size,
            price_precision: price_prec,
            quantity_precision: qty_prec,
            ..Default::default()
        }
    }

    /// Add fees
    pub fn with_fees(mut self, maker_fee: Decimal, taker_fee: Decimal) -> Self {
        self.maker_fee = maker_fee;
        self.taker_fee = taker_fee;
        self
    }

    /// Add minimum notional
    pub fn with_min_notional(mut self, min_notional: Decimal) -> Self {
        self.min_notional = Some(min_notional);
        self
    }

    /// Validate a price against specifications
    pub fn validate_price(&self, price: Decimal) -> Result<(), String> {
        if price <= Decimal::ZERO {
            return Err("Price must be positive".to_string());
        }

        // Check price increment
        let remainder = price % self.min_price_increment;
        if remainder != Decimal::ZERO {
            return Err(format!(
                "Price {} must be a multiple of tick size {}",
                price, self.min_price_increment
            ));
        }

        // Check price limits
        if let Some(high) = self.high_limit_price {
            if price > high {
                return Err(format!("Price {} exceeds high limit {}", price, high));
            }
        }
        if let Some(low) = self.low_limit_price {
            if price < low {
                return Err(format!("Price {} below low limit {}", price, low));
            }
        }

        Ok(())
    }

    /// Validate a quantity against specifications
    pub fn validate_quantity(&self, quantity: Decimal) -> Result<(), String> {
        if quantity <= Decimal::ZERO {
            return Err("Quantity must be positive".to_string());
        }

        if quantity < self.min_lot_size {
            return Err(format!(
                "Quantity {} below minimum lot size {}",
                quantity, self.min_lot_size
            ));
        }

        // Check quantity increment (lot size)
        let remainder = quantity % self.min_lot_size;
        if remainder != Decimal::ZERO {
            return Err(format!(
                "Quantity {} must be a multiple of lot size {}",
                quantity, self.min_lot_size
            ));
        }

        Ok(())
    }

    /// Calculate maker fee for a trade
    pub fn calc_maker_fee(&self, price: Decimal, quantity: Decimal) -> Decimal {
        price * quantity * self.maker_fee
    }

    /// Calculate taker fee for a trade
    pub fn calc_taker_fee(&self, price: Decimal, quantity: Decimal) -> Decimal {
        price * quantity * self.taker_fee
    }
}

impl Default for TradingSpecs {
    fn default() -> Self {
        Self::new(Decimal::new(1, 2), Decimal::new(1, 5)) // 0.01 tick, 0.00001 lot
    }
}

/// Match algorithm enumeration.
///
/// Databento: match_algorithm field (single char)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MatchAlgorithm {
    /// 'F' - FIFO (First In, First Out) - most common
    #[default]
    Fifo,
    /// 'P' - Pro-rata allocation
    ProRata,
    /// 'A' - Allocation-based
    Allocation,
    /// 'C' - FIFO with LMM (Lead Market Maker)
    FifoLmm,
    /// 'T' - Threshold Pro-rata
    ThresholdProRata,
    /// 'U' - Undefined/Unknown
    Undefined,
}

impl MatchAlgorithm {
    /// Get the single-character code (Databento format)
    pub fn as_char(&self) -> char {
        match self {
            Self::Fifo => 'F',
            Self::ProRata => 'P',
            Self::Allocation => 'A',
            Self::FifoLmm => 'C',
            Self::ThresholdProRata => 'T',
            Self::Undefined => 'U',
        }
    }

    /// Parse from single character
    pub fn from_char(c: char) -> Self {
        match c.to_ascii_uppercase() {
            'F' => Self::Fifo,
            'P' => Self::ProRata,
            'A' => Self::Allocation,
            'C' => Self::FifoLmm,
            'T' => Self::ThresholdProRata,
            _ => Self::Undefined,
        }
    }
}

impl fmt::Display for MatchAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fifo => write!(f, "FIFO"),
            Self::ProRata => write!(f, "PRO_RATA"),
            Self::Allocation => write!(f, "ALLOCATION"),
            Self::FifoLmm => write!(f, "FIFO_LMM"),
            Self::ThresholdProRata => write!(f, "THRESHOLD_PRO_RATA"),
            Self::Undefined => write!(f, "UNDEFINED"),
        }
    }
}

/// Margin requirement for leveraged instruments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginRequirement {
    /// Initial margin percentage (e.g., 0.10 = 10%)
    pub initial: Decimal,
    /// Maintenance margin percentage
    pub maintenance: Decimal,
}

impl MarginRequirement {
    /// Create new margin requirement
    pub fn new(initial: Decimal, maintenance: Decimal) -> Self {
        Self {
            initial,
            maintenance,
        }
    }

    /// Calculate required initial margin for a position
    pub fn calc_initial_margin(&self, notional: Decimal) -> Decimal {
        notional * self.initial
    }

    /// Calculate required maintenance margin for a position
    pub fn calc_maintenance_margin(&self, notional: Decimal) -> Decimal {
        notional * self.maintenance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_symbol_definition_creation() {
        let info = SymbolInfo::crypto_spot("BTC", "USDT");
        let venue_config = VenueConfig::binance("BTCUSDT");
        let trading_specs = TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5);

        let def = SymbolDefinition::new("BTCUSDT", "BINANCE", info, venue_config, trading_specs);

        assert_eq!(def.id.symbol, "BTCUSDT");
        assert_eq!(def.id.venue, "BINANCE");
        assert!(def.is_24_7());
        assert!(!def.is_derivative());
    }

    #[test]
    fn test_trading_specs_validation() {
        let specs = TradingSpecs::new(dec!(0.01), dec!(0.001));

        // Valid price
        assert!(specs.validate_price(dec!(100.01)).is_ok());

        // Invalid price (not a multiple of tick)
        assert!(specs.validate_price(dec!(100.005)).is_err());

        // Valid quantity
        assert!(specs.validate_quantity(dec!(1.001)).is_ok());

        // Invalid quantity (below minimum)
        assert!(specs.validate_quantity(dec!(0.0001)).is_err());
    }

    #[test]
    fn test_match_algorithm_char_conversion() {
        assert_eq!(MatchAlgorithm::Fifo.as_char(), 'F');
        assert_eq!(MatchAlgorithm::from_char('P'), MatchAlgorithm::ProRata);
        assert_eq!(MatchAlgorithm::from_char('X'), MatchAlgorithm::Undefined);
    }

    #[test]
    fn test_provider_symbol() {
        let ps = ProviderSymbol::new("ESH6")
            .with_dataset("GLBX.MDP3")
            .with_instrument_id(12345);

        assert_eq!(ps.symbol, "ESH6");
        assert_eq!(ps.dataset, Some("GLBX.MDP3".to_string()));
        assert_eq!(ps.instrument_id, Some(12345));
    }

    #[test]
    fn test_symbol_info_crypto() {
        let info = SymbolInfo::crypto_spot("ETH", "USDT");
        assert_eq!(info.asset, "ETH");
        assert_eq!(info.asset_class, AssetClass::Crypto);
        assert_eq!(info.instrument_class, InstrumentClass::Spot);
        assert_eq!(info.security_type, "SPOT");
    }

    #[test]
    fn test_venue_config_builders() {
        let binance = VenueConfig::binance("BTCUSDT");
        assert_eq!(binance.mic_code, mic_codes::BINANCE);

        let cme = VenueConfig::cme_globex("ESH6");
        assert_eq!(cme.mic_code, mic_codes::CME_GLOBEX);
        assert_eq!(cme.dataset, Some("GLBX.MDP3".to_string()));
        assert_eq!(cme.country, Some("US".to_string()));
    }
}
