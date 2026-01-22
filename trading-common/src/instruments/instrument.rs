//! Instrument trait and concrete implementations.
//!
//! This module defines the Instrument trait for tradable instruments and
//! provides concrete implementations for various asset types.

use super::types::{AssetClass, Currency, InstrumentClass, OptionType, Venue};
use crate::orders::InstrumentId;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Instrument specifications defining trading constraints.
///
/// Contains all the parameters needed to validate orders and calculate
/// costs for a tradable instrument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstrumentSpecs {
    /// Minimum price movement (e.g., 0.01 for 1 cent)
    pub tick_size: Decimal,
    /// Number of decimal places for price precision
    pub price_precision: u8,
    /// Minimum tradable quantity
    pub lot_size: Decimal,
    /// Number of decimal places for quantity precision
    pub size_precision: u8,
    /// Minimum order quantity
    pub min_quantity: Decimal,
    /// Maximum order quantity (None = unlimited)
    pub max_quantity: Option<Decimal>,
    /// Minimum notional value per order (None = no minimum)
    pub min_notional: Option<Decimal>,
    /// Contract multiplier (1.0 for spot, varies for futures/options)
    pub multiplier: Decimal,
    /// Maker fee rate (e.g., 0.001 for 0.1%)
    pub maker_fee: Decimal,
    /// Taker fee rate
    pub taker_fee: Decimal,
    /// Margin requirement (None for spot, Some for derivatives)
    pub margin_init: Option<Decimal>,
    /// Maintenance margin requirement
    pub margin_maint: Option<Decimal>,
}

impl InstrumentSpecs {
    /// Create new instrument specifications
    pub fn new(
        tick_size: Decimal,
        price_precision: u8,
        lot_size: Decimal,
        size_precision: u8,
    ) -> Self {
        Self {
            tick_size,
            price_precision,
            lot_size,
            size_precision,
            min_quantity: lot_size,
            max_quantity: None,
            min_notional: None,
            multiplier: Decimal::ONE,
            maker_fee: Decimal::ZERO,
            taker_fee: Decimal::ZERO,
            margin_init: None,
            margin_maint: None,
        }
    }

    /// Builder method to set minimum quantity
    pub fn with_min_quantity(mut self, min: Decimal) -> Self {
        self.min_quantity = min;
        self
    }

    /// Builder method to set maximum quantity
    pub fn with_max_quantity(mut self, max: Decimal) -> Self {
        self.max_quantity = Some(max);
        self
    }

    /// Builder method to set minimum notional
    pub fn with_min_notional(mut self, min: Decimal) -> Self {
        self.min_notional = Some(min);
        self
    }

    /// Builder method to set contract multiplier
    pub fn with_multiplier(mut self, multiplier: Decimal) -> Self {
        self.multiplier = multiplier;
        self
    }

    /// Builder method to set fees
    pub fn with_fees(mut self, maker: Decimal, taker: Decimal) -> Self {
        self.maker_fee = maker;
        self.taker_fee = taker;
        self
    }

    /// Builder method to set margin requirements
    pub fn with_margin(mut self, init: Decimal, maint: Decimal) -> Self {
        self.margin_init = Some(init);
        self.margin_maint = Some(maint);
        self
    }

    /// Round price to tick size
    pub fn round_price(&self, price: Decimal) -> Decimal {
        (price / self.tick_size).round() * self.tick_size
    }

    /// Round quantity to lot size
    pub fn round_quantity(&self, quantity: Decimal) -> Decimal {
        (quantity / self.lot_size).floor() * self.lot_size
    }

    /// Validate that a price conforms to tick size
    pub fn validate_price(&self, price: Decimal) -> Result<(), String> {
        if price <= Decimal::ZERO {
            return Err("Price must be positive".to_string());
        }
        let rounded = self.round_price(price);
        if (price - rounded).abs() > Decimal::new(1, 10) {
            return Err(format!(
                "Price {} does not conform to tick size {}",
                price, self.tick_size
            ));
        }
        Ok(())
    }

    /// Validate that a quantity conforms to lot size and limits
    pub fn validate_quantity(&self, quantity: Decimal) -> Result<(), String> {
        if quantity <= Decimal::ZERO {
            return Err("Quantity must be positive".to_string());
        }
        if quantity < self.min_quantity {
            return Err(format!(
                "Quantity {} below minimum {}",
                quantity, self.min_quantity
            ));
        }
        if let Some(max) = self.max_quantity {
            if quantity > max {
                return Err(format!("Quantity {} above maximum {}", quantity, max));
            }
        }
        let rounded = self.round_quantity(quantity);
        if (quantity - rounded).abs() > Decimal::new(1, 10) {
            return Err(format!(
                "Quantity {} does not conform to lot size {}",
                quantity, self.lot_size
            ));
        }
        Ok(())
    }

    /// Validate notional value
    pub fn validate_notional(&self, price: Decimal, quantity: Decimal) -> Result<(), String> {
        let notional = price * quantity * self.multiplier;
        if let Some(min) = self.min_notional {
            if notional < min {
                return Err(format!(
                    "Notional value {} below minimum {}",
                    notional, min
                ));
            }
        }
        Ok(())
    }

    /// Calculate notional value
    pub fn notional(&self, price: Decimal, quantity: Decimal) -> Decimal {
        price * quantity * self.multiplier
    }

    /// Calculate maker fee for a trade
    pub fn calc_maker_fee(&self, price: Decimal, quantity: Decimal) -> Decimal {
        self.notional(price, quantity) * self.maker_fee
    }

    /// Calculate taker fee for a trade
    pub fn calc_taker_fee(&self, price: Decimal, quantity: Decimal) -> Decimal {
        self.notional(price, quantity) * self.taker_fee
    }

    /// Calculate initial margin required
    pub fn calc_init_margin(&self, price: Decimal, quantity: Decimal) -> Option<Decimal> {
        self.margin_init
            .map(|rate| self.notional(price, quantity) * rate)
    }

    /// Calculate maintenance margin required
    pub fn calc_maint_margin(&self, price: Decimal, quantity: Decimal) -> Option<Decimal> {
        self.margin_maint
            .map(|rate| self.notional(price, quantity) * rate)
    }
}

impl Default for InstrumentSpecs {
    fn default() -> Self {
        Self {
            tick_size: Decimal::new(1, 2),     // 0.01
            price_precision: 2,
            lot_size: Decimal::new(1, 8),      // 0.00000001
            size_precision: 8,
            min_quantity: Decimal::new(1, 8),
            max_quantity: None,
            min_notional: Some(Decimal::new(10, 0)), // $10
            multiplier: Decimal::ONE,
            maker_fee: Decimal::new(1, 3),     // 0.001 = 0.1%
            taker_fee: Decimal::new(1, 3),
            margin_init: None,
            margin_maint: None,
        }
    }
}

/// Core trait for all tradable instruments.
///
/// This trait defines the interface that all instrument types must implement,
/// providing access to instrument properties and trading specifications.
pub trait Instrument: Send + Sync + fmt::Debug {
    /// Get the unique instrument identifier
    fn id(&self) -> &InstrumentId;

    /// Get the raw symbol (e.g., "BTCUSDT")
    fn symbol(&self) -> &str {
        &self.id().symbol
    }

    /// Get the venue/exchange
    fn venue(&self) -> &Venue;

    /// Get the asset class
    fn asset_class(&self) -> AssetClass;

    /// Get the instrument class
    fn instrument_class(&self) -> InstrumentClass;

    /// Get the base currency (e.g., BTC in BTCUSDT)
    fn base_currency(&self) -> &Currency;

    /// Get the quote currency (e.g., USDT in BTCUSDT)
    fn quote_currency(&self) -> &Currency;

    /// Get the settlement currency
    fn settlement_currency(&self) -> &Currency {
        self.quote_currency()
    }

    /// Get the instrument specifications
    fn specs(&self) -> &InstrumentSpecs;

    /// Check if instrument is currently tradable
    fn is_active(&self) -> bool {
        true
    }

    /// Get the expiry timestamp (for futures/options)
    fn expiry(&self) -> Option<DateTime<Utc>> {
        None
    }

    /// Get the strike price (for options)
    fn strike_price(&self) -> Option<Decimal> {
        None
    }

    /// Get the option type (for options)
    fn option_type(&self) -> Option<OptionType> {
        None
    }

    /// Validate an order price
    fn validate_price(&self, price: Decimal) -> Result<(), String> {
        self.specs().validate_price(price)
    }

    /// Validate an order quantity
    fn validate_quantity(&self, quantity: Decimal) -> Result<(), String> {
        self.specs().validate_quantity(quantity)
    }

    /// Calculate notional value
    fn notional(&self, price: Decimal, quantity: Decimal) -> Decimal {
        self.specs().notional(price, quantity)
    }
}

/// Crypto spot instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoSpot {
    pub id: InstrumentId,
    pub venue: Venue,
    pub base_currency: Currency,
    pub quote_currency: Currency,
    pub specs: InstrumentSpecs,
    pub is_active: bool,
}

impl CryptoSpot {
    /// Create a new crypto spot instrument
    pub fn new(
        symbol: impl Into<String>,
        venue: Venue,
        base: Currency,
        quote: Currency,
        specs: InstrumentSpecs,
    ) -> Self {
        let symbol = symbol.into();
        Self {
            id: InstrumentId::new(symbol, venue.code.clone()),
            venue,
            base_currency: base,
            quote_currency: quote,
            specs,
            is_active: true,
        }
    }

    /// Create a simple BTCUSDT instrument with defaults
    pub fn btcusdt_binance() -> Self {
        Self::new(
            "BTCUSDT",
            Venue::binance(),
            Currency::btc(),
            Currency::usdt(),
            InstrumentSpecs::new(
                Decimal::new(1, 2),      // 0.01 tick size
                2,
                Decimal::new(1, 5),      // 0.00001 lot size
                5,
            )
            .with_min_notional(Decimal::new(10, 0))
            .with_fees(Decimal::new(1, 3), Decimal::new(1, 3)),
        )
    }

    /// Create a simple ETHUSDT instrument with defaults
    pub fn ethusdt_binance() -> Self {
        Self::new(
            "ETHUSDT",
            Venue::binance(),
            Currency::eth(),
            Currency::usdt(),
            InstrumentSpecs::new(
                Decimal::new(1, 2),      // 0.01 tick size
                2,
                Decimal::new(1, 4),      // 0.0001 lot size
                4,
            )
            .with_min_notional(Decimal::new(10, 0))
            .with_fees(Decimal::new(1, 3), Decimal::new(1, 3)),
        )
    }
}

impl Instrument for CryptoSpot {
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn venue(&self) -> &Venue {
        &self.venue
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Crypto
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::Spot
    }

    fn base_currency(&self) -> &Currency {
        &self.base_currency
    }

    fn quote_currency(&self) -> &Currency {
        &self.quote_currency
    }

    fn specs(&self) -> &InstrumentSpecs {
        &self.specs
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}

/// Crypto perpetual futures instrument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoPerpetual {
    pub id: InstrumentId,
    pub venue: Venue,
    pub base_currency: Currency,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,
    pub specs: InstrumentSpecs,
    pub is_linear: bool,
    pub is_active: bool,
    pub max_leverage: Decimal,
    pub funding_rate_interval_hours: u32,
}

impl CryptoPerpetual {
    /// Create a new linear perpetual (settled in quote currency)
    pub fn linear(
        symbol: impl Into<String>,
        venue: Venue,
        base: Currency,
        quote: Currency,
        specs: InstrumentSpecs,
        max_leverage: Decimal,
    ) -> Self {
        let symbol = symbol.into();
        let settlement = quote.clone();
        Self {
            id: InstrumentId::new(symbol, venue.code.clone()),
            venue,
            base_currency: base,
            quote_currency: quote,
            settlement_currency: settlement,
            specs,
            is_linear: true,
            is_active: true,
            max_leverage,
            funding_rate_interval_hours: 8,
        }
    }

    /// Create a new inverse perpetual (settled in base currency)
    pub fn inverse(
        symbol: impl Into<String>,
        venue: Venue,
        base: Currency,
        quote: Currency,
        specs: InstrumentSpecs,
        max_leverage: Decimal,
    ) -> Self {
        let symbol = symbol.into();
        let settlement = base.clone();
        Self {
            id: InstrumentId::new(symbol, venue.code.clone()),
            venue,
            base_currency: base,
            quote_currency: quote,
            settlement_currency: settlement,
            specs,
            is_linear: false,
            is_active: true,
            max_leverage,
            funding_rate_interval_hours: 8,
        }
    }
}

impl Instrument for CryptoPerpetual {
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn venue(&self) -> &Venue {
        &self.venue
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Crypto
    }

    fn instrument_class(&self) -> InstrumentClass {
        if self.is_linear {
            InstrumentClass::PerpetualLinear
        } else {
            InstrumentClass::PerpetualInverse
        }
    }

    fn base_currency(&self) -> &Currency {
        &self.base_currency
    }

    fn quote_currency(&self) -> &Currency {
        &self.quote_currency
    }

    fn settlement_currency(&self) -> &Currency {
        &self.settlement_currency
    }

    fn specs(&self) -> &InstrumentSpecs {
        &self.specs
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}

/// Crypto futures with expiry date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CryptoFuture {
    pub id: InstrumentId,
    pub venue: Venue,
    pub base_currency: Currency,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,
    pub specs: InstrumentSpecs,
    pub expiry: DateTime<Utc>,
    pub is_active: bool,
}

impl CryptoFuture {
    /// Create a new crypto future
    pub fn new(
        symbol: impl Into<String>,
        venue: Venue,
        base: Currency,
        quote: Currency,
        specs: InstrumentSpecs,
        expiry: DateTime<Utc>,
    ) -> Self {
        let symbol = symbol.into();
        let settlement = quote.clone();
        Self {
            id: InstrumentId::new(symbol, venue.code.clone()),
            venue,
            base_currency: base,
            quote_currency: quote,
            settlement_currency: settlement,
            specs,
            expiry,
            is_active: true,
        }
    }
}

impl Instrument for CryptoFuture {
    fn id(&self) -> &InstrumentId {
        &self.id
    }

    fn venue(&self) -> &Venue {
        &self.venue
    }

    fn asset_class(&self) -> AssetClass {
        AssetClass::Crypto
    }

    fn instrument_class(&self) -> InstrumentClass {
        InstrumentClass::Future
    }

    fn base_currency(&self) -> &Currency {
        &self.base_currency
    }

    fn quote_currency(&self) -> &Currency {
        &self.quote_currency
    }

    fn settlement_currency(&self) -> &Currency {
        &self.settlement_currency
    }

    fn specs(&self) -> &InstrumentSpecs {
        &self.specs
    }

    fn expiry(&self) -> Option<DateTime<Utc>> {
        Some(self.expiry)
    }

    fn is_active(&self) -> bool {
        self.is_active && self.expiry > Utc::now()
    }
}

/// Wrapper for any instrument type (type-erased).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InstrumentAny {
    CryptoSpot(CryptoSpot),
    CryptoPerpetual(CryptoPerpetual),
    CryptoFuture(CryptoFuture),
}

impl InstrumentAny {
    /// Get a reference to the inner instrument trait object
    pub fn as_instrument(&self) -> &dyn Instrument {
        match self {
            InstrumentAny::CryptoSpot(i) => i,
            InstrumentAny::CryptoPerpetual(i) => i,
            InstrumentAny::CryptoFuture(i) => i,
        }
    }
}

impl Instrument for InstrumentAny {
    fn id(&self) -> &InstrumentId {
        self.as_instrument().id()
    }

    fn venue(&self) -> &Venue {
        self.as_instrument().venue()
    }

    fn asset_class(&self) -> AssetClass {
        self.as_instrument().asset_class()
    }

    fn instrument_class(&self) -> InstrumentClass {
        self.as_instrument().instrument_class()
    }

    fn base_currency(&self) -> &Currency {
        self.as_instrument().base_currency()
    }

    fn quote_currency(&self) -> &Currency {
        self.as_instrument().quote_currency()
    }

    fn settlement_currency(&self) -> &Currency {
        self.as_instrument().settlement_currency()
    }

    fn specs(&self) -> &InstrumentSpecs {
        self.as_instrument().specs()
    }

    fn is_active(&self) -> bool {
        self.as_instrument().is_active()
    }

    fn expiry(&self) -> Option<DateTime<Utc>> {
        self.as_instrument().expiry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_instrument_specs_validation() {
        let specs = InstrumentSpecs::new(dec!(0.01), 2, dec!(0.001), 3)
            .with_min_quantity(dec!(0.01))
            .with_min_notional(dec!(10));

        // Valid price
        assert!(specs.validate_price(dec!(100.00)).is_ok());
        assert!(specs.validate_price(dec!(100.01)).is_ok());

        // Invalid price - doesn't conform to tick size
        assert!(specs.validate_price(dec!(100.005)).is_err());

        // Valid quantity
        assert!(specs.validate_quantity(dec!(1.0)).is_ok());
        assert!(specs.validate_quantity(dec!(0.123)).is_ok());

        // Invalid quantity - below minimum
        assert!(specs.validate_quantity(dec!(0.001)).is_err());
    }

    #[test]
    fn test_instrument_specs_rounding() {
        let specs = InstrumentSpecs::new(dec!(0.01), 2, dec!(0.001), 3);

        // Note: rust_decimal uses banker's rounding (round half to even)
        // 100.005 / 0.01 = 10000.5 rounds to 10000 (even), so 100.00
        // 100.015 / 0.01 = 10001.5 rounds to 10002 (even), so 100.02
        assert_eq!(specs.round_price(dec!(100.006)), dec!(100.01));
        assert_eq!(specs.round_price(dec!(100.004)), dec!(100.00));
        assert_eq!(specs.round_quantity(dec!(1.2345)), dec!(1.234));
    }

    #[test]
    fn test_instrument_specs_notional() {
        let specs = InstrumentSpecs::new(dec!(0.01), 2, dec!(0.001), 3)
            .with_multiplier(dec!(100)); // 100x multiplier (like ES futures)

        let notional = specs.notional(dec!(4500), dec!(1));
        assert_eq!(notional, dec!(450000)); // 4500 * 1 * 100
    }

    #[test]
    fn test_instrument_specs_fees() {
        let specs = InstrumentSpecs::new(dec!(0.01), 2, dec!(0.001), 3)
            .with_fees(dec!(0.0002), dec!(0.0004)); // 0.02% maker, 0.04% taker

        let maker_fee = specs.calc_maker_fee(dec!(100), dec!(10));
        assert_eq!(maker_fee, dec!(0.2)); // 1000 * 0.0002

        let taker_fee = specs.calc_taker_fee(dec!(100), dec!(10));
        assert_eq!(taker_fee, dec!(0.4)); // 1000 * 0.0004
    }

    #[test]
    fn test_crypto_spot_instrument() {
        let btc = CryptoSpot::btcusdt_binance();

        assert_eq!(btc.symbol(), "BTCUSDT");
        assert_eq!(btc.venue().code, "BINANCE");
        assert_eq!(btc.asset_class(), AssetClass::Crypto);
        assert_eq!(btc.instrument_class(), InstrumentClass::Spot);
        assert_eq!(btc.base_currency().code, "BTC");
        assert_eq!(btc.quote_currency().code, "USDT");
        assert!(btc.is_active());
        assert!(btc.expiry().is_none());
    }

    #[test]
    fn test_crypto_perpetual_instrument() {
        let perp = CryptoPerpetual::linear(
            "BTCUSDT",
            Venue::binance_futures(),
            Currency::btc(),
            Currency::usdt(),
            InstrumentSpecs::default().with_margin(dec!(0.01), dec!(0.005)),
            dec!(125),
        );

        assert_eq!(perp.instrument_class(), InstrumentClass::PerpetualLinear);
        assert!(perp.specs().margin_init.is_some());
        assert_eq!(perp.max_leverage, dec!(125));
    }

    #[test]
    fn test_instrument_any() {
        let spot = CryptoSpot::btcusdt_binance();
        let any = InstrumentAny::CryptoSpot(spot);

        assert_eq!(any.symbol(), "BTCUSDT");
        assert_eq!(any.instrument_class(), InstrumentClass::Spot);
    }
}
