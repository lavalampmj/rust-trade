# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

### Backend (Rust)

```bash
# Build entire workspace (trading-core + trading-common + src-tauri)
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

### Trading Core CLI

```bash
cd trading-core

# Start live data collection (WebSocket from Binance)
cargo run live

# Start live data collection with paper trading enabled
cargo run live --paper-trading

# Start interactive backtesting interface
cargo run backtest

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

The project uses a Rust workspace with three crates:

- **trading-common** (shared library): Core domain models, backtesting engine, data repository, and caching
- **trading-core** (CLI application): Live data collection from exchanges, paper trading, CLI interface
- **src-tauri** (desktop app backend): Tauri commands exposing trading-common functionality to Next.js frontend

This separation enables code reuse: both trading-core and src-tauri depend on trading-common, avoiding duplication.

### Key Architectural Patterns

#### 1. Exchange Layer (trading-core/src/exchange/)

**Pattern**: Trait-based abstraction for multiple exchange support

- `Exchange` trait defines standard interface for all exchanges
- `BinanceExchange` implements WebSocket streaming with auto-reconnection (max 10 attempts with exponential backoff)
- Uses callback pattern: `subscribe_trades()` accepts closure for real-time tick processing
- Graceful shutdown via broadcast channel (`shutdown_rx`)

**Important**: Exchange callbacks execute synchronously - avoid blocking operations in handlers.

#### 2. Service Layer (trading-core/src/service/market_data.rs)

**Pattern**: Event-driven pipeline with batch processing

Data flow:
```
Exchange (WebSocket callback)
  → Collection Task (mpsc channel)
  → Processing Task
    → Cache Update (L1 + L2 in parallel)
    → Batch Accumulation (100 ticks or 1s timeout)
    → Database Bulk Insert (with retry: 3 attempts)
    → Paper Trading Processor (optional)
```

**Key features**:
- Dual-task architecture decouples collection from processing
- Batching: 100 ticks or 1 second window, whichever comes first
- Retry logic: Failed batches retry 3 times with 1000ms delay
- Graceful shutdown: Flushes remaining batches before exit

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
- `Strategy` trait: Implement `on_tick()` or `on_ohlc()` to generate signals (BUY/SELL/HOLD)
- `Portfolio`: Tracks cash, positions, trades, calculates P&L
- `BacktestMetrics`: Sharpe ratio, max drawdown, win rate, profit factor

**Built-in strategies**:
- SMA (Simple Moving Average): Crossover-based (short/long MA)
- RSI (Relative Strength Index): Overbought/oversold levels

**Adding new strategy**: Implement `Strategy` trait in `trading-common/src/backtest/strategy/`, register in `strategy/mod.rs`.

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

## Configuration System

### Environment Variables

Required in `.env` (root and trading-core/):
```
DATABASE_URL=postgresql://username:password@localhost/trading_core
REDIS_URL=redis://127.0.0.1:6379
RUN_MODE=development  # or production, test
```

Optional:
```
RUST_LOG=trading_core=info,sqlx=warn  # Logging levels
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

```
User: cargo run live --paper-trading

1. Load config (database, cache, symbols, strategy)
2. Initialize PostgreSQL pool, Redis cache, Strategy, Paper trading processor
3. Start WebSocket connection to Binance
4. For each tick:
   a. Update cache (immediate)
   b. Generate trading signal (if paper trading enabled)
   c. Add to batch accumulator
   d. Flush to database when batch full (100) or timeout (1s)
5. Output real-time: BUY/SELL signals, portfolio value, P&L
6. Shutdown (Ctrl+C): Graceful flush and exit
```

### Backtest Mode (CLI or Desktop)

```
User selects backtest config

1. Load historical tick data for symbol from database
2. Initialize BacktestEngine with strategy and parameters
3. Sequentially process each tick:
   - Update portfolio prices
   - Call strategy.on_tick() → Signal
   - Execute signal (buy/sell)
4. Calculate metrics (Sharpe, drawdown, win rate)
5. Return results to UI or CLI
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

### Adding a New Trading Strategy

1. Create `trading-common/src/backtest/strategy/your_strategy.rs`
2. Implement `Strategy` trait:
   ```rust
   impl Strategy for YourStrategy {
       fn name(&self) -> &str { "Your Strategy" }
       fn on_tick(&mut self, tick: &TickData) -> Signal { ... }
       fn reset(&mut self) { ... }
   }
   ```
3. Register in `trading-common/src/backtest/strategy/mod.rs`
4. Update frontend strategy selection UI

### Debugging Live Data Collection

```bash
# Enable debug logging
RUST_LOG=trading_core=debug,sqlx=info cargo run live

# Check database inserts
psql -d trading_core -c "SELECT COUNT(*) FROM tick_data WHERE created_at > NOW() - INTERVAL '5 minutes';"

# Monitor Redis cache
redis-cli KEYS "ticks:*"
redis-cli LRANGE "ticks:BTCUSDT" 0 10
```

### Performance Profiling

```bash
# Run benchmarks
cd trading-core
cargo bench

# Generate flamegraph (requires cargo-flamegraph)
cargo flamegraph --bin trading-core -- live
```

## Known Constraints and Trade-offs

1. **Batch processing latency**: Ticks buffered for up to 1 second before database flush - acceptable for backtesting, consider for latency-sensitive use cases

2. **Cache miss on restart**: In-memory cache (L1) cleared on restart - Redis (L2) persists but takes ~100µs vs ~10µs for memory

3. **Strategy execution order**: Backtests process ticks sequentially (no parallel strategy execution) - ensures deterministic results

4. **WebSocket reconnection limit**: Max 10 attempts with exponential backoff - manual restart required after persistent failures

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
- WebSocket throughput: Binance limit ~1000 msg/s per connection
