# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Per-crate documentation**: See `CLAUDE.md` in each crate for implementation details:
- `trading-common/CLAUDE.md` - Cache, repository, backtest, venue infrastructure
- `trading-core/CLAUDE.md` - Exchange, service layer, paper trading, alerting
- `data-manager/CLAUDE.md` - Providers, IPC transport, instruments
- `src-tauri/CLAUDE.md` - Tauri commands, state management

## Build Commands

```bash
cargo build                    # Build workspace
cargo build --release          # Optimized build
cargo test                     # Run tests
cargo fmt && cargo clippy      # Format and lint
cd trading-core && cargo bench # Benchmarks
```

## CLI Quick Reference

### data-manager
```bash
cd data-manager
cargo run serve --live --provider kraken --symbols BTCUSD,ETHUSD --ipc      # Live streaming
cargo run serve --live --ipc --persist                                       # With persistence
cargo run fetch --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31  # Historical
cargo run symbol list                                                        # Symbol management
cargo run db stats                                                           # Database stats
```

### trading-core
```bash
cd trading-core
cargo run live                  # Live data (requires data-manager)
cargo run live --paper-trading  # With paper trading
cargo run backtest              # Interactive backtesting
```

### Frontend/Tauri
```bash
cd frontend
npm run dev           # Web development
npm run tauri dev     # Desktop with hot reload
npm run tauri build   # Production build
```

## Architecture Overview

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  data-manager   │────>│  trading-core   │     │    src-tauri    │
│  (data infra)   │ IPC │  (paper trade)  │     │  (desktop app)  │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         └───────────────────────┴───────────────────────┘
                                 │
                    ┌────────────┴────────────┐
                    │     trading-common      │
                    │  (shared library)       │
                    └─────────────────────────┘
```

**Data flow**: data-manager (Exchange WebSocket) → IPC shared memory → trading-core

## DBT-Native Architecture

Uses **Databento (DBT) symbology** as canonical internal format.

**DBT format**: `BTCUSD`, `ETHUSD`, `SOLUSD` (base + quote, no separators)

| Location | Format | Example |
|----------|--------|---------|
| CLI args, config, database, cache | DBT canonical | `BTCUSD` |
| Venue WebSocket | Venue-specific | `BTCUSDT`, `BTC/USD`, `PI_XBTUSD` |

**Edge conversions** handled by `SymbolNormalizer` trait in providers.

**DBN data types**: `TradeMsg`, `BboMsg`, `Mbp10Msg`, `MboMsg` (fixed-point, compact)

## Configuration

### Environment Variables (.env)
```
DATABASE_URL=postgresql://user:pass@localhost/trading_core
REDIS_URL=redis://127.0.0.1:6379
RUN_MODE=development
DATABENTO_API_KEY=your_key  # For historical data
```

### Config Files (config/)
```toml
symbols = ["BTCUSD", "ETHUSD"]  # DBT canonical

[database]
max_connections = 5

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

[accounts.default]
id = "SIM-001"
currency = "USDT"
initial_balance = "100000"
```

## Database Setup

```bash
createdb trading_core
psql -d trading_core -f config/schema.sql
```

**Tables**: `tick_data` (trades), `live_strategy_log` (paper trading logs)

## Data Flow Patterns

### Live Mode
```
Terminal 1: cd data-manager && cargo run serve --live --ipc --persist --symbols BTCUSD
Terminal 2: cd trading-core && cargo run live --paper-trading

Flow: Exchange WebSocket → data-manager → IPC → trading-core → Cache + Paper Trading
```

### Backtest Mode
```
1. Load historical ticks from database
2. Generate BarData events per strategy's bar_data_mode()
3. Process signals, execute trades
4. Calculate metrics (Sharpe, drawdown, win rate)
```

## Critical Implementation Details

- **Async/Tokio**: All I/O non-blocking, `#[tokio::main]` runtime
- **Decimal precision**: Use `rust_decimal` for all financial calculations
- **Error handling**: Layered types (`ExchangeError`, `ServiceError`, `DataError`), `anyhow` for CLI
- **Graceful shutdown**: Broadcast channel pattern (`shutdown_tx.send(()).ok()`)
- **Testing**: `cargo test`, requires database and Redis with `test.toml` config

## Docker Deployment

```bash
cp .env.docker.example .env
docker-compose up -d
cd trading-core && cargo run live --paper-trading
```

IPC via volume mount: `/dev/shm/trading`

| Service | Port | Purpose |
|---------|------|---------|
| timescaledb | 5432 | Tick storage |
| redis | 6379 | L2 cache |
| data-manager | - | Market data + IPC |

## Known Constraints

1. **IPC dependency**: Start data-manager before trading-core
2. **Cache miss on restart**: L1 cleared, L2 persists (~100µs vs ~10µs)
3. **Sequential backtests**: No parallel strategy execution (deterministic)
4. **Single paper trading strategy**: Multiple require separate processes
5. **Multi-instance IPC**: Each data-manager needs unique `--instance-id`

## Performance

| Operation | Latency |
|-----------|---------|
| IPC read | ~10µs |
| Cache L1 hit | ~10µs |
| Cache L2 hit | ~100µs |
| Cache miss (DB) | ~11.6ms |
| Batch insert (1000) | ~116ms |
