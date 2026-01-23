# Strategy Developer Guide

This guide documents all available methods and interfaces for developing trading strategies in rust-trade. Examples are provided in both Rust and Python.

---

## Table of Contents

1. [Strategy Trait Interface](#1-strategy-trait-interface)
2. [Data Structures](#2-data-structures)
3. [BarsContext - Price Series Access](#3-barscontext---price-series-access)
4. [Signal Generation](#4-signal-generation)
5. [Order Management System](#5-order-management-system)
6. [Event-Driven Methods](#6-event-driven-methods)
7. [Portfolio Management](#7-portfolio-management)
8. [Python Strategy Development](#8-python-strategy-development)
9. [Complete Examples](#9-complete-examples)
10. [Best Practices](#10-best-practices)

---

## 1. Strategy Trait Interface

The `Strategy` trait defines the core interface for all trading strategies.

### Required Methods

| Method | Description |
|--------|-------------|
| `on_bar_data()` | Primary signal generation - called for each bar event |
| `initialize()` | Parse and validate strategy parameters |
| `is_ready()` | Check if strategy has sufficient data to generate signals |
| `warmup_period()` | Return minimum bars needed before ready |

### Optional Methods (with defaults)

| Method | Default | Description |
|--------|---------|-------------|
| `bar_data_mode()` | `OnCloseBar` | When to fire events (tick/price change/bar close) |
| `preferred_bar_type()` | `TimeBased(1m)` | Bar type (time-based or tick-based) |
| `max_bars_lookback()` | `Fixed(256)` | How much history to keep |
| `reset()` | no-op | Reset strategy state |
| `uses_order_management()` | `false` | Enable advanced order management |
| `get_orders()` | empty vec | Submit orders directly |
| `get_cancellations()` | empty vec | Cancel pending orders |
| `on_order_filled()` | no-op | Handle fill events (legacy) |
| `on_order_rejected()` | no-op | Handle rejection events |
| `on_order_canceled()` | no-op | Handle cancellation events |
| `on_order_update()` | no-op | Handle any order state change |
| `on_execution()` | no-op | Handle execution/fill details |
| `on_position_update()` | no-op | Handle position changes |
| `on_account_update()` | no-op | Handle account changes |
| `on_marketdata_update()` | no-op | Handle market data updates |
| `on_state_change()` | no-op | Handle strategy lifecycle events |
| `on_session_update()` | no-op | Handle trading session events |

### Rust Example

```rust
use trading_common::backtest::strategy::{Strategy, Signal, BarDataMode, BarType};
use trading_common::data::types::BarData;
use trading_common::series::BarsContext;
use trading_common::data::Timeframe;
use std::collections::HashMap;

pub struct MyStrategy {
    period: usize,
}

impl Strategy for MyStrategy {
    fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
        if !self.is_ready(bars) {
            return Signal::Hold;
        }
        // Strategy logic here
        Signal::Hold
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        if let Some(p) = params.get("period") {
            self.period = p.parse().map_err(|_| "Invalid period")?;
        }
        Ok(())
    }

    fn is_ready(&self, bars: &BarsContext) -> bool {
        bars.is_ready_for(self.period)
    }

    fn warmup_period(&self) -> usize {
        self.period
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }
}
```

### Python Example

```python
from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext
from typing import Dict, Any, Optional

class MyStrategy(BaseStrategy):
    def __init__(self):
        self.period = 20

    def name(self) -> str:
        return "My Strategy"

    def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
        if not self.is_ready(bars):
            return Signal.hold()
        # Strategy logic here
        return Signal.hold()

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        if "period" in params:
            self.period = int(params["period"])
        return None  # Return error string on failure

    def is_ready(self, bars: BarsContext) -> bool:
        return bars.is_ready_for(self.period)

    def warmup_period(self) -> int:
        return self.period

    def bar_data_mode(self) -> str:
        return "OnCloseBar"  # or "OnEachTick", "OnPriceMove"

    def preferred_bar_type(self) -> Dict[str, Any]:
        return {"type": "TimeBased", "timeframe": "1m"}
```

---

## 2. Data Structures

### BarData

Contains the current bar state and optional tick data.

| Field | Type | Description |
|-------|------|-------------|
| `current_tick` | `Option<TickData>` | Current tick (None in OnCloseBar mode) |
| `ohlc_bar` | `OHLCData` | Current OHLC state |
| `metadata` | `BarMetadata` | Bar metadata (is_closed, tick_count, etc.) |

#### Rust

```rust
fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    // Access OHLC
    let close = bar_data.ohlc_bar.close;
    let high = bar_data.ohlc_bar.high;
    let low = bar_data.ohlc_bar.low;
    let open = bar_data.ohlc_bar.open;
    let volume = bar_data.ohlc_bar.volume;
    let symbol = &bar_data.ohlc_bar.symbol;

    // Check if bar is closed
    if bar_data.metadata.is_closed {
        // Bar just closed
    }

    // Access current tick (only in OnEachTick/OnPriceMove modes)
    if let Some(tick) = &bar_data.current_tick {
        let tick_price = tick.price;
        let tick_qty = tick.quantity;
        let tick_side = &tick.side;  // TradeSide::Buy or TradeSide::Sell
    }

    Signal::Hold
}
```

#### Python

```python
def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
    # Access OHLC
    close = bar_data["close"]
    high = bar_data["high"]
    low = bar_data["low"]
    open_price = bar_data["open"]
    volume = bar_data["volume"]
    symbol = bar_data["symbol"]
    timestamp = bar_data["timestamp"]

    # Check if bar is closed
    if bar_data.get("is_closed", False):
        # Bar just closed
        pass

    # Access current tick (only in OnEachTick/OnPriceMove modes)
    if "current_tick" in bar_data:
        tick = bar_data["current_tick"]
        tick_price = tick["price"]
        tick_qty = tick["quantity"]

    return Signal.hold()
```

### OHLCData

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `DateTime<Utc>` | Bar timestamp |
| `symbol` | `String` | Trading symbol |
| `timeframe` | `Timeframe` | Bar timeframe |
| `open` | `Decimal` | Open price |
| `high` | `Decimal` | High price |
| `low` | `Decimal` | Low price |
| `close` | `Decimal` | Close price |
| `volume` | `Decimal` | Total volume |
| `trade_count` | `u64` | Number of trades |

### TickData

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `DateTime<Utc>` | Trade timestamp |
| `symbol` | `String` | Trading symbol |
| `price` | `Decimal` | Trade price |
| `quantity` | `Decimal` | Trade quantity |
| `side` | `TradeSide` | Buy or Sell |
| `trade_id` | `String` | Exchange trade ID |
| `is_buyer_maker` | `bool` | True if buyer was maker |

### BarDataMode

Controls when `on_bar_data()` is called.

| Mode | Description | `current_tick` |
|------|-------------|----------------|
| `OnEachTick` | Fire on every tick | `Some(tick)` |
| `OnPriceMove` | Fire when price changes | `Some(tick)` |
| `OnCloseBar` | Fire only when bar closes | `None` |

### BarType

| Type | Description |
|------|-------------|
| `TimeBased(Timeframe)` | Time-based bars (1m, 5m, 15m, 30m, 1h, 4h, 1d, 1w) |
| `TickBased(u32)` | N-tick bars (e.g., 100-tick, 500-tick) |

### Timeframe

| Value | Duration |
|-------|----------|
| `OneMinute` | 1 minute |
| `FiveMinutes` | 5 minutes |
| `FifteenMinutes` | 15 minutes |
| `ThirtyMinutes` | 30 minutes |
| `OneHour` | 1 hour |
| `FourHours` | 4 hours |
| `OneDay` | 1 day |
| `OneWeek` | 1 week |

---

## 3. BarsContext - Price Series Access

`BarsContext` provides access to OHLCV price series with **reverse indexing**:
- `bars.close[0]` = current bar (most recent)
- `bars.close[1]` = previous bar
- `bars.close[10]` = 10 bars ago

### OHLCV Series

| Series | Description |
|--------|-------------|
| `bars.open` | Open prices |
| `bars.high` | High prices |
| `bars.low` | Low prices |
| `bars.close` | Close prices |
| `bars.volume` | Volume |
| `bars.time` | Bar timestamps |
| `bars.trade_count` | Trade count per bar |

### Readiness Methods

| Method | Description |
|--------|-------------|
| `is_ready()` | Has at least 1 bar |
| `is_ready_for(n)` | Has >= n bars |
| `has_bars(n)` | Has >= n bars |
| `count()` | Number of bars available |

#### Rust

```rust
fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    // Check readiness
    if !bars.is_ready_for(20) {
        return Signal::Hold;
    }

    // Access price series (reverse indexing)
    let current_close = bars.close[0];
    let previous_close = bars.close[1];
    let close_10_bars_ago = bars.close[10];

    // Access other series
    let current_high = bars.high[0];
    let current_low = bars.low[0];
    let current_volume = bars.volume[0];

    Signal::Hold
}
```

#### Python

```python
def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
    # Check readiness
    if not bars.is_ready_for(20):
        return Signal.hold()

    # Access price series (reverse indexing)
    current_close = bars.close(0)
    previous_close = bars.close(1)
    close_10_bars_ago = bars.close(10)

    # Access other series
    current_high = bars.high(0)
    current_low = bars.low(0)
    current_volume = bars.volume(0)

    return Signal.hold()
```

### Built-in Indicators

| Method | Description |
|--------|-------------|
| `sma(period)` | Simple Moving Average of close prices |
| `sma_of(series, period)` | SMA of any series |
| `highest_high(period)` | Highest high in period |
| `lowest_low(period)` | Lowest low in period |
| `highest_close(period)` | Highest close in period |
| `lowest_close(period)` | Lowest close in period |
| `range()` | Current bar range (high - low) |
| `average_range(period)` | Average range over period |

#### Rust

```rust
fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    if !bars.is_ready_for(50) {
        return Signal::Hold;
    }

    // Simple Moving Average
    let sma_20 = bars.sma(20);
    let sma_50 = bars.sma(50);

    // High/Low lookback
    let highest_14 = bars.highest_high(14);
    let lowest_14 = bars.lowest_low(14);

    // Average range (ATR-like)
    let avg_range = bars.average_range(14);

    // Ready check with value (combined)
    let (sma_value, is_ready) = bars.sma_with_ready(50);
    if is_ready {
        if let Some(sma) = sma_value {
            // Use SMA
        }
    }

    // SMA crossover example
    if let (Some(short), Some(long)) = (sma_20, sma_50) {
        if short > long {
            return Signal::buy(bars.symbol(), Decimal::from(100));
        }
    }

    Signal::Hold
}
```

#### Python

```python
def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
    if not bars.is_ready_for(50):
        return Signal.hold()

    # Simple Moving Average
    sma_20 = bars.sma(20)
    sma_50 = bars.sma(50)

    # High/Low lookback
    highest_14 = bars.highest_high(14)
    lowest_14 = bars.lowest_low(14)

    # Average range
    avg_range = bars.average_range(14)

    # SMA crossover example
    if sma_20 is not None and sma_50 is not None:
        if sma_20 > sma_50:
            return Signal.buy(bar_data["symbol"], "100")

    return Signal.hold()
```

### Bar Analysis Methods

| Method | Description |
|--------|-------------|
| `is_up_bar()` | close > open |
| `is_down_bar()` | close < open |
| `change()` | Current - Previous close |
| `percent_change()` | Percentage change from previous |
| `symbol()` | Get the symbol name |
| `current_bar()` | Current bar index |

#### Rust

```rust
fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    // Bar analysis
    if bars.is_up_bar() {
        // Bullish bar
    }

    if bars.is_down_bar() {
        // Bearish bar
    }

    // Price change
    if let Some(change) = bars.change() {
        println!("Price change: {}", change);
    }

    if let Some(pct) = bars.percent_change() {
        if pct > Decimal::from(2) {
            // Price up more than 2%
        }
    }

    // Get symbol
    let symbol = bars.symbol();

    // Get bar count
    let bar_number = bars.current_bar();

    Signal::Hold
}
```

#### Python

```python
def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
    # Bar analysis
    if bars.is_up_bar():
        # Bullish bar
        pass

    if bars.is_down_bar():
        # Bearish bar
        pass

    # Price change
    change = bars.change()
    if change is not None:
        print(f"Price change: {change}")

    pct = bars.percent_change()
    if pct is not None and pct > 2.0:
        # Price up more than 2%
        pass

    # Get symbol
    symbol = bars.symbol()

    # Get bar count
    bar_number = bars.current_bar()

    return Signal.hold()
```

---

## 4. Signal Generation

### Signal Enum

| Variant | Description |
|---------|-------------|
| `Buy { symbol, quantity }` | Generate buy signal |
| `Sell { symbol, quantity }` | Generate sell signal |
| `Hold` | No action |

### Signal Methods

| Method | Description |
|--------|-------------|
| `Signal::buy(symbol, qty)` | Create buy signal |
| `Signal::sell(symbol, qty)` | Create sell signal |
| `is_buy()` | Check if buy signal |
| `is_sell()` | Check if sell signal |
| `is_hold()` | Check if hold |
| `symbol()` | Get symbol (Option) |
| `quantity()` | Get quantity (Option) |

#### Rust

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) -> Signal {
    let symbol = bars.symbol().to_string();

    // Create buy signal
    Signal::buy(&symbol, dec!(0.1))

    // Create sell signal
    Signal::sell(&symbol, dec!(0.1))

    // Or using struct syntax
    Signal::Buy {
        symbol: symbol.clone(),
        quantity: Decimal::from(100),
    }

    Signal::Sell {
        symbol: symbol.clone(),
        quantity: Decimal::from(100),
    }

    // Hold (no action)
    Signal::Hold
}
```

#### Python

```python
def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
    symbol = bar_data["symbol"]

    # Create buy signal
    return Signal.buy(symbol, "0.1")

    # Create sell signal
    return Signal.sell(symbol, "0.1")

    # Hold (no action)
    return Signal.hold()
```

---

## 5. Order Management System

For advanced strategies that need more control than simple signals.

### Enable Order Management

#### Rust

```rust
impl Strategy for MyStrategy {
    fn uses_order_management(&self) -> bool {
        true  // Enable order management mode
    }

    fn get_orders(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<Order> {
        // Return orders to submit
        vec![]
    }

    fn get_cancellations(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<ClientOrderId> {
        // Return order IDs to cancel
        vec![]
    }
}
```

### Order Types

| Type | Description |
|------|-------------|
| `Market` | Execute at market price |
| `Limit` | Execute at limit price or better |
| `Stop` | Trigger market order at stop price |
| `StopLimit` | Trigger limit order at stop price |
| `TrailingStop` | Stop with trailing offset |

### Creating Orders (Rust)

```rust
use trading_common::orders::{Order, OrderSide, TimeInForce};
use rust_decimal_macros::dec;

// Market order
let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1))
    .with_strategy_id("my-strategy")
    .build()?;

// Limit order
let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.1), dec!(50000))
    .with_time_in_force(TimeInForce::GTC)
    .with_strategy_id("my-strategy")
    .build()?;

// Stop order
let order = Order::stop("BTCUSDT", OrderSide::Sell, dec!(0.1), dec!(45000))
    .with_strategy_id("my-strategy")
    .build()?;

// Stop-limit order
let order = Order::stop_limit(
    "BTCUSDT",
    OrderSide::Sell,
    dec!(0.1),
    dec!(44000),  // Limit price
    dec!(45000),  // Stop/trigger price
)
    .with_strategy_id("my-strategy")
    .build()?;

// Trailing stop
let order = Order::trailing_stop(
    "BTCUSDT",
    OrderSide::Sell,
    dec!(0.1),
    dec!(500),  // Offset amount
    TrailingOffsetType::Price,
)
    .with_strategy_id("my-strategy")
    .build()?;
```

### Order Builder Methods

| Method | Description |
|--------|-------------|
| `with_venue(venue)` | Set exchange venue |
| `with_price(price)` | Set limit price |
| `with_trigger_price(price)` | Set stop/trigger price |
| `with_time_in_force(tif)` | Set time in force |
| `with_expire_time(time)` | Set expiration (for GTD) |
| `with_strategy_id(id)` | Set strategy identifier |
| `with_client_order_id(id)` | Set custom order ID |
| `reduce_only()` | Mark as reduce-only |
| `post_only()` | Mark as post-only (maker) |
| `with_tag(key, value)` | Add custom tag |
| `build()` | Build the order |

### Time in Force

| Value | Description |
|-------|-------------|
| `GTC` | Good-Till-Canceled |
| `IOC` | Immediate-Or-Cancel |
| `FOK` | Fill-Or-Kill |
| `GTD` | Good-Till-Date |
| `Day` | Day order |

### Order Events

Handle order lifecycle events:

#### Rust

```rust
impl Strategy for MyStrategy {
    fn on_order_filled(&mut self, event: &OrderFilled) {
        println!(
            "Order {} filled: {} @ {} (qty: {})",
            event.client_order_id,
            event.instrument_id.symbol,
            event.last_px,
            event.last_qty
        );

        // Check if fully filled
        if event.is_last_fill() {
            // Order completely filled
        }

        // Access fill details
        let commission = event.commission;
        let liquidity = &event.liquidity_side;  // Maker or Taker
    }

    fn on_order_rejected(&mut self, event: &OrderRejected) {
        println!(
            "Order {} rejected: {}",
            event.client_order_id,
            event.reason
        );
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        println!("Order {} canceled", event.client_order_id);
    }

    fn on_order_submitted(&mut self, order: &Order) {
        println!("Order {} submitted", order.client_order_id);
    }
}
```

### Order Status

| Status | Description |
|--------|-------------|
| `Initialized` | Order created |
| `Submitted` | Sent to venue |
| `Accepted` | Accepted by venue |
| `Rejected` | Rejected by venue |
| `PartiallyFilled` | Partially executed |
| `Filled` | Fully executed |
| `Canceled` | Canceled |
| `Expired` | Expired (GTD/Day) |

### Order Query Methods

```rust
// State queries
order.is_open()          // Still active
order.is_closed()        // Terminal state
order.is_filled()        // Completely filled
order.is_partially_filled()
order.is_cancelable()
order.fill_percent()     // Percent filled

// Value calculations
order.notional()         // quantity * price
order.filled_notional()  // filled_qty * avg_px
```

### Complete Order Management Example (Rust)

```rust
pub struct OrderManagementStrategy {
    pending_orders: HashMap<ClientOrderId, Order>,
    position: Decimal,
}

impl Strategy for OrderManagementStrategy {
    fn uses_order_management(&self) -> bool {
        true
    }

    fn get_orders(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<Order> {
        let mut orders = Vec::new();

        if !bars.is_ready_for(20) {
            return orders;
        }

        let sma = bars.sma(20).unwrap();
        let close = bars.close[0];
        let symbol = bars.symbol();

        // Entry logic
        if self.position == Decimal::ZERO && close > sma {
            // Submit limit buy below current price
            let entry_price = close * dec!(0.999);
            let order = Order::limit(symbol, OrderSide::Buy, dec!(0.1), entry_price)
                .with_strategy_id("order-mgmt")
                .with_time_in_force(TimeInForce::GTC)
                .build()
                .unwrap();

            self.pending_orders.insert(order.client_order_id.clone(), order.clone());
            orders.push(order);
        }

        orders
    }

    fn get_cancellations(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<ClientOrderId> {
        let mut cancellations = Vec::new();

        // Cancel stale orders
        for (id, order) in &self.pending_orders {
            // Cancel if price moved too far
            if let Some(price) = order.price {
                let close = bars.close[0];
                if (close - price).abs() / price > dec!(0.02) {
                    cancellations.push(id.clone());
                }
            }
        }

        cancellations
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        self.pending_orders.remove(&event.client_order_id);

        match event.order_side {
            OrderSide::Buy => self.position += event.last_qty,
            OrderSide::Sell => self.position -= event.last_qty,
        }
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        self.pending_orders.remove(&event.client_order_id);
    }

    // ... other required methods
}
```

---

## 6. Event-Driven Methods

NinjaTrader-style event-driven methods for reacting to order lifecycle, position changes, account updates, and more.

### Event Ordering Guarantee

For fill events that affect positions, the following order is **guaranteed**:

```
on_order_update() → on_execution() → on_position_update()
```

This ensures strategies see the order state change before processing the fill details, and see the fill before processing the resulting position change.

### Method Reference

| Method | Fires When | Key Data |
|--------|------------|----------|
| `on_order_update()` | Any order state change | Order event type, client/venue IDs |
| `on_execution()` | Order fill (partial/complete) | Fill price, quantity, commission |
| `on_position_update()` | Position changes | Side, quantity, P&L, avg entry |
| `on_account_update()` | Account balance/state changes | Balance, margin, state |
| `on_marketdata_update()` | Market data updates | Tick, quote, order book |
| `on_state_change()` | Strategy lifecycle transitions | Old/new state, reason |
| `on_session_update()` | Trading session changes | Session open/close, halts |

### on_order_update()

Called on **any** order state change. Always fires **before** `on_execution()` and `on_position_update()`.

#### Rust

```rust
use trading_common::orders::OrderEventAny;

fn on_order_update(&mut self, event: &OrderEventAny) {
    match event {
        OrderEventAny::Submitted(e) => {
            println!("Order {} submitted", e.client_order_id);
        }
        OrderEventAny::Accepted(e) => {
            println!("Order {} accepted by venue: {}",
                e.client_order_id, e.venue_order_id);
        }
        OrderEventAny::Rejected(e) => {
            println!("Order {} rejected: {}", e.client_order_id, e.reason);
            // Handle rejection - maybe retry with different params
        }
        OrderEventAny::Canceled(e) => {
            println!("Order {} canceled", e.client_order_id);
        }
        OrderEventAny::Filled(e) => {
            println!("Order {} filled (details in on_execution)", e.client_order_id);
        }
        OrderEventAny::Expired(e) => {
            println!("Order {} expired", e.client_order_id);
        }
        _ => {}
    }
}
```

#### Python

```python
def on_order_update(self, event: Dict[str, Any]):
    event_type = event.get("event_type")
    client_order_id = event.get("client_order_id")

    if event_type == "Submitted":
        print(f"Order {client_order_id} submitted")
    elif event_type == "Accepted":
        venue_order_id = event.get("venue_order_id")
        print(f"Order {client_order_id} accepted: {venue_order_id}")
    elif event_type == "Rejected":
        reason = event.get("reason", "unknown")
        print(f"Order {client_order_id} rejected: {reason}")
    elif event_type == "Canceled":
        print(f"Order {client_order_id} canceled")
    elif event_type == "Filled":
        print(f"Order {client_order_id} filled")
```

### on_execution()

Called on order fills (partial or complete). Fires **after** `on_order_update()`, **before** `on_position_update()`.

#### Rust

```rust
use trading_common::orders::OrderFilled;

fn on_execution(&mut self, event: &OrderFilled) {
    println!("Execution: {} {} @ {} (commission: {})",
        event.order_side,
        event.last_qty,
        event.last_px,
        event.commission
    );

    // Check if order is fully filled
    if event.leaves_qty.is_zero() {
        println!("Order {} fully filled. Total qty: {}",
            event.client_order_id, event.cum_qty);
    } else {
        println!("Partial fill. Remaining: {}", event.leaves_qty);
    }

    // Track slippage
    if let Some(expected_price) = self.expected_prices.get(&event.client_order_id) {
        let slippage = event.last_px - expected_price;
        println!("Slippage: {}", slippage);
    }
}
```

#### Python

```python
def on_execution(self, event: Dict[str, Any]):
    print(f"Execution: {event['order_side']} {event['last_qty']} @ {event['last_px']}")
    print(f"Commission: {event['commission']} {event['commission_currency']}")

    # Check if fully filled
    if event.get("is_last_fill", False):
        print(f"Order {event['client_order_id']} fully filled")
        print(f"Total quantity: {event['cum_qty']}")
    else:
        print(f"Partial fill. Remaining: {event['leaves_qty']}")
```

### on_position_update()

Called when positions change. Always fires **after** `on_order_update()` and `on_execution()` for fill events.

#### Rust

```rust
use trading_common::backtest::strategy::PositionEvent;
use trading_common::orders::PositionSide;

fn on_position_update(&mut self, event: &PositionEvent) {
    match event.side {
        PositionSide::Long => {
            println!("Long {} {} @ {} | Unrealized P&L: {}",
                event.quantity,
                event.symbol(),
                event.avg_entry_price,
                event.unrealized_pnl
            );
        }
        PositionSide::Short => {
            println!("Short {} {} @ {} | Unrealized P&L: {}",
                event.quantity.abs(),
                event.symbol(),
                event.avg_entry_price,
                event.unrealized_pnl
            );
        }
        PositionSide::Flat => {
            println!("Position closed. Realized P&L: {} (net: {})",
                event.realized_pnl,
                event.net_pnl()
            );
        }
    }

    // Access computed values
    let notional = event.notional();      // quantity * avg_entry_price
    let total_pnl = event.total_pnl();    // realized + unrealized
    let net_pnl = event.net_pnl();        // total - commission

    // Position state checks
    if event.is_open() { /* has position */ }
    if event.is_flat() { /* no position */ }
    if event.is_long() { /* long position */ }
    if event.is_short() { /* short position */ }
}
```

#### Python

```python
def on_position_update(self, event: Dict[str, Any]):
    side = event.get("side")
    symbol = event.get("symbol")
    quantity = event.get("quantity")
    avg_entry = event.get("avg_entry_price")

    if side == "Long":
        print(f"Long {quantity} {symbol} @ {avg_entry}")
        print(f"Unrealized P&L: {event['unrealized_pnl']}")
    elif side == "Short":
        print(f"Short {quantity} {symbol} @ {avg_entry}")
        print(f"Unrealized P&L: {event['unrealized_pnl']}")
    elif side == "Flat":
        print(f"Position closed. Realized P&L: {event['realized_pnl']}")
        print(f"Net P&L: {event['net_pnl']}")

    # Access computed values
    notional = event.get("notional")
    total_pnl = event.get("total_pnl")
    net_pnl = event.get("net_pnl")

    # Position state
    is_open = event.get("is_open", False)
    is_flat = event.get("is_flat", False)
    is_long = event.get("is_long", False)
    is_short = event.get("is_short", False)
```

### on_account_update()

Called when account balance or state changes.

#### Rust

```rust
use trading_common::accounts::AccountEvent;

fn on_account_update(&mut self, event: &AccountEvent) {
    match event {
        AccountEvent::Created { account_id, timestamp } => {
            println!("Account {} created", account_id);
        }
        AccountEvent::BalanceUpdated {
            account_id, currency, old_balance, new_balance, timestamp
        } => {
            println!("Account {} balance updated:", account_id);
            println!("  {} {} -> {} (free: {} -> {})",
                currency,
                old_balance.total, new_balance.total,
                old_balance.free, new_balance.free
            );
        }
        AccountEvent::StateChanged {
            account_id, old_state, new_state, timestamp
        } => {
            println!("Account {} state: {:?} -> {:?}",
                account_id, old_state, new_state);
        }
        AccountEvent::MarginUpdated {
            account_id, margin_used, margin_available, unrealized_pnl, timestamp
        } => {
            println!("Account {} margin: used={}, available={}, unrealized={}",
                account_id, margin_used, margin_available, unrealized_pnl);
        }
    }
}
```

#### Python

```python
def on_account_update(self, event: Dict[str, Any]):
    event_type = event.get("event_type")
    account_id = event.get("account_id")

    if event_type == "Created":
        print(f"Account {account_id} created")
    elif event_type == "BalanceUpdated":
        currency = event.get("currency")
        old_total = event.get("old_total")
        new_total = event.get("new_total")
        print(f"Account {account_id} balance: {old_total} -> {new_total} {currency}")
    elif event_type == "StateChanged":
        old_state = event.get("old_state")
        new_state = event.get("new_state")
        print(f"Account {account_id} state: {old_state} -> {new_state}")
    elif event_type == "MarginUpdated":
        margin_used = event.get("margin_used")
        margin_available = event.get("margin_available")
        print(f"Account {account_id} margin: used={margin_used}, avail={margin_available}")
```

### on_marketdata_update()

Called on market data updates (ticks, quotes, order book).

#### Rust

```rust
use trading_common::data::events::{MarketDataEvent, MarketDataType};

fn on_marketdata_update(&mut self, event: &MarketDataEvent) {
    match event.data_type {
        MarketDataType::Last => {
            // Trade tick
            if let Some(tick) = &event.tick {
                println!("Trade: {} {} @ {} ({})",
                    tick.side, tick.quantity, tick.price, tick.trade_id);
            }
        }
        MarketDataType::Quote => {
            // L1 BBO
            if let Some(quote) = &event.quote {
                println!("Quote: {} x {} / {} x {}",
                    quote.bid_size, quote.bid_price,
                    quote.ask_price, quote.ask_size);

                let spread = quote.ask_price - quote.bid_price;
                println!("Spread: {}", spread);
            }
        }
        MarketDataType::BookSnapshot => {
            // L2 order book
            if let Some(book) = &event.book_snapshot {
                println!("Order book snapshot received");
            }
        }
        MarketDataType::MarkPrice => {
            println!("Mark price update for {}", event.instrument_id.symbol);
        }
        MarketDataType::FundingRate => {
            println!("Funding rate update");
        }
        _ => {}
    }
}
```

#### Python

```python
def on_marketdata_update(self, event: Dict[str, Any]):
    data_type = event.get("data_type")
    symbol = event.get("symbol")

    if data_type == "Last":
        # Trade tick
        tick = event.get("tick")
        if tick:
            print(f"Trade: {tick['side']} {tick['quantity']} @ {tick['price']}")
    elif data_type == "Quote":
        # L1 BBO
        quote = event.get("quote")
        if quote:
            print(f"Quote: {quote['bid_size']} x {quote['bid_price']} / "
                  f"{quote['ask_price']} x {quote['ask_size']}")
    elif data_type == "BookSnapshot":
        print(f"Order book snapshot for {symbol}")
    elif data_type == "MarkPrice":
        print(f"Mark price update for {symbol}")
    elif data_type == "FundingRate":
        print(f"Funding rate update for {symbol}")
```

### on_state_change()

Called on strategy lifecycle transitions.

#### Strategy States

| State | Description |
|-------|-------------|
| `Undefined` | Not initialized |
| `SetDefaults` | Setting default values |
| `Configure` | Configuring parameters |
| `DataLoaded` | Data feeds connected |
| `Historical` | Processing historical data (backtest warmup) |
| `Transition` | Transitioning from historical to realtime |
| `Realtime` | Live processing |
| `Terminated` | Normal shutdown |
| `Faulted` | Fatal error occurred |

#### Rust

```rust
use trading_common::backtest::strategy::{StrategyState, StrategyStateEvent};

fn on_state_change(&mut self, event: &StrategyStateEvent) {
    println!("Strategy {} state: {:?} -> {:?}",
        event.strategy_id, event.old_state, event.new_state);

    if let Some(reason) = &event.reason {
        println!("Reason: {}", reason);
    }

    match event.new_state {
        StrategyState::Historical => {
            println!("Starting historical data processing (warmup)");
            self.is_live = false;
        }
        StrategyState::Realtime => {
            println!("Going live!");
            self.is_live = true;
            self.on_going_live();
        }
        StrategyState::Terminated => {
            println!("Strategy terminated");
            self.cleanup();
        }
        StrategyState::Faulted => {
            println!("Strategy faulted!");
            self.handle_fault();
        }
        _ => {}
    }

    // State query helpers
    if event.new_state.is_running() { /* Historical or Realtime */ }
    if event.new_state.is_terminal() { /* Terminated or Faulted */ }
    if event.new_state.can_process_data() { /* Can receive market data */ }
}
```

#### Python

```python
def on_state_change(self, event: Dict[str, Any]):
    strategy_id = event.get("strategy_id")
    old_state = event.get("old_state")
    new_state = event.get("new_state")
    reason = event.get("reason")

    print(f"Strategy {strategy_id} state: {old_state} -> {new_state}")
    if reason:
        print(f"Reason: {reason}")

    if new_state == "Historical":
        print("Starting historical data processing")
        self.is_live = False
    elif new_state == "Realtime":
        print("Going live!")
        self.is_live = True
        self.on_going_live()
    elif new_state == "Terminated":
        print("Strategy terminated")
        self.cleanup()
    elif new_state == "Faulted":
        print("Strategy faulted!")
        self.handle_fault()
```

### on_session_update()

Called on trading session changes.

#### Rust

```rust
use trading_common::instruments::SessionEvent;

fn on_session_update(&mut self, event: &SessionEvent) {
    match event {
        SessionEvent::SessionOpened { symbol, session } => {
            println!("Session '{}' opened for {} ({:?})",
                session.name, symbol, session.session_type);
            self.session_active = true;
        }
        SessionEvent::SessionClosed { symbol } => {
            println!("Session closed for {}", symbol);
            self.session_active = false;
            // Cancel open orders at session close
            self.cancel_all_orders();
        }
        SessionEvent::MarketHalted { symbol, reason } => {
            println!("Market halted for {}: {}", symbol, reason);
            self.halt_trading = true;
        }
        SessionEvent::MarketResumed { symbol } => {
            println!("Market resumed for {}", symbol);
            self.halt_trading = false;
        }
        SessionEvent::MaintenanceStarted { symbol } => {
            println!("Maintenance started for {}", symbol);
            self.in_maintenance = true;
        }
        SessionEvent::MaintenanceEnded { symbol } => {
            println!("Maintenance ended for {}", symbol);
            self.in_maintenance = false;
        }
    }
}
```

#### Python

```python
def on_session_update(self, event: Dict[str, Any]):
    event_type = event.get("event_type")
    symbol = event.get("symbol")

    if event_type == "SessionOpened":
        session_name = event.get("session_name")
        session_type = event.get("session_type")
        print(f"Session '{session_name}' opened for {symbol} ({session_type})")
        self.session_active = True
    elif event_type == "SessionClosed":
        print(f"Session closed for {symbol}")
        self.session_active = False
        self.cancel_all_orders()
    elif event_type == "MarketHalted":
        reason = event.get("reason")
        print(f"Market halted for {symbol}: {reason}")
        self.halt_trading = True
    elif event_type == "MarketResumed":
        print(f"Market resumed for {symbol}")
        self.halt_trading = False
    elif event_type == "MaintenanceStarted":
        print(f"Maintenance started for {symbol}")
        self.in_maintenance = True
    elif event_type == "MaintenanceEnded":
        print(f"Maintenance ended for {symbol}")
        self.in_maintenance = False
```

### Complete Event-Driven Strategy Example

#### Rust

```rust
use trading_common::backtest::strategy::{Strategy, Signal, PositionEvent, StrategyState, StrategyStateEvent};
use trading_common::orders::{OrderEventAny, OrderFilled};
use trading_common::accounts::AccountEvent;
use trading_common::data::events::MarketDataEvent;
use trading_common::instruments::SessionEvent;

pub struct EventDrivenStrategy {
    is_live: bool,
    session_active: bool,
    position_qty: Decimal,
    realized_pnl: Decimal,
}

impl Strategy for EventDrivenStrategy {
    // ... required methods ...

    fn on_order_update(&mut self, event: &OrderEventAny) {
        // Track all order state changes
        println!("[ORDER] {:?}", event);
    }

    fn on_execution(&mut self, event: &OrderFilled) {
        // Process fills
        println!("[FILL] {} {} @ {}",
            event.order_side, event.last_qty, event.last_px);
    }

    fn on_position_update(&mut self, event: &PositionEvent) {
        // Update position tracking
        self.position_qty = event.quantity;
        if event.is_flat() {
            self.realized_pnl += event.realized_pnl;
        }
        println!("[POSITION] {} {} | P&L: {}",
            event.side, event.quantity, event.total_pnl());
    }

    fn on_account_update(&mut self, event: &AccountEvent) {
        println!("[ACCOUNT] {:?}", event);
    }

    fn on_marketdata_update(&mut self, event: &MarketDataEvent) {
        // React to market data
        if let Some(quote) = &event.quote {
            let mid = (quote.bid_price + quote.ask_price) / dec!(2);
            // Use mid price for decisions
        }
    }

    fn on_state_change(&mut self, event: &StrategyStateEvent) {
        self.is_live = event.new_state == StrategyState::Realtime;
        println!("[STATE] {:?} -> {:?}", event.old_state, event.new_state);
    }

    fn on_session_update(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::SessionOpened { .. } => self.session_active = true,
            SessionEvent::SessionClosed { .. } => self.session_active = false,
            _ => {}
        }
        println!("[SESSION] {:?}", event);
    }
}
```

#### Python

```python
class EventDrivenStrategy(BaseStrategy):
    def __init__(self):
        self.is_live = False
        self.session_active = False
        self.position_qty = "0"
        self.realized_pnl = "0"

    # ... required methods ...

    def on_order_update(self, event: Dict[str, Any]):
        print(f"[ORDER] {event['event_type']}: {event.get('client_order_id')}")

    def on_execution(self, event: Dict[str, Any]):
        print(f"[FILL] {event['order_side']} {event['last_qty']} @ {event['last_px']}")

    def on_position_update(self, event: Dict[str, Any]):
        self.position_qty = event.get("quantity", "0")
        if event.get("is_flat", False):
            self.realized_pnl = str(
                float(self.realized_pnl) + float(event.get("realized_pnl", "0"))
            )
        print(f"[POSITION] {event['side']} {event['quantity']} | P&L: {event['total_pnl']}")

    def on_account_update(self, event: Dict[str, Any]):
        print(f"[ACCOUNT] {event['event_type']}")

    def on_marketdata_update(self, event: Dict[str, Any]):
        if "quote" in event:
            quote = event["quote"]
            bid = float(quote["bid_price"])
            ask = float(quote["ask_price"])
            mid = (bid + ask) / 2
            # Use mid price for decisions

    def on_state_change(self, event: Dict[str, Any]):
        self.is_live = event.get("new_state") == "Realtime"
        print(f"[STATE] {event['old_state']} -> {event['new_state']}")

    def on_session_update(self, event: Dict[str, Any]):
        event_type = event.get("event_type")
        if event_type == "SessionOpened":
            self.session_active = True
        elif event_type == "SessionClosed":
            self.session_active = False
        print(f"[SESSION] {event_type}")
```

---

## 7. Portfolio Management

The `Portfolio` struct tracks positions, cash, and trade history during backtesting.

### Portfolio Structure

| Field | Description |
|-------|-------------|
| `initial_capital` | Starting capital |
| `cash` | Current cash balance |
| `positions` | Map of symbol -> Position |
| `trades` | List of executed trades |
| `commission_rate` | Commission rate (e.g., 0.001) |

### Portfolio Methods

| Method | Description |
|--------|-------------|
| `new(capital)` | Create with initial capital |
| `with_commission_rate(rate)` | Set commission rate |
| `update_price(symbol, price)` | Update market price |
| `execute_buy(symbol, qty, price)` | Execute buy trade |
| `execute_sell(symbol, qty, price)` | Execute sell trade |
| `total_value()` | Total portfolio value |
| `total_pnl()` | Total P&L |
| `total_realized_pnl()` | Realized P&L |
| `total_unrealized_pnl()` | Unrealized P&L |
| `has_position(symbol)` | Check if has position |

### Position Structure

| Field | Description |
|-------|-------------|
| `symbol` | Trading symbol |
| `quantity` | Position size |
| `avg_price` | Average entry price |
| `market_value` | Current market value |
| `unrealized_pnl` | Unrealized P&L |

---

## 8. Python Strategy Development

### File Location

Place Python strategies in: `/strategies/` directory

### Base Class

```python
from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext
from typing import Dict, Any, Optional, List
```

### Signal Helper Class (Python)

```python
class Signal:
    @staticmethod
    def buy(symbol: str, quantity: str) -> Dict[str, Any]:
        return {"type": "Buy", "symbol": symbol, "quantity": quantity}

    @staticmethod
    def sell(symbol: str, quantity: str) -> Dict[str, Any]:
        return {"type": "Sell", "symbol": symbol, "quantity": quantity}

    @staticmethod
    def hold() -> Dict[str, Any]:
        return {"type": "Hold"}
```

### BarsContext Methods (Python)

```python
class BarsContext:
    def close(self, index: int) -> Optional[float]: ...
    def open(self, index: int) -> Optional[float]: ...
    def high(self, index: int) -> Optional[float]: ...
    def low(self, index: int) -> Optional[float]: ...
    def volume(self, index: int) -> Optional[float]: ...

    def sma(self, period: int) -> Optional[float]: ...
    def highest_high(self, period: int) -> Optional[float]: ...
    def lowest_low(self, period: int) -> Optional[float]: ...
    def average_range(self, period: int) -> Optional[float]: ...

    def is_ready(self) -> bool: ...
    def is_ready_for(self, lookback: int) -> bool: ...
    def is_up_bar(self) -> bool: ...
    def is_down_bar(self) -> bool: ...
    def change(self) -> Optional[float]: ...
    def percent_change(self) -> Optional[float]: ...

    def symbol(self) -> str: ...
    def current_bar(self) -> int: ...
    def count(self) -> int: ...
```

---

## 9. Complete Examples

### SMA Crossover Strategy (Rust)

```rust
use trading_common::backtest::strategy::{Strategy, Signal, BarDataMode, BarType, MaximumBarsLookBack};
use trading_common::data::types::BarData;
use trading_common::data::Timeframe;
use trading_common::series::BarsContext;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

pub struct SmaCrossoverStrategy {
    short_period: usize,
    long_period: usize,
    position_size: Decimal,
    is_long: bool,
}

impl SmaCrossoverStrategy {
    pub fn new() -> Self {
        Self {
            short_period: 10,
            long_period: 30,
            position_size: dec!(100),
            is_long: false,
        }
    }
}

impl Strategy for SmaCrossoverStrategy {
    fn on_bar_data(&mut self, _bar_data: &BarData, bars: &mut BarsContext) -> Signal {
        if !self.is_ready(bars) {
            return Signal::Hold;
        }

        let short_sma = match bars.sma(self.short_period) {
            Some(v) => v,
            None => return Signal::Hold,
        };

        let long_sma = match bars.sma(self.long_period) {
            Some(v) => v,
            None => return Signal::Hold,
        };

        let symbol = bars.symbol().to_string();

        // Golden cross - short crosses above long
        if short_sma > long_sma && !self.is_long {
            self.is_long = true;
            return Signal::buy(&symbol, self.position_size);
        }

        // Death cross - short crosses below long
        if short_sma < long_sma && self.is_long {
            self.is_long = false;
            return Signal::sell(&symbol, self.position_size);
        }

        Signal::Hold
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        if let Some(short) = params.get("short_period") {
            self.short_period = short.parse().map_err(|_| "Invalid short_period")?;
        }
        if let Some(long) = params.get("long_period") {
            self.long_period = long.parse().map_err(|_| "Invalid long_period")?;
        }
        if let Some(size) = params.get("position_size") {
            self.position_size = size.parse().map_err(|_| "Invalid position_size")?;
        }

        if self.short_period >= self.long_period {
            return Err("short_period must be less than long_period".to_string());
        }

        Ok(())
    }

    fn is_ready(&self, bars: &BarsContext) -> bool {
        bars.is_ready_for(self.long_period)
    }

    fn warmup_period(&self) -> usize {
        self.long_period
    }

    fn reset(&mut self) {
        self.is_long = false;
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::FiveMinutes)
    }

    fn max_bars_lookback(&self) -> MaximumBarsLookBack {
        MaximumBarsLookBack::Fixed(self.long_period * 2)
    }
}
```

### SMA Crossover Strategy (Python)

```python
from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext
from typing import Dict, Any, Optional

class SmaCrossoverStrategy(BaseStrategy):
    """
    Simple Moving Average Crossover Strategy

    Generates buy signals when short SMA crosses above long SMA (golden cross)
    Generates sell signals when short SMA crosses below long SMA (death cross)
    """

    def __init__(self):
        self.short_period = 10
        self.long_period = 30
        self.position_size = "100"
        self.is_long = False

    def name(self) -> str:
        return "SMA Crossover"

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """
        Parse and validate strategy parameters.
        Returns None on success, error string on failure.
        """
        try:
            if "short_period" in params:
                self.short_period = int(params["short_period"])
            if "long_period" in params:
                self.long_period = int(params["long_period"])
            if "position_size" in params:
                self.position_size = params["position_size"]

            if self.short_period >= self.long_period:
                return "short_period must be less than long_period"

            return None
        except Exception as e:
            return str(e)

    def is_ready(self, bars: BarsContext) -> bool:
        """Check if we have enough bars to calculate indicators."""
        return bars.is_ready_for(self.long_period)

    def warmup_period(self) -> int:
        """Return minimum bars needed before is_ready() can return True."""
        return self.long_period

    def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
        """
        Main strategy logic - called for each bar.
        Returns a Signal dict.
        """
        if not self.is_ready(bars):
            return Signal.hold()

        short_sma = bars.sma(self.short_period)
        long_sma = bars.sma(self.long_period)

        if short_sma is None or long_sma is None:
            return Signal.hold()

        symbol = bar_data["symbol"]

        # Golden cross - short crosses above long
        if short_sma > long_sma and not self.is_long:
            self.is_long = True
            return Signal.buy(symbol, self.position_size)

        # Death cross - short crosses below long
        if short_sma < long_sma and self.is_long:
            self.is_long = False
            return Signal.sell(symbol, self.position_size)

        return Signal.hold()

    def reset(self):
        """Reset strategy state for new backtest."""
        self.is_long = False

    def bar_data_mode(self) -> str:
        """When to receive bar events."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """What type of bars to use."""
        return {"type": "TimeBased", "timeframe": "5m"}

    def max_bars_lookback(self) -> Dict[str, Any]:
        """How much history to keep."""
        return {"type": "Fixed", "value": self.long_period * 2}
```

### RSI Strategy (Rust)

```rust
use trading_common::backtest::strategy::{Strategy, Signal, BarDataMode, BarType};
use trading_common::data::types::BarData;
use trading_common::data::Timeframe;
use trading_common::series::BarsContext;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

pub struct RsiStrategy {
    period: usize,
    overbought: Decimal,
    oversold: Decimal,
    position_size: Decimal,
    gains: Vec<Decimal>,
    losses: Vec<Decimal>,
    prev_close: Option<Decimal>,
    is_long: bool,
}

impl RsiStrategy {
    pub fn new() -> Self {
        Self {
            period: 14,
            overbought: dec!(70),
            oversold: dec!(30),
            position_size: dec!(100),
            gains: Vec::new(),
            losses: Vec::new(),
            prev_close: None,
            is_long: false,
        }
    }

    fn calculate_rsi(&mut self, close: Decimal) -> Option<Decimal> {
        if let Some(prev) = self.prev_close {
            let change = close - prev;
            if change > Decimal::ZERO {
                self.gains.push(change);
                self.losses.push(Decimal::ZERO);
            } else {
                self.gains.push(Decimal::ZERO);
                self.losses.push(change.abs());
            }

            // Keep only period values
            if self.gains.len() > self.period {
                self.gains.remove(0);
                self.losses.remove(0);
            }
        }
        self.prev_close = Some(close);

        if self.gains.len() < self.period {
            return None;
        }

        let avg_gain: Decimal = self.gains.iter().sum::<Decimal>() / Decimal::from(self.period);
        let avg_loss: Decimal = self.losses.iter().sum::<Decimal>() / Decimal::from(self.period);

        if avg_loss == Decimal::ZERO {
            return Some(dec!(100));
        }

        let rs = avg_gain / avg_loss;
        let rsi = dec!(100) - (dec!(100) / (dec!(1) + rs));
        Some(rsi)
    }
}

impl Strategy for RsiStrategy {
    fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal {
        let close = bar_data.ohlc_bar.close;
        let symbol = bars.symbol().to_string();

        let rsi = match self.calculate_rsi(close) {
            Some(v) => v,
            None => return Signal::Hold,
        };

        // Oversold - buy signal
        if rsi < self.oversold && !self.is_long {
            self.is_long = true;
            return Signal::buy(&symbol, self.position_size);
        }

        // Overbought - sell signal
        if rsi > self.overbought && self.is_long {
            self.is_long = false;
            return Signal::sell(&symbol, self.position_size);
        }

        Signal::Hold
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        if let Some(p) = params.get("period") {
            self.period = p.parse().map_err(|_| "Invalid period")?;
        }
        if let Some(ob) = params.get("overbought") {
            self.overbought = ob.parse().map_err(|_| "Invalid overbought")?;
        }
        if let Some(os) = params.get("oversold") {
            self.oversold = os.parse().map_err(|_| "Invalid oversold")?;
        }
        Ok(())
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        self.gains.len() >= self.period
    }

    fn warmup_period(&self) -> usize {
        self.period + 1
    }

    fn reset(&mut self) {
        self.gains.clear();
        self.losses.clear();
        self.prev_close = None;
        self.is_long = false;
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)
    }
}
```

### RSI Strategy (Python)

```python
from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext
from typing import Dict, Any, Optional, List
from decimal import Decimal

class RsiStrategy(BaseStrategy):
    """
    Relative Strength Index Strategy

    Buy when RSI crosses below oversold level
    Sell when RSI crosses above overbought level
    """

    def __init__(self):
        self.period = 14
        self.overbought = 70.0
        self.oversold = 30.0
        self.position_size = "100"
        self.gains: List[float] = []
        self.losses: List[float] = []
        self.prev_close: Optional[float] = None
        self.is_long = False

    def name(self) -> str:
        return "RSI Strategy"

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        try:
            if "period" in params:
                self.period = int(params["period"])
            if "overbought" in params:
                self.overbought = float(params["overbought"])
            if "oversold" in params:
                self.oversold = float(params["oversold"])
            if "position_size" in params:
                self.position_size = params["position_size"]
            return None
        except Exception as e:
            return str(e)

    def is_ready(self, bars: BarsContext) -> bool:
        return len(self.gains) >= self.period

    def warmup_period(self) -> int:
        return self.period + 1

    def _calculate_rsi(self, close: float) -> Optional[float]:
        if self.prev_close is not None:
            change = close - self.prev_close
            if change > 0:
                self.gains.append(change)
                self.losses.append(0.0)
            else:
                self.gains.append(0.0)
                self.losses.append(abs(change))

            # Keep only period values
            if len(self.gains) > self.period:
                self.gains.pop(0)
                self.losses.pop(0)

        self.prev_close = close

        if len(self.gains) < self.period:
            return None

        avg_gain = sum(self.gains) / self.period
        avg_loss = sum(self.losses) / self.period

        if avg_loss == 0:
            return 100.0

        rs = avg_gain / avg_loss
        rsi = 100.0 - (100.0 / (1.0 + rs))
        return rsi

    def on_bar_data(self, bar_data: Dict[str, Any], bars: BarsContext) -> Dict[str, Any]:
        close = float(bar_data["close"])
        symbol = bar_data["symbol"]

        rsi = self._calculate_rsi(close)
        if rsi is None:
            return Signal.hold()

        # Oversold - buy signal
        if rsi < self.oversold and not self.is_long:
            self.is_long = True
            return Signal.buy(symbol, self.position_size)

        # Overbought - sell signal
        if rsi > self.overbought and self.is_long:
            self.is_long = False
            return Signal.sell(symbol, self.position_size)

        return Signal.hold()

    def reset(self):
        self.gains.clear()
        self.losses.clear()
        self.prev_close = None
        self.is_long = False

    def bar_data_mode(self) -> str:
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        return {"type": "TimeBased", "timeframe": "1m"}
```

### Breakout Strategy with Order Management (Rust)

```rust
use trading_common::backtest::strategy::{Strategy, Signal, BarDataMode, BarType};
use trading_common::data::types::BarData;
use trading_common::data::Timeframe;
use trading_common::series::BarsContext;
use trading_common::orders::{Order, OrderSide, TimeInForce, ClientOrderId};
use trading_common::orders::events::{OrderFilled, OrderRejected, OrderCanceled};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;

pub struct BreakoutStrategy {
    lookback: usize,
    position_size: Decimal,
    stop_loss_pct: Decimal,
    entry_order_id: Option<ClientOrderId>,
    stop_order_id: Option<ClientOrderId>,
    position: Decimal,
}

impl BreakoutStrategy {
    pub fn new() -> Self {
        Self {
            lookback: 20,
            position_size: dec!(0.1),
            stop_loss_pct: dec!(0.02),
            entry_order_id: None,
            stop_order_id: None,
            position: Decimal::ZERO,
        }
    }
}

impl Strategy for BreakoutStrategy {
    fn uses_order_management(&self) -> bool {
        true  // Enable advanced order management
    }

    fn get_orders(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<Order> {
        let mut orders = Vec::new();

        if !bars.is_ready_for(self.lookback) {
            return orders;
        }

        let symbol = bars.symbol();
        let close = bar_data.ohlc_bar.close;

        // Only enter if no position and no pending entry
        if self.position == Decimal::ZERO && self.entry_order_id.is_none() {
            let highest = bars.highest_high(self.lookback).unwrap();

            // Breakout above highest high
            if close > highest {
                // Market entry order
                let entry = Order::market(symbol, OrderSide::Buy, self.position_size)
                    .with_strategy_id("breakout")
                    .build()
                    .unwrap();

                self.entry_order_id = Some(entry.client_order_id.clone());
                orders.push(entry);
            }
        }

        orders
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        if Some(&event.client_order_id) == self.entry_order_id.as_ref() {
            // Entry filled - place stop loss
            self.position = event.last_qty;
            self.entry_order_id = None;

            let stop_price = event.last_px * (dec!(1) - self.stop_loss_pct);

            // Note: In real implementation, you'd queue this for get_orders()
            println!(
                "Entry filled at {}. Stop loss should be at {}",
                event.last_px, stop_price
            );
        } else if Some(&event.client_order_id) == self.stop_order_id.as_ref() {
            // Stop loss hit
            self.position = Decimal::ZERO;
            self.stop_order_id = None;
            println!("Stop loss filled at {}", event.last_px);
        }
    }

    fn on_order_rejected(&mut self, event: &OrderRejected) {
        if Some(&event.client_order_id) == self.entry_order_id.as_ref() {
            self.entry_order_id = None;
            println!("Entry rejected: {}", event.reason);
        }
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        if Some(&event.client_order_id) == self.stop_order_id.as_ref() {
            self.stop_order_id = None;
        }
    }

    fn on_bar_data(&mut self, _bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        // Using order management, so signals are ignored
        Signal::Hold
    }

    fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
        if let Some(lb) = params.get("lookback") {
            self.lookback = lb.parse().map_err(|_| "Invalid lookback")?;
        }
        if let Some(sl) = params.get("stop_loss_pct") {
            self.stop_loss_pct = sl.parse().map_err(|_| "Invalid stop_loss_pct")?;
        }
        Ok(())
    }

    fn is_ready(&self, bars: &BarsContext) -> bool {
        bars.is_ready_for(self.lookback)
    }

    fn warmup_period(&self) -> usize {
        self.lookback
    }

    fn reset(&mut self) {
        self.entry_order_id = None;
        self.stop_order_id = None;
        self.position = Decimal::ZERO;
    }

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnCloseBar
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::FifteenMinutes)
    }
}
```

---

## 10. Best Practices

### Strategy Development Checklist

- [ ] Implement all required methods (`on_bar_data`, `initialize`, `is_ready`, `warmup_period`)
- [ ] Ensure `is_ready()` logic matches `warmup_period()` return value
- [ ] Choose appropriate `bar_data_mode()` (prefer `OnCloseBar` for efficiency)
- [ ] Set `max_bars_lookback()` to reasonable value (2x your longest indicator period)
- [ ] Implement `reset()` if strategy maintains state
- [ ] Validate parameters in `initialize()` and return meaningful errors
- [ ] Handle `None` values from indicator methods gracefully

### Performance Tips

1. **Use OnCloseBar mode** when possible - reduces processing overhead
2. **Set appropriate lookback** - `Fixed(256)` is usually sufficient
3. **Avoid allocations in on_bar_data** - pre-allocate vectors in initialize
4. **Cache indicator values** when computing multiple times per bar

### Common Pitfalls

1. **Forgetting to check is_ready()** - Always check before using indicators
2. **Mismatched warmup_period()** - Must match actual bars needed
3. **Not resetting state** - Implement reset() for backtesting accuracy
4. **Infinite lookback** - Can cause memory issues; use Fixed()
5. **Blocking operations** - Don't do I/O in on_bar_data()

### Testing Strategies

```bash
# Run backtest from CLI
cd trading-core
cargo run backtest

# Run with specific strategy
cargo run backtest --strategy sma --short-period 10 --long-period 30

# Enable debug logging
RUST_LOG=trading_common=debug cargo run backtest
```

---

## Appendix: Type Reference

### Decimal Usage

Always use `rust_decimal` for financial calculations:

```rust
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

let price = dec!(50000.50);
let qty = Decimal::from(100);
let pct = Decimal::new(25, 1);  // 2.5
```

### ID Types

| Type | Description | Example |
|------|-------------|---------|
| `ClientOrderId` | Client-assigned order ID | `ClientOrderId::generate()` |
| `VenueOrderId` | Exchange-assigned ID | From exchange response |
| `TradeId` | Fill/execution ID | From fill event |
| `StrategyId` | Strategy identifier | `"my-strategy".into()` |
| `InstrumentId` | Symbol + Venue | `InstrumentId::new("BTCUSDT", "BINANCE")` |

### Error Handling

```rust
// In initialize()
fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> {
    let period = params.get("period")
        .ok_or("Missing required parameter: period")?
        .parse::<usize>()
        .map_err(|_| "Invalid period: must be a positive integer")?;

    if period < 2 {
        return Err("Period must be at least 2".to_string());
    }

    self.period = period;
    Ok(())
}
```
