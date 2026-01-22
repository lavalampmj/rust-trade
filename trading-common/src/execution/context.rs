//! Strategy execution context providing order management capabilities.
//!
//! The `StrategyContext` bridges the Strategy trait with the order management system,
//! allowing strategies to submit, cancel, and query orders while maintaining
//! backward compatibility with the simple Signal-based approach.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;

use crate::orders::{
    AccountId, ClientOrderId, Order, OrderError, OrderEventAny, OrderManager, OrderManagerConfig,
    OrderSide, StrategyId, TimeInForce,
};

/// Configuration for the strategy execution context.
#[derive(Debug, Clone)]
pub struct StrategyContextConfig {
    /// Strategy identifier
    pub strategy_id: StrategyId,
    /// Account identifier
    pub account_id: AccountId,
    /// Default venue for orders
    pub default_venue: String,
    /// Default time-in-force for orders
    pub default_time_in_force: TimeInForce,
    /// Whether to allow multiple orders per symbol
    pub allow_multiple_orders_per_symbol: bool,
    /// Maximum orders allowed
    pub max_orders: usize,
}

impl Default for StrategyContextConfig {
    fn default() -> Self {
        Self {
            strategy_id: StrategyId::default(),
            account_id: AccountId::default(),
            default_venue: "DEFAULT".to_string(),
            default_time_in_force: TimeInForce::GTC,
            allow_multiple_orders_per_symbol: true,
            max_orders: 1000,
        }
    }
}

/// Execution context providing order management capabilities to strategies.
///
/// This context is passed to strategies during execution, allowing them to:
/// - Submit market, limit, stop, and other order types
/// - Cancel or modify existing orders
/// - Query order state and fills
/// - Access position information
///
/// # Example
/// ```ignore
/// impl Strategy for MyStrategy {
///     fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
///         // Use context for advanced order management
///         if let Some(ctx) = self.context.as_mut() {
///             // Submit a limit order
///             let order_id = ctx.limit_order(
///                 "BTCUSDT",
///                 OrderSide::Buy,
///                 dec!(0.1),
///                 dec!(49500),
///             );
///
///             // Check open orders
///             let open_orders = ctx.get_open_orders_for_symbol("BTCUSDT");
///         }
///         Signal::Hold
///     }
/// }
/// ```
pub struct StrategyContext {
    /// Configuration
    config: StrategyContextConfig,
    /// Order manager for tracking orders
    order_manager: Arc<OrderManager>,
    /// Current timestamp (set by execution engine)
    current_time: DateTime<Utc>,
    /// Current prices by symbol
    current_prices: HashMap<String, Decimal>,
    /// Position quantities by symbol (positive = long, negative = short)
    positions: HashMap<String, Decimal>,
    /// Cash balance
    cash: Decimal,
    /// Initial capital
    initial_capital: Decimal,
    /// Commission rate
    commission_rate: Decimal,
    /// Pending order events to be processed
    pending_events: Vec<OrderEventAny>,
}

impl StrategyContext {
    /// Create a new strategy context.
    pub fn new(config: StrategyContextConfig, initial_capital: Decimal) -> Self {
        let order_manager_config = OrderManagerConfig {
            max_open_orders: config.max_orders,
            max_orders_per_symbol: if config.allow_multiple_orders_per_symbol {
                100
            } else {
                1
            },
            allow_duplicate_ids: false,
            default_account_id: config.account_id.clone(),
            default_strategy_id: config.strategy_id.clone(),
            session_enforcement: Default::default(),
        };

        Self {
            config,
            order_manager: Arc::new(OrderManager::new(order_manager_config)),
            current_time: Utc::now(),
            current_prices: HashMap::new(),
            positions: HashMap::new(),
            cash: initial_capital,
            initial_capital,
            commission_rate: Decimal::new(1, 3), // 0.001 = 0.1%
            pending_events: Vec::new(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(initial_capital: Decimal) -> Self {
        Self::new(StrategyContextConfig::default(), initial_capital)
    }

    // ========================================================================
    // Order Submission Methods
    // ========================================================================

    /// Submit a market order.
    ///
    /// Returns the client order ID on success.
    pub fn market_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
    ) -> Result<ClientOrderId, ContextError> {
        let order = Order::market(symbol, side, quantity)
            .with_venue(&self.config.default_venue)
            .with_strategy_id(self.config.strategy_id.clone())
            .with_account_id(self.config.account_id.clone())
            .build()
            .map_err(|e| ContextError::OrderError(e))?;

        self.submit_order(order)
    }

    /// Submit a limit order.
    ///
    /// Returns the client order ID on success.
    pub fn limit_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
        price: Decimal,
    ) -> Result<ClientOrderId, ContextError> {
        let order = Order::limit(symbol, side, quantity, price)
            .with_venue(&self.config.default_venue)
            .with_strategy_id(self.config.strategy_id.clone())
            .with_account_id(self.config.account_id.clone())
            .with_time_in_force(self.config.default_time_in_force)
            .build()
            .map_err(|e| ContextError::OrderError(e))?;

        self.submit_order(order)
    }

    /// Submit a stop order.
    ///
    /// Returns the client order ID on success.
    pub fn stop_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
        trigger_price: Decimal,
    ) -> Result<ClientOrderId, ContextError> {
        let order = Order::stop(symbol, side, quantity, trigger_price)
            .with_venue(&self.config.default_venue)
            .with_strategy_id(self.config.strategy_id.clone())
            .with_account_id(self.config.account_id.clone())
            .with_time_in_force(self.config.default_time_in_force)
            .build()
            .map_err(|e| ContextError::OrderError(e))?;

        self.submit_order(order)
    }

    /// Submit a stop-limit order.
    ///
    /// Returns the client order ID on success.
    pub fn stop_limit_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
        price: Decimal,
        trigger_price: Decimal,
    ) -> Result<ClientOrderId, ContextError> {
        let order = Order::stop_limit(symbol, side, quantity, price, trigger_price)
            .with_venue(&self.config.default_venue)
            .with_strategy_id(self.config.strategy_id.clone())
            .with_account_id(self.config.account_id.clone())
            .with_time_in_force(self.config.default_time_in_force)
            .build()
            .map_err(|e| ContextError::OrderError(e))?;

        self.submit_order(order)
    }

    /// Submit a custom order (already built).
    ///
    /// Returns the client order ID on success.
    pub fn submit_order(&mut self, order: Order) -> Result<ClientOrderId, ContextError> {
        // Validate we have sufficient buying power for buy orders
        if order.side == OrderSide::Buy {
            let estimated_cost = order.quantity
                * order.price.unwrap_or_else(|| {
                    self.current_prices
                        .get(order.symbol())
                        .copied()
                        .unwrap_or(Decimal::ZERO)
                });

            if estimated_cost > self.cash {
                return Err(ContextError::InsufficientFunds {
                    required: estimated_cost,
                    available: self.cash,
                });
            }
        }

        // Validate we have sufficient position for sell orders
        if order.side == OrderSide::Sell {
            let position = self.positions.get(order.symbol()).copied().unwrap_or(Decimal::ZERO);
            if order.quantity > position {
                return Err(ContextError::InsufficientPosition {
                    required: order.quantity,
                    available: position,
                });
            }
        }

        let client_order_id = order.client_order_id.clone();

        // Submit to order manager (synchronous for backtesting)
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ContextError::RuntimeError(e.to_string()))?;

        runtime.block_on(async {
            self.order_manager.submit_order(order).await
        }).map_err(|e| ContextError::OrderManagerError(e.to_string()))?;

        Ok(client_order_id)
    }

    // ========================================================================
    // Order Management Methods
    // ========================================================================

    /// Cancel an order by client order ID.
    pub fn cancel_order(&mut self, client_order_id: &ClientOrderId) -> Result<(), ContextError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ContextError::RuntimeError(e.to_string()))?;

        runtime.block_on(async {
            self.order_manager
                .cancel_order(client_order_id, self.config.account_id.clone())
                .await
        }).map_err(|e| ContextError::OrderManagerError(e.to_string()))
    }

    /// Cancel all open orders.
    pub fn cancel_all_orders(&mut self) -> Result<Vec<ClientOrderId>, ContextError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ContextError::RuntimeError(e.to_string()))?;

        runtime.block_on(async {
            self.order_manager
                .cancel_all_orders(self.config.account_id.clone())
                .await
        }).map_err(|e| ContextError::OrderManagerError(e.to_string()))
    }

    /// Cancel all open orders for a specific symbol.
    pub fn cancel_orders_for_symbol(
        &mut self,
        symbol: &str,
    ) -> Result<Vec<ClientOrderId>, ContextError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ContextError::RuntimeError(e.to_string()))?;

        runtime.block_on(async {
            self.order_manager
                .cancel_orders_for_symbol(symbol, self.config.account_id.clone())
                .await
        }).map_err(|e| ContextError::OrderManagerError(e.to_string()))
    }

    // ========================================================================
    // Order Query Methods
    // ========================================================================

    /// Get an order by client order ID.
    pub fn get_order(&self, client_order_id: &ClientOrderId) -> Option<Order> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;

        runtime.block_on(async { self.order_manager.get_order(client_order_id).await })
    }

    /// Get all open orders.
    pub fn get_open_orders(&self) -> Vec<Order> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|_| panic!("Failed to create runtime"));

        runtime.block_on(async { self.order_manager.get_open_orders().await })
    }

    /// Get open orders for a specific symbol.
    pub fn get_open_orders_for_symbol(&self, symbol: &str) -> Vec<Order> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|_| panic!("Failed to create runtime"));

        runtime.block_on(async {
            self.order_manager.get_open_orders_for_symbol(symbol).await
        })
    }

    /// Check if there are any open orders for a symbol.
    pub fn has_open_orders_for_symbol(&self, symbol: &str) -> bool {
        !self.get_open_orders_for_symbol(symbol).is_empty()
    }

    /// Get the count of open orders.
    pub fn open_order_count(&self) -> usize {
        self.get_open_orders().len()
    }

    // ========================================================================
    // Position & Portfolio Methods
    // ========================================================================

    /// Get current position for a symbol.
    ///
    /// Returns 0 if no position exists.
    pub fn get_position(&self, symbol: &str) -> Decimal {
        self.positions.get(symbol).copied().unwrap_or(Decimal::ZERO)
    }

    /// Check if we have a position in a symbol.
    pub fn has_position(&self, symbol: &str) -> bool {
        self.positions
            .get(symbol)
            .map(|p| !p.is_zero())
            .unwrap_or(false)
    }

    /// Check if we have a long position in a symbol.
    pub fn is_long(&self, symbol: &str) -> bool {
        self.positions
            .get(symbol)
            .map(|p| p.is_sign_positive() && !p.is_zero())
            .unwrap_or(false)
    }

    /// Check if we have a short position in a symbol.
    pub fn is_short(&self, symbol: &str) -> bool {
        self.positions
            .get(symbol)
            .map(|p| p.is_sign_negative())
            .unwrap_or(false)
    }

    /// Get all positions.
    pub fn get_all_positions(&self) -> &HashMap<String, Decimal> {
        &self.positions
    }

    /// Get current cash balance.
    pub fn cash(&self) -> Decimal {
        self.cash
    }

    /// Get current portfolio value (cash + positions).
    pub fn portfolio_value(&self) -> Decimal {
        let position_value: Decimal = self
            .positions
            .iter()
            .map(|(symbol, qty)| {
                let price = self.current_prices.get(symbol).copied().unwrap_or(Decimal::ZERO);
                *qty * price
            })
            .sum();

        self.cash + position_value
    }

    /// Get current price for a symbol.
    pub fn get_price(&self, symbol: &str) -> Option<Decimal> {
        self.current_prices.get(symbol).copied()
    }

    /// Get the current timestamp.
    pub fn current_time(&self) -> DateTime<Utc> {
        self.current_time
    }

    // ========================================================================
    // Internal Methods (called by ExecutionEngine)
    // ========================================================================

    /// Update the current time (called by execution engine).
    pub(crate) fn set_current_time(&mut self, time: DateTime<Utc>) {
        self.current_time = time;
    }

    /// Update current price for a symbol (called by execution engine).
    pub(crate) fn update_price(&mut self, symbol: &str, price: Decimal) {
        self.current_prices.insert(symbol.to_string(), price);
    }

    /// Update position for a symbol (called by execution engine).
    pub(crate) fn update_position(&mut self, symbol: &str, quantity: Decimal) {
        if quantity.is_zero() {
            self.positions.remove(symbol);
        } else {
            self.positions.insert(symbol.to_string(), quantity);
        }
    }

    /// Update cash balance (called by execution engine).
    pub(crate) fn update_cash(&mut self, amount: Decimal) {
        self.cash = amount;
    }

    /// Get the order manager (for execution engine).
    pub(crate) fn order_manager(&self) -> &Arc<OrderManager> {
        &self.order_manager
    }

    /// Get commission rate.
    #[allow(dead_code)]
    pub(crate) fn commission_rate(&self) -> Decimal {
        self.commission_rate
    }

    /// Set commission rate.
    pub fn set_commission_rate(&mut self, rate: Decimal) {
        self.commission_rate = rate;
    }

    /// Get account ID.
    pub fn account_id(&self) -> &AccountId {
        &self.config.account_id
    }

    /// Get strategy ID.
    pub fn strategy_id(&self) -> &StrategyId {
        &self.config.strategy_id
    }

    /// Add a pending event (for future event sourcing).
    #[allow(dead_code)]
    pub(crate) fn add_pending_event(&mut self, event: OrderEventAny) {
        self.pending_events.push(event);
    }

    /// Take all pending events (for future event sourcing).
    #[allow(dead_code)]
    pub(crate) fn take_pending_events(&mut self) -> Vec<OrderEventAny> {
        std::mem::take(&mut self.pending_events)
    }

    /// Reset the context for a new run.
    pub fn reset(&mut self) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            self.order_manager.reset().await;
        });

        self.current_prices.clear();
        self.positions.clear();
        self.cash = self.initial_capital;
        self.pending_events.clear();
    }
}

/// Errors that can occur in the strategy context.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ContextError {
    #[error("Order error: {0}")]
    OrderError(#[from] OrderError),

    #[error("Order manager error: {0}")]
    OrderManagerError(String),

    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: Decimal, available: Decimal },

    #[error("Insufficient position: required {required}, available {available}")]
    InsufficientPosition { required: Decimal, available: Decimal },

    #[error("Runtime error: {0}")]
    RuntimeError(String),

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::OrderType;
    use rust_decimal_macros::dec;

    #[test]
    fn test_context_creation() {
        let ctx = StrategyContext::with_defaults(dec!(10000));
        assert_eq!(ctx.cash(), dec!(10000));
        assert_eq!(ctx.portfolio_value(), dec!(10000));
    }

    #[test]
    fn test_market_order_submission() {
        let mut ctx = StrategyContext::with_defaults(dec!(10000));
        ctx.update_price("BTCUSDT", dec!(50000));

        let order_id = ctx
            .market_order("BTCUSDT", OrderSide::Buy, dec!(0.1))
            .unwrap();

        let order = ctx.get_order(&order_id).unwrap();
        assert_eq!(order.order_type, OrderType::Market);
        assert_eq!(order.quantity, dec!(0.1));
    }

    #[test]
    fn test_limit_order_submission() {
        let mut ctx = StrategyContext::with_defaults(dec!(10000));

        let order_id = ctx
            .limit_order("BTCUSDT", OrderSide::Buy, dec!(0.1), dec!(49000))
            .unwrap();

        let order = ctx.get_order(&order_id).unwrap();
        assert_eq!(order.order_type, OrderType::Limit);
        assert_eq!(order.price, Some(dec!(49000)));
    }

    #[test]
    fn test_insufficient_funds() {
        let mut ctx = StrategyContext::with_defaults(dec!(1000));
        ctx.update_price("BTCUSDT", dec!(50000));

        let result = ctx.market_order("BTCUSDT", OrderSide::Buy, dec!(1.0));
        assert!(matches!(result, Err(ContextError::InsufficientFunds { .. })));
    }

    #[test]
    fn test_insufficient_position() {
        let mut ctx = StrategyContext::with_defaults(dec!(10000));

        // Try to sell without a position
        let result = ctx.market_order("BTCUSDT", OrderSide::Sell, dec!(1.0));
        assert!(matches!(
            result,
            Err(ContextError::InsufficientPosition { .. })
        ));
    }

    #[test]
    fn test_position_tracking() {
        let mut ctx = StrategyContext::with_defaults(dec!(10000));

        assert!(!ctx.has_position("BTCUSDT"));
        assert_eq!(ctx.get_position("BTCUSDT"), Decimal::ZERO);

        ctx.update_position("BTCUSDT", dec!(1.5));
        assert!(ctx.has_position("BTCUSDT"));
        assert!(ctx.is_long("BTCUSDT"));
        assert_eq!(ctx.get_position("BTCUSDT"), dec!(1.5));

        ctx.update_position("BTCUSDT", dec!(-0.5));
        assert!(ctx.is_short("BTCUSDT"));
    }

    #[test]
    fn test_portfolio_value() {
        let mut ctx = StrategyContext::with_defaults(dec!(10000));

        ctx.update_price("BTCUSDT", dec!(50000));
        ctx.update_position("BTCUSDT", dec!(0.1));

        // Portfolio = 10000 cash + 0.1 * 50000 = 10000 + 5000 = 15000
        assert_eq!(ctx.portfolio_value(), dec!(15000));
    }

    #[test]
    fn test_context_reset() {
        let mut ctx = StrategyContext::with_defaults(dec!(10000));

        ctx.update_price("BTCUSDT", dec!(50000));
        ctx.update_position("BTCUSDT", dec!(1.0));
        ctx.update_cash(dec!(5000));

        ctx.reset();

        assert_eq!(ctx.cash(), dec!(10000));
        assert!(!ctx.has_position("BTCUSDT"));
        assert!(ctx.current_prices.is_empty());
    }
}
