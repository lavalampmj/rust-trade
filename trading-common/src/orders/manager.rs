//! Order management and tracking.
//!
//! The `OrderManager` maintains the state of all orders and provides
//! methods for querying, submitting, canceling, and tracking orders.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rust_decimal::Decimal;

use super::events::{
    OrderAccepted, OrderCanceled, OrderDenied, OrderEventAny, OrderExpired, OrderFilled,
    OrderInitialized, OrderRejected, OrderSubmitted, OrderTriggered,
};
use super::order::{Order, OrderError};
use super::types::{
    AccountId, ClientOrderId, InstrumentId, LiquiditySide, OrderSide, OrderStatus, StrategyId,
    TradeId, VenueOrderId,
};

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
}

impl Default for OrderManagerConfig {
    fn default() -> Self {
        Self {
            max_open_orders: 1000,
            max_orders_per_symbol: 100,
            allow_duplicate_ids: false,
            default_account_id: AccountId::default(),
            default_strategy_id: StrategyId::default(),
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
        }
    }

    /// Create a new OrderManager with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(OrderManagerConfig::default())
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
}
