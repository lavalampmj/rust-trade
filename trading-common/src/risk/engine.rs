//! Risk engine for pre-trade validation and risk management.
//!
//! This module provides the RiskEngine for validating orders before submission
//! and monitoring portfolio risk levels.

use super::types::{RiskCheckResult, RiskCheckType, RiskLevel, TradingState};
use crate::orders::{Order, OrderSide};
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Risk limits configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    // ========================================================================
    // Order Limits
    // ========================================================================
    /// Maximum order quantity
    pub max_order_qty: Option<Decimal>,
    /// Maximum order notional value
    pub max_order_notional: Option<Decimal>,
    /// Minimum order notional value
    pub min_order_notional: Option<Decimal>,

    // ========================================================================
    // Position Limits
    // ========================================================================
    /// Maximum position size per symbol
    pub max_position_qty: Option<Decimal>,
    /// Maximum position notional per symbol
    pub max_position_notional: Option<Decimal>,
    /// Maximum total portfolio notional
    pub max_portfolio_notional: Option<Decimal>,
    /// Maximum concentration per symbol (as fraction of portfolio)
    pub max_concentration: Option<Decimal>,

    // ========================================================================
    // Loss Limits
    // ========================================================================
    /// Maximum daily loss (absolute value)
    pub max_daily_loss: Option<Decimal>,
    /// Maximum daily loss as percentage of equity
    pub max_daily_loss_pct: Option<Decimal>,
    /// Maximum drawdown from peak equity
    pub max_drawdown_pct: Option<Decimal>,

    // ========================================================================
    // Rate Limits
    // ========================================================================
    /// Maximum orders per second
    pub max_orders_per_second: Option<u32>,
    /// Maximum orders per minute
    pub max_orders_per_minute: Option<u32>,
    /// Maximum open orders
    pub max_open_orders: Option<u32>,
    /// Maximum open orders per symbol
    pub max_open_orders_per_symbol: Option<u32>,

    // ========================================================================
    // Price Limits
    // ========================================================================
    /// Maximum deviation from reference price (percentage)
    pub max_price_deviation_pct: Option<Decimal>,
    /// Minimum price
    pub min_price: Option<Decimal>,
    /// Maximum price
    pub max_price: Option<Decimal>,
}

impl RiskLimits {
    /// Create empty limits (no restrictions)
    pub fn none() -> Self {
        Self {
            max_order_qty: None,
            max_order_notional: None,
            min_order_notional: None,
            max_position_qty: None,
            max_position_notional: None,
            max_portfolio_notional: None,
            max_concentration: None,
            max_daily_loss: None,
            max_daily_loss_pct: None,
            max_drawdown_pct: None,
            max_orders_per_second: None,
            max_orders_per_minute: None,
            max_open_orders: None,
            max_open_orders_per_symbol: None,
            max_price_deviation_pct: None,
            min_price: None,
            max_price: None,
        }
    }

    /// Create conservative default limits
    pub fn conservative() -> Self {
        Self {
            max_order_qty: Some(Decimal::new(10, 0)),
            max_order_notional: Some(Decimal::new(100_000, 0)),
            min_order_notional: Some(Decimal::new(10, 0)),
            max_position_qty: Some(Decimal::new(100, 0)),
            max_position_notional: Some(Decimal::new(500_000, 0)),
            max_portfolio_notional: Some(Decimal::new(1_000_000, 0)),
            max_concentration: Some(Decimal::new(25, 2)), // 25%
            max_daily_loss: Some(Decimal::new(10_000, 0)),
            max_daily_loss_pct: Some(Decimal::new(5, 2)), // 5%
            max_drawdown_pct: Some(Decimal::new(20, 2)),  // 20%
            max_orders_per_second: Some(10),
            max_orders_per_minute: Some(100),
            max_open_orders: Some(100),
            max_open_orders_per_symbol: Some(10),
            max_price_deviation_pct: Some(Decimal::new(5, 2)), // 5%
            min_price: None,
            max_price: None,
        }
    }

    /// Builder method for max order quantity
    pub fn with_max_order_qty(mut self, qty: Decimal) -> Self {
        self.max_order_qty = Some(qty);
        self
    }

    /// Builder method for max order notional
    pub fn with_max_order_notional(mut self, notional: Decimal) -> Self {
        self.max_order_notional = Some(notional);
        self
    }

    /// Builder method for max position quantity
    pub fn with_max_position_qty(mut self, qty: Decimal) -> Self {
        self.max_position_qty = Some(qty);
        self
    }

    /// Builder method for max daily loss
    pub fn with_max_daily_loss(mut self, loss: Decimal) -> Self {
        self.max_daily_loss = Some(loss);
        self
    }

    /// Builder method for max drawdown
    pub fn with_max_drawdown_pct(mut self, pct: Decimal) -> Self {
        self.max_drawdown_pct = Some(pct);
        self
    }
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self::none()
    }
}

/// Portfolio state for risk calculations.
#[derive(Debug, Clone, Default)]
pub struct PortfolioState {
    /// Current equity
    pub equity: Decimal,
    /// Peak equity (for drawdown calculation)
    pub peak_equity: Decimal,
    /// Starting equity for the day
    pub start_of_day_equity: Decimal,
    /// Current positions by symbol
    pub positions: HashMap<String, Decimal>,
    /// Current prices by symbol
    pub prices: HashMap<String, Decimal>,
    /// Open orders count
    pub open_orders: u32,
    /// Open orders by symbol
    pub open_orders_by_symbol: HashMap<String, u32>,
    /// Today's realized PnL
    pub daily_pnl: Decimal,
}

impl PortfolioState {
    /// Create new portfolio state
    pub fn new(equity: Decimal) -> Self {
        Self {
            equity,
            peak_equity: equity,
            start_of_day_equity: equity,
            positions: HashMap::new(),
            prices: HashMap::new(),
            open_orders: 0,
            open_orders_by_symbol: HashMap::new(),
            daily_pnl: Decimal::ZERO,
        }
    }

    /// Update equity and track peak
    pub fn update_equity(&mut self, new_equity: Decimal) {
        self.equity = new_equity;
        if new_equity > self.peak_equity {
            self.peak_equity = new_equity;
        }
    }

    /// Reset for new trading day
    pub fn reset_daily(&mut self) {
        self.start_of_day_equity = self.equity;
        self.daily_pnl = Decimal::ZERO;
    }

    /// Calculate current drawdown percentage
    pub fn drawdown_pct(&self) -> Decimal {
        if self.peak_equity.is_zero() {
            Decimal::ZERO
        } else {
            ((self.peak_equity - self.equity) / self.peak_equity) * Decimal::new(100, 0)
        }
    }

    /// Calculate daily loss percentage
    pub fn daily_loss_pct(&self) -> Decimal {
        if self.start_of_day_equity.is_zero() {
            Decimal::ZERO
        } else {
            let loss = self.start_of_day_equity - self.equity;
            if loss > Decimal::ZERO {
                (loss / self.start_of_day_equity) * Decimal::new(100, 0)
            } else {
                Decimal::ZERO
            }
        }
    }

    /// Get position notional for a symbol
    pub fn position_notional(&self, symbol: &str) -> Decimal {
        let qty = self.positions.get(symbol).copied().unwrap_or(Decimal::ZERO);
        let price = self.prices.get(symbol).copied().unwrap_or(Decimal::ZERO);
        qty.abs() * price
    }

    /// Get total portfolio notional
    pub fn total_notional(&self) -> Decimal {
        self.positions
            .keys()
            .map(|s| self.position_notional(s))
            .sum()
    }

    /// Get concentration for a symbol
    pub fn concentration(&self, symbol: &str) -> Decimal {
        let total = self.total_notional();
        if total.is_zero() {
            Decimal::ZERO
        } else {
            self.position_notional(symbol) / total
        }
    }
}

/// Order rate tracker for rate limiting.
#[derive(Debug, Clone)]
struct OrderRateTracker {
    /// Timestamps of recent orders
    order_times: Vec<DateTime<Utc>>,
    /// Window for per-second limit
    second_window: Duration,
    /// Window for per-minute limit
    minute_window: Duration,
}

impl OrderRateTracker {
    fn new() -> Self {
        Self {
            order_times: Vec::new(),
            second_window: Duration::seconds(1),
            minute_window: Duration::minutes(1),
        }
    }

    fn record_order(&mut self) {
        let now = Utc::now();
        self.order_times.push(now);
        // Clean up old entries (older than 1 minute)
        let cutoff = now - Duration::minutes(2);
        self.order_times.retain(|t| *t > cutoff);
    }

    fn orders_in_last_second(&self) -> u32 {
        let cutoff = Utc::now() - self.second_window;
        self.order_times.iter().filter(|t| **t > cutoff).count() as u32
    }

    fn orders_in_last_minute(&self) -> u32 {
        let cutoff = Utc::now() - self.minute_window;
        self.order_times.iter().filter(|t| **t > cutoff).count() as u32
    }
}

impl Default for OrderRateTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Risk engine for pre-trade validation.
///
/// The RiskEngine validates orders against configured limits before
/// they are submitted to the execution system.
#[derive(Debug)]
pub struct RiskEngine {
    /// Risk limits configuration
    pub limits: RiskLimits,
    /// Current trading state
    pub trading_state: TradingState,
    /// Portfolio state for risk calculations
    pub portfolio: PortfolioState,
    /// Order rate tracker
    rate_tracker: OrderRateTracker,
    /// Symbol-specific limits (override global)
    symbol_limits: HashMap<String, RiskLimits>,
    /// Whether to enforce limits strictly
    pub strict_mode: bool,
}

impl RiskEngine {
    /// Create a new risk engine with given limits
    pub fn new(limits: RiskLimits) -> Self {
        Self {
            limits,
            trading_state: TradingState::Active,
            portfolio: PortfolioState::default(),
            rate_tracker: OrderRateTracker::new(),
            symbol_limits: HashMap::new(),
            strict_mode: true,
        }
    }

    /// Create with no limits (permissive mode)
    pub fn permissive() -> Self {
        Self::new(RiskLimits::none())
    }

    /// Create with conservative defaults
    pub fn conservative() -> Self {
        Self::new(RiskLimits::conservative())
    }

    /// Set the trading state
    pub fn set_trading_state(&mut self, state: TradingState) {
        self.trading_state = state;
    }

    /// Set portfolio state
    pub fn set_portfolio(&mut self, portfolio: PortfolioState) {
        self.portfolio = portfolio;
    }

    /// Update portfolio equity
    pub fn update_equity(&mut self, equity: Decimal) {
        self.portfolio.update_equity(equity);
    }

    /// Update position for a symbol
    pub fn update_position(&mut self, symbol: &str, qty: Decimal) {
        self.portfolio.positions.insert(symbol.to_string(), qty);
    }

    /// Update price for a symbol
    pub fn update_price(&mut self, symbol: &str, price: Decimal) {
        self.portfolio.prices.insert(symbol.to_string(), price);
    }

    /// Set symbol-specific limits
    pub fn set_symbol_limits(&mut self, symbol: impl Into<String>, limits: RiskLimits) {
        self.symbol_limits.insert(symbol.into(), limits);
    }

    /// Get effective limits for a symbol
    fn effective_limits(&self, symbol: &str) -> &RiskLimits {
        self.symbol_limits.get(symbol).unwrap_or(&self.limits)
    }

    /// Validate an order before submission.
    ///
    /// Returns a list of check results. If any check fails and strict_mode is true,
    /// the order should be rejected.
    pub fn validate_order(
        &mut self,
        order: &Order,
        reference_price: Option<Decimal>,
    ) -> Vec<RiskCheckResult> {
        let mut results = Vec::new();
        let symbol = &order.instrument_id.symbol;
        let limits = self.effective_limits(symbol).clone();

        // Trading state check
        results.push(self.check_trading_state(order));

        // Order quantity check
        if let Some(max_qty) = limits.max_order_qty {
            results.push(self.check_order_quantity(order, max_qty));
        }

        // Order notional check
        if limits.max_order_notional.is_some() || limits.min_order_notional.is_some() {
            if let Some(price) = order.price.or(reference_price) {
                results.push(self.check_order_notional(order, price, &limits));
            }
        }

        // Position check
        if limits.max_position_qty.is_some() || limits.max_position_notional.is_some() {
            results.push(self.check_position_limits(order, reference_price, &limits));
        }

        // Concentration check
        if let Some(max_conc) = limits.max_concentration {
            if let Some(price) = reference_price {
                results.push(self.check_concentration(order, price, max_conc));
            }
        }

        // Daily loss check
        if limits.max_daily_loss.is_some() || limits.max_daily_loss_pct.is_some() {
            results.push(self.check_daily_loss(&limits));
        }

        // Drawdown check
        if let Some(max_dd) = limits.max_drawdown_pct {
            results.push(self.check_drawdown(max_dd));
        }

        // Rate limit checks
        if limits.max_orders_per_second.is_some() || limits.max_orders_per_minute.is_some() {
            results.push(self.check_order_rate(&limits));
        }

        // Open orders check
        if let Some(max_open) = limits.max_open_orders {
            results.push(self.check_open_orders(max_open));
        }

        if let Some(max_open_symbol) = limits.max_open_orders_per_symbol {
            results.push(self.check_open_orders_per_symbol(symbol, max_open_symbol));
        }

        // Price deviation check
        if let (Some(max_dev), Some(ref_price), Some(order_price)) =
            (limits.max_price_deviation_pct, reference_price, order.price)
        {
            results.push(self.check_price_deviation(order_price, ref_price, max_dev));
        }

        results
    }

    /// Check if all validation results pass
    pub fn is_valid(&self, results: &[RiskCheckResult]) -> bool {
        results.iter().all(|r| r.passed)
    }

    /// Get the first failure from validation results
    pub fn first_failure<'a>(&self, results: &'a [RiskCheckResult]) -> Option<&'a RiskCheckResult> {
        results.iter().find(|r| !r.passed)
    }

    /// Record that an order was placed (for rate limiting)
    pub fn record_order(&mut self) {
        self.rate_tracker.record_order();
    }

    // ========================================================================
    // Individual Check Methods
    // ========================================================================

    fn check_trading_state(&self, order: &Order) -> RiskCheckResult {
        if !self.trading_state.can_place_orders() {
            return RiskCheckResult::fail(
                RiskCheckType::TradingState,
                format!(
                    "Trading state {} does not allow new orders",
                    self.trading_state
                ),
            );
        }

        // Check if this order increases position in reduce-only mode
        if !self.trading_state.can_increase_position() {
            let symbol = &order.instrument_id.symbol;
            let current_pos = self
                .portfolio
                .positions
                .get(symbol)
                .copied()
                .unwrap_or(Decimal::ZERO);

            let increases_position = match order.side {
                OrderSide::Buy => current_pos >= Decimal::ZERO,
                OrderSide::Sell => current_pos <= Decimal::ZERO,
            };

            if increases_position {
                return RiskCheckResult::fail(
                    RiskCheckType::TradingState,
                    format!(
                        "Trading state {} only allows position reduction",
                        self.trading_state
                    ),
                );
            }
        }

        RiskCheckResult::pass(RiskCheckType::TradingState)
    }

    fn check_order_quantity(&self, order: &Order, max_qty: Decimal) -> RiskCheckResult {
        if order.quantity > max_qty {
            RiskCheckResult::fail(
                RiskCheckType::OrderQuantity,
                "Order quantity exceeds maximum",
            )
            .with_current(order.quantity)
            .with_limit(max_qty)
        } else {
            RiskCheckResult::pass(RiskCheckType::OrderQuantity)
        }
    }

    fn check_order_notional(
        &self,
        order: &Order,
        price: Decimal,
        limits: &RiskLimits,
    ) -> RiskCheckResult {
        let notional = order.quantity * price;

        if let Some(max) = limits.max_order_notional {
            if notional > max {
                return RiskCheckResult::fail(
                    RiskCheckType::OrderNotional,
                    "Order notional exceeds maximum",
                )
                .with_current(notional)
                .with_limit(max);
            }
        }

        if let Some(min) = limits.min_order_notional {
            if notional < min {
                return RiskCheckResult::fail(
                    RiskCheckType::OrderNotional,
                    "Order notional below minimum",
                )
                .with_current(notional)
                .with_limit(min);
            }
        }

        RiskCheckResult::pass(RiskCheckType::OrderNotional)
    }

    fn check_position_limits(
        &self,
        order: &Order,
        reference_price: Option<Decimal>,
        limits: &RiskLimits,
    ) -> RiskCheckResult {
        let symbol = &order.instrument_id.symbol;
        let current_pos = self
            .portfolio
            .positions
            .get(symbol)
            .copied()
            .unwrap_or(Decimal::ZERO);

        // Calculate new position after order
        let new_pos = match order.side {
            OrderSide::Buy => current_pos + order.quantity,
            OrderSide::Sell => current_pos - order.quantity,
        };

        // Check position quantity
        if let Some(max_qty) = limits.max_position_qty {
            if new_pos.abs() > max_qty {
                return RiskCheckResult::fail(
                    RiskCheckType::PositionSize,
                    "Position size would exceed maximum",
                )
                .with_current(new_pos.abs())
                .with_limit(max_qty);
            }
        }

        // Check position notional
        if let (Some(max_notional), Some(price)) = (limits.max_position_notional, reference_price) {
            let new_notional = new_pos.abs() * price;
            if new_notional > max_notional {
                return RiskCheckResult::fail(
                    RiskCheckType::PositionNotional,
                    "Position notional would exceed maximum",
                )
                .with_current(new_notional)
                .with_limit(max_notional);
            }
        }

        RiskCheckResult::pass(RiskCheckType::PositionSize)
    }

    fn check_concentration(
        &self,
        order: &Order,
        price: Decimal,
        max_concentration: Decimal,
    ) -> RiskCheckResult {
        let symbol = &order.instrument_id.symbol;
        let current_pos = self
            .portfolio
            .positions
            .get(symbol)
            .copied()
            .unwrap_or(Decimal::ZERO);

        let new_pos = match order.side {
            OrderSide::Buy => current_pos + order.quantity,
            OrderSide::Sell => current_pos - order.quantity,
        };

        let new_notional = new_pos.abs() * price;
        let total_notional = self.portfolio.total_notional() + (order.quantity * price);

        if total_notional.is_zero() {
            return RiskCheckResult::pass(RiskCheckType::Concentration);
        }

        let concentration = new_notional / total_notional;
        if concentration > max_concentration {
            return RiskCheckResult::fail(
                RiskCheckType::Concentration,
                "Position concentration would exceed maximum",
            )
            .with_current(format!("{:.2}%", concentration * Decimal::new(100, 0)))
            .with_limit(format!("{:.2}%", max_concentration * Decimal::new(100, 0)));
        }

        RiskCheckResult::pass(RiskCheckType::Concentration)
    }

    fn check_daily_loss(&self, limits: &RiskLimits) -> RiskCheckResult {
        let daily_loss =
            (self.portfolio.start_of_day_equity - self.portfolio.equity).max(Decimal::ZERO);

        if let Some(max_loss) = limits.max_daily_loss {
            if daily_loss > max_loss {
                return RiskCheckResult::fail(
                    RiskCheckType::DailyLoss,
                    "Daily loss limit exceeded",
                )
                .with_current(daily_loss)
                .with_limit(max_loss);
            }
        }

        if let Some(max_loss_pct) = limits.max_daily_loss_pct {
            let loss_pct = self.portfolio.daily_loss_pct();
            if loss_pct > max_loss_pct {
                return RiskCheckResult::fail(
                    RiskCheckType::DailyLoss,
                    "Daily loss percentage limit exceeded",
                )
                .with_current(format!("{:.2}%", loss_pct))
                .with_limit(format!("{:.2}%", max_loss_pct));
            }
        }

        RiskCheckResult::pass(RiskCheckType::DailyLoss)
    }

    fn check_drawdown(&self, max_drawdown_pct: Decimal) -> RiskCheckResult {
        let drawdown = self.portfolio.drawdown_pct();
        if drawdown > max_drawdown_pct {
            return RiskCheckResult::fail(RiskCheckType::Drawdown, "Maximum drawdown exceeded")
                .with_current(format!("{:.2}%", drawdown))
                .with_limit(format!("{:.2}%", max_drawdown_pct));
        }

        // Warning at 80% of limit
        let warning_level = max_drawdown_pct * Decimal::new(80, 2);
        if drawdown > warning_level {
            return RiskCheckResult::pass_with_warning(
                RiskCheckType::Drawdown,
                format!("Drawdown at {:.2}%, approaching limit", drawdown),
            )
            .with_risk_level(RiskLevel::Warning);
        }

        RiskCheckResult::pass(RiskCheckType::Drawdown)
    }

    fn check_order_rate(&self, limits: &RiskLimits) -> RiskCheckResult {
        if let Some(max_per_second) = limits.max_orders_per_second {
            let rate = self.rate_tracker.orders_in_last_second();
            if rate >= max_per_second {
                return RiskCheckResult::fail(
                    RiskCheckType::OrderRate,
                    "Orders per second limit exceeded",
                )
                .with_current(rate)
                .with_limit(max_per_second);
            }
        }

        if let Some(max_per_minute) = limits.max_orders_per_minute {
            let rate = self.rate_tracker.orders_in_last_minute();
            if rate >= max_per_minute {
                return RiskCheckResult::fail(
                    RiskCheckType::OrderRate,
                    "Orders per minute limit exceeded",
                )
                .with_current(rate)
                .with_limit(max_per_minute);
            }
        }

        RiskCheckResult::pass(RiskCheckType::OrderRate)
    }

    fn check_open_orders(&self, max_open: u32) -> RiskCheckResult {
        if self.portfolio.open_orders >= max_open {
            return RiskCheckResult::fail(
                RiskCheckType::OrderRate,
                "Maximum open orders limit reached",
            )
            .with_current(self.portfolio.open_orders)
            .with_limit(max_open);
        }
        RiskCheckResult::pass(RiskCheckType::OrderRate)
    }

    fn check_open_orders_per_symbol(&self, symbol: &str, max_open: u32) -> RiskCheckResult {
        let current = self
            .portfolio
            .open_orders_by_symbol
            .get(symbol)
            .copied()
            .unwrap_or(0);

        if current >= max_open {
            return RiskCheckResult::fail(
                RiskCheckType::OrderRate,
                format!("Maximum open orders for {} limit reached", symbol),
            )
            .with_current(current)
            .with_limit(max_open);
        }
        RiskCheckResult::pass(RiskCheckType::OrderRate)
    }

    fn check_price_deviation(
        &self,
        order_price: Decimal,
        reference_price: Decimal,
        max_deviation_pct: Decimal,
    ) -> RiskCheckResult {
        if reference_price.is_zero() {
            return RiskCheckResult::pass(RiskCheckType::PriceValidation);
        }

        let deviation =
            ((order_price - reference_price).abs() / reference_price) * Decimal::new(100, 0);

        if deviation > max_deviation_pct {
            return RiskCheckResult::fail(
                RiskCheckType::PriceValidation,
                "Order price deviation exceeds maximum",
            )
            .with_current(format!("{:.2}%", deviation))
            .with_limit(format!("{:.2}%", max_deviation_pct));
        }

        RiskCheckResult::pass(RiskCheckType::PriceValidation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::{OrderBuilder, OrderSide, OrderType};
    use rust_decimal_macros::dec;

    fn create_test_order(symbol: &str, side: OrderSide, qty: Decimal) -> Order {
        OrderBuilder::new(OrderType::Market, symbol, side, qty)
            .build()
            .unwrap()
    }

    #[test]
    fn test_risk_engine_creation() {
        let engine = RiskEngine::conservative();
        assert!(engine.limits.max_order_qty.is_some());
        assert_eq!(engine.trading_state, TradingState::Active);
    }

    #[test]
    fn test_order_quantity_check() {
        let limits = RiskLimits::none().with_max_order_qty(dec!(10));
        let mut engine = RiskEngine::new(limits);

        let small_order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(5));
        let results = engine.validate_order(&small_order, Some(dec!(50000)));
        assert!(engine.is_valid(&results));

        let large_order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(15));
        let results = engine.validate_order(&large_order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));

        let failure = engine.first_failure(&results).unwrap();
        assert_eq!(failure.check_type, RiskCheckType::OrderQuantity);
    }

    #[test]
    fn test_position_limit_check() {
        let limits = RiskLimits::none().with_max_position_qty(dec!(20));
        let mut engine = RiskEngine::new(limits);

        // Set existing position
        engine.update_position("BTCUSDT", dec!(15));

        // Order that would exceed limit
        let order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(10));
        let results = engine.validate_order(&order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));

        // Order that stays within limit
        let small_order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(3));
        let results = engine.validate_order(&small_order, Some(dec!(50000)));
        assert!(engine.is_valid(&results));
    }

    #[test]
    fn test_trading_state_halted() {
        let mut engine = RiskEngine::permissive();
        engine.set_trading_state(TradingState::Halted);

        let order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(1));
        let results = engine.validate_order(&order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));

        let failure = engine.first_failure(&results).unwrap();
        assert_eq!(failure.check_type, RiskCheckType::TradingState);
    }

    #[test]
    fn test_trading_state_reduce_only() {
        let mut engine = RiskEngine::permissive();
        engine.set_trading_state(TradingState::ReduceOnly);
        engine.update_position("BTCUSDT", dec!(10)); // Long position

        // Sell (reduces position) - should pass
        let sell_order = create_test_order("BTCUSDT", OrderSide::Sell, dec!(5));
        let results = engine.validate_order(&sell_order, Some(dec!(50000)));
        assert!(engine.is_valid(&results));

        // Buy (increases position) - should fail
        let buy_order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(5));
        let results = engine.validate_order(&buy_order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));
    }

    #[test]
    fn test_drawdown_check() {
        let limits = RiskLimits::none().with_max_drawdown_pct(dec!(10));
        let mut engine = RiskEngine::new(limits);

        let mut portfolio = PortfolioState::new(dec!(100000));
        portfolio.peak_equity = dec!(100000);
        portfolio.equity = dec!(85000); // 15% drawdown
        engine.set_portfolio(portfolio);

        let order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(1));
        let results = engine.validate_order(&order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));
    }

    #[test]
    fn test_daily_loss_check() {
        let limits = RiskLimits::none().with_max_daily_loss(dec!(5000));
        let mut engine = RiskEngine::new(limits);

        let mut portfolio = PortfolioState::new(dec!(100000));
        portfolio.start_of_day_equity = dec!(100000);
        portfolio.equity = dec!(93000); // $7000 loss
        engine.set_portfolio(portfolio);

        let order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(1));
        let results = engine.validate_order(&order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));
    }

    #[test]
    fn test_portfolio_state_calculations() {
        let mut state = PortfolioState::new(dec!(100000));
        state.peak_equity = dec!(110000);
        state.equity = dec!(99000);
        state.start_of_day_equity = dec!(100000);

        // Drawdown: (110000 - 99000) / 110000 = 10%
        assert!(state.drawdown_pct() > dec!(9.9) && state.drawdown_pct() < dec!(10.1));

        // Daily loss: (100000 - 99000) / 100000 = 1%
        assert_eq!(state.daily_loss_pct(), dec!(1));
    }

    #[test]
    fn test_permissive_mode() {
        let mut engine = RiskEngine::permissive();

        // Large order should pass with no limits
        let order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(1000));
        let results = engine.validate_order(&order, Some(dec!(50000)));
        assert!(engine.is_valid(&results));
    }

    #[test]
    fn test_symbol_specific_limits() {
        let mut engine = RiskEngine::new(RiskLimits::none().with_max_order_qty(dec!(100)));

        // Set stricter limit for BTCUSDT
        engine.set_symbol_limits("BTCUSDT", RiskLimits::none().with_max_order_qty(dec!(10)));

        // BTCUSDT should use stricter limit
        let btc_order = create_test_order("BTCUSDT", OrderSide::Buy, dec!(50));
        let results = engine.validate_order(&btc_order, Some(dec!(50000)));
        assert!(!engine.is_valid(&results));

        // ETHUSDT should use global limit
        let eth_order = create_test_order("ETHUSDT", OrderSide::Buy, dec!(50));
        let results = engine.validate_order(&eth_order, Some(dec!(3000)));
        assert!(engine.is_valid(&results));
    }
}
