//! Strategy event dispatcher with ordering guarantees.
//!
//! This module provides a dispatcher that ensures correct event ordering
//! when notifying strategies of order lifecycle events. The critical guarantee is:
//!
//! ```text
//! on_order_update() → on_execution() → on_position_update()
//! ```
//!
//! This ordering ensures strategies can properly track state as orders are filled.

use crate::accounts::AccountEvent;
use crate::data::events::MarketDataEvent;
use crate::instruments::SessionEvent;
use crate::orders::{OrderEventAny, OrderFilled};

use super::base::Strategy;
use super::position::PositionEvent;
use super::state::StrategyStateEvent;

/// Event dispatcher that maintains ordering guarantees when dispatching events to strategies.
///
/// The dispatcher wraps a strategy reference and provides methods for dispatching
/// various event types while ensuring the correct ordering of related events.
///
/// # Ordering Guarantee
///
/// For fill events that also affect positions, the dispatcher guarantees:
/// 1. `on_order_update()` is called first with the order event
/// 2. `on_execution()` is called second with the fill details
/// 3. `on_position_update()` is called last with the position change
///
/// This allows strategies to see the order state before processing the fill,
/// and see the fill before processing the resulting position change.
///
/// # Example
///
/// ```ignore
/// use trading_common::backtest::strategy::dispatcher::StrategyEventDispatcher;
///
/// let mut dispatcher = StrategyEventDispatcher::new(&mut strategy);
///
/// // Single order event
/// dispatcher.dispatch_order_event(&order_event);
///
/// // Fill event with position update - guarantees ordering
/// dispatcher.dispatch_fill_with_position(&fill_event, &position_event);
/// ```
pub struct StrategyEventDispatcher<'a, S: Strategy + ?Sized> {
    strategy: &'a mut S,
}

impl<'a, S: Strategy + ?Sized> StrategyEventDispatcher<'a, S> {
    /// Create a new dispatcher for the given strategy
    pub fn new(strategy: &'a mut S) -> Self {
        Self { strategy }
    }

    /// Dispatch an order event to the strategy.
    ///
    /// Calls `on_order_update()` for any order state change.
    pub fn dispatch_order_event(&mut self, event: &OrderEventAny) {
        self.strategy.on_order_update(event);
    }

    /// Dispatch an execution (fill) event to the strategy.
    ///
    /// Calls `on_order_update()` first with the fill as an order event,
    /// then calls `on_execution()` with the fill details.
    ///
    /// For backward compatibility, also calls `on_order_filled()`.
    pub fn dispatch_execution(&mut self, event: &OrderFilled) {
        // First: order update
        let order_event = OrderEventAny::Filled(event.clone());
        self.strategy.on_order_update(&order_event);

        // Second: execution event
        self.strategy.on_execution(event);

        // Backward compatibility: legacy fill handler
        self.strategy.on_order_filled(event);
    }

    /// Dispatch a fill event with associated position update.
    ///
    /// This method guarantees the ordering:
    /// 1. `on_order_update()` - order state change
    /// 2. `on_execution()` - fill details
    /// 3. `on_position_update()` - resulting position change
    ///
    /// This is the primary method to use when a fill results in a position change.
    pub fn dispatch_fill_with_position(
        &mut self,
        fill_event: &OrderFilled,
        position_event: &PositionEvent,
    ) {
        // 1st: Order update
        let order_event = OrderEventAny::Filled(fill_event.clone());
        self.strategy.on_order_update(&order_event);

        // 2nd: Execution event
        self.strategy.on_execution(fill_event);

        // Legacy backward compatibility
        self.strategy.on_order_filled(fill_event);

        // 3rd: Position update (after fill is processed)
        self.strategy.on_position_update(position_event);
    }

    /// Dispatch a position update event.
    ///
    /// Use this for position updates that are not directly tied to a fill,
    /// such as position adjustments or mark-to-market updates.
    pub fn dispatch_position_update(&mut self, event: &PositionEvent) {
        self.strategy.on_position_update(event);
    }

    /// Dispatch an account update event.
    pub fn dispatch_account_update(&mut self, event: &AccountEvent) {
        self.strategy.on_account_update(event);
    }

    /// Dispatch a market data update event.
    pub fn dispatch_marketdata_update(&mut self, event: &MarketDataEvent) {
        self.strategy.on_marketdata_update(event);
    }

    /// Dispatch a state change event.
    pub fn dispatch_state_change(&mut self, event: &StrategyStateEvent) {
        self.strategy.on_state_change(event);
    }

    /// Dispatch a session update event.
    pub fn dispatch_session_update(&mut self, event: &SessionEvent) {
        self.strategy.on_session_update(event);
    }

    /// Get a reference to the underlying strategy
    pub fn strategy(&self) -> &S {
        self.strategy
    }

    /// Get a mutable reference to the underlying strategy
    pub fn strategy_mut(&mut self) -> &mut S {
        self.strategy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::{BarData, BarDataMode, BarType, Timeframe};
    use crate::orders::{
        AccountId, ClientOrderId, EventId, InstrumentId, LiquiditySide, OrderSide, OrderType,
        PositionId, PositionSide, StrategyId, TradeId, VenueOrderId,
    };
    use crate::series::bars_context::BarsContext;
    use crate::series::MaximumBarsLookBack;
    use chrono::Utc;
    use parking_lot::Mutex;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Test strategy that records all events it receives
    struct RecordingStrategy {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingStrategy {
        fn new(events: Arc<Mutex<Vec<String>>>) -> Self {
            Self { events }
        }
    }

    impl Strategy for RecordingStrategy {
        fn name(&self) -> &str {
            "RecordingStrategy"
        }

        fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) {
            // This strategy doesn't generate orders
        }

        fn bar_data_mode(&self) -> BarDataMode {
            BarDataMode::OnCloseBar
        }

        fn preferred_bar_type(&self) -> BarType {
            BarType::TimeBased(Timeframe::OneMinute)
        }

        fn max_bars_lookback(&self) -> MaximumBarsLookBack {
            MaximumBarsLookBack::Infinite
        }

        fn is_ready(&self, _bars: &BarsContext) -> bool {
            true
        }

        fn warmup_period(&self) -> usize {
            0
        }

        fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
            Ok(())
        }

        fn reset(&mut self) {}

        fn on_order_update(&mut self, _event: &OrderEventAny) {
            self.events.lock().push("on_order_update".to_string());
        }

        fn on_execution(&mut self, _event: &OrderFilled) {
            self.events.lock().push("on_execution".to_string());
        }

        fn on_position_update(&mut self, _event: &PositionEvent) {
            self.events.lock().push("on_position_update".to_string());
        }

        fn on_account_update(&mut self, _event: &AccountEvent) {
            self.events.lock().push("on_account_update".to_string());
        }

        fn on_marketdata_update(&mut self, _event: &MarketDataEvent) {
            self.events.lock().push("on_marketdata_update".to_string());
        }

        fn on_state_change(&mut self, _event: &StrategyStateEvent) {
            self.events.lock().push("on_state_change".to_string());
        }

        fn on_session_update(&mut self, _event: &SessionEvent) {
            self.events.lock().push("on_session_update".to_string());
        }
    }

    fn create_test_fill() -> OrderFilled {
        OrderFilled {
            event_id: EventId::new(),
            client_order_id: ClientOrderId::new("test-order-1"),
            venue_order_id: VenueOrderId::new("venue-1"),
            account_id: AccountId::new("account-1"),
            instrument_id: InstrumentId::new("BTCUSDT", "BINANCE"),
            trade_id: TradeId::new("trade-1"),
            position_id: None,
            strategy_id: StrategyId::new("test-strategy"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            last_qty: dec!(1.0),
            last_px: dec!(50000.0),
            cum_qty: dec!(1.0),
            leaves_qty: Decimal::ZERO,
            currency: "USDT".to_string(),
            commission: dec!(0.1),
            commission_currency: "USDT".to_string(),
            liquidity_side: LiquiditySide::Taker,
            ts_event: Utc::now(),
            ts_init: Utc::now(),
        }
    }

    fn create_test_position_event() -> PositionEvent {
        PositionEvent {
            event_id: EventId::new(),
            position_id: PositionId::new("pos-1"),
            account_id: AccountId::new("account-1"),
            instrument_id: InstrumentId::new("BTCUSDT", "BINANCE"),
            strategy_id: StrategyId::new("test-strategy"),
            side: PositionSide::Long,
            quantity: dec!(1.0),
            avg_entry_price: dec!(50000.0),
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            commission: dec!(0.1),
            mark_price: dec!(50000.0),
            ts_opened: Utc::now(),
            ts_last_fill: Some(Utc::now()),
            ts_event: Utc::now(),
        }
    }

    #[test]
    fn test_dispatch_fill_with_position_ordering() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut strategy = RecordingStrategy::new(events.clone());
        let mut dispatcher = StrategyEventDispatcher::new(&mut strategy);

        let fill = create_test_fill();
        let position = create_test_position_event();

        dispatcher.dispatch_fill_with_position(&fill, &position);

        let recorded = events.lock();

        // Verify ordering: on_order_update → on_execution → on_position_update
        assert_eq!(recorded.len(), 3);
        assert_eq!(recorded[0], "on_order_update");
        assert_eq!(recorded[1], "on_execution");
        assert_eq!(recorded[2], "on_position_update");
    }

    #[test]
    fn test_dispatch_execution_ordering() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut strategy = RecordingStrategy::new(events.clone());
        let mut dispatcher = StrategyEventDispatcher::new(&mut strategy);

        let fill = create_test_fill();
        dispatcher.dispatch_execution(&fill);

        let recorded = events.lock();

        // Verify ordering: on_order_update → on_execution
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0], "on_order_update");
        assert_eq!(recorded[1], "on_execution");
    }

    #[test]
    fn test_dispatch_order_event() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut strategy = RecordingStrategy::new(events.clone());
        let mut dispatcher = StrategyEventDispatcher::new(&mut strategy);

        let fill = create_test_fill();
        let order_event = OrderEventAny::Filled(fill);
        dispatcher.dispatch_order_event(&order_event);

        let recorded = events.lock();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], "on_order_update");
    }

    #[test]
    fn test_dispatch_position_update() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut strategy = RecordingStrategy::new(events.clone());
        let mut dispatcher = StrategyEventDispatcher::new(&mut strategy);

        let position = create_test_position_event();
        dispatcher.dispatch_position_update(&position);

        let recorded = events.lock();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], "on_position_update");
    }
}
