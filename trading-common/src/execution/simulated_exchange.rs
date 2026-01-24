//! Simulated exchange for realistic backtesting.
//!
//! The `SimulatedExchange` integrates latency modeling, order matching, and fill
//! simulation to provide a realistic exchange simulation for backtesting.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────────────────┐
//! │                          SimulatedExchange                                 │
//! │                                                                            │
//! │  ┌──────────────────┐                                                      │
//! │  │   InflightQueue  │  Orders submitted with latency delay                 │
//! │  │   (TradingCmd)   │◄────────────────────────────────────┐               │
//! │  └────────┬─────────┘                                     │               │
//! │           │                                               │               │
//! │           │ advance_time() / process_inflight_commands()  │               │
//! │           ▼                                               │               │
//! │  ┌──────────────────────────────────────────────────┐     │               │
//! │  │            Matching Engines                       │     │               │
//! │  │  HashMap<InstrumentId, OrderMatchingEngine>       │     │               │
//! │  │                                                   │     │               │
//! │  │  • process_bar()     ──────▶  Vec<OrderFilled>   │     │               │
//! │  │  • process_tick()    ──────▶  Vec<OrderFilled>   │     │               │
//! │  └──────────────────────────────────────────────────┘     │               │
//! │           │                                               │               │
//! │           │                                               │               │
//! │           ▼                                               │               │
//! │  ┌────────────────────────────────────────────────────────┴──────────┐    │
//! │  │                         Strategy                                   │    │
//! │  │                                                                    │    │
//! │  │  submit_order() ──────────────────────────────────────────────────┘    │
//! │  │  cancel_order()                                                        │
//! │  └────────────────────────────────────────────────────────────────────────┘
//! └───────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Synchronization
//!
//! The exchange maintains `current_time_ns` which is advanced by the backtest engine.
//! Orders submitted by the strategy are queued with latency and only processed
//! when time advances past their arrival time.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use super::inflight_queue::{InflightQueue, TradingCommand};
use super::latency_model::{LatencyModel, NoLatencyModel};
use super::matching::{MatchResult, MatchingEngineConfig, OrderMatchingEngine};
use crate::data::types::OHLCData;
use crate::orders::{
    ClientOrderId, EventId, InstrumentId, Order, OrderAccepted, OrderCanceled, OrderEventAny,
    OrderFilled, OrderRejected, VenueOrderId,
};
use crate::risk::{FeeModel, ZeroFeeModel};

/// Configuration for the simulated exchange.
#[derive(Debug, Clone)]
pub struct SimulatedExchangeConfig {
    /// Default venue name
    pub venue: String,
    /// Use OHLC bars for matching
    pub bar_execution: bool,
    /// Use trade ticks for matching
    pub trade_execution: bool,
    /// Use L2 order book for matching
    pub book_execution: bool,
    /// Track liquidity consumption
    pub liquidity_consumption: bool,
    /// Reject stop orders
    pub reject_stop_orders: bool,
    /// Support Good-Till-Date orders
    pub support_gtd_orders: bool,
    /// Support contingent orders (OTO/OCO/OUO)
    pub support_contingent_orders: bool,
    /// Matching engine config
    pub matching_config: MatchingEngineConfig,
}

impl Default for SimulatedExchangeConfig {
    fn default() -> Self {
        Self {
            venue: "SIMULATED".to_string(),
            bar_execution: true,
            trade_execution: true,
            book_execution: false,
            liquidity_consumption: false,
            reject_stop_orders: false,
            support_gtd_orders: true,
            support_contingent_orders: true,
            matching_config: MatchingEngineConfig::default(),
        }
    }
}

/// Stored order info for creating fill events.
///
/// Tracks cumulative fill information for proper partial fill handling.
#[derive(Debug, Clone)]
struct StoredOrder {
    /// The order (mutated as fills occur)
    order: Order,
    /// Venue-assigned order ID
    venue_order_id: VenueOrderId,
    /// Cumulative quantity filled across all fills
    cum_qty: Decimal,
}

/// Simulated exchange for backtesting.
///
/// Provides realistic exchange simulation with:
/// - Latency modeling (orders don't fill instantly)
/// - Per-instrument matching engines
/// - Pluggable fill and fee models
#[derive(Debug)]
pub struct SimulatedExchange {
    /// Venue name
    pub venue: String,
    /// Per-instrument matching engines
    matching_engines: HashMap<InstrumentId, OrderMatchingEngine>,
    /// Fee model for calculating commissions
    fee_model: Box<dyn FeeModel>,
    /// Latency model for order delays
    latency_model: Box<dyn LatencyModel>,
    /// Queue for latency-delayed commands
    inflight_queue: InflightQueue,
    /// Current simulated time in nanoseconds
    current_time_ns: u64,
    /// Configuration
    config: SimulatedExchangeConfig,
    /// Order ID to stored order mapping (for fills and cancels)
    stored_orders: HashMap<ClientOrderId, StoredOrder>,
}

impl SimulatedExchange {
    /// Create a new simulated exchange with default configuration.
    pub fn new(venue: &str) -> Self {
        Self::with_config(SimulatedExchangeConfig {
            venue: venue.to_string(),
            ..Default::default()
        })
    }

    /// Create a new simulated exchange with custom configuration.
    pub fn with_config(config: SimulatedExchangeConfig) -> Self {
        Self {
            venue: config.venue.clone(),
            matching_engines: HashMap::new(),
            fee_model: Box::new(ZeroFeeModel),
            latency_model: Box::new(NoLatencyModel),
            inflight_queue: InflightQueue::new(),
            current_time_ns: 0,
            config,
            stored_orders: HashMap::new(),
        }
    }

    /// Set the fee model.
    pub fn with_fee_model(mut self, model: Box<dyn FeeModel>) -> Self {
        self.fee_model = model;
        self
    }

    /// Set the latency model.
    pub fn with_latency_model(mut self, model: Box<dyn LatencyModel>) -> Self {
        self.latency_model = model;
        self
    }

    // === TIME MANAGEMENT ===

    /// Advance simulated time to match incoming market data.
    ///
    /// This should be called by the backtest engine BEFORE processing data.
    pub fn advance_time(&mut self, new_time_ns: u64) {
        self.current_time_ns = new_time_ns;
    }

    /// Get current simulated time.
    pub fn current_time_ns(&self) -> u64 {
        self.current_time_ns
    }

    /// Process commands that have "arrived" after latency delay.
    ///
    /// A command with arrival_time <= current_time_ns is considered ready.
    /// Returns events for each processed command.
    pub fn process_inflight_commands(&mut self) -> Vec<OrderEventAny> {
        let ready_commands = self.inflight_queue.pop_ready(self.current_time_ns);
        let mut events = Vec::new();

        for cmd in ready_commands {
            match cmd {
                TradingCommand::SubmitOrder(order) => {
                    events.extend(self.process_submit(order));
                }
                TradingCommand::CancelOrder(client_order_id) => {
                    events.extend(self.process_cancel(&client_order_id));
                }
                TradingCommand::ModifyOrder {
                    client_order_id,
                    new_price,
                    new_quantity,
                } => {
                    events.extend(self.process_modify(&client_order_id, new_price, new_quantity));
                }
            }
        }

        events
    }

    // === ORDER OPERATIONS ===

    /// Submit an order - queued with insert latency.
    ///
    /// The order enters the inflight queue and won't be processed until
    /// `advance_time()` moves past the arrival time.
    pub fn submit_order(&mut self, order: Order) {
        let latency = self.latency_model.insert_latency_nanos();
        let arrival_time = self.current_time_ns + latency;
        self.inflight_queue
            .push(TradingCommand::SubmitOrder(order), arrival_time);
    }

    /// Cancel an order - queued with delete latency.
    pub fn cancel_order(&mut self, client_order_id: ClientOrderId) {
        let latency = self.latency_model.delete_latency_nanos();
        let arrival_time = self.current_time_ns + latency;
        self.inflight_queue
            .push(TradingCommand::CancelOrder(client_order_id), arrival_time);
    }

    /// Modify an order - queued with update latency.
    pub fn modify_order(
        &mut self,
        client_order_id: ClientOrderId,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) {
        let latency = self.latency_model.update_latency_nanos();
        let arrival_time = self.current_time_ns + latency;
        self.inflight_queue.push(
            TradingCommand::ModifyOrder {
                client_order_id,
                new_price,
                new_quantity,
            },
            arrival_time,
        );
    }

    // === MARKET DATA PROCESSING ===

    /// Process an OHLC bar.
    ///
    /// Returns fill events for any orders that matched.
    pub fn process_bar(&mut self, bar: &OHLCData) -> Vec<OrderEventAny> {
        let instrument_id = InstrumentId::new(&bar.symbol, &self.venue);

        // Get or create matching engine for this instrument
        let engine = self.get_or_create_engine(&instrument_id);

        // Process bar through matching engine
        let matches = engine.process_bar(bar);

        // Convert to events
        self.process_matches(matches)
    }

    /// Process a trade tick.
    pub fn process_trade_tick(
        &mut self,
        instrument_id: &InstrumentId,
        price: Decimal,
        quantity: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<OrderEventAny> {
        let engine = self.get_or_create_engine(instrument_id);
        let matches = engine.process_trade_tick(price, quantity, timestamp);
        self.process_matches(matches)
    }

    /// Process a quote tick.
    pub fn process_quote_tick(
        &mut self,
        instrument_id: &InstrumentId,
        bid: Decimal,
        ask: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<OrderEventAny> {
        let engine = self.get_or_create_engine(instrument_id);
        let matches = engine.process_quote_tick(bid, ask, timestamp);
        self.process_matches(matches)
    }

    // === INTERNAL PROCESSING ===

    /// Process a submit order command.
    ///
    /// Properly transitions order state:
    /// Initialized → Submitted → Accepted (or Rejected)
    fn process_submit(&mut self, order: Order) -> Vec<OrderEventAny> {
        use crate::orders::OrderStatus;
        let mut events = Vec::new();

        // Create mutable order for state transitions
        let mut stored_order = order.clone();

        // Transition: Initialized → Submitted
        if let Err(e) = stored_order.transition_to(OrderStatus::Submitted) {
            // Order in invalid state for submission
            events.push(OrderEventAny::Rejected(OrderRejected {
                event_id: EventId(uuid::Uuid::new_v4()),
                client_order_id: order.client_order_id.clone(),
                account_id: order.account_id.clone(),
                reason: format!("Invalid order state for submission: {}", e),
                ts_event: self.current_datetime(),
                ts_init: Utc::now(),
            }));
            return events;
        }

        // Validate order
        if self.config.reject_stop_orders && order.is_stop_order() {
            // Transition to Rejected state
            let _ = stored_order.transition_to(OrderStatus::Rejected);
            events.push(OrderEventAny::Rejected(OrderRejected {
                event_id: EventId(uuid::Uuid::new_v4()),
                client_order_id: order.client_order_id.clone(),
                account_id: order.account_id.clone(),
                reason: "Stop orders not supported by this venue".to_string(),
                ts_event: self.current_datetime(),
                ts_init: Utc::now(),
            }));
            return events;
        }

        // Generate venue order ID
        let venue_order_id = VenueOrderId(format!("SIM-{}", uuid::Uuid::new_v4()));

        // Set venue order ID and transition to Accepted
        stored_order.venue_order_id = Some(venue_order_id.clone());
        if let Err(e) = stored_order.transition_to(OrderStatus::Accepted) {
            events.push(OrderEventAny::Rejected(OrderRejected {
                event_id: EventId(uuid::Uuid::new_v4()),
                client_order_id: order.client_order_id.clone(),
                account_id: order.account_id.clone(),
                reason: format!("Failed to accept order: {}", e),
                ts_event: self.current_datetime(),
                ts_init: Utc::now(),
            }));
            return events;
        }

        // Store order for later fill event creation
        self.stored_orders.insert(
            order.client_order_id.clone(),
            StoredOrder {
                order: stored_order.clone(),
                venue_order_id: venue_order_id.clone(),
                cum_qty: Decimal::ZERO,
            },
        );

        // Generate accepted event
        events.push(OrderEventAny::Accepted(OrderAccepted {
            event_id: EventId(uuid::Uuid::new_v4()),
            client_order_id: order.client_order_id.clone(),
            venue_order_id: venue_order_id,
            account_id: order.account_id.clone(),
            ts_event: self.current_datetime(),
            ts_init: Utc::now(),
        }));

        // Add to matching engine (pass the state-transitioned order)
        let engine = self.get_or_create_engine(&stored_order.instrument_id);
        engine.add_order(stored_order);

        events
    }

    /// Process a cancel order command.
    ///
    /// Properly transitions order state to Canceled if cancelable.
    fn process_cancel(&mut self, client_order_id: &ClientOrderId) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        // Find the stored order (get mutable reference)
        if let Some(stored) = self.stored_orders.get_mut(client_order_id) {
            // Check if order is cancelable
            if !stored.order.is_cancelable() {
                // Cannot cancel - order in non-cancelable state
                events.push(OrderEventAny::CancelRejected(
                    crate::orders::OrderCancelRejected {
                        event_id: EventId(uuid::Uuid::new_v4()),
                        client_order_id: client_order_id.clone(),
                        venue_order_id: Some(stored.venue_order_id.clone()),
                        account_id: stored.order.account_id.clone(),
                        reason: format!(
                            "Order cannot be canceled in status: {:?}",
                            stored.order.status
                        ),
                        ts_event: self.current_datetime(),
                        ts_init: Utc::now(),
                    },
                ));
                return events;
            }

            // Transition order to Canceled state
            if let Err(e) = stored.order.cancel() {
                events.push(OrderEventAny::CancelRejected(
                    crate::orders::OrderCancelRejected {
                        event_id: EventId(uuid::Uuid::new_v4()),
                        client_order_id: client_order_id.clone(),
                        venue_order_id: Some(stored.venue_order_id.clone()),
                        account_id: stored.order.account_id.clone(),
                        reason: format!("Failed to cancel order: {}", e),
                        ts_event: self.current_datetime(),
                        ts_init: Utc::now(),
                    },
                ));
                return events;
            }

            // Get values for event before removing
            let venue_order_id = stored.venue_order_id.clone();
            let account_id = stored.order.account_id.clone();
            let instrument_id = stored.order.instrument_id.clone();

            // Remove from matching engine
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.cancel_order(client_order_id);
            }

            // Generate canceled event
            events.push(OrderEventAny::Canceled(OrderCanceled {
                event_id: EventId(uuid::Uuid::new_v4()),
                client_order_id: client_order_id.clone(),
                venue_order_id: Some(venue_order_id),
                account_id,
                ts_event: self.current_datetime(),
                ts_init: Utc::now(),
            }));
        }

        // Remove from stored orders after processing
        if !events.is_empty() {
            if let Some(OrderEventAny::Canceled(_)) = events.first() {
                self.stored_orders.remove(client_order_id);
            }
        }

        events
    }

    /// Process a modify order command.
    fn process_modify(
        &mut self,
        _client_order_id: &ClientOrderId,
        _new_price: Option<Decimal>,
        _new_quantity: Option<Decimal>,
    ) -> Vec<OrderEventAny> {
        // TODO: Implement order modification
        // For now, modifications are not supported
        Vec::new()
    }

    /// Get or create a matching engine for an instrument.
    fn get_or_create_engine(&mut self, instrument_id: &InstrumentId) -> &mut OrderMatchingEngine {
        self.matching_engines
            .entry(instrument_id.clone())
            .or_insert_with(|| {
                OrderMatchingEngine::with_config(
                    instrument_id.clone(),
                    self.config.matching_config.clone(),
                )
            })
    }

    /// Convert match results to OrderFilled events.
    ///
    /// Properly handles partial fills by:
    /// 1. Tracking cumulative quantity across fills
    /// 2. Updating the stored order via apply_fill()
    /// 3. Only removing the order when fully filled
    fn process_matches(&mut self, matches: Vec<MatchResult>) -> Vec<OrderEventAny> {
        let mut events = Vec::new();
        let mut orders_to_remove = Vec::new();

        for m in matches {
            if !m.filled {
                continue;
            }

            // Get stored order info (mutable)
            if let Some(stored) = self.stored_orders.get_mut(&m.client_order_id) {
                // Update cumulative quantity
                stored.cum_qty += m.fill_qty;
                let cum_qty = stored.cum_qty;

                // Calculate leaves_qty based on total order quantity
                let order_qty = stored.order.quantity;
                let leaves_qty = (order_qty - cum_qty).max(Decimal::ZERO);

                // Set venue order ID on the order
                stored.order.venue_order_id = Some(stored.venue_order_id.clone());

                // Calculate commission
                let commission = self.fee_model.calculate_fee(
                    m.fill_qty,
                    m.fill_price,
                    &stored.order,
                    m.liquidity_side,
                );

                // Apply fill to order to update its internal state
                // This updates: filled_qty, leaves_qty, avg_px, status
                let _ = stored.order.apply_fill(
                    m.fill_qty,
                    m.fill_price,
                    m.trade_id.clone(),
                    commission,
                    m.liquidity_side,
                );

                // Create OrderFilled event with correct cumulative quantities
                let fill = OrderFilled {
                    event_id: EventId(uuid::Uuid::new_v4()),
                    client_order_id: m.client_order_id.clone(),
                    venue_order_id: stored.venue_order_id.clone(),
                    account_id: stored.order.account_id.clone(),
                    instrument_id: stored.order.instrument_id.clone(),
                    trade_id: m.trade_id,
                    position_id: stored.order.position_id.clone(),
                    strategy_id: stored.order.strategy_id.clone(),
                    order_side: stored.order.side,
                    order_type: stored.order.order_type,
                    last_qty: m.fill_qty,
                    last_px: m.fill_price,
                    cum_qty,
                    leaves_qty,
                    currency: "USD".to_string(), // Default currency
                    commission,
                    commission_currency: "USD".to_string(),
                    liquidity_side: m.liquidity_side,
                    ts_event: m.fill_time,
                    ts_init: Utc::now(),
                };

                events.push(OrderEventAny::Filled(fill));

                // Mark for removal if fully filled (leaves_qty == 0)
                if leaves_qty.is_zero() {
                    orders_to_remove.push(m.client_order_id.clone());
                }
            }
        }

        // Remove fully filled orders
        for order_id in orders_to_remove {
            self.stored_orders.remove(&order_id);
        }

        events
    }

    /// Get current time as DateTime.
    fn current_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(self.current_time_ns as i64)
    }

    // === QUERIES ===

    /// Get total open order count across all instruments.
    pub fn open_order_count(&self) -> usize {
        self.matching_engines
            .values()
            .map(|e| e.open_order_count())
            .sum()
    }

    /// Get pending (inflight) command count.
    pub fn pending_command_count(&self) -> usize {
        self.inflight_queue.len()
    }

    /// Check if an order exists (either pending or in matching engine).
    pub fn has_order(&self, client_order_id: &ClientOrderId) -> bool {
        // Check inflight queue
        if !self.inflight_queue.get_pending_for_order(client_order_id).is_empty() {
            return true;
        }

        // Check stored orders (in matching engines)
        self.stored_orders.contains_key(client_order_id)
    }

    /// Get a stored order by client order ID.
    ///
    /// Returns a clone of the order with its current state (including fill info).
    /// Returns None if the order has been fully filled or doesn't exist.
    pub fn get_order(&self, client_order_id: &ClientOrderId) -> Option<Order> {
        self.stored_orders.get(client_order_id).map(|s| s.order.clone())
    }

    /// Get order fill information (cumulative quantity filled).
    pub fn get_order_fill_info(&self, client_order_id: &ClientOrderId) -> Option<(Decimal, Decimal)> {
        self.stored_orders.get(client_order_id).map(|s| {
            let cum_qty = s.cum_qty;
            let leaves_qty = (s.order.quantity - cum_qty).max(Decimal::ZERO);
            (cum_qty, leaves_qty)
        })
    }

    /// Get the matching engine for an instrument (if exists).
    pub fn get_engine(&self, instrument_id: &InstrumentId) -> Option<&OrderMatchingEngine> {
        self.matching_engines.get(instrument_id)
    }

    /// Clear all orders and reset state.
    pub fn reset(&mut self) {
        self.matching_engines.clear();
        self.inflight_queue.clear();
        self.stored_orders.clear();
        self.current_time_ns = 0;
        self.latency_model.reset();
    }
}

impl Order {
    /// Check if this is a stop order type.
    fn is_stop_order(&self) -> bool {
        matches!(
            self.order_type,
            crate::orders::OrderType::Stop
                | crate::orders::OrderType::StopLimit
                | crate::orders::OrderType::TrailingStop
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::FixedLatencyModel;
    use crate::orders::OrderSide;
    use rust_decimal_macros::dec;

    const TEST_VENUE: &str = "TEST";

    fn create_bar(symbol: &str, close: Decimal) -> OHLCData {
        use crate::data::types::Timeframe;
        OHLCData {
            timestamp: Utc::now(),
            symbol: symbol.to_string(),
            timeframe: Timeframe::OneMinute,
            open: close - dec!(100),
            high: close + dec!(500),
            low: close - dec!(500),
            close,
            volume: dec!(1000),
            trade_count: 100,
        }
    }

    fn create_limit_order(symbol: &str, side: OrderSide, price: Decimal) -> Order {
        Order::limit(symbol, side, dec!(1.0), price)
            .with_venue(TEST_VENUE)
            .build()
            .unwrap()
    }

    fn create_market_order(symbol: &str, side: OrderSide) -> Order {
        Order::market(symbol, side, dec!(1.0))
            .with_venue(TEST_VENUE)
            .build()
            .unwrap()
    }

    fn create_stop_order(symbol: &str, side: OrderSide, trigger: Decimal) -> Order {
        Order::stop(symbol, side, dec!(1.0), trigger)
            .with_venue(TEST_VENUE)
            .build()
            .unwrap()
    }

    #[test]
    fn test_new_exchange() {
        let exchange = SimulatedExchange::new(TEST_VENUE);
        assert_eq!(exchange.venue, TEST_VENUE);
        assert_eq!(exchange.open_order_count(), 0);
        assert_eq!(exchange.pending_command_count(), 0);
    }

    #[test]
    fn test_submit_order_no_latency() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE);

        let order = create_market_order("BTCUSDT", OrderSide::Buy);

        exchange.submit_order(order);

        // With no latency, order should be ready immediately
        let events = exchange.process_inflight_commands();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert_eq!(exchange.open_order_count(), 1);
    }

    #[test]
    fn test_submit_order_with_latency() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE)
            .with_latency_model(Box::new(FixedLatencyModel::new(50_000_000))); // 50ms

        let order = create_market_order("BTCUSDT", OrderSide::Buy);

        exchange.submit_order(order);

        // Before latency passes, order not ready
        exchange.advance_time(30_000_000); // 30ms
        let events = exchange.process_inflight_commands();
        assert!(events.is_empty());
        assert_eq!(exchange.open_order_count(), 0);

        // After latency passes, order ready
        exchange.advance_time(50_000_000); // 50ms
        let events = exchange.process_inflight_commands();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
        assert_eq!(exchange.open_order_count(), 1);
    }

    #[test]
    fn test_order_fills_on_bar() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE);

        let order = create_limit_order("BTCUSDT", OrderSide::Buy, dec!(50000));

        exchange.submit_order(order);
        exchange.process_inflight_commands();

        // Process bar with low below limit
        let bar = create_bar("BTCUSDT", dec!(50500));
        let events = exchange.process_bar(&bar);

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Filled(_)));
        assert_eq!(exchange.open_order_count(), 0);
    }

    #[test]
    fn test_cancel_order() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE);

        let order = create_limit_order("BTCUSDT", OrderSide::Buy, dec!(48000));
        let id = order.client_order_id.clone();

        exchange.submit_order(order);
        exchange.process_inflight_commands();
        assert_eq!(exchange.open_order_count(), 1);

        exchange.cancel_order(id);
        let events = exchange.process_inflight_commands();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Canceled(_)));
        assert_eq!(exchange.open_order_count(), 0);
    }

    #[test]
    fn test_reject_stop_orders() {
        let config = SimulatedExchangeConfig {
            reject_stop_orders: true,
            ..Default::default()
        };
        let mut exchange = SimulatedExchange::with_config(config);

        let order = create_stop_order("BTCUSDT", OrderSide::Buy, dec!(52000));

        exchange.submit_order(order);
        let events = exchange.process_inflight_commands();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Rejected(_)));
        assert_eq!(exchange.open_order_count(), 0);
    }

    #[test]
    fn test_multiple_instruments() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE);

        // Submit orders for different instruments
        exchange.submit_order(create_limit_order("BTCUSDT", OrderSide::Buy, dec!(50000)));
        exchange.submit_order(
            Order::limit("ETHUSDT", OrderSide::Buy, dec!(10.0), dec!(3000))
                .with_venue(TEST_VENUE)
                .build()
                .unwrap(),
        );

        exchange.process_inflight_commands();
        assert_eq!(exchange.open_order_count(), 2);
        assert_eq!(exchange.matching_engines.len(), 2);

        // Fill only BTC
        let btc_bar = create_bar("BTCUSDT", dec!(50500));
        let events = exchange.process_bar(&btc_bar);
        assert_eq!(events.len(), 1);
        assert_eq!(exchange.open_order_count(), 1); // ETH order still open
    }

    #[test]
    fn test_has_order() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE);

        let order = create_limit_order("BTCUSDT", OrderSide::Buy, dec!(50000));
        let id = order.client_order_id.clone();

        assert!(!exchange.has_order(&id));

        exchange.submit_order(order);
        assert!(exchange.has_order(&id)); // In inflight queue

        exchange.process_inflight_commands();
        assert!(exchange.has_order(&id)); // In matching engine
    }

    #[test]
    fn test_reset() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE);

        exchange.submit_order(create_limit_order("BTCUSDT", OrderSide::Buy, dec!(50000)));
        exchange.process_inflight_commands();
        exchange.advance_time(1_000_000_000);

        assert_eq!(exchange.open_order_count(), 1);
        assert!(exchange.current_time_ns() > 0);

        exchange.reset();

        assert_eq!(exchange.open_order_count(), 0);
        assert_eq!(exchange.pending_command_count(), 0);
        assert_eq!(exchange.current_time_ns(), 0);
    }

    #[test]
    fn test_latency_prevents_same_bar_fill() {
        let mut exchange = SimulatedExchange::new(TEST_VENUE)
            .with_latency_model(Box::new(FixedLatencyModel::new(100_000_000))); // 100ms

        // Start at time 0
        exchange.advance_time(0);

        // Submit order
        let order = create_limit_order("BTCUSDT", OrderSide::Buy, dec!(50000));
        exchange.submit_order(order);

        // Process commands at time 0 - order not ready yet
        let events = exchange.process_inflight_commands();
        assert!(events.is_empty());

        // Bar arrives at time 50ms - order still not ready
        exchange.advance_time(50_000_000);
        exchange.process_inflight_commands();
        let bar1 = create_bar("BTCUSDT", dec!(49000)); // Would fill
        let fills = exchange.process_bar(&bar1);
        assert!(fills.is_empty()); // No fill - order not accepted yet

        // Time advances to 100ms - order now ready
        exchange.advance_time(100_000_000);
        let events = exchange.process_inflight_commands();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));

        // Next bar arrives and fills
        exchange.advance_time(150_000_000);
        let bar2 = create_bar("BTCUSDT", dec!(49000));
        let fills = exchange.process_bar(&bar2);
        assert_eq!(fills.len(), 1);
    }
}
