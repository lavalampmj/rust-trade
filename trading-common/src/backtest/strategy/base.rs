use crate::accounts::AccountEvent;
use crate::backtest::bar_generator::SessionAwareConfig;
use crate::data::events::MarketDataEvent;
use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
use crate::instruments::SessionEvent;
use crate::orders::{ClientOrderId, Order, OrderCanceled, OrderEventAny, OrderFilled, OrderRejected, OrderSide};
use crate::series::bars_context::BarsContext;
use crate::series::MaximumBarsLookBack;
use rust_decimal::Decimal;
use std::collections::HashMap;

use super::position::PositionEvent;
use super::state::StrategyStateEvent;

/// Signal output from a strategy.
///
/// For backward compatibility, strategies return signals which are then
/// converted to orders by the execution engine. For advanced order management,
/// strategies can use the `StrategyContext` directly.
#[derive(Debug, Clone)]
pub enum Signal {
    /// Buy signal with symbol and quantity
    Buy { symbol: String, quantity: Decimal },
    /// Sell signal with symbol and quantity
    Sell { symbol: String, quantity: Decimal },
    /// No action
    Hold,
}

impl Signal {
    /// Create a buy signal
    pub fn buy(symbol: impl Into<String>, quantity: Decimal) -> Self {
        Signal::Buy {
            symbol: symbol.into(),
            quantity,
        }
    }

    /// Create a sell signal
    pub fn sell(symbol: impl Into<String>, quantity: Decimal) -> Self {
        Signal::Sell {
            symbol: symbol.into(),
            quantity,
        }
    }

    /// Check if this is a buy signal
    pub fn is_buy(&self) -> bool {
        matches!(self, Signal::Buy { .. })
    }

    /// Check if this is a sell signal
    pub fn is_sell(&self) -> bool {
        matches!(self, Signal::Sell { .. })
    }

    /// Check if this is a hold signal
    pub fn is_hold(&self) -> bool {
        matches!(self, Signal::Hold)
    }

    /// Get the symbol if this is a buy or sell signal
    pub fn symbol(&self) -> Option<&str> {
        match self {
            Signal::Buy { symbol, .. } | Signal::Sell { symbol, .. } => Some(symbol),
            Signal::Hold => None,
        }
    }

    /// Get the quantity if this is a buy or sell signal
    pub fn quantity(&self) -> Option<Decimal> {
        match self {
            Signal::Buy { quantity, .. } | Signal::Sell { quantity, .. } => Some(*quantity),
            Signal::Hold => None,
        }
    }

    /// Convert signal to order side (if applicable)
    pub fn to_order_side(&self) -> Option<OrderSide> {
        match self {
            Signal::Buy { .. } => Some(OrderSide::Buy),
            Signal::Sell { .. } => Some(OrderSide::Sell),
            Signal::Hold => None,
        }
    }
}

pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;

    /// Unified bar data processing method with BarsContext
    ///
    /// This is the primary method for processing market data.
    ///
    /// # Arguments
    /// - `bar_data`: Current bar information including:
    ///   - current_tick: Optional tick (Some for OnEachTick/OnPriceMove, None for OnCloseBar)
    ///   - ohlc_bar: Current OHLC bar state (always present)
    ///   - metadata: Bar state information (first tick, closed, synthetic, etc.)
    /// - `bars`: BarsContext providing synchronized access to:
    ///   - OHLCV series with reverse indexing: `bars.close[0]` (current), `bars.close[1]` (previous)
    ///   - Built-in helpers: `bars.sma(period)`, `bars.highest_high(period)`, etc.
    ///   - Custom series registration for indicators
    ///
    /// # Example
    /// ```text
    /// fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    ///     // Access current and historical prices
    ///     let current_close = bars.close[0];
    ///     let prev_close = bars.close[1];
    ///
    ///     // Use built-in indicators
    ///     if let (Some(sma20), Some(sma50)) = (bars.sma(20), bars.sma(50)) {
    ///         if sma20 > sma50 {
    ///             return Signal::Buy { symbol: bars.symbol().to_string(), quantity: 1.into() };
    ///         }
    ///     }
    ///     Signal::Hold
    /// }
    /// ```
    fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal;

    /// Initialize strategy with parameters
    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String>;

    /// Reset strategy state for new backtest
    fn reset(&mut self) {
        // Default implementation does nothing
        // Strategies can override if needed
    }

    // ========================================================================
    // Warmup / Ready State
    // ========================================================================

    /// Check if strategy has enough data to generate valid signals.
    ///
    /// Returns true when all required indicators are warmed up.
    /// **REQUIRED** - no default implementation. Strategies must explicitly
    /// consider their warmup requirements.
    ///
    /// Each strategy knows its own readiness state based on its indicators.
    ///
    /// # Arguments
    /// - `bars`: BarsContext to check for data availability
    ///
    /// # Example
    /// ```text
    /// fn is_ready(&self, bars: &BarsContext) -> bool {
    ///     // Ready when we have enough data for our longest indicator
    ///     bars.is_ready_for(self.long_period)
    /// }
    /// ```
    fn is_ready(&self, bars: &BarsContext) -> bool;

    /// Return minimum bars needed before is_ready() can return true.
    ///
    /// Used by the framework for progress indication and optimization.
    /// **REQUIRED** - must match the logic in is_ready().
    ///
    /// # Example
    /// ```text
    /// fn warmup_period(&self) -> usize {
    ///     self.long_period // Longest indicator period
    /// }
    /// ```
    fn warmup_period(&self) -> usize;

    /// Specify the operational mode for bar data processing
    ///
    /// - OnEachTick: Fire on every tick
    /// - OnPriceMove: Fire only when price changes
    /// - OnCloseBar: Fire only when bar closes (default)
    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar // Default
    }

    /// Specify the preferred bar type for this strategy
    ///
    /// Returns the type of bars this strategy wants to process
    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute) // Default
    }

    /// Specify the maximum bars lookback for series data
    ///
    /// This determines how much historical data is kept in BarsContext.
    /// - Fixed(256): Default, keeps last 256 bars (FIFO eviction)
    /// - Infinite: No eviction, grows unbounded (use with caution)
    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        MaximumBarsLookBack::Fixed(256) // Default
    }

    /// Specify session-aware bar configuration for this strategy.
    ///
    /// Controls how bars align to trading session boundaries:
    /// - **Session schedule**: Defines trading hours (e.g., US equity 9:30-16:00 ET)
    /// - **Session open alignment**: Align first bar to session open, not first tick
    /// - **Session close truncation**: Force-close partial bars at session end
    ///
    /// # Session-Aligned Bars
    ///
    /// When `align_to_session_open` is true, the first bar of each session starts
    /// at the session open time rather than when the first tick arrives:
    ///
    /// ```text
    /// Session opens at 9:30 AM, first tick at 9:30:15 AM:
    /// - align_to_session_open = true:  First 1-min bar is 9:30:00-9:31:00
    /// - align_to_session_open = false: First 1-min bar is 9:30:15-9:31:15
    /// ```
    ///
    /// # Session-Truncated Bars
    ///
    /// When `truncate_at_session_close` is true, partial bars are closed at
    /// session end rather than carrying over to the next session:
    ///
    /// ```text
    /// 500-tick bars, session closes with 300 ticks accumulated:
    /// - truncate_at_session_close = true:  Close bar at tick 300 (truncated)
    /// - truncate_at_session_close = false: Continue bar to next session
    ///
    /// 1-minute bars, session closes at 15:59:30:
    /// - truncate_at_session_close = true:  Close 15:59 bar at 15:59:30
    /// - truncate_at_session_close = false: Let bar complete naturally
    /// ```
    ///
    /// # Bar Metadata
    ///
    /// Session-aware bars include metadata flags:
    /// - `is_session_aligned`: True if bar was aligned to session open
    /// - `is_session_truncated`: True if bar was truncated at session close
    ///
    /// # Example
    ///
    /// ```text
    /// use trading_common::backtest::SessionAwareConfig;
    /// use trading_common::instruments::SessionSchedule;
    /// use std::sync::Arc;
    ///
    /// fn session_config(&self) -> SessionAwareConfig {
    ///     // Use US equity schedule (9:30 AM - 4:00 PM ET)
    ///     let schedule = Arc::new(SessionSchedule::us_equity());
    ///     SessionAwareConfig::with_session(schedule)
    ///         .with_session_open_alignment(true)
    ///         .with_session_close_truncation(true)
    /// }
    /// ```
    ///
    /// # Default
    ///
    /// Returns `SessionAwareConfig::default()` which operates in 24/7 mode
    /// (no session boundaries, no alignment, no truncation).
    fn session_config(&self) -> SessionAwareConfig {
        SessionAwareConfig::default()
    }

    // ========================================================================
    // Order Event Handlers (Optional - for advanced order management)
    // ========================================================================

    /// Called when an order is filled (partial or complete).
    ///
    /// Override this method to react to order fills. This is useful for:
    /// - Updating internal state based on executed orders
    /// - Placing follow-up orders (e.g., stop-loss after entry)
    /// - Tracking realized P&L
    ///
    /// # Example
    /// ```text
    /// fn on_order_filled(&mut self, event: &OrderFilled) {
    ///     // Place a stop-loss after a buy is filled
    ///     if event.order_side == OrderSide::Buy {
    ///         let stop_price = event.last_px * Decimal::new(95, 2); // 5% below
    ///         self.pending_stop = Some(stop_price);
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_order_filled(&mut self, event: &OrderFilled) {
        // Default: no-op
    }

    /// Called when an order is rejected by the venue.
    ///
    /// Override this method to handle order rejections. This is useful for:
    /// - Logging rejection reasons
    /// - Adjusting order parameters and retrying
    /// - Updating strategy state
    #[allow(unused_variables)]
    fn on_order_rejected(&mut self, event: &OrderRejected) {
        // Default: no-op
    }

    /// Called when an order is canceled.
    ///
    /// Override this method to handle order cancellations. This is useful for:
    /// - Tracking canceled orders
    /// - Placing replacement orders
    /// - Updating order tracking state
    #[allow(unused_variables)]
    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        // Default: no-op
    }

    /// Called when a new order is submitted.
    ///
    /// Override this method to track submitted orders.
    #[allow(unused_variables)]
    fn on_order_submitted(&mut self, order: &Order) {
        // Default: no-op
    }

    // ========================================================================
    // Advanced Order Management (Optional)
    // ========================================================================

    /// Whether this strategy uses advanced order management.
    ///
    /// When true, the execution engine will:
    /// - Call order event handlers
    /// - Not auto-convert signals to market orders (strategy manages orders directly)
    ///
    /// Default is false for backward compatibility with signal-based strategies.
    fn uses_order_management(&self) -> bool {
        false // Default: use signal-based execution
    }

    /// Get orders to submit for this bar.
    ///
    /// Advanced strategies can override this to submit complex orders
    /// (limit orders, bracket orders, etc.) instead of using signals.
    ///
    /// Only called if `uses_order_management()` returns true.
    ///
    /// # Example
    /// ```text
    /// fn get_orders(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<Order> {
    ///     let mut orders = Vec::new();
    ///
    ///     if should_enter_long(bars) {
    ///         // Submit a limit order below current price
    ///         let entry_price = bars.close[0] * Decimal::new(99, 2);
    ///         orders.push(
    ///             Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.1), entry_price)
    ///                 .build()
    ///                 .unwrap()
    ///         );
    ///     }
    ///
    ///     orders
    /// }
    /// ```
    #[allow(unused_variables)]
    fn get_orders(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<Order> {
        Vec::new() // Default: no orders
    }

    /// Get orders to cancel for this bar.
    ///
    /// Advanced strategies can override this to cancel pending orders
    /// based on market conditions.
    ///
    /// Only called if `uses_order_management()` returns true.
    #[allow(unused_variables)]
    fn get_cancellations(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<ClientOrderId> {
        Vec::new() // Default: no cancellations
    }

    // ========================================================================
    // Event-Driven Handlers (NinjaTrader-style)
    // ========================================================================
    //
    // ORDERING GUARANTEE: For fill events that affect positions:
    //   on_order_update() → on_execution() → on_position_update()
    //
    // This ordering ensures strategies can properly track state as orders are filled.
    // Use StrategyEventDispatcher to maintain this guarantee.

    /// Called on ANY order state change.
    ///
    /// **ORDERING**: ALWAYS fires BEFORE `on_execution()` and `on_position_update()`
    /// for fill events that affect positions.
    ///
    /// This is the primary handler for tracking order lifecycle events including:
    /// - Submitted, Accepted, Rejected
    /// - Pending modifications and cancellations
    /// - Filled (before execution details)
    /// - Canceled, Expired
    ///
    /// # Example
    /// ```text
    /// fn on_order_update(&mut self, event: &OrderEventAny) {
    ///     match event {
    ///         OrderEventAny::Accepted(e) => {
    ///             println!("Order {} accepted", e.client_order_id);
    ///         }
    ///         OrderEventAny::Rejected(e) => {
    ///             println!("Order {} rejected: {}", e.client_order_id, e.reason);
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_order_update(&mut self, event: &OrderEventAny) {
        // Default: no-op
    }

    /// Called on order fills (partial or complete).
    ///
    /// **ORDERING**: Fires AFTER `on_order_update()`, BEFORE `on_position_update()`.
    ///
    /// Use this for detailed fill processing:
    /// - Track execution prices and quantities
    /// - Calculate slippage
    /// - Update fill statistics
    ///
    /// # Example
    /// ```text
    /// fn on_execution(&mut self, event: &OrderFilled) {
    ///     println!("Filled {} @ {} ({})",
    ///         event.last_qty, event.last_px,
    ///         if event.leaves_qty.is_zero() { "complete" } else { "partial" }
    ///     );
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_execution(&mut self, event: &OrderFilled) {
        // Default: no-op
    }

    /// Called on position changes.
    ///
    /// **ORDERING**: ALWAYS fires AFTER `on_order_update()` and `on_execution()`
    /// for fill events.
    ///
    /// Position events notify strategies of changes including:
    /// - Position opened (new position from flat)
    /// - Position increased (added to existing)
    /// - Position decreased (reduced existing)
    /// - Position closed (returned to flat)
    /// - Mark-to-market updates (unrealized P&L changes)
    ///
    /// # Example
    /// ```text
    /// fn on_position_update(&mut self, event: &PositionEvent) {
    ///     match event.side {
    ///         PositionSide::Long => {
    ///             println!("Long {} @ {} | PnL: {}",
    ///                 event.quantity, event.avg_entry_price, event.unrealized_pnl);
    ///         }
    ///         PositionSide::Flat => {
    ///             println!("Position closed. Realized P&L: {}", event.realized_pnl);
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_position_update(&mut self, event: &PositionEvent) {
        // Default: no-op
    }

    /// Called on account balance/state changes.
    ///
    /// Account events include:
    /// - Balance updates (deposits, withdrawals, P&L settlements)
    /// - State changes (active, suspended, etc.)
    /// - Margin updates
    ///
    /// # Example
    /// ```text
    /// fn on_account_update(&mut self, event: &AccountEvent) {
    ///     match event {
    ///         AccountEvent::BalanceUpdated { account_id, currency, balance, .. } => {
    ///             println!("Account {} balance: {} {}", account_id, balance, currency);
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_account_update(&mut self, event: &AccountEvent) {
        // Default: no-op
    }

    /// Called on market data updates.
    ///
    /// Market data events include:
    /// - Last trade (tick data)
    /// - Quote updates (L1 BBO)
    /// - Order book snapshots/deltas (L2)
    /// - Mark price, index price, funding rate
    ///
    /// # Example
    /// ```text
    /// fn on_marketdata_update(&mut self, event: &MarketDataEvent) {
    ///     match event.data_type {
    ///         MarketDataType::Last => {
    ///             if let Some(tick) = &event.tick {
    ///                 println!("Trade: {} @ {}", tick.quantity, tick.price);
    ///             }
    ///         }
    ///         MarketDataType::Quote => {
    ///             if let Some(quote) = &event.quote {
    ///                 println!("BBO: {} / {}", quote.bid_price, quote.ask_price);
    ///             }
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_marketdata_update(&mut self, event: &MarketDataEvent) {
        // Default: no-op
    }

    /// Called on strategy state transitions.
    ///
    /// State events track the strategy lifecycle:
    /// - Historical → Realtime (backtest warmup complete, going live)
    /// - Realtime → Terminated (shutdown)
    /// - Any → Faulted (fatal error)
    ///
    /// # Example
    /// ```text
    /// fn on_state_change(&mut self, event: &StrategyStateEvent) {
    ///     if event.new_state == StrategyState::Realtime {
    ///         println!("Going live!");
    ///         self.is_live = true;
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_state_change(&mut self, event: &StrategyStateEvent) {
        // Default: no-op
    }

    /// Called on trading session changes.
    ///
    /// Session events include:
    /// - Session open/close
    /// - Market halts and resumes
    /// - Maintenance windows
    ///
    /// # Example
    /// ```text
    /// fn on_session_update(&mut self, event: &SessionEvent) {
    ///     match event {
    ///         SessionEvent::MarketHalted { instrument_id, reason, .. } => {
    ///             println!("Market halted for {}: {}", instrument_id.symbol, reason);
    ///             self.halt_trading = true;
    ///         }
    ///         SessionEvent::MarketResumed { .. } => {
    ///             self.halt_trading = false;
    ///         }
    ///         _ => {}
    ///     }
    /// }
    /// ```
    #[allow(unused_variables)]
    fn on_session_update(&mut self, event: &SessionEvent) {
        // Default: no-op
    }
}
