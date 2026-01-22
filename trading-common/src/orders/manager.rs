//! Order management and tracking.
//!
//! The `OrderManager` maintains the state of all orders and provides
//! methods for querying, submitting, canceling, and tracking orders.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

use rust_decimal::Decimal;

use super::events::{
    OrderAccepted, OrderCanceled, OrderDenied, OrderEventAny, OrderExpired, OrderFilled,
    OrderInitialized, OrderRejected, OrderSubmitted, OrderTriggered,
};
use super::order::{Order, OrderError};
use super::types::{
    AccountId, ClientOrderId, InstrumentId, LiquiditySide, OrderSide, OrderStatus,
    SessionEnforcement, StrategyId, TradeId, VenueOrderId,
};
use crate::instruments::{MarketStatus, SessionManager};

/// Result type for order manager operations.
pub type OrderManagerResult<T> = Result<T, OrderManagerError>;

/// Errors that can occur in the order manager.
#[derive(Debug, Clone, thiserror::Error)]
pub enum OrderManagerError {
    #[error("Order not found: {0}")]
    OrderNotFound(ClientOrderId),

    #[error("Duplicate order ID: {0}")]
    DuplicateOrderId(ClientOrderId),

    #[error("Order error: {0}")]
    OrderError(#[from] OrderError),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Order submission denied: {0}")]
    SubmissionDenied(String),

    #[error("Market closed for {symbol}: {reason}")]
    MarketClosed { symbol: String, reason: String },

    #[error("Market halted for {symbol}: {reason}")]
    MarketHalted { symbol: String, reason: String },

    #[error("Session not found for instrument: {0}")]
    SessionNotFound(InstrumentId),
}

/// Callback type for order events.
pub type OrderEventCallback = Box<dyn Fn(&OrderEventAny) + Send + Sync>;

/// Configuration for the order manager.
#[derive(Debug, Clone)]
pub struct OrderManagerConfig {
    /// Maximum number of open orders allowed
    pub max_open_orders: usize,
    /// Maximum number of orders per symbol
    pub max_orders_per_symbol: usize,
    /// Whether to allow duplicate client order IDs (across different orders)
    pub allow_duplicate_ids: bool,
    /// Default account ID to use if not specified
    pub default_account_id: AccountId,
    /// Default strategy ID to use if not specified
    pub default_strategy_id: StrategyId,
    /// Session enforcement mode (Disabled, Warn, Strict)
    pub session_enforcement: SessionEnforcement,
}

impl Default for OrderManagerConfig {
    fn default() -> Self {
        Self {
            max_open_orders: 1000,
            max_orders_per_symbol: 100,
            allow_duplicate_ids: false,
            default_account_id: AccountId::default(),
            default_strategy_id: StrategyId::default(),
            session_enforcement: SessionEnforcement::default(),
        }
    }
}

/// Manages all orders with thread-safe access.
///
/// The OrderManager is the central component for:
/// - Tracking all orders and their states
/// - Processing order events
/// - Querying orders by various criteria
/// - Managing order lifecycle
///
/// # Example
/// ```ignore
/// use trading_common::orders::{OrderManager, Order, OrderSide};
/// use rust_decimal_macros::dec;
///
/// let manager = OrderManager::new(OrderManagerConfig::default());
///
/// // Create and submit an order
/// let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1)).build()?;
/// let client_order_id = manager.submit_order(order).await?;
///
/// // Query orders
/// let open_orders = manager.get_open_orders().await;
/// let btc_orders = manager.get_orders_for_symbol("BTCUSDT").await;
/// ```
pub struct OrderManager {
    /// All orders indexed by client order ID
    orders: Arc<RwLock<HashMap<ClientOrderId, Order>>>,
    /// Index: venue order ID -> client order ID
    venue_to_client: Arc<RwLock<HashMap<VenueOrderId, ClientOrderId>>>,
    /// Index: instrument ID -> list of client order IDs
    orders_by_instrument: Arc<RwLock<HashMap<InstrumentId, Vec<ClientOrderId>>>>,
    /// Index: strategy ID -> list of client order IDs
    orders_by_strategy: Arc<RwLock<HashMap<StrategyId, Vec<ClientOrderId>>>>,
    /// Event history for each order
    order_events: Arc<RwLock<HashMap<ClientOrderId, Vec<OrderEventAny>>>>,
    /// Configuration
    config: OrderManagerConfig,
    /// Event callbacks
    event_callbacks: Arc<RwLock<Vec<OrderEventCallback>>>,
    /// Optional session manager for session-aware order validation
    session_manager: Option<Arc<SessionManager>>,
}

impl OrderManager {
    /// Create a new OrderManager with the given configuration.
    pub fn new(config: OrderManagerConfig) -> Self {
        Self {
            orders: Arc::new(RwLock::new(HashMap::new())),
            venue_to_client: Arc::new(RwLock::new(HashMap::new())),
            orders_by_instrument: Arc::new(RwLock::new(HashMap::new())),
            orders_by_strategy: Arc::new(RwLock::new(HashMap::new())),
            order_events: Arc::new(RwLock::new(HashMap::new())),
            config,
            event_callbacks: Arc::new(RwLock::new(Vec::new())),
            session_manager: None,
        }
    }

    /// Create a new OrderManager with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(OrderManagerConfig::default())
    }

    /// Create a new OrderManager with session manager for session-aware order validation.
    pub fn with_session_manager(
        config: OrderManagerConfig,
        session_manager: Arc<SessionManager>,
    ) -> Self {
        Self {
            orders: Arc::new(RwLock::new(HashMap::new())),
            venue_to_client: Arc::new(RwLock::new(HashMap::new())),
            orders_by_instrument: Arc::new(RwLock::new(HashMap::new())),
            orders_by_strategy: Arc::new(RwLock::new(HashMap::new())),
            order_events: Arc::new(RwLock::new(HashMap::new())),
            config,
            event_callbacks: Arc::new(RwLock::new(Vec::new())),
            session_manager: Some(session_manager),
        }
    }

    /// Set the session manager for session-aware order validation.
    pub fn set_session_manager(&mut self, session_manager: Arc<SessionManager>) {
        self.session_manager = Some(session_manager);
    }

    /// Get the current session enforcement mode.
    pub fn session_enforcement(&self) -> SessionEnforcement {
        self.config.session_enforcement
    }

    /// Register a callback for order events.
    pub async fn on_event(&self, callback: OrderEventCallback) {
        let mut callbacks = self.event_callbacks.write().await;
        callbacks.push(callback);
    }

    /// Submit a new order.
    ///
    /// Returns the client order ID on success.
    pub async fn submit_order(&self, order: Order) -> OrderManagerResult<ClientOrderId> {
        let client_order_id = order.client_order_id.clone();
        let instrument_id = order.instrument_id.clone();
        let strategy_id = order.strategy_id.clone();

        // Check for duplicate order ID
        {
            let orders = self.orders.read().await;
            if orders.contains_key(&client_order_id) && !self.config.allow_duplicate_ids {
                return Err(OrderManagerError::DuplicateOrderId(client_order_id));
            }
        }

        // Check max open orders
        {
            let orders = self.orders.read().await;
            let open_count = orders.values().filter(|o| o.is_open()).count();
            if open_count >= self.config.max_open_orders {
                return Err(OrderManagerError::SubmissionDenied(format!(
                    "Maximum open orders ({}) reached",
                    self.config.max_open_orders
                )));
            }
        }

        // Check max orders per symbol
        {
            let orders_by_instrument = self.orders_by_instrument.read().await;
            let orders = self.orders.read().await;
            if let Some(order_ids) = orders_by_instrument.get(&instrument_id) {
                let open_count = order_ids
                    .iter()
                    .filter_map(|id| orders.get(id))
                    .filter(|o| o.is_open())
                    .count();
                if open_count >= self.config.max_orders_per_symbol {
                    return Err(OrderManagerError::SubmissionDenied(format!(
                        "Maximum orders per symbol ({}) reached for {}",
                        self.config.max_orders_per_symbol, instrument_id
                    )));
                }
            }
        }

        // Session validation (if enabled and session manager is available)
        if self.config.session_enforcement.is_enabled() {
            if let Some(ref session_manager) = self.session_manager {
                if let Some(status) = session_manager.get_status(&instrument_id) {
                    match status {
                        MarketStatus::Closed => {
                            let reason = "Market is closed".to_string();
                            if self.config.session_enforcement.should_reject() {
                                return Err(OrderManagerError::MarketClosed {
                                    symbol: instrument_id.to_string(),
                                    reason,
                                });
                            } else {
                                // Warn mode - log but continue
                                warn!(
                                    "Order submitted while market closed: {} for {}",
                                    client_order_id, instrument_id
                                );
                            }
                        }
                        MarketStatus::Halted => {
                            let state = session_manager.get_state(&instrument_id);
                            let reason = state
                                .and_then(|s| s.reason)
                                .unwrap_or_else(|| "Trading halted".to_string());
                            if self.config.session_enforcement.should_reject() {
                                return Err(OrderManagerError::MarketHalted {
                                    symbol: instrument_id.to_string(),
                                    reason,
                                });
                            } else {
                                warn!(
                                    "Order submitted while market halted: {} for {} ({})",
                                    client_order_id, instrument_id, reason
                                );
                            }
                        }
                        MarketStatus::Maintenance => {
                            let reason = "Market in maintenance".to_string();
                            if self.config.session_enforcement.should_reject() {
                                return Err(OrderManagerError::MarketClosed {
                                    symbol: instrument_id.to_string(),
                                    reason,
                                });
                            } else {
                                warn!(
                                    "Order submitted during maintenance: {} for {}",
                                    client_order_id, instrument_id
                                );
                            }
                        }
                        // Open, PreMarket, AfterHours, Auction - these are tradeable states
                        MarketStatus::Open
                        | MarketStatus::PreMarket
                        | MarketStatus::AfterHours
                        | MarketStatus::Auction => {
                            // Order allowed
                        }
                    }
                }
                // If no status found, the symbol might not be registered - allow order
                // to proceed (fail-open behavior)
            }
        }

        // Generate OrderInitialized event
        let init_event = OrderInitialized::new(
            client_order_id.clone(),
            strategy_id.clone(),
            instrument_id.clone(),
            order.side,
            order.order_type,
            order.quantity,
            order.price,
            order.trigger_price,
            order.time_in_force,
            order.expire_time,
            order.is_post_only,
            order.is_reduce_only,
        );

        // Store the order
        {
            let mut orders = self.orders.write().await;
            orders.insert(client_order_id.clone(), order);
        }

        // Update indexes
        {
            let mut orders_by_instrument = self.orders_by_instrument.write().await;
            orders_by_instrument
                .entry(instrument_id)
                .or_default()
                .push(client_order_id.clone());
        }

        {
            let mut orders_by_strategy = self.orders_by_strategy.write().await;
            orders_by_strategy
                .entry(strategy_id)
                .or_default()
                .push(client_order_id.clone());
        }

        // Store and dispatch event
        self.store_and_dispatch_event(client_order_id.clone(), init_event.into())
            .await;

        Ok(client_order_id)
    }

    /// Get an order by client order ID.
    pub async fn get_order(&self, client_order_id: &ClientOrderId) -> Option<Order> {
        let orders = self.orders.read().await;
        orders.get(client_order_id).cloned()
    }

    /// Get an order by venue order ID.
    pub async fn get_order_by_venue_id(&self, venue_order_id: &VenueOrderId) -> Option<Order> {
        let venue_to_client = self.venue_to_client.read().await;
        if let Some(client_id) = venue_to_client.get(venue_order_id) {
            let orders = self.orders.read().await;
            return orders.get(client_id).cloned();
        }
        None
    }

    /// Get all orders.
    pub async fn get_all_orders(&self) -> Vec<Order> {
        let orders = self.orders.read().await;
        orders.values().cloned().collect()
    }

    /// Get all open orders.
    pub async fn get_open_orders(&self) -> Vec<Order> {
        let orders = self.orders.read().await;
        orders.values().filter(|o| o.is_open()).cloned().collect()
    }

    /// Get all closed orders.
    pub async fn get_closed_orders(&self) -> Vec<Order> {
        let orders = self.orders.read().await;
        orders
            .values()
            .filter(|o| o.is_closed())
            .cloned()
            .collect()
    }

    /// Get orders for a specific symbol.
    pub async fn get_orders_for_symbol(&self, symbol: &str) -> Vec<Order> {
        let orders = self.orders.read().await;
        orders
            .values()
            .filter(|o| o.symbol() == symbol)
            .cloned()
            .collect()
    }

    /// Get open orders for a specific symbol.
    pub async fn get_open_orders_for_symbol(&self, symbol: &str) -> Vec<Order> {
        let orders = self.orders.read().await;
        orders
            .values()
            .filter(|o| o.symbol() == symbol && o.is_open())
            .cloned()
            .collect()
    }

    /// Get orders for a specific instrument.
    pub async fn get_orders_for_instrument(&self, instrument_id: &InstrumentId) -> Vec<Order> {
        let orders_by_instrument = self.orders_by_instrument.read().await;
        let orders = self.orders.read().await;

        orders_by_instrument
            .get(instrument_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| orders.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get orders for a specific strategy.
    pub async fn get_orders_for_strategy(&self, strategy_id: &StrategyId) -> Vec<Order> {
        let orders_by_strategy = self.orders_by_strategy.read().await;
        let orders = self.orders.read().await;

        orders_by_strategy
            .get(strategy_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| orders.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get orders with a specific status.
    pub async fn get_orders_by_status(&self, status: OrderStatus) -> Vec<Order> {
        let orders = self.orders.read().await;
        orders
            .values()
            .filter(|o| o.status == status)
            .cloned()
            .collect()
    }

    /// Get the count of open orders.
    pub async fn open_order_count(&self) -> usize {
        let orders = self.orders.read().await;
        orders.values().filter(|o| o.is_open()).count()
    }

    /// Get the count of orders for a symbol.
    pub async fn order_count_for_symbol(&self, symbol: &str) -> usize {
        let orders = self.orders.read().await;
        orders.values().filter(|o| o.symbol() == symbol).count()
    }

    /// Mark an order as submitted.
    pub async fn mark_submitted(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        order.transition_to(OrderStatus::Submitted)?;

        let event = OrderSubmitted::new(client_order_id.clone(), account_id);

        drop(orders); // Release lock before dispatching
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Mark an order as accepted by the venue.
    pub async fn mark_accepted(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        // Update venue ID mapping
        {
            let mut venue_to_client = self.venue_to_client.write().await;
            venue_to_client.insert(venue_order_id.clone(), client_order_id.clone());
        }

        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        order.transition_to(OrderStatus::Accepted)?;
        order.set_venue_order_id(venue_order_id.clone());

        let event = OrderAccepted::new(client_order_id.clone(), venue_order_id, account_id);

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Mark an order as rejected.
    pub async fn mark_rejected(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
        reason: impl Into<String>,
    ) -> OrderManagerResult<()> {
        let reason_str = reason.into();
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        order.reject(&reason_str)?;

        let event = OrderRejected::new(client_order_id.clone(), account_id, reason_str);

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Mark an order as denied (pre-submission risk check failed).
    pub async fn mark_denied(
        &self,
        client_order_id: &ClientOrderId,
        reason: impl Into<String>,
    ) -> OrderManagerResult<()> {
        let reason_str = reason.into();
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        order.deny(&reason_str)?;

        let event = OrderDenied::new(client_order_id.clone(), reason_str);

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Apply a fill to an order.
    #[allow(clippy::too_many_arguments)]
    pub async fn apply_fill(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        fill_qty: Decimal,
        fill_price: Decimal,
        commission: Decimal,
        commission_currency: String,
        liquidity_side: LiquiditySide,
    ) -> OrderManagerResult<()> {
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        // Apply fill to order
        order.apply_fill(
            fill_qty,
            fill_price,
            trade_id.clone(),
            commission,
            liquidity_side,
        )?;

        // Create fill event
        let event = OrderFilled::new(
            client_order_id.clone(),
            venue_order_id,
            account_id,
            order.instrument_id.clone(),
            trade_id,
            order.strategy_id.clone(),
            order.side,
            order.order_type,
            fill_qty,
            fill_price,
            order.filled_qty,
            order.leaves_qty,
            order.instrument_id.symbol.clone(), // Use symbol as currency for now
            commission,
            commission_currency,
            liquidity_side,
        );

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Cancel an order.
    pub async fn cancel_order(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        let venue_order_id = order.venue_order_id.clone();
        order.cancel()?;

        let event = OrderCanceled::new(client_order_id.clone(), venue_order_id, account_id);

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Expire an order.
    pub async fn expire_order(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        let venue_order_id = order.venue_order_id.clone();
        order.expire()?;

        let event = OrderExpired::new(client_order_id.clone(), venue_order_id, account_id);

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Mark an order as triggered (for stop/conditional orders).
    pub async fn mark_triggered(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        let mut orders = self.orders.write().await;
        let order = orders
            .get_mut(client_order_id)
            .ok_or_else(|| OrderManagerError::OrderNotFound(client_order_id.clone()))?;

        let venue_order_id = order.venue_order_id.clone();
        order.transition_to(OrderStatus::Triggered)?;

        let event = OrderTriggered::new(client_order_id.clone(), venue_order_id, account_id);

        drop(orders);
        self.store_and_dispatch_event(client_order_id.clone(), event.into())
            .await;

        Ok(())
    }

    /// Cancel all open orders.
    pub async fn cancel_all_orders(&self, account_id: AccountId) -> OrderManagerResult<Vec<ClientOrderId>> {
        let orders = self.orders.read().await;
        let open_order_ids: Vec<ClientOrderId> = orders
            .values()
            .filter(|o| o.is_cancelable())
            .map(|o| o.client_order_id.clone())
            .collect();
        drop(orders);

        let mut canceled = Vec::new();
        for order_id in open_order_ids {
            if self.cancel_order(&order_id, account_id.clone()).await.is_ok() {
                canceled.push(order_id);
            }
        }

        Ok(canceled)
    }

    /// Cancel all open orders for a specific symbol.
    pub async fn cancel_orders_for_symbol(
        &self,
        symbol: &str,
        account_id: AccountId,
    ) -> OrderManagerResult<Vec<ClientOrderId>> {
        let orders = self.orders.read().await;
        let open_order_ids: Vec<ClientOrderId> = orders
            .values()
            .filter(|o| o.symbol() == symbol && o.is_cancelable())
            .map(|o| o.client_order_id.clone())
            .collect();
        drop(orders);

        let mut canceled = Vec::new();
        for order_id in open_order_ids {
            if self.cancel_order(&order_id, account_id.clone()).await.is_ok() {
                canceled.push(order_id);
            }
        }

        Ok(canceled)
    }

    /// Get the event history for an order.
    pub async fn get_order_events(&self, client_order_id: &ClientOrderId) -> Vec<OrderEventAny> {
        let events = self.order_events.read().await;
        events.get(client_order_id).cloned().unwrap_or_default()
    }

    /// Check if an order exists.
    pub async fn has_order(&self, client_order_id: &ClientOrderId) -> bool {
        let orders = self.orders.read().await;
        orders.contains_key(client_order_id)
    }

    /// Get total filled quantity for a symbol across all orders.
    pub async fn get_total_filled_for_symbol(&self, symbol: &str, side: OrderSide) -> Decimal {
        let orders = self.orders.read().await;
        orders
            .values()
            .filter(|o| o.symbol() == symbol && o.side == side)
            .map(|o| o.filled_qty)
            .sum()
    }

    /// Get statistics for the order manager.
    pub async fn get_stats(&self) -> OrderManagerStats {
        let orders = self.orders.read().await;

        let total_orders = orders.len();
        let open_orders = orders.values().filter(|o| o.is_open()).count();
        let filled_orders = orders
            .values()
            .filter(|o| o.status == OrderStatus::Filled)
            .count();
        let canceled_orders = orders
            .values()
            .filter(|o| o.status == OrderStatus::Canceled)
            .count();
        let rejected_orders = orders
            .values()
            .filter(|o| o.status == OrderStatus::Rejected || o.status == OrderStatus::Denied)
            .count();

        let total_filled_qty: Decimal = orders.values().map(|o| o.filled_qty).sum();

        let total_commission: Decimal = orders.values().map(|o| o.commission).sum();

        OrderManagerStats {
            total_orders,
            open_orders,
            filled_orders,
            canceled_orders,
            rejected_orders,
            total_filled_qty,
            total_commission,
        }
    }

    /// Clear all closed orders (for memory management).
    pub async fn clear_closed_orders(&self) {
        let mut orders = self.orders.write().await;
        let closed_ids: Vec<ClientOrderId> = orders
            .iter()
            .filter(|(_, o)| o.is_closed())
            .map(|(id, _)| id.clone())
            .collect();

        for id in &closed_ids {
            orders.remove(id);
        }

        // Also clear from indexes and events
        let mut order_events = self.order_events.write().await;
        for id in &closed_ids {
            order_events.remove(id);
        }

        // Note: We don't remove from orders_by_instrument/strategy indexes
        // as that would require more complex cleanup. In production, consider
        // periodic full index rebuilding or more sophisticated index management.
    }

    /// Reset the order manager (clear all orders).
    pub async fn reset(&self) {
        let mut orders = self.orders.write().await;
        orders.clear();

        let mut venue_to_client = self.venue_to_client.write().await;
        venue_to_client.clear();

        let mut orders_by_instrument = self.orders_by_instrument.write().await;
        orders_by_instrument.clear();

        let mut orders_by_strategy = self.orders_by_strategy.write().await;
        orders_by_strategy.clear();

        let mut order_events = self.order_events.write().await;
        order_events.clear();
    }

    // === Private Methods ===

    async fn store_and_dispatch_event(&self, client_order_id: ClientOrderId, event: OrderEventAny) {
        // Store event
        {
            let mut events = self.order_events.write().await;
            events
                .entry(client_order_id)
                .or_default()
                .push(event.clone());
        }

        // Dispatch to callbacks
        let callbacks = self.event_callbacks.read().await;
        for callback in callbacks.iter() {
            callback(&event);
        }
    }
}

/// Statistics for the order manager.
#[derive(Debug, Clone, Default)]
pub struct OrderManagerStats {
    pub total_orders: usize,
    pub open_orders: usize,
    pub filled_orders: usize,
    pub canceled_orders: usize,
    pub rejected_orders: usize,
    pub total_filled_qty: Decimal,
    pub total_commission: Decimal,
}

/// A synchronous wrapper for use in non-async contexts (like backtesting).
///
/// This provides a simpler API that doesn't require async/await,
/// suitable for backtesting where everything runs in a single thread.
pub struct SyncOrderManager {
    inner: OrderManager,
    runtime: tokio::runtime::Runtime,
}

impl SyncOrderManager {
    /// Create a new synchronous order manager.
    pub fn new(config: OrderManagerConfig) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        Self {
            inner: OrderManager::new(config),
            runtime,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(OrderManagerConfig::default())
    }

    /// Submit an order synchronously.
    pub fn submit_order(&self, order: Order) -> OrderManagerResult<ClientOrderId> {
        self.runtime.block_on(self.inner.submit_order(order))
    }

    /// Get an order by ID.
    pub fn get_order(&self, client_order_id: &ClientOrderId) -> Option<Order> {
        self.runtime.block_on(self.inner.get_order(client_order_id))
    }

    /// Get all open orders.
    pub fn get_open_orders(&self) -> Vec<Order> {
        self.runtime.block_on(self.inner.get_open_orders())
    }

    /// Mark order as submitted.
    pub fn mark_submitted(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        self.runtime
            .block_on(self.inner.mark_submitted(client_order_id, account_id))
    }

    /// Mark order as accepted.
    pub fn mark_accepted(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        self.runtime.block_on(
            self.inner
                .mark_accepted(client_order_id, venue_order_id, account_id),
        )
    }

    /// Apply a fill to an order.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_fill(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        fill_qty: Decimal,
        fill_price: Decimal,
        commission: Decimal,
        commission_currency: String,
        liquidity_side: LiquiditySide,
    ) -> OrderManagerResult<()> {
        self.runtime.block_on(self.inner.apply_fill(
            client_order_id,
            venue_order_id,
            account_id,
            trade_id,
            fill_qty,
            fill_price,
            commission,
            commission_currency,
            liquidity_side,
        ))
    }

    /// Cancel an order.
    pub fn cancel_order(
        &self,
        client_order_id: &ClientOrderId,
        account_id: AccountId,
    ) -> OrderManagerResult<()> {
        self.runtime
            .block_on(self.inner.cancel_order(client_order_id, account_id))
    }

    /// Get order manager stats.
    pub fn get_stats(&self) -> OrderManagerStats {
        self.runtime.block_on(self.inner.get_stats())
    }

    /// Reset the order manager.
    pub fn reset(&self) {
        self.runtime.block_on(self.inner.reset())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use tokio::task::JoinHandle;

    /// Helper to join all handles (replacement for futures::future::join_all)
    async fn tokio_join_all<T>(handles: Vec<JoinHandle<T>>) -> Vec<Result<T, tokio::task::JoinError>> {
        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            results.push(handle.await);
        }
        results
    }

    #[tokio::test]
    async fn test_submit_order() {
        let manager = OrderManager::with_defaults();

        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        let client_id = manager.submit_order(order).await.unwrap();

        assert!(manager.has_order(&client_id).await);

        let retrieved = manager.get_order(&client_id).await.unwrap();
        assert_eq!(retrieved.status, OrderStatus::Initialized);
        assert_eq!(retrieved.symbol(), "BTCUSDT");
    }

    #[tokio::test]
    async fn test_order_lifecycle() {
        let manager = OrderManager::with_defaults();

        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .build()
            .unwrap();

        let client_id = manager.submit_order(order).await.unwrap();
        let account_id = AccountId::new("test-account");

        // Submit
        manager
            .mark_submitted(&client_id, account_id.clone())
            .await
            .unwrap();
        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Submitted);

        // Accept
        let venue_id = VenueOrderId::new("venue-123");
        manager
            .mark_accepted(&client_id, venue_id.clone(), account_id.clone())
            .await
            .unwrap();
        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Accepted);
        assert_eq!(order.venue_order_id, Some(venue_id.clone()));

        // Partial fill
        manager
            .apply_fill(
                &client_id,
                venue_id.clone(),
                account_id.clone(),
                TradeId::generate(),
                dec!(0.5),
                dec!(50000),
                dec!(0.05),
                "USDT".to_string(),
                LiquiditySide::Taker,
            )
            .await
            .unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty, dec!(0.5));

        // Complete fill
        manager
            .apply_fill(
                &client_id,
                venue_id,
                account_id,
                TradeId::generate(),
                dec!(0.5),
                dec!(50010),
                dec!(0.05),
                "USDT".to_string(),
                LiquiditySide::Maker,
            )
            .await
            .unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
        assert!(order.is_closed());
    }

    #[tokio::test]
    async fn test_cancel_order() {
        let manager = OrderManager::with_defaults();

        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .build()
            .unwrap();

        let client_id = manager.submit_order(order).await.unwrap();
        let account_id = AccountId::new("test-account");

        manager
            .mark_submitted(&client_id, account_id.clone())
            .await
            .unwrap();
        manager
            .mark_accepted(
                &client_id,
                VenueOrderId::new("venue-123"),
                account_id.clone(),
            )
            .await
            .unwrap();

        manager.cancel_order(&client_id, account_id).await.unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Canceled);
    }

    #[tokio::test]
    async fn test_max_open_orders_limit() {
        let config = OrderManagerConfig {
            max_open_orders: 2,
            ..Default::default()
        };
        let manager = OrderManager::new(config);

        // Submit 2 orders - should work
        let order1 = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let order2 = Order::market("ETHUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        manager.submit_order(order1).await.unwrap();
        manager.submit_order(order2).await.unwrap();

        // Third order should fail
        let order3 = Order::market("LTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        let result = manager.submit_order(order3).await;
        assert!(matches!(
            result,
            Err(OrderManagerError::SubmissionDenied(_))
        ));
    }

    #[tokio::test]
    async fn test_get_orders_by_symbol() {
        let manager = OrderManager::with_defaults();

        let btc_order1 = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let btc_order2 = Order::market("BTCUSDT", OrderSide::Sell, dec!(0.5))
            .build()
            .unwrap();
        let eth_order = Order::market("ETHUSDT", OrderSide::Buy, dec!(2.0))
            .build()
            .unwrap();

        manager.submit_order(btc_order1).await.unwrap();
        manager.submit_order(btc_order2).await.unwrap();
        manager.submit_order(eth_order).await.unwrap();

        let btc_orders = manager.get_orders_for_symbol("BTCUSDT").await;
        assert_eq!(btc_orders.len(), 2);

        let eth_orders = manager.get_orders_for_symbol("ETHUSDT").await;
        assert_eq!(eth_orders.len(), 1);
    }

    #[tokio::test]
    async fn test_order_events() {
        let manager = OrderManager::with_defaults();

        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        let client_id = manager.submit_order(order).await.unwrap();
        let account_id = AccountId::new("test-account");

        manager
            .mark_submitted(&client_id, account_id.clone())
            .await
            .unwrap();
        manager
            .mark_accepted(
                &client_id,
                VenueOrderId::new("venue-123"),
                account_id.clone(),
            )
            .await
            .unwrap();

        let events = manager.get_order_events(&client_id).await;
        assert_eq!(events.len(), 3); // Initialized, Submitted, Accepted
    }

    #[tokio::test]
    async fn test_stats() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("test-account");

        // Submit and fill an order
        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let client_id = manager.submit_order(order).await.unwrap();
        manager
            .mark_submitted(&client_id, account_id.clone())
            .await
            .unwrap();
        manager
            .mark_accepted(
                &client_id,
                VenueOrderId::new("v1"),
                account_id.clone(),
            )
            .await
            .unwrap();
        manager
            .apply_fill(
                &client_id,
                VenueOrderId::new("v1"),
                account_id.clone(),
                TradeId::generate(),
                dec!(1.0),
                dec!(50000),
                dec!(0.1),
                "USDT".to_string(),
                LiquiditySide::Taker,
            )
            .await
            .unwrap();

        // Submit and cancel another order
        let order2 = Order::limit("ETHUSDT", OrderSide::Sell, dec!(2.0), dec!(3000))
            .build()
            .unwrap();
        let client_id2 = manager.submit_order(order2).await.unwrap();
        manager
            .mark_submitted(&client_id2, account_id.clone())
            .await
            .unwrap();
        manager
            .mark_accepted(
                &client_id2,
                VenueOrderId::new("v2"),
                account_id.clone(),
            )
            .await
            .unwrap();
        manager
            .cancel_order(&client_id2, account_id)
            .await
            .unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total_orders, 2);
        assert_eq!(stats.open_orders, 0);
        assert_eq!(stats.filled_orders, 1);
        assert_eq!(stats.canceled_orders, 1);
        assert_eq!(stats.total_filled_qty, dec!(1.0));
        assert_eq!(stats.total_commission, dec!(0.1));
    }

    #[test]
    fn test_sync_order_manager() {
        let manager = SyncOrderManager::with_defaults();

        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        let client_id = manager.submit_order(order).unwrap();

        let retrieved = manager.get_order(&client_id).unwrap();
        assert_eq!(retrieved.symbol(), "BTCUSDT");

        let open_orders = manager.get_open_orders();
        assert_eq!(open_orders.len(), 1);
    }

    // === Integration Tests ===

    #[tokio::test]
    async fn test_full_order_lifecycle_integration() {
        // Test complete order flow: initialized -> submitted -> accepted -> partial fill -> complete fill
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("integration-test-account");

        // Create multiple orders
        let btc_buy = Order::limit("BTCUSDT", OrderSide::Buy, dec!(2.0), dec!(50000))
            .with_strategy_id(StrategyId::new("scalping-strategy"))
            .build()
            .unwrap();
        let btc_sell = Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.5), dec!(52000))
            .with_strategy_id(StrategyId::new("scalping-strategy"))
            .build()
            .unwrap();
        let eth_buy = Order::market("ETHUSDT", OrderSide::Buy, dec!(10.0))
            .with_strategy_id(StrategyId::new("momentum-strategy"))
            .build()
            .unwrap();

        // Submit all orders
        let btc_buy_id = manager.submit_order(btc_buy).await.unwrap();
        let btc_sell_id = manager.submit_order(btc_sell).await.unwrap();
        let eth_buy_id = manager.submit_order(eth_buy).await.unwrap();

        // Verify initial state
        assert_eq!(manager.open_order_count().await, 3);
        assert_eq!(manager.order_count_for_symbol("BTCUSDT").await, 2);
        assert_eq!(manager.order_count_for_symbol("ETHUSDT").await, 1);

        // Move orders through lifecycle
        for id in [&btc_buy_id, &btc_sell_id, &eth_buy_id] {
            manager.mark_submitted(id, account_id.clone()).await.unwrap();
        }

        // Accept orders with venue IDs
        manager.mark_accepted(&btc_buy_id, VenueOrderId::new("BTC-BUY-001"), account_id.clone()).await.unwrap();
        manager.mark_accepted(&btc_sell_id, VenueOrderId::new("BTC-SELL-001"), account_id.clone()).await.unwrap();
        manager.mark_accepted(&eth_buy_id, VenueOrderId::new("ETH-BUY-001"), account_id.clone()).await.unwrap();

        // Verify venue ID lookup works
        let order_by_venue = manager.get_order_by_venue_id(&VenueOrderId::new("BTC-BUY-001")).await;
        assert!(order_by_venue.is_some());
        assert_eq!(order_by_venue.unwrap().client_order_id, btc_buy_id);

        // Partial fill BTC buy
        manager.apply_fill(
            &btc_buy_id,
            VenueOrderId::new("BTC-BUY-001"),
            account_id.clone(),
            TradeId::generate(),
            dec!(1.0),
            dec!(49999),
            dec!(0.1),
            "USDT".to_string(),
            LiquiditySide::Maker,
        ).await.unwrap();

        let btc_buy_order = manager.get_order(&btc_buy_id).await.unwrap();
        assert_eq!(btc_buy_order.status, OrderStatus::PartiallyFilled);
        assert_eq!(btc_buy_order.filled_qty, dec!(1.0));
        assert_eq!(btc_buy_order.leaves_qty, dec!(1.0));

        // Complete BTC buy fill
        manager.apply_fill(
            &btc_buy_id,
            VenueOrderId::new("BTC-BUY-001"),
            account_id.clone(),
            TradeId::generate(),
            dec!(1.0),
            dec!(50001),
            dec!(0.1),
            "USDT".to_string(),
            LiquiditySide::Taker,
        ).await.unwrap();

        let btc_buy_order = manager.get_order(&btc_buy_id).await.unwrap();
        assert_eq!(btc_buy_order.status, OrderStatus::Filled);
        assert!(btc_buy_order.is_closed());

        // Fill ETH order completely in one shot
        manager.apply_fill(
            &eth_buy_id,
            VenueOrderId::new("ETH-BUY-001"),
            account_id.clone(),
            TradeId::generate(),
            dec!(10.0),
            dec!(3500),
            dec!(0.5),
            "USDT".to_string(),
            LiquiditySide::Taker,
        ).await.unwrap();

        // Cancel BTC sell order
        manager.cancel_order(&btc_sell_id, account_id.clone()).await.unwrap();
        let btc_sell_order = manager.get_order(&btc_sell_id).await.unwrap();
        assert_eq!(btc_sell_order.status, OrderStatus::Canceled);

        // Final stats verification
        let stats = manager.get_stats().await;
        assert_eq!(stats.total_orders, 3);
        assert_eq!(stats.open_orders, 0);
        assert_eq!(stats.filled_orders, 2);
        assert_eq!(stats.canceled_orders, 1);
        assert_eq!(stats.total_filled_qty, dec!(12.0)); // 2.0 + 10.0
        assert_eq!(stats.total_commission, dec!(0.7)); // 0.1 + 0.1 + 0.5

        // Verify event history
        let btc_buy_events = manager.get_order_events(&btc_buy_id).await;
        assert_eq!(btc_buy_events.len(), 5); // Init, Submit, Accept, PartialFill, Fill
    }

    #[tokio::test]
    async fn test_event_callback_integration() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let manager = OrderManager::with_defaults();
        let event_count = Arc::new(AtomicUsize::new(0));
        let event_count_clone = event_count.clone();

        // Register callback
        manager.on_event(Box::new(move |_event| {
            event_count_clone.fetch_add(1, Ordering::SeqCst);
        })).await;

        // Submit and process order
        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let client_id = manager.submit_order(order).await.unwrap();
        let account_id = AccountId::new("callback-test");

        manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&client_id, VenueOrderId::new("v1"), account_id.clone()).await.unwrap();
        manager.apply_fill(
            &client_id,
            VenueOrderId::new("v1"),
            account_id,
            TradeId::generate(),
            dec!(1.0),
            dec!(50000),
            dec!(0.1),
            "USDT".to_string(),
            LiquiditySide::Taker,
        ).await.unwrap();

        // Should have received 4 events: Init, Submit, Accept, Fill
        assert_eq!(event_count.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn test_order_rejection_flow() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("reject-test");

        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .build()
            .unwrap();
        let client_id = manager.submit_order(order).await.unwrap();

        manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
        manager.mark_rejected(&client_id, account_id, "Insufficient funds").await.unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Rejected);
        assert!(order.is_closed());

        let stats = manager.get_stats().await;
        assert_eq!(stats.rejected_orders, 1);
    }

    #[tokio::test]
    async fn test_order_denial_flow() {
        let manager = OrderManager::with_defaults();

        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1000000.0)) // Very large order
            .build()
            .unwrap();
        let client_id = manager.submit_order(order).await.unwrap();

        manager.mark_denied(&client_id, "Order size exceeds risk limit").await.unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Denied);
        assert!(order.is_closed());

        let stats = manager.get_stats().await;
        assert_eq!(stats.rejected_orders, 1); // Denied counts as rejected
    }

    #[tokio::test]
    async fn test_order_expiration_flow() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("expire-test");

        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(40000))
            .build()
            .unwrap();
        let client_id = manager.submit_order(order).await.unwrap();

        manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&client_id, VenueOrderId::new("v1"), account_id.clone()).await.unwrap();
        manager.expire_order(&client_id, account_id).await.unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Expired);
        assert!(order.is_closed());
    }

    #[tokio::test]
    async fn test_stop_order_trigger_flow() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("trigger-test");

        let order = Order::stop("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(48000))
            .build()
            .unwrap();
        let client_id = manager.submit_order(order).await.unwrap();

        manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&client_id, VenueOrderId::new("v1"), account_id.clone()).await.unwrap();

        // Trigger the stop order (price reached trigger level)
        manager.mark_triggered(&client_id, account_id.clone()).await.unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Triggered);

        // Now fill the triggered order
        manager.apply_fill(
            &client_id,
            VenueOrderId::new("v1"),
            account_id,
            TradeId::generate(),
            dec!(1.0),
            dec!(47900),
            dec!(0.1),
            "USDT".to_string(),
            LiquiditySide::Taker,
        ).await.unwrap();

        let order = manager.get_order(&client_id).await.unwrap();
        assert_eq!(order.status, OrderStatus::Filled);
    }

    #[tokio::test]
    async fn test_cancel_all_orders() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("cancel-all-test");

        // Submit multiple orders
        for (i, symbol) in ["BTCUSDT", "ETHUSDT", "LTCUSDT"].iter().enumerate() {
            let order = Order::limit(*symbol, OrderSide::Buy, dec!(1.0), dec!(100))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
            manager.mark_accepted(&client_id, VenueOrderId::new(format!("venue-{}", i)), account_id.clone()).await.unwrap();
        }

        assert_eq!(manager.open_order_count().await, 3);

        let canceled = manager.cancel_all_orders(account_id).await.unwrap();
        assert_eq!(canceled.len(), 3);
        assert_eq!(manager.open_order_count().await, 0);
    }

    #[tokio::test]
    async fn test_cancel_orders_for_symbol() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("cancel-symbol-test");

        // Submit 2 BTC orders and 1 ETH order
        for i in 0..2 {
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(100))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
            manager.mark_accepted(&client_id, VenueOrderId::new(format!("btc-venue-{}", i)), account_id.clone()).await.unwrap();
        }

        let eth_order = Order::limit("ETHUSDT", OrderSide::Buy, dec!(1.0), dec!(100))
            .build()
            .unwrap();
        let eth_id = manager.submit_order(eth_order).await.unwrap();
        manager.mark_submitted(&eth_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&eth_id, VenueOrderId::new("eth-venue-1"), account_id.clone()).await.unwrap();

        assert_eq!(manager.open_order_count().await, 3);

        let canceled = manager.cancel_orders_for_symbol("BTCUSDT", account_id).await.unwrap();
        assert_eq!(canceled.len(), 2);
        assert_eq!(manager.open_order_count().await, 1);

        // ETH order should still be open
        let eth_order = manager.get_order(&eth_id).await.unwrap();
        assert!(eth_order.is_open());
    }

    #[tokio::test]
    async fn test_get_orders_by_strategy() {
        let manager = OrderManager::with_defaults();

        let scalp_order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .with_strategy_id(StrategyId::new("scalping"))
            .build()
            .unwrap();
        let momentum_order = Order::market("ETHUSDT", OrderSide::Buy, dec!(2.0))
            .with_strategy_id(StrategyId::new("momentum"))
            .build()
            .unwrap();

        manager.submit_order(scalp_order).await.unwrap();
        manager.submit_order(momentum_order).await.unwrap();

        let scalp_orders = manager.get_orders_for_strategy(&StrategyId::new("scalping")).await;
        assert_eq!(scalp_orders.len(), 1);
        assert_eq!(scalp_orders[0].symbol(), "BTCUSDT");

        let momentum_orders = manager.get_orders_for_strategy(&StrategyId::new("momentum")).await;
        assert_eq!(momentum_orders.len(), 1);
        assert_eq!(momentum_orders[0].symbol(), "ETHUSDT");
    }

    #[tokio::test]
    async fn test_get_orders_by_instrument() {
        let manager = OrderManager::with_defaults();

        // Submit orders for different instruments
        for _ in 0..3 {
            let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
                .build()
                .unwrap();
            manager.submit_order(order).await.unwrap();
        }

        // Default venue is "DEFAULT" when not specified
        let instrument_id = InstrumentId::new("BTCUSDT", "DEFAULT");
        let orders = manager.get_orders_for_instrument(&instrument_id).await;
        assert_eq!(orders.len(), 3);
    }

    #[tokio::test]
    async fn test_get_total_filled_by_side() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("side-test");

        // Create and fill buy orders
        let buy_order = Order::market("BTCUSDT", OrderSide::Buy, dec!(2.0))
            .build()
            .unwrap();
        let buy_id = manager.submit_order(buy_order).await.unwrap();
        manager.mark_submitted(&buy_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&buy_id, VenueOrderId::new("b1"), account_id.clone()).await.unwrap();
        manager.apply_fill(&buy_id, VenueOrderId::new("b1"), account_id.clone(),
            TradeId::generate(), dec!(2.0), dec!(50000), dec!(0.1), "USDT".to_string(),
            LiquiditySide::Taker).await.unwrap();

        // Create and fill sell order
        let sell_order = Order::market("BTCUSDT", OrderSide::Sell, dec!(1.0))
            .build()
            .unwrap();
        let sell_id = manager.submit_order(sell_order).await.unwrap();
        manager.mark_submitted(&sell_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&sell_id, VenueOrderId::new("s1"), account_id.clone()).await.unwrap();
        manager.apply_fill(&sell_id, VenueOrderId::new("s1"), account_id,
            TradeId::generate(), dec!(1.0), dec!(51000), dec!(0.1), "USDT".to_string(),
            LiquiditySide::Taker).await.unwrap();

        let buy_filled = manager.get_total_filled_for_symbol("BTCUSDT", OrderSide::Buy).await;
        let sell_filled = manager.get_total_filled_for_symbol("BTCUSDT", OrderSide::Sell).await;

        assert_eq!(buy_filled, dec!(2.0));
        assert_eq!(sell_filled, dec!(1.0));
    }

    #[tokio::test]
    async fn test_clear_closed_orders() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("clear-test");

        // Create and fill an order (closed)
        let filled_order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let filled_id = manager.submit_order(filled_order).await.unwrap();
        manager.mark_submitted(&filled_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&filled_id, VenueOrderId::new("v1"), account_id.clone()).await.unwrap();
        manager.apply_fill(&filled_id, VenueOrderId::new("v1"), account_id.clone(),
            TradeId::generate(), dec!(1.0), dec!(50000), dec!(0.1), "USDT".to_string(),
            LiquiditySide::Taker).await.unwrap();

        // Create an open order
        let open_order = Order::limit("ETHUSDT", OrderSide::Buy, dec!(1.0), dec!(3000))
            .build()
            .unwrap();
        let open_id = manager.submit_order(open_order).await.unwrap();
        manager.mark_submitted(&open_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&open_id, VenueOrderId::new("v2"), account_id).await.unwrap();

        assert_eq!(manager.get_all_orders().await.len(), 2);

        manager.clear_closed_orders().await;

        let remaining = manager.get_all_orders().await;
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].symbol(), "ETHUSDT");

        // Verify closed order is gone
        assert!(manager.get_order(&filled_id).await.is_none());
        // Events should also be cleared
        assert!(manager.get_order_events(&filled_id).await.is_empty());
    }

    #[tokio::test]
    async fn test_reset() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("reset-test");

        // Create some orders
        for i in 0..5 {
            let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            manager.mark_submitted(&client_id, account_id.clone()).await.unwrap();
            manager.mark_accepted(&client_id, VenueOrderId::new(format!("reset-venue-{}", i)), account_id.clone()).await.unwrap();
        }

        assert_eq!(manager.get_all_orders().await.len(), 5);

        manager.reset().await;

        assert_eq!(manager.get_all_orders().await.len(), 0);
        assert_eq!(manager.open_order_count().await, 0);
        let stats = manager.get_stats().await;
        assert_eq!(stats.total_orders, 0);
    }

    #[tokio::test]
    async fn test_max_orders_per_symbol() {
        let config = OrderManagerConfig {
            max_orders_per_symbol: 2,
            ..Default::default()
        };
        let manager = OrderManager::new(config);

        // Submit 2 orders for BTCUSDT - should work
        for _ in 0..2 {
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
                .build()
                .unwrap();
            manager.submit_order(order).await.unwrap();
        }

        // Third order for BTCUSDT should fail
        let order = Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(51000))
            .build()
            .unwrap();
        let result = manager.submit_order(order).await;
        assert!(matches!(result, Err(OrderManagerError::SubmissionDenied(_))));

        // But order for different symbol should work
        let eth_order = Order::limit("ETHUSDT", OrderSide::Buy, dec!(1.0), dec!(3000))
            .build()
            .unwrap();
        assert!(manager.submit_order(eth_order).await.is_ok());
    }

    #[tokio::test]
    async fn test_duplicate_order_id_rejected() {
        let config = OrderManagerConfig {
            allow_duplicate_ids: false,
            ..Default::default()
        };
        let manager = OrderManager::new(config);

        let client_order_id = ClientOrderId::new("duplicate-id");

        let order1 = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .with_client_order_id(client_order_id.clone())
            .build()
            .unwrap();

        manager.submit_order(order1).await.unwrap();

        // Second order with same ID should fail
        let order2 = Order::market("ETHUSDT", OrderSide::Sell, dec!(2.0))
            .with_client_order_id(client_order_id)
            .build()
            .unwrap();

        let result = manager.submit_order(order2).await;
        assert!(matches!(result, Err(OrderManagerError::DuplicateOrderId(_))));
    }

    #[tokio::test]
    async fn test_order_not_found_errors() {
        let manager = OrderManager::with_defaults();
        let fake_id = ClientOrderId::new("nonexistent");
        let account_id = AccountId::new("test");

        // All operations should return OrderNotFound
        assert!(matches!(
            manager.mark_submitted(&fake_id, account_id.clone()).await,
            Err(OrderManagerError::OrderNotFound(_))
        ));

        assert!(matches!(
            manager.mark_accepted(&fake_id, VenueOrderId::new("v"), account_id.clone()).await,
            Err(OrderManagerError::OrderNotFound(_))
        ));

        assert!(matches!(
            manager.mark_rejected(&fake_id, account_id.clone(), "reason").await,
            Err(OrderManagerError::OrderNotFound(_))
        ));

        assert!(matches!(
            manager.cancel_order(&fake_id, account_id).await,
            Err(OrderManagerError::OrderNotFound(_))
        ));
    }

    #[tokio::test]
    async fn test_get_orders_by_status() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("status-test");

        // Create orders in different states
        let init_order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        manager.submit_order(init_order).await.unwrap();

        let submitted_order = Order::market("ETHUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let submitted_id = manager.submit_order(submitted_order).await.unwrap();
        manager.mark_submitted(&submitted_id, account_id.clone()).await.unwrap();

        let accepted_order = Order::limit("LTCUSDT", OrderSide::Buy, dec!(1.0), dec!(100))
            .build()
            .unwrap();
        let accepted_id = manager.submit_order(accepted_order).await.unwrap();
        manager.mark_submitted(&accepted_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&accepted_id, VenueOrderId::new("v1"), account_id).await.unwrap();

        let initialized = manager.get_orders_by_status(OrderStatus::Initialized).await;
        assert_eq!(initialized.len(), 1);

        let submitted = manager.get_orders_by_status(OrderStatus::Submitted).await;
        assert_eq!(submitted.len(), 1);

        let accepted = manager.get_orders_by_status(OrderStatus::Accepted).await;
        assert_eq!(accepted.len(), 1);
    }

    #[tokio::test]
    async fn test_open_vs_closed_orders() {
        let manager = OrderManager::with_defaults();
        let account_id = AccountId::new("open-close-test");

        // Create an open order
        let open_order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .build()
            .unwrap();
        let open_id = manager.submit_order(open_order).await.unwrap();
        manager.mark_submitted(&open_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&open_id, VenueOrderId::new("v1"), account_id.clone()).await.unwrap();

        // Create a closed (filled) order
        let filled_order = Order::market("ETHUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let filled_id = manager.submit_order(filled_order).await.unwrap();
        manager.mark_submitted(&filled_id, account_id.clone()).await.unwrap();
        manager.mark_accepted(&filled_id, VenueOrderId::new("v2"), account_id.clone()).await.unwrap();
        manager.apply_fill(&filled_id, VenueOrderId::new("v2"), account_id,
            TradeId::generate(), dec!(1.0), dec!(3000), dec!(0.1), "USDT".to_string(),
            LiquiditySide::Taker).await.unwrap();

        let open_orders = manager.get_open_orders().await;
        assert_eq!(open_orders.len(), 1);
        assert_eq!(open_orders[0].symbol(), "BTCUSDT");

        let closed_orders = manager.get_closed_orders().await;
        assert_eq!(closed_orders.len(), 1);
        assert_eq!(closed_orders[0].symbol(), "ETHUSDT");

        let open_symbol_orders = manager.get_open_orders_for_symbol("BTCUSDT").await;
        assert_eq!(open_symbol_orders.len(), 1);

        let open_eth_orders = manager.get_open_orders_for_symbol("ETHUSDT").await;
        assert!(open_eth_orders.is_empty());
    }

    // === Concurrency Tests ===

    #[tokio::test]
    async fn test_concurrent_order_submission() {
        let manager = Arc::new(OrderManager::with_defaults());
        let mut handles = Vec::new();

        // Submit 100 orders concurrently
        for i in 0..100 {
            let manager_clone = manager.clone();
            let symbol = if i % 3 == 0 {
                "BTCUSDT"
            } else if i % 3 == 1 {
                "ETHUSDT"
            } else {
                "LTCUSDT"
            };

            handles.push(tokio::spawn(async move {
                let order = Order::market(symbol, OrderSide::Buy, dec!(0.1))
                    .build()
                    .unwrap();
                manager_clone.submit_order(order).await
            }));
        }

        // Wait for all submissions
        let results: Vec<_> = tokio_join_all(handles).await;

        // All should succeed
        let successful = results.iter().filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok()).count();
        assert_eq!(successful, 100);

        // Verify order counts
        assert_eq!(manager.get_all_orders().await.len(), 100);
        assert_eq!(manager.open_order_count().await, 100);
    }

    #[tokio::test]
    async fn test_concurrent_order_lifecycle() {
        let manager = Arc::new(OrderManager::with_defaults());
        let account_id = AccountId::new("concurrent-test");
        let mut order_ids = Vec::new();

        // First, submit a batch of orders sequentially to get their IDs
        for i in 0..50 {
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            order_ids.push((i, client_id));
        }

        // Now process them concurrently through their lifecycle
        let mut handles = Vec::new();
        for (i, client_id) in order_ids {
            let manager_clone = manager.clone();
            let account_id_clone = account_id.clone();
            let client_id_clone = client_id.clone();

            handles.push(tokio::spawn(async move {
                // Submit
                manager_clone
                    .mark_submitted(&client_id_clone, account_id_clone.clone())
                    .await?;

                // Accept with unique venue ID
                manager_clone
                    .mark_accepted(
                        &client_id_clone,
                        VenueOrderId::new(format!("venue-{}", i)),
                        account_id_clone.clone(),
                    )
                    .await?;

                // Fill
                manager_clone
                    .apply_fill(
                        &client_id_clone,
                        VenueOrderId::new(format!("venue-{}", i)),
                        account_id_clone,
                        TradeId::generate(),
                        dec!(1.0),
                        dec!(50000),
                        dec!(0.1),
                        "USDT".to_string(),
                        LiquiditySide::Taker,
                    )
                    .await?;

                Ok::<_, OrderManagerError>(())
            }));
        }

        // Wait for all
        let results: Vec<_> = tokio_join_all(handles).await;

        // All should succeed
        for result in results {
            assert!(result.is_ok(), "Task panicked: {:?}", result);
            assert!(result.unwrap().is_ok(), "Order lifecycle failed");
        }

        // Verify all orders are filled
        assert_eq!(manager.open_order_count().await, 0);
        let stats = manager.get_stats().await;
        assert_eq!(stats.filled_orders, 50);
        assert_eq!(stats.total_filled_qty, dec!(50.0)); // 50 orders * 1.0 qty
    }

    #[tokio::test]
    async fn test_concurrent_cancel_all() {
        let manager = Arc::new(OrderManager::with_defaults());
        let account_id = AccountId::new("cancel-test");

        // Submit and accept orders
        for i in 0..20 {
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            manager
                .mark_submitted(&client_id, account_id.clone())
                .await
                .unwrap();
            manager
                .mark_accepted(
                    &client_id,
                    VenueOrderId::new(format!("v-{}", i)),
                    account_id.clone(),
                )
                .await
                .unwrap();
        }

        assert_eq!(manager.open_order_count().await, 20);

        // Concurrently try to cancel all from multiple tasks
        let mut handles = Vec::new();
        for _ in 0..5 {
            let manager_clone = manager.clone();
            let account_id_clone = account_id.clone();
            handles.push(tokio::spawn(async move {
                manager_clone.cancel_all_orders(account_id_clone).await
            }));
        }

        let results: Vec<_> = tokio_join_all(handles).await;

        // At least one should have canceled some orders
        let total_canceled: usize = results
            .iter()
            .filter_map(|r| r.as_ref().ok())
            .filter_map(|r| r.as_ref().ok())
            .map(|ids| ids.len())
            .sum();

        // Due to concurrent cancellation, total should be 20 (each order canceled once)
        assert_eq!(total_canceled, 20);

        // All orders should be canceled
        assert_eq!(manager.open_order_count().await, 0);
    }

    #[tokio::test]
    async fn test_concurrent_mixed_operations() {
        let manager = Arc::new(OrderManager::with_defaults());
        let account_id = AccountId::new("mixed-test");
        let mut handles = Vec::new();

        // Task 1: Submit orders
        {
            let manager_clone = manager.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..30 {
                    let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1))
                        .build()
                        .unwrap();
                    let _ = manager_clone.submit_order(order).await;
                }
            }));
        }

        // Task 2: Query orders repeatedly
        {
            let manager_clone = manager.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..50 {
                    let _ = manager_clone.get_all_orders().await;
                    let _ = manager_clone.get_open_orders().await;
                    let _ = manager_clone.get_stats().await;
                    tokio::task::yield_now().await;
                }
            }));
        }

        // Task 3: Submit different orders
        {
            let manager_clone = manager.clone();
            let account_id_clone = account_id.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..20 {
                    let order = Order::limit("ETHUSDT", OrderSide::Sell, dec!(1.0), dec!(3000))
                        .build()
                        .unwrap();
                    if let Ok(client_id) = manager_clone.submit_order(order).await {
                        let _ = manager_clone
                            .mark_submitted(&client_id, account_id_clone.clone())
                            .await;
                        let _ = manager_clone
                            .mark_accepted(
                                &client_id,
                                VenueOrderId::new(format!("eth-{}", i)),
                                account_id_clone.clone(),
                            )
                            .await;
                    }
                }
            }));
        }

        // Wait for all tasks
        let results: Vec<_> = tokio_join_all(handles).await;

        // No panics should occur
        for result in results {
            assert!(result.is_ok(), "Task panicked: {:?}", result);
        }

        // Manager should still be in consistent state
        let all_orders = manager.get_all_orders().await;
        let stats = manager.get_stats().await;
        assert_eq!(all_orders.len(), stats.total_orders);
    }

    #[tokio::test]
    async fn test_concurrent_event_callbacks() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let manager = Arc::new(OrderManager::with_defaults());
        let event_count = Arc::new(AtomicUsize::new(0));
        let event_count_clone = event_count.clone();

        // Register callback
        manager
            .on_event(Box::new(move |_event| {
                event_count_clone.fetch_add(1, Ordering::SeqCst);
            }))
            .await;

        let account_id = AccountId::new("callback-test");

        // Submit orders concurrently
        let mut handles = Vec::new();
        for i in 0..10 {
            let manager_clone = manager.clone();
            let account_id_clone = account_id.clone();
            handles.push(tokio::spawn(async move {
                let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1))
                    .build()
                    .unwrap();
                let client_id = manager_clone.submit_order(order).await.unwrap();
                manager_clone
                    .mark_submitted(&client_id, account_id_clone.clone())
                    .await
                    .unwrap();
                manager_clone
                    .mark_accepted(
                        &client_id,
                        VenueOrderId::new(format!("v-{}", i)),
                        account_id_clone.clone(),
                    )
                    .await
                    .unwrap();
                manager_clone
                    .apply_fill(
                        &client_id,
                        VenueOrderId::new(format!("v-{}", i)),
                        account_id_clone,
                        TradeId::generate(),
                        dec!(0.1),
                        dec!(50000),
                        dec!(0.01),
                        "USDT".to_string(),
                        LiquiditySide::Taker,
                    )
                    .await
                    .unwrap();
            }));
        }

        tokio_join_all(handles).await;

        // Each order should generate 4 events: Init, Submit, Accept, Fill
        // 10 orders * 4 events = 40 events
        assert_eq!(event_count.load(Ordering::SeqCst), 40);
    }

    #[tokio::test]
    async fn test_concurrent_stats_consistency() {
        let manager = Arc::new(OrderManager::with_defaults());
        let account_id = AccountId::new("stats-test");

        // Pre-populate with some orders
        for i in 0..10 {
            let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            manager
                .mark_submitted(&client_id, account_id.clone())
                .await
                .unwrap();
            manager
                .mark_accepted(
                    &client_id,
                    VenueOrderId::new(format!("v-{}", i)),
                    account_id.clone(),
                )
                .await
                .unwrap();
        }

        // Concurrently: fill some, cancel some, add more
        let mut handles = Vec::new();

        // Fill orders 0-4
        for i in 0..5 {
            let manager_clone = manager.clone();
            let account_id_clone = account_id.clone();
            handles.push(tokio::spawn(async move {
                let orders = manager_clone.get_open_orders().await;
                if let Some(order) = orders.get(0) {
                    let _ = manager_clone
                        .apply_fill(
                            &order.client_order_id,
                            VenueOrderId::new(format!("v-{}", i)),
                            account_id_clone,
                            TradeId::generate(),
                            dec!(1.0),
                            dec!(50000),
                            dec!(0.1),
                            "USDT".to_string(),
                            LiquiditySide::Taker,
                        )
                        .await;
                }
            }));
        }

        // Cancel remaining
        for _ in 0..5 {
            let manager_clone = manager.clone();
            let account_id_clone = account_id.clone();
            handles.push(tokio::spawn(async move {
                let orders = manager_clone.get_open_orders().await;
                if let Some(order) = orders.get(0) {
                    let _ = manager_clone
                        .cancel_order(&order.client_order_id, account_id_clone)
                        .await;
                }
            }));
        }

        tokio_join_all(handles).await;

        // Verify stats are consistent
        let stats = manager.get_stats().await;
        let all_orders = manager.get_all_orders().await;
        let open_orders = manager.get_open_orders().await;
        let closed_orders = manager.get_closed_orders().await;

        // Total should match
        assert_eq!(stats.total_orders, all_orders.len());
        // Open + closed should equal total
        assert_eq!(open_orders.len() + closed_orders.len(), all_orders.len());
        // Filled + canceled should equal closed
        assert_eq!(
            stats.filled_orders + stats.canceled_orders,
            closed_orders.len()
        );
    }

    #[tokio::test]
    async fn test_concurrent_reset() {
        let manager = Arc::new(OrderManager::with_defaults());
        let account_id = AccountId::new("reset-test");

        // Submit orders
        for i in 0..50 {
            let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
                .build()
                .unwrap();
            let client_id = manager.submit_order(order).await.unwrap();
            manager
                .mark_submitted(&client_id, account_id.clone())
                .await
                .unwrap();
            manager
                .mark_accepted(
                    &client_id,
                    VenueOrderId::new(format!("v-{}", i)),
                    account_id.clone(),
                )
                .await
                .unwrap();
        }

        assert_eq!(manager.get_all_orders().await.len(), 50);

        // Concurrently: reset + query + submit
        let mut handles = Vec::new();

        // Reset
        {
            let manager_clone = manager.clone();
            handles.push(tokio::spawn(async move {
                manager_clone.reset().await;
            }));
        }

        // Query (should not panic)
        {
            let manager_clone = manager.clone();
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    let _ = manager_clone.get_all_orders().await;
                    let _ = manager_clone.get_stats().await;
                    tokio::task::yield_now().await;
                }
            }));
        }

        tokio_join_all(handles).await;

        // After reset, manager should be empty or have only newly added orders
        // The exact state depends on race condition timing, but should be consistent
        let final_orders = manager.get_all_orders().await;
        let final_stats = manager.get_stats().await;
        assert_eq!(final_orders.len(), final_stats.total_orders);
    }

    // === Session Validation Tests ===

    #[test]
    fn test_session_enforcement_default() {
        let config = OrderManagerConfig::default();
        assert_eq!(config.session_enforcement, SessionEnforcement::Disabled);
    }

    #[tokio::test]
    async fn test_session_enforcement_disabled_allows_orders() {
        // With session enforcement disabled, orders should be accepted even without session manager
        let config = OrderManagerConfig {
            session_enforcement: SessionEnforcement::Disabled,
            ..Default::default()
        };
        let manager = OrderManager::new(config);

        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        // Should succeed - no session validation
        let result = manager.submit_order(order).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_session_enforcement_without_session_manager() {
        // Even with strict enforcement, orders should be accepted if no session manager is set
        // (fail-open behavior)
        let config = OrderManagerConfig {
            session_enforcement: SessionEnforcement::Strict,
            ..Default::default()
        };
        let manager = OrderManager::new(config);

        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();

        // Should succeed - no session manager configured
        let result = manager.submit_order(order).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_session_enforcement_is_enabled() {
        assert!(!SessionEnforcement::Disabled.is_enabled());
        assert!(SessionEnforcement::Warn.is_enabled());
        assert!(SessionEnforcement::Strict.is_enabled());
    }

    #[test]
    fn test_session_enforcement_should_reject() {
        assert!(!SessionEnforcement::Disabled.should_reject());
        assert!(!SessionEnforcement::Warn.should_reject());
        assert!(SessionEnforcement::Strict.should_reject());
    }

    #[test]
    fn test_order_manager_error_display() {
        let closed_err = OrderManagerError::MarketClosed {
            symbol: "BTCUSDT.BINANCE".to_string(),
            reason: "Market is closed".to_string(),
        };
        assert!(closed_err.to_string().contains("Market closed"));
        assert!(closed_err.to_string().contains("BTCUSDT.BINANCE"));

        let halted_err = OrderManagerError::MarketHalted {
            symbol: "AAPL.XNAS".to_string(),
            reason: "Circuit breaker triggered".to_string(),
        };
        assert!(halted_err.to_string().contains("Market halted"));
        assert!(halted_err.to_string().contains("AAPL.XNAS"));

        let not_found_err = OrderManagerError::SessionNotFound(InstrumentId::new("BTCUSDT", "BINANCE"));
        assert!(not_found_err.to_string().contains("Session not found"));
    }
}
