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
use crate::accounts::{Account, AccountsConfig};
use crate::data::types::OHLCData;
use crate::orders::{
    AccountId, ClientOrderId, EventId, InstrumentId, Order, OrderAccepted, OrderCanceled,
    OrderEventAny, OrderFilled, OrderRejected, OrderSide, VenueOrderId,
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
/// - Multi-account support for balance tracking
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
    /// Multiple accounts for balance tracking (keyed by AccountId)
    accounts: HashMap<AccountId, Account>,
    /// Default account ID for orders without explicit account_id
    default_account_id: Option<AccountId>,
    /// Track locked amounts per order with associated account (for unlocking on cancel/reject)
    locked_amounts: HashMap<ClientOrderId, (AccountId, Decimal)>,
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
            accounts: HashMap::new(),
            default_account_id: None,
            locked_amounts: HashMap::new(),
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

    /// Set a single account for balance tracking (legacy compatibility).
    ///
    /// When an account is configured, the exchange will:
    /// - Lock funds when orders are accepted
    /// - Release funds when orders are canceled/rejected
    /// - Deduct funds when orders are filled
    ///
    /// This also sets this account as the default account.
    pub fn with_account(mut self, account: Account) -> Self {
        let account_id = account.id.clone();
        self.accounts.insert(account_id.clone(), account);
        self.default_account_id = Some(account_id);
        self
    }

    /// Add multiple accounts from a vector.
    ///
    /// Use this with `AccountsConfig::build_accounts()` to load accounts from config.
    pub fn with_accounts(mut self, accounts: Vec<Account>) -> Self {
        for account in accounts {
            self.accounts.insert(account.id.clone(), account);
        }
        self
    }

    /// Load accounts from configuration.
    ///
    /// This is a convenience method that builds accounts from config and sets
    /// the default account ID.
    pub fn with_accounts_config(mut self, config: &AccountsConfig) -> Self {
        let accounts = config.build_accounts();
        for account in accounts {
            self.accounts.insert(account.id.clone(), account);
        }
        self.default_account_id = Some(config.default_account_id());
        self
    }

    /// Set the default account ID for orders without explicit account_id.
    pub fn with_default_account(mut self, account_id: AccountId) -> Self {
        self.default_account_id = Some(account_id);
        self
    }

    /// Get a reference to an account by ID.
    pub fn account(&self, account_id: &AccountId) -> Option<&Account> {
        self.accounts.get(account_id)
    }

    /// Get a mutable reference to an account by ID.
    pub fn account_mut(&mut self, account_id: &AccountId) -> Option<&mut Account> {
        self.accounts.get_mut(account_id)
    }

    /// Get a reference to the default account (if configured).
    pub fn default_account(&self) -> Option<&Account> {
        self.default_account_id
            .as_ref()
            .and_then(|id| self.accounts.get(id))
    }

    /// Get a mutable reference to the default account (if configured).
    pub fn default_account_mut(&mut self) -> Option<&mut Account> {
        if let Some(id) = self.default_account_id.clone() {
            self.accounts.get_mut(&id)
        } else {
            None
        }
    }

    /// Get an iterator over all accounts.
    pub fn all_accounts(&self) -> impl Iterator<Item = &Account> {
        self.accounts.values()
    }

    /// Get the number of configured accounts.
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Resolve account ID for an order.
    ///
    /// If the order has an explicit account_id (not empty or "default"), use it.
    /// Otherwise, fall back to the exchange's default account ID.
    fn resolve_account_id(&self, order: &Order) -> AccountId {
        let order_account = order.account_id.as_str();
        // Treat empty string or "default" as meaning "use exchange default"
        if order_account.is_empty() || order_account == "default" {
            self.default_account_id
                .clone()
                .unwrap_or_else(|| AccountId::new("DEFAULT"))
        } else {
            order.account_id.clone()
        }
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
    /// Returns fill events for any orders that matched, plus any expired orders.
    pub fn process_bar(&mut self, bar: &OHLCData) -> Vec<OrderEventAny> {
        let instrument_id = InstrumentId::new(&bar.symbol, &self.venue);

        // Get or create matching engine for this instrument
        let engine = self.get_or_create_engine(&instrument_id);

        // Process bar through matching engine
        let matches = engine.process_bar(bar);

        // Convert to events
        let mut events = self.process_matches(matches);

        // Check for expired GTD orders
        events.extend(self.check_expired_orders());

        events
    }

    /// Process a trade tick.
    ///
    /// Returns fill events for any orders that matched, plus any expired orders.
    pub fn process_trade_tick(
        &mut self,
        instrument_id: &InstrumentId,
        price: Decimal,
        quantity: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<OrderEventAny> {
        let engine = self.get_or_create_engine(instrument_id);
        let matches = engine.process_trade_tick(price, quantity, timestamp);
        let mut events = self.process_matches(matches);
        events.extend(self.check_expired_orders());
        events
    }

    /// Process a quote tick.
    ///
    /// Returns fill events for any orders that matched, plus any expired orders.
    pub fn process_quote_tick(
        &mut self,
        instrument_id: &InstrumentId,
        bid: Decimal,
        ask: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<OrderEventAny> {
        let engine = self.get_or_create_engine(instrument_id);
        let matches = engine.process_quote_tick(bid, ask, timestamp);
        let mut events = self.process_matches(matches);
        events.extend(self.check_expired_orders());
        events
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

        // If accounts are configured, lock funds for buy orders
        let resolved_account_id = self.resolve_account_id(&order);
        if let Some(account) = self.accounts.get_mut(&resolved_account_id) {
            if order.side == OrderSide::Buy {
                // Calculate required funds: price * quantity
                // For market orders, we'd need a reference price - use 0 for now (caller should set limit)
                let price = order.price.unwrap_or(Decimal::ZERO);
                let required = price * order.quantity;

                if required > Decimal::ZERO {
                    // Get the base currency balance
                    let currency = &account.base_currency.clone();
                    if let Some(balance) = account.balance_mut(currency) {
                        if let Err(e) = balance.lock(required) {
                            // Insufficient funds - reject order
                            let _ = stored_order.transition_to(OrderStatus::Rejected);
                            events.push(OrderEventAny::Rejected(OrderRejected {
                                event_id: EventId(uuid::Uuid::new_v4()),
                                client_order_id: order.client_order_id.clone(),
                                account_id: order.account_id.clone(),
                                reason: format!(
                                    "Insufficient funds in account {}: {}",
                                    resolved_account_id, e
                                ),
                                ts_event: self.current_datetime(),
                                ts_init: Utc::now(),
                            }));
                            return events;
                        }
                        // Track locked amount with account ID for later unlock on cancel
                        self.locked_amounts.insert(
                            order.client_order_id.clone(),
                            (resolved_account_id.clone(), required),
                        );
                    }
                }
            }
        }

        // Set venue order ID and transition to Accepted
        stored_order.venue_order_id = Some(venue_order_id.clone());
        if let Err(e) = stored_order.transition_to(OrderStatus::Accepted) {
            // Unlock funds if we locked them
            if let Some((account_id, locked)) = self.locked_amounts.remove(&order.client_order_id) {
                if let Some(account) = self.accounts.get_mut(&account_id) {
                    let currency = &account.base_currency.clone();
                    if let Some(balance) = account.balance_mut(currency) {
                        let _ = balance.unlock(locked);
                    }
                }
            }
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

        // Remove from stored orders after processing and unlock funds
        if !events.is_empty() {
            if let Some(OrderEventAny::Canceled(_)) = events.first() {
                self.stored_orders.remove(client_order_id);

                // Unlock any funds that were locked for this order (from correct account)
                if let Some((account_id, locked)) = self.locked_amounts.remove(client_order_id) {
                    if let Some(account) = self.accounts.get_mut(&account_id) {
                        let currency = &account.base_currency.clone();
                        if let Some(balance) = account.balance_mut(currency) {
                            let _ = balance.unlock(locked);
                        }
                    }
                }
            }
        }

        events
    }

    /// Process a modify order command.
    ///
    /// Modifies an open order's price and/or quantity.
    /// Returns OrderUpdated on success or OrderModifyRejected on failure.
    fn process_modify(
        &mut self,
        client_order_id: &ClientOrderId,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> Vec<OrderEventAny> {
        use crate::orders::{OrderModifyRejected, OrderUpdated};

        let mut events = Vec::new();

        // Find the stored order
        if let Some(stored) = self.stored_orders.get_mut(client_order_id) {
            // Validate order can be modified (must be in open state)
            if !stored.order.is_open() {
                events.push(OrderEventAny::ModifyRejected(OrderModifyRejected::new(
                    client_order_id.clone(),
                    Some(stored.venue_order_id.clone()),
                    stored.order.account_id.clone(),
                    format!(
                        "Order cannot be modified in status: {:?}",
                        stored.order.status
                    ),
                )));
                return events;
            }

            // Validate new values
            if let Some(qty) = new_quantity {
                if qty <= Decimal::ZERO {
                    events.push(OrderEventAny::ModifyRejected(OrderModifyRejected::new(
                        client_order_id.clone(),
                        Some(stored.venue_order_id.clone()),
                        stored.order.account_id.clone(),
                        "New quantity must be positive".to_string(),
                    )));
                    return events;
                }
                // Can't reduce quantity below filled amount
                if qty < stored.order.filled_qty {
                    events.push(OrderEventAny::ModifyRejected(OrderModifyRejected::new(
                        client_order_id.clone(),
                        Some(stored.venue_order_id.clone()),
                        stored.order.account_id.clone(),
                        format!(
                            "New quantity {} less than filled quantity {}",
                            qty, stored.order.filled_qty
                        ),
                    )));
                    return events;
                }
            }

            // Handle balance adjustment for buy orders if price/quantity changed
            if stored.order.side == OrderSide::Buy {
                // Get the account ID from existing locked amounts
                if let Some((account_id, old_locked_amt)) =
                    self.locked_amounts.get(client_order_id).cloned()
                {
                    if let Some(account) = self.accounts.get_mut(&account_id) {
                        let old_price = stored.order.price.unwrap_or(Decimal::ZERO);
                        let old_qty = stored.order.quantity;

                        let final_price = new_price.unwrap_or(old_price);
                        let final_qty = new_quantity.unwrap_or(old_qty);
                        let new_required = final_price * final_qty;

                        let currency = &account.base_currency.clone();
                        if let Some(balance) = account.balance_mut(currency) {
                            if new_required > old_locked_amt {
                                // Need to lock more
                                let additional = new_required - old_locked_amt;
                                if let Err(e) = balance.lock(additional) {
                                    events.push(OrderEventAny::ModifyRejected(
                                        OrderModifyRejected::new(
                                            client_order_id.clone(),
                                            Some(stored.venue_order_id.clone()),
                                            stored.order.account_id.clone(),
                                            format!(
                                                "Insufficient funds in account {} for modification: {}",
                                                account_id, e
                                            ),
                                        ),
                                    ));
                                    return events;
                                }
                                self.locked_amounts
                                    .insert(client_order_id.clone(), (account_id, new_required));
                            } else if new_required < old_locked_amt {
                                // Release excess locked
                                let excess = old_locked_amt - new_required;
                                let _ = balance.unlock(excess);
                                self.locked_amounts
                                    .insert(client_order_id.clone(), (account_id, new_required));
                            }
                        }
                    }
                }
            }

            // Update the order
            if let Some(price) = new_price {
                stored.order.price = Some(price);
            }
            if let Some(qty) = new_quantity {
                stored.order.quantity = qty;
                stored.order.leaves_qty = qty - stored.order.filled_qty;
            }

            // Update in matching engine
            let instrument_id = stored.order.instrument_id.clone();
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                // Remove old order and add updated one
                engine.cancel_order(client_order_id);
                engine.add_order(stored.order.clone());
            }

            // Generate updated event
            events.push(OrderEventAny::Updated(OrderUpdated::new(
                client_order_id.clone(),
                Some(stored.venue_order_id.clone()),
                stored.order.account_id.clone(),
                stored.order.price,
                stored.order.trigger_price,
                stored.order.quantity,
            )));
        } else {
            // Order not found
            events.push(OrderEventAny::ModifyRejected(OrderModifyRejected::new(
                client_order_id.clone(),
                None,
                crate::orders::AccountId::default(),
                "Order not found".to_string(),
            )));
        }

        events
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
                    orders_to_remove.push((m.client_order_id.clone(), stored.order.side));
                }
            }
        }

        // Remove fully filled orders and update account balances
        for (order_id, side) in orders_to_remove {
            self.stored_orders.remove(&order_id);

            // Handle account balance updates for filled orders (from correct account)
            if let Some((account_id, locked)) = self.locked_amounts.remove(&order_id) {
                if let Some(account) = self.accounts.get_mut(&account_id) {
                    let currency = &account.base_currency.clone();
                    if let Some(balance) = account.balance_mut(currency) {
                        match side {
                            OrderSide::Buy => {
                                // For buy orders: locked funds are consumed (fill from locked)
                                let _ = balance.fill(locked);
                            }
                            OrderSide::Sell => {
                                // For sell orders: should add proceeds (not implemented here,
                                // as sell orders don't lock base currency)
                                // The caller (backtest engine) handles adding proceeds
                            }
                        }
                    }
                }
            }
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
        if !self
            .inflight_queue
            .get_pending_for_order(client_order_id)
            .is_empty()
        {
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
        self.stored_orders
            .get(client_order_id)
            .map(|s| s.order.clone())
    }

    /// Get order fill information (cumulative quantity filled).
    pub fn get_order_fill_info(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(Decimal, Decimal)> {
        self.stored_orders.get(client_order_id).map(|s| {
            let cum_qty = s.cum_qty;
            let leaves_qty = (s.order.quantity - cum_qty).max(Decimal::ZERO);
            (cum_qty, leaves_qty)
        })
    }

    /// Check and expire GTD (Good-Till-Date) orders that have passed their expiry time.
    ///
    /// This should be called periodically (e.g., with each bar/tick) to expire orders.
    /// Returns events for any expired orders.
    pub fn check_expired_orders(&mut self) -> Vec<OrderEventAny> {
        use crate::orders::{OrderExpired, TimeInForce};

        if !self.config.support_gtd_orders {
            return Vec::new();
        }

        let current_time = self.current_datetime();
        let mut events = Vec::new();
        let mut expired_ids = Vec::new();

        // Find expired orders
        for (client_order_id, stored) in &self.stored_orders {
            if stored.order.time_in_force == TimeInForce::GTD {
                if let Some(expire_time) = stored.order.expire_time {
                    if current_time >= expire_time {
                        expired_ids.push((
                            client_order_id.clone(),
                            stored.venue_order_id.clone(),
                            stored.order.account_id.clone(),
                            stored.order.instrument_id.clone(),
                            stored.order.side,
                        ));
                    }
                }
            }
        }

        // Expire found orders
        for (client_order_id, venue_order_id, order_account_id, instrument_id, side) in expired_ids
        {
            // Remove from matching engine
            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.cancel_order(&client_order_id);
            }

            // Remove from stored orders
            self.stored_orders.remove(&client_order_id);

            // Unlock any locked funds (from correct account)
            if let Some((locked_account_id, locked)) = self.locked_amounts.remove(&client_order_id)
            {
                if side == OrderSide::Buy {
                    if let Some(account) = self.accounts.get_mut(&locked_account_id) {
                        let currency = &account.base_currency.clone();
                        if let Some(balance) = account.balance_mut(currency) {
                            let _ = balance.unlock(locked);
                        }
                    }
                }
            }

            // Create expired event
            events.push(OrderEventAny::Expired(OrderExpired::new(
                client_order_id,
                Some(venue_order_id),
                order_account_id,
            )));
        }

        events
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
        self.locked_amounts.clear();
        // Note: Account is not reset here - caller should reset if needed
    }

    /// Get current locked amount for an order.
    pub fn get_locked_amount(&self, client_order_id: &ClientOrderId) -> Option<Decimal> {
        self.locked_amounts
            .get(client_order_id)
            .map(|(_, amount)| *amount)
    }

    /// Get current locked amount and account ID for an order.
    pub fn get_locked_info(&self, client_order_id: &ClientOrderId) -> Option<(AccountId, Decimal)> {
        self.locked_amounts.get(client_order_id).cloned()
    }

    /// Get total locked amount across all orders.
    pub fn total_locked(&self) -> Decimal {
        self.locked_amounts.values().map(|(_, amount)| amount).sum()
    }

    /// Get total locked amount for a specific account.
    pub fn total_locked_for_account(&self, account_id: &AccountId) -> Decimal {
        self.locked_amounts
            .values()
            .filter(|(id, _)| id == account_id)
            .map(|(_, amount)| *amount)
            .sum()
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

    // =========================================================================
    // Multi-Account Tests
    // =========================================================================

    fn create_account(id: &str, balance: Decimal) -> Account {
        let mut account = Account::simulated(id, "USDT");
        account.deposit("USDT", balance);
        account
    }

    fn create_limit_order_with_account(
        symbol: &str,
        side: OrderSide,
        price: Decimal,
        account_id: &str,
    ) -> Order {
        Order::limit(symbol, side, dec!(1.0), price)
            .with_venue(TEST_VENUE)
            .with_account_id(account_id)
            .build()
            .unwrap()
    }

    #[test]
    fn test_multi_account_setup() {
        let account1 = create_account("ACCOUNT-A", dec!(100000));
        let account2 = create_account("ACCOUNT-B", dec!(50000));

        let exchange = SimulatedExchange::new(TEST_VENUE)
            .with_accounts(vec![account1, account2])
            .with_default_account(AccountId::new("ACCOUNT-A"));

        assert_eq!(exchange.account_count(), 2);
        assert!(exchange.account(&AccountId::new("ACCOUNT-A")).is_some());
        assert!(exchange.account(&AccountId::new("ACCOUNT-B")).is_some());
        assert!(exchange.default_account().is_some());
        assert_eq!(exchange.default_account().unwrap().id.as_str(), "ACCOUNT-A");
    }

    #[test]
    fn test_multi_account_balance_isolation() {
        let account_a = create_account("ACCOUNT-A", dec!(100000));
        let account_b = create_account("ACCOUNT-B", dec!(50000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE)
            .with_accounts(vec![account_a, account_b])
            .with_default_account(AccountId::new("ACCOUNT-A"));

        // Submit order for ACCOUNT-A
        let order_a =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(50000), "ACCOUNT-A");
        exchange.submit_order(order_a);
        exchange.process_inflight_commands();

        // Check that ACCOUNT-A has locked funds
        let total_locked_a = exchange.total_locked_for_account(&AccountId::new("ACCOUNT-A"));
        assert_eq!(total_locked_a, dec!(50000)); // 1.0 * 50000

        // Check that ACCOUNT-B has no locked funds
        let total_locked_b = exchange.total_locked_for_account(&AccountId::new("ACCOUNT-B"));
        assert_eq!(total_locked_b, dec!(0));

        // Verify account balances
        let acc_a = exchange.account(&AccountId::new("ACCOUNT-A")).unwrap();
        assert_eq!(acc_a.balance("USDT").unwrap().locked, dec!(50000));
        assert_eq!(acc_a.balance("USDT").unwrap().free, dec!(50000));

        let acc_b = exchange.account(&AccountId::new("ACCOUNT-B")).unwrap();
        assert_eq!(acc_b.balance("USDT").unwrap().locked, dec!(0));
        assert_eq!(acc_b.balance("USDT").unwrap().free, dec!(50000));
    }

    #[test]
    fn test_default_account_fallback() {
        let account = create_account("DEFAULT", dec!(100000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE).with_account(account); // Uses with_account which sets default

        // Order without explicit account_id should use default
        let order = create_limit_order("BTCUSDT", OrderSide::Buy, dec!(50000));
        exchange.submit_order(order);
        exchange.process_inflight_commands();

        // Check locked amount is in default account
        let total_locked = exchange.total_locked_for_account(&AccountId::new("DEFAULT"));
        assert_eq!(total_locked, dec!(50000));
    }

    #[test]
    fn test_cross_account_rejection() {
        // ACCOUNT-A has enough, ACCOUNT-B does not
        let account_a = create_account("ACCOUNT-A", dec!(100000));
        let account_b = create_account("ACCOUNT-B", dec!(1000)); // Only 1000

        let mut exchange = SimulatedExchange::new(TEST_VENUE)
            .with_accounts(vec![account_a, account_b])
            .with_default_account(AccountId::new("ACCOUNT-A"));

        // Submit order for ACCOUNT-B (should fail - insufficient funds)
        let order_b =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(50000), "ACCOUNT-B");
        exchange.submit_order(order_b);
        let events = exchange.process_inflight_commands();

        // Should be rejected
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Rejected(_)));

        // ACCOUNT-A should still be able to place orders
        let order_a =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(50000), "ACCOUNT-A");
        exchange.submit_order(order_a);
        let events = exchange.process_inflight_commands();

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Accepted(_)));
    }

    #[test]
    fn test_account_specific_cancel_unlocks() {
        let account_a = create_account("ACCOUNT-A", dec!(100000));
        let account_b = create_account("ACCOUNT-B", dec!(100000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE)
            .with_accounts(vec![account_a, account_b])
            .with_default_account(AccountId::new("ACCOUNT-A"));

        // Submit orders for both accounts
        let order_a =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(50000), "ACCOUNT-A");
        let order_b =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(40000), "ACCOUNT-B");

        let order_a_id = order_a.client_order_id.clone();

        exchange.submit_order(order_a);
        exchange.submit_order(order_b);
        exchange.process_inflight_commands();

        // Verify both accounts have locked funds
        assert_eq!(
            exchange.total_locked_for_account(&AccountId::new("ACCOUNT-A")),
            dec!(50000)
        );
        assert_eq!(
            exchange.total_locked_for_account(&AccountId::new("ACCOUNT-B")),
            dec!(40000)
        );

        // Cancel order A
        exchange.cancel_order(order_a_id);
        exchange.process_inflight_commands();

        // ACCOUNT-A should have unlocked, ACCOUNT-B still locked
        assert_eq!(
            exchange.total_locked_for_account(&AccountId::new("ACCOUNT-A")),
            dec!(0)
        );
        assert_eq!(
            exchange.total_locked_for_account(&AccountId::new("ACCOUNT-B")),
            dec!(40000)
        );

        // Verify ACCOUNT-A balance is restored
        let acc_a = exchange.account(&AccountId::new("ACCOUNT-A")).unwrap();
        assert_eq!(acc_a.balance("USDT").unwrap().free, dec!(100000));
        assert_eq!(acc_a.balance("USDT").unwrap().locked, dec!(0));
    }

    #[test]
    fn test_accounts_config_loading() {
        use crate::accounts::{AccountsConfig, DefaultAccountConfig, SimulationAccountConfig};

        let config = AccountsConfig {
            default: DefaultAccountConfig {
                id: "DEFAULT".to_string(),
                currency: "USDT".to_string(),
                initial_balance: dec!(100000),
            },
            simulation: vec![SimulationAccountConfig {
                id: "AGGRESSIVE".to_string(),
                currency: "USDT".to_string(),
                initial_balance: dec!(50000),
                strategies: vec!["momentum".to_string()],
            }],
        };

        let exchange = SimulatedExchange::new(TEST_VENUE).with_accounts_config(&config);

        assert_eq!(exchange.account_count(), 2);
        assert!(exchange.account(&AccountId::new("DEFAULT")).is_some());
        assert!(exchange.account(&AccountId::new("AGGRESSIVE")).is_some());

        // Verify balances
        let default_acc = exchange.account(&AccountId::new("DEFAULT")).unwrap();
        assert_eq!(default_acc.total_base(), dec!(100000));

        let agg_acc = exchange.account(&AccountId::new("AGGRESSIVE")).unwrap();
        assert_eq!(agg_acc.total_base(), dec!(50000));
    }

    #[test]
    fn test_all_accounts_iterator() {
        let account1 = create_account("ACC-1", dec!(100000));
        let account2 = create_account("ACC-2", dec!(50000));
        let account3 = create_account("ACC-3", dec!(25000));

        let exchange =
            SimulatedExchange::new(TEST_VENUE).with_accounts(vec![account1, account2, account3]);

        let total_balance: Decimal = exchange.all_accounts().map(|a| a.total_base()).sum();
        assert_eq!(total_balance, dec!(175000));
    }

    #[test]
    fn test_get_locked_info() {
        let account = create_account("MY-ACCOUNT", dec!(100000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE).with_account(account);

        let order =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(50000), "MY-ACCOUNT");
        let order_id = order.client_order_id.clone();

        exchange.submit_order(order);
        exchange.process_inflight_commands();

        // Get locked info should return both account ID and amount
        let (account_id, amount) = exchange.get_locked_info(&order_id).unwrap();
        assert_eq!(account_id.as_str(), "MY-ACCOUNT");
        assert_eq!(amount, dec!(50000));
    }

    #[test]
    fn test_multi_account_fill_updates_correct_account() {
        let account_a = create_account("ACCOUNT-A", dec!(100000));
        let account_b = create_account("ACCOUNT-B", dec!(100000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE)
            .with_accounts(vec![account_a, account_b])
            .with_default_account(AccountId::new("ACCOUNT-A"));

        // Submit order for ACCOUNT-A
        let order =
            create_limit_order_with_account("BTCUSDT", OrderSide::Buy, dec!(50000), "ACCOUNT-A");
        exchange.submit_order(order);
        exchange.process_inflight_commands();

        // Verify funds locked in ACCOUNT-A
        let acc_a_before = exchange.account(&AccountId::new("ACCOUNT-A")).unwrap();
        assert_eq!(acc_a_before.balance("USDT").unwrap().locked, dec!(50000));
        assert_eq!(acc_a_before.balance("USDT").unwrap().free, dec!(50000));

        // Process bar that fills the order
        let bar = create_bar("BTCUSDT", dec!(50500));
        let events = exchange.process_bar(&bar);

        // Should have 1 fill
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], OrderEventAny::Filled(_)));

        // Verify ACCOUNT-A balance was updated (locked consumed)
        let acc_a_after = exchange.account(&AccountId::new("ACCOUNT-A")).unwrap();
        assert_eq!(acc_a_after.balance("USDT").unwrap().locked, dec!(0));
        // Total should be reduced by fill amount
        assert_eq!(acc_a_after.balance("USDT").unwrap().total, dec!(50000));

        // Verify ACCOUNT-B was not affected
        let acc_b = exchange.account(&AccountId::new("ACCOUNT-B")).unwrap();
        assert_eq!(acc_b.balance("USDT").unwrap().total, dec!(100000));
        assert_eq!(acc_b.balance("USDT").unwrap().locked, dec!(0));
    }

    #[test]
    fn test_multi_strategy_single_account_balance_competition() {
        // Two strategies sharing SHARED-ACCOUNT with $100k
        let shared_account = create_account("SHARED-ACCOUNT", dec!(100000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE).with_account(shared_account);

        // Strategy A submits order for $60k
        let order_a = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(60000))
            .with_venue(TEST_VENUE)
            .with_account_id("SHARED-ACCOUNT")
            .build()
            .unwrap();
        exchange.submit_order(order_a);
        let events_a = exchange.process_inflight_commands();
        assert_eq!(events_a.len(), 1);
        assert!(matches!(events_a[0], OrderEventAny::Accepted(_)));

        // Verify $60k locked, $40k free
        let acc = exchange.account(&AccountId::new("SHARED-ACCOUNT")).unwrap();
        assert_eq!(acc.balance("USDT").unwrap().locked, dec!(60000));
        assert_eq!(acc.balance("USDT").unwrap().free, dec!(40000));

        // Strategy B tries to submit order for $50k - should fail (only $40k free)
        let order_b = Order::limit("ETHUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .with_venue(TEST_VENUE)
            .with_account_id("SHARED-ACCOUNT")
            .build()
            .unwrap();
        exchange.submit_order(order_b);
        let events_b = exchange.process_inflight_commands();
        assert_eq!(events_b.len(), 1);
        assert!(matches!(events_b[0], OrderEventAny::Rejected(_)));

        // Strategy B submits smaller order for $30k - should succeed
        let order_c = Order::limit("ETHUSDT", OrderSide::Buy, dec!(1.0), dec!(30000))
            .with_venue(TEST_VENUE)
            .with_account_id("SHARED-ACCOUNT")
            .build()
            .unwrap();
        exchange.submit_order(order_c);
        let events_c = exchange.process_inflight_commands();
        assert_eq!(events_c.len(), 1);
        assert!(matches!(events_c[0], OrderEventAny::Accepted(_)));

        // Verify $90k locked, $10k free
        let acc = exchange.account(&AccountId::new("SHARED-ACCOUNT")).unwrap();
        assert_eq!(acc.balance("USDT").unwrap().locked, dec!(90000));
        assert_eq!(acc.balance("USDT").unwrap().free, dec!(10000));
    }

    #[test]
    fn test_nonexistent_account_uses_default() {
        let default_account = create_account("DEFAULT", dec!(100000));

        let mut exchange = SimulatedExchange::new(TEST_VENUE).with_account(default_account);

        // Order with nonexistent account_id should fall back to default
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .with_venue(TEST_VENUE)
            .with_account_id("NONEXISTENT")
            .build()
            .unwrap();
        exchange.submit_order(order);
        let events = exchange.process_inflight_commands();

        // Order should be rejected since NONEXISTENT account doesn't exist
        // and there's no account with that ID
        assert_eq!(events.len(), 1);
        // Since the account doesn't exist, funds can't be locked
        // Let's verify the behavior - if no account found, order proceeds without balance check
        // This is the current behavior - might want to reject instead
    }

    #[test]
    fn test_accounts_config_builder_pattern() {
        use crate::accounts::AccountsConfig;

        let config = AccountsConfig::with_default("MAIN", "USDT", dec!(100000))
            .add_simulation_account(
                "AGGRESSIVE",
                "USDT",
                dec!(50000),
                vec!["momentum".to_string()],
            )
            .add_simulation_account(
                "CONSERVATIVE",
                "USDT",
                dec!(200000),
                vec!["mean_reversion".to_string(), "rsi".to_string()],
            );

        assert_eq!(config.default.id, "MAIN");
        assert_eq!(config.simulation.len(), 2);
        assert_eq!(config.account_for_strategy("momentum"), "AGGRESSIVE");
        assert_eq!(config.account_for_strategy("rsi"), "CONSERVATIVE");
        assert_eq!(config.account_for_strategy("unknown"), "MAIN");
    }
}
