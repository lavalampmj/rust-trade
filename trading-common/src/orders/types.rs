//! Core order types and enums for the order management system.
//!
//! This module defines the fundamental types used throughout the order system:
//! - `OrderSide` - Buy or Sell
//! - `OrderType` - Market, Limit, Stop, StopLimit, TrailingStop
//! - `OrderStatus` - Full lifecycle from New to terminal states
//! - `TimeInForce` - Order duration policies (GTC, IOC, FOK, GTD, DAY)
//! - `ContingencyType` - OCO, OTO, OUO bracket orders

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Order side indicating buy or sell direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderSide {
    /// Buy order - acquire the base asset
    Buy,
    /// Sell order - dispose of the base asset
    Sell,
}

impl OrderSide {
    /// Returns the opposite side
    pub fn opposite(&self) -> Self {
        match self {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        }
    }

    /// Returns true if this is a buy order
    pub fn is_buy(&self) -> bool {
        matches!(self, OrderSide::Buy)
    }

    /// Returns true if this is a sell order
    pub fn is_sell(&self) -> bool {
        matches!(self, OrderSide::Sell)
    }
}

impl fmt::Display for OrderSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderSide::Buy => write!(f, "BUY"),
            OrderSide::Sell => write!(f, "SELL"),
        }
    }
}

/// Order type determining execution behavior.
///
/// Supports standard order types found in professional trading systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderType {
    /// Market order - execute immediately at best available price
    Market,
    /// Limit order - execute at specified price or better
    Limit,
    /// Stop order - becomes market order when stop price is triggered
    Stop,
    /// Stop-limit order - becomes limit order when stop price is triggered
    StopLimit,
    /// Trailing stop order - stop price trails market by fixed amount/percentage
    TrailingStop,
    /// Market-to-limit order - market order that converts to limit at execution price
    MarketToLimit,
    /// Limit-if-touched - becomes limit order when trigger price is touched
    LimitIfTouched,
}

impl OrderType {
    /// Returns true if this order type requires a limit price
    pub fn requires_price(&self) -> bool {
        matches!(
            self,
            OrderType::Limit
                | OrderType::StopLimit
                | OrderType::MarketToLimit
                | OrderType::LimitIfTouched
        )
    }

    /// Returns true if this order type requires a trigger/stop price
    pub fn requires_trigger_price(&self) -> bool {
        matches!(
            self,
            OrderType::Stop
                | OrderType::StopLimit
                | OrderType::TrailingStop
                | OrderType::LimitIfTouched
        )
    }

    /// Returns true if this is an aggressive order type (executes immediately)
    pub fn is_aggressive(&self) -> bool {
        matches!(self, OrderType::Market)
    }

    /// Returns true if this is a passive order type (adds liquidity)
    pub fn is_passive(&self) -> bool {
        matches!(self, OrderType::Limit)
    }
}

impl fmt::Display for OrderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderType::Market => write!(f, "MARKET"),
            OrderType::Limit => write!(f, "LIMIT"),
            OrderType::Stop => write!(f, "STOP"),
            OrderType::StopLimit => write!(f, "STOP_LIMIT"),
            OrderType::TrailingStop => write!(f, "TRAILING_STOP"),
            OrderType::MarketToLimit => write!(f, "MARKET_TO_LIMIT"),
            OrderType::LimitIfTouched => write!(f, "LIMIT_IF_TOUCHED"),
        }
    }
}

/// Order status representing the current state in the order lifecycle.
///
/// Follows a state machine pattern with clear transition rules between states.
///
/// State transitions:
/// ```text
/// Initialized → Submitted → Accepted ─┬→ Filled
///                    │                 ├→ PartiallyFilled → Filled
///                    │                 ├→ Canceled
///                    │                 ├→ Expired
///                    │                 └→ PendingCancel → Canceled
///                    └→ Rejected
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderStatus {
    /// Order has been initialized but not yet submitted
    Initialized,
    /// Order has been submitted to the execution system
    Submitted,
    /// Order has been denied submission (pre-trade risk check failed)
    Denied,
    /// Order has been accepted by the venue/broker
    Accepted,
    /// Order has been rejected by the venue/broker
    Rejected,
    /// Order cancellation is pending
    PendingCancel,
    /// Order has been canceled (terminal state)
    Canceled,
    /// Order has been partially filled (qty_filled > 0 but < qty_total)
    PartiallyFilled,
    /// Order has been completely filled (terminal state)
    Filled,
    /// Order has expired (terminal state)
    Expired,
    /// Order is pending update/modification
    PendingUpdate,
    /// Order triggered (for stop/conditional orders)
    Triggered,
}

impl OrderStatus {
    /// Returns true if the order is in a terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OrderStatus::Filled
                | OrderStatus::Canceled
                | OrderStatus::Rejected
                | OrderStatus::Expired
                | OrderStatus::Denied
        )
    }

    /// Returns true if the order is still active/open
    pub fn is_open(&self) -> bool {
        matches!(
            self,
            OrderStatus::Initialized
                | OrderStatus::Submitted
                | OrderStatus::Accepted
                | OrderStatus::PartiallyFilled
                | OrderStatus::PendingCancel
                | OrderStatus::PendingUpdate
                | OrderStatus::Triggered
        )
    }

    /// Returns true if the order is in-flight (submitted but not yet accepted/rejected)
    pub fn is_inflight(&self) -> bool {
        matches!(
            self,
            OrderStatus::Submitted | OrderStatus::PendingCancel | OrderStatus::PendingUpdate
        )
    }

    /// Returns true if the order can be canceled
    pub fn is_cancelable(&self) -> bool {
        matches!(
            self,
            OrderStatus::Initialized
                | OrderStatus::Submitted
                | OrderStatus::Accepted
                | OrderStatus::PartiallyFilled
                | OrderStatus::Triggered
        )
    }

    /// Returns true if the order can be modified
    pub fn is_modifiable(&self) -> bool {
        matches!(
            self,
            OrderStatus::Accepted | OrderStatus::PartiallyFilled | OrderStatus::Triggered
        )
    }

    /// Check if transition from current status to target status is valid
    pub fn can_transition_to(&self, target: OrderStatus) -> bool {
        match self {
            OrderStatus::Initialized => matches!(
                target,
                OrderStatus::Submitted | OrderStatus::Denied | OrderStatus::Canceled
            ),
            OrderStatus::Submitted => matches!(
                target,
                OrderStatus::Accepted | OrderStatus::Rejected | OrderStatus::Canceled
            ),
            OrderStatus::Accepted => matches!(
                target,
                OrderStatus::PartiallyFilled
                    | OrderStatus::Filled
                    | OrderStatus::PendingCancel
                    | OrderStatus::PendingUpdate
                    | OrderStatus::Canceled
                    | OrderStatus::Expired
                    | OrderStatus::Triggered
            ),
            OrderStatus::PartiallyFilled => matches!(
                target,
                OrderStatus::PartiallyFilled
                    | OrderStatus::Filled
                    | OrderStatus::PendingCancel
                    | OrderStatus::Canceled
                    | OrderStatus::Expired
            ),
            OrderStatus::PendingCancel => {
                matches!(target, OrderStatus::Canceled | OrderStatus::Filled)
            }
            OrderStatus::PendingUpdate => matches!(
                target,
                OrderStatus::Accepted | OrderStatus::PartiallyFilled | OrderStatus::Rejected
            ),
            OrderStatus::Triggered => matches!(
                target,
                OrderStatus::Accepted
                    | OrderStatus::PartiallyFilled
                    | OrderStatus::Filled
                    | OrderStatus::Rejected
                    | OrderStatus::Canceled
            ),
            // Terminal states cannot transition
            OrderStatus::Filled
            | OrderStatus::Canceled
            | OrderStatus::Rejected
            | OrderStatus::Expired
            | OrderStatus::Denied => false,
        }
    }
}

impl fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderStatus::Initialized => write!(f, "INITIALIZED"),
            OrderStatus::Submitted => write!(f, "SUBMITTED"),
            OrderStatus::Denied => write!(f, "DENIED"),
            OrderStatus::Accepted => write!(f, "ACCEPTED"),
            OrderStatus::Rejected => write!(f, "REJECTED"),
            OrderStatus::PendingCancel => write!(f, "PENDING_CANCEL"),
            OrderStatus::Canceled => write!(f, "CANCELED"),
            OrderStatus::PartiallyFilled => write!(f, "PARTIALLY_FILLED"),
            OrderStatus::Filled => write!(f, "FILLED"),
            OrderStatus::Expired => write!(f, "EXPIRED"),
            OrderStatus::PendingUpdate => write!(f, "PENDING_UPDATE"),
            OrderStatus::Triggered => write!(f, "TRIGGERED"),
        }
    }
}

/// Time-in-force specifying how long an order remains active.
///
/// Determines when an order expires if not filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TimeInForce {
    /// Good-Till-Canceled - remains active until filled or explicitly canceled
    GTC,
    /// Immediate-Or-Cancel - fill immediately (partially ok), cancel remainder
    IOC,
    /// Fill-Or-Kill - fill entire quantity immediately or cancel entire order
    FOK,
    /// Good-Till-Date - remains active until specified expiry time
    GTD,
    /// Day order - expires at end of trading day
    Day,
    /// At-The-Open - execute at market open
    AtTheOpen,
    /// At-The-Close - execute at market close
    AtTheClose,
}

impl TimeInForce {
    /// Returns true if this TIF requires an expiry timestamp
    pub fn requires_expire_time(&self) -> bool {
        matches!(self, TimeInForce::GTD)
    }

    /// Returns true if this TIF allows partial fills
    pub fn allows_partial_fill(&self) -> bool {
        !matches!(self, TimeInForce::FOK)
    }
}

impl Default for TimeInForce {
    fn default() -> Self {
        TimeInForce::GTC
    }
}

impl fmt::Display for TimeInForce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeInForce::GTC => write!(f, "GTC"),
            TimeInForce::IOC => write!(f, "IOC"),
            TimeInForce::FOK => write!(f, "FOK"),
            TimeInForce::GTD => write!(f, "GTD"),
            TimeInForce::Day => write!(f, "DAY"),
            TimeInForce::AtTheOpen => write!(f, "AT_THE_OPEN"),
            TimeInForce::AtTheClose => write!(f, "AT_THE_CLOSE"),
        }
    }
}

/// Contingency type for linked orders (bracket orders, OCO, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContingencyType {
    /// No contingency - standalone order
    None,
    /// One-Cancels-Other - when one order fills/cancels, cancel the other
    OCO,
    /// One-Triggers-Other - when first order fills, submit the other
    OTO,
    /// One-Updates-Other - when first order fills, update the other
    OUO,
}

impl Default for ContingencyType {
    fn default() -> Self {
        ContingencyType::None
    }
}

impl fmt::Display for ContingencyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContingencyType::None => write!(f, "NONE"),
            ContingencyType::OCO => write!(f, "OCO"),
            ContingencyType::OTO => write!(f, "OTO"),
            ContingencyType::OUO => write!(f, "OUO"),
        }
    }
}

/// Trigger type for conditional orders (stop orders, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TriggerType {
    /// No trigger - not a conditional order
    None,
    /// Trigger on last trade price
    LastPrice,
    /// Trigger on bid price
    BidPrice,
    /// Trigger on ask price
    AskPrice,
    /// Trigger on mid price (bid + ask) / 2
    MidPrice,
    /// Trigger on mark price (for perpetuals/futures)
    MarkPrice,
    /// Trigger on index price
    IndexPrice,
}

impl Default for TriggerType {
    fn default() -> Self {
        TriggerType::LastPrice
    }
}

impl fmt::Display for TriggerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriggerType::None => write!(f, "NONE"),
            TriggerType::LastPrice => write!(f, "LAST_PRICE"),
            TriggerType::BidPrice => write!(f, "BID_PRICE"),
            TriggerType::AskPrice => write!(f, "ASK_PRICE"),
            TriggerType::MidPrice => write!(f, "MID_PRICE"),
            TriggerType::MarkPrice => write!(f, "MARK_PRICE"),
            TriggerType::IndexPrice => write!(f, "INDEX_PRICE"),
        }
    }
}

/// Trailing stop type - fixed offset or percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrailingOffsetType {
    /// No trailing offset
    None,
    /// Fixed price offset (e.g., $10 below market)
    Price,
    /// Percentage offset (e.g., 2% below market)
    BasisPoints,
    /// Number of ticks offset
    Ticks,
    /// Price tier offset
    PriceTier,
}

impl Default for TrailingOffsetType {
    fn default() -> Self {
        TrailingOffsetType::None
    }
}

impl fmt::Display for TrailingOffsetType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrailingOffsetType::None => write!(f, "NONE"),
            TrailingOffsetType::Price => write!(f, "PRICE"),
            TrailingOffsetType::BasisPoints => write!(f, "BASIS_POINTS"),
            TrailingOffsetType::Ticks => write!(f, "TICKS"),
            TrailingOffsetType::PriceTier => write!(f, "PRICE_TIER"),
        }
    }
}

/// Position side for position-related operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PositionSide {
    /// No position
    Flat,
    /// Long position (bought)
    Long,
    /// Short position (sold, for margin/futures)
    Short,
}

impl Default for PositionSide {
    fn default() -> Self {
        PositionSide::Flat
    }
}

impl fmt::Display for PositionSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PositionSide::Flat => write!(f, "FLAT"),
            PositionSide::Long => write!(f, "LONG"),
            PositionSide::Short => write!(f, "SHORT"),
        }
    }
}

/// Liquidity side indicating whether an order provided or took liquidity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LiquiditySide {
    /// No liquidity information
    None,
    /// Order was a maker (provided liquidity)
    Maker,
    /// Order was a taker (took liquidity)
    Taker,
}

impl Default for LiquiditySide {
    fn default() -> Self {
        LiquiditySide::None
    }
}

impl fmt::Display for LiquiditySide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiquiditySide::None => write!(f, "NONE"),
            LiquiditySide::Maker => write!(f, "MAKER"),
            LiquiditySide::Taker => write!(f, "TAKER"),
        }
    }
}

/// Money representation for currency amounts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    /// The amount in the currency's smallest unit
    pub amount: Decimal,
    /// Currency code (e.g., "USD", "BTC", "USDT")
    pub currency: String,
}

impl Money {
    /// Create a new Money instance
    pub fn new(amount: Decimal, currency: impl Into<String>) -> Self {
        Self {
            amount,
            currency: currency.into(),
        }
    }

    /// Create zero amount in the given currency
    pub fn zero(currency: impl Into<String>) -> Self {
        Self {
            amount: Decimal::ZERO,
            currency: currency.into(),
        }
    }

    /// Returns true if the amount is zero
    pub fn is_zero(&self) -> bool {
        self.amount.is_zero()
    }

    /// Returns true if the amount is positive
    pub fn is_positive(&self) -> bool {
        self.amount.is_sign_positive() && !self.amount.is_zero()
    }

    /// Returns true if the amount is negative
    pub fn is_negative(&self) -> bool {
        self.amount.is_sign_negative()
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency)
    }
}

/// Quantity with precision tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Quantity {
    /// The raw quantity value
    pub raw: Decimal,
    /// Precision (number of decimal places)
    pub precision: u8,
}

impl Quantity {
    /// Create a new Quantity with explicit precision
    pub fn new(raw: Decimal, precision: u8) -> Self {
        Self { raw, precision }
    }

    /// Create a Quantity from a Decimal, inferring precision
    pub fn from_decimal(value: Decimal) -> Self {
        let precision = value.scale() as u8;
        Self {
            raw: value,
            precision,
        }
    }

    /// Create zero quantity with given precision
    pub fn zero(precision: u8) -> Self {
        Self {
            raw: Decimal::ZERO,
            precision,
        }
    }

    /// Returns true if quantity is zero
    pub fn is_zero(&self) -> bool {
        self.raw.is_zero()
    }

    /// Returns true if quantity is positive
    pub fn is_positive(&self) -> bool {
        self.raw.is_sign_positive() && !self.raw.is_zero()
    }

    /// Returns the raw Decimal value
    pub fn as_decimal(&self) -> Decimal {
        self.raw
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl From<Decimal> for Quantity {
    fn from(value: Decimal) -> Self {
        Self::from_decimal(value)
    }
}

/// Price with precision tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Price {
    /// The raw price value
    pub raw: Decimal,
    /// Precision (number of decimal places)
    pub precision: u8,
}

impl Price {
    /// Create a new Price with explicit precision
    pub fn new(raw: Decimal, precision: u8) -> Self {
        Self { raw, precision }
    }

    /// Create a Price from a Decimal, inferring precision
    pub fn from_decimal(value: Decimal) -> Self {
        let precision = value.scale() as u8;
        Self {
            raw: value,
            precision,
        }
    }

    /// Returns true if price is zero
    pub fn is_zero(&self) -> bool {
        self.raw.is_zero()
    }

    /// Returns the raw Decimal value
    pub fn as_decimal(&self) -> Decimal {
        self.raw
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl From<Decimal> for Price {
    fn from(value: Decimal) -> Self {
        Self::from_decimal(value)
    }
}

/// Client order ID - unique identifier assigned by the client/strategy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientOrderId(pub String);

impl ClientOrderId {
    /// Create a new ClientOrderId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a new unique ClientOrderId using UUID
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClientOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ClientOrderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ClientOrderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Venue order ID - unique identifier assigned by the venue/exchange.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VenueOrderId(pub String);

impl VenueOrderId {
    /// Create a new VenueOrderId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for VenueOrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for VenueOrderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for VenueOrderId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Trade ID - unique identifier for a fill/execution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TradeId(pub String);

impl TradeId {
    /// Create a new TradeId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a new unique TradeId using UUID
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TradeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for TradeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TradeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Strategy ID - identifier for the strategy that created the order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StrategyId(pub String);

impl StrategyId {
    /// Create a new StrategyId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StrategyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for StrategyId {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl From<String> for StrategyId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for StrategyId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Account ID - identifier for the trading account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(pub String);

impl AccountId {
    /// Create a new AccountId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for AccountId {
    fn default() -> Self {
        Self("default".to_string())
    }
}

impl From<String> for AccountId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AccountId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Instrument ID - unique identifier for a tradable instrument.
///
/// Format: `{symbol}.{venue}` (e.g., "BTCUSDT.BINANCE")
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstrumentId {
    /// The symbol (e.g., "BTCUSDT")
    pub symbol: String,
    /// The venue/exchange (e.g., "BINANCE")
    pub venue: String,
}

impl InstrumentId {
    /// Create a new InstrumentId
    pub fn new(symbol: impl Into<String>, venue: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            venue: venue.into(),
        }
    }

    /// Parse from string format "SYMBOL.VENUE"
    pub fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() == 2 {
            Some(Self {
                symbol: parts[0].to_string(),
                venue: parts[1].to_string(),
            })
        } else {
            None
        }
    }

    /// Create with default venue (for backward compatibility)
    pub fn from_symbol(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            venue: "DEFAULT".to_string(),
        }
    }
}

impl fmt::Display for InstrumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.symbol, self.venue)
    }
}

/// Position ID - unique identifier for a position.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PositionId(pub String);

impl PositionId {
    /// Create a new PositionId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a new unique PositionId using UUID
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PositionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for PositionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for PositionId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Order list ID - identifier for linked orders (bracket, OCO, etc.)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrderListId(pub String);

impl OrderListId {
    /// Create a new OrderListId
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Generate a new unique OrderListId using UUID
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Get the inner string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for OrderListId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for OrderListId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for OrderListId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Session enforcement mode for order submission.
///
/// Determines how the order manager handles orders when the market is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionEnforcement {
    /// No session enforcement - orders accepted regardless of session state
    #[default]
    Disabled,
    /// Warn when submitting during closed session but accept the order
    Warn,
    /// Reject orders when session is closed (strict mode)
    Strict,
}

impl SessionEnforcement {
    /// Returns true if session validation is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self, SessionEnforcement::Disabled)
    }

    /// Returns true if orders should be rejected when session is closed
    pub fn should_reject(&self) -> bool {
        matches!(self, SessionEnforcement::Strict)
    }
}

impl fmt::Display for SessionEnforcement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionEnforcement::Disabled => write!(f, "DISABLED"),
            SessionEnforcement::Warn => write!(f, "WARN"),
            SessionEnforcement::Strict => write!(f, "STRICT"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_order_side_opposite() {
        assert_eq!(OrderSide::Buy.opposite(), OrderSide::Sell);
        assert_eq!(OrderSide::Sell.opposite(), OrderSide::Buy);
    }

    #[test]
    fn test_order_type_requirements() {
        assert!(!OrderType::Market.requires_price());
        assert!(OrderType::Limit.requires_price());
        assert!(OrderType::StopLimit.requires_price());

        assert!(!OrderType::Market.requires_trigger_price());
        assert!(OrderType::Stop.requires_trigger_price());
        assert!(OrderType::TrailingStop.requires_trigger_price());
    }

    #[test]
    fn test_order_status_terminal() {
        assert!(OrderStatus::Filled.is_terminal());
        assert!(OrderStatus::Canceled.is_terminal());
        assert!(OrderStatus::Rejected.is_terminal());
        assert!(OrderStatus::Expired.is_terminal());

        assert!(!OrderStatus::Accepted.is_terminal());
        assert!(!OrderStatus::PartiallyFilled.is_terminal());
    }

    #[test]
    fn test_order_status_transitions() {
        assert!(OrderStatus::Initialized.can_transition_to(OrderStatus::Submitted));
        assert!(OrderStatus::Submitted.can_transition_to(OrderStatus::Accepted));
        assert!(OrderStatus::Accepted.can_transition_to(OrderStatus::Filled));
        assert!(OrderStatus::Accepted.can_transition_to(OrderStatus::PartiallyFilled));

        // Invalid transitions
        assert!(!OrderStatus::Filled.can_transition_to(OrderStatus::Canceled));
        assert!(!OrderStatus::Rejected.can_transition_to(OrderStatus::Accepted));
    }

    #[test]
    fn test_time_in_force_defaults() {
        assert_eq!(TimeInForce::default(), TimeInForce::GTC);
        assert!(TimeInForce::GTC.allows_partial_fill());
        assert!(!TimeInForce::FOK.allows_partial_fill());
    }

    #[test]
    fn test_money_operations() {
        let money = Money::new(dec!(100.50), "USD");
        assert!(money.is_positive());
        assert!(!money.is_zero());

        let zero = Money::zero("BTC");
        assert!(zero.is_zero());
    }

    #[test]
    fn test_quantity_operations() {
        let qty = Quantity::new(dec!(1.5), 1);
        assert!(qty.is_positive());
        assert_eq!(qty.as_decimal(), dec!(1.5));

        let zero = Quantity::zero(8);
        assert!(zero.is_zero());
    }

    #[test]
    fn test_client_order_id() {
        let id = ClientOrderId::new("test-123");
        assert_eq!(id.as_str(), "test-123");

        let generated = ClientOrderId::generate();
        assert!(!generated.as_str().is_empty());
    }

    #[test]
    fn test_instrument_id() {
        let id = InstrumentId::new("BTCUSDT", "BINANCE");
        assert_eq!(id.symbol, "BTCUSDT");
        assert_eq!(id.venue, "BINANCE");
        assert_eq!(format!("{}", id), "BTCUSDT.BINANCE");

        let parsed = InstrumentId::from_str("ETHUSDT.KRAKEN").unwrap();
        assert_eq!(parsed.symbol, "ETHUSDT");
        assert_eq!(parsed.venue, "KRAKEN");
    }
}
