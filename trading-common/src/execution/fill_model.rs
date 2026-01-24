//! Fill models for simulating order execution during backtesting.
//!
//! Fill models determine how and when orders are filled based on market data.
//! Different models provide varying levels of realism:
//!
//! - `ImmediateFillModel`: Fills all orders immediately at current price
//! - `LimitOrderFillModel`: Respects limit prices, fills when price crosses
//! - `SlippageAwareFillModel`: Adds slippage based on order size
//!
//! # Example
//! ```ignore
//! let fill_model = SlippageAwareFillModel::new(
//!     Decimal::new(1, 3), // 0.1% base slippage
//!     Decimal::new(5, 4), // 0.05% per unit volume impact
//! );
//!
//! let fill = fill_model.get_fill(&order, &market_data)?;
//! ```

use chrono::{DateTime, Utc};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use std::fmt;
use std::sync::Mutex;

use crate::data::types::TickData;
use crate::orders::{LiquiditySide, Order, OrderSide, OrderType, TradeId};

/// Result of attempting to fill an order.
#[derive(Debug, Clone)]
pub struct FillResult {
    /// Whether a fill occurred
    pub filled: bool,
    /// Quantity filled (0 if no fill)
    pub fill_qty: Decimal,
    /// Fill price (None if no fill)
    pub fill_price: Option<Decimal>,
    /// Commission for this fill
    pub commission: Decimal,
    /// Whether this fill was maker or taker
    pub liquidity_side: LiquiditySide,
    /// Trade ID for tracking
    pub trade_id: TradeId,
    /// Timestamp of the fill
    pub fill_time: DateTime<Utc>,
    /// Slippage from target price (if applicable)
    pub slippage: Option<Decimal>,
}

impl FillResult {
    /// Create a result indicating no fill occurred.
    pub fn no_fill() -> Self {
        Self {
            filled: false,
            fill_qty: Decimal::ZERO,
            fill_price: None,
            commission: Decimal::ZERO,
            liquidity_side: LiquiditySide::None,
            trade_id: TradeId::generate(),
            fill_time: Utc::now(),
            slippage: None,
        }
    }

    /// Create a fill result.
    pub fn fill(
        fill_qty: Decimal,
        fill_price: Decimal,
        commission: Decimal,
        liquidity_side: LiquiditySide,
        fill_time: DateTime<Utc>,
    ) -> Self {
        Self {
            filled: true,
            fill_qty,
            fill_price: Some(fill_price),
            commission,
            liquidity_side,
            trade_id: TradeId::generate(),
            fill_time,
            slippage: None,
        }
    }

    /// Create a fill result with slippage information.
    pub fn fill_with_slippage(
        fill_qty: Decimal,
        fill_price: Decimal,
        commission: Decimal,
        liquidity_side: LiquiditySide,
        fill_time: DateTime<Utc>,
        slippage: Decimal,
    ) -> Self {
        Self {
            filled: true,
            fill_qty,
            fill_price: Some(fill_price),
            commission,
            liquidity_side,
            trade_id: TradeId::generate(),
            fill_time,
            slippage: Some(slippage),
        }
    }
}

/// Market data snapshot for fill evaluation.
#[derive(Debug, Clone)]
pub struct MarketSnapshot {
    /// Current timestamp
    pub timestamp: DateTime<Utc>,
    /// Last trade price
    pub last_price: Decimal,
    /// Best bid price (if available)
    pub bid_price: Option<Decimal>,
    /// Best ask price (if available)
    pub ask_price: Option<Decimal>,
    /// Volume at current price level
    pub volume: Option<Decimal>,
    /// High price of current bar
    pub high: Option<Decimal>,
    /// Low price of current bar
    pub low: Option<Decimal>,
    /// Open price of current bar
    pub open: Option<Decimal>,
}

impl MarketSnapshot {
    /// Create from a tick.
    pub fn from_tick(tick: &TickData) -> Self {
        Self {
            timestamp: tick.timestamp,
            last_price: tick.price,
            bid_price: None,
            ask_price: None,
            volume: Some(tick.quantity),
            high: None,
            low: None,
            open: None,
        }
    }

    /// Create with just a price.
    pub fn from_price(price: Decimal, timestamp: DateTime<Utc>) -> Self {
        Self {
            timestamp,
            last_price: price,
            bid_price: None,
            ask_price: None,
            volume: None,
            high: None,
            low: None,
            open: None,
        }
    }

    /// Create with OHLC data.
    pub fn from_ohlc(
        open: Decimal,
        high: Decimal,
        low: Decimal,
        close: Decimal,
        volume: Decimal,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            timestamp,
            last_price: close,
            bid_price: None,
            ask_price: None,
            volume: Some(volume),
            high: Some(high),
            low: Some(low),
            open: Some(open),
        }
    }

    /// Get the mid price if bid and ask are available.
    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.bid_price, self.ask_price) {
            (Some(bid), Some(ask)) => Some((bid + ask) / Decimal::TWO),
            _ => None,
        }
    }

    /// Get the spread if bid and ask are available.
    pub fn spread(&self) -> Option<Decimal> {
        match (self.bid_price, self.ask_price) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }
}

/// Trait for implementing fill models.
///
/// Fill models determine when and at what price orders are filled
/// during backtesting simulation.
pub trait FillModel: Send + Sync + fmt::Debug {
    /// Attempt to fill an order given current market data.
    ///
    /// Returns a `FillResult` indicating whether a fill occurred
    /// and the details of that fill.
    fn get_fill(
        &self,
        order: &Order,
        market: &MarketSnapshot,
        commission_rate: Decimal,
    ) -> FillResult;

    /// Get the name of this fill model.
    fn name(&self) -> &str;
}

/// Immediate fill model - fills all orders immediately at current price.
///
/// This is the simplest fill model and provides optimistic fills.
/// Market orders fill at last price, limit orders fill at limit price
/// regardless of market conditions.
#[derive(Debug, Clone, Default)]
pub struct ImmediateFillModel;

impl ImmediateFillModel {
    /// Create a new immediate fill model.
    pub fn new() -> Self {
        Self
    }
}

impl FillModel for ImmediateFillModel {
    fn get_fill(
        &self,
        order: &Order,
        market: &MarketSnapshot,
        commission_rate: Decimal,
    ) -> FillResult {
        // Only fill open orders
        if order.is_closed() {
            return FillResult::no_fill();
        }

        // Determine fill price based on order type
        let fill_price = match order.order_type {
            OrderType::Market => market.last_price,
            OrderType::Limit => order.price.unwrap_or(market.last_price),
            OrderType::Stop => {
                // Stop orders trigger when price crosses trigger level
                if let Some(trigger) = order.trigger_price {
                    match order.side {
                        OrderSide::Buy => {
                            if market.last_price >= trigger {
                                market.last_price
                            } else {
                                return FillResult::no_fill();
                            }
                        }
                        OrderSide::Sell => {
                            if market.last_price <= trigger {
                                market.last_price
                            } else {
                                return FillResult::no_fill();
                            }
                        }
                    }
                } else {
                    return FillResult::no_fill();
                }
            }
            OrderType::StopLimit => {
                // Stop-limit: check trigger first, then fill at limit
                if let (Some(trigger), Some(limit)) = (order.trigger_price, order.price) {
                    let triggered = match order.side {
                        OrderSide::Buy => market.last_price >= trigger,
                        OrderSide::Sell => market.last_price <= trigger,
                    };
                    if triggered {
                        limit
                    } else {
                        return FillResult::no_fill();
                    }
                } else {
                    return FillResult::no_fill();
                }
            }
            _ => market.last_price,
        };

        let fill_qty = order.leaves_qty;
        let notional = fill_qty * fill_price;
        let commission = notional * commission_rate;

        FillResult::fill(
            fill_qty,
            fill_price,
            commission,
            LiquiditySide::Taker,
            market.timestamp,
        )
    }

    fn name(&self) -> &str {
        "ImmediateFillModel"
    }
}

/// Limit-aware fill model - respects limit prices and OHLC ranges.
///
/// This model provides more realistic fills by:
/// - Only filling limit orders when price crosses the limit
/// - Using OHLC data to determine if limit price was touched
/// - Filling at limit price (not market price) for limit orders
#[derive(Debug, Clone, Default)]
pub struct LimitAwareFillModel;

impl LimitAwareFillModel {
    /// Create a new limit-aware fill model.
    pub fn new() -> Self {
        Self
    }

    /// Check if a limit order would be filled given the market snapshot.
    fn check_limit_fill(&self, order: &Order, market: &MarketSnapshot) -> Option<Decimal> {
        let limit_price = order.price?;

        // Use OHLC range if available, otherwise use last price
        let (price_low, price_high) = match (market.low, market.high) {
            (Some(low), Some(high)) => (low, high),
            _ => (market.last_price, market.last_price),
        };

        match order.side {
            OrderSide::Buy => {
                // Buy limit fills if price dips to or below limit
                if price_low <= limit_price {
                    Some(limit_price)
                } else {
                    None
                }
            }
            OrderSide::Sell => {
                // Sell limit fills if price rises to or above limit
                if price_high >= limit_price {
                    Some(limit_price)
                } else {
                    None
                }
            }
        }
    }

    /// Check if a stop order would be triggered.
    fn check_stop_trigger(&self, order: &Order, market: &MarketSnapshot) -> bool {
        let trigger = match order.trigger_price {
            Some(t) => t,
            None => return false,
        };

        let (price_low, price_high) = match (market.low, market.high) {
            (Some(low), Some(high)) => (low, high),
            _ => (market.last_price, market.last_price),
        };

        match order.side {
            // Buy stop triggers when price rises to trigger
            OrderSide::Buy => price_high >= trigger,
            // Sell stop triggers when price falls to trigger
            OrderSide::Sell => price_low <= trigger,
        }
    }
}

impl FillModel for LimitAwareFillModel {
    fn get_fill(
        &self,
        order: &Order,
        market: &MarketSnapshot,
        commission_rate: Decimal,
    ) -> FillResult {
        if order.is_closed() {
            return FillResult::no_fill();
        }

        let fill_price = match order.order_type {
            OrderType::Market => {
                // Market orders fill at current price
                Some(market.last_price)
            }
            OrderType::Limit => {
                // Limit orders only fill if price crosses limit
                self.check_limit_fill(order, market)
            }
            OrderType::Stop => {
                // Stop orders trigger then fill at market
                if self.check_stop_trigger(order, market) {
                    Some(market.last_price)
                } else {
                    None
                }
            }
            OrderType::StopLimit => {
                // Stop-limit: must trigger first, then respect limit
                if self.check_stop_trigger(order, market) {
                    self.check_limit_fill(order, market)
                } else {
                    None
                }
            }
            _ => Some(market.last_price),
        };

        match fill_price {
            Some(price) => {
                let fill_qty = order.leaves_qty;
                let notional = fill_qty * price;
                let commission = notional * commission_rate;

                let liquidity_side = match order.order_type {
                    OrderType::Limit => LiquiditySide::Maker,
                    _ => LiquiditySide::Taker,
                };

                FillResult::fill(
                    fill_qty,
                    price,
                    commission,
                    liquidity_side,
                    market.timestamp,
                )
            }
            None => FillResult::no_fill(),
        }
    }

    fn name(&self) -> &str {
        "LimitAwareFillModel"
    }
}

/// Slippage-aware fill model - adds price impact based on order size.
///
/// This model simulates market impact by:
/// - Adding slippage proportional to order size
/// - Using configurable base slippage and volume impact factors
/// - Differentiating between maker and taker fills
#[derive(Debug, Clone)]
pub struct SlippageAwareFillModel {
    /// Base slippage as a decimal (e.g., 0.001 = 0.1%)
    base_slippage: Decimal,
    /// Volume impact factor (slippage per unit of order size)
    volume_impact: Decimal,
    /// Maximum slippage cap as a decimal
    max_slippage: Decimal,
    /// Inner fill model for basic fill logic
    inner: LimitAwareFillModel,
}

impl SlippageAwareFillModel {
    /// Create a new slippage-aware fill model.
    ///
    /// # Arguments
    /// - `base_slippage`: Base slippage as decimal (e.g., 0.001 = 0.1%)
    /// - `volume_impact`: Additional slippage per unit volume
    pub fn new(base_slippage: Decimal, volume_impact: Decimal) -> Self {
        Self {
            base_slippage,
            volume_impact,
            max_slippage: Decimal::new(5, 2), // 5% max
            inner: LimitAwareFillModel::new(),
        }
    }

    /// Create with custom max slippage.
    pub fn with_max_slippage(mut self, max: Decimal) -> Self {
        self.max_slippage = max;
        self
    }

    /// Calculate slippage for an order.
    fn calculate_slippage(&self, order: &Order, base_price: Decimal) -> Decimal {
        // Slippage = base_slippage + (volume_impact * quantity)
        let raw_slippage = self.base_slippage + (self.volume_impact * order.quantity);
        let capped_slippage = raw_slippage.min(self.max_slippage);

        // Convert to price delta
        base_price * capped_slippage
    }
}

impl FillModel for SlippageAwareFillModel {
    fn get_fill(
        &self,
        order: &Order,
        market: &MarketSnapshot,
        commission_rate: Decimal,
    ) -> FillResult {
        // Get base fill from inner model
        let mut fill = self.inner.get_fill(order, market, commission_rate);

        if !fill.filled {
            return fill;
        }

        let base_price = fill.fill_price.unwrap();

        // Only apply slippage to market/taker orders
        if matches!(
            order.order_type,
            OrderType::Market | OrderType::Stop | OrderType::MarketToLimit
        ) {
            let slippage = self.calculate_slippage(order, base_price);

            // Apply slippage in the adverse direction
            let adjusted_price = match order.side {
                OrderSide::Buy => base_price + slippage,
                OrderSide::Sell => base_price - slippage,
            };

            // Recalculate commission
            let notional = fill.fill_qty * adjusted_price;
            let commission = notional * commission_rate;

            fill = FillResult::fill_with_slippage(
                fill.fill_qty,
                adjusted_price,
                commission,
                LiquiditySide::Taker,
                market.timestamp,
                slippage,
            );
        }

        fill
    }

    fn name(&self) -> &str {
        "SlippageAwareFillModel"
    }
}

/// Probabilistic fill model - adds randomness to fill decisions.
///
/// This model provides stochastic behavior for more realistic backtesting:
/// - Configurable probability that limit orders fill when price is touched
/// - Configurable probability and magnitude of slippage on market orders
/// - Seeded RNG for reproducible backtest results
///
/// # Example
/// ```ignore
/// let model = ProbabilisticFillModel::new(0.8, 0.2, 2, 42);
/// // 80% chance limit fills when touched
/// // 20% chance of slippage on market orders
/// // Up to 2 ticks of slippage
/// // Seed 42 for reproducibility
/// ```
pub struct ProbabilisticFillModel {
    /// Probability (0.0-1.0) that a limit order fills when price is touched
    prob_fill_on_limit: f64,
    /// Probability (0.0-1.0) of slippage on market orders
    prob_slippage: f64,
    /// Maximum slippage in price ticks
    max_slippage_ticks: u32,
    /// Price tick size for slippage calculation
    tick_size: Decimal,
    /// Seeded RNG for reproducibility (wrapped in Mutex for interior mutability)
    rng: Mutex<StdRng>,
    /// Current seed for display/debugging
    seed: u64,
    /// Inner fill model for basic limit/stop logic
    inner: LimitAwareFillModel,
}

impl fmt::Debug for ProbabilisticFillModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProbabilisticFillModel")
            .field("prob_fill_on_limit", &self.prob_fill_on_limit)
            .field("prob_slippage", &self.prob_slippage)
            .field("max_slippage_ticks", &self.max_slippage_ticks)
            .field("tick_size", &self.tick_size)
            .field("seed", &self.seed)
            .finish()
    }
}

impl ProbabilisticFillModel {
    /// Create a new probabilistic fill model.
    ///
    /// # Arguments
    /// - `prob_fill_on_limit`: Probability (0.0-1.0) limit fills when price touched
    /// - `prob_slippage`: Probability (0.0-1.0) of slippage on market orders
    /// - `max_slippage_ticks`: Maximum slippage in ticks
    /// - `seed`: RNG seed for reproducibility (0 = random seed)
    pub fn new(
        prob_fill_on_limit: f64,
        prob_slippage: f64,
        max_slippage_ticks: u32,
        seed: u64,
    ) -> Self {
        let actual_seed = if seed == 0 {
            rand::thread_rng().gen()
        } else {
            seed
        };

        Self {
            prob_fill_on_limit: prob_fill_on_limit.clamp(0.0, 1.0),
            prob_slippage: prob_slippage.clamp(0.0, 1.0),
            max_slippage_ticks,
            tick_size: Decimal::new(1, 2), // Default 0.01 tick size
            rng: Mutex::new(StdRng::seed_from_u64(actual_seed)),
            seed: actual_seed,
            inner: LimitAwareFillModel::new(),
        }
    }

    /// Set custom tick size for slippage calculation.
    pub fn with_tick_size(mut self, tick_size: Decimal) -> Self {
        self.tick_size = tick_size;
        self
    }

    /// Reset the RNG with a new seed.
    ///
    /// Call this at the start of each backtest run for reproducibility.
    pub fn set_seed(&self, seed: u64) {
        let actual_seed = if seed == 0 {
            rand::thread_rng().gen()
        } else {
            seed
        };
        let mut rng = self.rng.lock().unwrap();
        *rng = StdRng::seed_from_u64(actual_seed);
    }

    /// Get the current seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Check if limit order should fill based on probability.
    fn should_fill_limit(&self) -> bool {
        let mut rng = self.rng.lock().unwrap();
        rng.gen::<f64>() < self.prob_fill_on_limit
    }

    /// Check if slippage should be applied.
    fn should_apply_slippage(&self) -> bool {
        let mut rng = self.rng.lock().unwrap();
        rng.gen::<f64>() < self.prob_slippage
    }

    /// Calculate random slippage amount in ticks.
    fn random_slippage_ticks(&self) -> u32 {
        if self.max_slippage_ticks == 0 {
            return 0;
        }
        let mut rng = self.rng.lock().unwrap();
        rng.gen_range(1..=self.max_slippage_ticks)
    }
}

impl FillModel for ProbabilisticFillModel {
    fn get_fill(
        &self,
        order: &Order,
        market: &MarketSnapshot,
        commission_rate: Decimal,
    ) -> FillResult {
        if order.is_closed() {
            return FillResult::no_fill();
        }

        match order.order_type {
            OrderType::Market => {
                // Market orders always fill, but may have slippage
                let base_price = market.last_price;
                let (fill_price, slippage) = if self.should_apply_slippage() {
                    let slippage_ticks = self.random_slippage_ticks();
                    let slippage_amount = self.tick_size * Decimal::from(slippage_ticks);

                    let adjusted_price = match order.side {
                        OrderSide::Buy => base_price + slippage_amount,
                        OrderSide::Sell => base_price - slippage_amount,
                    };
                    (adjusted_price, Some(slippage_amount))
                } else {
                    (base_price, None)
                };

                let fill_qty = order.leaves_qty;
                let notional = fill_qty * fill_price;
                let commission = notional * commission_rate;

                if let Some(slip) = slippage {
                    FillResult::fill_with_slippage(
                        fill_qty,
                        fill_price,
                        commission,
                        LiquiditySide::Taker,
                        market.timestamp,
                        slip,
                    )
                } else {
                    FillResult::fill(
                        fill_qty,
                        fill_price,
                        commission,
                        LiquiditySide::Taker,
                        market.timestamp,
                    )
                }
            }
            OrderType::Limit => {
                // Check if price touched limit (using inner model's logic)
                let base_fill = self.inner.get_fill(order, market, commission_rate);

                if !base_fill.filled {
                    return FillResult::no_fill();
                }

                // Apply probabilistic fill decision
                if self.should_fill_limit() {
                    base_fill
                } else {
                    FillResult::no_fill()
                }
            }
            OrderType::Stop | OrderType::StopLimit => {
                // Stop orders use underlying logic for trigger
                // Apply slippage for stop market, probabilistic for stop-limit
                let base_fill = self.inner.get_fill(order, market, commission_rate);

                if !base_fill.filled {
                    return base_fill;
                }

                // Stop orders that trigger get market treatment (potential slippage)
                if order.order_type == OrderType::Stop && self.should_apply_slippage() {
                    let base_price = base_fill.fill_price.unwrap();
                    let slippage_ticks = self.random_slippage_ticks();
                    let slippage_amount = self.tick_size * Decimal::from(slippage_ticks);

                    let adjusted_price = match order.side {
                        OrderSide::Buy => base_price + slippage_amount,
                        OrderSide::Sell => base_price - slippage_amount,
                    };

                    let fill_qty = order.leaves_qty;
                    let notional = fill_qty * adjusted_price;
                    let commission = notional * commission_rate;

                    FillResult::fill_with_slippage(
                        fill_qty,
                        adjusted_price,
                        commission,
                        LiquiditySide::Taker,
                        market.timestamp,
                        slippage_amount,
                    )
                } else if order.order_type == OrderType::StopLimit {
                    // Stop-limit after trigger becomes limit - apply probabilistic fill
                    if self.should_fill_limit() {
                        base_fill
                    } else {
                        FillResult::no_fill()
                    }
                } else {
                    base_fill
                }
            }
            _ => {
                // Other order types: delegate to inner model
                self.inner.get_fill(order, market, commission_rate)
            }
        }
    }

    fn name(&self) -> &str {
        "ProbabilisticFillModel"
    }
}

/// Create a default fill model.
pub fn default_fill_model() -> Box<dyn FillModel> {
    Box::new(LimitAwareFillModel::new())
}

/// Create a probabilistic fill model with common defaults.
///
/// - 80% chance limit orders fill when touched
/// - 20% chance of slippage on market orders
/// - Up to 2 ticks of slippage
pub fn probabilistic_fill_model(seed: u64) -> Box<dyn FillModel> {
    Box::new(ProbabilisticFillModel::new(0.8, 0.2, 2, seed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::OrderSide;
    use rust_decimal_macros::dec;

    fn create_test_order(order_type: OrderType, side: OrderSide, price: Option<Decimal>) -> Order {
        let builder = match order_type {
            OrderType::Market => Order::market("BTCUSDT", side, dec!(1.0)),
            OrderType::Limit => Order::limit("BTCUSDT", side, dec!(1.0), price.unwrap()),
            OrderType::Stop => Order::stop("BTCUSDT", side, dec!(1.0), price.unwrap()),
            _ => Order::market("BTCUSDT", side, dec!(1.0)),
        };

        builder.build().unwrap()
    }

    fn create_market_snapshot(price: Decimal) -> MarketSnapshot {
        MarketSnapshot::from_price(price, Utc::now())
    }

    #[test]
    fn test_immediate_fill_market_order() {
        let model = ImmediateFillModel::new();
        let order = create_test_order(OrderType::Market, OrderSide::Buy, None);
        let market = create_market_snapshot(dec!(50000));

        let fill = model.get_fill(&order, &market, dec!(0.001));

        assert!(fill.filled);
        assert_eq!(fill.fill_price, Some(dec!(50000)));
        assert_eq!(fill.fill_qty, dec!(1.0));
    }

    #[test]
    fn test_limit_aware_buy_limit_fill() {
        let model = LimitAwareFillModel::new();

        // Buy limit at 49000, market at 50000 - should not fill
        let order = create_test_order(OrderType::Limit, OrderSide::Buy, Some(dec!(49000)));
        let market = MarketSnapshot::from_price(dec!(50000), Utc::now());
        let fill = model.get_fill(&order, &market, dec!(0.001));
        assert!(!fill.filled);

        // Market drops to 48500 - should fill at limit price
        let market_low = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(50100),
            dec!(48500),
            dec!(48800),
            dec!(100),
            Utc::now(),
        );
        let fill = model.get_fill(&order, &market_low, dec!(0.001));
        assert!(fill.filled);
        assert_eq!(fill.fill_price, Some(dec!(49000))); // Fills at limit
    }

    #[test]
    fn test_limit_aware_sell_limit_fill() {
        let model = LimitAwareFillModel::new();

        // Sell limit at 51000, market at 50000 - should not fill
        let order = create_test_order(OrderType::Limit, OrderSide::Sell, Some(dec!(51000)));
        let market = MarketSnapshot::from_price(dec!(50000), Utc::now());
        let fill = model.get_fill(&order, &market, dec!(0.001));
        assert!(!fill.filled);

        // Market rises to 51500 - should fill at limit price
        let market_high = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(51500),
            dec!(49800),
            dec!(51200),
            dec!(100),
            Utc::now(),
        );
        let fill = model.get_fill(&order, &market_high, dec!(0.001));
        assert!(fill.filled);
        assert_eq!(fill.fill_price, Some(dec!(51000))); // Fills at limit
    }

    #[test]
    fn test_slippage_aware_model() {
        let model = SlippageAwareFillModel::new(dec!(0.001), dec!(0.0001));
        let order = create_test_order(OrderType::Market, OrderSide::Buy, None);
        let market = create_market_snapshot(dec!(50000));

        let fill = model.get_fill(&order, &market, dec!(0.001));

        assert!(fill.filled);
        // Buy with slippage should be higher than base price
        assert!(fill.fill_price.unwrap() > dec!(50000));
        assert!(fill.slippage.is_some());
    }

    #[test]
    fn test_slippage_not_applied_to_limit() {
        let model = SlippageAwareFillModel::new(dec!(0.001), dec!(0.0001));
        let order = create_test_order(OrderType::Limit, OrderSide::Buy, Some(dec!(50000)));
        let market = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(50100),
            dec!(49900),
            dec!(50000),
            dec!(100),
            Utc::now(),
        );

        let fill = model.get_fill(&order, &market, dec!(0.001));

        assert!(fill.filled);
        // Limit order fills at limit price without slippage
        assert_eq!(fill.fill_price, Some(dec!(50000)));
        assert!(fill.slippage.is_none());
    }

    #[test]
    fn test_stop_order_trigger() {
        let model = LimitAwareFillModel::new();

        // Buy stop at 51000, market at 50000 - should not trigger
        let order = create_test_order(OrderType::Stop, OrderSide::Buy, Some(dec!(51000)));
        let market = MarketSnapshot::from_price(dec!(50000), Utc::now());
        let fill = model.get_fill(&order, &market, dec!(0.001));
        assert!(!fill.filled);

        // Market rises to 51500 - should trigger and fill
        let market_high = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(51500),
            dec!(49800),
            dec!(51200),
            dec!(100),
            Utc::now(),
        );
        let fill = model.get_fill(&order, &market_high, dec!(0.001));
        assert!(fill.filled);
    }

    #[test]
    fn test_commission_calculation() {
        let model = ImmediateFillModel::new();
        let order = create_test_order(OrderType::Market, OrderSide::Buy, None);
        let market = create_market_snapshot(dec!(50000));

        let fill = model.get_fill(&order, &market, dec!(0.001)); // 0.1% commission

        // Commission = 1.0 * 50000 * 0.001 = 50
        assert_eq!(fill.commission, dec!(50));
    }

    #[test]
    fn test_market_snapshot_helpers() {
        let snapshot = MarketSnapshot {
            timestamp: Utc::now(),
            last_price: dec!(50000),
            bid_price: Some(dec!(49990)),
            ask_price: Some(dec!(50010)),
            volume: Some(dec!(100)),
            high: Some(dec!(50100)),
            low: Some(dec!(49900)),
            open: Some(dec!(49950)),
        };

        assert_eq!(snapshot.mid_price(), Some(dec!(50000)));
        assert_eq!(snapshot.spread(), Some(dec!(20)));
    }

    #[test]
    fn test_probabilistic_fill_model_market_order() {
        // With seed, behavior is deterministic
        let model = ProbabilisticFillModel::new(0.8, 0.5, 2, 42);
        let order = create_test_order(OrderType::Market, OrderSide::Buy, None);
        let market = create_market_snapshot(dec!(50000));

        let fill = model.get_fill(&order, &market, dec!(0.001));

        // Market orders always fill
        assert!(fill.filled);
        assert!(fill.fill_price.is_some());
    }

    #[test]
    fn test_probabilistic_fill_model_reproducibility() {
        // Same seed should produce same sequence of decisions
        let model1 = ProbabilisticFillModel::new(0.5, 0.5, 2, 12345);
        let model2 = ProbabilisticFillModel::new(0.5, 0.5, 2, 12345);

        let order = create_test_order(OrderType::Limit, OrderSide::Buy, Some(dec!(50000)));
        let market = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(50100),
            dec!(49900),
            dec!(50000),
            dec!(100),
            Utc::now(),
        );

        // Run multiple fills and compare
        let mut results1 = Vec::new();
        let mut results2 = Vec::new();

        for _ in 0..10 {
            results1.push(model1.get_fill(&order, &market, dec!(0.001)).filled);
            results2.push(model2.get_fill(&order, &market, dec!(0.001)).filled);
        }

        // Same seed = same sequence
        assert_eq!(results1, results2);
    }

    #[test]
    fn test_probabilistic_fill_model_limit_probability() {
        // prob_fill_on_limit = 0.0 means limit never fills even when touched
        let model_never = ProbabilisticFillModel::new(0.0, 0.0, 0, 42);
        // prob_fill_on_limit = 1.0 means limit always fills when touched
        let model_always = ProbabilisticFillModel::new(1.0, 0.0, 0, 42);

        let order = create_test_order(OrderType::Limit, OrderSide::Buy, Some(dec!(50000)));
        let market = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(50100),
            dec!(49900),
            dec!(50000),
            dec!(100),
            Utc::now(),
        );

        // With prob=0, should never fill
        let fill = model_never.get_fill(&order, &market, dec!(0.001));
        assert!(!fill.filled);

        // With prob=1, should always fill
        let fill = model_always.get_fill(&order, &market, dec!(0.001));
        assert!(fill.filled);
        assert_eq!(fill.fill_price, Some(dec!(50000)));
    }

    #[test]
    fn test_probabilistic_fill_model_slippage() {
        // prob_slippage = 1.0 means always slippage
        let model = ProbabilisticFillModel::new(1.0, 1.0, 3, 42).with_tick_size(dec!(0.01));

        let order = create_test_order(OrderType::Market, OrderSide::Buy, None);
        let market = create_market_snapshot(dec!(100.00));

        let fill = model.get_fill(&order, &market, dec!(0.001));

        assert!(fill.filled);
        // Buy with slippage should be higher than base price
        let price = fill.fill_price.unwrap();
        assert!(price > dec!(100.00));
        // But within max slippage (3 ticks * 0.01 = 0.03)
        assert!(price <= dec!(100.03));
        assert!(fill.slippage.is_some());
    }

    #[test]
    fn test_probabilistic_fill_model_no_slippage() {
        // prob_slippage = 0.0 means no slippage
        let model = ProbabilisticFillModel::new(1.0, 0.0, 5, 42);

        let order = create_test_order(OrderType::Market, OrderSide::Buy, None);
        let market = create_market_snapshot(dec!(50000));

        let fill = model.get_fill(&order, &market, dec!(0.001));

        assert!(fill.filled);
        assert_eq!(fill.fill_price, Some(dec!(50000)));
        assert!(fill.slippage.is_none());
    }

    #[test]
    fn test_probabilistic_fill_model_set_seed() {
        let model = ProbabilisticFillModel::new(0.5, 0.5, 2, 1);

        let order = create_test_order(OrderType::Limit, OrderSide::Buy, Some(dec!(50000)));
        let market = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(50100),
            dec!(49900),
            dec!(50000),
            dec!(100),
            Utc::now(),
        );

        // Get initial sequence
        let mut initial_results = Vec::new();
        for _ in 0..5 {
            initial_results.push(model.get_fill(&order, &market, dec!(0.001)).filled);
        }

        // Reset seed
        model.set_seed(1);

        // Should get same sequence again
        let mut reset_results = Vec::new();
        for _ in 0..5 {
            reset_results.push(model.get_fill(&order, &market, dec!(0.001)).filled);
        }

        assert_eq!(initial_results, reset_results);
    }

    #[test]
    fn test_probabilistic_fill_model_stop_order_slippage() {
        // Stop orders that trigger can have slippage
        let model = ProbabilisticFillModel::new(1.0, 1.0, 2, 42).with_tick_size(dec!(1.0));

        // Sell stop at 49000
        let mut order = create_test_order(OrderType::Stop, OrderSide::Sell, Some(dec!(49000)));
        order.trigger_price = Some(dec!(49000));

        // Market drops below trigger
        let market = MarketSnapshot::from_ohlc(
            dec!(50000),
            dec!(50000),
            dec!(48500),
            dec!(48700),
            dec!(100),
            Utc::now(),
        );

        let fill = model.get_fill(&order, &market, dec!(0.001));

        assert!(fill.filled);
        // Sell with slippage should be lower than market price
        let price = fill.fill_price.unwrap();
        assert!(price < dec!(48700));
        assert!(fill.slippage.is_some());
    }
}
