# trading-core

CLI application for live data collection, paper trading, and backtesting interface.

## Module Overview

```
trading-core/src/
├── alerting/        # Alert rules and evaluation
├── data_source/     # IPC exchange client
├── exchange/        # Exchange trait abstraction
├── live_trading/    # Paper trading processor
├── service/         # Market data service
└── main.rs          # CLI entry point
```

## CLI Commands

```bash
# Start live data collection (requires data-manager running)
cargo run live

# Live with paper trading
cargo run live --paper-trading

# Interactive backtesting
cargo run backtest

# Hash strategy file
cargo run hash-strategy <file_path>
```

## Exchange Layer (exchange/ and data_source/)

**Pattern**: Trait-based abstraction for data sources

- `Exchange` trait: Standard interface for all data sources
- `IpcExchange`: Receives data from data-manager via shared memory IPC
- Callback pattern: `subscribe_trades()` accepts closure for real-time processing
- Graceful shutdown via broadcast channel (`shutdown_rx`)

**Data flow**: data-manager (WebSocket) → IPC shared memory → trading-core (IpcExchange)

**Important**: Exchange callbacks execute synchronously - avoid blocking operations.

## Service Layer (service/market_data.rs)

**Pattern**: Event-driven pipeline for cache updates and paper trading

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

## Paper Trading (live_trading/paper_trading.rs)

Real-time strategy validation against live data.

**Per-tick processing**:
1. Fetch recent 20 ticks from cache for context
2. Generate signal via strategy
3. Execute against simulated portfolio
4. Log to `live_strategy_log` table
5. Print P&L and metrics

Paper trading executes in the processing pipeline (~50-100µs per-tick latency).

## Alerting System (alerting/)

**Pattern**: Rule-based monitoring with periodic evaluation and cooldown

**Components**:
- `AlertRule`: Threshold-based conditions
- `AlertEvaluator`: Background task (default: 30s interval)
- `AlertHandler`: Pluggable handlers (logging, future: email, Slack)

**Built-in Rules**:
1. **IPC Disconnected** (CRITICAL): Connection status = 0
2. **IPC Reconnection Storm** (WARNING): Reconnections > 10
3. **Channel Backpressure** (WARNING): Utilization ≥ 80%
4. **Cache Failure Rate** (WARNING): Failures ≥ 10%

**Cooldown**: 5-minute default between repeated alerts.

**Configuration** (config/development.toml):
```toml
[alerting]
enabled = true
interval_secs = 30
cooldown_secs = 300
channel_backpressure_threshold = 80.0
reconnection_storm_threshold = 10
cache_failure_threshold = 0.1
```

## IPC Client (data_source/ipc.rs)

Connects to data-manager via shared memory with registry discovery.

**Multi-instance discovery**:
```bash
# data-manager instances register in service registry
cargo run serve --live --provider kraken --symbols BTCUSD --ipc --instance-id kraken-prod

# trading-core auto-discovers correct instance
cargo run live --paper-trading --symbols BTCUSD  # → Uses kraken-prod
```

**Registry**: Shared memory at `/data_manager_registry`, auto-discovers which data-manager serves each symbol.

## Debugging

```bash
# Debug logging
RUST_LOG=trading_core=debug,sqlx=info cargo run live

# Check cache
redis-cli KEYS "ticks:*"
redis-cli LRANGE "ticks:BTCUSD" 0 10
```

## Performance

| Operation | Latency |
|-----------|---------|
| IPC read | ~10µs |
| Cache hit (L1) | ~10µs |
| Cache hit (L2) | ~100µs |
| Paper trade signal | ~50-100µs |
