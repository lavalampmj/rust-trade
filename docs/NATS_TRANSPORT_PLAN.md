# Implementation Plan: NATS/JetStream Transport for Market Data Distribution

## Overview

Add NATS/JetStream as a remote transport option alongside the existing IPC (shared memory) transport. This enables multi-tenant scenarios where multiple trading-core instances consume market data from a centralized data-manager over the network.

### Transport Selection Strategy

| Scenario | Transport | Latency | Use Case |
|----------|-----------|---------|----------|
| Same machine | IPC (shared memory) | ~1-5µs | Single-tenant, lowest latency |
| Remote/distributed | NATS/JetStream | ~50-200µs | Multi-tenant, scalable |
| Hybrid | Both | Mixed | Local + remote consumers |

### Subject Hierarchy

```
market.{exchange}.{symbol}.trades    # Individual symbol trades
market.{exchange}.*.trades           # All symbols on exchange (wildcard)
market.*.{symbol}.trades             # Same symbol across exchanges
```

Examples:
- `market.binance.BTCUSDT.trades`
- `market.binance.ETHUSDT.trades`
- `market.binance.*.trades` (subscribe to all Binance symbols)

---

## Phase 1: Dependencies & Core Types

### 1.1 Add Dependencies

**data-manager/Cargo.toml:**
```toml
[dependencies]
async-nats = "0.35"          # NATS client with JetStream support
```

**trading-core/Cargo.toml:**
```toml
[dependencies]
async-nats = "0.35"
```

### 1.2 Shared Types (trading-common)

**File: `trading-common/src/transport/mod.rs`** (NEW)

```rust
pub mod nats_config;

pub use nats_config::{NatsConfig, JetStreamConfig, ConsumerConfig};
```

**File: `trading-common/src/transport/nats_config.rs`** (NEW)

```rust
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    /// NATS server URLs (e.g., "nats://localhost:4222")
    pub servers: Vec<String>,

    /// Optional authentication
    pub auth: Option<NatsAuth>,

    /// Connection name for monitoring
    pub name: Option<String>,

    /// JetStream configuration
    pub jetstream: JetStreamConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NatsAuth {
    Token(String),
    UserPassword { user: String, password: String },
    NKey { seed: String },
    Jwt { jwt: String, seed: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JetStreamConfig {
    /// Stream name for market data
    pub stream_name: String,  // default: "MARKET_DATA"

    /// Subject prefix
    pub subject_prefix: String,  // default: "market"

    /// Retention policy
    pub retention: RetentionPolicy,

    /// Max age of messages
    #[serde(with = "humantime_serde")]
    pub max_age: Duration,  // default: 1 hour

    /// Max bytes per subject
    pub max_bytes_per_subject: i64,  // default: 100MB

    /// Number of replicas (for HA)
    pub replicas: usize,  // default: 1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RetentionPolicy {
    Limits,      // Delete oldest when limits reached
    Interest,    // Delete when no consumers interested
    WorkQueue,   // Delete after acknowledged (not for this use case)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumerConfig {
    /// Durable consumer name (unique per trading-core instance)
    pub durable_name: String,

    /// Symbols to subscribe to
    pub symbols: Vec<String>,

    /// Exchange filter (None = all exchanges)
    pub exchange: Option<String>,

    /// Replay policy on reconnect
    pub replay_policy: ReplayPolicy,

    /// Max pending messages before ack required
    pub max_ack_pending: i64,  // default: 1000

    /// Ack wait timeout
    #[serde(with = "humantime_serde")]
    pub ack_wait: Duration,  // default: 30s
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplayPolicy {
    Instant,     // Replay as fast as possible
    Original,    // Replay at original rate
    ByStartTime(DateTime<Utc>),  // Replay from specific time
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            servers: vec!["nats://localhost:4222".to_string()],
            auth: None,
            name: Some("data-manager".to_string()),
            jetstream: JetStreamConfig::default(),
        }
    }
}

impl Default for JetStreamConfig {
    fn default() -> Self {
        Self {
            stream_name: "MARKET_DATA".to_string(),
            subject_prefix: "market".to_string(),
            retention: RetentionPolicy::Limits,
            max_age: Duration::from_secs(3600),  // 1 hour
            max_bytes_per_subject: 100 * 1024 * 1024,  // 100MB
            replicas: 1,
        }
    }
}
```

---

## Phase 2: Data-Manager Publisher

### 2.1 NATS Transport Implementation

**File: `data-manager/src/transport/nats/mod.rs`** (REPLACE placeholder)

```rust
//! NATS/JetStream transport for market data distribution
//!
//! Provides pub/sub-based data distribution for remote consumers.
//! Supports fan-out to multiple trading-core instances.

mod publisher;
mod stream_manager;

pub use publisher::NatsPublisher;
pub use stream_manager::StreamManager;

use crate::transport::traits::{Transport, TransportStats};
use async_nats::jetstream::{self, Context};
use trading_common::transport::NatsConfig;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use anyhow::Result;

pub struct NatsTransport {
    jetstream: Context,
    config: NatsConfig,
    stats: Arc<NatsTransportStats>,
}

struct NatsTransportStats {
    messages_sent: AtomicU64,
    bytes_sent: AtomicU64,
    publish_errors: AtomicU64,
}

impl NatsTransport {
    pub async fn connect(config: NatsConfig) -> Result<Self> {
        // Build connection options
        let mut options = async_nats::ConnectOptions::new();

        if let Some(name) = &config.name {
            options = options.name(name);
        }

        if let Some(auth) = &config.auth {
            options = match auth {
                NatsAuth::Token(token) => options.token(token.clone()),
                NatsAuth::UserPassword { user, password } => {
                    options.user_and_password(user.clone(), password.clone())
                }
                // ... other auth methods
            };
        }

        // Connect to NATS
        let client = options
            .connect(config.servers.join(","))
            .await?;

        // Create JetStream context
        let jetstream = jetstream::new(client);

        // Ensure stream exists
        Self::ensure_stream(&jetstream, &config.jetstream).await?;

        Ok(Self {
            jetstream,
            config,
            stats: Arc::new(NatsTransportStats {
                messages_sent: AtomicU64::new(0),
                bytes_sent: AtomicU64::new(0),
                publish_errors: AtomicU64::new(0),
            }),
        })
    }

    async fn ensure_stream(js: &Context, config: &JetStreamConfig) -> Result<()> {
        use async_nats::jetstream::stream::{Config, RetentionPolicy};

        let stream_config = Config {
            name: config.stream_name.clone(),
            subjects: vec![format!("{}.>", config.subject_prefix)],
            retention: match config.retention {
                RetentionPolicy::Limits => RetentionPolicy::Limits,
                RetentionPolicy::Interest => RetentionPolicy::Interest,
                _ => RetentionPolicy::Limits,
            },
            max_age: config.max_age,
            max_bytes_per_subject: config.max_bytes_per_subject,
            num_replicas: config.replicas,
            ..Default::default()
        };

        // Create or update stream
        js.get_or_create_stream(stream_config).await?;

        Ok(())
    }

    fn build_subject(&self, symbol: &str, exchange: &str) -> String {
        format!(
            "{}.{}.{}.trades",
            self.config.jetstream.subject_prefix,
            exchange.to_lowercase(),
            symbol.to_uppercase()
        )
    }
}

#[async_trait::async_trait]
impl Transport for NatsTransport {
    async fn send_msg(&self, msg: &TradeMsg, symbol: &str, exchange: &str) -> Result<()> {
        let subject = self.build_subject(symbol, exchange);
        let payload = msg.as_bytes();  // TradeMsg is 48 bytes, already packed

        match self.jetstream.publish(subject, payload.into()).await {
            Ok(ack) => {
                // Wait for ack to ensure durability
                ack.await?;
                self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
                self.stats.bytes_sent.fetch_add(payload.len() as u64, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                self.stats.publish_errors.fetch_add(1, Ordering::Relaxed);
                Err(e.into())
            }
        }
    }

    async fn send_batch(&self, msgs: &[TradeMsg], symbol: &str, exchange: &str) -> Result<()> {
        let subject = self.build_subject(symbol, exchange);

        // Batch publish with single ack
        for msg in msgs {
            let payload = msg.as_bytes();
            self.jetstream.publish(subject.clone(), payload.into()).await?;
        }

        self.stats.messages_sent.fetch_add(msgs.len() as u64, Ordering::Relaxed);
        Ok(())
    }

    fn is_ready(&self) -> bool {
        // NATS client handles reconnection automatically
        true
    }

    fn stats(&self) -> TransportStats {
        TransportStats {
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            bytes_sent: self.stats.bytes_sent.load(Ordering::Relaxed),
            send_errors: self.stats.publish_errors.load(Ordering::Relaxed),
            ..Default::default()
        }
    }
}
```

### 2.2 Update Transport Module

**File: `data-manager/src/transport/mod.rs`** (MODIFY)

Add NATS module export:
```rust
pub mod ipc;
pub mod grpc;
pub mod nats;  // ADD
pub mod traits;

pub use ipc::{SharedMemoryTransport, SharedMemoryConfig};
pub use nats::NatsTransport;  // ADD
pub use traits::{Transport, Consumer, TransportStats};
```

### 2.3 Update Configuration

**File: `data-manager/src/config/settings.rs`** (MODIFY)

```rust
use trading_common::transport::NatsConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportSettings {
    pub ipc: IpcSettings,
    pub grpc: Option<GrpcSettings>,
    pub nats: Option<NatsConfig>,  // ADD
}

impl Default for TransportSettings {
    fn default() -> Self {
        Self {
            ipc: IpcSettings::default(),
            grpc: None,
            nats: None,  // Disabled by default
        }
    }
}
```

### 2.4 Update Serve Command

**File: `data-manager/src/cli/serve.rs`** (MODIFY)

Add NATS transport initialization and publishing:

```rust
// In run_serve() function:

// Initialize transports
let ipc_transport = if config.transport.ipc.enabled {
    Some(SharedMemoryTransport::new(config.transport.ipc.clone().into())?)
} else {
    None
};

let nats_transport = if let Some(nats_config) = &config.transport.nats {
    Some(NatsTransport::connect(nats_config.clone()).await?)
} else {
    None
};

// In the streaming callback:
let tick_callback = {
    let ipc = ipc_transport.clone();
    let nats = nats_transport.clone();

    move |tick: TickData| {
        // Publish to IPC (local consumers)
        if let Some(ref transport) = ipc {
            let _ = publish_to_ipc(transport, &tick);
        }

        // Publish to NATS (remote consumers)
        if let Some(ref transport) = nats {
            let msg = tick.to_trade_msg();
            tokio::spawn(async move {
                if let Err(e) = transport.send_msg(&msg, &tick.symbol, &tick.exchange).await {
                    warn!("NATS publish error: {}", e);
                }
            });
        }
    }
};
```

---

## Phase 3: Trading-Core Consumer

### 3.1 NATS Exchange Implementation

**File: `trading-core/src/data_source/nats_exchange.rs`** (NEW)

```rust
//! NATS/JetStream consumer for receiving market data from remote data-manager
//!
//! Implements the Exchange trait for seamless integration with existing
//! data processing pipeline.

use crate::exchange::{Exchange, ExchangeError};
use async_nats::jetstream::{self, consumer::PullConsumer, stream::Stream};
use trading_common::data::types::TickData;
use trading_common::transport::{ConsumerConfig, NatsConfig};
use tokio::sync::broadcast;
use std::sync::Arc;
use anyhow::Result;

pub struct NatsExchange {
    config: NatsConfig,
    consumer_config: ConsumerConfig,
    stream: Option<Stream>,
    consumer: Option<PullConsumer>,
}

impl NatsExchange {
    pub fn new(config: NatsConfig, consumer_config: ConsumerConfig) -> Self {
        Self {
            config,
            consumer_config,
            stream: None,
            consumer: None,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        // Connect to NATS
        let client = async_nats::connect(&self.config.servers.join(",")).await?;
        let jetstream = jetstream::new(client);

        // Get stream
        let stream = jetstream
            .get_stream(&self.config.jetstream.stream_name)
            .await?;

        // Build filter subjects based on subscribed symbols
        let filter_subjects: Vec<String> = self.consumer_config.symbols
            .iter()
            .map(|symbol| {
                let exchange = self.consumer_config.exchange
                    .as_deref()
                    .unwrap_or("*");
                format!(
                    "{}.{}.{}.trades",
                    self.config.jetstream.subject_prefix,
                    exchange.to_lowercase(),
                    symbol.to_uppercase()
                )
            })
            .collect();

        // Create or get durable consumer
        let consumer_config = jetstream::consumer::pull::Config {
            durable_name: Some(self.consumer_config.durable_name.clone()),
            filter_subjects,
            max_ack_pending: self.consumer_config.max_ack_pending,
            ack_wait: self.consumer_config.ack_wait,
            deliver_policy: match &self.consumer_config.replay_policy {
                ReplayPolicy::Instant => DeliverPolicy::All,
                ReplayPolicy::ByStartTime(ts) => DeliverPolicy::ByStartTime {
                    start_time: *ts
                },
                _ => DeliverPolicy::New,
            },
            ..Default::default()
        };

        let consumer = stream
            .get_or_create_consumer(&self.consumer_config.durable_name, consumer_config)
            .await?;

        self.stream = Some(stream);
        self.consumer = Some(consumer);

        Ok(())
    }
}

#[async_trait::async_trait]
impl Exchange for NatsExchange {
    async fn subscribe_trades(
        &self,
        _symbols: Vec<String>,
        callback: Arc<dyn Fn(TickData) + Send + Sync>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ExchangeError> {
        let consumer = self.consumer.as_ref()
            .ok_or_else(|| ExchangeError::NotConnected)?;

        let mut messages = consumer.messages().await
            .map_err(|e| ExchangeError::SubscriptionFailed(e.to_string()))?;

        let mut shutdown_rx = shutdown_rx;

        loop {
            tokio::select! {
                // Check for shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("NATS consumer shutting down");
                    break;
                }

                // Process next message
                msg_result = messages.next() => {
                    match msg_result {
                        Some(Ok(msg)) => {
                            // Parse TradeMsg from payload
                            if let Ok(trade_msg) = TradeMsg::from_bytes(&msg.payload) {
                                let tick = TickData::from(trade_msg);
                                callback(tick);
                            }

                            // Acknowledge message
                            if let Err(e) = msg.ack().await {
                                warn!("Failed to ack message: {}", e);
                            }
                        }
                        Some(Err(e)) => {
                            warn!("Error receiving message: {}", e);
                        }
                        None => {
                            // Stream ended
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "NATS"
    }
}
```

### 3.2 Update Data Source Module

**File: `trading-core/src/data_source/mod.rs`** (MODIFY)

```rust
pub mod ipc;
pub mod nats;  // ADD

pub use ipc::IpcExchange;
pub use nats::NatsExchange;  // ADD
```

### 3.3 Update Configuration

**File: `trading-core/src/config.rs`** (MODIFY)

```rust
use trading_common::transport::{NatsConfig, ConsumerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceConfig {
    /// Use IPC transport (local data-manager)
    pub ipc: Option<IpcConfig>,

    /// Use NATS transport (remote data-manager)
    pub nats: Option<NatsSourceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsSourceConfig {
    /// NATS connection config
    pub connection: NatsConfig,

    /// Consumer config (durable name, symbols, etc.)
    pub consumer: ConsumerConfig,
}
```

### 3.4 Update Live Command

**File: `trading-core/src/cli/live.rs`** (MODIFY)

```rust
// Select data source based on configuration
let exchange: Box<dyn Exchange> = match &config.data_source {
    DataSourceConfig { ipc: Some(ipc_config), .. } => {
        info!("Using IPC transport (local data-manager)");
        Box::new(IpcExchange::new(ipc_config.clone()))
    }
    DataSourceConfig { nats: Some(nats_config), .. } => {
        info!("Using NATS transport (remote data-manager)");
        let mut exchange = NatsExchange::new(
            nats_config.connection.clone(),
            nats_config.consumer.clone(),
        );
        exchange.connect().await?;
        Box::new(exchange)
    }
    _ => {
        return Err(anyhow!("No data source configured"));
    }
};
```

---

## Phase 4: Configuration Files

### 4.1 Data-Manager Config

**File: `config/development.toml`** (MODIFY)

```toml
[transport.nats]
servers = ["nats://localhost:4222"]
name = "data-manager-dev"

[transport.nats.jetstream]
stream_name = "MARKET_DATA"
subject_prefix = "market"
max_age = "1h"
max_bytes_per_subject = 104857600  # 100MB
replicas = 1
```

### 4.2 Trading-Core Config

**File: `trading-core/config/development.toml`** (MODIFY)

```toml
# Option 1: Local IPC (default, fastest)
[data_source.ipc]
shm_path_prefix = "/data_manager_"

# Option 2: Remote NATS (comment out IPC, uncomment this)
# [data_source.nats.connection]
# servers = ["nats://data-manager.internal:4222"]
# name = "trading-core-1"
#
# [data_source.nats.consumer]
# durable_name = "trading-core-1"
# symbols = ["BTCUSDT", "ETHUSDT"]
# exchange = "binance"
# max_ack_pending = 1000
# ack_wait = "30s"
```

---

## Phase 5: Docker & Infrastructure

### 5.1 Docker Compose for Development

**File: `docker-compose.nats.yml`** (NEW)

```yaml
version: '3.8'

services:
  nats:
    image: nats:2.10-alpine
    ports:
      - "4222:4222"   # Client connections
      - "8222:8222"   # HTTP monitoring
    command: >
      -js
      -sd /data
      -m 8222
    volumes:
      - nats-data:/data
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:8222/healthz"]
      interval: 5s
      timeout: 3s
      retries: 3

volumes:
  nats-data:
```

### 5.2 NATS Cluster for Production

**File: `docker-compose.nats-cluster.yml`** (NEW)

```yaml
version: '3.8'

services:
  nats-1:
    image: nats:2.10-alpine
    command: >
      -js
      -sd /data
      -cluster nats://0.0.0.0:6222
      -cluster_name nats-cluster
      -routes nats://nats-2:6222,nats://nats-3:6222
    volumes:
      - nats-1-data:/data

  nats-2:
    image: nats:2.10-alpine
    command: >
      -js
      -sd /data
      -cluster nats://0.0.0.0:6222
      -cluster_name nats-cluster
      -routes nats://nats-1:6222,nats://nats-3:6222
    volumes:
      - nats-2-data:/data

  nats-3:
    image: nats:2.10-alpine
    command: >
      -js
      -sd /data
      -cluster nats://0.0.0.0:6222
      -cluster_name nats-cluster
      -routes nats://nats-1:6222,nats://nats-2:6222
    volumes:
      - nats-3-data:/data

volumes:
  nats-1-data:
  nats-2-data:
  nats-3-data:
```

---

## Phase 6: Testing

### 6.1 Unit Tests

**File: `data-manager/src/transport/nats/tests.rs`** (NEW)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_subject_building() {
        let config = NatsConfig::default();
        let transport = NatsTransport::connect(config).await.unwrap();

        assert_eq!(
            transport.build_subject("BTCUSDT", "BINANCE"),
            "market.binance.BTCUSDT.trades"
        );
    }

    #[tokio::test]
    #[ignore]  // Requires running NATS server
    async fn test_publish_and_consume() {
        // Integration test with real NATS
    }
}
```

### 6.2 Integration Tests

**File: `tests/nats_integration_test.rs`** (NEW)

```rust
//! Integration tests for NATS transport
//!
//! Requires: docker-compose -f docker-compose.nats.yml up -d

#[tokio::test]
#[ignore]
async fn test_end_to_end_nats_flow() {
    // 1. Start NATS publisher (simulating data-manager)
    // 2. Start NATS consumer (simulating trading-core)
    // 3. Publish test messages
    // 4. Verify consumer receives all messages in order
    // 5. Verify replay works after consumer restart
}

#[tokio::test]
#[ignore]
async fn test_multiple_consumers_same_stream() {
    // Verify fan-out to multiple trading-core instances
}

#[tokio::test]
#[ignore]
async fn test_consumer_reconnection_replay() {
    // Verify message replay after consumer disconnect/reconnect
}
```

---

## Files Summary

### New Files

| File | Purpose |
|------|---------|
| `trading-common/src/transport/mod.rs` | Transport module root |
| `trading-common/src/transport/nats_config.rs` | NATS/JetStream configuration types |
| `data-manager/src/transport/nats/mod.rs` | NATS publisher implementation |
| `trading-core/src/data_source/nats.rs` | NATS consumer (Exchange impl) |
| `docker-compose.nats.yml` | Development NATS server |
| `docker-compose.nats-cluster.yml` | Production NATS cluster |
| `tests/nats_integration_test.rs` | Integration tests |

### Modified Files

| File | Changes |
|------|---------|
| `data-manager/Cargo.toml` | Add `async-nats` dependency |
| `trading-core/Cargo.toml` | Add `async-nats` dependency |
| `trading-common/src/lib.rs` | Export transport module |
| `data-manager/src/transport/mod.rs` | Export NATS module |
| `data-manager/src/config/settings.rs` | Add NATS config |
| `data-manager/src/cli/serve.rs` | Add NATS publishing |
| `trading-core/src/data_source/mod.rs` | Export NATS exchange |
| `trading-core/src/config.rs` | Add NATS config |
| `trading-core/src/cli/live.rs` | Add NATS source selection |
| `config/development.toml` | Add NATS section |

---

## Verification

### Manual Testing

```bash
# 1. Start NATS server
docker-compose -f docker-compose.nats.yml up -d

# 2. Verify NATS is running
curl http://localhost:8222/healthz

# 3. Start data-manager with NATS enabled
cd data-manager
cargo run serve --live --ipc --nats --symbols BTCUSDT,ETHUSDT

# 4. Start trading-core with NATS consumer
cd trading-core
cargo run live --nats --symbols BTCUSDT

# 5. Verify data flow
# - Check data-manager logs for "Published to NATS"
# - Check trading-core logs for received ticks
# - Monitor NATS: curl http://localhost:8222/jsz

# 6. Test replay (restart trading-core, verify it catches up)
```

### Automated Tests

```bash
# Unit tests
cargo test --package data-manager nats
cargo test --package trading-core nats

# Integration tests (requires NATS running)
docker-compose -f docker-compose.nats.yml up -d
cargo test --test nats_integration_test -- --ignored
```

---

## Future Enhancements (Out of Scope)

1. **Authentication**: Add NATS credentials/NKey for production
2. **TLS**: Encrypt NATS connections
3. **Metrics**: Expose NATS metrics to Prometheus
4. **Multi-stream**: Separate streams per exchange for isolation
5. **Key-value store**: Use NATS KV for symbol registry
6. **Object store**: Use NATS Object Store for historical data distribution
