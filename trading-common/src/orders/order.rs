//! Order struct and builder for creating and managing orders.
//!
//! The `Order` struct represents a trading order with full lifecycle support.
//! Orders are immutable once created - state changes are tracked via events.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use super::types::{
    AccountId, ClientOrderId, ContingencyType, InstrumentId, LiquiditySide, OrderListId,
    OrderSide, OrderStatus, OrderType, PositionId, StrategyId, TimeInForce, TradeId,
    TrailingOffsetType, TriggerType, VenueOrderId,
};

/// A trading order with full lifecycle tracking.
///
/// Orders follow a state machine pattern where transitions are controlled
/// and validated. The order struct is designed to be immutable after creation,
/// with state changes tracked through events.
///
/// # Example
/// ```ignore
/// use rust_decimal_macros::dec;
/// use trading_common::orders::{Order, OrderSide, OrderType, TimeInForce};
///
/// let order = Order::market(
///     "BTCUSDT",
///     OrderSide::Buy,
///     dec!(0.1),
/// )
/// .with_strategy_id("my-strategy")
/// .build();
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    // === Identifiers ===
    /// Client-assigned order ID (unique within client/strategy)
    pub client_order_id: ClientOrderId,
    /// Venue-assigned order ID (set after acceptance)
    pub venue_order_id: Option<VenueOrderId>,
    /// Position ID this order is associated with (optional)
    pub position_id: Option<PositionId>,
    /// Account ID for this order
    pub account_id: AccountId,
    /// Instrument being traded
    pub instrument_id: InstrumentId,
    /// Strategy that created this order
    pub strategy_id: StrategyId,

    // === Order Specification ===
    /// Buy or Sell
    pub side: OrderSide,
    /// Order type (Market, Limit, Stop, etc.)
    pub order_type: OrderType,
    /// Total quantity ordered
    pub quantity: Decimal,
    /// Limit price (required for Limit, StopLimit)
    pub price: Option<Decimal>,
    /// Trigger/stop price (required for Stop, StopLimit, TrailingStop)
    pub trigger_price: Option<Decimal>,
    /// Time in force
    pub time_in_force: TimeInForce,
    /// Expiration time (for GTD orders)
    pub expire_time: Option<DateTime<Utc>>,

    // === Trailing Stop Fields ===
    /// Trailing offset type
    pub trailing_offset_type: TrailingOffsetType,
    /// Trailing offset value
    pub trailing_offset: Option<Decimal>,

    // === Trigger Configuration ===
    /// How to determine when stop/conditional orders trigger
    pub trigger_type: TriggerType,

    // === Execution State ===
    /// Current order status
    pub status: OrderStatus,
    /// Quantity filled so far
    pub filled_qty: Decimal,
    /// Quantity remaining to fill
    pub leaves_qty: Decimal,
    /// Average fill price (volume-weighted)
    pub avg_px: Option<Decimal>,
    /// Last fill price
    pub last_px: Option<Decimal>,
    /// Last fill quantity
    pub last_qty: Option<Decimal>,
    /// Slippage from intended price
    pub slippage: Option<Decimal>,

    // === Linked Orders (Brackets, OCO, etc.) ===
    /// Order list ID for linked orders
    pub order_list_id: Option<OrderListId>,
    /// IDs of linked orders
    pub linked_order_ids: Vec<ClientOrderId>,
    /// Contingency type for linked orders
    pub contingency_type: ContingencyType,

    // === Costs ===
    /// Total commission paid
    pub commission: Decimal,
    /// Currency of commission
    pub commission_currency: Option<String>,

    // === Liquidity ===
    /// Whether this order was maker or taker
    pub liquidity_side: LiquiditySide,

    // === Timestamps ===
    /// When the order was initialized/created
    pub init_time: DateTime<Utc>,
    /// When the order was last updated
    pub ts_last: DateTime<Utc>,

    // === Flags ===
    /// Whether this is a reduce-only order (futures/perps)
    pub is_reduce_only: bool,
    /// Whether this is a post-only order (maker only)
    pub is_post_only: bool,
    /// Whether to display quantity (iceberg orders)
    pub display_qty: Option<Decimal>,

    // === Custom Tags ===
    /// Custom key-value tags for strategy use
    pub tags: HashMap<String, String>,

    // === Fill History ===
    /// List of trade IDs for fills on this order
    pub trade_ids: Vec<TradeId>,
}

impl Order {
    /// Create a new market order builder
    pub fn market(
        symbol: impl Into<String>,
        side: OrderSide,
        quantity: Decimal,
    ) -> OrderBuilder {
        OrderBuilder::new(OrderType::Market, symbol, side, quantity)
    }

    /// Create a new limit order builder
    pub fn limit(
        symbol: impl Into<String>,
        side: OrderSide,
        quantity: Decimal,
        price: Decimal,
    ) -> OrderBuilder {
        OrderBuilder::new(OrderType::Limit, symbol, side, quantity).with_price(price)
    }

    /// Create a new stop order builder
    pub fn stop(
        symbol: impl Into<String>,
        side: OrderSide,
        quantity: Decimal,
        trigger_price: Decimal,
    ) -> OrderBuilder {
        OrderBuilder::new(OrderType::Stop, symbol, side, quantity)
            .with_trigger_price(trigger_price)
    }

    /// Create a new stop-limit order builder
    pub fn stop_limit(
        symbol: impl Into<String>,
        side: OrderSide,
        quantity: Decimal,
        price: Decimal,
        trigger_price: Decimal,
    ) -> OrderBuilder {
        OrderBuilder::new(OrderType::StopLimit, symbol, side, quantity)
            .with_price(price)
            .with_trigger_price(trigger_price)
    }

    /// Create a new trailing stop order builder
    pub fn trailing_stop(
        symbol: impl Into<String>,
        side: OrderSide,
        quantity: Decimal,
        offset: Decimal,
        offset_type: TrailingOffsetType,
    ) -> OrderBuilder {
        OrderBuilder::new(OrderType::TrailingStop, symbol, side, quantity)
            .with_trailing_offset(offset, offset_type)
    }

    // === State Queries ===

    /// Returns true if the order is in a terminal state
    pub fn is_closed(&self) -> bool {
        self.status.is_terminal()
    }

    /// Returns true if the order is still active
    pub fn is_open(&self) -> bool {
        self.status.is_open()
    }

    /// Returns true if the order is in-flight (awaiting response)
    pub fn is_inflight(&self) -> bool {
        self.status.is_inflight()
    }

    /// Returns true if the order can be canceled
    pub fn is_cancelable(&self) -> bool {
        self.status.is_cancelable()
    }

    /// Returns true if the order can be modified
    pub fn is_modifiable(&self) -> bool {
        self.status.is_modifiable()
    }

    /// Returns true if the order has any fills
    pub fn is_partially_filled(&self) -> bool {
        self.filled_qty > Decimal::ZERO && self.filled_qty < self.quantity
    }

    /// Returns true if the order is completely filled
    pub fn is_filled(&self) -> bool {
        self.status == OrderStatus::Filled
    }

    /// Returns the fill percentage (0.0 to 1.0)
    pub fn fill_percent(&self) -> Decimal {
        if self.quantity.is_zero() {
            Decimal::ZERO
        } else {
            self.filled_qty / self.quantity
        }
    }

    /// Returns the notional value of the order (quantity * price)
    pub fn notional(&self) -> Option<Decimal> {
        self.price.map(|p| self.quantity * p)
    }

    /// Returns the filled notional value
    pub fn filled_notional(&self) -> Option<Decimal> {
        self.avg_px.map(|p| self.filled_qty * p)
    }

    // === State Transitions ===

    /// Apply a status change to the order
    ///
    /// Returns an error if the transition is invalid according to the state machine.
    pub fn transition_to(&mut self, new_status: OrderStatus) -> Result<(), OrderError> {
        if !self.status.can_transition_to(new_status) {
            return Err(OrderError::InvalidTransition {
                from: self.status,
                to: new_status,
                order_id: self.client_order_id.clone(),
            });
        }
        self.status = new_status;
        self.ts_last = Utc::now();
        Ok(())
    }

    /// Record a fill on the order
    pub fn apply_fill(
        &mut self,
        fill_qty: Decimal,
        fill_price: Decimal,
        trade_id: TradeId,
        commission: Decimal,
        liquidity_side: LiquiditySide,
    ) -> Result<(), OrderError> {
        if self.is_closed() {
            return Err(OrderError::OrderClosed {
                order_id: self.client_order_id.clone(),
            });
        }

        if fill_qty > self.leaves_qty {
            return Err(OrderError::OverFill {
                order_id: self.client_order_id.clone(),
                fill_qty,
                leaves_qty: self.leaves_qty,
            });
        }

        // Update average price (volume-weighted)
        let total_filled = self.filled_qty + fill_qty;
        if let Some(current_avg) = self.avg_px {
            self.avg_px = Some(
                (current_avg * self.filled_qty + fill_price * fill_qty) / total_filled,
            );
        } else {
            self.avg_px = Some(fill_price);
        }

        // Update quantities
        self.filled_qty = total_filled;
        self.leaves_qty = self.quantity - total_filled;
        self.last_px = Some(fill_price);
        self.last_qty = Some(fill_qty);

        // Update commission
        self.commission += commission;
        self.liquidity_side = liquidity_side;

        // Track trade ID
        self.trade_ids.push(trade_id);

        // Update status
        if self.leaves_qty.is_zero() {
            self.status = OrderStatus::Filled;
        } else {
            self.status = OrderStatus::PartiallyFilled;
        }

        // Calculate slippage if we have a target price
        if let Some(target_price) = self.price {
            let slippage = match self.side {
                OrderSide::Buy => fill_price - target_price,
                OrderSide::Sell => target_price - fill_price,
            };
            self.slippage = Some(slippage);
        }

        self.ts_last = Utc::now();
        Ok(())
    }

    /// Set the venue order ID (after order is accepted by exchange)
    pub fn set_venue_order_id(&mut self, venue_order_id: VenueOrderId) {
        self.venue_order_id = Some(venue_order_id);
        self.ts_last = Utc::now();
    }

    /// Cancel the order (if cancelable)
    pub fn cancel(&mut self) -> Result<(), OrderError> {
        if !self.is_cancelable() {
            return Err(OrderError::NotCancelable {
                order_id: self.client_order_id.clone(),
                status: self.status,
            });
        }
        self.transition_to(OrderStatus::Canceled)
    }

    /// Expire the order
    pub fn expire(&mut self) -> Result<(), OrderError> {
        if self.is_closed() {
            return Err(OrderError::OrderClosed {
                order_id: self.client_order_id.clone(),
            });
        }
        self.transition_to(OrderStatus::Expired)
    }

    /// Reject the order with a reason
    pub fn reject(&mut self, reason: &str) -> Result<(), OrderError> {
        self.transition_to(OrderStatus::Rejected)?;
        self.tags
            .insert("reject_reason".to_string(), reason.to_string());
        Ok(())
    }

    /// Deny the order (pre-submission risk check failed)
    pub fn deny(&mut self, reason: &str) -> Result<(), OrderError> {
        self.transition_to(OrderStatus::Denied)?;
        self.tags
            .insert("deny_reason".to_string(), reason.to_string());
        Ok(())
    }

    // === Utility Methods ===

    /// Get a custom tag value
    pub fn get_tag(&self, key: &str) -> Option<&String> {
        self.tags.get(key)
    }

    /// Set a custom tag
    pub fn set_tag(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.tags.insert(key.into(), value.into());
    }

    /// Get the symbol (convenience method)
    pub fn symbol(&self) -> &str {
        &self.instrument_id.symbol
    }

    /// Get the venue (convenience method)
    pub fn venue(&self) -> &str {
        &self.instrument_id.venue
    }
}

impl fmt::Display for Order {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Order({} {} {} {} @ {} status={} filled={}/{})",
            self.client_order_id,
            self.side,
            self.order_type,
            self.instrument_id,
            self.price
                .map(|p| p.to_string())
                .unwrap_or_else(|| "MARKET".to_string()),
            self.status,
            self.filled_qty,
            self.quantity,
        )
    }
}

/// Builder for constructing orders with validation.
#[derive(Debug)]
pub struct OrderBuilder {
    order_type: OrderType,
    symbol: String,
    venue: String,
    side: OrderSide,
    quantity: Decimal,
    price: Option<Decimal>,
    trigger_price: Option<Decimal>,
    time_in_force: TimeInForce,
    expire_time: Option<DateTime<Utc>>,
    trailing_offset: Option<Decimal>,
    trailing_offset_type: TrailingOffsetType,
    trigger_type: TriggerType,
    client_order_id: Option<ClientOrderId>,
    strategy_id: StrategyId,
    account_id: AccountId,
    position_id: Option<PositionId>,
    order_list_id: Option<OrderListId>,
    linked_order_ids: Vec<ClientOrderId>,
    contingency_type: ContingencyType,
    is_reduce_only: bool,
    is_post_only: bool,
    display_qty: Option<Decimal>,
    tags: HashMap<String, String>,
}

impl OrderBuilder {
    /// Create a new order builder
    pub fn new(
        order_type: OrderType,
        symbol: impl Into<String>,
        side: OrderSide,
        quantity: Decimal,
    ) -> Self {
        Self {
            order_type,
            symbol: symbol.into(),
            venue: "DEFAULT".to_string(),
            side,
            quantity,
            price: None,
            trigger_price: None,
            time_in_force: TimeInForce::GTC,
            expire_time: None,
            trailing_offset: None,
            trailing_offset_type: TrailingOffsetType::None,
            trigger_type: TriggerType::LastPrice,
            client_order_id: None,
            strategy_id: StrategyId::default(),
            account_id: AccountId::default(),
            position_id: None,
            order_list_id: None,
            linked_order_ids: Vec::new(),
            contingency_type: ContingencyType::None,
            is_reduce_only: false,
            is_post_only: false,
            display_qty: None,
            tags: HashMap::new(),
        }
    }

    /// Set the venue/exchange
    pub fn with_venue(mut self, venue: impl Into<String>) -> Self {
        self.venue = venue.into();
        self
    }

    /// Set the limit price
    pub fn with_price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Set the trigger/stop price
    pub fn with_trigger_price(mut self, trigger_price: Decimal) -> Self {
        self.trigger_price = Some(trigger_price);
        self
    }

    /// Set the time in force
    pub fn with_time_in_force(mut self, tif: TimeInForce) -> Self {
        self.time_in_force = tif;
        self
    }

    /// Set the expiry time (for GTD orders)
    pub fn with_expire_time(mut self, expire_time: DateTime<Utc>) -> Self {
        self.expire_time = Some(expire_time);
        self.time_in_force = TimeInForce::GTD;
        self
    }

    /// Set trailing stop offset
    pub fn with_trailing_offset(
        mut self,
        offset: Decimal,
        offset_type: TrailingOffsetType,
    ) -> Self {
        self.trailing_offset = Some(offset);
        self.trailing_offset_type = offset_type;
        self
    }

    /// Set the trigger type for conditional orders
    pub fn with_trigger_type(mut self, trigger_type: TriggerType) -> Self {
        self.trigger_type = trigger_type;
        self
    }

    /// Set a specific client order ID (otherwise auto-generated)
    pub fn with_client_order_id(mut self, id: impl Into<ClientOrderId>) -> Self {
        self.client_order_id = Some(id.into());
        self
    }

    /// Set the strategy ID
    pub fn with_strategy_id(mut self, id: impl Into<StrategyId>) -> Self {
        self.strategy_id = id.into();
        self
    }

    /// Set the account ID
    pub fn with_account_id(mut self, id: impl Into<AccountId>) -> Self {
        self.account_id = id.into();
        self
    }

    /// Set the position ID
    pub fn with_position_id(mut self, id: impl Into<PositionId>) -> Self {
        self.position_id = Some(id.into());
        self
    }

    /// Set order list ID for linked orders
    pub fn with_order_list_id(mut self, id: impl Into<OrderListId>) -> Self {
        self.order_list_id = Some(id.into());
        self
    }

    /// Add a linked order ID
    pub fn with_linked_order(mut self, id: impl Into<ClientOrderId>) -> Self {
        self.linked_order_ids.push(id.into());
        self
    }

    /// Set contingency type
    pub fn with_contingency_type(mut self, contingency_type: ContingencyType) -> Self {
        self.contingency_type = contingency_type;
        self
    }

    /// Mark as reduce-only
    pub fn reduce_only(mut self) -> Self {
        self.is_reduce_only = true;
        self
    }

    /// Mark as post-only (maker only)
    pub fn post_only(mut self) -> Self {
        self.is_post_only = true;
        self
    }

    /// Set display quantity for iceberg orders
    pub fn with_display_qty(mut self, qty: Decimal) -> Self {
        self.display_qty = Some(qty);
        self
    }

    /// Add a custom tag
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }

    /// Validate and build the order
    pub fn build(self) -> Result<Order, OrderError> {
        // Validation
        if self.quantity <= Decimal::ZERO {
            return Err(OrderError::InvalidQuantity {
                quantity: self.quantity,
                reason: "Quantity must be positive".to_string(),
            });
        }

        if self.order_type.requires_price() && self.price.is_none() {
            return Err(OrderError::MissingPrice {
                order_type: self.order_type,
            });
        }

        if self.order_type.requires_trigger_price() && self.trigger_price.is_none() {
            return Err(OrderError::MissingTriggerPrice {
                order_type: self.order_type,
            });
        }

        if self.time_in_force == TimeInForce::GTD && self.expire_time.is_none() {
            return Err(OrderError::MissingExpireTime);
        }

        if let Some(price) = self.price {
            if price <= Decimal::ZERO {
                return Err(OrderError::InvalidPrice {
                    price,
                    reason: "Price must be positive".to_string(),
                });
            }
        }

        if let Some(trigger_price) = self.trigger_price {
            if trigger_price <= Decimal::ZERO {
                return Err(OrderError::InvalidPrice {
                    price: trigger_price,
                    reason: "Trigger price must be positive".to_string(),
                });
            }
        }

        let now = Utc::now();
        let client_order_id = self
            .client_order_id
            .unwrap_or_else(ClientOrderId::generate);

        Ok(Order {
            client_order_id,
            venue_order_id: None,
            position_id: self.position_id,
            account_id: self.account_id,
            instrument_id: InstrumentId::new(self.symbol, self.venue),
            strategy_id: self.strategy_id,
            side: self.side,
            order_type: self.order_type,
            quantity: self.quantity,
            price: self.price,
            trigger_price: self.trigger_price,
            time_in_force: self.time_in_force,
            expire_time: self.expire_time,
            trailing_offset_type: self.trailing_offset_type,
            trailing_offset: self.trailing_offset,
            trigger_type: self.trigger_type,
            status: OrderStatus::Initialized,
            filled_qty: Decimal::ZERO,
            leaves_qty: self.quantity,
            avg_px: None,
            last_px: None,
            last_qty: None,
            slippage: None,
            order_list_id: self.order_list_id,
            linked_order_ids: self.linked_order_ids,
            contingency_type: self.contingency_type,
            commission: Decimal::ZERO,
            commission_currency: None,
            liquidity_side: LiquiditySide::None,
            init_time: now,
            ts_last: now,
            is_reduce_only: self.is_reduce_only,
            is_post_only: self.is_post_only,
            display_qty: self.display_qty,
            tags: self.tags,
            trade_ids: Vec::new(),
        })
    }
}

/// Errors that can occur during order operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum OrderError {
    #[error("Invalid state transition from {from} to {to} for order {order_id}")]
    InvalidTransition {
        from: OrderStatus,
        to: OrderStatus,
        order_id: ClientOrderId,
    },

    #[error("Order {order_id} is closed and cannot be modified")]
    OrderClosed { order_id: ClientOrderId },

    #[error("Order {order_id} cannot be canceled in status {status}")]
    NotCancelable {
        order_id: ClientOrderId,
        status: OrderStatus,
    },

    #[error("Over-fill: order {order_id} fill_qty={fill_qty} > leaves_qty={leaves_qty}")]
    OverFill {
        order_id: ClientOrderId,
        fill_qty: Decimal,
        leaves_qty: Decimal,
    },

    #[error("Invalid quantity {quantity}: {reason}")]
    InvalidQuantity { quantity: Decimal, reason: String },

    #[error("Invalid price {price}: {reason}")]
    InvalidPrice { price: Decimal, reason: String },

    #[error("{order_type} order requires a limit price")]
    MissingPrice { order_type: OrderType },

    #[error("{order_type} order requires a trigger price")]
    MissingTriggerPrice { order_type: OrderType },

    #[error("GTD orders require an expire_time")]
    MissingExpireTime,

    #[error("Order not found: {0}")]
    NotFound(ClientOrderId),

    #[error("Duplicate order ID: {0}")]
    DuplicateOrderId(ClientOrderId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_market_order_creation() {
        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1))
            .with_strategy_id("test-strategy")
            .build()
            .unwrap();

        assert_eq!(order.order_type, OrderType::Market);
        assert_eq!(order.side, OrderSide::Buy);
        assert_eq!(order.quantity, dec!(0.1));
        assert_eq!(order.status, OrderStatus::Initialized);
        assert!(order.price.is_none());
    }

    #[test]
    fn test_limit_order_creation() {
        let order = Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(50000))
            .with_venue("BINANCE")
            .with_time_in_force(TimeInForce::IOC)
            .build()
            .unwrap();

        assert_eq!(order.order_type, OrderType::Limit);
        assert_eq!(order.side, OrderSide::Sell);
        assert_eq!(order.price, Some(dec!(50000)));
        assert_eq!(order.time_in_force, TimeInForce::IOC);
        assert_eq!(order.venue(), "BINANCE");
    }

    #[test]
    fn test_stop_limit_order_creation() {
        let order =
            Order::stop_limit("ETHUSDT", OrderSide::Sell, dec!(2.0), dec!(2500), dec!(2510))
                .build()
                .unwrap();

        assert_eq!(order.order_type, OrderType::StopLimit);
        assert_eq!(order.price, Some(dec!(2500)));
        assert_eq!(order.trigger_price, Some(dec!(2510)));
    }

    #[test]
    fn test_missing_price_validation() {
        let result = OrderBuilder::new(OrderType::Limit, "BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build(); // Missing price

        assert!(matches!(result, Err(OrderError::MissingPrice { .. })));
    }

    #[test]
    fn test_missing_trigger_price_validation() {
        let result = OrderBuilder::new(OrderType::Stop, "BTCUSDT", OrderSide::Sell, dec!(1.0))
            .build(); // Missing trigger price

        assert!(matches!(result, Err(OrderError::MissingTriggerPrice { .. })));
    }

    #[test]
    fn test_invalid_quantity_validation() {
        let result = Order::market("BTCUSDT", OrderSide::Buy, dec!(0)).build();

        assert!(matches!(result, Err(OrderError::InvalidQuantity { .. })));
    }

    #[test]
    fn test_order_state_transitions() {
        let mut order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        assert_eq!(order.status, OrderStatus::Initialized);

        // Valid transition: Initialized -> Submitted
        order.transition_to(OrderStatus::Submitted).unwrap();
        assert_eq!(order.status, OrderStatus::Submitted);

        // Valid transition: Submitted -> Accepted
        order.transition_to(OrderStatus::Accepted).unwrap();
        assert_eq!(order.status, OrderStatus::Accepted);

        // Valid transition: Accepted -> Filled
        order.transition_to(OrderStatus::Filled).unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
        assert!(order.is_closed());

        // Invalid transition: Filled -> Canceled (terminal state)
        let result = order.transition_to(OrderStatus::Canceled);
        assert!(matches!(result, Err(OrderError::InvalidTransition { .. })));
    }

    #[test]
    fn test_order_fill() {
        let mut order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(10.0), dec!(50000))
            .build()
            .unwrap();

        order.transition_to(OrderStatus::Submitted).unwrap();
        order.transition_to(OrderStatus::Accepted).unwrap();

        // Partial fill
        order
            .apply_fill(
                dec!(3.0),
                dec!(49990),
                TradeId::generate(),
                dec!(0.1),
                LiquiditySide::Taker,
            )
            .unwrap();

        assert_eq!(order.filled_qty, dec!(3.0));
        assert_eq!(order.leaves_qty, dec!(7.0));
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
        assert_eq!(order.avg_px, Some(dec!(49990)));

        // Another fill
        order
            .apply_fill(
                dec!(7.0),
                dec!(50010),
                TradeId::generate(),
                dec!(0.2),
                LiquiditySide::Maker,
            )
            .unwrap();

        assert_eq!(order.filled_qty, dec!(10.0));
        assert_eq!(order.leaves_qty, dec!(0.0));
        assert_eq!(order.status, OrderStatus::Filled);
        assert!(order.is_closed());

        // Volume-weighted average price
        // (49990 * 3 + 50010 * 7) / 10 = (149970 + 350070) / 10 = 50004
        assert_eq!(order.avg_px, Some(dec!(50004)));
    }

    #[test]
    fn test_over_fill_prevention() {
        let mut order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        order.transition_to(OrderStatus::Submitted).unwrap();
        order.transition_to(OrderStatus::Accepted).unwrap();

        let result = order.apply_fill(
            dec!(2.0), // More than order quantity
            dec!(50000),
            TradeId::generate(),
            dec!(0.1),
            LiquiditySide::Taker,
        );

        assert!(matches!(result, Err(OrderError::OverFill { .. })));
    }

    #[test]
    fn test_order_cancel() {
        let mut order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .build()
            .unwrap();

        order.transition_to(OrderStatus::Submitted).unwrap();
        order.transition_to(OrderStatus::Accepted).unwrap();

        order.cancel().unwrap();
        assert_eq!(order.status, OrderStatus::Canceled);
        assert!(order.is_closed());
    }

    #[test]
    fn test_cannot_cancel_filled_order() {
        let mut order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        order.transition_to(OrderStatus::Submitted).unwrap();
        order.transition_to(OrderStatus::Accepted).unwrap();
        order.transition_to(OrderStatus::Filled).unwrap();

        let result = order.cancel();
        assert!(matches!(result, Err(OrderError::NotCancelable { .. })));
    }

    #[test]
    fn test_order_tags() {
        let mut order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .with_tag("source", "signal-generator")
            .build()
            .unwrap();

        assert_eq!(order.get_tag("source"), Some(&"signal-generator".to_string()));

        order.set_tag("execution_id", "12345");
        assert_eq!(order.get_tag("execution_id"), Some(&"12345".to_string()));
    }

    #[test]
    fn test_post_only_and_reduce_only() {
        let order = Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(50000))
            .post_only()
            .reduce_only()
            .build()
            .unwrap();

        assert!(order.is_post_only);
        assert!(order.is_reduce_only);
    }

    #[test]
    fn test_gtd_requires_expire_time() {
        let result = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .with_time_in_force(TimeInForce::GTD)
            .build();

        assert!(matches!(result, Err(OrderError::MissingExpireTime)));

        // With expire time should work
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .with_expire_time(Utc::now() + chrono::Duration::hours(24))
            .build()
            .unwrap();

        assert_eq!(order.time_in_force, TimeInForce::GTD);
        assert!(order.expire_time.is_some());
    }
}
