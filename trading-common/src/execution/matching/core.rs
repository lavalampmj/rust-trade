//! Core order matching logic.
//!
//! This module provides the fundamental matching algorithms for different
//! order types against market prices.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::orders::{
    ClientOrderId, LiquiditySide, Order, OrderSide, OrderStatus, OrderType, TradeId, TriggerType,
};

/// Result of an order match attempt.
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// The client order ID that was matched
    pub client_order_id: ClientOrderId,
    /// Whether the order was filled
    pub filled: bool,
    /// Fill quantity (may be partial)
    pub fill_qty: Decimal,
    /// Fill price
    pub fill_price: Decimal,
    /// Trade ID for this fill
    pub trade_id: TradeId,
    /// Liquidity side (maker/taker)
    pub liquidity_side: LiquiditySide,
    /// Timestamp of the fill
    pub fill_time: DateTime<Utc>,
    /// Was this a triggered order (stop became market)
    pub was_triggered: bool,
    /// Slippage from requested price (if any)
    pub slippage: Decimal,
}

impl MatchResult {
    /// Create a new match result for a fill.
    pub fn fill(
        client_order_id: ClientOrderId,
        fill_qty: Decimal,
        fill_price: Decimal,
        liquidity_side: LiquiditySide,
        fill_time: DateTime<Utc>,
    ) -> Self {
        Self {
            client_order_id,
            filled: true,
            fill_qty,
            fill_price,
            trade_id: TradeId(Uuid::new_v4().to_string()),
            liquidity_side,
            fill_time,
            was_triggered: false,
            slippage: Decimal::ZERO,
        }
    }

    /// Create a match result for a triggered stop order.
    pub fn triggered(
        client_order_id: ClientOrderId,
        fill_qty: Decimal,
        fill_price: Decimal,
        fill_time: DateTime<Utc>,
    ) -> Self {
        Self {
            client_order_id,
            filled: true,
            fill_qty,
            fill_price,
            trade_id: TradeId(Uuid::new_v4().to_string()),
            liquidity_side: LiquiditySide::Taker,
            fill_time,
            was_triggered: true,
            slippage: Decimal::ZERO,
        }
    }

    /// Set slippage amount.
    pub fn with_slippage(mut self, slippage: Decimal) -> Self {
        self.slippage = slippage;
        self
    }
}

/// Core matching logic for orders.
///
/// Maintains lists of trigger orders (stops) and resting orders (limits)
/// and provides matching methods against incoming prices.
#[derive(Debug, Default)]
pub struct OrderMatchingCore {
    /// Orders waiting for trigger (stops)
    pub trigger_orders: Vec<Order>,
    /// Orders resting at a price (limits)
    pub resting_orders: Vec<Order>,
}

impl OrderMatchingCore {
    /// Create a new matching core.
    pub fn new() -> Self {
        Self {
            trigger_orders: Vec::new(),
            resting_orders: Vec::new(),
        }
    }

    /// Add an order to the appropriate list based on its type.
    pub fn add_order(&mut self, order: Order) {
        match order.order_type {
            OrderType::Stop | OrderType::StopLimit | OrderType::TrailingStop => {
                self.trigger_orders.push(order);
            }
            OrderType::Limit | OrderType::LimitIfTouched | OrderType::MarketToLimit => {
                self.resting_orders.push(order);
            }
            OrderType::Market => {
                // Market orders don't rest - they should be matched immediately
                // They're added to resting temporarily for next price tick
                self.resting_orders.push(order);
            }
        }
    }

    /// Remove an order by client order ID.
    pub fn remove_order(&mut self, client_order_id: &ClientOrderId) -> Option<Order> {
        // Check trigger orders
        if let Some(pos) = self
            .trigger_orders
            .iter()
            .position(|o| &o.client_order_id == client_order_id)
        {
            return Some(self.trigger_orders.remove(pos));
        }

        // Check resting orders
        if let Some(pos) = self
            .resting_orders
            .iter()
            .position(|o| &o.client_order_id == client_order_id)
        {
            return Some(self.resting_orders.remove(pos));
        }

        None
    }

    /// Check and trigger stop orders based on price levels.
    ///
    /// Returns orders that were triggered (converted to market orders).
    pub fn check_triggers(
        &mut self,
        bid: Option<Decimal>,
        ask: Option<Decimal>,
        last: Option<Decimal>,
        high: Option<Decimal>,
        low: Option<Decimal>,
    ) -> Vec<Order> {
        let mut triggered = Vec::new();
        let mut remaining = Vec::new();

        for order in self.trigger_orders.drain(..) {
            if Self::should_trigger(&order, bid, ask, last, high, low) {
                // Convert to market order (for stops) or keep as limit (for stop-limit)
                let mut triggered_order = order.clone();
                triggered_order.status = OrderStatus::Triggered;

                if triggered_order.order_type == OrderType::Stop {
                    // Stop becomes market
                    triggered_order.order_type = OrderType::Market;
                } else if triggered_order.order_type == OrderType::StopLimit {
                    // StopLimit becomes limit at the limit price
                    triggered_order.order_type = OrderType::Limit;
                }

                triggered.push(triggered_order);
            } else {
                remaining.push(order);
            }
        }

        self.trigger_orders = remaining;
        triggered
    }

    /// Check if an order should trigger.
    fn should_trigger(
        order: &Order,
        bid: Option<Decimal>,
        ask: Option<Decimal>,
        last: Option<Decimal>,
        high: Option<Decimal>,
        low: Option<Decimal>,
    ) -> bool {
        let trigger_price = match order.trigger_price {
            Some(p) => p,
            None => return false,
        };

        // Determine which price to use based on trigger type
        let check_price = match order.trigger_type {
            TriggerType::LastPrice | TriggerType::None => last,
            TriggerType::BidPrice => bid,
            TriggerType::AskPrice => ask,
            TriggerType::MidPrice => {
                if let (Some(b), Some(a)) = (bid, ask) {
                    Some((b + a) / Decimal::TWO)
                } else {
                    None
                }
            }
            TriggerType::MarkPrice | TriggerType::IndexPrice => last, // Fallback to last
        };

        match order.side {
            OrderSide::Buy => {
                // Buy stop triggers when price rises above trigger
                // Check against high if available, otherwise use point price
                if let Some(h) = high {
                    h >= trigger_price
                } else if let Some(p) = check_price {
                    p >= trigger_price
                } else {
                    false
                }
            }
            OrderSide::Sell => {
                // Sell stop triggers when price falls below trigger
                // Check against low if available, otherwise use point price
                if let Some(l) = low {
                    l <= trigger_price
                } else if let Some(p) = check_price {
                    p <= trigger_price
                } else {
                    false
                }
            }
        }
    }

    /// Match resting orders against current prices.
    ///
    /// For bar data, uses OHLC range to determine fills.
    /// For tick data, uses the specific price point.
    pub fn match_orders(
        &mut self,
        bid: Option<Decimal>,
        ask: Option<Decimal>,
        last: Option<Decimal>,
        high: Option<Decimal>,
        low: Option<Decimal>,
        timestamp: DateTime<Utc>,
    ) -> Vec<MatchResult> {
        let mut results = Vec::new();
        let mut remaining = Vec::new();

        for order in self.resting_orders.drain(..) {
            if let Some(result) = Self::try_fill_order(&order, bid, ask, last, high, low, timestamp)
            {
                results.push(result);
            } else {
                // Market orders that couldn't fill are problematic
                if order.order_type == OrderType::Market {
                    // Force fill at last price if we have one
                    if let Some(price) = last.or(bid).or(ask) {
                        results.push(MatchResult::fill(
                            order.client_order_id.clone(),
                            order.leaves_qty,
                            price,
                            LiquiditySide::Taker,
                            timestamp,
                        ));
                    } else {
                        remaining.push(order);
                    }
                } else {
                    remaining.push(order);
                }
            }
        }

        self.resting_orders = remaining;
        results
    }

    /// Try to fill a single order.
    fn try_fill_order(
        order: &Order,
        bid: Option<Decimal>,
        ask: Option<Decimal>,
        last: Option<Decimal>,
        high: Option<Decimal>,
        low: Option<Decimal>,
        timestamp: DateTime<Utc>,
    ) -> Option<MatchResult> {
        match order.order_type {
            OrderType::Market => {
                // Market orders fill immediately at best available price
                let fill_price = match order.side {
                    OrderSide::Buy => ask.or(last)?,
                    OrderSide::Sell => bid.or(last)?,
                };

                Some(MatchResult::fill(
                    order.client_order_id.clone(),
                    order.leaves_qty,
                    fill_price,
                    LiquiditySide::Taker,
                    timestamp,
                ))
            }

            OrderType::Limit => {
                let limit_price = order.price?;

                match order.side {
                    OrderSide::Buy => {
                        // Buy limit fills if price drops to or below limit
                        let can_fill = if let Some(l) = low {
                            l <= limit_price
                        } else if let Some(a) = ask {
                            a <= limit_price
                        } else if let Some(p) = last {
                            p <= limit_price
                        } else {
                            false
                        };

                        if can_fill {
                            // Fill at limit price (maker gets price improvement)
                            Some(MatchResult::fill(
                                order.client_order_id.clone(),
                                order.leaves_qty,
                                limit_price,
                                LiquiditySide::Maker,
                                timestamp,
                            ))
                        } else {
                            None
                        }
                    }
                    OrderSide::Sell => {
                        // Sell limit fills if price rises to or above limit
                        let can_fill = if let Some(h) = high {
                            h >= limit_price
                        } else if let Some(b) = bid {
                            b >= limit_price
                        } else if let Some(p) = last {
                            p >= limit_price
                        } else {
                            false
                        };

                        if can_fill {
                            Some(MatchResult::fill(
                                order.client_order_id.clone(),
                                order.leaves_qty,
                                limit_price,
                                LiquiditySide::Maker,
                                timestamp,
                            ))
                        } else {
                            None
                        }
                    }
                }
            }

            OrderType::LimitIfTouched => {
                // Similar to limit but with trigger logic
                // For now, treat as limit
                let limit_price = order.price?;
                let touched = match order.side {
                    OrderSide::Buy => low.map(|l| l <= limit_price).unwrap_or(false),
                    OrderSide::Sell => high.map(|h| h >= limit_price).unwrap_or(false),
                };

                if touched {
                    Some(MatchResult::fill(
                        order.client_order_id.clone(),
                        order.leaves_qty,
                        limit_price,
                        LiquiditySide::Maker,
                        timestamp,
                    ))
                } else {
                    None
                }
            }

            _ => None, // Other types handled elsewhere
        }
    }

    /// Get all open orders.
    pub fn open_orders(&self) -> impl Iterator<Item = &Order> {
        self.trigger_orders.iter().chain(self.resting_orders.iter())
    }

    /// Get count of open orders.
    pub fn open_order_count(&self) -> usize {
        self.trigger_orders.len() + self.resting_orders.len()
    }

    /// Check if an order exists.
    pub fn has_order(&self, client_order_id: &ClientOrderId) -> bool {
        self.trigger_orders
            .iter()
            .any(|o| &o.client_order_id == client_order_id)
            || self
                .resting_orders
                .iter()
                .any(|o| &o.client_order_id == client_order_id)
    }

    /// Clear all orders.
    pub fn clear(&mut self) {
        self.trigger_orders.clear();
        self.resting_orders.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn create_limit_buy(price: Decimal) -> Order {
        Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), price)
            .build()
            .unwrap()
    }

    fn create_limit_sell(price: Decimal) -> Order {
        Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.0), price)
            .build()
            .unwrap()
    }

    fn create_stop_buy(trigger: Decimal) -> Order {
        Order::stop("BTCUSDT", OrderSide::Buy, dec!(1.0), trigger)
            .build()
            .unwrap()
    }

    fn create_stop_sell(trigger: Decimal) -> Order {
        Order::stop("BTCUSDT", OrderSide::Sell, dec!(1.0), trigger)
            .build()
            .unwrap()
    }

    fn create_market_buy() -> Order {
        Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap()
    }

    #[test]
    fn test_add_limit_order() {
        let mut core = OrderMatchingCore::new();
        let order = create_limit_buy(dec!(50000));

        core.add_order(order);

        assert_eq!(core.resting_orders.len(), 1);
        assert_eq!(core.trigger_orders.len(), 0);
    }

    #[test]
    fn test_add_stop_order() {
        let mut core = OrderMatchingCore::new();
        let order = create_stop_buy(dec!(51000));

        core.add_order(order);

        assert_eq!(core.resting_orders.len(), 0);
        assert_eq!(core.trigger_orders.len(), 1);
    }

    #[test]
    fn test_remove_order() {
        let mut core = OrderMatchingCore::new();
        let order = create_limit_buy(dec!(50000));
        let id = order.client_order_id.clone();

        core.add_order(order);
        assert_eq!(core.resting_orders.len(), 1);

        let removed = core.remove_order(&id);
        assert!(removed.is_some());
        assert_eq!(core.resting_orders.len(), 0);
    }

    #[test]
    fn test_limit_buy_fills_when_low_touches() {
        let mut core = OrderMatchingCore::new();
        let order = create_limit_buy(dec!(50000));

        core.add_order(order);

        // Bar with low that touches limit
        let results = core.match_orders(
            None,
            None,
            Some(dec!(50500)),
            Some(dec!(51000)),
            Some(dec!(49500)), // Low below limit
            Utc::now(),
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(50000));
        assert_eq!(results[0].liquidity_side, LiquiditySide::Maker);
    }

    #[test]
    fn test_limit_buy_no_fill_when_low_above() {
        let mut core = OrderMatchingCore::new();
        let order = create_limit_buy(dec!(50000));

        core.add_order(order);

        // Bar with low above limit
        let results = core.match_orders(
            None,
            None,
            Some(dec!(51000)),
            Some(dec!(52000)),
            Some(dec!(50500)), // Low above limit
            Utc::now(),
        );

        assert!(results.is_empty());
        assert_eq!(core.resting_orders.len(), 1);
    }

    #[test]
    fn test_limit_sell_fills_when_high_touches() {
        let mut core = OrderMatchingCore::new();
        let order = create_limit_sell(dec!(52000));

        core.add_order(order);

        // Bar with high that touches limit
        let results = core.match_orders(
            None,
            None,
            Some(dec!(51000)),
            Some(dec!(52500)), // High above limit
            Some(dec!(50000)),
            Utc::now(),
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(52000));
    }

    #[test]
    fn test_stop_buy_triggers_when_high_crosses() {
        let mut core = OrderMatchingCore::new();
        let order = create_stop_buy(dec!(51000));

        core.add_order(order);
        assert_eq!(core.trigger_orders.len(), 1);

        // Price rises above trigger
        let triggered = core.check_triggers(
            None,
            None,
            Some(dec!(51500)),
            Some(dec!(51500)),
            Some(dec!(50000)),
        );

        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].order_type, OrderType::Market);
        assert_eq!(core.trigger_orders.len(), 0);
    }

    #[test]
    fn test_stop_sell_triggers_when_low_crosses() {
        let mut core = OrderMatchingCore::new();
        let order = create_stop_sell(dec!(49000));

        core.add_order(order);

        // Price falls below trigger
        let triggered = core.check_triggers(
            None,
            None,
            Some(dec!(48500)),
            Some(dec!(50000)),
            Some(dec!(48500)),
        );

        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].order_type, OrderType::Market);
    }

    #[test]
    fn test_stop_no_trigger_when_price_not_crossed() {
        let mut core = OrderMatchingCore::new();
        let order = create_stop_buy(dec!(51000));

        core.add_order(order);

        // Price stays below trigger
        let triggered = core.check_triggers(
            None,
            None,
            Some(dec!(50500)),
            Some(dec!(50800)),
            Some(dec!(50000)),
        );

        assert!(triggered.is_empty());
        assert_eq!(core.trigger_orders.len(), 1);
    }

    #[test]
    fn test_market_order_fills_immediately() {
        let mut core = OrderMatchingCore::new();
        let order = create_market_buy();

        core.add_order(order);

        let results = core.match_orders(
            Some(dec!(49900)),
            Some(dec!(50100)),
            Some(dec!(50000)),
            None,
            None,
            Utc::now(),
        );

        assert_eq!(results.len(), 1);
        assert!(results[0].filled);
        assert_eq!(results[0].fill_price, dec!(50100)); // Fills at ask
        assert_eq!(results[0].liquidity_side, LiquiditySide::Taker);
    }

    #[test]
    fn test_multiple_orders_match() {
        let mut core = OrderMatchingCore::new();

        core.add_order(create_limit_buy(dec!(49000)));
        core.add_order(create_limit_buy(dec!(50000)));
        core.add_order(create_limit_sell(dec!(52000)));

        // Bar that touches all limits
        let results = core.match_orders(
            None,
            None,
            Some(dec!(50500)),
            Some(dec!(53000)),
            Some(dec!(48000)),
            Utc::now(),
        );

        assert_eq!(results.len(), 3);
        assert!(core.resting_orders.is_empty());
    }

    #[test]
    fn test_open_order_count() {
        let mut core = OrderMatchingCore::new();

        core.add_order(create_limit_buy(dec!(50000)));
        core.add_order(create_stop_buy(dec!(51000)));

        assert_eq!(core.open_order_count(), 2);
        assert_eq!(core.resting_orders.len(), 1);
        assert_eq!(core.trigger_orders.len(), 1);
    }

    #[test]
    fn test_has_order() {
        let mut core = OrderMatchingCore::new();
        let order = create_limit_buy(dec!(50000));
        let id = order.client_order_id.clone();

        assert!(!core.has_order(&id));

        core.add_order(order);

        assert!(core.has_order(&id));
    }

    #[test]
    fn test_clear() {
        let mut core = OrderMatchingCore::new();

        core.add_order(create_limit_buy(dec!(50000)));
        core.add_order(create_stop_buy(dec!(51000)));

        assert_eq!(core.open_order_count(), 2);

        core.clear();

        assert_eq!(core.open_order_count(), 0);
    }
}
