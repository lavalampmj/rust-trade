//! Instrument and asset type definitions for multi-asset trading.
//!
//! This module provides comprehensive instrument modeling for various asset classes:
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
//! };
//! use rust_decimal_macros::dec;
//!
//! // Create a BTC/USDT spot instrument
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
//! // Validate order parameters
//! btcusdt.validate_price(dec!(50000.00))?;
//! btcusdt.validate_quantity(dec!(0.1))?;
//!
//! // Calculate notional and fees
//! let notional = btcusdt.notional(dec!(50000), dec!(0.1));
//! let fee = btcusdt.specs().calc_taker_fee(dec!(50000), dec!(0.1));
//! ```

mod instrument;
mod types;

// Re-export types
pub use types::{
    AssetClass, Currency, CurrencyType, InstrumentClass, OptionType, SecurityType, Venue,
    VenueType,
};

// Re-export instrument trait and implementations
pub use instrument::{
    CryptoFuture, CryptoPerpetual, CryptoSpot, Instrument, InstrumentAny, InstrumentSpecs,
};
