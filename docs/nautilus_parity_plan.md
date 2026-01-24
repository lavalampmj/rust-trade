# NautilusTrader Parity: Orders & Executions

## Overview

Achieve parity with NautilusTrader's domain model for Orders (Section 4) and Executions (Section 5) to enable realistic backtesting with proper exchange simulation, latency modeling, and order matching.

---

## Gap Analysis Summary

| Feature | Our Implementation | NautilusTrader | Gap |
|---------|-------------------|----------------|-----|
| Order Types | 7 types | 9+ types | Missing MarketIfTouched, TrailingStopMarket/Limit distinction |
| Order Event History | Not stored | `events: Vec<OrderEventAny>` on order | **Major gap** |
| Execution Timestamps | Only init_time, ts_last | ts_submitted, ts_accepted, ts_closed | **Gap** |
| SimulatedExchange | None | Full venue simulation | **Critical gap** |
| LatencyModel | None | insert/update/delete latencies | **Critical gap** |
| OrderMatchingEngine | None | Per-instrument matching | **Critical gap** |
| Liquidity Consumption | None | Tracks consumed qty per price | **Gap** |
| Probabilistic Fills | Deterministic | prob_fill_on_limit, prob_slippage | **Gap** |
| OrderFactory | None | Auto-generates strategy-tagged IDs | **Gap** |
| Contingent Orders | Types defined, not managed | Full OTO/OCO/OUO automation | **Gap** |
| OrderBook Integration | Exists but unused | Drives matching engine | **Gap** |
| Execution Reports | None | FillReport, OrderStatusReport | Minor gap |

---

## Implementation Phases

### Phase 1: Order Model Enhancements

**File: `trading-common/src/orders/order.rs`**

Add to `Order` struct:
```rust
// Event History
pub events: Vec<OrderEventAny>,

// Timestamps
pub ts_submitted: Option<DateTime<Utc>>,
pub ts_accepted: Option<DateTime<Utc>>,
pub ts_closed: Option<DateTime<Utc>>,

// Multi-currency commissions
pub commissions: HashMap<String, Decimal>,

// Execution algorithm (future)
pub exec_algorithm_id: Option<String>,
pub exec_algorithm_params: Option<HashMap<String, String>>,
```

**File: `trading-common/src/orders/types.rs`**

Add to `OrderType`:
```rust
MarketIfTouched,
TrailingStopMarket,
TrailingStopLimit,
```

**New File: `trading-common/src/orders/factory.rs`**
```rust
pub struct OrderFactory {
    strategy_id: StrategyId,
    account_id: AccountId,
    venue: String,
    order_counter: AtomicU64,
}

impl OrderFactory {
    pub fn market(&self, symbol: &str, side: OrderSide, qty: Decimal) -> Order;
    pub fn limit(&self, symbol: &str, side: OrderSide, qty: Decimal, price: Decimal) -> Order;
    pub fn bracket_order(...) -> OrderList;
}
```

**New File: `trading-common/src/orders/order_list.rs`**
```rust
pub struct OrderList {
    pub id: OrderListId,
    pub orders: Vec<Order>,
    pub contingency_type: ContingencyType,
}
```

---

### Phase 2: Latency Modeling

**New File: `trading-common/src/execution/latency_model.rs`**
```rust
pub trait LatencyModel: Send + Sync + Debug {
    fn insert_latency_nanos(&self) -> u64;
    fn update_latency_nanos(&self) -> u64;
    fn delete_latency_nanos(&self) -> u64;
}

pub struct FixedLatencyModel { ... }
pub struct VariableLatencyModel { ... }
pub struct NoLatencyModel;
```

**New File: `trading-common/src/execution/inflight_queue.rs`**
```rust
pub enum TradingCommand {
    SubmitOrder(Order),
    CancelOrder(ClientOrderId),
    ModifyOrder { ... },
}

pub struct InflightCommand {
    pub command: TradingCommand,
    pub arrival_time_ns: u64,
}

pub struct InflightQueue {
    queue: BinaryHeap<Reverse<InflightCommand>>,
}
```

---

### Phase 3: Order Matching Engine

**Critical: Works with ANY data granularity (L2, L1, ticks, or bars only)**

The matching engine does NOT require L2 or tick data. It supports multiple data modes:

| Data Available | How Matching Works |
|----------------|-------------------|
| **Bars only** | Use OHLC extremes: buy limit fills if `low <= limit`, sell limit if `high >= limit` |
| **Last ticks** | Use last trade price for stop triggers and market fills |
| **L1 quotes** | Use bid/ask spread for realistic market order fills |
| **L2 depth** | Full order book matching with liquidity consumption |

**New File: `trading-common/src/execution/matching/core.rs`**
```rust
pub struct OrderMatchingCore {
    trigger_orders: Vec<Order>,  // Stops waiting for trigger
    resting_orders: Vec<Order>,  // Limits at price
}

impl OrderMatchingCore {
    pub fn check_triggers(&mut self, bid: Decimal, ask: Decimal, last: Decimal) -> Vec<Order>;
    pub fn match_orders(&mut self, ...) -> Vec<MatchResult>;
}
```

**New File: `trading-common/src/execution/matching/engine.rs`**
```rust
pub struct OrderMatchingEngine {
    pub instrument_id: InstrumentId,
    book: Option<OrderBook>,  // Optional! Only for L2 mode
    core: OrderMatchingCore,
    bid_consumption: HashMap<Decimal, Decimal>,
    ask_consumption: HashMap<Decimal, Decimal>,
    config: MatchingEngineConfig,

    // Price state (updated from ANY data source)
    last_bid: Option<Decimal>,
    last_ask: Option<Decimal>,
    last_trade: Option<Decimal>,
}

impl OrderMatchingEngine {
    /// Process OHLC bar - MINIMUM data requirement
    pub fn process_bar(&mut self, bar: &OHLCData) -> Vec<OrderFilled> {
        // Buy limit: fills if low <= limit price
        // Sell limit: fills if high >= limit price
        // Stop triggers: check if high/low crossed trigger
        // Market orders: fill at close price
        self.match_with_bar_range(bar.open, bar.high, bar.low, bar.close)
    }

    /// Process trade tick (if available)
    pub fn process_trade_tick(&mut self, trade: &TickData) -> Vec<OrderFilled>;

    /// Process quote tick (if available)
    pub fn process_quote_tick(&mut self, quote: &QuoteTick) -> Vec<OrderFilled>;

    /// Process L2 book delta (most realistic, if available)
    pub fn process_book_delta(&mut self, delta: &OrderBookDelta) -> Vec<OrderFilled>;

    pub fn add_order(&mut self, order: Order);
    pub fn cancel_order(&mut self, id: &ClientOrderId) -> Option<OrderCanceled>;
}

pub struct MatchingEngineConfig {
    pub bar_execution: bool,       // Use OHLC (default: true)
    pub trade_execution: bool,     // Use trade ticks
    pub quote_execution: bool,     // Use quote ticks
    pub book_execution: bool,      // Use L2 (requires OrderBook)
    pub liquidity_consumption: bool,
    pub reject_stop_orders: bool,
    pub use_random_fills: bool,
}
```

---

### Phase 4: Probabilistic Fill Model

**File: `trading-common/src/execution/fill_model.rs`** (enhance)

Add:
```rust
pub struct ProbabilisticFillModel {
    pub prob_fill_on_limit: f64,  // 0.0-1.0
    pub prob_slippage: f64,       // 0.0-1.0
    pub max_slippage_ticks: u32,
    seed: u64,
    rng: StdRng,
}

impl ProbabilisticFillModel {
    pub fn new(prob_fill_on_limit: f64, prob_slippage: f64, seed: u64) -> Self;
    pub fn set_seed(&mut self, seed: u64);  // For reproducibility
}
```

---

### Phase 5: Simulated Exchange

**New File: `trading-common/src/execution/simulated_exchange.rs`**
```rust
pub struct SimulatedExchange {
    pub venue: String,
    matching_engines: HashMap<InstrumentId, OrderMatchingEngine>,
    fill_model: Box<dyn FillModel>,
    fee_model: Box<dyn FeeModel>,
    latency_model: Box<dyn LatencyModel>,
    inflight_queue: InflightQueue,
    current_time_ns: u64,  // Tracks simulated time (from market data)
    config: SimulatedExchangeConfig,
}

impl SimulatedExchange {
    pub fn new(venue: &str, config: SimulatedExchangeConfig) -> Self;
    pub fn with_fill_model(self, model: Box<dyn FillModel>) -> Self;
    pub fn with_latency_model(self, model: Box<dyn LatencyModel>) -> Self;

    // === TIME MANAGEMENT (Critical for synchronization) ===

    /// Advance simulated time to match incoming market data
    /// Called by BacktestEngine BEFORE processing data
    pub fn advance_time(&mut self, new_time_ns: u64) {
        self.current_time_ns = new_time_ns;
    }

    /// Process commands that have "arrived" after latency delay
    /// A command with arrival_time <= current_time_ns is ready
    pub fn process_inflight_commands(&mut self) -> Vec<OrderEventAny> {
        let ready_commands = self.inflight_queue.pop_ready(self.current_time_ns);
        let mut events = Vec::new();

        for cmd in ready_commands {
            match cmd {
                TradingCommand::SubmitOrder(order) => {
                    // NOW the order enters the matching engine
                    events.push(OrderEventAny::Accepted(...));
                    self.get_engine(&order.instrument_id).add_order(order);
                }
                TradingCommand::CancelOrder(id) => { ... }
                TradingCommand::ModifyOrder { .. } => { ... }
            }
        }
        events
    }

    // === ORDER OPERATIONS (queue with latency) ===

    /// Submit order - queued with insert_latency
    pub fn submit_order(&mut self, order: Order) {
        let arrival_time = self.current_time_ns
            + self.latency_model.insert_latency_nanos();
        self.inflight_queue.push(
            TradingCommand::SubmitOrder(order),
            arrival_time
        );
        // Order won't be processed until advance_time() passes arrival_time
    }

    pub fn cancel_order(&mut self, client_order_id: &ClientOrderId);
    pub fn modify_order(&mut self, ...);

    // === MARKET DATA PROCESSING ===

    pub fn process_quote_tick(&mut self, quote: &QuoteTick) -> Vec<OrderEventAny>;
    pub fn process_trade_tick(&mut self, trade: &TickData) -> Vec<OrderEventAny>;
    pub fn process_bar(&mut self, bar: &OHLCData) -> Vec<OrderEventAny>;
}

pub struct SimulatedExchangeConfig {
    pub bar_execution: bool,
    pub trade_execution: bool,
    pub liquidity_consumption: bool,
    pub reject_stop_orders: bool,
    pub matching_config: MatchingEngineConfig,
}
```

**Latency Timeline Example:**
```
Time 0ns:   Strategy submits buy order
            → Order enters inflight_queue with arrival = 0 + 50ms = 50_000_000ns

Time 1ms:   Next bar arrives, exchange.advance_time(1_000_000ns)
            → inflight check: 1_000_000 < 50_000_000, order still queued

Time 50ms:  Bar arrives, exchange.advance_time(50_000_000ns)
            → inflight check: 50_000_000 >= 50_000_000, order ready!
            → Order moves to matching engine
            → OrderAccepted event generated

Time 51ms:  Price crosses limit
            → Order fills, OrderFilled event
```

---

### Phase 6: Contingent Order Management

**Handles automatic OTO for bracket orders (entry → stop loss + take profit targets)**

| Contingency Type | Trigger | Action |
|-----------------|---------|--------|
| **OTO** (One-Triggers-Other) | Entry order fills | Submit stop loss + take profit limit orders |
| **OCO** (One-Cancels-Other) | Stop loss OR take profit fills | Cancel the other (prevents double exit) |
| **OUO** (One-Updates-Other) | Partial fill on entry | Reduce qty on stop loss + take profit |

**Example: Bracket Order Flow**
```
1. Strategy submits bracket: Entry(Buy), StopLoss(Sell), TakeProfit(Sell)
   → All 3 orders registered in OrderList with OTO + OCO contingencies

2. Entry fills at $100
   → OTO triggers: StopLoss and TakeProfit submitted to exchange

3. Price rises to $110, TakeProfit fills
   → OCO triggers: StopLoss automatically canceled (prevents double exit)

4. Portfolio: Long closed with profit, no orphan orders
```

**New File: `trading-common/src/orders/contingent_manager.rs`**
```rust
pub struct ContingentOrderManager {
    order_lists: HashMap<OrderListId, OrderList>,
    order_to_list: HashMap<ClientOrderId, OrderListId>,
}

impl ContingentOrderManager {
    pub fn register_list(&mut self, list: OrderList);

    /// Called on every order event - returns actions to take
    pub fn on_order_event(&mut self, event: &OrderEventAny) -> Vec<ContingentAction> {
        match event {
            OrderEventAny::Filled(fill) => {
                // Check if this order has linked contingent orders
                if let Some(list) = self.get_list_for_order(&fill.client_order_id) {
                    match list.contingency_type {
                        ContingencyType::OTO => {
                            // Entry filled -> submit linked orders (stop, target)
                            vec![ContingentAction::SubmitOrders(list.child_orders())]
                        }
                        ContingencyType::OCO => {
                            // One side filled -> cancel other side
                            vec![ContingentAction::CancelOrders(list.sibling_orders(&fill.client_order_id))]
                        }
                        _ => vec![]
                    }
                }
            }
            OrderEventAny::Canceled(_) => {
                // OCO: If one canceled, cancel siblings
            }
            _ => vec![]
        }
    }
}

pub enum ContingentAction {
    SubmitOrders(Vec<Order>),      // OTO: entry filled -> submit stops + targets
    CancelOrders(Vec<ClientOrderId>), // OCO: one filled/canceled -> cancel others
    UpdateOrders(Vec<(ClientOrderId, Decimal)>), // OUO: partial fill -> update qty
}
```

---

### Phase 7: Backtest Engine Integration

**Critical: Synchronization Model**

The BacktestEngine is a **single-threaded event loop** that guarantees the exchange and strategy see the **exact same data** in strict order:

```
┌─────────────────────────────────────────────────────────────┐
│                    BACKTEST EVENT LOOP                       │
│                  (Single-threaded, deterministic)            │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  for each bar_data in chronological order:                  │
│                                                             │
│    ┌─────────────────────────────────────────┐              │
│    │ Step 1: EXCHANGE PROCESSES DATA FIRST   │              │
│    │                                         │              │
│    │ exchange.process_bar(&bar_data)         │              │
│    │   - Advance time to bar_data.timestamp  │              │
│    │   - Process inflight commands (latency) │              │
│    │   - Match orders against new prices     │              │
│    │   - Generate fill events                │              │
│    └─────────────────────────────────────────┘              │
│                      ↓                                      │
│    ┌─────────────────────────────────────────┐              │
│    │ Step 2: HANDLE FILL EVENTS              │              │
│    │                                         │              │
│    │ for event in fill_events:               │              │
│    │   - Update portfolio positions          │              │
│    │   - Process contingent orders           │              │
│    │   - Update equity curve                 │              │
│    └─────────────────────────────────────────┘              │
│                      ↓                                      │
│    ┌─────────────────────────────────────────┐              │
│    │ Step 3: STRATEGY SEES SAME DATA         │              │
│    │                                         │              │
│    │ strategy.on_bar_data(&bar_data, bars)   │              │
│    │   - Strategy sees IDENTICAL bar_data    │              │
│    │   - Can check portfolio for fills       │              │
│    │   - Makes trading decisions             │              │
│    └─────────────────────────────────────────┘              │
│                      ↓                                      │
│    ┌─────────────────────────────────────────┐              │
│    │ Step 4: SUBMIT NEW ORDERS (WITH LATENCY)│              │
│    │                                         │              │
│    │ orders = strategy.get_orders()          │              │
│    │ for order in orders:                    │              │
│    │   exchange.submit_order(order)          │              │
│    │   → Order enters inflight_queue         │              │
│    │   → arrival_time = now + latency        │              │
│    │   → WON'T fill until future bar!        │              │
│    └─────────────────────────────────────────┘              │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

**Why This Works:**
1. **Single source of truth**: `bar_data` passed to both exchange AND strategy
2. **Strict ordering**: Exchange processes BEFORE strategy decides
3. **Latency isolation**: New orders queue with latency, can't fill on same bar
4. **Deterministic**: Same input → same output (with seeded RNG)

**File: `trading-common/src/backtest/engine.rs`** (modify)

```rust
pub struct BacktestEngine {
    // ... existing fields ...
    exchanges: HashMap<String, SimulatedExchange>,
    contingent_manager: ContingentOrderManager,
    order_factory: OrderFactory,
}

impl BacktestEngine {
    pub fn with_exchange(mut self, exchange: SimulatedExchange) -> Self;
    pub fn with_fill_model(mut self, model: Box<dyn FillModel>) -> Self;
    pub fn with_latency_model(mut self, model: Box<dyn LatencyModel>) -> Self;

    pub fn run_unified(&mut self, data: BacktestData) -> BacktestResult {
        for bar_data in bar_events {
            // === SYNCHRONIZATION POINT: Same bar_data for all ===

            // Step 1: Exchange processes market data FIRST
            //         - Advances simulated time
            //         - Processes latent commands that have "arrived"
            //         - Matches orders against new prices
            let fill_events = self.process_market_data(&bar_data);

            // Step 2: Handle fills (portfolio updates, contingent orders)
            for event in fill_events {
                self.handle_order_event(&event);
            }

            // Step 3: Strategy sees IDENTICAL bar_data
            //         (after fills are applied, so it can react)
            self.strategy.on_bar_data(&bar_data, &mut self.bars_context);

            // Step 4: New orders queue with latency
            //         (won't process until FUTURE bar_data arrives)
            let orders = self.strategy.get_orders(&bar_data, &mut self.bars_context);
            for order in orders {
                self.submit_order(order);  // → inflight_queue
            }
        }

        self.calculate_results()
    }

    fn process_market_data(&mut self, bar_data: &BarData) -> Vec<OrderEventAny> {
        let timestamp_ns = bar_data.timestamp.timestamp_nanos() as u64;
        let mut all_events = Vec::new();

        for exchange in self.exchanges.values_mut() {
            // Advance exchange time to match bar_data
            exchange.advance_time(timestamp_ns);

            // Process any commands that have "arrived" after latency
            let latent_events = exchange.process_inflight_commands();
            all_events.extend(latent_events);

            // Process bar through matching engine
            let fill_events = exchange.process_bar(&bar_data.ohlc_bar);
            all_events.extend(fill_events);
        }

        all_events
    }
}
```

---

### Phase 8: Execution Reports

**New File: `trading-common/src/execution/reports.rs`**
```rust
pub struct FillReport {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub trade_id: TradeId,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub last_qty: Decimal,
    pub last_px: Decimal,
    pub commission: Decimal,
    pub liquidity_side: LiquiditySide,
    pub reconciliation: bool,
}

pub struct OrderStatusReport { ... }
pub struct PositionStatusReport { ... }
```

---

## Dependency Graph

```
Phase 1 (Order Enhancements)
    ↓
Phase 2 (Latency Model)
    ↓
Phase 3 (Matching Engine) ← uses OrderBook from data/orderbook.rs
    ↓
Phase 4 (Probabilistic Fills)
    ↓
Phase 5 (SimulatedExchange) ← integrates 2, 3, 4
    ↓
Phase 6 (Contingent Orders)
    ↓
Phase 7 (Backtest Integration) ← integrates 5, 6
    ↓
Phase 8 (Reports)
```

---

## New Files to Create

| Path | Description |
|------|-------------|
| `trading-common/src/orders/factory.rs` | OrderFactory for creating orders |
| `trading-common/src/orders/order_list.rs` | OrderList for bracket orders |
| `trading-common/src/orders/contingent_manager.rs` | OTO/OCO/OUO management |
| `trading-common/src/execution/latency_model.rs` | LatencyModel trait + implementations |
| `trading-common/src/execution/inflight_queue.rs` | Command queue with latency |
| `trading-common/src/execution/matching/mod.rs` | Matching engine module |
| `trading-common/src/execution/matching/core.rs` | Core matching logic |
| `trading-common/src/execution/matching/engine.rs` | Per-instrument matching engine |
| `trading-common/src/execution/simulated_exchange.rs` | SimulatedExchange |
| `trading-common/src/execution/reports.rs` | Execution reports |

---

## Files to Modify

| Path | Changes |
|------|---------|
| `trading-common/src/orders/order.rs` | Add events, timestamps, commissions fields |
| `trading-common/src/orders/types.rs` | Add MarketIfTouched, TrailingStopMarket/Limit |
| `trading-common/src/orders/mod.rs` | Export new modules |
| `trading-common/src/execution/fill_model.rs` | Add ProbabilisticFillModel |
| `trading-common/src/execution/mod.rs` | Export new modules |
| `trading-common/src/backtest/engine.rs` | Integrate SimulatedExchange |

---

## Synchronization Guarantees

**Q: How is the exchange guaranteed to work with the same data as the strategy?**

**A: Single-threaded event loop with shared `bar_data` reference**

```rust
// BacktestEngine owns the bar_data and passes SAME reference to both
for bar_data in bar_events {
    // 1. Exchange sees bar_data FIRST
    let events = exchange.process_bar(&bar_data.ohlc_bar);

    // 2. Strategy sees IDENTICAL bar_data AFTER
    strategy.on_bar_data(&bar_data, &mut bars);
}
```

**Key guarantees:**
1. **Same data**: Both receive reference to identical `bar_data` struct
2. **Strict ordering**: Exchange processes → fills applied → strategy decides
3. **No race conditions**: Single-threaded, no async within the loop
4. **Time synchronization**: Exchange's `current_time_ns` always matches `bar_data.timestamp`
5. **Deterministic**: Given same inputs and RNG seed, produces identical results

**What prevents look-ahead bias?**
- Strategy's new orders enter `inflight_queue` with future arrival time
- Orders can't fill on the SAME bar that triggered them
- Latency ensures realistic delay between decision and execution

---

## TOML Configuration

All execution/matching parameters loaded from config files.

**File: `config/development.toml`** (or `production.toml`)

```toml
# =============================================================================
# SIMULATED EXCHANGE CONFIGURATION
# =============================================================================

[exchange]
# Default venue name
default_venue = "SIMULATED"

# Execution modes - which data types trigger order matching
bar_execution = true        # Use OHLC bars for matching
trade_execution = true      # Use trade ticks for matching
quote_execution = true      # Use quote ticks for matching
book_execution = false      # Use L2 order book (requires book data)

# Order handling
reject_stop_orders = false  # Reject stop orders (some venues don't support)
support_gtd_orders = true   # Support Good-Till-Date orders
support_contingent_orders = true  # Support OTO/OCO/OUO

# =============================================================================
# LATENCY MODEL CONFIGURATION
# =============================================================================

[exchange.latency]
# Model type: "none", "fixed", "variable"
model = "fixed"

# Fixed latency (nanoseconds) - used if model = "fixed"
insert_latency_ns = 50_000_000   # 50ms order submission
update_latency_ns = 50_000_000   # 50ms order modification
delete_latency_ns = 30_000_000   # 30ms order cancellation

# Variable latency (nanoseconds) - used if model = "variable"
base_latency_ns = 30_000_000     # Base latency
jitter_ns = 20_000_000           # Random +/- jitter

# =============================================================================
# FILL MODEL CONFIGURATION
# =============================================================================

[exchange.fill_model]
# Model type: "immediate", "limit_aware", "slippage_aware", "probabilistic"
model = "limit_aware"

# Probabilistic fill settings (used if model = "probabilistic")
prob_fill_on_limit = 0.8    # Probability limit fills when price touched (0.0-1.0)
prob_slippage = 0.2         # Probability of slippage on market orders (0.0-1.0)
max_slippage_ticks = 2      # Maximum slippage in ticks
seed = 42                   # RNG seed for reproducibility (0 = random seed)

# Slippage-aware settings (used if model = "slippage_aware")
base_slippage_pct = 0.001   # Base slippage percentage (0.1%)
volume_impact = 0.0001      # Additional slippage per unit volume
max_slippage_pct = 0.05     # Cap slippage at 5%

# =============================================================================
# MATCHING ENGINE CONFIGURATION
# =============================================================================

[exchange.matching]
# Liquidity tracking
liquidity_consumption = false   # Track consumed liquidity at each price level
reset_consumption_on_bar = true # Reset consumption when new bar arrives

# Fill behavior
use_random_fills = false        # Add randomness to fill determination
partial_fills_enabled = true    # Allow partial fills
min_fill_pct = 0.1              # Minimum fill percentage when partial

# Bar execution specifics
bar_open_fills = false          # Fill market orders at bar open (vs close)
use_ohlc_range = true           # Use high/low to determine if limit touched

# =============================================================================
# FEE MODEL CONFIGURATION
# =============================================================================

[exchange.fees]
# Model type: "percentage", "tiered", "fixed", "hybrid", "zero"
model = "percentage"

# Percentage fees (used if model = "percentage")
maker_rate = 0.001    # 0.1% maker fee
taker_rate = 0.001    # 0.1% taker fee

# Fixed fees (used if model = "fixed" or "hybrid")
fixed_per_fill = 0.0  # Fixed fee per fill
min_fee = 0.0         # Minimum fee
max_fee = 1000000.0   # Maximum fee cap

# Tiered fees (used if model = "tiered")
tier_volume_thresholds = [0, 1000000, 5000000, 10000000]  # 30-day volume tiers
tier_maker_rates = [0.001, 0.0009, 0.0008, 0.0007]        # Maker rates per tier
tier_taker_rates = [0.001, 0.0009, 0.00085, 0.00075]      # Taker rates per tier

# =============================================================================
# CONTINGENT ORDER MANAGEMENT
# =============================================================================

[exchange.contingent]
manage_contingent_orders = true  # Auto-manage OTO/OCO/OUO
manage_gtd_expiry = true         # Auto-cancel expired GTD orders
oto_submit_delay_ns = 0          # Delay before submitting OTO children
oco_cancel_delay_ns = 0          # Delay before canceling OCO siblings
```

**Loading in Rust:**

```rust
// trading-common/src/execution/config.rs
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeConfig {
    pub default_venue: String,
    pub bar_execution: bool,
    pub trade_execution: bool,
    pub quote_execution: bool,
    pub book_execution: bool,
    pub reject_stop_orders: bool,
    pub support_gtd_orders: bool,
    pub support_contingent_orders: bool,
    pub latency: LatencyConfig,
    pub fill_model: FillModelConfig,
    pub matching: MatchingConfig,
    pub fees: FeeConfig,
    pub contingent: ContingentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LatencyConfig {
    pub model: String,  // "none", "fixed", "variable"
    pub insert_latency_ns: u64,
    pub update_latency_ns: u64,
    pub delete_latency_ns: u64,
    pub base_latency_ns: u64,
    pub jitter_ns: u64,
}

// Similar structs for FillModelConfig, MatchingConfig, FeeConfig, ContingentConfig

impl ExchangeConfig {
    pub fn load_from_toml(path: &str) -> Result<Self, ConfigError> {
        let config = config::Config::builder()
            .add_source(config::File::with_name(path))
            .build()?;
        config.get::<ExchangeConfig>("exchange")
    }
}
```

---

## Backward Compatibility

1. **Default behavior preserved**: Without configuration, backtest uses `NoLatencyModel` + `LimitAwareFillModel`
2. **Builder pattern opt-in**: Advanced features via `.with_exchange()`, `.with_latency_model()`
3. **Existing strategies work**: `get_orders()` pattern unchanged
4. **TOML optional**: Can construct config programmatically if TOML not provided

---

## Verification

### Per-Phase Tests

**Phase 1**: Orders store event history; timestamps populated on transitions
**Phase 2**: InflightQueue processes commands after latency delay
**Phase 3**: MatchingEngine fills limits only when price crosses; tracks liquidity consumption
**Phase 4**: ProbabilisticFillModel reproducible with same seed
**Phase 5**: SimulatedExchange integrates all components; events emitted correctly
**Phase 6**: OCO cancels linked orders; OTO submits stops on entry fill
**Phase 7**: Different latency models produce different backtest results

### Integration Test
```bash
cargo test --package trading-common matching
cargo test --package trading-common simulated_exchange
cargo test --package trading-common backtest
```

### Manual Verification
1. Run backtest with `NoLatencyModel` vs `FixedLatencyModel(100ms)` - results should differ
2. Run backtest with `prob_fill_on_limit=1.0` vs `0.5` - fill count should differ
3. Submit bracket order - verify stop/TP submitted after entry fill

---

## Priority Recommendation

**High Impact (Implement First)**:
1. Phase 5: SimulatedExchange (core architecture)
2. Phase 3: OrderMatchingEngine (realistic fills)
3. Phase 2: LatencyModel (realistic timing)

**Medium Impact**:
4. Phase 4: ProbabilisticFillModel
5. Phase 6: ContingentOrderManager
6. Phase 1: Order enhancements

**Lower Priority**:
7. Phase 7: Full integration
8. Phase 8: Reports
