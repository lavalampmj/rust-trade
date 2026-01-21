# Data Manager Implementation Plan

## Overview

The Data Manager is a new standalone application responsible for:
1. **Historical Data Loading**: Import and store historical market data from various sources
2. **Real-time Data Collection**: Stream live market data continuously (24/7 operation)
3. **Data Distribution**: Serve data to consumers (strategies, backtesting engine) via efficient IPC

Target scale: **20,000 symbols** across multiple data providers.

---

## Architecture

### Crate Structure

```
rust-trade/
├── data-manager/              # NEW: Standalone binary
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # Entry point, CLI commands
│       ├── lib.rs             # Library exports
│       ├── config.rs          # Configuration management
│       │
│       ├── providers/         # Data provider adapters (pluggable)
│       │   ├── mod.rs
│       │   ├── traits.rs      # DataProvider trait
│       │   ├── databento/     # Databento implementation
│       │   │   ├── mod.rs
│       │   │   ├── historical.rs
│       │   │   ├── live.rs
│       │   │   └── types.rs   # DBN type mappings
│       │   └── binance/       # Future: Binance adapter
│       │
│       ├── storage/           # TimescaleDB integration
│       │   ├── mod.rs
│       │   ├── schema.rs      # Table definitions
│       │   ├── repository.rs  # CRUD operations
│       │   └── migrations/    # Schema migrations
│       │
│       ├── symbols/           # Symbol universe management
│       │   ├── mod.rs
│       │   ├── registry.rs    # Symbol registry (hybrid: config+DB+discovery)
│       │   └── discovery.rs   # Auto-discovery from providers
│       │
│       ├── realtime/          # Real-time data distribution
│       │   ├── mod.rs
│       │   ├── subscription.rs    # Subscription manager
│       │   ├── transport/         # Pluggable transport layer
│       │   │   ├── mod.rs
│       │   │   ├── traits.rs      # Transport trait
│       │   │   ├── shm.rs         # Shared memory (primary)
│       │   │   └── grpc.rs        # gRPC (future)
│       │   └── publisher.rs       # Pub/sub coordinator
│       │
│       └── backfill/          # Historical data backfill
│           ├── mod.rs
│           ├── scheduler.rs   # Scheduled bulk imports
│           └── ondemand.rs    # On-demand fetching
│
├── trading-common/            # EXISTING: Shared library
│   └── src/
│       └── data/
│           └── types.rs       # Add normalized TradeData type
│
└── data-manager-client/       # NEW: Client library for consumers
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── historical.rs      # Direct DB query helpers
        └── realtime.rs        # Subscription client (IPC)
```

### Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           DATA MANAGER                                   │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                     PROVIDER LAYER (Pluggable)                   │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │   │
│  │  │  Databento  │  │   Binance   │  │   Future    │              │   │
│  │  │  (Primary)  │  │  (Crypto)   │  │  Providers  │              │   │
│  │  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘              │   │
│  └─────────┼────────────────┼────────────────┼──────────────────────┘   │
│            │                │                │                          │
│            ▼                ▼                ▼                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                    NORMALIZATION LAYER                           │   │
│  │         Provider-specific → Common TradeData Schema              │   │
│  └──────────────────────────┬──────────────────────────────────────┘   │
│                             │                                          │
│            ┌────────────────┼────────────────┐                         │
│            ▼                ▼                ▼                         │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐             │
│  │   STORAGE    │  │   REAL-TIME  │  │  SYMBOL REGISTRY │             │
│  │  TimescaleDB │  │  PUBLISHER   │  │  (Hybrid Mgmt)   │             │
│  │              │  │              │  │                  │             │
│  │ • Trades     │  │ • Topic sub  │  │ • Config file    │             │
│  │ • Bars       │  │ • Filtered   │  │ • Database       │             │
│  │ • Metadata   │  │ • Snapshot+Δ │  │ • Discovery      │             │
│  └──────┬───────┘  └──────┬───────┘  └──────────────────┘             │
│         │                 │                                            │
└─────────┼─────────────────┼────────────────────────────────────────────┘
          │                 │
          ▼                 ▼
┌─────────────────┐  ┌─────────────────────────────────────┐
│  CONSUMERS      │  │         TRANSPORT LAYER              │
│  (Historical)   │  │  ┌─────────────┐  ┌─────────────┐   │
│                 │  │  │ Shared Mem  │  │    gRPC     │   │
│ Direct SQL      │  │  │  (Primary)  │  │  (Future)   │   │
│ queries to      │  │  │  <10ms      │  │  Distributed│   │
│ TimescaleDB     │  │  └─────────────┘  └─────────────┘   │
└─────────────────┘  └─────────────────────────────────────┘
```

---

## Data Provider Interface

### Trait Definition

```rust
// data-manager/src/providers/traits.rs

use async_trait::async_trait;
use tokio::sync::mpsc;

/// Normalized trade from any provider
#[derive(Debug, Clone)]
pub struct NormalizedTrade {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,              // Databento symbology (e.g., "ES.FUT")
    pub price: Decimal,
    pub quantity: Decimal,
    pub side: TradeSide,
    pub trade_id: String,
    pub provider: DataProvider,
    pub raw_symbol: String,          // Original provider symbol
    pub exchange: Option<String>,    // Exchange/venue
}

#[derive(Debug, Clone, Copy)]
pub enum DataProvider {
    Databento,
    Binance,
    // Future providers...
}

/// Historical data request parameters
pub struct HistoricalRequest {
    pub symbols: Vec<String>,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub schema: DataSchema,          // Trades, MBP, OHLCV, etc.
}

#[derive(Debug, Clone, Copy)]
pub enum DataSchema {
    Trades,
    Mbp1,      // Best bid/offer
    Mbp10,     // Top 10 levels
    Ohlcv1m,   // 1-minute bars
    Ohlcv1h,   // 1-hour bars
    // etc.
}

/// Subscription for live data
pub struct LiveSubscription {
    pub symbols: Vec<String>,
    pub schema: DataSchema,
}

#[async_trait]
pub trait HistoricalDataProvider: Send + Sync {
    /// Fetch historical data, streaming results to channel
    async fn fetch_historical(
        &self,
        request: HistoricalRequest,
        tx: mpsc::Sender<NormalizedTrade>,
    ) -> Result<(), ProviderError>;

    /// Get available symbols from this provider
    async fn discover_symbols(&self) -> Result<Vec<SymbolInfo>, ProviderError>;

    /// Get available date range for a symbol
    async fn get_available_range(
        &self,
        symbol: &str,
    ) -> Result<(DateTime<Utc>, DateTime<Utc>), ProviderError>;
}

#[async_trait]
pub trait LiveDataProvider: Send + Sync {
    /// Subscribe to live data stream
    async fn subscribe(
        &self,
        subscription: LiveSubscription,
        tx: mpsc::Sender<NormalizedTrade>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ProviderError>;

    /// Check connection health
    async fn health_check(&self) -> Result<ProviderHealth, ProviderError>;
}
```

### Databento Implementation

```rust
// data-manager/src/providers/databento/mod.rs

use databento::{HistoricalClient, LiveClient};
use dbn::{TradeMsg, Record};

pub struct DatabentoProvider {
    api_key: String,
    dataset: String,  // e.g., "GLBX.MDP3" for CME futures
}

impl DatabentoProvider {
    pub fn new(api_key: String, dataset: String) -> Self {
        Self { api_key, dataset }
    }

    /// Convert DBN TradeMsg to NormalizedTrade
    fn normalize_trade(&self, msg: &TradeMsg, symbol: &str) -> NormalizedTrade {
        NormalizedTrade {
            timestamp: Utc.timestamp_nanos(msg.ts_event as i64),
            symbol: symbol.to_string(),
            price: Decimal::from_i64(msg.price).unwrap() / Decimal::new(1_000_000_000, 0),
            quantity: Decimal::from_u32(msg.size).unwrap(),
            side: match msg.side {
                'A' => TradeSide::Sell,  // Ask side
                'B' => TradeSide::Buy,   // Bid side
                _ => TradeSide::Buy,     // Default
            },
            trade_id: format!("{}", msg.sequence),
            provider: DataProvider::Databento,
            raw_symbol: symbol.to_string(),
            exchange: Some(self.dataset.clone()),
        }
    }
}

#[async_trait]
impl HistoricalDataProvider for DatabentoProvider {
    async fn fetch_historical(
        &self,
        request: HistoricalRequest,
        tx: mpsc::Sender<NormalizedTrade>,
    ) -> Result<(), ProviderError> {
        let client = HistoricalClient::builder()
            .key(&self.api_key)?
            .build()?;

        let mut decoder = client.timeseries().get_range(
            &GetRangeParams::builder()
                .dataset(&self.dataset)
                .date_time_range((request.start, request.end))
                .symbols(&request.symbols)
                .schema(dbn::Schema::Trades)
                .build(),
        ).await?;

        while let Some(record) = decoder.decode_record::<TradeMsg>().await? {
            let symbol = decoder.symbol_for(record)?;
            let normalized = self.normalize_trade(&record, symbol);
            tx.send(normalized).await?;
        }

        Ok(())
    }

    // ... other methods
}

#[async_trait]
impl LiveDataProvider for DatabentoProvider {
    async fn subscribe(
        &self,
        subscription: LiveSubscription,
        tx: mpsc::Sender<NormalizedTrade>,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ProviderError> {
        let mut client = LiveClient::builder()
            .key(&self.api_key)?
            .dataset(&self.dataset)
            .build()
            .await?;

        client.subscribe(
            &SubscriptionBuilder::new()
                .symbols(&subscription.symbols)
                .schema(dbn::Schema::Trades)
                .build()
        ).await?;

        client.start().await?;

        loop {
            tokio::select! {
                record = client.next_record() => {
                    match record? {
                        Some(rec) => {
                            if let Some(trade) = rec.get::<TradeMsg>() {
                                let symbol = client.symbol_for(&trade)?;
                                let normalized = self.normalize_trade(&trade, symbol);
                                tx.send(normalized).await?;
                            }
                        }
                        None => break,
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("Shutting down Databento live stream");
                    break;
                }
            }
        }

        Ok(())
    }
}
```

---

## Storage Layer (TimescaleDB)

### Schema Design

```sql
-- data-manager/src/storage/migrations/001_initial_schema.sql

-- Enable TimescaleDB extension
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Normalized trades table (primary data store)
CREATE TABLE trades (
    time            TIMESTAMPTZ NOT NULL,
    symbol          TEXT NOT NULL,
    price           NUMERIC(20, 10) NOT NULL,
    quantity        NUMERIC(20, 10) NOT NULL,
    side            CHAR(1) NOT NULL,           -- 'B' or 'S'
    trade_id        TEXT NOT NULL,
    provider        TEXT NOT NULL,              -- 'databento', 'binance', etc.
    raw_symbol      TEXT NOT NULL,              -- Original provider symbol
    exchange        TEXT,

    -- Composite unique constraint
    UNIQUE (symbol, trade_id, time)
);

-- Convert to hypertable (TimescaleDB)
SELECT create_hypertable('trades', 'time',
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- Indexes for common query patterns
CREATE INDEX idx_trades_symbol_time ON trades (symbol, time DESC);
CREATE INDEX idx_trades_provider ON trades (provider, time DESC);

-- Enable compression for older chunks (after 7 days)
ALTER TABLE trades SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'symbol',
    timescaledb.compress_orderby = 'time DESC'
);

SELECT add_compression_policy('trades', INTERVAL '7 days');

-- Symbol registry table
CREATE TABLE symbols (
    id              SERIAL PRIMARY KEY,
    symbol          TEXT UNIQUE NOT NULL,       -- Normalized symbol
    provider        TEXT NOT NULL,
    raw_symbol      TEXT NOT NULL,              -- Provider's symbol
    asset_class     TEXT,                       -- 'equity', 'future', 'crypto', etc.
    exchange        TEXT,
    enabled         BOOLEAN DEFAULT TRUE,
    discovered_at   TIMESTAMPTZ DEFAULT NOW(),
    metadata        JSONB                       -- Provider-specific metadata
);

CREATE INDEX idx_symbols_enabled ON symbols (enabled) WHERE enabled = TRUE;

-- Data availability tracking
CREATE TABLE data_availability (
    symbol          TEXT NOT NULL,
    provider        TEXT NOT NULL,
    date            DATE NOT NULL,
    trade_count     BIGINT NOT NULL,
    first_trade     TIMESTAMPTZ,
    last_trade      TIMESTAMPTZ,

    PRIMARY KEY (symbol, provider, date)
);

-- Continuous aggregates for pre-computed OHLCV
CREATE MATERIALIZED VIEW ohlcv_1m
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 minute', time) AS bucket,
    symbol,
    first(price, time) AS open,
    max(price) AS high,
    min(price) AS low,
    last(price, time) AS close,
    sum(quantity) AS volume,
    count(*) AS trade_count
FROM trades
GROUP BY bucket, symbol
WITH NO DATA;

-- Refresh policy for continuous aggregate
SELECT add_continuous_aggregate_policy('ohlcv_1m',
    start_offset => INTERVAL '1 hour',
    end_offset => INTERVAL '1 minute',
    schedule_interval => INTERVAL '1 minute'
);

-- 1-hour OHLCV (aggregated from 1-minute)
CREATE MATERIALIZED VIEW ohlcv_1h
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', bucket) AS bucket,
    symbol,
    first(open, bucket) AS open,
    max(high) AS high,
    min(low) AS low,
    last(close, bucket) AS close,
    sum(volume) AS volume,
    sum(trade_count) AS trade_count
FROM ohlcv_1m
GROUP BY time_bucket('1 hour', bucket), symbol
WITH NO DATA;

SELECT add_continuous_aggregate_policy('ohlcv_1h',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour'
);
```

### Repository Implementation

```rust
// data-manager/src/storage/repository.rs

pub struct TradeRepository {
    pool: PgPool,
}

impl TradeRepository {
    /// Batch insert trades with ON CONFLICT handling
    pub async fn batch_insert(&self, trades: &[NormalizedTrade]) -> Result<usize, StorageError> {
        if trades.is_empty() {
            return Ok(0);
        }

        // Chunk into batches of 1000 for optimal performance
        let mut total_inserted = 0;
        for chunk in trades.chunks(1000) {
            let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
                "INSERT INTO trades (time, symbol, price, quantity, side, trade_id, provider, raw_symbol, exchange) "
            );

            query_builder.push_values(chunk, |mut b, trade| {
                b.push_bind(&trade.timestamp)
                    .push_bind(&trade.symbol)
                    .push_bind(&trade.price)
                    .push_bind(&trade.quantity)
                    .push_bind(trade.side.as_char())
                    .push_bind(&trade.trade_id)
                    .push_bind(trade.provider.as_str())
                    .push_bind(&trade.raw_symbol)
                    .push_bind(&trade.exchange);
            });

            query_builder.push(" ON CONFLICT (symbol, trade_id, time) DO NOTHING");

            let result = query_builder.build().execute(&self.pool).await?;
            total_inserted += result.rows_affected() as usize;
        }

        Ok(total_inserted)
    }

    /// Query trades for backtesting (chronological order)
    pub async fn get_trades_for_backtest(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<NormalizedTrade>, StorageError> {
        let trades = sqlx::query_as!(
            TradeRow,
            r#"
            SELECT time, symbol, price, quantity, side, trade_id, provider, raw_symbol, exchange
            FROM trades
            WHERE symbol = $1 AND time >= $2 AND time < $3
            ORDER BY time ASC
            "#,
            symbol, start, end
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(trades.into_iter().map(|r| r.into()).collect())
    }

    /// Get pre-computed OHLCV bars
    pub async fn get_ohlcv(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<OHLCVBar>, StorageError> {
        let view_name = match timeframe {
            Timeframe::OneMinute => "ohlcv_1m",
            Timeframe::OneHour => "ohlcv_1h",
            // Add more as needed
            _ => return Err(StorageError::UnsupportedTimeframe(timeframe)),
        };

        // Dynamic query based on timeframe view
        let query = format!(
            "SELECT bucket, symbol, open, high, low, close, volume, trade_count
             FROM {} WHERE symbol = $1 AND bucket >= $2 AND bucket < $3
             ORDER BY bucket ASC",
            view_name
        );

        let bars = sqlx::query_as::<_, OHLCVRow>(&query)
            .bind(symbol)
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await?;

        Ok(bars.into_iter().map(|r| r.into()).collect())
    }
}
```

---

## Real-time Distribution Layer

### Subscription Manager

```rust
// data-manager/src/realtime/subscription.rs

use std::collections::{HashMap, HashSet};
use tokio::sync::{RwLock, broadcast};

/// Subscription types supported
#[derive(Debug, Clone)]
pub enum SubscriptionType {
    /// Receive all trades for symbols
    TopicBased { symbols: Vec<String> },

    /// Receive trades matching filter criteria
    Filtered {
        symbols: Vec<String>,
        min_price_change_pct: Option<Decimal>,  // Only on X% move
        min_quantity: Option<Decimal>,           // Only trades > X size
        time_throttle_ms: Option<u64>,           // Max 1 per N ms
    },

    /// Initial snapshot + incremental updates
    SnapshotDelta {
        symbols: Vec<String>,
        snapshot_depth: usize,  // Number of recent trades in snapshot
    },
}

#[derive(Debug, Clone)]
pub struct Subscription {
    pub id: SubscriptionId,
    pub client_id: ClientId,
    pub sub_type: SubscriptionType,
    pub created_at: Instant,
}

pub struct SubscriptionManager {
    subscriptions: RwLock<HashMap<SubscriptionId, Subscription>>,
    symbol_subscribers: RwLock<HashMap<String, HashSet<SubscriptionId>>>,
    trade_broadcast: broadcast::Sender<NormalizedTrade>,
}

impl SubscriptionManager {
    pub async fn subscribe(
        &self,
        client_id: ClientId,
        sub_type: SubscriptionType,
    ) -> Result<SubscriptionId, SubscriptionError> {
        let sub_id = SubscriptionId::new();
        let symbols = sub_type.symbols();

        let subscription = Subscription {
            id: sub_id,
            client_id,
            sub_type,
            created_at: Instant::now(),
        };

        // Register subscription
        {
            let mut subs = self.subscriptions.write().await;
            subs.insert(sub_id, subscription);
        }

        // Map symbols to subscription
        {
            let mut symbol_subs = self.symbol_subscribers.write().await;
            for symbol in symbols {
                symbol_subs
                    .entry(symbol.clone())
                    .or_insert_with(HashSet::new)
                    .insert(sub_id);
            }
        }

        Ok(sub_id)
    }

    pub async fn unsubscribe(&self, sub_id: SubscriptionId) -> Result<(), SubscriptionError> {
        // Remove from subscriptions and symbol mappings
        // ...
    }

    /// Called when new trade arrives from provider
    pub async fn publish_trade(&self, trade: NormalizedTrade) {
        // Broadcast to all interested subscribers
        let _ = self.trade_broadcast.send(trade);
    }
}
```

### Transport Layer (Pluggable)

```rust
// data-manager/src/realtime/transport/traits.rs

#[async_trait]
pub trait TransportServer: Send + Sync {
    /// Start the transport server
    async fn start(&self, addr: SocketAddr) -> Result<(), TransportError>;

    /// Stop the transport server
    async fn stop(&self) -> Result<(), TransportError>;

    /// Get current connection count
    fn connection_count(&self) -> usize;
}

#[async_trait]
pub trait TransportClient: Send + Sync {
    /// Connect to server
    async fn connect(&mut self, addr: SocketAddr) -> Result<(), TransportError>;

    /// Subscribe to symbols
    async fn subscribe(&self, sub_type: SubscriptionType) -> Result<SubscriptionId, TransportError>;

    /// Receive next trade (blocking)
    async fn recv(&mut self) -> Result<Option<NormalizedTrade>, TransportError>;

    /// Disconnect
    async fn disconnect(&mut self) -> Result<(), TransportError>;
}
```

### Shared Memory Transport (Primary)

```rust
// data-manager/src/realtime/transport/shm.rs

use shared_memory::{Shmem, ShmemConf};
use crossbeam_channel::{bounded, Sender, Receiver};

/// Ring buffer structure in shared memory
#[repr(C)]
pub struct ShmRingBuffer {
    write_pos: AtomicU64,
    read_positions: [AtomicU64; MAX_CLIENTS],  // Per-client read positions
    capacity: u64,
    trade_size: u64,
    data: [u8],  // Flexible array member
}

pub struct ShmTransportServer {
    shmem: Shmem,
    ring_buffer: *mut ShmRingBuffer,
    subscription_manager: Arc<SubscriptionManager>,
}

impl ShmTransportServer {
    pub fn new(name: &str, capacity: usize) -> Result<Self, TransportError> {
        let trade_size = std::mem::size_of::<NormalizedTradeBinary>();
        let buffer_size = std::mem::size_of::<ShmRingBuffer>() + (capacity * trade_size);

        let shmem = ShmemConf::new()
            .size(buffer_size)
            .os_id(name)
            .create()?;

        // Initialize ring buffer header
        // ...

        Ok(Self { shmem, ring_buffer, subscription_manager })
    }

    /// Write trade to shared memory ring buffer
    pub fn publish(&self, trade: &NormalizedTrade) {
        unsafe {
            let buffer = &mut *self.ring_buffer;
            let write_pos = buffer.write_pos.load(Ordering::Acquire);
            let slot = (write_pos % buffer.capacity) as usize;

            // Serialize trade to binary format
            let trade_bytes = trade.to_binary();
            let offset = std::mem::size_of::<ShmRingBuffer>() + (slot * buffer.trade_size as usize);

            std::ptr::copy_nonoverlapping(
                trade_bytes.as_ptr(),
                (self.ring_buffer as *mut u8).add(offset),
                trade_bytes.len()
            );

            buffer.write_pos.store(write_pos + 1, Ordering::Release);
        }
    }
}

pub struct ShmTransportClient {
    shmem: Shmem,
    ring_buffer: *const ShmRingBuffer,
    client_id: usize,
    last_read_pos: u64,
}

impl ShmTransportClient {
    pub fn connect(name: &str, client_id: usize) -> Result<Self, TransportError> {
        let shmem = ShmemConf::new()
            .os_id(name)
            .open()?;

        let ring_buffer = shmem.as_ptr() as *const ShmRingBuffer;

        Ok(Self { shmem, ring_buffer, client_id, last_read_pos: 0 })
    }

    /// Read next trade from shared memory (non-blocking)
    pub fn try_recv(&mut self) -> Option<NormalizedTrade> {
        unsafe {
            let buffer = &*self.ring_buffer;
            let write_pos = buffer.write_pos.load(Ordering::Acquire);

            if self.last_read_pos >= write_pos {
                return None;  // No new data
            }

            let slot = (self.last_read_pos % buffer.capacity) as usize;
            let offset = std::mem::size_of::<ShmRingBuffer>() + (slot * buffer.trade_size as usize);

            let trade_ptr = (self.ring_buffer as *const u8).add(offset);
            let trade = NormalizedTrade::from_binary(trade_ptr);

            self.last_read_pos += 1;
            buffer.read_positions[self.client_id].store(self.last_read_pos, Ordering::Release);

            Some(trade)
        }
    }
}
```

---

## Symbol Management

### Hybrid Registry

```rust
// data-manager/src/symbols/registry.rs

pub struct SymbolRegistry {
    // Static config (base symbols)
    config_symbols: HashSet<String>,

    // Database-backed (dynamic additions/removals)
    db_pool: PgPool,

    // In-memory cache of active symbols
    active_symbols: RwLock<HashMap<String, SymbolInfo>>,

    // Provider discovery clients
    providers: Vec<Arc<dyn SymbolDiscovery>>,
}

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub symbol: String,
    pub provider: DataProvider,
    pub raw_symbol: String,
    pub asset_class: AssetClass,
    pub exchange: Option<String>,
    pub enabled: bool,
    pub metadata: serde_json::Value,
}

impl SymbolRegistry {
    /// Load symbols from all sources
    pub async fn initialize(&self) -> Result<(), RegistryError> {
        // 1. Load from config file
        for symbol in &self.config_symbols {
            self.add_symbol(symbol.clone(), SymbolSource::Config).await?;
        }

        // 2. Load from database (overrides/additions)
        let db_symbols = self.load_from_database().await?;
        for symbol in db_symbols {
            self.active_symbols.write().await.insert(symbol.symbol.clone(), symbol);
        }

        // 3. Optionally run discovery
        // self.run_discovery().await?;

        Ok(())
    }

    /// Discover symbols from providers
    pub async fn run_discovery(&self) -> Result<Vec<SymbolInfo>, RegistryError> {
        let mut discovered = Vec::new();

        for provider in &self.providers {
            match provider.discover_symbols().await {
                Ok(symbols) => {
                    for symbol in symbols {
                        // Only add if not already tracked
                        if !self.active_symbols.read().await.contains_key(&symbol.symbol) {
                            self.add_symbol_to_db(&symbol).await?;
                            discovered.push(symbol);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Discovery failed for provider: {}", e);
                }
            }
        }

        Ok(discovered)
    }

    /// Get all enabled symbols for a provider
    pub async fn get_symbols_for_provider(&self, provider: DataProvider) -> Vec<SymbolInfo> {
        self.active_symbols
            .read()
            .await
            .values()
            .filter(|s| s.provider == provider && s.enabled)
            .cloned()
            .collect()
    }
}
```

---

## CLI Interface

```rust
// data-manager/src/main.rs

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "data-manager")]
#[command(about = "Market data management service")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the data manager service (live collection + distribution)
    Start {
        #[arg(short, long, default_value = "config/data-manager.toml")]
        config: PathBuf,
    },

    /// Backfill historical data
    Backfill {
        #[arg(short, long)]
        provider: String,

        #[arg(short, long)]
        symbols: Vec<String>,

        #[arg(long)]
        start: String,  // ISO 8601 date

        #[arg(long)]
        end: String,
    },

    /// Manage symbol universe
    Symbols {
        #[command(subcommand)]
        action: SymbolAction,
    },

    /// Database migrations
    Migrate {
        #[arg(long)]
        up: bool,

        #[arg(long)]
        down: bool,
    },

    /// Health check
    Status,
}

#[derive(Subcommand)]
enum SymbolAction {
    /// List all tracked symbols
    List {
        #[arg(short, long)]
        provider: Option<String>,
    },

    /// Add symbol to tracking
    Add {
        symbol: String,
        #[arg(short, long)]
        provider: String,
    },

    /// Remove symbol from tracking
    Remove { symbol: String },

    /// Run auto-discovery
    Discover {
        #[arg(short, long)]
        provider: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { config } => {
            let config = Config::load(&config)?;
            let service = DataManagerService::new(config).await?;
            service.run().await?;
        }

        Commands::Backfill { provider, symbols, start, end } => {
            // Run backfill job
        }

        Commands::Symbols { action } => {
            // Handle symbol management
        }

        Commands::Migrate { up, down } => {
            // Run migrations
        }

        Commands::Status => {
            // Health check
        }
    }

    Ok(())
}
```

---

## Configuration

```toml
# config/data-manager.toml

[service]
name = "data-manager"
log_level = "info"

[database]
url = "postgresql://user:pass@localhost/trading_data"
max_connections = 20
min_connections = 5

[providers.databento]
enabled = true
api_key_env = "DATABENTO_API_KEY"  # Read from environment
datasets = ["GLBX.MDP3", "XNAS.ITCH"]  # CME, NASDAQ

[providers.binance]
enabled = false
# Future configuration

[symbols]
# Static symbol list (base configuration)
config_file = "config/symbols.toml"

# Enable database overrides
database_enabled = true

# Enable auto-discovery
discovery_enabled = true
discovery_interval_hours = 24

[realtime]
# Transport configuration
transport = "shm"  # "shm" or "grpc"

[realtime.shm]
name = "data_manager_trades"
ring_buffer_capacity = 1_000_000  # 1M trades

[realtime.grpc]
bind_address = "0.0.0.0:50051"

[backfill]
# Scheduled backfill
enabled = true
schedule = "0 2 * * *"  # 2 AM daily

# On-demand backfill settings
max_concurrent_requests = 5
chunk_size_days = 7

[retention]
# Keep tick data forever (no auto-deletion)
trades_retention_days = null

# Compression settings (TimescaleDB)
compress_after_days = 7
```

---

## Implementation Phases

### Phase 1: Foundation (Week 1-2)
- [ ] Create `data-manager` crate structure
- [ ] Implement TimescaleDB schema and migrations
- [ ] Implement `TradeRepository` with batch insert
- [ ] Basic CLI scaffolding

### Phase 2: Databento Integration (Week 2-3)
- [ ] Implement `DatabentoProvider` for historical data
- [ ] Implement `DatabentoProvider` for live streaming
- [ ] Trade normalization (DBN → NormalizedTrade)
- [ ] Historical backfill command

### Phase 3: Symbol Management (Week 3-4)
- [ ] Implement `SymbolRegistry` with config loading
- [ ] Database-backed symbol storage
- [ ] Discovery integration with Databento

### Phase 4: Real-time Distribution (Week 4-5)
- [ ] Implement `SubscriptionManager`
- [ ] Shared memory transport (ring buffer)
- [ ] Client library (`data-manager-client`)

### Phase 5: Integration & Testing (Week 5-6)
- [ ] Integration tests with mock providers
- [ ] Performance benchmarks (latency, throughput)
- [ ] Documentation
- [ ] Integration with existing `trading-core` strategies

---

## Dependencies

```toml
# data-manager/Cargo.toml

[package]
name = "data-manager"
version = "0.1.0"
edition = "2021"

[dependencies]
# Databento SDK
databento = "0.15"
dbn = "0.22"

# Async runtime
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"

# Database
sqlx = { version = "0.7", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "rust_decimal"] }

# Shared memory IPC
shared_memory = "0.12"

# Configuration
config = "0.13"
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"

# CLI
clap = { version = "4.4", features = ["derive"] }

# Shared types from trading-common
trading-common = { path = "../trading-common" }

# Utilities
rust_decimal = "1.32"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = "0.3"
thiserror = "1.0"
anyhow = "1.0"

[dev-dependencies]
tokio-test = "0.4"
mockall = "0.12"
```

---

## Open Questions

1. **Databento Symbology**: Should we adopt Databento's symbology as the canonical format, or create our own mapping layer?

2. **Multi-provider Symbol Conflicts**: How to handle when same instrument exists on multiple providers (e.g., BTCUSD on Binance vs BTC futures on CME)?

3. **Backpressure Handling**: When shared memory ring buffer is full and slow consumers haven't caught up, should we drop oldest data or block?

4. **Cold Start**: When Data Manager restarts, should it replay recent data from DB to shared memory for late-joining consumers?

5. **High Availability**: For 24/7 operation, what's the failover strategy? Active-passive with shared TimescaleDB?
