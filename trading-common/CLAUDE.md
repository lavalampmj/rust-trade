# trading-common

Shared library containing core domain models, backtesting engine, data repository, caching, and venue infrastructure.

## Module Overview

```
trading-common/src/
├── accounts/       # Account configuration and management
├── backtest/       # Backtesting engine and strategies
├── config/         # Configuration types
├── data/           # Cache, repository, DBN types
├── error/          # Error types
├── execution/      # Order execution (re-exports from venue/)
├── instruments/    # Instrument definitions
├── logging/        # Logging utilities
├── orders/         # Order types and IDs
├── risk/           # Risk management
├── series/         # Time series utilities
├── state/          # State management
├── transforms/     # Data transformations
├── validation/     # Validation utilities
└── venue/          # Unified venue infrastructure
```

## Caching System (data/cache.rs)

**Pattern**: Tiered L1 (memory) + L2 (Redis) cache

- **InMemoryTickCache**: `HashMap<String, VecDeque<TickData>>`, fixed size per symbol, FIFO eviction
- **RedisTickCache**: Persistent cache with TTL, survives restarts
- **TieredCache**: Orchestrator that writes to both in parallel, reads L1 → L2 → empty

**Read strategy**:
1. Check L1 (memory) first
2. On L1 miss, check L2 (Redis)
3. On L2 hit, backfill L1 for future locality
4. On both miss, return empty

Cache failures don't block main flow - graceful degradation is built-in.

## Repository Pattern (data/repository.rs)

Handles all database operations with cache integration:

- **Insert**: `insert_tick()` (single), `batch_insert()` (bulk with 1000-row chunking)
- **Query**: `get_ticks()` (cache-aware), `get_latest_price()`, `get_historical_data_for_backtest()`
- **OHLC**: `generate_ohlc_from_ticks()` supports 8 timeframes (1m to 1w)

**Critical distinction**:
- Backtest queries: ASC order (chronological replay)
- Live queries: DESC order (most recent first)

Cache-aware logic: Queries with `start_time` within last hour check cache first.

## Backtesting Engine (backtest/)

**Pattern**: Strategy pattern + Portfolio simulation

**Components**:
- `BacktestEngine`: Orchestrates tick processing, signal generation, trade execution
- `Strategy` trait: Implement `on_bar_data()` to generate signals (BUY/SELL/HOLD)
- `Portfolio`: Tracks cash, positions, trades, calculates P&L
- `BacktestMetrics`: Sharpe ratio, max drawdown, win rate, profit factor

**Built-in strategies**: SMA (crossover), RSI (overbought/oversold)

### Strategy Interface

```rust
pub trait Strategy {
    fn on_bar_data(&mut self, bar_data: &BarData) -> Signal;
    fn bar_data_mode(&self) -> BarDataMode { BarDataMode::OnEachTick }
    fn preferred_bar_type(&self) -> BarType { BarType::TimeBased(Timeframe::OneMinute) }
}
```

**BarData Structure**:
- `current_tick: Option<TickData>` - Tick that triggered event (None for bar close)
- `ohlc_bar: OHLCData` - Current OHLC state
- `metadata: BarMetadata` - Bar state info (is_first_tick, is_bar_closed, tick_count)

**BarDataMode**:
- `OnEachTick`: Event for every tick with accumulated OHLC
- `OnPriceMove`: Event only when price changes
- `OnCloseBar`: Event only when bar completes

**BarType**:
- `TimeBased(Timeframe)`: 1m, 5m, 15m, 1h, 4h, 1d, 1w (timer-based closing)
- `TickBased(u32)`: N-tick bars (closed after N ticks)

### Multi-Account Support

```rust
let config = AccountsConfig::with_default("MAIN", "USDT", dec!(100000))
    .add_simulation_account("AGGRESSIVE", "USDT", dec!(50000), vec!["momentum".into()])
    .add_simulation_account("CONSERVATIVE", "USDT", dec!(200000), vec!["rsi".into()]);

let exchange = SimulatedExchange::new().with_accounts_config(&config);
```

**Account methods**:
- `account(&AccountId)`: Get specific account
- `default_account()`: Get default account
- `all_accounts()`: Iterate all accounts
- `total_locked_for_account(&AccountId)`: Locked funds per account

## Unified Venue Infrastructure (venue/)

**Pattern**: Shared base traits for data ingestion and order execution

### Module Structure

```
venue/
├── mod.rs              # Public exports
├── connection.rs       # VenueConnection trait, ConnectionStatus
├── symbology.rs        # SymbolNormalizer trait, NativeDbVenue marker
├── error.rs            # VenueError with error classification
├── config.rs           # VenueConfig, RestConfig, StreamConfig, AuthConfig
├── types.rs            # VenueInfo, VenueCapabilities
├── http/               # HTTP client with rate limiting
└── websocket/          # WebSocket client with reconnection
```

### VenueConnection Trait

```rust
#[async_trait]
pub trait VenueConnection: Send + Sync {
    fn info(&self) -> &VenueInfo;
    async fn connect(&mut self) -> VenueResult<()>;
    async fn disconnect(&mut self) -> VenueResult<()>;
    fn is_connected(&self) -> bool;
    fn connection_status(&self) -> ConnectionStatus;
}
```

**ConnectionStatus**: `Disconnected`, `Connecting`, `Connected`, `Reconnecting`, `Error`

### SymbolNormalizer Trait

DBT canonical symbol conversion (BTCUSDT ↔ BTCUSD):

```rust
pub trait SymbolNormalizer: Send + Sync {
    fn to_canonical(&self, venue_symbol: &str) -> VenueResult<String>;
    fn to_venue(&self, canonical_symbol: &str) -> VenueResult<String>;
    fn venue_id(&self) -> &str;
    fn register_symbols(&self, symbols: &[String], registry: &InstrumentRegistry)
        -> VenueResult<HashMap<String, u32>>;
    fn is_native_dbt(&self) -> bool { false }
}

// Marker for DBT-native venues (auto-implements SymbolNormalizer)
pub trait NativeDbVenue: Send + Sync {
    fn venue_id(&self) -> &str;
}
```

### Error Classification

```rust
pub enum VenueError {
    Connection(String),      // Transient - auto-retry
    Authentication(String),  // Permanent - do not retry
    RateLimit(String),       // ResourceExhausted - wait and retry
    OrderRejected(String),   // Permanent
    InsufficientBalance(String), // ResourceExhausted
    // ...
}
```

Methods: `is_transient()`, `is_retryable()`, `suggested_retry_delay()`

### HTTP Infrastructure (venue/http/)

- `HttpClient`: Authenticated client with rate limiting
- `RequestSigner`: Trait for venue-specific HMAC signing
- `RateLimiter`: Multi-bucket rate limiter (governor crate)

### WebSocket Infrastructure (venue/websocket/)

- `WebSocketClient`: Reconnecting client with exponential backoff
- Ping/pong keepalive
- Graceful shutdown via broadcast channel

```rust
let client = WebSocketClientBuilder::new("wss://stream.example.com/ws")
    .max_reconnect_attempts(10)
    .reconnect_initial_delay(Duration::from_secs(1))
    .ping_interval(Duration::from_secs(30))
    .build();
```

### Backward Compatibility

`execution/venue/` re-exports from `venue/` for existing code:
```rust
// Old path (still works)
use trading_common::execution::venue::{VenueError, VenueConfig};
// New canonical path (preferred)
use trading_common::venue::{VenueError, VenueConfig};
```

## DBN Data Types (data/dbn_types.rs)

Uses Databento Binary types for market data:
- `TradeMsg` - Trade/tick data (48 bytes fixed-size)
- `BboMsg` - Best bid/offer quotes (L1)
- `Mbp10Msg` - Market-by-price 10 levels (L2)
- `MboMsg` - Market-by-order (L3)

**Benefits**: Fixed-point math (i64 with 1e-9 scale), compact memory layout, native Databento compatibility.

**Extension traits**: `TradeMsgExt`, `create_trade_msg_from_decimals()`

## Adding a New Strategy

1. Create `src/backtest/strategy/your_strategy.rs`
2. Implement `Strategy` trait
3. Register in `strategy/mod.rs`

```rust
impl Strategy for YourStrategy {
    fn name(&self) -> &str { "Your Strategy" }

    fn on_bar_data(&mut self, bar_data: &BarData) -> Signal {
        // bar_data.ohlc_bar, bar_data.current_tick, bar_data.metadata
        Signal::Hold
    }

    fn bar_data_mode(&self) -> BarDataMode { BarDataMode::OnCloseBar }
    fn preferred_bar_type(&self) -> BarType { BarType::TimeBased(Timeframe::OneMinute) }
}
```
