//! Execution engine for processing orders and managing fills.
//!
//! The `ExecutionEngine` is responsible for:
//! - Processing pending orders each tick
//! - Simulating fills using configurable fill models
//! - Managing position updates
//! - Converting signals to orders (backward compatibility)
//! - Tracking execution metrics

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;

use crate::data::types::TickData;
use crate::orders::{ClientOrderId, Order, OrderFilled, OrderSide, OrderStatus, VenueOrderId};

use super::context::{ContextError, StrategyContext};
use super::fill_model::{default_fill_model, FillModel, FillResult, MarketSnapshot};

/// Configuration for the execution engine.
#[derive(Debug, Clone)]
pub struct ExecutionEngineConfig {
    /// Initial capital
    pub initial_capital: Decimal,
    /// Commission rate (e.g., 0.001 = 0.1%)
    pub commission_rate: Decimal,
    /// Default position sizing for orders (as fraction of portfolio)
    pub default_position_size: Decimal,
    /// Whether to use all available cash for position sizing
    pub use_all_available_cash: bool,
}

impl Default for ExecutionEngineConfig {
    fn default() -> Self {
        Self {
            initial_capital: Decimal::new(10000, 0),
            commission_rate: Decimal::new(1, 3),       // 0.1%
            default_position_size: Decimal::new(1, 1), // 10% of portfolio
            use_all_available_cash: false,
        }
    }
}

/// Execution metrics tracking fill statistics.
#[derive(Debug, Clone, Default)]
pub struct ExecutionMetrics {
    /// Total number of orders processed
    pub total_orders: u64,
    /// Number of filled orders
    pub filled_orders: u64,
    /// Number of canceled orders
    pub canceled_orders: u64,
    /// Number of rejected orders
    pub rejected_orders: u64,
    /// Total commission paid
    pub total_commission: Decimal,
    /// Total slippage incurred
    pub total_slippage: Decimal,
    /// Average fill price deviation from target
    pub avg_fill_deviation: Decimal,
    /// Total notional traded
    pub total_notional: Decimal,
}

impl ExecutionMetrics {
    /// Record a fill.
    pub fn record_fill(&mut self, fill: &FillResult) {
        self.filled_orders += 1;
        self.total_commission += fill.commission;
        if let Some(slippage) = fill.slippage {
            self.total_slippage += slippage.abs();
        }
        if let Some(price) = fill.fill_price {
            self.total_notional += fill.fill_qty * price;
        }
    }
}

/// Execution engine for processing orders during backtesting.
///
/// The engine maintains:
/// - Strategy context with positions and orders
/// - Fill model for simulating execution
/// - Execution metrics
///
/// # Example
/// ```ignore
/// let config = ExecutionEngineConfig::default();
/// let mut engine = ExecutionEngine::new(config);
///
/// // Process tick data
/// engine.on_tick(&tick);
///
/// // Convert signal to order
/// if let Signal::Buy { symbol, quantity } = signal {
///     engine.execute_signal(&signal, current_price);
/// }
///
/// // Check fills
/// let fills = engine.process_pending_orders();
/// ```
pub struct ExecutionEngine {
    /// Configuration
    config: ExecutionEngineConfig,
    /// Strategy context with positions and orders
    context: StrategyContext,
    /// Fill model for simulating execution
    fill_model: Box<dyn FillModel>,
    /// Execution metrics
    metrics: ExecutionMetrics,
    /// Current market snapshot by symbol
    market_snapshots: HashMap<String, MarketSnapshot>,
    /// Order counter for generating venue order IDs
    order_counter: u64,
}

impl ExecutionEngine {
    /// Create a new execution engine.
    pub fn new(config: ExecutionEngineConfig) -> Self {
        let context = StrategyContext::with_defaults(config.initial_capital);
        Self {
            config,
            context,
            fill_model: default_fill_model(),
            metrics: ExecutionMetrics::default(),
            market_snapshots: HashMap::new(),
            order_counter: 0,
        }
    }

    /// Create with a custom fill model.
    pub fn with_fill_model(mut self, fill_model: Box<dyn FillModel>) -> Self {
        self.fill_model = fill_model;
        self
    }

    /// Create with a custom strategy context.
    pub fn with_context(mut self, context: StrategyContext) -> Self {
        self.context = context;
        self
    }

    /// Get mutable reference to the strategy context.
    pub fn context_mut(&mut self) -> &mut StrategyContext {
        &mut self.context
    }

    /// Get reference to the strategy context.
    pub fn context(&self) -> &StrategyContext {
        &self.context
    }

    /// Get execution metrics.
    pub fn metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }

    /// Update the current time.
    pub fn set_time(&mut self, time: DateTime<Utc>) {
        self.context.set_current_time(time);
    }

    /// Process a tick update.
    ///
    /// Updates the market snapshot and processes any pending orders.
    pub fn on_tick(&mut self, tick: &TickData) -> Vec<OrderFilled> {
        // Update market snapshot
        let snapshot = MarketSnapshot::from_tick(tick);
        self.market_snapshots
            .insert(tick.symbol.clone(), snapshot.clone());

        // Update price in context
        self.context.update_price(&tick.symbol, tick.price);
        self.context.set_current_time(tick.timestamp);

        // Process pending orders for this symbol
        self.process_orders_for_symbol(&tick.symbol)
    }

    /// Process a bar update (using close price).
    pub fn on_bar(
        &mut self,
        symbol: &str,
        open: Decimal,
        high: Decimal,
        low: Decimal,
        close: Decimal,
        volume: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<OrderFilled> {
        // Update market snapshot with OHLC
        let snapshot = MarketSnapshot::from_ohlc(open, high, low, close, volume, timestamp);
        self.market_snapshots.insert(symbol.to_string(), snapshot);

        // Update price in context
        self.context.update_price(symbol, close);
        self.context.set_current_time(timestamp);

        // Process pending orders
        self.process_orders_for_symbol(symbol)
    }

    /// Process orders for a specific symbol.
    fn process_orders_for_symbol(&mut self, symbol: &str) -> Vec<OrderFilled> {
        let snapshot = match self.market_snapshots.get(symbol) {
            Some(s) => s.clone(),
            None => return Vec::new(),
        };

        let open_orders = self.context.get_open_orders_for_symbol(symbol);
        let mut fills = Vec::new();

        for order in open_orders {
            // Skip if order is not in a fillable state
            if !matches!(
                order.status,
                OrderStatus::Accepted | OrderStatus::PartiallyFilled | OrderStatus::Triggered
            ) {
                // Try to advance order through lifecycle
                self.advance_order_status(&order);
                continue;
            }

            // Attempt to fill
            let fill_result =
                self.fill_model
                    .get_fill(&order, &snapshot, self.config.commission_rate);

            if fill_result.filled {
                if let Some(fill_event) = self.apply_fill(&order, fill_result) {
                    fills.push(fill_event);
                }
            }
        }

        fills
    }

    /// Advance an order through its lifecycle (initialized -> submitted -> accepted).
    fn advance_order_status(&mut self, order: &Order) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let order_manager = self.context.order_manager().clone();
        let account_id = self.context.account_id().clone();

        runtime.block_on(async {
            match order.status {
                OrderStatus::Initialized => {
                    // Submit the order
                    let _ = order_manager
                        .mark_submitted(&order.client_order_id, account_id.clone())
                        .await;
                    // Immediately accept (in backtesting)
                    self.order_counter += 1;
                    let venue_id = VenueOrderId::new(format!("SIM-{}", self.order_counter));
                    let _ = order_manager
                        .mark_accepted(&order.client_order_id, venue_id, account_id)
                        .await;
                }
                OrderStatus::Submitted => {
                    // Accept the order
                    self.order_counter += 1;
                    let venue_id = VenueOrderId::new(format!("SIM-{}", self.order_counter));
                    let _ = order_manager
                        .mark_accepted(&order.client_order_id, venue_id, account_id)
                        .await;
                }
                _ => {}
            }
        });
    }

    /// Apply a fill to an order and update positions.
    fn apply_fill(&mut self, order: &Order, fill: FillResult) -> Option<OrderFilled> {
        let fill_price = fill.fill_price?;

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;

        let order_manager = self.context.order_manager().clone();
        let account_id = self.context.account_id().clone();

        // Apply fill to order manager
        let venue_order_id = order
            .venue_order_id
            .clone()
            .unwrap_or_else(|| VenueOrderId::new(format!("SIM-{}", self.order_counter)));

        runtime.block_on(async {
            order_manager
                .apply_fill(
                    &order.client_order_id,
                    venue_order_id.clone(),
                    account_id.clone(),
                    fill.trade_id.clone(),
                    fill.fill_qty,
                    fill_price,
                    fill.commission,
                    "USD".to_string(),
                    fill.liquidity_side,
                )
                .await
                .ok()
        })?;

        // Update position
        let current_position = self.context.get_position(order.symbol());
        let position_delta = match order.side {
            OrderSide::Buy => fill.fill_qty,
            OrderSide::Sell => -fill.fill_qty,
        };
        let new_position = current_position + position_delta;
        self.context.update_position(order.symbol(), new_position);

        // Update cash
        let cash_delta = match order.side {
            OrderSide::Buy => -(fill.fill_qty * fill_price + fill.commission),
            OrderSide::Sell => fill.fill_qty * fill_price - fill.commission,
        };
        let new_cash = self.context.cash() + cash_delta;
        self.context.update_cash(new_cash);

        // Update metrics
        self.metrics.record_fill(&fill);
        self.metrics.total_orders += 1;

        // Create fill event
        Some(OrderFilled::new(
            order.client_order_id.clone(),
            venue_order_id,
            account_id,
            order.instrument_id.clone(),
            fill.trade_id,
            order.strategy_id.clone(),
            order.side,
            order.order_type,
            fill.fill_qty,
            fill_price,
            fill.fill_qty, // cum_qty - simplified
            Decimal::ZERO, // leaves_qty - simplified
            "USD".to_string(),
            fill.commission,
            "USD".to_string(),
            fill.liquidity_side,
        ))
    }

    /// Execute an order directly.
    ///
    /// Submits the order and advances it through the lifecycle.
    pub fn execute_order(&mut self, order: &Order) -> Result<ClientOrderId, ContextError> {
        let order_id = self.context.submit_order(order.clone())?;

        // Advance order through lifecycle
        if let Some(submitted_order) = self.context.get_order(&order_id) {
            self.advance_order_status(&submitted_order);
        }

        Ok(order_id)
    }

    /// Execute a market order for a given symbol, side, and quantity.
    pub fn market_order(
        &mut self,
        symbol: &str,
        side: OrderSide,
        quantity: Decimal,
    ) -> Result<ClientOrderId, ContextError> {
        let order_id = self.context.market_order(symbol, side, quantity)?;

        // Advance order through lifecycle
        if let Some(order) = self.context.get_order(&order_id) {
            self.advance_order_status(&order);
        }

        Ok(order_id)
    }

    /// Calculate buy quantity based on position sizing rules.
    pub fn calculate_buy_quantity(&self, _symbol: &str, price: Decimal) -> Decimal {
        if price.is_zero() {
            return Decimal::ZERO;
        }

        let available_cash = if self.config.use_all_available_cash {
            self.context.cash()
        } else {
            self.context.portfolio_value() * self.config.default_position_size
        };

        // Account for commission
        let effective_cash = available_cash / (Decimal::ONE + self.config.commission_rate);

        (effective_cash / price).round_dp(8)
    }

    /// Calculate sell quantity (entire position).
    pub fn calculate_sell_quantity(&self, symbol: &str) -> Decimal {
        self.context.get_position(symbol).max(Decimal::ZERO)
    }

    /// Process all pending orders across all symbols.
    pub fn process_all_orders(&mut self) -> Vec<OrderFilled> {
        let symbols: Vec<String> = self.market_snapshots.keys().cloned().collect();
        let mut all_fills = Vec::new();

        for symbol in symbols {
            let fills = self.process_orders_for_symbol(&symbol);
            all_fills.extend(fills);
        }

        all_fills
    }

    /// Get current position for a symbol.
    pub fn get_position(&self, symbol: &str) -> Decimal {
        self.context.get_position(symbol)
    }

    /// Get current cash balance.
    pub fn get_cash(&self) -> Decimal {
        self.context.cash()
    }

    /// Get current portfolio value.
    pub fn get_portfolio_value(&self) -> Decimal {
        self.context.portfolio_value()
    }

    /// Get all positions.
    pub fn get_positions(&self) -> &HashMap<String, Decimal> {
        self.context.get_all_positions()
    }

    /// Reset the execution engine for a new run.
    pub fn reset(&mut self) {
        self.context.reset();
        self.metrics = ExecutionMetrics::default();
        self.market_snapshots.clear();
        self.order_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::TradeSide;
    use rust_decimal_macros::dec;

    fn create_test_tick(symbol: &str, price: Decimal, quantity: Decimal) -> TickData {
        TickData {
            timestamp: Utc::now(),
            ts_recv: Utc::now(),
            symbol: symbol.to_string(),
            exchange: "TEST".to_string(),
            price,
            quantity,
            side: TradeSide::Buy,
            provider: "test".to_string(),
            trade_id: "test-1".to_string(),
            is_buyer_maker: false,
            sequence: 1,
            raw_dbn: None,
        }
    }

    #[test]
    fn test_engine_creation() {
        let config = ExecutionEngineConfig::default();
        let engine = ExecutionEngine::new(config.clone());

        assert_eq!(engine.get_cash(), config.initial_capital);
        assert_eq!(engine.get_portfolio_value(), config.initial_capital);
    }

    #[test]
    fn test_tick_processing() {
        let config = ExecutionEngineConfig::default();
        let mut engine = ExecutionEngine::new(config);

        let tick = create_test_tick("BTCUSDT", dec!(50000), dec!(1.0));
        engine.on_tick(&tick);

        assert_eq!(engine.context.get_price("BTCUSDT"), Some(dec!(50000)));
    }

    #[test]
    fn test_order_execution() {
        let config = ExecutionEngineConfig {
            initial_capital: dec!(100000),
            ..Default::default()
        };
        let mut engine = ExecutionEngine::new(config);

        // Update price first
        let tick = create_test_tick("BTCUSDT", dec!(50000), dec!(1.0));
        engine.on_tick(&tick);

        // Execute market buy order
        let order_id = engine
            .market_order("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .unwrap();

        // Process the order
        let fills = engine.process_orders_for_symbol("BTCUSDT");
        assert_eq!(fills.len(), 1);

        // Check position was updated
        assert!(engine.get_position("BTCUSDT") > Decimal::ZERO);
        // Verify order ID is a valid UUID format
        assert!(!order_id.0.is_empty());
    }

    #[test]
    fn test_position_tracking() {
        let config = ExecutionEngineConfig {
            initial_capital: dec!(100000),
            ..Default::default()
        };
        let mut engine = ExecutionEngine::new(config);

        let tick = create_test_tick("BTCUSDT", dec!(50000), dec!(1.0));
        engine.on_tick(&tick);

        // Buy
        engine
            .market_order("BTCUSDT", OrderSide::Buy, dec!(0.5))
            .unwrap();
        engine.process_orders_for_symbol("BTCUSDT");

        let position_after_buy = engine.get_position("BTCUSDT");
        assert_eq!(position_after_buy, dec!(0.5));

        // Sell half
        engine
            .market_order("BTCUSDT", OrderSide::Sell, dec!(0.25))
            .unwrap();
        engine.process_orders_for_symbol("BTCUSDT");

        let position_after_sell = engine.get_position("BTCUSDT");
        assert_eq!(position_after_sell, dec!(0.25));
    }

    #[test]
    fn test_limit_order_fill() {
        let config = ExecutionEngineConfig::default();
        let mut engine = ExecutionEngine::new(config);

        // Initial tick
        let tick = create_test_tick("BTCUSDT", dec!(50000), dec!(1.0));
        engine.on_tick(&tick);

        // Submit limit buy below current price
        let _order_id = engine
            .context
            .limit_order("BTCUSDT", OrderSide::Buy, dec!(0.1), dec!(49000))
            .unwrap();

        // Process at current price - should not fill (but will advance order status)
        let fills = engine.process_orders_for_symbol("BTCUSDT");
        assert!(fills.is_empty());

        // Price drops - update with OHLC that touched limit
        // on_bar internally calls process_orders_for_symbol and returns fills
        let fills = engine.on_bar(
            "BTCUSDT",
            dec!(50000),
            dec!(50100),
            dec!(48500),
            dec!(49200),
            dec!(100),
            Utc::now(),
        );

        // Now the limit order should have filled
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].last_px, dec!(49000)); // Filled at limit
    }

    #[test]
    fn test_execution_metrics() {
        let config = ExecutionEngineConfig {
            initial_capital: dec!(100000),
            commission_rate: dec!(0.001),
            ..Default::default()
        };
        let mut engine = ExecutionEngine::new(config);

        let tick = create_test_tick("BTCUSDT", dec!(50000), dec!(1.0));
        engine.on_tick(&tick);

        engine
            .market_order("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .unwrap();
        engine.process_orders_for_symbol("BTCUSDT");

        let metrics = engine.metrics();
        assert_eq!(metrics.filled_orders, 1);
        assert!(metrics.total_commission > Decimal::ZERO);
        assert!(metrics.total_notional > Decimal::ZERO);
    }

    #[test]
    fn test_reset() {
        let config = ExecutionEngineConfig::default();
        let mut engine = ExecutionEngine::new(config.clone());

        let tick = create_test_tick("BTCUSDT", dec!(50000), dec!(1.0));
        engine.on_tick(&tick);

        engine
            .market_order("BTCUSDT", OrderSide::Buy, dec!(0.1))
            .unwrap();
        engine.process_orders_for_symbol("BTCUSDT");

        engine.reset();

        assert_eq!(engine.get_cash(), config.initial_capital);
        assert_eq!(engine.get_position("BTCUSDT"), Decimal::ZERO);
        assert_eq!(engine.metrics().filled_orders, 0);
    }
}
