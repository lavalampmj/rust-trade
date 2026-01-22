# rust-trade Order Management System Architecture

## Overview

This document describes the comprehensive order management, instrument, account, market data, and risk management infrastructure added to rust-trade. The architecture follows professional trading system patterns, adapted for Rust and crypto markets.

## Module Structure

```
trading-common/src/
├── orders/           # Order management system
│   ├── types.rs      # Core enums and ID types
│   ├── order.rs      # Order struct and builder
│   ├── events.rs     # Order lifecycle events
│   └── manager.rs    # Order tracking and management
├── execution/        # Execution and fill simulation
│   ├── context.rs    # Strategy execution context
│   ├── fill_model.rs # Fill simulation models
│   └── engine.rs     # Execution engine
├── instruments/      # Instrument definitions
│   ├── types.rs      # Asset classes, currencies, venues
│   └── instrument.rs # Instrument trait and implementations
├── accounts/         # Account management
│   ├── types.rs      # Account types and states
│   └── account.rs    # Account and balance tracking
├── data/
│   ├── quotes.rs     # Quote tick data
│   └── orderbook.rs  # Order book structures
└── risk/             # Risk management
    ├── types.rs      # Trading states and risk levels
    ├── fee_model.rs  # Fee calculation models
    └── engine.rs     # Risk engine and limits
```

---

## 1. Orders Module

### 1.1 Enums

#### OrderSide
```rust
pub enum OrderSide {
    Buy,   // Acquire the base asset
    Sell,  // Dispose of the base asset
}
```

#### OrderType
```rust
pub enum OrderType {
    Market,         // Execute immediately at best price
    Limit,          // Execute at specified price or better
    Stop,           // Becomes market when trigger hit
    StopLimit,      // Becomes limit when trigger hit
    TrailingStop,   // Stop trails market by offset
    MarketToLimit,  // Market that converts to limit
    LimitIfTouched, // Becomes limit when touched
}
```

#### OrderStatus
```rust
pub enum OrderStatus {
    Initialized,    // Created but not submitted
    Submitted,      // Sent to execution system
    Denied,         // Pre-trade risk check failed
    Accepted,       // Accepted by venue
    Rejected,       // Rejected by venue
    PendingCancel,  // Cancellation in progress
    Canceled,       // Canceled (terminal)
    PartiallyFilled,// Partial execution
    Filled,         // Fully executed (terminal)
    Expired,        // Time expired (terminal)
    PendingUpdate,  // Modification in progress
    Triggered,      // Stop/conditional triggered
}
```

**State Machine:**
```
Initialized → Submitted → Accepted ─┬→ Filled
                   │                 ├→ PartiallyFilled → Filled
                   │                 ├→ Canceled
                   │                 ├→ Expired
                   │                 └→ PendingCancel → Canceled
                   └→ Rejected
```

#### TimeInForce
```rust
pub enum TimeInForce {
    GTC,        // Good-Till-Canceled
    IOC,        // Immediate-Or-Cancel
    FOK,        // Fill-Or-Kill
    GTD,        // Good-Till-Date
    Day,        // End of trading day
    AtTheOpen,  // Execute at market open
    AtTheClose, // Execute at market close
}
```

#### Other Enums
- **ContingencyType**: None, OCO, OTO, OUO
- **TriggerType**: None, LastPrice, BidPrice, AskPrice, MidPrice, MarkPrice, IndexPrice
- **TrailingOffsetType**: None, Price, BasisPoints, Ticks, PriceTier
- **PositionSide**: Flat, Long, Short
- **LiquiditySide**: None, Maker, Taker

### 1.2 ID Types

```rust
pub struct ClientOrderId(pub String);   // Client-assigned order ID
pub struct VenueOrderId(pub String);    // Exchange-assigned order ID
pub struct TradeId(pub String);         // Fill/execution ID
pub struct StrategyId(pub String);      // Strategy identifier
pub struct AccountId(pub String);       // Trading account ID
pub struct PositionId(pub String);      // Position identifier
pub struct OrderListId(pub String);     // Linked orders group ID

pub struct InstrumentId {
    pub symbol: String,  // e.g., "BTCUSDT"
    pub venue: String,   // e.g., "BINANCE"
}
```

### 1.3 Value Types

```rust
pub struct Money {
    pub amount: Decimal,
    pub currency: String,
}

pub struct Quantity {
    pub raw: Decimal,
    pub precision: u8,
}

pub struct Price {
    pub raw: Decimal,
    pub precision: u8,
}
```

### 1.4 Order Struct

```rust
pub struct Order {
    // Identifiers
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument_id: InstrumentId,

    // Order specification
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Decimal,
    pub price: Option<Decimal>,           // For limit orders
    pub trigger_price: Option<Decimal>,   // For stop orders
    pub time_in_force: TimeInForce,
    pub expire_time: Option<DateTime<Utc>>,

    // Trailing stop
    pub trailing_offset: Option<Decimal>,
    pub trailing_offset_type: TrailingOffsetType,
    pub trigger_type: TriggerType,

    // Relationships
    pub strategy_id: StrategyId,
    pub account_id: AccountId,
    pub position_id: Option<PositionId>,
    pub order_list_id: Option<OrderListId>,
    pub linked_order_ids: Vec<ClientOrderId>,
    pub contingency_type: ContingencyType,

    // State
    pub status: OrderStatus,
    pub filled_qty: Decimal,
    pub avg_price: Decimal,
    pub commission: Decimal,

    // Flags
    pub is_reduce_only: bool,
    pub is_post_only: bool,

    // Timestamps
    pub init_time: DateTime<Utc>,
    pub last_update_time: DateTime<Utc>,

    // Metadata
    pub tags: HashMap<String, String>,
}
```

**Key Methods:**
- `leaves_qty()` - Remaining quantity to fill
- `is_filled()` / `is_open()` / `is_terminal()` - State checks
- `apply_fill(qty, price, commission)` - Record a fill
- `can_transition_to(status)` - Validate state transition

### 1.5 OrderBuilder

```rust
let order = OrderBuilder::new(OrderType::Limit, "BTCUSDT", OrderSide::Buy, dec!(1.0))
    .with_venue("BINANCE")
    .with_price(dec!(50000))
    .with_time_in_force(TimeInForce::GTC)
    .with_strategy_id("sma_crossover")
    .with_reduce_only(false)
    .with_tag("signal", "golden_cross")
    .build()?;
```

### 1.6 Order Events

All events implement the `OrderEvent` trait:

```rust
pub trait OrderEvent: Send + Sync {
    fn client_order_id(&self) -> &ClientOrderId;
    fn venue_order_id(&self) -> Option<&VenueOrderId>;
    fn instrument_id(&self) -> &InstrumentId;
    fn strategy_id(&self) -> &StrategyId;
    fn account_id(&self) -> &AccountId;
    fn ts_event(&self) -> DateTime<Utc>;
    fn implied_status(&self) -> OrderStatus;
}
```

**Event Types:**

| Event | Description | Terminal |
|-------|-------------|----------|
| `OrderInitialized` | Order created | No |
| `OrderSubmitted` | Sent to venue | No |
| `OrderAccepted` | Venue acknowledged | No |
| `OrderRejected` | Venue rejected | Yes |
| `OrderCanceled` | Order canceled | Yes |
| `OrderExpired` | Time expired | Yes |
| `OrderFilled` | Partial or complete fill | Depends |
| `OrderUpdated` | Order modified | No |
| `OrderTriggered` | Stop triggered | No |

**OrderFilled Event:**
```rust
pub struct OrderFilled {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument_id: InstrumentId,
    pub strategy_id: StrategyId,
    pub account_id: AccountId,
    pub trade_id: TradeId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub last_qty: Decimal,      // This fill quantity
    pub last_px: Decimal,       // This fill price
    pub cum_qty: Decimal,       // Total filled so far
    pub leaves_qty: Decimal,    // Remaining quantity
    pub avg_px: Decimal,        // Volume-weighted average price
    pub commission: Decimal,
    pub liquidity_side: LiquiditySide,
    pub ts_event: DateTime<Utc>,
}
```

**Unified Event Enum:**
```rust
pub enum OrderEventAny {
    Initialized(OrderInitialized),
    Submitted(OrderSubmitted),
    Accepted(OrderAccepted),
    Rejected(OrderRejected),
    Canceled(OrderCanceled),
    Expired(OrderExpired),
    Filled(OrderFilled),
    Updated(OrderUpdated),
    Triggered(OrderTriggered),
}
```

### 1.7 OrderManager

Thread-safe order tracking with async support:

```rust
pub struct OrderManager {
    orders: Arc<RwLock<HashMap<ClientOrderId, Order>>>,
    venue_to_client: Arc<RwLock<HashMap<VenueOrderId, ClientOrderId>>>,
    orders_by_instrument: Arc<RwLock<HashMap<InstrumentId, Vec<ClientOrderId>>>>,
    orders_by_strategy: Arc<RwLock<HashMap<StrategyId, Vec<ClientOrderId>>>>,
    event_log: Arc<RwLock<Vec<OrderEventAny>>>,
    max_open_orders: Option<usize>,
}
```

**Key Methods:**
```rust
// Lifecycle
async fn submit(&self, order: Order) -> Result<(), OrderManagerError>
async fn cancel(&self, id: &ClientOrderId) -> Result<(), OrderManagerError>
async fn apply_event(&self, event: OrderEventAny) -> Result<(), OrderManagerError>

// Queries
async fn get(&self, id: &ClientOrderId) -> Option<Order>
async fn get_by_venue_id(&self, venue_id: &VenueOrderId) -> Option<Order>
async fn get_open_orders(&self) -> Vec<Order>
async fn get_orders_by_instrument(&self, id: &InstrumentId) -> Vec<Order>
async fn get_orders_by_strategy(&self, id: &StrategyId) -> Vec<Order>

// Statistics
async fn stats(&self) -> OrderManagerStats
```

**SyncOrderManager** - Synchronous wrapper for backtesting.

---

## 2. Execution Module

### 2.1 StrategyContext

Provides order management capabilities to strategies:

```rust
pub struct StrategyContext {
    config: StrategyContextConfig,
    order_manager: Arc<OrderManager>,
    positions: HashMap<String, Decimal>,
    cash: Decimal,
    pending_orders: Vec<Order>,
    pending_cancellations: Vec<ClientOrderId>,
}
```

**Order Submission Methods:**
```rust
fn market_order(&mut self, symbol: &str, side: OrderSide, quantity: Decimal)
    -> Result<ClientOrderId, ContextError>

fn limit_order(&mut self, symbol: &str, side: OrderSide, quantity: Decimal, price: Decimal)
    -> Result<ClientOrderId, ContextError>

fn stop_order(&mut self, symbol: &str, side: OrderSide, quantity: Decimal, stop_price: Decimal)
    -> Result<ClientOrderId, ContextError>

fn cancel_order(&mut self, order_id: &ClientOrderId)
    -> Result<(), ContextError>
```

**Portfolio Queries:**
```rust
fn position(&self, symbol: &str) -> Decimal
fn cash(&self) -> Decimal
fn portfolio_value(&self, prices: &HashMap<String, Decimal>) -> Decimal
```

### 2.2 FillModel Trait

```rust
pub trait FillModel: Send + Sync {
    fn try_fill(
        &self,
        order: &Order,
        market: &MarketSnapshot,
    ) -> Option<FillResult>;
}

pub struct MarketSnapshot {
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub last_price: Decimal,
    pub bid_price: Option<Decimal>,
    pub ask_price: Option<Decimal>,
    pub bid_size: Option<Decimal>,
    pub ask_size: Option<Decimal>,
}

pub struct FillResult {
    pub fill_price: Decimal,
    pub fill_qty: Decimal,
    pub commission: Decimal,
    pub liquidity_side: LiquiditySide,
}
```

**Implementations:**

| Model | Description |
|-------|-------------|
| `ImmediateFillModel` | Market orders fill at last price immediately |
| `LimitAwareFillModel` | Limit orders fill only when price crosses |
| `SlippageAwareFillModel` | Adds configurable slippage to market orders |

### 2.3 ExecutionEngine

Processes orders during backtesting:

```rust
pub struct ExecutionEngine {
    fill_model: Box<dyn FillModel>,
    pending_orders: Vec<Order>,
    positions: HashMap<String, Decimal>,
    cash: Decimal,
    metrics: ExecutionMetrics,
}
```

**Key Methods:**
```rust
// Order processing
fn submit_order(&mut self, order: Order)
fn on_bar(&mut self, bar_data: &BarData, bars: &BarsContext) -> Vec<OrderFilled>
fn process_orders_for_symbol(&mut self, symbol: &str) -> Vec<OrderFilled>

// Signal conversion (backward compatibility)
fn execute_signal(&mut self, signal: &Signal, current_price: Decimal)
    -> Result<Option<ClientOrderId>, ContextError>

// State management
fn update_position(&mut self, symbol: &str, fill: &OrderFilled)
fn reset(&mut self)
```

**ExecutionMetrics:**
```rust
pub struct ExecutionMetrics {
    pub orders_submitted: u64,
    pub orders_filled: u64,
    pub orders_canceled: u64,
    pub total_commission: Decimal,
    pub total_volume: Decimal,
}
```

---

## 3. Instruments Module

### 3.1 Enums

#### AssetClass
```rust
pub enum AssetClass {
    Crypto,      // Cryptocurrencies
    FX,          // Foreign exchange
    Equity,      // Stocks
    Commodity,   // Physical commodities
    Bond,        // Fixed income
    Index,       // Index instruments
    Alternative, // Other assets
}
```

#### InstrumentClass (SecurityType)
```rust
pub enum InstrumentClass {
    Spot,             // Direct ownership
    PerpetualLinear,  // Settled in quote currency
    PerpetualInverse, // Settled in base currency
    Future,           // Dated futures
    Option,           // Options contracts
    Betting,          // Prediction markets
    CFD,              // Contracts for difference
    Warrant,          // Warrants
}
```

#### CurrencyType
```rust
pub enum CurrencyType {
    Fiat,            // USD, EUR, etc.
    Crypto,          // BTC, ETH, etc.
    Stablecoin,      // USDT, USDC, etc.
    CommodityBacked, // Gold-backed, etc.
}
```

#### VenueType
```rust
pub enum VenueType {
    Exchange,   // Centralized exchange
    DEX,        // Decentralized exchange
    OTC,        // Over-the-counter
    DarkPool,   // Dark pool
    Simulated,  // Paper trading
}
```

### 3.2 Currency and Venue

```rust
pub struct Currency {
    pub code: String,       // "USD", "BTC", "USDT"
    pub precision: u8,      // Decimal places
    pub currency_type: CurrencyType,
}

pub struct Venue {
    pub code: String,       // "BINANCE", "KRAKEN"
    pub venue_type: VenueType,
}
```

### 3.3 InstrumentSpecs

Trading constraints for an instrument:

```rust
pub struct InstrumentSpecs {
    pub tick_size: Decimal,       // Minimum price movement
    pub price_precision: u8,      // Price decimal places
    pub lot_size: Decimal,        // Minimum quantity
    pub size_precision: u8,       // Quantity decimal places
    pub min_quantity: Decimal,    // Minimum order qty
    pub max_quantity: Option<Decimal>,
    pub min_notional: Option<Decimal>,
    pub multiplier: Decimal,      // Contract multiplier
    pub maker_fee: Decimal,
    pub taker_fee: Decimal,
    pub margin_init: Option<Decimal>,    // Initial margin
    pub margin_maint: Option<Decimal>,   // Maintenance margin
}
```

**Key Methods:**
```rust
fn round_price(&self, price: Decimal) -> Decimal
fn round_quantity(&self, quantity: Decimal) -> Decimal
fn validate_price(&self, price: Decimal) -> Result<(), String>
fn validate_quantity(&self, quantity: Decimal) -> Result<(), String>
fn validate_notional(&self, price: Decimal, quantity: Decimal) -> Result<(), String>
fn notional(&self, price: Decimal, quantity: Decimal) -> Decimal
fn calc_maker_fee(&self, price: Decimal, quantity: Decimal) -> Decimal
fn calc_taker_fee(&self, price: Decimal, quantity: Decimal) -> Decimal
fn calc_init_margin(&self, price: Decimal, quantity: Decimal) -> Option<Decimal>
```

### 3.4 Instrument Trait

```rust
pub trait Instrument: Send + Sync + fmt::Debug {
    fn id(&self) -> &InstrumentId;
    fn symbol(&self) -> &str;
    fn venue(&self) -> &Venue;
    fn asset_class(&self) -> AssetClass;
    fn instrument_class(&self) -> InstrumentClass;
    fn base_currency(&self) -> &Currency;
    fn quote_currency(&self) -> &Currency;
    fn settlement_currency(&self) -> &Currency;
    fn specs(&self) -> &InstrumentSpecs;
    fn is_active(&self) -> bool;
    fn expiry(&self) -> Option<DateTime<Utc>>;
    fn strike_price(&self) -> Option<Decimal>;  // For options
    fn option_type(&self) -> Option<OptionType>;
}
```

### 3.5 Instrument Implementations

```rust
pub struct CryptoSpot {
    pub id: InstrumentId,
    pub venue: Venue,
    pub base_currency: Currency,
    pub quote_currency: Currency,
    pub specs: InstrumentSpecs,
    pub is_active: bool,
}

pub struct CryptoPerpetual {
    // ... similar fields plus:
    pub is_linear: bool,
    pub max_leverage: Decimal,
    pub funding_rate_interval_hours: u32,
}

pub struct CryptoFuture {
    // ... similar fields plus:
    pub expiry: DateTime<Utc>,
}
```

**Type-Erased Wrapper:**
```rust
pub enum InstrumentAny {
    CryptoSpot(CryptoSpot),
    CryptoPerpetual(CryptoPerpetual),
    CryptoFuture(CryptoFuture),
}
```

---

## 4. Accounts Module

### 4.1 Enums

#### AccountType
```rust
pub enum AccountType {
    Cash,    // No leverage, full collateral
    Margin,  // Allows leverage
    Betting, // Prediction markets
}
```

#### AccountState
```rust
pub enum AccountState {
    Active,      // Normal trading
    Suspended,   // Suspended (margin call)
    Liquidating, // Positions being closed
    Closed,      // Account closed
}
```

#### MarginMode
```rust
pub enum MarginMode {
    Cross,    // All positions share margin
    Isolated, // Each position has dedicated margin
}
```

#### PositionMode
```rust
pub enum PositionMode {
    Hedge,  // Can hold long and short simultaneously
    OneWay, // Only one direction allowed
}
```

### 4.2 AccountBalance

```rust
pub struct AccountBalance {
    pub currency: String,
    pub total: Decimal,   // Total balance
    pub free: Decimal,    // Available for trading
    pub locked: Decimal,  // In orders/margin
}
```

**Key Methods:**
```rust
fn lock(&mut self, amount: Decimal) -> Result<(), String>
fn unlock(&mut self, amount: Decimal) -> Result<(), String>
fn deposit(&mut self, amount: Decimal)
fn withdraw(&mut self, amount: Decimal) -> Result<(), String>
fn fill(&mut self, amount: Decimal) -> Result<(), String>  // Deduct from locked
fn has_sufficient(&self, amount: Decimal) -> bool
```

### 4.3 MarginAccount

```rust
pub struct MarginAccount {
    pub margin_balance: Decimal,
    pub margin_used: Decimal,
    pub margin_available: Decimal,
    pub margin_initial: Decimal,
    pub margin_maintenance: Decimal,
    pub unrealized_pnl: Decimal,
    pub max_leverage: Decimal,
    pub leverage: Decimal,
    pub margin_mode: MarginMode,
    pub position_mode: PositionMode,
    pub margin_call_level: Decimal,
    pub liquidation_level: Decimal,
}
```

**Key Methods:**
```rust
fn margin_ratio(&self) -> Decimal
fn equity(&self) -> Decimal  // balance + unrealized PnL
fn is_margin_call(&self) -> bool
fn is_liquidation(&self) -> bool
fn available_for_trading(&self) -> Decimal
fn buying_power(&self) -> Decimal  // available * leverage
fn update_margin(&mut self, used: Decimal, maintenance: Decimal)
fn update_unrealized_pnl(&mut self, pnl: Decimal)
```

### 4.4 Account

```rust
pub struct Account {
    pub id: AccountId,
    pub account_type: AccountType,
    pub state: AccountState,
    pub base_currency: String,
    pub balances: HashMap<String, AccountBalance>,
    pub margin: Option<MarginAccount>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_simulated: bool,
}
```

**Factory Methods:**
```rust
Account::cash(id, base_currency) -> Self
Account::margin(id, base_currency, max_leverage) -> Self
Account::simulated(id, base_currency) -> Self
```

**Key Methods:**
```rust
fn balance(&self, currency: &str) -> Option<&AccountBalance>
fn set_balance(&mut self, currency: &str, total: Decimal)
fn deposit(&mut self, currency: &str, amount: Decimal)
fn withdraw(&mut self, currency: &str, amount: Decimal) -> Result<(), String>
fn total_value(&self, prices: &HashMap<String, Decimal>) -> Decimal
fn free_base(&self) -> Decimal
fn total_base(&self) -> Decimal
fn can_trade(&self) -> bool
fn is_reduce_only(&self) -> bool
fn buying_power(&self) -> Decimal
fn equity(&self) -> Decimal
fn suspend(&mut self)
fn activate(&mut self)
fn close(&mut self)
```

### 4.5 AccountEvent

```rust
pub enum AccountEvent {
    Created { account_id: AccountId, timestamp: DateTime<Utc> },
    BalanceUpdated {
        account_id: AccountId,
        currency: String,
        old_balance: AccountBalance,
        new_balance: AccountBalance,
        timestamp: DateTime<Utc>,
    },
    StateChanged {
        account_id: AccountId,
        old_state: AccountState,
        new_state: AccountState,
        timestamp: DateTime<Utc>,
    },
    MarginUpdated {
        account_id: AccountId,
        margin_used: Decimal,
        margin_available: Decimal,
        unrealized_pnl: Decimal,
        timestamp: DateTime<Utc>,
    },
}
```

---

## 5. Market Data Module

### 5.1 QuoteTick

Best bid/ask (L1) data:

```rust
pub struct QuoteTick {
    pub ts_event: DateTime<Utc>,
    pub ts_recv: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub bid_price: Decimal,
    pub ask_price: Decimal,
    pub bid_size: Decimal,
    pub ask_size: Decimal,
    pub sequence: u64,
}
```

**Key Methods:**
```rust
fn spread(&self) -> Decimal
fn spread_bps(&self) -> Decimal           // Spread in basis points
fn mid_price(&self) -> Decimal
fn weighted_mid_price(&self) -> Decimal   // Weighted by size
fn is_valid(&self) -> bool
fn is_crossed(&self) -> bool
fn has_zero_liquidity(&self) -> bool
fn total_liquidity(&self) -> Decimal
fn imbalance(&self) -> Decimal            // (bid - ask) / total
fn from_trade_price(ts, symbol, price, spread) -> Self
```

### 5.2 QuoteStats

```rust
pub struct QuoteStats {
    pub symbol: String,
    pub count: u64,
    pub avg_spread: Decimal,
    pub min_spread: Decimal,
    pub max_spread: Decimal,
    pub avg_bid_size: Decimal,
    pub avg_ask_size: Decimal,
    pub twap: Decimal,        // Time-weighted average price
    pub vwap: Option<Decimal>,
}
```

### 5.3 BookLevel

```rust
pub struct BookLevel {
    pub price: Decimal,
    pub size: Decimal,
    pub order_count: u32,
}
```

### 5.4 Order Book Enums

```rust
pub enum BookSide {
    Bid,
    Ask,
}

pub enum BookAction {
    Add,
    Update,
    Delete,
    Clear,
}

pub enum BookDepth {
    L1,          // Top of book only
    L2(usize),   // N levels of depth
    Full,        // All levels
}
```

### 5.5 OrderBook

```rust
pub struct OrderBook {
    pub symbol: String,
    pub exchange: String,
    bids: BTreeMap<ReverseDecimal, BookLevel>,  // Sorted descending
    asks: BTreeMap<ReverseDecimal, BookLevel>,  // Sorted ascending
    pub ts_event: DateTime<Utc>,
    pub sequence: u64,
}
```

**Bid/Ask Management:**
```rust
fn update_bid(&mut self, price: Decimal, size: Decimal, order_count: u32)
fn update_ask(&mut self, price: Decimal, size: Decimal, order_count: u32)
fn remove_bid(&mut self, price: Decimal)
fn remove_ask(&mut self, price: Decimal)
fn clear_bids(&mut self)
fn clear_asks(&mut self)
fn clear(&mut self)
```

**L1 Access:**
```rust
fn best_bid(&self) -> Option<&BookLevel>
fn best_ask(&self) -> Option<&BookLevel>
fn spread(&self) -> Option<Decimal>
fn mid_price(&self) -> Option<Decimal>
fn weighted_mid_price(&self) -> Option<Decimal>
```

**L2 Access:**
```rust
fn bids(&self, depth: usize) -> Vec<&BookLevel>
fn asks(&self, depth: usize) -> Vec<&BookLevel>
fn all_bids(&self) -> Vec<&BookLevel>
fn all_asks(&self) -> Vec<&BookLevel>
fn bid_depth(&self) -> usize
fn ask_depth(&self) -> usize
```

**Analysis:**
```rust
fn is_empty(&self) -> bool
fn is_crossed(&self) -> bool
fn imbalance(&self) -> Option<Decimal>
fn cumulative_imbalance(&self, depth: usize) -> Option<Decimal>
fn total_bid_size(&self) -> Decimal
fn total_ask_size(&self) -> Decimal
fn total_bid_notional(&self) -> Decimal
fn total_ask_notional(&self) -> Decimal
```

**Order Simulation:**
```rust
fn simulate_market_buy(&self, quantity: Decimal)
    -> (avg_price, total_cost, filled_qty, remaining_qty)
fn simulate_market_sell(&self, quantity: Decimal)
    -> (avg_price, total_proceeds, filled_qty, remaining_qty)
```

**Conversion:**
```rust
fn to_quote_tick(&self) -> Option<QuoteTick>
```

### 5.6 OrderBookDelta

```rust
pub struct OrderBookDelta {
    pub symbol: String,
    pub exchange: String,
    pub ts_event: DateTime<Utc>,
    pub ts_recv: DateTime<Utc>,
    pub side: BookSide,
    pub action: BookAction,
    pub price: Decimal,
    pub size: Decimal,
    pub order_count: u32,
    pub sequence: u64,
    pub flags: u8,
}
```

**Factory Methods:**
```rust
OrderBookDelta::new(symbol, ts_event, side, action, price, size) -> Self
OrderBookDelta::update(symbol, ts_event, side, price, size) -> Self
OrderBookDelta::delete(symbol, ts_event, side, price) -> Self
```

**Application:**
```rust
fn apply(&self, book: &mut OrderBook)
```

### 5.7 OrderBookDeltas

Batch of deltas (snapshot or incremental):

```rust
pub struct OrderBookDeltas {
    pub symbol: String,
    pub exchange: String,
    pub is_snapshot: bool,
    pub ts_event: DateTime<Utc>,
    pub sequence: u64,
    pub deltas: Vec<OrderBookDelta>,
}
```

**Factory Methods:**
```rust
OrderBookDeltas::snapshot(symbol) -> Self
OrderBookDeltas::incremental(symbol) -> Self
```

**Application:**
```rust
fn add(&mut self, delta: OrderBookDelta)
fn apply(&self, book: &mut OrderBook)  // Clears book first if snapshot
```

---

## 6. Risk Module

### 6.1 Enums

#### TradingState
```rust
pub enum TradingState {
    Active,      // Normal trading
    Halted,      // No new orders
    ReduceOnly,  // Only position-reducing orders
    Liquidating, // System closing positions
    Paused,      // Orders queued, not submitted
}
```

**Permissions:**
| State | Place Orders | Increase Position | Decrease Position | Cancel Orders |
|-------|--------------|-------------------|-------------------|---------------|
| Active | ✓ | ✓ | ✓ | ✓ |
| Halted | ✗ | ✗ | ✗ | ✓ |
| ReduceOnly | ✓ | ✗ | ✓ | ✓ |
| Liquidating | ✗ | ✗ | ✓ | ✗ |
| Paused | ✗ | ✗ | ✗ | ✓ |

#### RiskLevel
```rust
pub enum RiskLevel {
    Normal,   // No concerns
    Elevated, // Monitor closely
    Warning,  // Consider reducing exposure
    Critical, // Immediate action required
    Breach,   // Limits exceeded
}
```

#### RiskCheckType
```rust
pub enum RiskCheckType {
    OrderQuantity,
    OrderNotional,
    PositionSize,
    PositionNotional,
    AccountEquity,
    MarginRequirement,
    Drawdown,
    DailyLoss,
    OrderRate,
    Concentration,
    TradingState,
    InstrumentValidation,
    PriceValidation,
    Custom,
}
```

### 6.2 RiskCheckResult

```rust
pub struct RiskCheckResult {
    pub check_type: RiskCheckType,
    pub passed: bool,
    pub risk_level: RiskLevel,
    pub message: String,
    pub current_value: Option<String>,
    pub limit_value: Option<String>,
}
```

**Factory Methods:**
```rust
RiskCheckResult::pass(check_type) -> Self
RiskCheckResult::pass_with_warning(check_type, message) -> Self
RiskCheckResult::fail(check_type, message) -> Self
```

**Builder Methods:**
```rust
fn with_current(self, value: impl ToString) -> Self
fn with_limit(self, value: impl ToString) -> Self
fn with_risk_level(self, level: RiskLevel) -> Self
```

### 6.3 FeeModel Trait

```rust
pub trait FeeModel: Send + Sync + fmt::Debug {
    fn calculate_fee(
        &self,
        fill_qty: Decimal,
        fill_price: Decimal,
        order: &Order,
        liquidity_side: LiquiditySide,
    ) -> Decimal;

    fn maker_rate(&self) -> Decimal;
    fn taker_rate(&self) -> Decimal;
    fn rate_for_side(&self, side: LiquiditySide) -> Decimal;
}
```

### 6.4 Fee Model Implementations

| Model | Description |
|-------|-------------|
| `PercentageFeeModel` | Percentage of notional (maker/taker rates) |
| `TieredFeeModel` | Volume-based tiers (VIP levels) |
| `FixedFeeModel` | Flat fee per fill |
| `HybridFeeModel` | Percentage + fixed components |
| `ZeroFeeModel` | No fees (testing) |

**PercentageFeeModel:**
```rust
pub struct PercentageFeeModel {
    pub maker_rate: Decimal,
    pub taker_rate: Decimal,
}

// Presets
PercentageFeeModel::binance_spot()     // 0.1% / 0.1%
PercentageFeeModel::binance_spot_bnb() // 0.075% / 0.075%
PercentageFeeModel::binance_futures()  // 0.02% / 0.04%
PercentageFeeModel::flat(rate)
PercentageFeeModel::zero()
```

**TieredFeeModel:**
```rust
pub struct TieredFeeModel {
    pub tiers: Vec<FeeTier>,
    current_tier: usize,
}

pub struct FeeTier {
    pub name: String,
    pub min_volume: Decimal,
    pub maker_rate: Decimal,
    pub taker_rate: Decimal,
}

// Set tier based on 30-day volume
model.set_tier_by_volume(volume_30d);
```

### 6.5 RiskLimits

```rust
pub struct RiskLimits {
    // Order Limits
    pub max_order_qty: Option<Decimal>,
    pub max_order_notional: Option<Decimal>,
    pub min_order_notional: Option<Decimal>,

    // Position Limits
    pub max_position_qty: Option<Decimal>,
    pub max_position_notional: Option<Decimal>,
    pub max_portfolio_notional: Option<Decimal>,
    pub max_concentration: Option<Decimal>,

    // Loss Limits
    pub max_daily_loss: Option<Decimal>,
    pub max_daily_loss_pct: Option<Decimal>,
    pub max_drawdown_pct: Option<Decimal>,

    // Rate Limits
    pub max_orders_per_second: Option<u32>,
    pub max_orders_per_minute: Option<u32>,
    pub max_open_orders: Option<u32>,
    pub max_open_orders_per_symbol: Option<u32>,

    // Price Limits
    pub max_price_deviation_pct: Option<Decimal>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
}
```

**Presets:**
```rust
RiskLimits::none()         // No restrictions
RiskLimits::conservative() // Safe defaults
```

### 6.6 PortfolioState

```rust
pub struct PortfolioState {
    pub equity: Decimal,
    pub peak_equity: Decimal,
    pub start_of_day_equity: Decimal,
    pub positions: HashMap<String, Decimal>,
    pub prices: HashMap<String, Decimal>,
    pub open_orders: u32,
    pub open_orders_by_symbol: HashMap<String, u32>,
    pub daily_pnl: Decimal,
}
```

**Key Methods:**
```rust
fn update_equity(&mut self, new_equity: Decimal)  // Tracks peak
fn reset_daily(&mut self)                          // New trading day
fn drawdown_pct(&self) -> Decimal
fn daily_loss_pct(&self) -> Decimal
fn position_notional(&self, symbol: &str) -> Decimal
fn total_notional(&self) -> Decimal
fn concentration(&self, symbol: &str) -> Decimal
```

### 6.7 RiskEngine

```rust
pub struct RiskEngine {
    pub limits: RiskLimits,
    pub trading_state: TradingState,
    pub portfolio: PortfolioState,
    rate_tracker: OrderRateTracker,
    symbol_limits: HashMap<String, RiskLimits>,
    pub strict_mode: bool,
}
```

**Factory Methods:**
```rust
RiskEngine::new(limits) -> Self
RiskEngine::permissive() -> Self       // No limits
RiskEngine::conservative() -> Self     // Safe defaults
```

**State Management:**
```rust
fn set_trading_state(&mut self, state: TradingState)
fn set_portfolio(&mut self, portfolio: PortfolioState)
fn update_equity(&mut self, equity: Decimal)
fn update_position(&mut self, symbol: &str, qty: Decimal)
fn update_price(&mut self, symbol: &str, price: Decimal)
fn set_symbol_limits(&mut self, symbol: &str, limits: RiskLimits)
```

**Validation:**
```rust
fn validate_order(&mut self, order: &Order, reference_price: Option<Decimal>)
    -> Vec<RiskCheckResult>
fn is_valid(&self, results: &[RiskCheckResult]) -> bool
fn first_failure(&self, results: &[RiskCheckResult]) -> Option<&RiskCheckResult>
fn record_order(&mut self)  // For rate limiting
```

**Checks Performed:**
1. Trading state allows order
2. Order quantity within limits
3. Order notional within limits
4. Position size within limits
5. Concentration within limits
6. Daily loss within limits
7. Drawdown within limits
8. Order rate within limits
9. Open orders within limits
10. Price deviation within limits

---

## 7. System Flow

### 7.1 Order Lifecycle

```
┌─────────────────────────────────────────────────────────────────────┐
│                        ORDER LIFECYCLE                               │
└─────────────────────────────────────────────────────────────────────┘

1. Strategy generates signal
   │
   ▼
2. StrategyContext.market_order() / limit_order()
   │
   ├──► OrderBuilder creates Order (status: Initialized)
   │    └── OrderInitialized event
   │
   ▼
3. RiskEngine.validate_order()
   │
   ├──► Checks: trading state, quantity, notional, position, etc.
   │
   ├── FAIL ──► Order denied (OrderRejected event)
   │
   └── PASS
       │
       ▼
4. OrderManager.submit()
   │
   ├──► Order status: Submitted
   │    └── OrderSubmitted event
   │
   ▼
5. ExecutionEngine / Venue
   │
   ├── REJECT ──► OrderRejected event (terminal)
   │
   └── ACCEPT ──► Order status: Accepted
                  └── OrderAccepted event
                      │
                      ▼
6. Fill Simulation / Venue Execution
   │
   ├──► FillModel.try_fill() evaluates market conditions
   │
   ├── NO FILL ──► Order remains open
   │
   └── FILL
       │
       ├──► Order.apply_fill(qty, price, commission)
       │
       ├── PARTIAL ──► Order status: PartiallyFilled
       │               └── OrderFilled event (leaves_qty > 0)
       │
       └── COMPLETE ──► Order status: Filled
                        └── OrderFilled event (leaves_qty = 0)
                            │
                            ▼
7. Position & Account Update
   │
   ├──► Account.fill() deducts from locked balance
   ├──► Position updated
   └──► P&L calculated
```

### 7.2 Backtest Data Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                     BACKTEST DATA FLOW                               │
└─────────────────────────────────────────────────────────────────────┘

Historical Data
     │
     ▼
┌────────────────┐
│ BarGenerator   │ ──► Converts ticks to BarData events
└────────────────┘
     │
     ├── OnEachTick mode: Fire for every tick
     ├── OnPriceMove mode: Fire when price changes
     └── OnCloseBar mode: Fire when bar closes
     │
     ▼
┌────────────────┐
│ BarsContext    │ ──► Maintains OHLCV series with lookback
└────────────────┘
     │
     │  bars.close[0] = current
     │  bars.close[1] = previous
     │  bars.sma(20) = indicator
     │
     ▼
┌────────────────┐
│ Strategy       │
│                │
│ fn on_bar_data │
│   (&mut self,  │
│    bar_data,   │ ──► Strategy receives bar + context
│    bars)       │
│   -> Signal    │
└────────────────┘
     │
     │  Signal::Buy / Sell / Hold
     │
     ▼
┌─────────────────┐
│ ExecutionEngine │
│                 │
│ execute_signal()│ ──► Converts Signal to Order
│                 │
│ on_bar()        │ ──► Processes pending orders
└─────────────────┘
     │
     │  Uses FillModel for realistic fills
     │
     ▼
┌────────────────┐
│ OrderManager   │ ──► Tracks all orders and events
└────────────────┘
     │
     ▼
┌────────────────┐
│ Portfolio      │ ──► Updates positions, cash, P&L
└────────────────┘
     │
     ▼
┌────────────────┐
│ Metrics        │ ──► Sharpe, drawdown, win rate, etc.
└────────────────┘
```

### 7.3 Risk Validation Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    RISK VALIDATION FLOW                              │
└─────────────────────────────────────────────────────────────────────┘

Order Submission
     │
     ▼
┌────────────────────────────────────────────────────────────┐
│                    RiskEngine.validate_order()              │
├────────────────────────────────────────────────────────────┤
│                                                             │
│  1. check_trading_state()                                   │
│     ├── Can place orders?                                   │
│     └── Can increase position? (reduce-only check)          │
│                                                             │
│  2. check_order_quantity()                                  │
│     └── qty <= max_order_qty?                               │
│                                                             │
│  3. check_order_notional()                                  │
│     └── min <= notional <= max?                             │
│                                                             │
│  4. check_position_limits()                                 │
│     ├── new_position <= max_position_qty?                   │
│     └── new_notional <= max_position_notional?              │
│                                                             │
│  5. check_concentration()                                   │
│     └── symbol_exposure / total_exposure <= max?            │
│                                                             │
│  6. check_daily_loss()                                      │
│     └── today_loss <= max_daily_loss?                       │
│                                                             │
│  7. check_drawdown()                                        │
│     └── (peak - current) / peak <= max_drawdown?            │
│                                                             │
│  8. check_order_rate()                                      │
│     ├── orders_per_second <= max?                           │
│     └── orders_per_minute <= max?                           │
│                                                             │
│  9. check_open_orders()                                     │
│     └── open_orders < max_open_orders?                      │
│                                                             │
│ 10. check_price_deviation()                                 │
│     └── |order_price - market_price| / market_price <= max? │
│                                                             │
└────────────────────────────────────────────────────────────┘
     │
     ▼
┌────────────────┐     ┌────────────────┐
│ All Passed?    │──NO─▶│ Return Failures│──▶ Order Rejected
└────────────────┘      └────────────────┘
     │
    YES
     │
     ▼
Order Proceeds to Execution
```

### 7.4 Order Book Update Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                  ORDER BOOK UPDATE FLOW                              │
└─────────────────────────────────────────────────────────────────────┘

WebSocket Feed
     │
     ▼
┌────────────────┐
│ Parse Message  │
└────────────────┘
     │
     ├── Snapshot ──► OrderBookDeltas::snapshot()
     │                     │
     │                     ├── Add all bid deltas
     │                     └── Add all ask deltas
     │
     └── Update ──► OrderBookDelta::update()
     │
     ▼
┌────────────────┐
│ Apply to Book  │
│                │
│ deltas.apply() │──► Clears book if snapshot
│                │    Then applies each delta
└────────────────┘
     │
     ▼
┌────────────────┐
│ Updated Book   │
│                │
│ best_bid()     │──► Access L1
│ bids(5)        │──► Access L2
│ spread()       │──► Analytics
│ imbalance()    │
└────────────────┘
     │
     ▼
┌────────────────┐
│ Strategies     │──► Use book for decisions
└────────────────┘
```

---

## 8. Usage Examples

### 8.1 Creating and Managing Orders

```rust
use trading_common::orders::*;
use rust_decimal_macros::dec;

// Create a limit order
let order = OrderBuilder::new(OrderType::Limit, "BTCUSDT", OrderSide::Buy, dec!(0.1))
    .with_venue("BINANCE")
    .with_price(dec!(50000))
    .with_time_in_force(TimeInForce::GTC)
    .with_strategy_id("mean_reversion")
    .build()?;

// Submit to order manager
let manager = OrderManager::new();
manager.submit(order.clone()).await?;

// Simulate a fill
let fill_event = OrderFilled {
    client_order_id: order.client_order_id.clone(),
    last_qty: dec!(0.05),
    last_px: dec!(49950),
    cum_qty: dec!(0.05),
    leaves_qty: dec!(0.05),
    // ... other fields
};

manager.apply_event(OrderEventAny::Filled(fill_event)).await?;

// Query order state
let updated = manager.get(&order.client_order_id).await.unwrap();
assert_eq!(updated.status, OrderStatus::PartiallyFilled);
```

### 8.2 Using the Risk Engine

```rust
use trading_common::risk::*;
use rust_decimal_macros::dec;

// Create risk engine with limits
let limits = RiskLimits::none()
    .with_max_order_qty(dec!(10))
    .with_max_position_qty(dec!(50))
    .with_max_daily_loss(dec!(5000))
    .with_max_drawdown_pct(dec!(15));

let mut engine = RiskEngine::new(limits);

// Update portfolio state
engine.update_equity(dec!(100000));
engine.update_position("BTCUSDT", dec!(5));
engine.update_price("BTCUSDT", dec!(50000));

// Validate an order
let order = create_order("BTCUSDT", OrderSide::Buy, dec!(8));
let results = engine.validate_order(&order, Some(dec!(50000)));

if engine.is_valid(&results) {
    // Order passes all risk checks
    engine.record_order();
    // Proceed with submission
} else {
    // Order rejected
    let failure = engine.first_failure(&results).unwrap();
    println!("Order rejected: {}", failure);
}
```

### 8.3 Working with Order Books

```rust
use trading_common::data::*;
use rust_decimal_macros::dec;

// Create and populate an order book
let mut book = OrderBook::new("BTCUSDT");

book.update_bid(dec!(50000), dec!(1.5), 5);
book.update_bid(dec!(49999), dec!(2.0), 3);
book.update_ask(dec!(50001), dec!(1.0), 4);
book.update_ask(dec!(50002), dec!(3.0), 7);

// Access L1 data
let spread = book.spread().unwrap();  // 1.0
let mid = book.mid_price().unwrap();  // 50000.5

// Access L2 data
let top_5_bids = book.bids(5);
let top_5_asks = book.asks(5);

// Simulate market impact
let (avg_price, cost, filled, remaining) = book.simulate_market_buy(dec!(2));

// Convert to quote tick
let quote = book.to_quote_tick().unwrap();

// Apply incremental update
let delta = OrderBookDelta::update("BTCUSDT", Utc::now(), BookSide::Bid, dec!(50000), dec!(2.0));
delta.apply(&mut book);
```

### 8.4 Account and Margin Management

```rust
use trading_common::accounts::*;
use rust_decimal_macros::dec;

// Create a margin account
let mut account = Account::margin("main-account", "USDT", dec!(20));

// Deposit funds
account.deposit("USDT", dec!(10000));

// Check buying power
let buying_power = account.buying_power();  // 200,000 with 20x leverage

// Monitor margin
if let Some(margin) = &account.margin {
    println!("Equity: {}", margin.equity());
    println!("Available: {}", margin.available_for_trading());

    if margin.is_margin_call() {
        account.suspend();
    }
}

// Multi-currency tracking
account.deposit("BTC", dec!(0.5));
let prices = HashMap::from([
    ("BTC".to_string(), dec!(50000)),
]);
let total_value = account.total_value(&prices);
```

### 8.5 Fee Calculation

```rust
use trading_common::risk::*;
use rust_decimal_macros::dec;

// Percentage fee model
let fee_model = PercentageFeeModel::binance_futures();
let fee = fee_model.calculate_fee(
    dec!(1.0),           // quantity
    dec!(50000),         // price
    &order,
    LiquiditySide::Taker,
);
// Fee: 50000 * 0.0004 = 20 USDT

// Tiered fee model
let mut tiered = TieredFeeModel::binance_spot_tiers();
tiered.set_tier_by_volume(dec!(10_000_000));  // VIP2
let rate = tiered.maker_rate();  // 0.08%
```

---

## 9. Integration Points

### 9.1 Strategy Integration

The `Strategy` trait has been enhanced with order management capabilities:

```rust
pub trait Strategy: Send + Sync {
    // Core method (existing)
    fn on_bar_data(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Signal;

    // Order event handlers (new)
    fn on_order_filled(&mut self, event: &OrderFilled) { }
    fn on_order_rejected(&mut self, event: &OrderRejected) { }
    fn on_order_canceled(&mut self, event: &OrderCanceled) { }
    fn on_order_submitted(&mut self, order: &Order) { }

    // Advanced order management (new)
    fn uses_order_management(&self) -> bool { false }
    fn get_orders(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<Order>;
    fn get_cancellations(&mut self, bar_data: &BarData, bars: &mut BarsContext) -> Vec<ClientOrderId>;
}
```

### 9.2 Backward Compatibility

The system maintains full backward compatibility:

1. **Signal-based strategies** continue to work unchanged
2. `ExecutionEngine::execute_signal()` converts signals to orders
3. Simple strategies return `Signal::Buy/Sell/Hold`
4. Advanced strategies can use direct order submission

### 9.3 Future Integration Points

The architecture supports future additions:

- **Live Trading**: OrderManager can route to exchange adapters
- **Multi-venue**: InstrumentId includes venue for routing
- **Portfolio Management**: Account/Position tracking ready for aggregation
- **Risk Monitoring**: RiskEngine can be run continuously
- **Event Sourcing**: All events logged for replay/audit

---

## 10. Test Coverage

| Module | Tests | Coverage Areas |
|--------|-------|----------------|
| orders/types | 10 | Enums, IDs, state transitions |
| orders/order | 12 | Creation, builder, fills, validation |
| orders/events | 4 | Event creation, status inference |
| orders/manager | 9 | CRUD, queries, concurrency |
| execution/context | 7 | Order submission, position tracking |
| execution/fill_model | 7 | Fill simulation models |
| execution/engine | 7 | Order processing, signal conversion |
| instruments/types | 5 | Asset classes, currencies |
| instruments/instrument | 7 | Specs, validation, implementations |
| accounts/types | 4 | Account states, modes |
| accounts/account | 8 | Balance, margin, state management |
| data/quotes | 6 | Quote creation, analytics |
| data/orderbook | 10 | Book management, simulation |
| risk/types | 4 | Trading states, risk levels |
| risk/fee_model | 7 | Fee calculations |
| risk/engine | 10 | Validation, limits |

**Total: 125+ new tests**
