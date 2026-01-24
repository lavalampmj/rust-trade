//! Per-instrument order matching engine.
//!
//! Wraps the core matching logic with instrument-specific configuration
//! and optional order book integration.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;

use super::core::{MatchResult, OrderMatchingCore};
use crate::data::types::OHLCData;
use crate::orders::{ClientOrderId, InstrumentId, Order};

/// Configuration for the matching engine.
#[derive(Debug, Clone)]
pub struct MatchingEngineConfig {
    /// Use OHLC bar data for matching (default: true)
    pub bar_execution: bool,
    /// Use trade ticks for matching
    pub trade_execution: bool,
    /// Use quote ticks for matching
    pub quote_execution: bool,
    /// Use L2 order book for matching (requires book data)
    pub book_execution: bool,
    /// Track liquidity consumption at each price level
    pub liquidity_consumption: bool,
    /// Reject stop orders (some venues don't support them)
    pub reject_stop_orders: bool,
    /// Add randomness to fill determination
    pub use_random_fills: bool,
    /// Fill market orders at bar open (vs close)
    pub bar_open_fills: bool,
    /// Use OHLC high/low to determine if limit touched
    pub use_ohlc_range: bool,
    /// Allow partial fills
    pub partial_fills_enabled: bool,
    /// Minimum fill percentage when partial
    pub min_fill_pct: Decimal,
}

impl Default for MatchingEngineConfig {
    fn default() -> Self {
        Self {
            bar_execution: true,
            trade_execution: true,
            quote_execution: true,
            book_execution: false,
            liquidity_consumption: false,
            reject_stop_orders: false,
            use_random_fills: false,
            bar_open_fills: false,
            use_ohlc_range: true,
            partial_fills_enabled: true,
            min_fill_pct: Decimal::new(1, 1), // 0.1 = 10%
        }
    }
}

/// Per-instrument order matching engine.
///
/// Handles order matching for a single instrument, supporting multiple
/// data granularities (bars, ticks, quotes, L2 depth).
#[derive(Debug)]
pub struct OrderMatchingEngine {
    /// Instrument this engine handles
    pub instrument_id: InstrumentId,
    /// Core matching logic
    core: OrderMatchingCore,
    /// Consumed bid liquidity at each price level
    bid_consumption: HashMap<Decimal, Decimal>,
    /// Consumed ask liquidity at each price level
    ask_consumption: HashMap<Decimal, Decimal>,
    /// Configuration
    config: MatchingEngineConfig,
    /// Last known bid price
    last_bid: Option<Decimal>,
    /// Last known ask price
    last_ask: Option<Decimal>,
    /// Last trade price
    last_trade: Option<Decimal>,
    /// Last bar timestamp
    last_bar_time: Option<DateTime<Utc>>,
}

impl OrderMatchingEngine {
    /// Create a new matching engine for an instrument.
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self::with_config(instrument_id, MatchingEngineConfig::default())
    }

    /// Create a new matching engine with custom configuration.
    pub fn with_config(instrument_id: InstrumentId, config: MatchingEngineConfig) -> Self {
        Self {
            instrument_id,
            core: OrderMatchingCore::new(),
            bid_consumption: HashMap::new(),
            ask_consumption: HashMap::new(),
            config,
            last_bid: None,
            last_ask: None,
            last_trade: None,
            last_bar_time: None,
        }
    }

    /// Process an OHLC bar - MINIMUM data requirement.
    ///
    /// Uses the bar's OHLC range to determine fills:
    /// - Buy limit: fills if low <= limit price
    /// - Sell limit: fills if high >= limit price
    /// - Stop triggers: check if high/low crossed trigger
    /// - Market orders: fill at close price (or open if configured)
    pub fn process_bar(&mut self, bar: &OHLCData) -> Vec<MatchResult> {
        if !self.config.bar_execution {
            return Vec::new();
        }

        let timestamp = bar.timestamp;
        self.last_bar_time = Some(timestamp);

        // Update last trade price
        self.last_trade = Some(bar.close);

        // Determine fill price for market orders
        let market_fill_price = if self.config.bar_open_fills {
            bar.open
        } else {
            bar.close
        };

        // Check stop triggers first
        let triggered = self.core.check_triggers(
            self.last_bid,
            self.last_ask,
            Some(bar.close),
            Some(bar.high),
            Some(bar.low),
        );

        // Add triggered orders back for immediate matching
        for order in triggered {
            self.core.add_order(order);
        }

        // Match resting orders using OHLC range
        let (high, low) = if self.config.use_ohlc_range {
            (Some(bar.high), Some(bar.low))
        } else {
            (None, None)
        };

        self.core.match_orders(
            self.last_bid,
            Some(market_fill_price), // Use as ask for buys
            Some(bar.close),
            high,
            low,
            timestamp,
        )
    }

    /// Process a trade tick.
    pub fn process_trade_tick(
        &mut self,
        price: Decimal,
        _quantity: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<MatchResult> {
        if !self.config.trade_execution {
            return Vec::new();
        }

        self.last_trade = Some(price);

        // Check triggers
        let triggered = self.core.check_triggers(
            self.last_bid,
            self.last_ask,
            Some(price),
            None,
            None,
        );

        for order in triggered {
            self.core.add_order(order);
        }

        // Match orders
        self.core.match_orders(
            self.last_bid,
            self.last_ask,
            Some(price),
            None,
            None,
            timestamp,
        )
    }

    /// Process a quote tick (bid/ask update).
    pub fn process_quote_tick(
        &mut self,
        bid: Decimal,
        ask: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Vec<MatchResult> {
        if !self.config.quote_execution {
            return Vec::new();
        }

        self.last_bid = Some(bid);
        self.last_ask = Some(ask);

        // Check triggers with mid price as last
        let mid = (bid + ask) / Decimal::TWO;

        let triggered = self.core.check_triggers(
            Some(bid),
            Some(ask),
            Some(mid),
            None,
            None,
        );

        for order in triggered {
            self.core.add_order(order);
        }

        // Match orders
        self.core.match_orders(
            Some(bid),
            Some(ask),
            self.last_trade,
            None,
            None,
            timestamp,
        )
    }

    /// Add an order to the matching engine.
    pub fn add_order(&mut self, order: Order) {
        self.core.add_order(order);
    }

    /// Cancel an order by client order ID.
    pub fn cancel_order(&mut self, client_order_id: &ClientOrderId) -> Option<Order> {
        self.core.remove_order(client_order_id)
    }

    /// Check if an order exists in this engine.
    pub fn has_order(&self, client_order_id: &ClientOrderId) -> bool {
        self.core.has_order(client_order_id)
    }

    /// Get count of open orders.
    pub fn open_order_count(&self) -> usize {
        self.core.open_order_count()
    }

    /// Get all open orders.
    pub fn open_orders(&self) -> impl Iterator<Item = &Order> {
        self.core.open_orders()
    }

    /// Reset liquidity consumption tracking.
    pub fn reset_consumption(&mut self) {
        self.bid_consumption.clear();
        self.ask_consumption.clear();
    }

    /// Clear all orders from the engine.
    pub fn clear(&mut self) {
        self.core.clear();
        self.reset_consumption();
    }

    /// Get last known prices.
    pub fn last_prices(&self) -> (Option<Decimal>, Option<Decimal>, Option<Decimal>) {
        (self.last_bid, self.last_ask, self.last_trade)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::OrderSide;
    use rust_decimal_macros::dec;

    fn create_instrument() -> InstrumentId {
        InstrumentId::new("BTCUSDT", "TEST")
    }

    fn create_bar(open: Decimal, high: Decimal, low: Decimal, close: Decimal) -> OHLCData {
        use crate::data::types::Timeframe;
        OHLCData {
            timestamp: Utc::now(),
            symbol: "BTCUSDT".to_string(),
            timeframe: Timeframe::OneMinute,
            open,
            high,
            low,
            close,
            volume: dec!(100),
            trade_count: 50,
        }
    }

    fn create_limit_order(side: OrderSide, price: Decimal) -> Order {
        Order::limit("BTCUSDT", side, dec!(1.0), price)
            .build()
            .unwrap()
    }

    fn create_stop_order(side: OrderSide, trigger: Decimal) -> Order {
        Order::stop("BTCUSDT", side, dec!(1.0), trigger)
            .build()
            .unwrap()
    }

    fn create_market_order(side: OrderSide) -> Order {
        Order::market("BTCUSDT", side, dec!(1.0))
            .build()
            .unwrap()
    }

    #[test]
    fn test_new_engine() {
        let engine = OrderMatchingEngine::new(create_instrument());
        assert_eq!(engine.open_order_count(), 0);
        assert!(engine.last_prices() == (None, None, None));
    }

    #[test]
    fn test_add_and_cancel_order() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_limit_order(OrderSide::Buy, dec!(50000));
        let id = order.client_order_id.clone();

        engine.add_order(order);
        assert_eq!(engine.open_order_count(), 1);
        assert!(engine.has_order(&id));

        let canceled = engine.cancel_order(&id);
        assert!(canceled.is_some());
        assert_eq!(engine.open_order_count(), 0);
    }

    #[test]
    fn test_limit_buy_fills_on_bar() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_limit_order(OrderSide::Buy, dec!(50000));
        engine.add_order(order);

        // Bar with low below limit
        let bar = create_bar(dec!(51000), dec!(52000), dec!(49000), dec!(51500));
        let results = engine.process_bar(&bar);

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(50000));
        assert_eq!(engine.open_order_count(), 0);
    }

    #[test]
    fn test_limit_sell_fills_on_bar() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_limit_order(OrderSide::Sell, dec!(52000));
        engine.add_order(order);

        // Bar with high above limit
        let bar = create_bar(dec!(51000), dec!(53000), dec!(50000), dec!(51500));
        let results = engine.process_bar(&bar);

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(52000));
    }

    #[test]
    fn test_stop_triggers_and_fills() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_stop_order(OrderSide::Buy, dec!(52000));
        engine.add_order(order);

        // Bar with high that triggers stop
        let bar = create_bar(dec!(51000), dec!(53000), dec!(50000), dec!(52500));
        let results = engine.process_bar(&bar);

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        // Stop triggered and filled as market at close
        assert_eq!(results[0].fill_price, dec!(52500));
        assert_eq!(engine.open_order_count(), 0);
    }

    #[test]
    fn test_market_order_fills_at_close() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_market_order(OrderSide::Buy);
        engine.add_order(order);

        let bar = create_bar(dec!(50000), dec!(51000), dec!(49000), dec!(50500));
        let results = engine.process_bar(&bar);

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(50500)); // Filled at close
    }

    #[test]
    fn test_market_order_fills_at_open() {
        let config = MatchingEngineConfig {
            bar_open_fills: true,
            ..Default::default()
        };
        let mut engine = OrderMatchingEngine::with_config(create_instrument(), config);

        let order = create_market_order(OrderSide::Buy);
        engine.add_order(order);

        let bar = create_bar(dec!(50000), dec!(51000), dec!(49000), dec!(50500));
        let results = engine.process_bar(&bar);

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(50000)); // Filled at open
    }

    #[test]
    fn test_no_fill_when_price_not_touched() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_limit_order(OrderSide::Buy, dec!(48000)); // Below bar range
        engine.add_order(order);

        let bar = create_bar(dec!(50000), dec!(51000), dec!(49000), dec!(50500));
        let results = engine.process_bar(&bar);

        assert!(results.is_empty());
        assert_eq!(engine.open_order_count(), 1);
    }

    #[test]
    fn test_process_quote_tick() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        let order = create_limit_order(OrderSide::Buy, dec!(50000));
        engine.add_order(order);

        // Quote with ask at limit
        let results = engine.process_quote_tick(dec!(49900), dec!(50000), Utc::now());

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(50000));
    }

    #[test]
    fn test_process_trade_tick() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        // First set bid/ask via quote
        engine.process_quote_tick(dec!(49900), dec!(50100), Utc::now());

        let order = create_market_order(OrderSide::Buy);
        engine.add_order(order);

        // Trade tick
        let results = engine.process_trade_tick(dec!(50050), dec!(1.0), Utc::now());

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
    }

    #[test]
    fn test_multiple_orders() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        // Add multiple limit orders
        for i in 1..=5 {
            let price = dec!(48000) + Decimal::from(i) * dec!(500);
            let order = create_limit_order(OrderSide::Buy, price);
            engine.add_order(order);
        }

        assert_eq!(engine.open_order_count(), 5);

        // Bar that fills some orders
        let bar = create_bar(dec!(50000), dec!(51000), dec!(49000), dec!(50500));
        let results = engine.process_bar(&bar);

        // Orders at 48500, 49000, 49500, 50000, 50500
        // Low at 49000 fills buy limits at or above 49000: 49000, 49500, 50000, 50500
        // (Buy limit fills when market price goes DOWN to the limit)
        assert_eq!(results.len(), 4);
        assert_eq!(engine.open_order_count(), 1); // Only 48500 unfilled
    }

    #[test]
    fn test_clear() {
        let mut engine = OrderMatchingEngine::new(create_instrument());

        engine.add_order(create_limit_order(OrderSide::Buy, dec!(50000)));

        assert_eq!(engine.open_order_count(), 1);

        engine.clear();

        assert_eq!(engine.open_order_count(), 0);
    }

    #[test]
    fn test_bar_execution_disabled() {
        let config = MatchingEngineConfig {
            bar_execution: false,
            ..Default::default()
        };
        let mut engine = OrderMatchingEngine::with_config(create_instrument(), config);

        engine.add_order(create_market_order(OrderSide::Buy));

        let bar = create_bar(dec!(50000), dec!(51000), dec!(49000), dec!(50500));
        let results = engine.process_bar(&bar);

        assert!(results.is_empty());
        assert_eq!(engine.open_order_count(), 1);
    }
}
