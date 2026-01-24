# NautilusTrader Domain Model Reference

This document details the structure, relationships, and types of core domain objects in NautilusTrader.

## Table of Contents

1. [Identifiers (Symbols, Instruments)](#1-identifiers-symbols-instruments)
2. [Market Data (TradeTick, QuoteTick)](#2-market-data-tradetick-quotetick)
3. [OrderBook](#3-orderbook)
4. [Orders](#4-orders)
   - 4.1 [Strategy Order Management](#41-strategy-order-management)
   - 4.2 [Backtesting Order Flow](#42-backtesting-order-flow)
5. [Executions](#5-executions)
6. [Positions](#6-positions)    
7. [Accounts](#7-accounts)
8. [Timers and Clocks](#8-timers-and-clocks)
9. [Risk Management](#9-risk-management)
10. [Message Bus](#10-message-bus)
11. [Strategy and Actor](#11-strategy-and-actor)
12. [Cache](#12-cache)
13. [Portfolio](#13-portfolio)
14. [Sessions](#14-sessions)
15. [Domain Object Relationships](#15-domain-object-relationships)

---

## 1. Identifiers (Symbols, Instruments)

### InstrumentId

The primary identifier for tradable instruments, composed of Symbol + Venue.

**File:** `crates/model/src/identifiers/instrument_id.rs`

```rust
pub struct InstrumentId {
    pub symbol: Symbol,
    pub venue: Venue,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `symbol` | `Symbol` | The instrument's ticker symbol |
| `venue` | `Venue` | The trading venue/exchange |

**Format:** `SYMBOL.VENUE` (e.g., `ETHUSDT.BINANCE`, `AAPL.XNAS`)

**Methods:**
- `is_synthetic()` - Check if instrument is on a synthetic venue
- Supports DEX addresses: `0xC31E...fa443.Arbitrum:UniswapV3`

### Symbol

**File:** `crates/model/src/identifiers/symbol.rs`

```rust
pub struct Symbol(Ustr);  // Interned string
```

- UTF-8 validated
- **Composite symbols:** Can contain periods (e.g., `CL.FUT`, `ES.c.0`)
  - `root()` - Substring before first period
  - `topic()` - Root with wildcard for pattern matching
  - `is_composite()` - Checks for period presence

### Venue

**File:** `crates/model/src/identifiers/venue.rs`

```rust
pub struct Venue(Ustr);  // Interned string
```

- ASCII-only validated
- **Synthetic:** `SYNTH` constant for derived instruments
- **DEX Format:** `Chain:DexId` (e.g., `Ethereum:UniswapV3`, `Arbitrum:CamelotV3`)

### Instrument Types

**File:** `crates/model/src/instruments/any.rs`

All instruments are wrapped in `InstrumentAny` enum:

| Type | Description | Key Fields |
|------|-------------|------------|
| `CurrencyPair` | Spot/cash FX and crypto | `base_currency`, `quote_currency` |
| `Equity` | Stocks and shares | Optional `isin` |
| `FuturesContract` | Deliverable futures | `underlying`, `expiration_ns`, `activation_ns` |
| `OptionContract` | Options | `strike_price`, `option_kind`, `underlying` |
| `CryptoPerpetual` | Perpetual swaps | `settlement_currency`, `is_inverse` |
| `CryptoFuture` | Dated crypto futures | Like perpetual + expiration |
| `CryptoOption` | Crypto options | Strike, expiration, underlying |
| `FuturesSpread` | Multi-leg futures | Leg instruments |
| `OptionSpread` | Multi-leg options | Leg instruments |
| `BettingInstrument` | Sports betting | Betting-specific fields |
| `BinaryOption` | Binary/digital options | Binary-specific fields |

**Common Instrument Fields:**

```rust
// Identification
id: InstrumentId
raw_symbol: Symbol           // Venue-specific symbol

// Pricing & Sizing
price_precision: u8          // Decimal places for prices
size_precision: u8           // Decimal places for quantities
price_increment: Price       // Minimum tick size
size_increment: Quantity     // Minimum order size
multiplier: Quantity         // Contract multiplier

// Trading Limits
max_quantity, min_quantity: Option<Quantity>
max_price, min_price: Option<Price>
max_notional, min_notional: Option<Money>
lot_size: Option<Quantity>

// Costs
margin_init: Decimal         // Initial margin requirement
margin_maint: Decimal        // Maintenance margin
maker_fee: Decimal
taker_fee: Decimal

// Timestamps
ts_event: UnixNanos
ts_init: UnixNanos
```

---

## 2. Market Data (TradeTick, QuoteTick)

### TradeTick

Represents a single trade execution in the market.

**File:** `crates/model/src/data/trade.rs`

```rust
#[repr(C)]
pub struct TradeTick {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub aggressor_side: AggressorSide,
    pub trade_id: TradeId,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `instrument_id` | `InstrumentId` | Identifies the traded instrument |
| `price` | `Price` | Traded price |
| `size` | `Quantity` | Traded volume (must be positive) |
| `aggressor_side` | `AggressorSide` | Buyer, Seller, or NoAggressor |
| `trade_id` | `TradeId` | Venue-assigned match ID |
| `ts_event` | `UnixNanos` | When trade occurred at venue |
| `ts_init` | `UnixNanos` | When instance was created |

### QuoteTick

Represents a top-of-book bid/ask quote.

**File:** `crates/model/src/data/quote.rs`

```rust
#[repr(C)]
pub struct QuoteTick {
    pub instrument_id: InstrumentId,
    pub bid_price: Price,
    pub ask_price: Price,
    pub bid_size: Quantity,
    pub ask_size: Quantity,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `instrument_id` | `InstrumentId` | Identifies the quoted instrument |
| `bid_price` | `Price` | Top-of-book bid price |
| `ask_price` | `Price` | Top-of-book ask price |
| `bid_size` | `Quantity` | Top-of-book bid quantity |
| `ask_size` | `Quantity` | Top-of-book ask quantity |
| `ts_event` | `UnixNanos` | When quote event occurred |
| `ts_init` | `UnixNanos` | When instance was created |

**Methods:**
- `extract_price(PriceType)` - Get bid, ask, or mid price
- `extract_size(PriceType)` - Get bid, ask, or mid size

### Bar (Aggregated Data)

**File:** `crates/model/src/data/bar.rs`

```rust
pub struct Bar {
    pub bar_type: BarType,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

**BarSpecification:** Defines aggregation rules (step, aggregation type, price type)

**Aggregation Types:** Second, Minute, Hour, Day, Week, Month, Year, Tick, Volume, Value

**Price Types:** Bid, Ask, Mid, Last

### Timestamp Precision

- `ts_event`: UNIX timestamp (nanoseconds) when data occurred at exchange
- `ts_init`: UNIX timestamp (nanoseconds) when data was ingested by Nautilus
- Type: `UnixNanos` (u64 wrapper)

**Numeric Precision:**
- **High-precision** (Linux/macOS): 128-bit fixed-point, 16 decimal places
- **Standard-precision** (Windows): 64-bit fixed-point, 9 decimal places

---

## 3. OrderBook

### OrderBook Structure

**File:** `crates/model/src/orderbook/book.rs`

```rust
pub struct OrderBook {
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    pub sequence: u64,
    pub ts_last: UnixNanos,
    pub update_count: u64,
    pub(crate) bids: BookLadder,
    pub(crate) asks: BookLadder,
}
```

| Field | Type | Description |
|-------|------|-------------|
| `instrument_id` | `InstrumentId` | Identifies the instrument |
| `book_type` | `BookType` | L1_MBP, L2_MBP, or L3_MBO |
| `sequence` | `u64` | Current sequence number |
| `ts_last` | `UnixNanos` | Last update timestamp |
| `update_count` | `u64` | Total updates applied |
| `bids` | `BookLadder` | Bid-side price levels |
| `asks` | `BookLadder` | Ask-side price levels |

**BookType:**
- `L1_MBP` - Level 1 Market-By-Price (top of book only)
- `L2_MBP` - Level 2 Market-By-Price (aggregated levels)
- `L3_MBO` - Level 3 Market-By-Order (individual orders)

**Key Methods:**
- `add()`, `update()`, `delete()` - Apply order operations
- `apply_delta()`, `apply_deltas()` - Apply delta updates
- `apply_depth()` - Apply depth snapshot
- `best_bid_price()`, `best_ask_price()` - Get top of book
- `spread()`, `midpoint()` - Calculate spread metrics
- `simulate_fills()` - Simulate order execution
- `get_quantity_for_price()` - Get available liquidity

### BookOrder

**File:** `crates/model/src/data/order.rs`

```rust
#[repr(C)]
pub struct BookOrder {
    pub side: OrderSide,
    pub price: Price,
    pub size: Quantity,
    pub order_id: OrderId,  // u64
}
```

Represents a single order in the book (for L3 data) or aggregated level (for L2).

### OrderBookDelta

**File:** `crates/model/src/data/delta.rs`

```rust
#[repr(C)]
pub struct OrderBookDelta {
    pub instrument_id: InstrumentId,
    pub action: BookAction,
    pub order: BookOrder,
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

**BookAction:**
- `Add` - Add new order/level
- `Update` - Update existing order/level
- `Delete` - Remove order/level
- `Clear` - Clear entire side or book

### OrderBookDeltas

**File:** `crates/model/src/data/deltas.rs`

```rust
pub struct OrderBookDeltas {
    pub instrument_id: InstrumentId,
    pub deltas: Vec<OrderBookDelta>,
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

Container for batch delta updates (more efficient than individual deltas).

### OrderBookDepth10

**File:** `crates/model/src/data/depth.rs`

```rust
#[repr(C)]
pub struct OrderBookDepth10 {
    pub instrument_id: InstrumentId,
    pub bids: [BookOrder; 10],
    pub asks: [BookOrder; 10],
    pub bid_counts: [u32; 10],
    pub ask_counts: [u32; 10],
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

Fixed 10-level depth snapshot (optimized for common use case).

### BookLevel

**File:** `crates/model/src/orderbook/level.rs`

```rust
pub struct BookLevel {
    pub price: BookPrice,
    pub(crate) orders: IndexMap<OrderId, BookOrder>,
}
```

Represents a single price level with FIFO-ordered orders.

**Methods:**
- `size()` - Total quantity at level
- `exposure()` - Notional exposure at level
- `add()`, `update()`, `delete()` - Modify orders

---

## 4. Orders

### Order Architecture

**File:** `crates/model/src/orders/mod.rs`

Orders use `enum_dispatch` pattern with `OrderAny` as the unified wrapper:

```rust
pub enum OrderAny {
    Limit(LimitOrder),
    Market(MarketOrder),
    StopMarket(StopMarketOrder),
    StopLimit(StopLimitOrder),
    MarketIfTouched(MarketIfTouchedOrder),
    LimitIfTouched(LimitIfTouchedOrder),
    MarketToLimit(MarketToLimitOrder),
    TrailingStopMarket(TrailingStopMarketOrder),
    TrailingStopLimit(TrailingStopLimitOrder),
}
```

### Order Types

| Type | Description | Key Fields |
|------|-------------|------------|
| `MarketOrder` | Immediate execution at best price | `protection_price` (optional) |
| `LimitOrder` | Execute at specific price or better | `price`, `is_post_only`, `display_qty` |
| `StopMarketOrder` | Market order when trigger reached | `trigger_price`, `trigger_type` |
| `StopLimitOrder` | Limit order when trigger reached | `price`, `trigger_price` |
| `MarketIfTouchedOrder` | Market when price touched | `trigger_price` |
| `LimitIfTouchedOrder` | Limit when price touched | `price`, `trigger_price` |
| `MarketToLimitOrder` | Market converts to limit | Converts at best price |
| `TrailingStopMarketOrder` | Stop follows market | `trailing_offset`, `trailing_offset_type` |
| `TrailingStopLimitOrder` | Trailing stop → limit | `price`, `trailing_offset` |

### OrderCore (Shared Fields)

**File:** `crates/model/src/orders/mod.rs` (lines 560-603)

```rust
// Identity
client_order_id: ClientOrderId       // System-assigned order ID
venue_order_id: Option<VenueOrderId> // Exchange-assigned ID
init_id: UUID4                       // Unique init event ID
position_id: Option<PositionId>
account_id: Option<AccountId>

// Trader/Strategy
trader_id: TraderId
strategy_id: StrategyId
instrument_id: InstrumentId

// Order Specification
order_type: OrderType
side: OrderSide                      // Buy/Sell
quantity: Quantity
time_in_force: TimeInForce           // GTC, IOC, FOK, GTD, DAY, ATO, ATC
liquidity_side: Option<LiquiditySide>

// Flags
is_reduce_only: bool
is_quote_quantity: bool
status: OrderStatus
previous_status: Option<OrderStatus>

// Fill Tracking
filled_qty: Quantity
leaves_qty: Quantity
overfill_qty: Quantity
trade_ids: Vec<TradeId>
last_trade_id: Option<TradeId>

// Pricing
avg_px: Option<f64>                  // Volume-weighted average fill price
slippage: Option<f64>

// Contingency/Algo
contingency_type: Option<ContingencyType>
order_list_id: Option<OrderListId>
linked_order_ids: Option<Vec<ClientOrderId>>
parent_order_id: Option<ClientOrderId>
exec_algorithm_id: Option<ExecAlgorithmId>
exec_algorithm_params: Option<IndexMap<Ustr, Ustr>>

// Timestamps
ts_init: UnixNanos
ts_submitted: Option<UnixNanos>
ts_accepted: Option<UnixNanos>
ts_closed: Option<UnixNanos>
ts_last: UnixNanos

// Events
events: Vec<OrderEventAny>           // Full event history
commissions: IndexMap<Currency, Money>
```

### OrderStatus (State Machine)

**File:** `crates/model/src/enums.rs` (lines 1115-1144)

```
Initialized
  ├─→ Denied
  ├─→ Emulated
  │   ├─→ Released → Submitted
  │   ├─→ Canceled
  │   └─→ Expired
  └─→ Submitted
      ├─→ Accepted
      │   ├─→ Triggered
      │   ├─→ PartiallyFilled
      │   ├─→ Filled
      │   ├─→ Canceled
      │   └─→ PendingUpdate/PendingCancel
      ├─→ Rejected
      └─→ Filled

Final States: Denied, Rejected, Canceled, Expired, Filled
Open States: Accepted, Triggered, PendingUpdate, PendingCancel, PartiallyFilled
```

### TimeInForce Options

| Value | Description |
|-------|-------------|
| `GTC` | Good Till Canceled |
| `IOC` | Immediate Or Cancel |
| `FOK` | Fill Or Kill |
| `GTD` | Good Till Date |
| `DAY` | Day order |
| `ATO` | At The Open |
| `ATC` | At The Close |

### 4.1 Strategy Order Management

This section covers how strategies create, submit, modify, and cancel orders.

**Files:**
- `nautilus_trader/trading/strategy.pyx` - Strategy base class
- `nautilus_trader/common/factories.pyx` - OrderFactory
- `nautilus_trader/execution/manager.pyx` - OrderManager

#### OrderFactory

Each strategy has an `OrderFactory` instance for creating orders. The factory automatically generates unique `ClientOrderId` values and tags them with the trader and strategy IDs.

```python
# Available in strategy via self.order_factory
order = self.order_factory.market(
    instrument_id=instrument_id,
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("1.0"),
)
```

**Factory Methods:**

| Method | Order Type | Key Parameters |
|--------|------------|----------------|
| `market()` | MarketOrder | `instrument_id`, `order_side`, `quantity` |
| `limit()` | LimitOrder | `instrument_id`, `order_side`, `quantity`, `price` |
| `stop_market()` | StopMarketOrder | `instrument_id`, `order_side`, `quantity`, `trigger_price` |
| `stop_limit()` | StopLimitOrder | `instrument_id`, `order_side`, `quantity`, `price`, `trigger_price` |
| `market_if_touched()` | MarketIfTouchedOrder | `instrument_id`, `order_side`, `quantity`, `trigger_price` |
| `limit_if_touched()` | LimitIfTouchedOrder | `instrument_id`, `order_side`, `quantity`, `price`, `trigger_price` |
| `trailing_stop_market()` | TrailingStopMarketOrder | `instrument_id`, `order_side`, `quantity`, `trailing_offset` |
| `trailing_stop_limit()` | TrailingStopLimitOrder | `instrument_id`, `order_side`, `quantity`, `price`, `trailing_offset` |

**Common Optional Parameters:**
- `time_in_force` - Order duration (GTC, IOC, FOK, GTD, DAY)
- `expire_time` - Expiration time for GTD orders
- `reduce_only` - Only reduce existing position
- `post_only` - Only add liquidity (maker)
- `display_qty` - Iceberg/hidden quantity
- `exec_algorithm_id` - Execution algorithm routing
- `tags` - Custom tags for tracking

#### Order Submission

**`submit_order(order, position_id=None, client_id=None, params=None)`**

Submits a single order. The order is routed based on its configuration:

```python
# Simple submission
self.submit_order(order)

# With position ID (for hedging OMS)
self.submit_order(order, position_id=position.id)

# With custom client routing
self.submit_order(order, client_id=ClientId("BINANCE"))
```

**Routing Logic:**
1. If `order.emulation_trigger != NO_TRIGGER` → OrderEmulator
2. If `order.exec_algorithm_id` is set → ExecAlgorithm
3. Otherwise → RiskEngine → ExecutionEngine → Venue

**`submit_order_list(order_list, position_id=None, client_id=None, params=None)`**

Submits multiple related orders (e.g., bracket orders with contingencies):

```python
# Create bracket order
entry = self.order_factory.market(...)
stop_loss = self.order_factory.stop_market(...)
take_profit = self.order_factory.limit(...)

order_list = OrderList(
    order_list_id=self.order_factory.generate_order_list_id(),
    orders=[entry, stop_loss, take_profit],
)
self.submit_order_list(order_list)
```

#### Order Modification

**`modify_order(order, quantity=None, price=None, trigger_price=None, client_id=None, params=None)`**

Modifies an open order. At least one parameter must differ from the current values:

```python
# Modify price
self.modify_order(order, price=Price.from_str("50000.00"))

# Modify quantity
self.modify_order(order, quantity=Quantity.from_str("2.0"))

# Modify both
self.modify_order(
    order,
    quantity=Quantity.from_str("2.0"),
    price=Price.from_str("50000.00"),
)
```

#### Order Cancellation

**`cancel_order(order, client_id=None, params=None)`**

Cancels a single order:

```python
self.cancel_order(order)
```

**`cancel_orders(orders, client_id=None, params=None)`**

Batch cancels multiple orders (must be same instrument, non-emulated):

```python
open_orders = self.cache.orders_open(instrument_id=instrument_id, strategy_id=self.id)
self.cancel_orders(open_orders)
```

**`cancel_all_orders(instrument_id, order_side=NO_ORDER_SIDE, client_id=None, params=None)`**

Cancels all open and emulated orders for an instrument:

```python
# Cancel all orders for instrument
self.cancel_all_orders(instrument_id)

# Cancel only buy orders
self.cancel_all_orders(instrument_id, order_side=OrderSide.BUY)
```

#### Position Management

**`close_position(position, client_id=None, tags=None, time_in_force=GTC, reduce_only=True, quote_quantity=False, params=None)`**

Closes a position with a market order:

```python
position = self.cache.position(position_id)
self.close_position(position)
```

**`close_all_positions(instrument_id, position_side=NO_POSITION_SIDE, client_id=None, tags=None, time_in_force=GTC, reduce_only=True, quote_quantity=False, params=None)`**

Closes all positions for an instrument:

```python
# Close all positions
self.close_all_positions(instrument_id)

# Close only long positions
self.close_all_positions(instrument_id, position_side=PositionSide.LONG)
```

#### Order Tracking via Cache

Strategies access order state through the cache:

```python
# Single order lookup
order = self.cache.order(client_order_id)
order = self.cache.order_by_venue_id(venue_order_id)

# Filtered queries
open_orders = self.cache.orders_open(
    venue=None,
    instrument_id=instrument_id,
    strategy_id=self.id,
    side=OrderSide.BUY,
)

emulated_orders = self.cache.orders_emulated(strategy_id=self.id)
inflight_orders = self.cache.orders_inflight(strategy_id=self.id)

# State checks
exists = self.cache.order_exists(client_order_id)
is_open = self.cache.is_order_open(client_order_id)
```

#### Order Event Handlers

Strategies receive order lifecycle events via callbacks:

```python
class MyStrategy(Strategy):
    def on_order_initialized(self, event: OrderInitialized): ...
    def on_order_denied(self, event: OrderDenied): ...
    def on_order_emulated(self, event: OrderEmulated): ...
    def on_order_released(self, event: OrderReleased): ...
    def on_order_submitted(self, event: OrderSubmitted): ...
    def on_order_accepted(self, event: OrderAccepted): ...
    def on_order_rejected(self, event: OrderRejected): ...
    def on_order_canceled(self, event: OrderCanceled): ...
    def on_order_expired(self, event: OrderExpired): ...
    def on_order_triggered(self, event: OrderTriggered): ...
    def on_order_pending_update(self, event: OrderPendingUpdate): ...
    def on_order_pending_cancel(self, event: OrderPendingCancel): ...
    def on_order_modify_rejected(self, event: OrderModifyRejected): ...
    def on_order_cancel_rejected(self, event: OrderCancelRejected): ...
    def on_order_updated(self, event: OrderUpdated): ...
    def on_order_filled(self, event: OrderFilled): ...
```

#### OrderManager (Contingent Orders)

The `OrderManager` handles contingent order relationships (OTO, OCO, OUO):

**ContingencyType:**

| Type | Description |
|------|-------------|
| `NO_CONTINGENCY` | Independent order |
| `OTO` | One-Triggers-Other: Parent fill triggers child submission |
| `OCO` | One-Cancels-Other: One fill/cancel cancels siblings |
| `OUO` | One-Updates-Other: Parent update propagates to children |

When `manage_contingent_orders=True` in `StrategyConfig`, the OrderManager automatically:
- Submits child orders when OTO parent fills
- Cancels sibling orders when OCO order fills/cancels
- Updates linked orders when OUO parent is modified

#### GTD Expiry Management

When `manage_gtd_expiry=True` in `StrategyConfig`, the strategy automatically:
- Sets timers for GTD order expiration
- Cancels GTD orders when they expire
- Cleans up timers when orders are filled/canceled

```python
# Strategy config
config = StrategyConfig(
    strategy_id="MyStrategy-001",
    manage_gtd_expiry=True,
    manage_contingent_orders=True,
)
```

#### Order Flow Diagram (Live Trading)

```
┌──────────────┐
│   Strategy   │
│ submit_order │
└──────┬───────┘
       │
       ▼
┌──────────────┐    ┌────────────────┐
│ OrderEmulator│◄───│ Emulated Order │
│  (optional)  │    │ trigger check  │
└──────┬───────┘    └────────────────┘
       │
       ▼
┌──────────────┐
│ ExecAlgorithm│◄─── Algo Orders (TWAP, VWAP, etc.)
│  (optional)  │
└──────┬───────┘
       │
       ▼
┌──────────────┐
│  RiskEngine  │──── Order validation, rate limiting
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ExecutionEngine──── Order routing, position management
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ExecutionClient──── Adapter to venue
└──────┬───────┘
       │
       ▼
┌──────────────┐
│    Venue     │──── Exchange/Broker
└──────┬───────┘
       │
       ▼ (events)
┌──────────────┐
│   Strategy   │──── on_order_filled(), on_order_accepted(), etc.
└──────────────┘
```

---

### 4.2 Backtesting Order Flow

This section covers how orders are processed during backtesting with the `SimulatedExchange`.

**Files:**
- `crates/backtest/src/exchange.rs` - SimulatedExchange
- `crates/execution/src/matching_engine/engine.rs` - OrderMatchingEngine
- `crates/execution/src/models/fill.rs` - FillModel
- `crates/execution/src/models/fee.rs` - FeeModel
- `crates/execution/src/models/latency.rs` - LatencyModel

#### SimulatedExchange

The `SimulatedExchange` simulates a trading venue during backtesting:

```rust
pub struct SimulatedExchange {
    pub id: Venue,
    pub oms_type: OmsType,
    pub account_type: AccountType,
    // ...
    matching_engines: AHashMap<InstrumentId, OrderMatchingEngine>,
    fill_model: FillModel,
    fee_model: FeeModelAny,
    latency_model: Option<Box<dyn LatencyModel>>,
    inflight_queue: BinaryHeap<InflightCommand>,
    // ...
}
```

**Key Configuration:**

| Option | Default | Description |
|--------|---------|-------------|
| `bar_execution` | `true` | Process orders on bar data |
| `trade_execution` | `true` | Process orders on trade data |
| `liquidity_consumption` | `true` | Track consumed liquidity at price levels |
| `reject_stop_orders` | `true` | Reject stops without trigger price |
| `support_gtd_orders` | `true` | Support GTD time-in-force |
| `support_contingent_orders` | `true` | Support OTO/OCO/OUO |
| `use_position_ids` | `true` | Generate position IDs |
| `use_random_ids` | `false` | Use random venue order IDs |
| `use_reduce_only` | `true` | Enforce reduce-only orders |
| `use_message_queue` | `true` | Queue commands for ordered processing |
| `frozen_account` | `false` | Disable balance updates |

#### Latency Model

The latency model simulates network delays:

```rust
pub trait LatencyModel {
    /// Base latency for command insertion (submission)
    fn insert_latency_nanos(&self) -> u64;

    /// Latency for command updates (modifications)
    fn update_latency_nanos(&self) -> u64;

    /// Latency for command deletions (cancellations)
    fn delete_latency_nanos(&self) -> u64;
}
```

Commands are placed in an `inflight_queue` (priority queue ordered by timestamp) and processed when market data advances past the command's arrival time.

#### OrderMatchingEngine

Each instrument has a dedicated matching engine:

```rust
pub struct OrderMatchingEngine {
    pub venue: Venue,
    pub instrument: InstrumentAny,
    pub book_type: BookType,
    pub oms_type: OmsType,
    pub market_status: MarketStatus,
    book: OrderBook,
    core: OrderMatchingCore,
    fill_model: FillModel,
    fee_model: FeeModelAny,
    // Liquidity tracking
    bid_consumption: AHashMap<PriceRaw, (QuantityRaw, QuantityRaw)>,
    ask_consumption: AHashMap<PriceRaw, (QuantityRaw, QuantityRaw)>,
    // ...
}
```

**Order Processing by Type:**

| Order Type | Fill Trigger |
|------------|--------------|
| `MarketOrder` | Immediately at best available price |
| `LimitOrder` | When market price crosses limit price |
| `StopMarketOrder` | When trigger price reached → market order |
| `StopLimitOrder` | When trigger price reached → limit order |
| `MarketIfTouchedOrder` | When touch price reached → market order |
| `LimitIfTouchedOrder` | When touch price reached → limit order |
| `TrailingStopMarket` | Dynamic trigger based on market movement |
| `TrailingStopLimit` | Dynamic trigger → limit order |

#### Fill Determination

Fills are determined from the order book or market data:

**From Order Book (L2/L3):**
```rust
// Simulate fills by walking the book
let fills = book.simulate_fills(order_side, quantity);
// Returns Vec<(Price, Quantity)> of fill levels
```

**Liquidity Consumption:**
When `liquidity_consumption=true`, the engine tracks consumed liquidity at each price level to prevent filling more than available:

```rust
// Track (original_size, consumed_amount) per price level
bid_consumption: AHashMap<PriceRaw, (QuantityRaw, QuantityRaw)>
ask_consumption: AHashMap<PriceRaw, (QuantityRaw, QuantityRaw)>
```

The engine resets consumption tracking when new market data arrives with different book state.

**From Quote/Trade Data:**
When no order book is available, fills use best bid/ask from quotes or last trade price.

#### FillModel

The `FillModel` adds probabilistic fill behavior:

```rust
pub struct FillModel {
    /// Probability of limit fill when price touched (0.0 to 1.0)
    prob_fill_on_limit: f64,
    /// Probability of one-tick slippage (0.0 to 1.0)
    prob_slippage: f64,
    rng: StdRng,  // Seeded RNG for reproducibility
}
```

**Methods:**
- `is_limit_filled()` - Returns `true` based on `prob_fill_on_limit`
- `is_slipped()` - Returns `true` based on `prob_slippage`

**Default:** `prob_fill_on_limit=1.0`, `prob_slippage=0.0` (fills at exact price)

#### FeeModel

Two fee model implementations:

**MakerTakerFeeModel (default):**
```rust
// Uses instrument's maker_fee/taker_fee based on liquidity side
commission = notional_value * fee_rate
```

**FixedFeeModel:**
```rust
// Fixed commission per order (or per fill)
commission = fixed_amount
// change_commission_once: charge only on first fill
```

#### Event Generation

The matching engine generates events that flow back to the strategy:

```
OrderAccepted → Order acknowledged by simulated venue
OrderTriggered → Stop/conditional order triggered
OrderFilled → Fill execution (partial or complete)
OrderCanceled → Order canceled
OrderExpired → GTD order expired
OrderRejected → Order rejected (validation failure)
```

#### Backtesting Order Flow Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                         BACKTESTING ENGINE                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌────────────┐                                                     │
│  │  Strategy  │                                                     │
│  │submit_order│                                                     │
│  └─────┬──────┘                                                     │
│        │                                                            │
│        ▼                                                            │
│  ┌────────────┐                                                     │
│  │ RiskEngine │ ─── Validation (same as live)                       │
│  └─────┬──────┘                                                     │
│        │                                                            │
│        ▼                                                            │
│  ┌─────────────────┐                                                │
│  │ExecutionEngine  │ ─── Routes to BacktestExecutionClient          │
│  └────────┬────────┘                                                │
│           │                                                         │
│           ▼                                                         │
│  ┌─────────────────────┐                                            │
│  │BacktestExecClient   │ ─── Passes commands to SimulatedExchange   │
│  └────────┬────────────┘                                            │
│           │                                                         │
│           ▼                                                         │
│  ┌─────────────────────┐    ┌─────────────────┐                     │
│  │  SimulatedExchange  │◄───│  LatencyModel   │                     │
│  │                     │    │ (insert/update/ │                     │
│  │  inflight_queue     │    │  delete delays) │                     │
│  └────────┬────────────┘    └─────────────────┘                     │
│           │                                                         │
│           │ (commands processed when market time advances)          │
│           ▼                                                         │
│  ┌─────────────────────┐                                            │
│  │OrderMatchingEngine  │ ─── Per-instrument matching                │
│  │                     │                                            │
│  │  ├─ OrderBook       │ ─── Maintains simulated book               │
│  │  ├─ OrderMatchCore  │ ─── Price/trigger matching                 │
│  │  ├─ FillModel       │ ─── Probabilistic fills                    │
│  │  └─ FeeModel        │ ─── Commission calculation                 │
│  └────────┬────────────┘                                            │
│           │                                                         │
│           │ (order events)                                          │
│           ▼                                                         │
│  ┌─────────────────────┐                                            │
│  │    Message Bus      │ ─── Events published to topics             │
│  └────────┬────────────┘                                            │
│           │                                                         │
│           ▼                                                         │
│  ┌────────────┐                                                     │
│  │  Strategy  │ ─── on_order_filled(), on_order_accepted(), etc.    │
│  └────────────┘                                                     │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

#### Market Data Driven Execution

The matching engine processes orders when market data arrives:

**Quote Tick Processing:**
```
1. Update best bid/ask prices
2. Check stop/trigger orders for activation
3. Match limit orders that cross new prices
4. Match market orders at available prices
```

**Trade Tick Processing:**
```
1. Update last trade price
2. Check stop orders using last price (if configured)
3. Process any triggered orders
```

**Order Book Delta Processing:**
```
1. Apply delta to internal book
2. Recalculate best bid/ask
3. Reset liquidity consumption for affected levels
4. Process matching orders
```

**Bar Processing:**
```
1. Use bar OHLC to simulate price movement
2. Process orders at high/low extremes
3. Optionally consume bar volume for fills
```

#### Liquidity Consumption Example

```
Initial Book:        After Buy 150:
Ask: 100.02 x 100    Ask: 100.02 x 50   (100 consumed)
Ask: 100.01 x 100    Ask: 100.01 x 0    (100 consumed, level empty)
─────────────────    ─────────────────
Bid: 100.00 x 100    Bid: 100.00 x 100
Bid: 99.99 x 100     Bid: 99.99 x 100

Fill result: 100 @ 100.01, 50 @ 100.02
Remaining: 0 (fully filled)
```

When new book data arrives with different quantities, consumption tracking resets.

---

## 5. Executions

### OrderFilled Event

The primary execution event representing a trade fill.

**File:** `crates/model/src/events/order/filled.rs`

```rust
pub struct OrderFilled {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub account_id: AccountId,
    pub trade_id: TradeId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub last_qty: Quantity,              // Fill quantity
    pub last_px: Price,                  // Fill price
    pub currency: Currency,
    pub liquidity_side: LiquiditySide,   // Maker or Taker
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
    pub reconciliation: bool,
    pub position_id: Option<PositionId>,
    pub commission: Option<Money>,
}
```

### Order Event Lifecycle

**File:** `crates/model/src/events/order/`

| Event | Description | Key Fields |
|-------|-------------|------------|
| `OrderInitialized` | Order created | All order specs |
| `OrderDenied` | System denial | `reason` |
| `OrderEmulated` | Emulation started | - |
| `OrderReleased` | Released from emulation | - |
| `OrderSubmitted` | Sent to venue | - |
| `OrderAccepted` | Venue acknowledged | `venue_order_id` |
| `OrderRejected` | Venue rejected | `reason`, `due_post_only` |
| `OrderCanceled` | Order canceled | - |
| `OrderExpired` | GTD expiration | - |
| `OrderTriggered` | Stop triggered | - |
| `OrderPendingUpdate` | Modification pending | - |
| `OrderPendingCancel` | Cancellation pending | - |
| `OrderUpdated` | Order modified | Updated fields |
| `OrderFilled` | Execution occurred | Fill details |
| `OrderModifyRejected` | Modification rejected | `reason` |
| `OrderCancelRejected` | Cancellation rejected | `reason` |

### Execution Reports

**File:** `crates/model/src/reports/`

**FillReport:** Single execution report snapshot
```rust
pub struct FillReport {
    pub account_id: AccountId,
    pub instrument_id: InstrumentId,
    pub venue_order_id: VenueOrderId,
    pub trade_id: TradeId,
    pub order_side: OrderSide,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub commission: Money,
    pub liquidity_side: LiquiditySide,
    pub client_order_id: Option<ClientOrderId>,
    pub venue_position_id: Option<PositionId>,
    // timestamps...
}
```

**OrderStatusReport:** Order state snapshot from venue

**PositionStatusReport:** Position state snapshot from venue

---

## 6. Positions

### Position Structure

**File:** `crates/model/src/position.rs`

```rust
pub struct Position {
    // Event tracking
    pub events: Vec<OrderFilled>,
    pub adjustments: Vec<PositionAdjusted>,

    // Identifiers
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub closing_order_id: Option<ClientOrderId>,

    // Position State
    pub entry: OrderSide,              // Entry direction
    pub side: PositionSide,            // Long, Short, Flat
    pub signed_qty: f64,               // +ve for long, -ve for short
    pub quantity: Quantity,            // Absolute open quantity
    pub peak_qty: Quantity,            // Highest quantity reached

    // Instrument Properties
    pub price_precision: u8,
    pub size_precision: u8,
    pub multiplier: Quantity,
    pub is_inverse: bool,
    pub is_currency_pair: bool,
    pub instrument_class: InstrumentClass,
    pub base_currency: Option<Currency>,
    pub quote_currency: Currency,
    pub settlement_currency: Currency,

    // Timing
    pub ts_init: UnixNanos,
    pub ts_opened: UnixNanos,
    pub ts_last: UnixNanos,
    pub ts_closed: Option<UnixNanos>,
    pub duration_ns: u64,

    // Pricing & PnL
    pub avg_px_open: f64,
    pub avg_px_close: Option<f64>,
    pub realized_return: f64,
    pub realized_pnl: Option<Money>,

    // Trade tracking
    pub trade_ids: Vec<TradeId>,
    pub buy_qty: Quantity,
    pub sell_qty: Quantity,
    pub commissions: AHashMap<Currency, Money>,
}
```

### PositionSide

**File:** `crates/model/src/enums.rs`

| Value | Description |
|-------|-------------|
| `NoPositionSide` | Unspecified (filter-only) |
| `Flat` | No position (quantity = 0) |
| `Long` | Positive quantity (signed_qty > 0) |
| `Short` | Negative quantity (signed_qty < 0) |

### Position Lifecycle

1. **Creation:** `Position::new(instrument, first_fill)`
2. **Modification:** `position.apply(fill)` for subsequent fills
3. **Adjustment:** `position.apply_adjustment()` for commissions/funding
4. **Closing:** Opposite-side fills reduce quantity to zero

### Position Calculations

| Method | Description |
|--------|-------------|
| `calculate_avg_px()` | Weighted average price calculation |
| `calculate_pnl()` | Realized PnL between entry and exit |
| `unrealized_pnl(last_price)` | Mark-to-market unrealized PnL |
| `total_pnl(last_price)` | Realized + Unrealized |
| `calculate_return()` | Percentage return on capital |
| `notional_value(last_price)` | Market value of position |

### Position Events

| Event | Trigger |
|-------|---------|
| `PositionOpened` | Flat → Long/Short |
| `PositionChanged` | Position modified (add to existing) |
| `PositionClosed` | Long/Short → Flat |
| `PositionAdjusted` | Commission or funding applied |

---

## 7. Accounts

### Account Types

**File:** `crates/model/src/accounts/`

#### CashAccount

```rust
pub struct CashAccount {
    pub base: BaseAccount,
    pub allow_borrowing: bool,
}
```

- Unleveraged positions only
- Simple balance tracking
- `is_unleveraged()` always returns `true`

#### MarginAccount

```rust
pub struct MarginAccount {
    pub base: BaseAccount,
    pub leverages: AHashMap<InstrumentId, Decimal>,
    pub margins: AHashMap<InstrumentId, MarginBalance>,
    pub default_leverage: Decimal,
}
```

- Supports leveraged positions
- Per-instrument leverage tracking
- Initial and maintenance margin tracking

### BaseAccount Fields

**File:** `crates/model/src/accounts/base.rs`

```rust
pub struct BaseAccount {
    pub id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub calculate_account_state: bool,
    pub events: Vec<AccountState>,
    pub commissions: AHashMap<Currency, f64>,
    pub balances: AHashMap<Currency, AccountBalance>,
    pub balances_starting: AHashMap<Currency, Money>,
}
```

### AccountBalance

**File:** `crates/model/src/types/balance.rs`

```rust
pub struct AccountBalance {
    pub currency: Currency,
    pub total: Money,      // Total balance
    pub locked: Money,     // Locked in pending orders
    pub free: Money,       // Available (total - locked)
}

pub struct MarginBalance {
    pub initial: Money,        // Initial margin requirement
    pub maintenance: Money,    // Maintenance margin
    pub currency: Currency,
    pub instrument_id: InstrumentId,
}
```

**Invariant:** `total == locked + free`

### AccountState Event

**File:** `crates/model/src/events/account/state.rs`

```rust
pub struct AccountState {
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub balances: Vec<AccountBalance>,
    pub margins: Vec<MarginBalance>,
    pub is_reported: bool,         // From exchange vs calculated
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

### Account Trait Interface

```rust
pub trait Account {
    // Identity
    fn id(&self) -> AccountId;
    fn account_type(&self) -> AccountType;

    // Balances
    fn balance_total(&self, currency: Option<Currency>) -> Option<Money>;
    fn balance_free(&self, currency: Option<Currency>) -> Option<Money>;
    fn balance_locked(&self, currency: Option<Currency>) -> Option<Money>;

    // Events
    fn apply(&mut self, event: AccountState);
    fn last_event(&self) -> Option<AccountState>;

    // Calculations
    fn calculate_balance_locked(...) -> Money;
    fn calculate_pnls(...) -> Vec<Money>;
    fn calculate_commission(...) -> Money;
}
```

---

## 8. Timers and Clocks

### TimeEvent

**File:** `crates/common/src/timer.rs`

```rust
#[repr(C)]
pub struct TimeEvent {
    pub name: Ustr,              // Timer/alert name
    pub event_id: UUID4,         // Unique event ID
    pub ts_event: UnixNanos,     // When event fired
    pub ts_init: UnixNanos,      // When created
}
```

Represents a scheduled time event fired by a timer or alert.

### Timer Types

**TestTimer** (for backtesting):

**File:** `crates/common/src/timer.rs`

```rust
pub struct TestTimer {
    pub name: Ustr,
    pub interval_ns: NonZeroU64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    pub fire_immediately: bool,
    next_time_ns: UnixNanos,
    is_expired: bool,
}
```

**LiveTimer** (for live trading):

**File:** `crates/common/src/live/timer.rs`

```rust
pub struct LiveTimer {
    pub name: Ustr,
    pub interval_ns: NonZeroU64,
    pub start_time_ns: UnixNanos,
    pub stop_time_ns: Option<UnixNanos>,
    pub fire_immediately: bool,
    next_time_ns: Arc<AtomicU64>,
    callback: TimeEventCallback,
    task_handle: Option<JoinHandle<()>>,
}
```

### Clock Types

**File:** `crates/common/src/clock.rs`

**Clock Trait Interface:**

```rust
pub trait Clock {
    // Time queries
    fn timestamp_ns(&self) -> u64;
    fn timestamp_us(&self) -> u64;
    fn timestamp_ms(&self) -> u64;
    fn timestamp(&self) -> f64;
    fn utc_now(&self) -> DateTime<Utc>;

    // Timer management
    fn timer_names(&self) -> Vec<&str>;
    fn timer_count(&self) -> usize;
    fn set_time_alert_ns(&mut self, name, alert_time_ns, callback);
    fn set_timer_ns(&mut self, name, interval_ns, start_time_ns, stop_time_ns, callback);
    fn next_time_ns(&self, name) -> Option<UnixNanos>;
    fn cancel_timer(&mut self, name);
    fn cancel_timers(&mut self);
}
```

**TestClock:**
```rust
pub struct TestClock {
    time: AtomicTime,
    timers: BTreeMap<Ustr, TestTimer>,
    callbacks: CallbackRegistry,
}
```

- Manual time advancement for deterministic testing
- `advance_time(to_time_ns)` returns all fired events

**LiveClock:**
```rust
pub struct LiveClock {
    time: &'static AtomicTime,
    timers: BTreeMap<Ustr, LiveTimer>,
    callbacks: CallbackRegistry,
}
```

- Uses real system time
- Spawns Tokio async tasks for real-time scheduling

### TimeEventCallback

```rust
pub enum TimeEventCallback {
    Python(Py<PyAny>),                        // Python callable
    Rust(Arc<dyn Fn(TimeEvent) + Send + Sync>), // Thread-safe
    RustLocal(Rc<dyn Fn(TimeEvent)>),         // Single-threaded
}
```

---

## 9. Risk Management

### RiskEngine

**File:** `crates/risk/src/engine/mod.rs`

```rust
pub struct RiskEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    portfolio: Portfolio,
    pub throttled_submit_order: Throttler<SubmitOrder, SubmitOrderFn>,
    pub throttled_modify_order: Throttler<ModifyOrder, ModifyOrderFn>,
    max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    trading_state: TradingState,
    config: RiskEngineConfig,
}
```

**Key Responsibilities:**
- Pre-trade order validation
- Order/position limit enforcement
- Trading state management
- Rate limiting for submissions/modifications

### TradingState

**File:** `crates/model/src/enums.rs`

```rust
pub enum TradingState {
    Active = 1,    // Normal trading operations
    Halted = 2,    // No new orders allowed
    Reducing = 3,  // Only position-reducing orders
}
```

### RiskEngineConfig

**File:** `crates/risk/src/engine/config.rs`

```rust
pub struct RiskEngineConfig {
    pub bypass: bool,
    pub max_order_submit: RateLimit,   // Default: 100/second
    pub max_order_modify: RateLimit,   // Default: 100/second
    pub max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    pub debug: bool,
}
```

### Throttler

**File:** `crates/common/src/throttler.rs`

```rust
pub struct RateLimit {
    pub limit: usize,       // Messages allowed
    pub interval_ns: u64,   // Time window
}

pub struct Throttler<T, F> {
    pub recv_count: usize,
    pub sent_count: usize,
    pub is_limiting: bool,
    pub limit: usize,
    pub buffer: VecDeque<T>,
    pub timestamps: VecDeque<UnixNanos>,
    // ...
}
```

### Risk Checks Performed

| Check | Description |
|-------|-------------|
| GTD Expiry | GTD orders must expire in future |
| Price Precision | Price matches instrument precision |
| Quantity Limits | Quantity within min/max bounds |
| Notional Limits | Notional within min/max bounds |
| Free Balance | Notional ≤ available balance |
| Reduce-Only | Position exists and would reduce |

### Position Sizing

**File:** `crates/risk/src/sizing.rs`

```rust
pub fn calculate_fixed_risk_position_size(
    instrument: InstrumentAny,
    entry: Price,
    stop_loss: Price,
    equity: Money,
    risk: Decimal,
    commission_rate: Decimal,
    exchange_rate: Decimal,
    hard_limit: Option<Decimal>,
    unit_batch_size: Decimal,
    units: usize,
) -> Quantity
```

---

## 10. Message Bus

### MessageBus Structure

**File:** `crates/common/src/msgbus/core.rs`

```rust
pub struct MessageBus {
    pub trader_id: TraderId,
    pub instance_id: UUID4,
    pub name: String,
    switchboard: MessagingSwitchboard,
    subscriptions: AHashSet<Subscription>,
    topics: IndexMap<Topic, Vec<Subscription>>,
    endpoints: IndexMap<Endpoint, ShareableMessageHandler>,
    correlation_index: AHashMap<UUID4, ...>,
}
```

### Subscription Patterns

Supports wildcard pattern matching:
- `*` - matches zero or more characters
- `?` - matches exactly one character

**Examples:**
- `data.quotes.*` - all quotes
- `data.*.BINANCE.*` - all data from BINANCE venue
- `events.order.*` - all order events

### Handler Types

**File:** `crates/common/src/msgbus/handler.rs`

```rust
pub trait MessageHandler: Any {
    fn id(&self) -> Ustr;
    fn handle(&self, message: &dyn Any);
}

pub trait Handler<T>: 'static {
    fn id(&self) -> Ustr;
    fn handle(&self, message: &T);
}
```

### TopicRouter (Typed Routing)

**File:** `crates/common/src/msgbus/typed_router.rs`

```rust
pub struct TopicRouter<T> {
    subscriptions: Vec<TypedSubscription<T>>,
    topic_cache: IndexMap<Topic, SmallVec<[usize; 64]>>,
}
```

High-performance typed routing for specific data types.

### Built-in Routers

The message bus includes typed routers for:
- `QuoteTick`, `TradeTick`, `Bar`
- `OrderBookDeltas`, `OrderBookDepth10`
- `MarkPriceUpdate`, `IndexPriceUpdate`, `FundingRateUpdate`
- `OrderEventAny`, `PositionEvent`, `AccountState`

---

## 11. Strategy and Actor

### Actor Trait

**File:** `crates/common/src/actor/data_actor.rs`

```rust
pub trait DataActor: Component + Deref<Target = DataActorCore> {
    fn on_save(&self) -> anyhow::Result<HashMap<String, Bytes>>;
    fn on_load(&mut self, state: HashMap<String, Bytes>);
    fn on_start(&mut self);
    fn on_stop(&mut self);
    fn on_resume(&mut self);
    fn on_order_filled(&mut self, fill: &OrderFilled);
    fn on_order_canceled(&mut self, canceled: &OrderCanceled);
}
```

### DataActorCore

```rust
pub struct DataActorCore {
    pub id: ComponentId,
    pub state: ComponentState,
    pub clock: Rc<RefCell<dyn Clock>>,
    pub cache: Rc<RefCell<Cache>>,
    pub msgbus: Rc<RefCell<MessageBus>>,
    // subscriptions, indicators, etc.
}
```

### Strategy Trait

**File:** `crates/trading/src/strategy/mod.rs`

```rust
pub trait Strategy: DataActor {
    fn core_mut(&mut self) -> &mut StrategyCore;

    // Order management
    fn submit_order(&mut self, order: OrderAny);
    fn modify_order(&mut self, order: &OrderAny, ...);
    fn cancel_order(&mut self, order: &OrderAny);
    fn close_position(&mut self, position: &Position, ...);

    // Event handlers (default empty implementations)
    fn on_order_initialized(&mut self, event: &OrderInitialized) {}
    fn on_order_denied(&mut self, event: &OrderDenied) {}
    fn on_order_submitted(&mut self, event: &OrderSubmitted) {}
    fn on_order_accepted(&mut self, event: &OrderAccepted) {}
    fn on_order_rejected(&mut self, event: &OrderRejected) {}
    fn on_order_filled(&mut self, event: &OrderFilled) {}
    fn on_position_opened(&mut self, event: &PositionOpened) {}
    fn on_position_changed(&mut self, event: &PositionChanged) {}
    fn on_position_closed(&mut self, event: &PositionClosed) {}
    // ... more handlers
}
```

### StrategyCore

**File:** `crates/trading/src/strategy/core.rs`

```rust
pub struct StrategyCore {
    pub actor: DataActorCore,
    pub config: StrategyConfig,
    pub order_manager: Option<OrderManager>,
    pub order_factory: Option<OrderFactory>,
    pub portfolio: Option<Rc<RefCell<Portfolio>>>,
    pub gtd_timers: AHashMap<ClientOrderId, Ustr>,
}
```

### StrategyConfig

**File:** `crates/trading/src/strategy/config.rs`

```rust
pub struct StrategyConfig {
    pub strategy_id: Option<StrategyId>,
    pub order_id_tag: Option<String>,
    pub use_uuid_client_order_ids: bool,
    pub oms_type: Option<OmsType>,
    pub external_order_claims: Option<Vec<InstrumentId>>,
    pub manage_contingent_orders: bool,
    pub manage_gtd_expiry: bool,
    pub log_events: bool,
    pub log_commands: bool,
}
```

### OmsType (Order Management System)

| Type | Description |
|------|-------------|
| `Unspecified` | Defaults to venue-specific |
| `Hedging` | Position ID per new position |
| `Netting` | Single position per instrument |

---

## 12. Cache

### Cache Structure

**File:** `crates/common/src/cache/mod.rs`

```rust
pub struct Cache {
    config: CacheConfig,
    index: CacheIndex,

    // Market Data
    quotes: AHashMap<InstrumentId, VecDeque<QuoteTick>>,
    trades: AHashMap<InstrumentId, VecDeque<TradeTick>>,
    bars: AHashMap<BarType, VecDeque<Bar>>,
    books: AHashMap<InstrumentId, OrderBook>,

    // Pricing
    mark_prices: AHashMap<InstrumentId, VecDeque<MarkPriceUpdate>>,
    index_prices: AHashMap<InstrumentId, VecDeque<IndexPriceUpdate>>,
    funding_rates: AHashMap<InstrumentId, FundingRateUpdate>,

    // Domain Objects
    currencies: AHashMap<Ustr, Currency>,
    instruments: AHashMap<InstrumentId, InstrumentAny>,
    synthetics: AHashMap<InstrumentId, SyntheticInstrument>,

    // Execution Data
    accounts: AHashMap<AccountId, AccountAny>,
    orders: AHashMap<ClientOrderId, OrderAny>,
    order_lists: AHashMap<OrderListId, OrderList>,
    positions: AHashMap<PositionId, Position>,

    // Optional backing
    database: Option<Box<dyn CacheDatabaseAdapter>>,
}
```

### CacheIndex

**File:** `crates/common/src/cache/index.rs`

Fast lookup indices for efficient queries:

```rust
pub struct CacheIndex {
    // Order indexing
    venue_orders: AHashMap<Venue, AHashSet<ClientOrderId>>,
    instrument_orders: AHashMap<InstrumentId, AHashSet<ClientOrderId>>,
    strategy_orders: AHashMap<StrategyId, AHashSet<ClientOrderId>>,
    orders_open: AHashSet<ClientOrderId>,
    orders_closed: AHashSet<ClientOrderId>,
    orders_emulated: AHashSet<ClientOrderId>,
    orders_inflight: AHashSet<ClientOrderId>,

    // Position indexing
    venue_positions: AHashMap<Venue, AHashSet<PositionId>>,
    instrument_positions: AHashMap<InstrumentId, AHashSet<PositionId>>,
    strategy_positions: AHashMap<StrategyId, AHashSet<PositionId>>,
    positions_open: AHashSet<PositionId>,
    positions_closed: AHashSet<PositionId>,

    // Mappings
    venue_order_ids: AHashMap<VenueOrderId, ClientOrderId>,
    order_position: AHashMap<ClientOrderId, PositionId>,
    order_strategy: AHashMap<ClientOrderId, StrategyId>,
}
```

### CacheConfig

**File:** `crates/common/src/cache/config.rs`

```rust
pub struct CacheConfig {
    pub database: Option<DatabaseConfig>,
    pub encoding: SerializationEncoding,
    pub tick_capacity: usize,          // Default: 10,000
    pub bar_capacity: usize,           // Default: 10,000
    pub save_market_data: bool,
    pub flush_on_start: bool,
}
```

### Key Query Methods

```rust
// Single lookups
fn account(account_id: &AccountId) -> Option<&AccountAny>
fn order(client_order_id: &ClientOrderId) -> Option<&OrderAny>
fn position(position_id: &PositionId) -> Option<&Position>
fn instrument(instrument_id: &InstrumentId) -> Option<&InstrumentAny>

// Filtered collections
fn orders_open(venue, instrument_id, strategy_id, side) -> Vec<&OrderAny>
fn positions_open(venue, instrument_id, strategy_id, side) -> Vec<&Position>

// State checks
fn order_exists(client_order_id) -> bool
fn is_order_open(client_order_id) -> bool
fn position_exists(position_id) -> bool
fn is_position_open(position_id) -> bool
```

---

## 13. Portfolio

### Portfolio Structure

**File:** `crates/portfolio/src/portfolio.rs`

```rust
pub struct Portfolio {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    inner: Rc<RefCell<PortfolioState>>,
    config: PortfolioConfig,
}

struct PortfolioState {
    accounts: AccountsManager,
    analyzer: PortfolioAnalyzer,
    unrealized_pnls: AHashMap<InstrumentId, Money>,
    realized_pnls: AHashMap<InstrumentId, Money>,
    net_positions: AHashMap<InstrumentId, Decimal>,
    pending_calcs: AHashSet<InstrumentId>,
}
```

### PortfolioConfig

**File:** `crates/portfolio/src/config.rs`

```rust
pub struct PortfolioConfig {
    pub use_mark_prices: bool,
    pub use_mark_xrates: bool,
    pub bar_updates: bool,
    pub convert_to_account_base_currency: bool,
    pub debug: bool,
}
```

### Portfolio Calculations

| Method | Description |
|--------|-------------|
| `unrealized_pnl(instrument_id)` | Mark-to-market unrealized PnL |
| `realized_pnl(instrument_id)` | Realized PnL from closed trades |
| `total_pnl(instrument_id)` | Realized + Unrealized |
| `net_exposures(venue)` | Net exposure per currency |
| `balances_locked(venue)` | Locked balances per currency |
| `margins_init(venue)` | Initial margin per instrument |
| `is_net_long(instrument_id)` | Check if net long |
| `is_net_short(instrument_id)` | Check if net short |
| `is_flat(instrument_id)` | Check if flat |

### Message Bus Subscriptions

The Portfolio subscribes to:
- `events.account.*` - Account state changes
- `events.position.*` - Position events
- `events.order.*` - Order events
- `data.quotes.*` - Quote tick updates
- `data.bars.*EXTERNAL` - Bar updates (if enabled)
- `data.mark_prices.*` - Mark price updates (if enabled)

---

## 14. Sessions

### ForexSession

Represents major Forex market sessions.

**File:** `crates/trading/src/sessions.rs`

```rust
pub enum ForexSession {
    Sydney,   // 0700-1600 Australia/Sydney
    Tokyo,    // 0900-1800 Asia/Tokyo
    London,   // 0800-1600 Europe/London
    NewYork,  // 0800-1700 America/New_York
}
```

All FX sessions run Monday to Friday local time.

**Session Functions:**

| Function | Description |
|----------|-------------|
| `fx_local_from_utc(session, time)` | Convert UTC to session local time |
| `fx_next_start(session, time)` | Next session start in UTC |
| `fx_prev_start(session, time)` | Previous session start in UTC |
| `fx_next_end(session, time)` | Next session end in UTC |
| `fx_prev_end(session, time)` | Previous session end in UTC |

### InstrumentClose

Represents session/contract close prices.

**File:** `crates/model/src/data/close.rs`

```rust
pub struct InstrumentClose {
    pub instrument_id: InstrumentId,
    pub close_price: Price,
    pub close_type: InstrumentCloseType,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
```

### InstrumentCloseType

**File:** `crates/model/src/enums.rs`

| Value | Description |
|-------|-------------|
| `EndOfSession` | Market session ended |
| `ContractExpired` | Instrument expiration reached |

### DataBackendSession

Data query session for persistence/backtesting.

**File:** `crates/persistence/src/backend/session.rs`

```rust
pub struct DataBackendSession {
    pub chunk_size: usize,
    pub runtime: Arc<tokio::runtime::Runtime>,
    session_ctx: SessionContext,          // DataFusion
    batch_streams: Vec<EagerStream<...>>,
    registered_tables: AHashSet<String>,
}
```

Used for registering Parquet files and querying historical data with DataFusion.

---

## 15. Domain Object Relationships

### Relationship Diagram

```
                              ┌─────────────────┐
                              │   Instrument    │
                              │  (InstrumentId) │
                              └────────┬────────┘
                                       │
       ┌───────────────┬───────────────┼───────────────┬───────────────┐
       │               │               │               │               │
       ▼               ▼               ▼               ▼               ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│  QuoteTick  │ │  TradeTick  │ │  OrderBook  │ │    Order    │ │    Bar      │
└─────────────┘ └─────────────┘ └─────────────┘ └──────┬──────┘ └─────────────┘
       │               │                               │
       └───────────────┴───────────────┬───────────────┘
                                       │
                                       ▼
                               ┌───────────────┐
                               │   Strategy    │◄────────┐
                               │   (Actor)     │         │
                               └───────┬───────┘         │
                                       │                 │
                    ┌──────────────────┼─────────────────┤
                    │                  │                 │
                    ▼                  ▼                 │
             ┌─────────────┐    ┌─────────────┐         │
             │ RiskEngine  │    │ OrderFilled │         │
             └──────┬──────┘    └──────┬──────┘         │
                    │                  │                 │
                    │                  ▼                 │
                    │           ┌─────────────┐         │
                    │           │  Position   │─────────┘
                    │           └──────┬──────┘
                    │                  │
                    ▼                  ▼
             ┌─────────────┐    ┌─────────────┐
             │  Portfolio  │◄───│   Account   │
             └─────────────┘    └─────────────┘
                    │
                    ▼
             ┌─────────────┐
             │    Cache    │◄──── (stores all domain objects)
             └─────────────┘

  Supporting Infrastructure:
  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
  │ MessageBus  │    │    Clock    │    │   Timers    │
  └─────────────┘    └─────────────┘    └─────────────┘
```

### Key Identifier Relationships

| Parent | Child | Link Field |
|--------|-------|------------|
| `Instrument` | `Order` | `instrument_id` |
| `Instrument` | `Position` | `instrument_id` |
| `Instrument` | `QuoteTick` | `instrument_id` |
| `Instrument` | `TradeTick` | `instrument_id` |
| `Instrument` | `OrderBook` | `instrument_id` |
| `Instrument` | `Bar` | `bar_type.instrument_id` |
| `Order` | `Position` | `position_id` |
| `Order` | `OrderFilled` | `client_order_id` |
| `Position` | `OrderFilled` | events vector |
| `Account` | `Order` | `account_id` |
| `Account` | `Position` | `account_id` |
| `Strategy` | `Order` | `strategy_id` |
| `Strategy` | `Position` | `strategy_id` |
| `Clock` | `Timer` | timer name |
| `Clock` | `TimeEvent` | callback |
| `Cache` | `All Objects` | indexed storage |
| `Portfolio` | `Account` | via cache |
| `Portfolio` | `Position` | via cache |
| `RiskEngine` | `Order` | validation |
| `MessageBus` | `Handler` | subscription pattern |

### Data Flow

**Market Data Flow:**
1. **Adapters** receive raw data from exchanges (WebSocket/REST)
2. Data parsed into domain objects (`QuoteTick`, `TradeTick`, `OrderBookDelta`, etc.)
3. Objects published to **Message Bus** with `instrument_id` routing
4. **Cache** stores data with configurable capacity
5. **Strategies** receive data via pattern subscriptions

**Order Flow:**
1. **Strategy** generates order via `submit_order()`
2. **RiskEngine** validates order (limits, balances, trading state)
3. **Throttler** applies rate limiting
4. Order published to **Execution Engine**
5. **Adapter** routes order to venue
6. Venue sends acknowledgments/fills
7. **OrderFilled** events flow back through message bus
8. **Cache** updates order state
9. **Position** created/updated from fills
10. **Portfolio** recalculates PnL and exposures
11. **Account** balances updated
12. **Strategy** receives event callbacks

**Timer Flow:**
1. **Strategy** sets timer via `set_timer_ns()`
2. **Clock** schedules timer (TestClock or LiveClock)
3. Timer fires at scheduled time
4. **TimeEvent** delivered via callback
5. **Strategy** handles event in `on_time_event()`

### File Locations Summary

| Component | Path |
|-----------|------|
| **Identifiers** | `crates/model/src/identifiers/` |
| **Instruments** | `crates/model/src/instruments/` |
| **Data Types** | `crates/model/src/data/` |
| **OrderBook** | `crates/model/src/orderbook/` |
| **Orders** | `crates/model/src/orders/` |
| **Order Events** | `crates/model/src/events/order/` |
| **Positions** | `crates/model/src/position.rs` |
| **Position Events** | `crates/model/src/events/position/` |
| **Accounts** | `crates/model/src/accounts/` |
| **Account Events** | `crates/model/src/events/account/` |
| **Enums** | `crates/model/src/enums.rs` |
| **Reports** | `crates/model/src/reports/` |
| **Timers** | `crates/common/src/timer.rs` |
| **Clock** | `crates/common/src/clock.rs` |
| **LiveClock** | `crates/common/src/live/clock.rs` |
| **Risk Engine** | `crates/risk/src/engine/` |
| **Throttler** | `crates/common/src/throttler.rs` |
| **Message Bus** | `crates/common/src/msgbus/` |
| **Actor** | `crates/common/src/actor/` |
| **Strategy** | `crates/trading/src/strategy/` |
| **Cache** | `crates/common/src/cache/` |
| **Portfolio** | `crates/portfolio/src/` |
| **Sessions** | `crates/trading/src/sessions.rs` |
