//! Instrument and asset type definitions for multi-asset trading.
//!
//! This module provides comprehensive instrument modeling for various asset classes,
//! including symbol definitions, trading sessions, and continuous contracts.
//!
//! # Core Modules
//!
//! - **types**: Asset classes, instrument classes, currencies, venues
//! - **instrument**: Instrument trait and implementations (CryptoSpot, etc.)
//! - **symbol_definition**: Complete symbol metadata (Databento-aligned)
//! - **session**: Trading sessions, market hours, calendars
//! - **contract**: Derivative contract specifications
//! - **continuous**: Continuous contract support (ES.c.0 notation)
//!
//! # Asset Classes
//! - **Crypto**: Cryptocurrencies (BTC, ETH, etc.)
//! - **FX**: Foreign exchange pairs
//! - **Equity**: Stocks and ETFs
//! - **Commodity**: Physical commodities
//!
//! # Instrument Classes
//! - **Spot**: Direct ownership of the asset
//! - **Perpetual**: Perpetual futures (linear or inverse)
//! - **Future**: Dated futures with expiry
//! - **Option**: Call/put options
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::{
//!     CryptoSpot, Instrument, InstrumentSpecs, Currency, Venue,
//!     SymbolDefinition, SymbolInfo, VenueConfig, TradingSpecs,
//! };
//! use rust_decimal_macros::dec;
//!
//! // Create a BTC/USDT spot instrument using the old API
//! let btcusdt = CryptoSpot::new(
//!     "BTCUSDT",
//!     Venue::binance(),
//!     Currency::btc(),
//!     Currency::usdt(),
//!     InstrumentSpecs::new(dec!(0.01), 2, dec!(0.00001), 5)
//!         .with_min_notional(dec!(10))
//!         .with_fees(dec!(0.001), dec!(0.001)),
//! );
//!
//! // Or create using the new SymbolDefinition API
//! let btcusdt_def = SymbolDefinition::new(
//!     "BTCUSDT",
//!     "BINANCE",
//!     SymbolInfo::crypto_spot("BTC", "USDT"),
//!     VenueConfig::binance("BTCUSDT"),
//!     TradingSpecs::crypto(dec!(0.01), dec!(0.00001), 2, 5),
//! );
//!
//! // Validate order parameters
//! btcusdt.validate_price(dec!(50000.00))?;
//! btcusdt.validate_quantity(dec!(0.1))?;
//!
//! // Calculate notional and fees
//! let notional = btcusdt.notional(dec!(50000), dec!(0.1));
//! let fee = btcusdt.specs().calc_taker_fee(dec!(50000), dec!(0.1));
//! ```
//!
//! # Continuous Contracts
//!
//! ```ignore
//! use trading_common::instruments::continuous::{ContinuousSymbol, ContinuousContract};
//!
//! // Parse Databento-format continuous symbols
//! let symbol = ContinuousSymbol::parse("ES.c.0")?;  // Front month by calendar
//! let symbol = ContinuousSymbol::parse("CL.v.0")?;  // Front by volume
//! let symbol = ContinuousSymbol::parse("GC.n.1")?;  // Second by open interest
//!
//! // Create a continuous contract
//! let contract = ContinuousContract::new("ES", "GLBX", ContinuousRollMethod::Calendar, 0);
//! ```
//!
//! # Trading Sessions
//!
//! ```ignore
//! use trading_common::instruments::session::{SessionSchedule, presets};
//!
//! // Use preset session schedules
//! let nyse_schedule = presets::us_equity();
//! let cme_schedule = presets::cme_globex();
//! let crypto_schedule = presets::crypto_24_7();
//!
//! // Check if market is open
//! let is_open = nyse_schedule.is_open(Utc::now());
//! ```

mod instrument;
mod types;

// New modules for comprehensive symbol management
pub mod continuous;
pub mod contract;
pub mod registry;
pub mod roll_manager;
pub mod session;
pub mod session_manager;
pub mod symbol_definition;
pub mod symbol_resolver;

// Re-export types
pub use types::{
    AssetClass, Currency, CurrencyType, InstrumentClass, OptionType, SecurityType, Venue, VenueType,
};

// Re-export instrument trait and implementations
pub use instrument::{
    CryptoFuture, CryptoPerpetual, CryptoSpot, Instrument, InstrumentAny, InstrumentSpecs,
};

// Re-export symbol definition types
pub use symbol_definition::{
    mic_codes, MarginRequirement, MatchAlgorithm, ProviderSymbol, RateLimits, SymbolDefinition,
    SymbolInfo, SymbolStatus, TradingSpecs, VenueConfig,
};

// Re-export session types
pub use session::{
    MaintenanceWindow, MarketCalendar, MarketStatus, SessionEvent, SessionSchedule, SessionState,
    SessionType, TradingSession,
};

// Re-export session presets
pub use session::presets as session_presets;

// Re-export contract types
pub use contract::{
    code_to_month, month_to_code, parse_contract_symbol, AdjustmentMethod, ContinuousRollMethod,
    ContractSpec, ExerciseStyle, RollRule, SettlementType,
};

// Re-export continuous contract types
pub use continuous::{
    AdjustmentFactor, AdjustmentType, ContinuousContract, ContinuousSymbol, ContinuousSymbolError,
    ContractInfo, RollEvent,
};

// Re-export registry types
pub use registry::{
    AssetDefaults, RegistryError, RegistryResult, RegistryStats, SymbolRegistry, VenueDefaults,
};

// Re-export session manager types
pub use session_manager::{SessionManager, SessionManagerConfig};

// Re-export roll manager types
pub use roll_manager::{ContinuousContractState, RollManager, RollManagerConfig, RollManagerError};

// Re-export symbol resolver types
pub use symbol_resolver::{ResolverError, SymbolResolver};
