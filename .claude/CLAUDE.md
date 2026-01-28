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
cargo run serve --live --provider binance --symbols BTCUSDT,ETHUSDT --ipc

# Start with Kraken Spot streaming
cargo run serve --live --provider kraken --symbols XBT/USD,ETH/USD --ipc

# Start with Kraken Futures streaming (demo/testnet)
cargo run serve --live --provider kraken_futures --demo --symbols PI_XBTUSD,PI_ETHUSD --ipc

# Start with persistence to database
cargo run serve --live --ipc --persist

# Start with custom config file
cargo run serve -c ../config/development.toml --live --ipc

# Fetch historical data from Databento
cargo run fetch --symbols BTCUSDT --exchange binance --provider databento --start 2024-01-01 --end 2024-01-31

# Dry run to see what would be fetched
cargo run fetch --symbols BTCUSDT --exchange binance --start 2024-01-01 --end 2024-01-31 --dry-run

# Symbol management
cargo run symbol list                           # List registered symbols
cargo run symbol add --symbols BTCUSDT,ETHUSDT  # Add symbols
cargo run symbol remove --symbols DOGEUSDT      # Remove symbols
cargo run symbol discover --provider binance    # Discover available symbols

# Database operations
cargo run db migrate                            # Run database migrations
cargo run db stats                              # Show database statistics
cargo run db compress                           # Compress old data

# Backfill with cost tracking
cargo run backfill estimate --symbols BTCUSDT --start 2024-01-01 --end 2024-01-31
cargo run backfill fetch --symbols BTCUSDT --start 2024-01-01 --end 2024-01-31
cargo run backfill fill-gaps --symbols BTCUSDT  # Detect and fill data gaps
cargo run backfill status                       # Show backfill status
cargo run backfill cost-report                  # Show cost report

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
symbols = ["BTCUSDT", "ETHUSDT", ...]  # Trading pairs to monitor

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
Terminal 1: cd data-manager && cargo run serve --live --ipc --persist --symbols BTCUSDT
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
   - `normalizer.rs` - Convert venue messages to `TickData`, `QuoteTick`, `OrderBook`
   - `symbol.rs` - Symbol format conversion (optional)
2. Register in `data-manager/src/provider/mod.rs`
3. Add provider settings in `data-manager/src/config/settings.rs`
4. Add provider dispatch in `data-manager/src/cli/serve.rs`

**Existing providers**:
- Binance: trades (live)
- Kraken Spot/Futures: trades, L1 quotes, L2 orderbook (live)
- Databento: trades, L1 quotes (BBO), L2 orderbook (MBP), L3 full book (MBO), OHLC (historical + live)

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
RUST_LOG=data_manager=debug cargo run serve --live --ipc --symbols BTCUSDT

# Terminal 2: Start trading-core with debug logging
cd trading-core
RUST_LOG=trading_core=debug,sqlx=info cargo run live

# Check database inserts
psql -d trading_core -c "SELECT COUNT(*) FROM tick_data WHERE created_at > NOW() - INTERVAL '5 minutes';"

# Monitor Redis cache
redis-cli KEYS "ticks:*"
redis-cli LRANGE "ticks:BTCUSDT" 0 10

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
- `TRADING_SYMBOLS`: Symbols to subscribe (default: BTCUSDT,ETHUSDT)
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
