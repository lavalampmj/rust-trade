# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

### Backend (Rust)

```bash
# Build entire workspace (trading-common + trading-core + data-manager + src-tauri)
cargo build

# Build with optimizations
cargo build --release

# Run tests
cargo test

# Run benchmarks
cd trading-core && cargo bench

# Format code
cargo fmt

# Run linting
cargo clippy
```

### Data Manager CLI

```bash
cd data-manager

# Start data manager service with live Binance streaming and IPC transport
# Note: Use DBT canonical symbology (BTCUSD) - converted to venue format automatically
cargo run serve --live --provider binance --symbols BTCUSD,ETHUSD --ipc

# Start with Kraken Spot streaming (DBT format converted to BTC/USD internally)
cargo run serve --live --provider kraken --symbols BTCUSD,ETHUSD,SOLUSD --ipc

# Start with Kraken Futures streaming (demo/testnet)
cargo run serve --live --provider kraken_futures --demo --symbols BTCUSD,ETHUSD --ipc

# Start with persistence to database
cargo run serve --live --ipc --persist

# Selective IPC streaming: subscribe to many symbols for database,
# but only stream a subset to IPC for trading (saves memory)
cargo run serve --live --provider kraken \
  --symbols BTCUSD,ETHUSD,SOLUSD,XRPUSD,ADAUSD,DOGEUSD,AVAXUSD,DOTUSD,LINKUSD \
  --ipc --ipc-symbols BTCUSD,ETHUSD,SOLUSD \
  --persist
# Result: All 9 symbols persisted to DB, only 3 IPC channels created (~9MB vs ~28MB)

# Start with custom config file
cargo run serve -c ../config/development.toml --live --ipc

# Fetch historical data with automatic routing (futures/equities via Databento)
# Provider and dataset are automatically resolved from asset type and exchange
cargo run fetch --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31

# Explicit asset type for better routing
cargo run fetch --symbols AAPL --exchange NASDAQ --asset-type equity --start 2024-01-01 --end 2024-01-31

# Dry run to see routing resolution without fetching
cargo run fetch --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31 --dry-run --verbose

# Override provider/dataset when needed
cargo run fetch --symbols ESH5 --exchange CME --provider databento --dataset GLBX.MDP3 --start 2024-01-01 --end 2024-01-31

# Symbol management
cargo run symbol list                         # List registered symbols
cargo run symbol add --symbols BTCUSD,ETHUSD  # Add symbols (DBT canonical format)
cargo run symbol remove --symbols DOGEUSD     # Remove symbols
cargo run symbol discover --provider kraken   # Discover crypto symbols
cargo run symbol discover --provider databento # Discover futures/equities

# Database operations
cargo run db migrate                            # Run database migrations
cargo run db stats                              # Show database statistics
cargo run db compress                           # Compress old data

# Backfill with cost tracking (auto-routes to Databento for futures/equities)
cargo run backfill estimate --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31

# Fetch with asset type for better routing
cargo run backfill fetch --symbols ESH5 --exchange CME --asset-type futures --start 2024-01-01 --end 2024-01-31

# Verbose mode shows routing resolution details
cargo run backfill estimate --symbols AAPL --exchange NASDAQ --asset-type equity --start 2024-01-01 --end 2024-01-31 --verbose

# Fill gaps and check status
cargo run backfill fill-gaps --symbols ESH5 --exchange CME   # Detect and fill data gaps
cargo run backfill status --detailed          # Show status with routing config
cargo run backfill cost-report                # Show cost report

# Show help
cargo run -- --help
```

### Trading Core CLI

```bash
cd trading-core

# Start live data collection (receives data via IPC from data-manager)
# Note: data-manager must be running first!
cargo run live

# Start live data collection with paper trading enabled
cargo run live --paper-trading

# Start interactive backtesting interface
cargo run backtest

# Calculate SHA256 hash of strategy file
cargo run hash-strategy <file_path>

# Show help
cargo run -- --help
```

### Frontend (Next.js)

```bash
cd frontend

# Install dependencies
npm install

# Development mode (web only)
npm run dev

# Build for production (web)
npm run build
npm start

# Lint frontend code
npm run lint
```

### Desktop Application (Tauri)

```bash
cd frontend

# Development mode with hot reload
npm run tauri dev

# Production build (creates installer/bundle)
npm run tauri build
```

## Architecture Overview

### Workspace Structure

The project uses a Rust workspace with four crates:

- **trading-common** (shared library): Core domain models, backtesting engine, data repository, and caching
- **trading-core** (CLI application): Live data collection from exchanges, paper trading, CLI interface
- **data-manager** (data infrastructure): Centralized market data loading, streaming, and distribution via IPC
- **src-tauri** (desktop app backend): Tauri commands exposing trading-common functionality to Next.js frontend

This separation enables code reuse: trading-core, data-manager, and src-tauri all depend on trading-common, avoiding duplication.

**Data flow architecture**:
- **IPC mode only**: data-manager streams from exchanges (Binance, Kraken) → shared memory IPC → trading-core consumes
- trading-core does not persist tick data to database (handled by data-manager with TimescaleDB)
- trading-core only updates cache and processes paper trading signals

### DBT-Native Architecture

The framework uses **Databento (DBT) data structures and symbology** as the canonical internal format throughout the system. Conversions to/from venue-specific formats happen only at the edges.

#### Canonical Symbology

**DBT format**: `BTCUSD`, `ETHUSD`, `SOLUSD` (base + quote, no separators)

| Component | Format Used | Example |
|-----------|-------------|---------|
| CLI arguments | DBT canonical | `--symbols BTCUSD,ETHUSD` |
| Config files | DBT canonical | `symbols = ["BTCUSD", "ETHUSD"]` |
| Internal data structures | DBT canonical | `tick.symbol = "BTCUSD"` |
| Database storage | DBT canonical | `symbol VARCHAR = 'BTCUSD'` |
| Cache keys | DBT canonical | `ticks:BTCUSD` |
| Strategy instances | DBT canonical | `StrategyInstanceConfig::new("rsi", "BTCUSD")` |

#### Edge Conversions

Venue-specific symbol formats are handled **only** at data ingestion and order submission:

```
CLI Input (DBT)     →  Provider Adapter  →  Venue WebSocket
   BTCUSD           →  to_kraken_spot()  →  BTC/USD
   BTCUSD           →  to_binance()      →  BTCUSDT
   BTCUSD           →  to_kraken_futures →  PI_XBTUSD

Venue Response      →  Normalizer        →  Internal (DBT)
   BTC/USD          →  from_kraken()     →  BTCUSD
   BTCUSDT          →  from_binance()    →  BTCUSD
```

**Key files for symbol conversion**:
- `data-manager/src/provider/kraken/symbol.rs` - Kraken spot/futures conversion
- `data-manager/src/provider/binance/symbol.rs` - Binance conversion
- `data-manager/src/provider/databento/` - Native DBT (no conversion needed)
- `data-manager/src/provider/traits.rs` - `SymbolNormalizer` trait for data providers
- `trading-common/src/venue/symbology.rs` - Unified `SymbolNormalizer` for execution venues

#### Instrument ID Registry

The `InstrumentRegistry` provides persistent, deterministic `instrument_id` assignment for all symbols:

```
┌─────────────────────────────────────────────────────────────────┐
│                     InstrumentRegistry                          │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────────┐ │
│  │  L1 Cache   │───>│  L2 Cache   │───>│  ID Generation      │ │
│  │  (DashMap)  │    │  (Postgres) │    │  (deterministic)    │ │
│  └─────────────┘    └─────────────┘    └─────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

**ID generation**: For non-Databento sources, IDs are generated via `hash(symbol + exchange) >> 32` for deterministic, collision-resistant u32 IDs.

**Usage pattern**:
```rust
// During subscription, pre-register symbols
let registry = InstrumentRegistry::new(pool).await?;
let id = registry.get_or_create("BTCUSD", "BINANCE").await?;
// Returns same ID on subsequent calls (cached + persisted)
```

**Key files**:
- `data-manager/src/instruments/registry.rs` - Registry implementation
- `data-manager/src/provider/*/normalizer.rs` - Registry integration in normalizers

#### DBN Data Types

For market data, the framework uses DBN (Databento Binary) types internally:
- `TradeMsg` - Trade/tick data (48 bytes fixed-size)
- `BboMsg` - Best bid/offer quotes (L1)
- `Mbp10Msg` - Market-by-price 10 levels (L2)
- `MboMsg` - Market-by-order (L3)

**Benefits**:
- Fixed-point math (i64 with 1e-9 scale) faster than Decimal for comparisons
- Compact memory layout for high-frequency processing
- Native Databento compatibility for futures/equities data

**Extension traits** in `trading-common/src/data/dbn_types.rs`:
- `TradeMsgExt` - Helper methods for TradeMsg
- `create_trade_msg_from_decimals()` - Construct from Decimal prices

#### Data Providers by Asset Class

Different providers support different asset classes:

| Provider | Asset Classes | Data Types | Use Case |
|----------|---------------|------------|----------|
| **Kraken** | Crypto | Live streaming | `--provider kraken` for BTC, ETH, SOL, etc. |
| **Kraken Futures** | Crypto Perpetuals | Live streaming | `--provider kraken_futures` for PI_XBTUSD |
| **Binance** | Crypto | Live streaming | `--provider binance` for crypto pairs |
| **Databento** | Equities, Futures | Historical + Live | Backfill ES, CL, AAPL, etc. |

**Symbol examples by provider**:
```bash
# Crypto (live streaming)
--provider kraken --symbols BTCUSD,ETHUSD,SOLUSD
--provider binance --symbols BTCUSD,ETHUSD

# Futures/Equities (historical backfill via Databento)
--symbols ESH5 --dataset GLBX.MDP3    # E-mini S&P 500 March 2025
--symbols CLH5 --dataset GLBX.MDP3    # Crude Oil March 2025
--symbols AAPL --dataset XNAS.ITCH    # Apple on NASDAQ
```

**Note**: Crypto historical data is not yet supported via Databento. Use live streaming from Kraken/Binance and persist with `--persist` flag.

#### Provider Routing System

The framework includes an automatic **provider routing system** that selects the appropriate data provider based on asset type and exchange. This eliminates the need to manually specify `--provider` for most operations.

**Three-tier resolution strategy**:
1. **Symbol-specific routing** (highest priority): Exact match on symbol+exchange
2. **Asset type + exchange fallback**: Match on asset class (futures, equity, crypto)
3. **Global default**: Falls back to `databento`

**CLI usage with automatic routing**:
```bash
# Futures - automatically routes to databento
cargo run fetch --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31
# → Resolves: provider=databento, dataset=GLBX.MDP3

# Equities - automatically routes to databento
cargo run fetch --symbols AAPL --exchange NASDAQ --start 2024-01-01 --end 2024-01-31
# → Resolves: provider=databento, dataset=XNAS.ITCH

# Crypto - infers asset type, routes appropriately
cargo run fetch --symbols BTCUSD --exchange KRAKEN --asset-type crypto --dry-run
# → Resolves: provider=databento for historical (realtime would use kraken)

# Explicit override when needed
cargo run fetch --symbols ESH5 --exchange CME --provider databento --dataset GLBX.MDP3
```

**Configuration** (in `config/development.toml`):
```toml
[routing]
default_provider = "databento"

[routing.databento.datasets]
"futures@CME" = "GLBX.MDP3"
"equity@NASDAQ" = "XNAS.ITCH"

[routing.asset_types.futures]
historical = "databento"
realtime = "databento"

[routing.asset_types.crypto]
historical = "databento"
realtime = "kraken"
```

**Key files**:
- `data-manager/src/config/routing.rs` - Routing configuration types
- `data-manager/src/config/router.rs` - `ProviderRouter` implementation
- `data-manager/src/provider/factory.rs` - `ProviderFactory` for creating providers

### Key Architectural Patterns

#### 1. Exchange Layer (trading-core/src/exchange/ and src/data_source/)

**Pattern**: Trait-based abstraction for data sources

- `Exchange` trait defines standard interface for all data sources
- `IpcExchange` receives data from data-manager via shared memory IPC
- Uses callback pattern: `subscribe_trades()` accepts closure for real-time tick processing
- Graceful shutdown via broadcast channel (`shutdown_rx`)

**Data flow**: data-manager (Binance/Kraken WebSocket) → IPC shared memory → trading-core (IpcExchange)

**Important**: Exchange callbacks execute synchronously - avoid blocking operations in handlers.

#### 2. Service Layer (trading-core/src/service/market_data.rs)

**Pattern**: Event-driven pipeline for cache updates and paper trading

Data flow:
```
IpcExchange (shared memory callback)
  → Collection Task (mpsc channel)
  → Processing Task
    → Tick Validation
    → Cache Update (L1 + L2 in parallel)
    → Paper Trading Processor (optional)
```

**Key features**:
- Dual-task architecture decouples collection from processing
- Cache-only updates (no database writes - handled by data-manager)
- Real-time tick validation
- Graceful shutdown via broadcast channel

**Note**: Tick data persistence is handled by data-manager using TimescaleDB. trading-core only maintains cache for paper trading strategy context.

#### 3. Caching System (trading-common/src/data/cache.rs)

**Pattern**: Tiered L1 (memory) + L2 (Redis) cache

- **InMemoryTickCache**: `HashMap<String, VecDeque<TickData>>`, fixed size per symbol, FIFO eviction
- **RedisTickCache**: Persistent cache with TTL, survives restarts
- **TieredCache**: Orchestrator that writes to both in parallel, reads L1 → L2 → empty

**Read strategy**:
1. Check L1 (memory) first
2. On L1 miss, check L2 (Redis)
3. On L2 hit, backfill L1 for future locality
4. On both miss, return empty

**Important**: Cache failures don't block main flow - graceful degradation is built-in.

#### 4. Repository Pattern (trading-common/src/data/repository.rs)

Handles all database operations with cache integration:

- **Insert**: `insert_tick()` (single), `batch_insert()` (bulk with 1000-row chunking)
- **Query**: `get_ticks()` (cache-aware), `get_latest_price()`, `get_historical_data_for_backtest()`
- **OHLC**: `generate_ohlc_from_ticks()` supports 8 timeframes (1m to 1w)

**Critical distinction**:
- Backtest queries: ASC order (chronological replay)
- Live queries: DESC order (most recent first)

**Cache-aware logic**: Queries with `start_time` within last hour check cache first before database.

#### 5. Backtesting Engine (trading-common/src/backtest/)

**Pattern**: Strategy pattern + Portfolio simulation

Components:
- `BacktestEngine`: Orchestrates tick processing, signal generation, trade execution
- `Strategy` trait: Implement `on_bar_data()` to generate signals (BUY/SELL/HOLD) using unified bar interface
- `Portfolio`: Tracks cash, positions, trades, calculates P&L
- `BacktestMetrics`: Sharpe ratio, max drawdown, win rate, profit factor

**Built-in strategies**:
- SMA (Simple Moving Average): Crossover-based (short/long MA)
- RSI (Relative Strength Index): Overbought/oversold levels

**Adding new strategy**: Implement `Strategy` trait in `trading-common/src/backtest/strategy/`, register in `strategy/mod.rs`.

**Strategy Bar Data Processing** (Unified Interface):

All strategies now use a unified `on_bar_data()` interface that provides consistent behavior between backtesting and live trading:

```rust
pub trait Strategy {
    fn on_bar_data(&mut self, bar_data: &BarData) -> Signal;

    fn bar_data_mode(&self) -> BarDataMode {
        BarDataMode::OnEachTick  // Default
    }

    fn preferred_bar_type(&self) -> BarType {
        BarType::TimeBased(Timeframe::OneMinute)  // Default
    }
}
```

**BarData Structure**:
- `current_tick: Option<TickData>` - The tick that triggered this event (None for bar close events)
- `ohlc_bar: OHLCData` - Current OHLC state of the bar
- `metadata: BarMetadata` - Bar metadata (is_first_tick, is_bar_closed, tick_count, etc.)

**Three Operational Modes** (`BarDataMode`):

1. **OnEachTick**: Strategy receives event for every tick with accumulated OHLC state
   - Use for tick-by-tick strategies that need to react to each price update
   - Both `current_tick` and `ohlc_bar` are available
   - Example: Scalping strategies, market-making

2. **OnPriceMove**: Strategy receives event only when price changes
   - Filters out duplicate prices to reduce noise
   - More efficient than OnEachTick when price updates are frequent
   - Example: Price action strategies

3. **OnCloseBar**: Strategy receives event only when bar completes (e.g., every 1 minute)
   - `current_tick` is None, only `ohlc_bar` is available
   - Most efficient for time-based strategies
   - Example: SMA, RSI, MACD strategies that operate on completed candles

**Bar Types**:
- `TimeBased(Timeframe)`: Time-based bars (1m, 5m, 15m, 1h, 4h, 1d, 1w)
  - Closed by wall-clock timer (every 1 second check)
  - Synthetic bars generated when no ticks during interval (OHLC = last price)
- `TickBased(u32)`: N-tick bars (e.g., 100-tick, 500-tick)
  - Closed after N ticks received
  - More consistent bar sizes in high-frequency scenarios

**Real-time vs Historical Parity**:
- `RealtimeOHLCGenerator` (live trading): Accumulates ticks, timer-based closing (1s checks), synthetic bars for gaps
- `HistoricalOHLCGenerator` (backtesting): Processes tick history, gap detection, synthetic bars for missing periods
- Both generators produce identical `BarData` events given same input ticks
- Synthetic bars ensure continuity: if no ticks during a period, bar generated with O=H=L=C=last_price

**Multi-Account Support** (SimulatedExchange):

The `SimulatedExchange` supports multiple simulation accounts with isolated balances:

```rust
// Single account (simple setup)
let exchange = SimulatedExchange::new()
    .with_account(Account::simulated("DEFAULT", "USDT"))
    .with_default_account(AccountId::new("DEFAULT"));

// Multi-account from TOML config
let config = AccountsConfig::with_default("MAIN", "USDT", dec!(100000))
    .add_simulation_account("AGGRESSIVE", "USDT", dec!(50000), vec!["momentum".into()])
    .add_simulation_account("CONSERVATIVE", "USDT", dec!(200000), vec!["rsi".into()]);

let exchange = SimulatedExchange::new()
    .with_accounts_config(&config);
```

**Key features**:
- `HashMap<AccountId, Account>`: Multiple accounts with isolated balances
- `locked_amounts: HashMap<ClientOrderId, (AccountId, Decimal)>`: Per-order fund locking tracks source account
- `resolve_account_id()`: Routes orders to correct account (falls back to default if empty)
- Strategy-account mapping: Strategies can specify `account_id()` to use specific accounts

**Account access methods**:
- `account(&AccountId)`: Get specific account by ID
- `default_account()`: Get the default account
- `all_accounts()`: Iterate over all accounts
- `total_locked_for_account(&AccountId)`: Get total locked funds for an account

#### 6. Paper Trading (trading-core/src/live_trading/paper_trading.rs)

Real-time strategy validation against live data.

**Per-tick processing**:
1. Fetch recent 20 ticks from cache for context
2. Generate signal via strategy
3. Execute against simulated portfolio
4. Log to `live_strategy_log` table
5. Print P&L and metrics

**Important**: Paper trading executes in the processing pipeline - slight per-tick latency increase (~50-100µs).

#### 7. Tauri Desktop Integration (src-tauri/src/)

**Pattern**: IPC (Inter-Process Communication) between Next.js frontend and Rust backend

Exposed commands (src-tauri/src/commands.rs):
- `get_data_info()`: Backtest metadata (symbols, date ranges, record counts)
- `get_available_strategies()`: List strategies with descriptions
- `run_backtest()`: Execute backtest with parameters
- `get_ohlc_preview()`: Generate candle data for charts
- `validate_backtest_config()`: Check data availability

**State management**: `AppState` wraps `Arc<TickDataRepository>` for thread-safe sharing across Tauri commands.

#### 8. Alerting System (trading-core/src/alerting/)

**Pattern**: Rule-based monitoring with periodic evaluation and cooldown

Components:
- `AlertRule`: Threshold-based conditions (IPC connection, cache health, channel utilization)
- `AlertEvaluator`: Background task that evaluates rules periodically (default: every 30s)
- `AlertHandler`: Pluggable handlers (logging, future: email, Slack, PagerDuty)

**Alert Rules** (4 production-ready rules):
1. **IPC Disconnected** (CRITICAL): IPC connection status = 0
2. **IPC Reconnection Storm** (WARNING): Reconnections > 10 attempts
3. **Channel Backpressure** (WARNING): Channel utilization ≥ 80%
4. **Cache Failure Rate** (WARNING): Cache update failures ≥ 10% of processed ticks

**Cooldown mechanism**: 5-minute default cooldown between repeated alerts to prevent spam

**Integration**:
- Initialized during application startup in `main.rs::init_alerting_system()`
- Reads thresholds from `config/development.toml` `[alerting]` section
- Monitors Prometheus metrics (IPC status, cache failures, channel utilization)
- Logs alerts to stdout/stderr with severity levels (INFO/WARNING/CRITICAL)

**Configuration** (config/development.toml):
```toml
[alerting]
enabled = true
interval_secs = 30                      # Evaluation frequency
cooldown_secs = 300                     # 5 minutes between repeated alerts
channel_backpressure_threshold = 80.0   # 80% WARNING
reconnection_storm_threshold = 10       # >10 reconnects WARNING
cache_failure_threshold = 0.1           # 10% WARNING
```

**Test coverage**: Tests passing (see `alerting/tests.rs`)

#### 9. IPC Transport System (data-manager/src/transport/ipc/)

**Pattern**: Lock-free shared memory ring buffers with service registry for multi-instance discovery

The IPC transport provides ultra-low latency (~10µs) data distribution from data-manager to trading-core using POSIX shared memory.

**Components**:
- `SharedMemoryTransport`: Creates/manages per-symbol ring buffer channels
- `SharedMemoryChannel`: Single symbol's shared memory segment with SPSC ring buffer
- `ControlChannel`: Request-response channel for dynamic subscription
- `Registry`: Service registry for multi-instance discovery

**Multi-Instance Support**:

When running multiple data-manager instances (e.g., one for Kraken, one for Binance), the service registry enables trading-core to automatically discover which instance serves each symbol.

```bash
# Terminal 1: Kraken instance
cargo run serve --live --provider kraken --symbols BTCUSD,ETHUSD --ipc --instance-id kraken-prod

# Terminal 2: Binance instance
cargo run serve --live --provider binance --symbols BNBUSD,SOLUSD --ipc --instance-id binance-prod

# Terminal 3: trading-core auto-discovers correct instance
cargo run live --paper-trading --symbols BTCUSD  # → Uses kraken-prod
cargo run live --paper-trading --symbols BNBUSD  # → Uses binance-prod
```

**Service Registry** (`data-manager/src/transport/ipc/registry.rs`):
- Shared memory at `/data_manager_registry` (64 slots, ~28KB total)
- Each entry (432 bytes): instance_id, provider, exchange, channel_prefix, symbols, heartbeat
- Heartbeat every 5 seconds, entries stale after 30 seconds
- Automatic deregistration on graceful shutdown

**Instance-Specific IPC Paths**:
- Default: `/data_manager_BTCUSD_KRAKEN`
- With instance ID: `/data_manager_kraken-prod__BTCUSD_KRAKEN`
- Prevents conflicts between multiple data-manager instances

**Dynamic Subscription via Control Channel**:

If trading-core requests a symbol that's subscribed but not streaming via IPC, the control channel allows runtime channel creation:

```
trading-core                    data-manager
     │                               │
     ├── SUBSCRIBE BTCUSD ──────────►│
     │                               │ (checks: is BTCUSD subscribed?)
     │                               │ (creates IPC channel if yes)
     │◄──────── SUCCESS ─────────────┤
     │                               │
     ├── (opens IPC channel) ────────┤
```

**Key files**:
- `data-manager/src/transport/ipc/shared_memory.rs` - Transport and channel implementation
- `data-manager/src/transport/ipc/ring_buffer.rs` - Lock-free SPSC ring buffer
- `data-manager/src/transport/ipc/control.rs` - Control channel protocol
- `data-manager/src/transport/ipc/registry.rs` - Service registry
- `data-manager/src/cli/control_handler.rs` - Server-side request handling
- `trading-core/src/data_source/ipc.rs` - Client-side with registry discovery

**CLI flags for serve command**:
- `--ipc`: Enable IPC transport
- `--ipc-symbols BTCUSD,ETHUSD`: Subset of symbols to stream via IPC (default: all)
- `--instance-id my-instance`: Custom instance identifier (default: auto-generated)

#### 10. Unified Venue Infrastructure (trading-common/src/venue/)

**Pattern**: Shared base traits and infrastructure for both data ingestion and order execution

The `venue` module consolidates connection lifecycle, symbol normalization, error handling, and HTTP/WebSocket infrastructure that was previously duplicated between data providers and execution venues.

**Module Structure**:
```
trading-common/src/venue/
├── mod.rs              # Public exports
├── connection.rs       # VenueConnection trait, ConnectionStatus
├── symbology.rs        # SymbolNormalizer trait, NativeDbVenue marker
├── error.rs            # VenueError with error classification
├── config.rs           # VenueConfig, RestConfig, StreamConfig, AuthConfig
├── types.rs            # VenueInfo, VenueCapabilities
├── data/               # Data plane traits (future: DataStreamVenue)
├── execution/          # Execution plane traits (future: OrderSubmissionVenue)
├── http/               # HTTP client with rate limiting and request signing
│   ├── client.rs       # HttpClient with automatic retries
│   ├── rate_limiter.rs # Multi-bucket rate limiter (governor crate)
│   └── signer.rs       # RequestSigner trait for venue-specific auth
└── websocket/          # WebSocket client with reconnection
    └── client.rs       # WebSocketClient with exponential backoff
```

**VenueConnection Base Trait**:

All venues (data or execution) implement this shared interface:

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

**ConnectionStatus**:
- `Disconnected` - Not connected
- `Connecting` - Connection in progress
- `Connected` - Ready for operations
- `Reconnecting` - Temporarily disconnected, auto-reconnecting
- `Error` - Connection failed (check logs)

**Unified SymbolNormalizer**:

Located in `trading-common/src/venue/symbology.rs`, provides DBT canonical symbol conversion:

```rust
pub trait SymbolNormalizer: Send + Sync {
    fn to_canonical(&self, venue_symbol: &str) -> VenueResult<String>;
    fn to_venue(&self, canonical_symbol: &str) -> VenueResult<String>;
    fn venue_id(&self) -> &str;
    fn register_symbols(&self, symbols: &[String], registry: &InstrumentRegistry)
        -> VenueResult<HashMap<String, u32>>;
    fn is_native_dbt(&self) -> bool { false }
}

// Marker for DBT-native venues (auto-implements SymbolNormalizer with no-op)
pub trait NativeDbVenue: Send + Sync {
    fn venue_id(&self) -> &str;
}
```

**Error Classification** (VenueError):

Errors are classified for retry logic:
- **Transient**: Network timeouts, rate limits (auto-retry with backoff)
- **Permanent**: Authentication failures, invalid orders (do not retry)
- **ResourceExhausted**: Rate limits, insufficient balance (wait and retry)

```rust
pub enum VenueError {
    Connection(String),      // Transient
    Authentication(String),  // Permanent
    RateLimit(String),       // ResourceExhausted
    OrderRejected(String),   // Permanent
    InsufficientBalance(String), // ResourceExhausted
    // ... other variants
}

impl VenueError {
    pub fn is_transient(&self) -> bool;
    pub fn is_retryable(&self) -> bool;
    pub fn suggested_retry_delay(&self) -> Option<Duration>;
}
```

**HTTP Infrastructure** (`venue/http/`):

- `HttpClient`: Authenticated HTTP client with automatic rate limiting
- `RequestSigner`: Trait for venue-specific HMAC/signature generation
- `RateLimiter`: Multi-bucket rate limiter using governor crate

```rust
// Example: Creating authenticated client
let signer = BinanceSigner::new(api_key, api_secret);
let client = HttpClient::new(config, Arc::new(signer))?;
let response = client.get("/api/v3/account").await?;
```

**WebSocket Infrastructure** (`venue/websocket/`):

- `WebSocketClient`: Reconnecting WebSocket with exponential backoff
- Ping/pong keepalive handling
- Graceful shutdown via broadcast channel

```rust
let client = WebSocketClientBuilder::new("wss://stream.example.com/ws")
    .max_reconnect_attempts(10)
    .reconnect_initial_delay(Duration::from_secs(1))
    .ping_interval(Duration::from_secs(30))
    .build();

client.run(callback, shutdown_rx).await?;
```

**Backward Compatibility**:

The `execution/venue/` module re-exports from `venue/` for existing code:

```rust
// Old path (still works)
use trading_common::execution::venue::{VenueError, VenueConfig};

// New canonical path (preferred)
use trading_common::venue::{VenueError, VenueConfig};
```

**Key files**:
- `trading-common/src/venue/mod.rs` - Central exports
- `trading-common/src/venue/connection.rs` - VenueConnection trait
- `trading-common/src/venue/symbology.rs` - SymbolNormalizer trait
- `trading-common/src/venue/error.rs` - VenueError with classification
- `trading-common/src/execution/venue/mod.rs` - Backward-compatible re-exports

## Configuration System

### Environment Variables

Required in `.env` (root directory):
```
DATABASE_URL=postgresql://username:password@localhost/trading_core
REDIS_URL=redis://127.0.0.1:6379
RUN_MODE=development  # or production, test
```

Optional:
```
RUST_LOG=trading_core=info,data_manager=info,sqlx=warn  # Logging levels
DATABENTO_API_KEY=your_api_key  # Required for data-manager fetch/backfill commands
DATA_MANAGER_CONFIG_DIR=/custom/config/path  # Custom config directory for data-manager
```

### Config Files (config/)

- `development.toml`: Dev environment settings
- `production.toml`: Production settings
- `test.toml`: Test environment
- `schema.sql`: PostgreSQL table definitions

**Key configuration**:
```toml
symbols = ["BTCUSD", "ETHUSD", ...]  # Trading pairs (DBT canonical format)

[database]
max_connections = 5
min_connections = 1

[cache.memory]
max_ticks_per_symbol = 1000
ttl_seconds = 300

[cache.redis]
max_ticks_per_symbol = 10000
ttl_seconds = 3600

[paper_trading]
enabled = true
strategy = "rsi"
initial_capital = 10000.0

# =============================================================================
# SIMULATION ACCOUNTS (Multi-Account Support)
# =============================================================================
[accounts.default]
id = "SIM-001"
currency = "USDT"
initial_balance = "100000"

[[accounts.simulation]]
id = "SIM-AGGRESSIVE"
currency = "USDT"
initial_balance = "50000"
strategies = ["momentum", "breakout"]  # Strategies that use this account

[[accounts.simulation]]
id = "SIM-CONSERVATIVE"
currency = "USDT"
initial_balance = "200000"
strategies = ["mean_reversion", "rsi"]
```

**Accounts configuration** (trading-common/src/accounts/config.rs):
- `AccountsConfig`: Root config with default account + simulation accounts
- `account_for_strategy(strategy_id)`: Maps strategy to its account (falls back to default)
- `build_accounts()`: Creates `Vec<Account>` from config
- `build_accounts_map()`: Creates `HashMap<AccountId, Account>` for exchange

**Builder pattern for programmatic setup**:
```rust
use trading_common::accounts::AccountsConfig;
use rust_decimal_macros::dec;

let config = AccountsConfig::with_default("MAIN", "USDT", dec!(100000))
    .add_simulation_account("AGGRESSIVE", "USDT", dec!(50000), vec!["momentum".into()])
    .add_simulation_account("CONSERVATIVE", "USDT", dec!(200000), vec!["rsi".into()]);

// Use with BacktestEngine
let engine = BacktestEngine::new()
    .with_accounts_config(&config);
```

## Database Setup

```bash
# Create database
createdb trading_core

# Apply schema
psql -d trading_core -f config/schema.sql

# Verify tables
psql -d trading_core -c "\dt"
# Expected: tick_data, live_strategy_log
```

**Important tables**:
- `tick_data`: Stores all tick-level trades (symbol, price, quantity, side, timestamp, trade_id)
- `live_strategy_log`: Paper trading execution logs

## Data Flow Patterns

### Live Mode (with Paper Trading)

**Architecture**: trading-core receives all live data via IPC from data-manager. Tick persistence is handled by data-manager.

```
Terminal 1: cd data-manager && cargo run serve --live --ipc --persist --symbols BTCUSD
Terminal 2: cd trading-core && cargo run live --paper-trading

Data flow:
  data-manager (Exchange WebSocket: Binance, Kraken, etc.)
    → Shared memory ring buffer (IPC)
    → trading-core (IpcExchange)
    → Processing pipeline:
        1. Load config (cache, symbols, strategy)
        2. Initialize Redis cache, Strategy, Paper trading processor
        3. For each tick from IPC:
           a. Validate tick data
           b. Update cache (L1 memory + L2 Redis)
           c. Generate trading signal (if paper trading enabled)
           d. Log paper trade to live_strategy_log table
        4. Output real-time: BUY/SELL signals, portfolio value, P&L
        5. Shutdown (Ctrl+C): Graceful exit

Note: Tick data persistence to database is handled by data-manager (--persist flag).
trading-core only writes paper trading logs to live_strategy_log table.
```

### Backtest Mode (CLI or Desktop)

```
User selects backtest config

1. Load historical tick data for symbol from database
2. Initialize BacktestEngine with strategy and parameters
3. Generate BarData events based on strategy's bar_data_mode() and preferred_bar_type()
4. Sequentially process each bar event:
   - Update portfolio prices
   - Call strategy.on_bar_data(bar_data) → Signal
   - Execute signal (buy/sell)
5. Calculate metrics (Sharpe, drawdown, win rate)
6. Return results to UI or CLI
```

## Critical Implementation Details

### Async/Await and Tokio

All I/O is non-blocking using Tokio. Main runtime created in `main.rs`:
```rust
#[tokio::main]
async fn main() -> Result<()> { ... }
```

### Decimal Precision

Use `rust_decimal` crate for all financial calculations to avoid floating-point errors:
```rust
use rust_decimal::Decimal;
let price = Decimal::from_str("42150.25")?;
```

### Error Handling

Layered error types:
- `ExchangeError`: Network, WebSocket, parsing
- `ServiceError`: Aggregates exchange + data errors
- `DataError`: Database, validation, cache

Use `?` operator for propagation, `anyhow` for application-level errors.

### Graceful Shutdown

All long-running tasks use broadcast channel for coordinated shutdown:
```rust
let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
// In signal handler:
shutdown_tx.send(()).ok();
```

### Testing Strategy

- Unit tests: `cargo test` (in relevant module)
- Integration tests: `cargo test --test integration_test`
- Benchmarks: `cd trading-core && cargo bench`

**Important**: Tests require database and Redis. Use `test.toml` config with test database.

## Common Development Tasks

### Adding a New Exchange

1. Create `src/exchange/your_exchange.rs`
2. Implement `Exchange` trait with WebSocket connection logic
3. Add error variants to `ExchangeError` enum
4. Register in `src/exchange/mod.rs`
5. Update config to support new exchange selection

### Adding a New Data Provider (data-manager)

1. Create `data-manager/src/provider/your_provider/` directory with:
   - `mod.rs` - Public exports
   - `client.rs` - Implement `DataProvider` + `LiveStreamProvider` traits
   - `types.rs` - WebSocket message structs
   - `normalizer.rs` - Implement `SymbolNormalizer` trait + convert venue messages to `TickData`, `QuoteTick`, `OrderBook`
   - `symbol.rs` - Symbol format conversion functions (`to_canonical()`, `to_venue()`)
2. Register in `data-manager/src/provider/mod.rs`
3. Add provider settings in `data-manager/src/config/settings.rs`
4. Add provider dispatch in `data-manager/src/cli/serve.rs`

**Required trait implementations**:
- `DataProvider` - Base connectivity (connect, disconnect, discover_symbols)
- `LiveStreamProvider` - Real-time streaming (subscribe, unsubscribe)
- `SymbolNormalizer` - DBT symbology conversion (to_canonical, to_venue, register_symbols)

**Existing providers**:
- Binance: trades (live)
- Kraken Spot/Futures: trades, L1 quotes, L2 orderbook (live)
- Databento: trades, L1 quotes (BBO), L2 orderbook (MBP), L3 full book (MBO), OHLC (historical + live)

### Provider Trait Hierarchy

The framework enforces consistent interfaces via traits in `data-manager/src/provider/traits.rs`:

```
DataProvider (base - required for all providers)
├── connect(), disconnect(), is_connected()
├── discover_symbols()
└── info()

├── LiveStreamProvider (for real-time data)
│   ├── subscribe(), unsubscribe()
│   └── subscription_status()

├── HistoricalDataProvider (for backfill)
│   ├── fetch_ticks(), fetch_ohlc()
│   └── check_availability()

└── SymbolNormalizer (for non-DBT native providers)
    ├── to_canonical()      # Venue → DBT (BTCUSDT → BTCUSD)
    ├── to_venue()          # DBT → Venue (BTCUSD → BTCUSDT)
    ├── exchange_name()     # "BINANCE", "KRAKEN", etc.
    └── register_symbols()  # InstrumentRegistry integration
```

**SymbolNormalizer trait** enforces DBT canonical symbology at compile-time:

```rust
#[async_trait]
pub trait SymbolNormalizer: Send + Sync {
    /// Convert venue symbol to DBT canonical (BTCUSDT → BTCUSD)
    fn to_canonical(&self, venue_symbol: &str) -> Result<String, ProviderError>;

    /// Convert DBT canonical to venue format (BTCUSD → BTCUSDT)
    fn to_venue(&self, canonical_symbol: &str) -> Result<String, ProviderError>;

    /// Exchange identifier for registry lookups
    fn exchange_name(&self) -> &str;

    /// Pre-register symbols with InstrumentRegistry (default impl provided)
    async fn register_symbols(&self, symbols: &[String], registry: &Arc<InstrumentRegistry>)
        -> Result<HashMap<String, u32>, ProviderError>;
}
```

**For DBT-native providers** (like Databento), implement the marker trait instead:

```rust
pub trait NativeDbProvider: Send + Sync {
    fn exchange_name(&self) -> &str;
}
// Auto-implements SymbolNormalizer with no-op conversions
```

**Note**: A unified `SymbolNormalizer` trait also exists in `trading-common/src/venue/symbology.rs` for execution venues. The data-manager version uses `ProviderError` while the trading-common version uses `VenueError`. Error conversion is provided via `From<VenueError> for ProviderError`.

**Current implementations**:

| Provider | Trait | Conversions |
|----------|-------|-------------|
| `BinanceNormalizer` | `SymbolNormalizer` | BTCUSDT ↔ BTCUSD |
| `KrakenNormalizer` (Spot) | `SymbolNormalizer` | BTC/USD ↔ BTCUSD |
| `KrakenNormalizer` (Futures) | `SymbolNormalizer` | PI_XBTUSD ↔ BTCUSD |
| Databento | `NativeDbProvider` | No conversion needed |

**Key files**:
- `data-manager/src/provider/traits.rs` - Data provider trait definitions
- `data-manager/src/provider/binance/normalizer.rs` - Binance implementation
- `data-manager/src/provider/kraken/normalizer.rs` - Kraken implementation
- `data-manager/src/instruments/registry.rs` - InstrumentRegistry for persistent IDs
- `trading-common/src/venue/symbology.rs` - Unified SymbolNormalizer for execution venues

### Adding a New Trading Strategy

1. Create `trading-common/src/backtest/strategy/your_strategy.rs`
2. Implement `Strategy` trait:
   ```rust
   impl Strategy for YourStrategy {
       fn name(&self) -> &str { "Your Strategy" }

       fn on_bar_data(&mut self, bar_data: &BarData) -> Signal {
           // Access bar_data.ohlc_bar for OHLC data
           // Access bar_data.current_tick for current tick (if in OnEachTick mode)
           // Access bar_data.metadata for bar state info
           Signal::Hold
       }

       fn bar_data_mode(&self) -> BarDataMode {
           BarDataMode::OnCloseBar  // or OnEachTick, OnPriceMove
       }

       fn preferred_bar_type(&self) -> BarType {
           BarType::TimeBased(Timeframe::OneMinute)
       }

       fn initialize(&mut self, params: HashMap<String, String>) -> Result<(), String> { Ok(()) }
       fn reset(&mut self) { }
   }
   ```
3. Register in `trading-common/src/backtest/strategy/mod.rs`
4. Update frontend strategy selection UI

### Debugging Live Data Collection

```bash
# Terminal 1: Start data-manager with debug logging
cd data-manager
RUST_LOG=data_manager=debug cargo run serve --live --ipc --symbols BTCUSD

# Terminal 2: Start trading-core with debug logging
cd trading-core
RUST_LOG=trading_core=debug,sqlx=info cargo run live

# Check database inserts
psql -d trading_core -c "SELECT COUNT(*) FROM tick_data WHERE created_at > NOW() - INTERVAL '5 minutes';"

# Monitor Redis cache
redis-cli KEYS "ticks:*"
redis-cli LRANGE "ticks:BTCUSD" 0 10

# Check data-manager database stats
cd data-manager && cargo run db stats
```

### Performance Profiling

```bash
# Run benchmarks
cd trading-core
cargo bench

# Generate flamegraph (requires cargo-flamegraph)
cargo flamegraph --bin trading-core -- live
```

## Docker Deployment

The system supports running data-manager, TimescaleDB, and Redis in Docker containers while trading-core runs on the host for lowest latency.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Docker Containers                         │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │ TimescaleDB │  │   Redis     │  │   data-manager      │  │
│  │  :5432      │  │   :6379     │  │  (Exchange WS)      │  │
│  └─────────────┘  └─────────────┘  └──────────┬──────────┘  │
│                                               │              │
│                                     /dev/shm/trading (IPC)  │
└─────────────────────────────────────────────────────────────┘
                                               │
                                        volume mount
                                               │
┌─────────────────────────────────────────────────────────────┐
│                         Host                                 │
│  ┌─────────────────────┐                                    │
│  │   trading-core      │◄───── /dev/shm/trading (IPC)       │
│  │   (paper trading)   │                                    │
│  └─────────────────────┘                                    │
└─────────────────────────────────────────────────────────────┘
```

### Quick Start

```bash
# 1. Copy environment example and configure
cp .env.docker.example .env
# Edit .env with your POSTGRES_PASSWORD and other settings

# 2. Start containers
docker-compose up -d

# 3. Verify services are healthy
docker-compose ps

# 4. Start trading-core on host
cd trading-core && cargo run live --paper-trading
```

### Configuration Files

- `docker-compose.yml`: Main orchestration file
- `docker/Dockerfile.data-manager`: Multi-stage build for data-manager
- `.env.docker.example`: Environment variable template
- `config/schema.sql`: Database initialization (mounted as init script)

### Services

| Service | Container Name | Ports | Purpose |
|---------|---------------|-------|---------|
| timescaledb | trading-timescaledb | 5432:5432 | Time-series database for tick storage |
| redis | trading-redis | 6379:6379 | L2 cache layer |
| data-manager | trading-data-manager | - | Market data ingestion and IPC distribution |

### IPC Communication

Data flows via shared memory at `/dev/shm/trading`:
- **Container side**: data-manager writes ticks to ring buffer
- **Host side**: trading-core reads from same location via volume mount
- **Performance**: Near-native speed (~10µs latency)

### Commands

```bash
# View logs
docker-compose logs -f data-manager
docker-compose logs -f timescaledb

# Stop all services
docker-compose down

# Stop and remove volumes (WARNING: deletes data)
docker-compose down -v

# Rebuild data-manager image
docker-compose build data-manager

# Check database stats
docker-compose exec timescaledb psql -U trading -d trading_core -c "SELECT COUNT(*) FROM tick_data;"
```

### Environment Variables

Required:
- `POSTGRES_PASSWORD`: Database password (no default, must be set)

Optional:
- `POSTGRES_USER`: Database user (default: trading)
- `POSTGRES_DB`: Database name (default: trading_core)
- `TRADING_SYMBOLS`: Symbols to subscribe (default: BTCUSD,ETHUSD) - DBT canonical format
- `RUST_LOG`: Log levels (default: data_manager=info,sqlx=warn)
- `LOG_FORMAT`: Log format (default: json)
- `BINANCE_API_KEY`, `BINANCE_API_SECRET`: For authenticated Binance endpoints
- `DATABENTO_API_KEY`: For historical data fetching

## Known Constraints and Trade-offs

1. **IPC dependency**: trading-core requires data-manager to be running for live data - start data-manager first

2. **Cache miss on restart**: In-memory cache (L1) cleared on restart - Redis (L2) persists but takes ~100µs vs ~10µs for memory

3. **Strategy execution order**: Backtests process ticks sequentially (no parallel strategy execution) - ensures deterministic results

4. **Separation of concerns**: trading-core handles paper trading only, data-manager handles tick persistence - ensures clean architecture

5. **Database duplicate handling**: `ON CONFLICT DO NOTHING` silently ignores duplicates - no error logged, check `live_strategy_log` for actual insert counts

6. **Paper trading single strategy**: Only one strategy runs in live mode - multiple strategies require separate processes

7. **Multi-instance IPC**: When running multiple data-manager instances, each needs a unique `--instance-id` to avoid shared memory conflicts. Trading-core uses the service registry to discover instances automatically.

8. **Registry stale entries**: If a data-manager crashes without graceful shutdown, its registry entry becomes stale after 30 seconds. The registry auto-cleans stale entries when new instances register.

## Performance Expectations

Based on benchmarks (trading-core/benches/):

| Operation | Performance | Notes |
|-----------|-------------|-------|
| Single tick insert | ~390µs | Individual writes |
| Batch insert (100) | ~13ms | Optimized bulk operations |
| Batch insert (1000) | ~116ms | Large batch processing |
| Cache hit (L1) | ~10µs | Memory retrieval |
| Cache hit (L2) | ~100µs | Redis retrieval |
| Cache miss | ~11.6ms | Database fallback |
| Historical query | ~450µs | Backtest data retrieval |

**Bottlenecks**:
- Database inserts: Use batching (already implemented)
- Cache misses: Increase L2 TTL and max_ticks_per_symbol
- IPC throughput: Limited by shared memory ring buffer size
