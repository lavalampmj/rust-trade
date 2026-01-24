//! Order lifecycle events for tracking state changes.
//!
//! Each significant state change generates an event that can be processed
//! by the system and logged for audit.
//!
//! Event Flow:
//! ```text
//! OrderInitialized
//!       ↓
//! OrderSubmitted (or OrderDenied)
//!       ↓
//! OrderAccepted (or OrderRejected)
//!       ↓
//! OrderFilled / OrderPartiallyFilled
//!  (or OrderCanceled / OrderExpired)
//! ```

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::types::{
    AccountId, ClientOrderId, ContingencyType, InstrumentId, LiquiditySide, OrderListId, OrderSide,
    OrderStatus, OrderType, PositionId, StrategyId, TimeInForce, TradeId, TrailingOffsetType,
    TriggerType, VenueOrderId,
};

/// Unique identifier for an event
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    /// Generate a new unique event ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for EventId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Base trait for all order events
pub trait OrderEvent: fmt::Debug + Send + Sync {
    /// Get the event ID
    fn event_id(&self) -> &EventId;

    /// Get the client order ID
    fn client_order_id(&self) -> &ClientOrderId;

    /// Get the event timestamp
    fn ts_event(&self) -> DateTime<Utc>;

    /// Get the event initialization timestamp (when event was created locally)
    fn ts_init(&self) -> DateTime<Utc>;

    /// Get the instrument ID if available
    fn instrument_id(&self) -> Option<&InstrumentId>;

    /// Get the venue order ID if available
    fn venue_order_id(&self) -> Option<&VenueOrderId>;
}

/// Event generated when an order is initialized (created by strategy).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderInitialized {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub price: Option<Decimal>,
    pub trigger_price: Option<Decimal>,
    pub time_in_force: TimeInForce,
    pub expire_time: Option<DateTime<Utc>>,
    pub post_only: bool,
    pub reduce_only: bool,
    pub display_qty: Option<Decimal>,
    pub order_list_id: Option<OrderListId>,
    pub contingency_type: ContingencyType,
    pub linked_order_ids: Vec<ClientOrderId>,
    pub trailing_offset: Option<Decimal>,
    pub trailing_offset_type: TrailingOffsetType,
    pub trigger_type: TriggerType,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderInitialized {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        Some(&self.instrument_id)
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        None
    }
}

impl OrderInitialized {
    /// Create a new OrderInitialized event
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Decimal,
        price: Option<Decimal>,
        trigger_price: Option<Decimal>,
        time_in_force: TimeInForce,
        expire_time: Option<DateTime<Utc>>,
        post_only: bool,
        reduce_only: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            strategy_id,
            instrument_id,
            order_side,
            order_type,
            quantity,
            price,
            trigger_price,
            time_in_force,
            expire_time,
            post_only,
            reduce_only,
            display_qty: None,
            order_list_id: None,
            contingency_type: ContingencyType::None,
            linked_order_ids: Vec::new(),
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::None,
            trigger_type: TriggerType::LastPrice,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order is denied (pre-trade risk check failed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDenied {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub reason: String,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderDenied {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        None
    }
}

impl OrderDenied {
    /// Create a new OrderDenied event
    pub fn new(client_order_id: ClientOrderId, reason: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            reason: reason.into(),
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order is submitted to the execution system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderSubmitted {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub account_id: AccountId,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderSubmitted {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        None
    }
}

impl OrderSubmitted {
    /// Create a new OrderSubmitted event
    pub fn new(client_order_id: ClientOrderId, account_id: AccountId) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            account_id,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order is accepted by the venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderAccepted {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderAccepted {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        Some(&self.venue_order_id)
    }
}

impl OrderAccepted {
    /// Create a new OrderAccepted event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order is rejected by the venue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRejected {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub account_id: AccountId,
    pub reason: String,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderRejected {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        None
    }
}

impl OrderRejected {
    /// Create a new OrderRejected event
    pub fn new(
        client_order_id: ClientOrderId,
        account_id: AccountId,
        reason: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            account_id,
            reason: reason.into(),
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when a cancel request is submitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderPendingCancel {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderPendingCancel {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderPendingCancel {
    /// Create a new OrderPendingCancel event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order is canceled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCanceled {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderCanceled {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderCanceled {
    /// Create a new OrderCanceled event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when a cancel request is rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCancelRejected {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub reason: String,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderCancelRejected {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderCancelRejected {
    /// Create a new OrderCancelRejected event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
        reason: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            reason: reason.into(),
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an update/modify request is submitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderPendingUpdate {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub price: Option<Decimal>,
    pub trigger_price: Option<Decimal>,
    pub quantity: Option<Decimal>,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderPendingUpdate {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderPendingUpdate {
    /// Create a new OrderPendingUpdate event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
        price: Option<Decimal>,
        trigger_price: Option<Decimal>,
        quantity: Option<Decimal>,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            price,
            trigger_price,
            quantity,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order is updated/modified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderUpdated {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub price: Option<Decimal>,
    pub trigger_price: Option<Decimal>,
    pub quantity: Decimal,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderUpdated {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderUpdated {
    /// Create a new OrderUpdated event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
        price: Option<Decimal>,
        trigger_price: Option<Decimal>,
        quantity: Decimal,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            price,
            trigger_price,
            quantity,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an update request is rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderModifyRejected {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub reason: String,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderModifyRejected {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderModifyRejected {
    /// Create a new OrderModifyRejected event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
        reason: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            reason: reason.into(),
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when a stop/conditional order is triggered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderTriggered {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderTriggered {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderTriggered {
    /// Create a new OrderTriggered event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order expires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderExpired {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub account_id: AccountId,
    pub ts_event: DateTime<Utc>,
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderExpired {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        None
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        self.venue_order_id.as_ref()
    }
}

impl OrderExpired {
    /// Create a new OrderExpired event
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        account_id: AccountId,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            ts_event: now,
            ts_init: now,
        }
    }
}

/// Event generated when an order receives a fill (partial or complete).
///
/// This is the most important event for tracking execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderFilled {
    pub event_id: EventId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub instrument_id: InstrumentId,
    pub trade_id: TradeId,
    pub position_id: Option<PositionId>,
    pub strategy_id: StrategyId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    /// Quantity filled in this execution
    pub last_qty: Decimal,
    /// Price of this execution
    pub last_px: Decimal,
    /// Total quantity filled so far
    pub cum_qty: Decimal,
    /// Remaining quantity to fill
    pub leaves_qty: Decimal,
    /// Currency of the instrument
    pub currency: String,
    /// Commission for this fill
    pub commission: Decimal,
    /// Commission currency
    pub commission_currency: String,
    /// Whether this fill was maker or taker
    pub liquidity_side: LiquiditySide,
    /// Exchange timestamp of the fill
    pub ts_event: DateTime<Utc>,
    /// Local timestamp when event was created
    pub ts_init: DateTime<Utc>,
}

impl OrderEvent for OrderFilled {
    fn event_id(&self) -> &EventId {
        &self.event_id
    }

    fn client_order_id(&self) -> &ClientOrderId {
        &self.client_order_id
    }

    fn ts_event(&self) -> DateTime<Utc> {
        self.ts_event
    }

    fn ts_init(&self) -> DateTime<Utc> {
        self.ts_init
    }

    fn instrument_id(&self) -> Option<&InstrumentId> {
        Some(&self.instrument_id)
    }

    fn venue_order_id(&self) -> Option<&VenueOrderId> {
        Some(&self.venue_order_id)
    }
}

impl OrderFilled {
    /// Create a new OrderFilled event
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        trade_id: TradeId,
        strategy_id: StrategyId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Decimal,
        last_px: Decimal,
        cum_qty: Decimal,
        leaves_qty: Decimal,
        currency: String,
        commission: Decimal,
        commission_currency: String,
        liquidity_side: LiquiditySide,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            client_order_id,
            venue_order_id,
            account_id,
            instrument_id,
            trade_id,
            position_id: None,
            strategy_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            cum_qty,
            leaves_qty,
            currency,
            commission,
            commission_currency,
            liquidity_side,
            ts_event: now,
            ts_init: now,
        }
    }

    /// Set the position ID
    pub fn with_position_id(mut self, position_id: PositionId) -> Self {
        self.position_id = Some(position_id);
        self
    }

    /// Returns true if this fill completed the order
    pub fn is_last_fill(&self) -> bool {
        self.leaves_qty.is_zero()
    }

    /// Calculate the notional value of this fill
    pub fn notional(&self) -> Decimal {
        self.last_qty * self.last_px
    }
}

/// Enum containing all possible order events for unified handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OrderEventAny {
    Initialized(OrderInitialized),
    Denied(OrderDenied),
    Submitted(OrderSubmitted),
    Accepted(OrderAccepted),
    Rejected(OrderRejected),
    PendingCancel(OrderPendingCancel),
    Canceled(OrderCanceled),
    CancelRejected(OrderCancelRejected),
    PendingUpdate(OrderPendingUpdate),
    Updated(OrderUpdated),
    ModifyRejected(OrderModifyRejected),
    Triggered(OrderTriggered),
    Expired(OrderExpired),
    Filled(OrderFilled),
}

impl OrderEventAny {
    /// Get the client order ID from any event type
    pub fn client_order_id(&self) -> &ClientOrderId {
        match self {
            OrderEventAny::Initialized(e) => &e.client_order_id,
            OrderEventAny::Denied(e) => &e.client_order_id,
            OrderEventAny::Submitted(e) => &e.client_order_id,
            OrderEventAny::Accepted(e) => &e.client_order_id,
            OrderEventAny::Rejected(e) => &e.client_order_id,
            OrderEventAny::PendingCancel(e) => &e.client_order_id,
            OrderEventAny::Canceled(e) => &e.client_order_id,
            OrderEventAny::CancelRejected(e) => &e.client_order_id,
            OrderEventAny::PendingUpdate(e) => &e.client_order_id,
            OrderEventAny::Updated(e) => &e.client_order_id,
            OrderEventAny::ModifyRejected(e) => &e.client_order_id,
            OrderEventAny::Triggered(e) => &e.client_order_id,
            OrderEventAny::Expired(e) => &e.client_order_id,
            OrderEventAny::Filled(e) => &e.client_order_id,
        }
    }

    /// Get the event timestamp
    pub fn ts_event(&self) -> DateTime<Utc> {
        match self {
            OrderEventAny::Initialized(e) => e.ts_event,
            OrderEventAny::Denied(e) => e.ts_event,
            OrderEventAny::Submitted(e) => e.ts_event,
            OrderEventAny::Accepted(e) => e.ts_event,
            OrderEventAny::Rejected(e) => e.ts_event,
            OrderEventAny::PendingCancel(e) => e.ts_event,
            OrderEventAny::Canceled(e) => e.ts_event,
            OrderEventAny::CancelRejected(e) => e.ts_event,
            OrderEventAny::PendingUpdate(e) => e.ts_event,
            OrderEventAny::Updated(e) => e.ts_event,
            OrderEventAny::ModifyRejected(e) => e.ts_event,
            OrderEventAny::Triggered(e) => e.ts_event,
            OrderEventAny::Expired(e) => e.ts_event,
            OrderEventAny::Filled(e) => e.ts_event,
        }
    }

    /// Get the event ID
    pub fn event_id(&self) -> &EventId {
        match self {
            OrderEventAny::Initialized(e) => &e.event_id,
            OrderEventAny::Denied(e) => &e.event_id,
            OrderEventAny::Submitted(e) => &e.event_id,
            OrderEventAny::Accepted(e) => &e.event_id,
            OrderEventAny::Rejected(e) => &e.event_id,
            OrderEventAny::PendingCancel(e) => &e.event_id,
            OrderEventAny::Canceled(e) => &e.event_id,
            OrderEventAny::CancelRejected(e) => &e.event_id,
            OrderEventAny::PendingUpdate(e) => &e.event_id,
            OrderEventAny::Updated(e) => &e.event_id,
            OrderEventAny::ModifyRejected(e) => &e.event_id,
            OrderEventAny::Triggered(e) => &e.event_id,
            OrderEventAny::Expired(e) => &e.event_id,
            OrderEventAny::Filled(e) => &e.event_id,
        }
    }

    /// Returns true if this is a terminal event
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OrderEventAny::Denied(_)
                | OrderEventAny::Rejected(_)
                | OrderEventAny::Canceled(_)
                | OrderEventAny::Expired(_)
        ) || matches!(self, OrderEventAny::Filled(e) if e.is_last_fill())
    }

    /// Get the order status implied by this event
    pub fn implied_status(&self) -> OrderStatus {
        match self {
            OrderEventAny::Initialized(_) => OrderStatus::Initialized,
            OrderEventAny::Denied(_) => OrderStatus::Denied,
            OrderEventAny::Submitted(_) => OrderStatus::Submitted,
            OrderEventAny::Accepted(_) => OrderStatus::Accepted,
            OrderEventAny::Rejected(_) => OrderStatus::Rejected,
            OrderEventAny::PendingCancel(_) => OrderStatus::PendingCancel,
            OrderEventAny::Canceled(_) => OrderStatus::Canceled,
            OrderEventAny::CancelRejected(_) => OrderStatus::Accepted, // Revert to Accepted
            OrderEventAny::PendingUpdate(_) => OrderStatus::PendingUpdate,
            OrderEventAny::Updated(_) => OrderStatus::Accepted,
            OrderEventAny::ModifyRejected(_) => OrderStatus::Accepted, // Revert to Accepted
            OrderEventAny::Triggered(_) => OrderStatus::Triggered,
            OrderEventAny::Expired(_) => OrderStatus::Expired,
            OrderEventAny::Filled(e) => {
                if e.is_last_fill() {
                    OrderStatus::Filled
                } else {
                    OrderStatus::PartiallyFilled
                }
            }
        }
    }
}

impl From<OrderInitialized> for OrderEventAny {
    fn from(e: OrderInitialized) -> Self {
        OrderEventAny::Initialized(e)
    }
}

impl From<OrderDenied> for OrderEventAny {
    fn from(e: OrderDenied) -> Self {
        OrderEventAny::Denied(e)
    }
}

impl From<OrderSubmitted> for OrderEventAny {
    fn from(e: OrderSubmitted) -> Self {
        OrderEventAny::Submitted(e)
    }
}

impl From<OrderAccepted> for OrderEventAny {
    fn from(e: OrderAccepted) -> Self {
        OrderEventAny::Accepted(e)
    }
}

impl From<OrderRejected> for OrderEventAny {
    fn from(e: OrderRejected) -> Self {
        OrderEventAny::Rejected(e)
    }
}

impl From<OrderPendingCancel> for OrderEventAny {
    fn from(e: OrderPendingCancel) -> Self {
        OrderEventAny::PendingCancel(e)
    }
}

impl From<OrderCanceled> for OrderEventAny {
    fn from(e: OrderCanceled) -> Self {
        OrderEventAny::Canceled(e)
    }
}

impl From<OrderCancelRejected> for OrderEventAny {
    fn from(e: OrderCancelRejected) -> Self {
        OrderEventAny::CancelRejected(e)
    }
}

impl From<OrderPendingUpdate> for OrderEventAny {
    fn from(e: OrderPendingUpdate) -> Self {
        OrderEventAny::PendingUpdate(e)
    }
}

impl From<OrderUpdated> for OrderEventAny {
    fn from(e: OrderUpdated) -> Self {
        OrderEventAny::Updated(e)
    }
}

impl From<OrderModifyRejected> for OrderEventAny {
    fn from(e: OrderModifyRejected) -> Self {
        OrderEventAny::ModifyRejected(e)
    }
}

impl From<OrderTriggered> for OrderEventAny {
    fn from(e: OrderTriggered) -> Self {
        OrderEventAny::Triggered(e)
    }
}

impl From<OrderExpired> for OrderEventAny {
    fn from(e: OrderExpired) -> Self {
        OrderEventAny::Expired(e)
    }
}

impl From<OrderFilled> for OrderEventAny {
    fn from(e: OrderFilled) -> Self {
        OrderEventAny::Filled(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_order_initialized_event() {
        let event = OrderInitialized::new(
            ClientOrderId::new("test-order-1"),
            StrategyId::new("test-strategy"),
            InstrumentId::new("BTCUSDT", "BINANCE"),
            OrderSide::Buy,
            OrderType::Limit,
            dec!(1.0),
            Some(dec!(50000)),
            None,
            TimeInForce::GTC,
            None,
            false,
            false,
        );

        assert_eq!(event.client_order_id.as_str(), "test-order-1");
        assert_eq!(event.order_side, OrderSide::Buy);
        assert_eq!(event.order_type, OrderType::Limit);
        assert_eq!(event.quantity, dec!(1.0));
        assert_eq!(event.price, Some(dec!(50000)));
    }

    #[test]
    fn test_order_filled_event() {
        let event = OrderFilled::new(
            ClientOrderId::new("test-order-1"),
            VenueOrderId::new("venue-123"),
            AccountId::new("account-1"),
            InstrumentId::new("BTCUSDT", "BINANCE"),
            TradeId::generate(),
            StrategyId::new("test-strategy"),
            OrderSide::Buy,
            OrderType::Market,
            dec!(0.5),          // last_qty
            dec!(50000),        // last_px
            dec!(0.5),          // cum_qty
            dec!(0.5),          // leaves_qty
            "USDT".to_string(), // currency
            dec!(0.05),         // commission
            "USDT".to_string(), // commission_currency
            LiquiditySide::Taker,
        );

        assert_eq!(event.last_qty, dec!(0.5));
        assert_eq!(event.last_px, dec!(50000));
        assert_eq!(event.notional(), dec!(25000));
        assert!(!event.is_last_fill());
    }

    #[test]
    fn test_order_event_any_conversion() {
        let initialized = OrderInitialized::new(
            ClientOrderId::new("test-order-1"),
            StrategyId::new("test-strategy"),
            InstrumentId::new("BTCUSDT", "BINANCE"),
            OrderSide::Buy,
            OrderType::Market,
            dec!(1.0),
            None,
            None,
            TimeInForce::GTC,
            None,
            false,
            false,
        );

        let event_any: OrderEventAny = initialized.into();

        match event_any {
            OrderEventAny::Initialized(e) => {
                assert_eq!(e.client_order_id.as_str(), "test-order-1");
            }
            _ => panic!("Expected Initialized event"),
        }
    }

    #[test]
    fn test_implied_status() {
        let denied = OrderDenied::new(ClientOrderId::new("test"), "risk limit exceeded");
        let event_any: OrderEventAny = denied.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Denied);
        assert!(event_any.is_terminal());

        let accepted = OrderAccepted::new(
            ClientOrderId::new("test"),
            VenueOrderId::new("venue-1"),
            AccountId::new("acc-1"),
        );
        let event_any: OrderEventAny = accepted.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Accepted);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_denied_event() {
        let event = OrderDenied::new(ClientOrderId::new("order-123"), "Insufficient margin");

        assert_eq!(event.client_order_id.as_str(), "order-123");
        assert_eq!(event.reason, "Insufficient margin");
        assert!(event.instrument_id().is_none());
        assert!(event.venue_order_id().is_none());
    }

    #[test]
    fn test_order_submitted_event() {
        let event =
            OrderSubmitted::new(ClientOrderId::new("order-456"), AccountId::new("account-1"));

        assert_eq!(event.client_order_id.as_str(), "order-456");
        assert_eq!(event.account_id.as_str(), "account-1");
        assert!(event.instrument_id().is_none());
        assert!(event.venue_order_id().is_none());

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Submitted);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_rejected_event() {
        let event = OrderRejected::new(
            ClientOrderId::new("order-789"),
            AccountId::new("account-2"),
            "Invalid quantity",
        );

        assert_eq!(event.client_order_id.as_str(), "order-789");
        assert_eq!(event.account_id.as_str(), "account-2");
        assert_eq!(event.reason, "Invalid quantity");

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Rejected);
        assert!(event_any.is_terminal());
    }

    #[test]
    fn test_order_pending_cancel_event() {
        let event = OrderPendingCancel::new(
            ClientOrderId::new("order-100"),
            Some(VenueOrderId::new("venue-100")),
            AccountId::new("account-1"),
        );

        assert_eq!(event.client_order_id.as_str(), "order-100");
        assert_eq!(
            event.venue_order_id().map(|v| v.as_str()),
            Some("venue-100")
        );
        assert_eq!(event.account_id.as_str(), "account-1");

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::PendingCancel);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_pending_cancel_without_venue_id() {
        let event = OrderPendingCancel::new(
            ClientOrderId::new("order-101"),
            None,
            AccountId::new("account-1"),
        );

        assert!(event.venue_order_id().is_none());
    }

    #[test]
    fn test_order_canceled_event() {
        let event = OrderCanceled::new(
            ClientOrderId::new("order-200"),
            Some(VenueOrderId::new("venue-200")),
            AccountId::new("account-1"),
        );

        assert_eq!(event.client_order_id.as_str(), "order-200");
        assert_eq!(
            event.venue_order_id().map(|v| v.as_str()),
            Some("venue-200")
        );

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Canceled);
        assert!(event_any.is_terminal());
    }

    #[test]
    fn test_order_cancel_rejected_event() {
        let event = OrderCancelRejected::new(
            ClientOrderId::new("order-300"),
            Some(VenueOrderId::new("venue-300")),
            AccountId::new("account-1"),
            "Order already filled",
        );

        assert_eq!(event.client_order_id.as_str(), "order-300");
        assert_eq!(event.reason, "Order already filled");

        // Test conversion to OrderEventAny (cancel reject keeps Accepted status)
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Accepted);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_pending_update_event() {
        let event = OrderPendingUpdate::new(
            ClientOrderId::new("order-400"),
            Some(VenueOrderId::new("venue-400")),
            AccountId::new("account-1"),
            Some(dec!(51000)), // new price
            Some(dec!(50500)), // new trigger price
            Some(dec!(2.0)),   // new quantity
        );

        assert_eq!(event.client_order_id.as_str(), "order-400");
        assert_eq!(event.price, Some(dec!(51000)));
        assert_eq!(event.trigger_price, Some(dec!(50500)));
        assert_eq!(event.quantity, Some(dec!(2.0)));

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::PendingUpdate);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_pending_update_partial() {
        // Test with only price update (no trigger or quantity)
        let event = OrderPendingUpdate::new(
            ClientOrderId::new("order-401"),
            None,
            AccountId::new("account-1"),
            Some(dec!(52000)),
            None,
            None,
        );

        assert_eq!(event.price, Some(dec!(52000)));
        assert!(event.trigger_price.is_none());
        assert!(event.quantity.is_none());
    }

    #[test]
    fn test_order_updated_event() {
        let event = OrderUpdated::new(
            ClientOrderId::new("order-500"),
            Some(VenueOrderId::new("venue-500")),
            AccountId::new("account-1"),
            Some(dec!(53000)), // updated price
            None,              // no trigger price change
            dec!(1.5),         // updated quantity
        );

        assert_eq!(event.client_order_id.as_str(), "order-500");
        assert_eq!(event.price, Some(dec!(53000)));
        assert!(event.trigger_price.is_none());
        assert_eq!(event.quantity, dec!(1.5));

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Accepted);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_modify_rejected_event() {
        let event = OrderModifyRejected::new(
            ClientOrderId::new("order-600"),
            Some(VenueOrderId::new("venue-600")),
            AccountId::new("account-1"),
            "Price outside trading range",
        );

        assert_eq!(event.client_order_id.as_str(), "order-600");
        assert_eq!(event.reason, "Price outside trading range");

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Accepted);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_triggered_event() {
        let event = OrderTriggered::new(
            ClientOrderId::new("order-700"),
            Some(VenueOrderId::new("venue-700")),
            AccountId::new("account-1"),
        );

        assert_eq!(event.client_order_id.as_str(), "order-700");
        assert_eq!(
            event.venue_order_id().map(|v| v.as_str()),
            Some("venue-700")
        );

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Triggered);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_expired_event() {
        let event = OrderExpired::new(
            ClientOrderId::new("order-800"),
            Some(VenueOrderId::new("venue-800")),
            AccountId::new("account-1"),
        );

        assert_eq!(event.client_order_id.as_str(), "order-800");
        assert_eq!(
            event.venue_order_id().map(|v| v.as_str()),
            Some("venue-800")
        );

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = event.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Expired);
        assert!(event_any.is_terminal());
    }

    #[test]
    fn test_order_filled_is_last_fill() {
        // Test complete fill (leaves_qty = 0)
        let complete_fill = OrderFilled::new(
            ClientOrderId::new("order-901"),
            VenueOrderId::new("venue-901"),
            AccountId::new("account-1"),
            InstrumentId::new("BTCUSDT", "BINANCE"),
            TradeId::generate(),
            StrategyId::new("test-strategy"),
            OrderSide::Buy,
            OrderType::Market,
            dec!(1.0),   // last_qty
            dec!(50000), // last_px
            dec!(1.0),   // cum_qty
            dec!(0),     // leaves_qty = 0 (complete)
            "USDT".to_string(),
            dec!(0.1),
            "USDT".to_string(),
            LiquiditySide::Taker,
        );

        assert!(complete_fill.is_last_fill());
        assert_eq!(complete_fill.notional(), dec!(50000));

        // Test conversion to OrderEventAny
        let event_any: OrderEventAny = complete_fill.into();
        assert_eq!(event_any.implied_status(), OrderStatus::Filled);
        assert!(event_any.is_terminal());
    }

    #[test]
    fn test_order_filled_partial() {
        // Test partial fill (leaves_qty > 0)
        let partial_fill = OrderFilled::new(
            ClientOrderId::new("order-902"),
            VenueOrderId::new("venue-902"),
            AccountId::new("account-1"),
            InstrumentId::new("ETHUSDT", "BINANCE"),
            TradeId::generate(),
            StrategyId::new("test-strategy"),
            OrderSide::Sell,
            OrderType::Limit,
            dec!(0.3),  // last_qty
            dec!(3000), // last_px
            dec!(0.3),  // cum_qty
            dec!(0.7),  // leaves_qty > 0 (partial)
            "USDT".to_string(),
            dec!(0.03),
            "USDT".to_string(),
            LiquiditySide::Maker,
        );

        assert!(!partial_fill.is_last_fill());
        assert_eq!(partial_fill.notional(), dec!(900)); // 0.3 * 3000

        // Test conversion - partial fill implies PartiallyFilled status
        let event_any: OrderEventAny = partial_fill.into();
        assert_eq!(event_any.implied_status(), OrderStatus::PartiallyFilled);
        assert!(!event_any.is_terminal());
    }

    #[test]
    fn test_order_filled_with_position_id() {
        let fill = OrderFilled::new(
            ClientOrderId::new("order-903"),
            VenueOrderId::new("venue-903"),
            AccountId::new("account-1"),
            InstrumentId::new("BTCUSDT", "BINANCE"),
            TradeId::generate(),
            StrategyId::new("test-strategy"),
            OrderSide::Buy,
            OrderType::Market,
            dec!(0.5),
            dec!(45000),
            dec!(0.5),
            dec!(0),
            "USDT".to_string(),
            dec!(0.05),
            "USDT".to_string(),
            LiquiditySide::Taker,
        )
        .with_position_id(PositionId::new("position-123"));

        assert_eq!(
            fill.position_id.as_ref().map(|p| p.as_str()),
            Some("position-123")
        );
    }

    #[test]
    fn test_event_id_generation() {
        let id1 = EventId::new();
        let id2 = EventId::new();

        // Each event ID should be unique
        assert_ne!(id1, id2);

        // Display should work
        let display = format!("{}", id1);
        assert!(!display.is_empty());
    }

    #[test]
    fn test_order_event_trait_timestamps() {
        let submitted = OrderSubmitted::new(ClientOrderId::new("test"), AccountId::new("acc-1"));

        // ts_event and ts_init should be very close (both set to now())
        let diff = (submitted.ts_event() - submitted.ts_init())
            .num_milliseconds()
            .abs();
        assert!(diff < 10, "Timestamps should be within 10ms of each other");
    }

    #[test]
    fn test_all_terminal_states() {
        // Test all terminal status events
        let terminal_events: Vec<OrderEventAny> = vec![
            OrderDenied::new(ClientOrderId::new("1"), "denied").into(),
            OrderRejected::new(ClientOrderId::new("2"), AccountId::new("a"), "rejected").into(),
            OrderCanceled::new(ClientOrderId::new("3"), None, AccountId::new("a")).into(),
            OrderExpired::new(ClientOrderId::new("4"), None, AccountId::new("a")).into(),
            OrderFilled::new(
                ClientOrderId::new("5"),
                VenueOrderId::new("v5"),
                AccountId::new("a"),
                InstrumentId::new("BTC", "BINANCE"),
                TradeId::generate(),
                StrategyId::new("s"),
                OrderSide::Buy,
                OrderType::Market,
                dec!(1),
                dec!(100),
                dec!(1),
                dec!(0),
                "USD".into(),
                dec!(0),
                "USD".into(),
                LiquiditySide::Taker,
            )
            .into(),
        ];

        for event in terminal_events {
            assert!(
                event.is_terminal(),
                "Event {:?} should be terminal",
                event.implied_status()
            );
        }
    }

    #[test]
    fn test_all_non_terminal_states() {
        // Test all non-terminal status events
        let non_terminal_events: Vec<OrderEventAny> = vec![
            OrderSubmitted::new(ClientOrderId::new("1"), AccountId::new("a")).into(),
            OrderAccepted::new(
                ClientOrderId::new("2"),
                VenueOrderId::new("v"),
                AccountId::new("a"),
            )
            .into(),
            OrderPendingCancel::new(ClientOrderId::new("3"), None, AccountId::new("a")).into(),
            OrderPendingUpdate::new(
                ClientOrderId::new("4"),
                None,
                AccountId::new("a"),
                None,
                None,
                None,
            )
            .into(),
            OrderTriggered::new(ClientOrderId::new("5"), None, AccountId::new("a")).into(),
            OrderUpdated::new(
                ClientOrderId::new("6"),
                None,
                AccountId::new("a"),
                None,
                None,
                dec!(1),
            )
            .into(),
            // Partial fill is not terminal
            OrderFilled::new(
                ClientOrderId::new("7"),
                VenueOrderId::new("v7"),
                AccountId::new("a"),
                InstrumentId::new("BTC", "BINANCE"),
                TradeId::generate(),
                StrategyId::new("s"),
                OrderSide::Buy,
                OrderType::Market,
                dec!(0.5),
                dec!(100),
                dec!(0.5),
                dec!(0.5), // leaves_qty > 0
                "USD".into(),
                dec!(0),
                "USD".into(),
                LiquiditySide::Taker,
            )
            .into(),
        ];

        for event in non_terminal_events {
            assert!(
                !event.is_terminal(),
                "Event {:?} should NOT be terminal",
                event.implied_status()
            );
        }
    }
}
