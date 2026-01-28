# data-manager

Centralized market data infrastructure: loading, streaming, persistence, and IPC distribution.

## Module Overview

```
data-manager/src/
├── cli/             # CLI commands (serve, fetch, backfill, db, symbol, import)
├── config/          # Settings, routing configuration
├── instruments/     # InstrumentRegistry for persistent IDs
├── provider/        # Data providers (binance, kraken, databento)
│   └── kraken/
│       └── csv_parser.rs  # Kraken T&S CSV import with Lee-Ready side inference
├── storage/         # Database persistence
└── transport/       # IPC shared memory transport
```

## CLI Commands

```bash
# Live streaming with IPC
cargo run serve --live --provider kraken --symbols BTCUSD,ETHUSD --ipc

# With persistence
cargo run serve --live --ipc --persist

# Selective IPC (stream subset to IPC)
cargo run serve --live --provider kraken \
  --symbols BTCUSD,ETHUSD,SOLUSD,XRPUSD \
  --ipc --ipc-symbols BTCUSD,ETHUSD \
  --persist

# Historical fetch with auto-routing
cargo run fetch --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31

# Symbol management
cargo run symbol list
cargo run symbol add --symbols BTCUSD,ETHUSD
cargo run symbol discover --provider kraken

# Database operations
cargo run db migrate
cargo run db stats
cargo run db compress

# Backfill
cargo run backfill estimate --symbols ESH5 --exchange CME --start 2024-01-01 --end 2024-01-31
cargo run backfill fetch --symbols ESH5 --exchange CME --asset-type futures --start 2024-01-01 --end 2024-01-31

# Import historical data from files
cargo run import kraken --zip data/Kraken_Trading_History.zip --list          # List symbols
cargo run import kraken --zip data/history.zip --symbols BTCUSD --dry-run     # Dry run
cargo run import kraken --zip data/history.zip --symbols BTCUSD,ETHUSD        # Import specific
cargo run import kraken --zip data/history.zip --all --start 2023-01-01       # Import all with date filter
cargo run import kraken --input XBTUSD.csv --infer-side true                  # Single file
cargo run import csv --input trades.csv --exchange KRAKEN --symbol BTCUSD     # Generic CSV
```

## Provider Trait Hierarchy (provider/traits.rs)

```
DataProvider (base - required)
├── connect(), disconnect(), is_connected()
├── discover_symbols()
└── info()

├── LiveStreamProvider (real-time)
│   ├── subscribe(), unsubscribe()
│   └── subscription_status()

├── HistoricalDataProvider (backfill)
│   ├── fetch_ticks(), fetch_ohlc()
│   └── check_availability()

└── SymbolNormalizer (non-DBT providers)
    ├── to_canonical()      # BTCUSDT → BTCUSD
    ├── to_venue()          # BTCUSD → BTCUSDT
    ├── exchange_name()
    └── register_symbols()
```

### SymbolNormalizer

```rust
#[async_trait]
pub trait SymbolNormalizer: Send + Sync {
    fn to_canonical(&self, venue_symbol: &str) -> Result<String, ProviderError>;
    fn to_venue(&self, canonical_symbol: &str) -> Result<String, ProviderError>;
    fn exchange_name(&self) -> &str;
    async fn register_symbols(&self, symbols: &[String], registry: &Arc<InstrumentRegistry>)
        -> Result<HashMap<String, u32>, ProviderError>;
}

// For DBT-native providers (Databento)
pub trait NativeDbProvider: Send + Sync {
    fn exchange_name(&self) -> &str;
}
```

### Current Implementations

| Provider | Trait | Conversions |
|----------|-------|-------------|
| `BinanceNormalizer` | `SymbolNormalizer` | BTCUSDT ↔ BTCUSD |
| `KrakenNormalizer` (Spot) | `SymbolNormalizer` | BTC/USD ↔ BTCUSD |
| `KrakenNormalizer` (Futures) | `SymbolNormalizer` | PI_XBTUSD ↔ BTCUSD |
| Databento | `NativeDbProvider` | No conversion |

## Instrument Registry (instruments/registry.rs)

Persistent, deterministic `instrument_id` assignment:

```
┌─────────────────────────────────────────────────────────┐
│                   InstrumentRegistry                     │
│  ┌───────────┐    ┌───────────┐    ┌─────────────────┐ │
│  │ L1 Cache  │───>│ L2 Cache  │───>│ ID Generation   │ │
│  │ (DashMap) │    │ (Postgres)│    │ (deterministic) │ │
│  └───────────┘    └───────────┘    └─────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

**ID generation**: `hash(symbol + exchange) >> 32` for deterministic u32 IDs.

```rust
let registry = InstrumentRegistry::new(pool).await?;
let id = registry.get_or_create("BTCUSD", "BINANCE").await?;
```

## IPC Transport (transport/ipc/)

**Pattern**: Lock-free shared memory ring buffers with service registry

Ultra-low latency (~10µs) data distribution via POSIX shared memory.

### Components

- `SharedMemoryTransport`: Per-symbol ring buffer channels
- `SharedMemoryChannel`: Single symbol's shared memory segment
- `ControlChannel`: Dynamic subscription requests
- `Registry`: Multi-instance discovery

### Multi-Instance Support

```bash
# Terminal 1: Kraken
cargo run serve --live --provider kraken --symbols BTCUSD,ETHUSD --ipc --instance-id kraken-prod

# Terminal 2: Binance
cargo run serve --live --provider binance --symbols BNBUSD,SOLUSD --ipc --instance-id binance-prod
```

### Service Registry

- Location: `/data_manager_registry` (shared memory)
- 64 slots, ~28KB total
- Entry: instance_id, provider, exchange, channel_prefix, symbols, heartbeat
- Heartbeat: 5s interval, stale after 30s
- Auto-deregister on graceful shutdown

### IPC Paths

- Default: `/data_manager_BTCUSD_KRAKEN`
- With instance: `/data_manager_kraken-prod__BTCUSD_KRAKEN`

### Dynamic Subscription

```
trading-core                    data-manager
     │                               │
     ├── SUBSCRIBE BTCUSD ──────────►│
     │                               │ (creates IPC channel)
     │◄──────── SUCCESS ─────────────┤
     ├── (opens IPC channel) ────────┤
```

## Provider Routing (config/routing.rs, router.rs)

Auto-selects provider based on asset type and exchange.

**Resolution order**:
1. Symbol-specific routing (exact match)
2. Asset type + exchange fallback
3. Global default (`databento`)

**Configuration**:
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

## Adding a New Provider

1. Create `src/provider/your_provider/`:
   - `mod.rs` - Public exports
   - `client.rs` - Implement `DataProvider` + `LiveStreamProvider`
   - `types.rs` - WebSocket message structs
   - `normalizer.rs` - Implement `SymbolNormalizer`
   - `symbol.rs` - Symbol conversion functions

2. Register in `src/provider/mod.rs`
3. Add settings in `src/config/settings.rs`
4. Add dispatch in `src/cli/serve.rs`

## Kraken CSV Import (cli/import/kraken.rs, provider/kraken/csv_parser.rs)

Import historical trade data from Kraken Time & Sales ZIP archives.

**Features**:
- Parse Kraken T&S ZIP archives (1000+ symbols)
- Lee-Ready tick rule for side inference
- Symbol conversion: `XBTUSD` → `BTCUSD` (DBT canonical)
- Scientific notation support for small volumes
- Batch database inserts (10,000 per batch)
- Progress bar, date filtering, dry-run mode

**Lee-Ready Tick Rule** (side inference for historical data without side info):
```
Price > previous → Buy
Price < previous → Sell
Price == previous → Inherit previous side
First trade → Default to Buy
```

**TradeSide enum** (`trading-common/src/data/types.rs`):
```rust
pub enum TradeSide {
    Buy,
    Sell,
    #[default]
    Unknown,  // For historical data before side inference
}
```

**CSV Parser** (`provider/kraken/csv_parser.rs`):
- `CsvTradeIterator`: Streaming iterator over CSV lines
- `SideInferrer`: Stateful Lee-Ready side inference
- `parse_filename()`: Extract symbol from Kraken filenames (XBTUSD.csv → BTCUSD)
- `parse_decimal_with_scientific()`: Handle volumes like `7.314e-05`

## Key Files

- `provider/traits.rs` - Trait definitions
- `provider/binance/normalizer.rs` - Binance implementation
- `provider/kraken/normalizer.rs` - Kraken implementation
- `provider/kraken/csv_parser.rs` - Kraken T&S CSV parser with Lee-Ready inference
- `cli/import/kraken.rs` - Kraken import CLI command
- `cli/import/mod.rs` - Import subcommands (csv, kraken)
- `instruments/registry.rs` - InstrumentRegistry
- `transport/ipc/shared_memory.rs` - IPC transport
- `transport/ipc/registry.rs` - Service registry
- `config/router.rs` - ProviderRouter
