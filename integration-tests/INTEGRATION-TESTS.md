# Integration Tests Crate - Complete Code Explanation

## Overview

The `integration-tests` crate is an **end-to-end test harness** for stress testing the trading data pipeline. It simulates the full data flow:

```
Data Emulator → data-manager (WebSocket) → IPC → trading-core → strategies
```

---

## Architecture Diagram

```
┌────────────────────────────────────────────────────────────────────────────┐
│                        INTEGRATION TEST HARNESS                            │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌──────────────────────┐         ┌────────────────────────────────────┐  │
│  │  TEST DATA GENERATOR │         │     METRICS COLLECTOR              │  │
│  │  (generator.rs)      │         │     (metrics.rs)                   │  │
│  │  - Deterministic     │         │     - Tick counts                  │  │
│  │  - Volume profiles   │         │     - Latency (Welford's algo)     │  │
│  │  - DBN format        │         │     - Validation                   │  │
│  └──────────┬───────────┘         └────────────────────────────────────┘  │
│             │ Vec<TradeMsg>                                               │
│             ▼                                                              │
│  ┌──────────────────────┐                                                 │
│  │  TEST DATA EMULATOR  │                                                 │
│  │  (emulator.rs)       │                                                 │
│  │  - LiveStreamProvider│                                                 │
│  │  - Timestamp rewrite │                                                 │
│  │  - Latency embedding │                                                 │
│  └──────────┬───────────┘                                                 │
│             │ StreamEvent::Tick                                           │
│             ▼                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐ │
│  │                    TRANSPORT LAYER (transport/)                       │ │
│  │                                                                       │ │
│  │  WebSocket Mode (default):       Direct Mode:                         │ │
│  │  ┌──────────────────────────────┐ ┌────────────────────┐             │ │
│  │  │  WebSocketTransport          │ │  DirectTransport   │             │ │
│  │  │  - Network-based (~90μs)     │ │  - In-memory (~12μs)│             │ │
│  │  │  - MessagePack serialization │ │  - No serialization │             │ │
│  │  │  - Server + Client           │ └────────────────────┘             │ │
│  │  │  - Optional batching         │                                    │ │
│  │  └──────────────────────────────┘                                    │ │
│  └──────────────────────────────────────────────────────────────────────┘ │
│             │ callback(StreamEvent::Tick)                                 │
│             ▼                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐ │
│  │                    STRATEGY RUNNERS (runner/)                         │ │
│  │  ┌────────────────────┐     ┌──────────────────────────┐             │ │
│  │  │  RustStrategyRunner│     │  PythonStrategyRunner    │             │ │
│  │  │  - N concurrent    │     │  - N concurrent (stub)   │             │ │
│  │  │  - Tick counting   │     │  - Tick counting         │             │ │
│  │  │  - Latency capture │     │  - Latency capture       │             │ │
│  │  └────────────────────┘     └──────────────────────────┘             │ │
│  └──────────────────────────────────────────────────────────────────────┘ │
│                                       │                                    │
│                                       ▼                                    │
│                          ┌─────────────────────────┐                      │
│                          │   REPORT GENERATOR      │                      │
│                          │   (report.rs)           │                      │
│                          │   - Pretty/Simple/Compact│                     │
│                          └─────────────────────────┘                      │
└────────────────────────────────────────────────────────────────────────────┘
```

---

## File-by-File Breakdown

### 1. `Cargo.toml`

**Purpose**: Crate manifest defining dependencies and test targets.

**Key dependencies**:
- `trading-common`, `data-manager`, `dbn` - Core workspace crates
- `tokio` - Async runtime with full features
- `rand`, `rand_chacha` - Deterministic random generation
- `parking_lot` - Fast synchronization primitives

**Test targets**:
```toml
[[test]] name = "e2e_pipeline"      # Full pipeline tests
[[test]] name = "generator_tests"   # Data generation verification
[[test]] name = "latency_tests"     # Latency measurement tests
```

---

### 2. `src/lib.rs`

**Purpose**: Library entry point with module exports and re-exports.

Organizes the crate into 7 modules and re-exports commonly used types for ergonomic imports:

```rust
pub use config::{DataGenConfig, EmulatorConfig, ...};
pub use emulator::{extract_latency_us, EmulatorMetrics, TestDataEmulator};
pub use generator::{TestDataBundle, TestDataGenerator, ...};
pub use metrics::{LatencyStats, MetricsCollector, ...};
pub use report::{generate_report, ReportFormat};
pub use runner::{RustStrategyRunner, StrategyRunner, ...};
pub use strategies::TestTickCounterStrategy;
```

---

### 3. `src/config.rs`

**Purpose**: Configuration structs controlling all test parameters.

**Key types**:

| Struct | Purpose |
|--------|---------|
| `VolumeProfile` | Enum: Heavy (1000 tps), Normal (250 tps), Lite (25 tps) per symbol |
| `DataGenConfig` | Symbol count, time window, seed, exchange, base price |
| `EmulatorConfig` | Replay speed, send time embedding, minimum delay |
| `StrategyConfig` | Rust/Python strategy counts, strategy type |
| `MetricsConfig` | Sample limits, tolerance thresholds, max latencies |
| `TestConfig` | Timeout, settling time, database verification |
| `IntegrationTestConfig` | Aggregates all above with preset methods |

**Preset configurations**:
```rust
IntegrationTestConfig::lite()   // 5 symbols, 10s, ~1,250 ticks
IntegrationTestConfig::normal() // 10 symbols, 60s, ~150,000 ticks
IntegrationTestConfig::heavy()  // 100 symbols, 60s, ~6,000,000 ticks
```

---

### 4. `src/generator.rs`

**Purpose**: Deterministic test data generator producing DBN-format `TradeMsg` ticks.

**Key components**:

- **`SymbolState`**: Per-symbol state tracking price (random walk), sequence, size, and buy/sell alternation.

- **`TestDataGenerator`**: Main generator using `ChaCha8Rng` for reproducibility.

**Timestamp generation by profile**:
| Profile | Method | Characteristics |
|---------|--------|-----------------|
| Heavy | `generate_heavy_timestamps()` | Clustered bursts of 10-50 ticks |
| Normal | `generate_normal_timestamps()` | Poisson distribution (exponential inter-arrival) |
| Lite | `generate_lite_timestamps()` | Uniform with ±10% jitter |

**Output**: `TestDataBundle` containing:
- `ticks: Vec<TradeMsg>` sorted by timestamp
- `metadata: TestDataMetadata` with generation info
- `symbol_counts: HashMap<String, u64>` per-symbol tick counts

---

### 5. `src/emulator.rs`

**Purpose**: Mock `LiveStreamProvider` that replays pre-generated data.

**Key mechanisms**:

1. **Timestamp Rewriting**:
   ```rust
   let now_micros = now.timestamp_micros();
   let ts_in_delta = (now_micros % 0x7FFF_FFFF) as i32;  // Embed send time
   ```
   Rewrites all timestamps to current system time for realistic latency measurement.

2. **Delay Calculation**:
   ```rust
   let adjusted_nanos = (diff_nanos as f64 / self.config.replay_speed) as u64;
   ```
   Scales inter-tick delays by replay speed multiplier.

3. **Latency Extraction**:
   ```rust
   pub fn extract_latency_us(ts_in_delta: i32) -> Option<u64>
   ```
   Computes round-trip latency by comparing embedded send time with receive time, handling 31-bit wraparound.

---

### 6. `src/metrics.rs`

**Purpose**: Latency statistics and test result aggregation.

**`LatencyStats`**:
- Uses **Welford's algorithm** for streaming mean/variance calculation
- Maintains sample reservoir for percentile computation (capped at configurable limit)
- Supports merging for aggregation across strategies

Key methods:
```rust
fn record(&mut self, latency_us: u64)    // O(1) online update
fn percentile_us(&self, p: f64) -> u64   // Requires sorting samples
fn merge(&mut self, other: &LatencyStats) // Parallel merge of statistics
```

**`StrategyMetrics`**:
- Thread-safe per-strategy counters using `AtomicU64`
- Tracks first/last tick times for throughput calculation
- Records latency samples via `record_tick(latency_us)`

**`TestResults`**:
- Aggregates all metrics with validation against thresholds
- Computes tick loss rate: `1.0 - (avg_received / ticks_sent)`
- Validates average and p99 latency against config limits

**`MetricsCollector`**:
- Thread-safe coordinator for all strategy metrics
- Manages test timing (start/stop)
- Builds final `TestResults` with validation

---

### 7. `src/report.rs`

**Purpose**: Formatted output generation in three styles.

**Report formats**:

| Format | Use Case | Output Style |
|--------|----------|--------------|
| `Pretty` | Terminal display | Box-drawing characters, aligned columns |
| `Simple` | Plain text | Line-based, no formatting |
| `Compact` | Logging/CI | Single line with key metrics |

**Pretty report example** (generated):
```
╔════════════════════════════════════════════════════════════════╗
║            INTEGRATION TEST RESULTS                            ║
╠════════════════════════════════════════════════════════════════╣
║ Status: PASSED                                                 ║
║ Duration: 65.2s                                                ║
╠════════════════════════════════════════════════════════════════╣
║ TICK COUNTS:                                                   ║
║   Generated:         1,500,000                                 ║
║   Sent:              1,500,000                                 ║
║   Loss Rate:              0.01%                                ║
╠════════════════════════════════════════════════════════════════╣
║ LATENCY (μs):                                                  ║
║   Average:                245.3                                ║
║   p99:                    1,250                                ║
╚════════════════════════════════════════════════════════════════╝
```

---

### 8. `src/runner/mod.rs`

**Purpose**: Strategy runner trait and coordination framework.

**`StrategyRunner` trait**:
```rust
#[async_trait]
pub trait StrategyRunner: Send + Sync {
    fn id(&self) -> &str;
    fn strategy_type(&self) -> &str;
    async fn process_tick(&self, tick: &NormalizedTick);
    fn metrics(&self) -> Arc<StrategyMetrics>;
    async fn shutdown(&self);
}
```

**`StrategyRunnerFactory`**:
Creates runner instances based on config (N Rust + M Python strategies).

**`StrategyRunnerManager`**:
- Coordinates multiple runners
- `broadcast_tick()` - Sends tick to all runners
- `all_metrics()` - Collects metrics from all runners
- `shutdown_all()` - Graceful shutdown

---

### 9. `src/runner/rust_runner.rs`

**Purpose**: Rust strategy runner that wraps Strategy implementations.

**`RustStrategyRunner`**:
- Wraps `Strategy` in `parking_lot::Mutex` for interior mutability
- Computes latency from `ts_recv` vs current time
- Records metrics via `StrategyMetrics`
- `with_tick_counter()` - Convenience constructor using `TestTickCounterStrategy`

---

### 10. `src/runner/python_runner.rs`

**Purpose**: Python strategy runner (currently a stub).

**`PythonStrategyRunner`**:
- Placeholder for PyO3 integration
- `check_python_available()` returns `false` (not yet implemented)
- Still tracks metrics and latency for testing infrastructure

---

### 11. `src/strategies/`

**Purpose**: Test strategy implementations (separate from runners).

**Structure**:
```
src/strategies/
├── mod.rs                  # Module exports
├── test_tick_counter.rs    # Rust: TestTickCounterStrategy implementation
└── test_tick_counter.py    # Python: Test strategy for PythonStrategyRunner
```

**`TestTickCounterStrategy`** (`test_tick_counter.rs`):
Implements `Strategy` trait from `trading-common`:
- Counts every tick received
- Accumulates total value (price × quantity)
- Returns `Signal::Hold` always (no trading)
- `bar_data_mode()` → `OnEachTick`

**`test_tick_counter.py`**: Python equivalent for PythonStrategyRunner testing.

---

### 12. `src/transport/`

**Purpose**: Configurable transport layer for tick delivery.

**Structure**:
```
src/transport/
├── mod.rs                  # Transport trait, config, and factory
├── direct.rs               # DirectTransport (in-memory callback)
└── websocket.rs            # WebSocketTransport (network-based)
```

**Transport Modes**:

| Mode | Latency | Use Case |
|------|---------|----------|
| `WebSocket` | ~90μs | **Default** - realistic network simulation |
| `Direct` | ~12μs | Baseline latency measurement (in-memory) |

**Key Types**:

- **`TransportMode`** enum: `Direct` or `WebSocket`
- **`TransportConfig`**: Mode selection + WebSocket settings
- **`WebSocketConfig`**: Host, port, batching options
- **`TickTransport`** trait: Common interface for all transports

**WebSocket Architecture**:
```
Emulator → WebSocketServer → MessagePack serialize → network → WebSocketClient → deserialize → callback
```

**Serialization**: Uses MessagePack (`rmp-serde`) for efficient binary serialization instead of JSON,
maintaining the spirit of DBN's binary efficiency. MessagePack is ~5x faster than JSON with smaller payloads.

**Configuration Examples**:
```rust
// Default: WebSocket transport (realistic simulation)
EmulatorConfig::default().with_port(19100)

// Direct transport (for baseline measurements)
EmulatorConfig::default().with_direct()

// WebSocket with batching
EmulatorConfig {
    transport: TransportConfig {
        mode: TransportMode::WebSocket,
        websocket: WebSocketConfig {
            port: 9999,
            enable_batching: true,
            batch_size: 100,
            ..Default::default()
        },
    },
    ..Default::default()
}
```

**Batching**: Optional message batching reduces network overhead for high-throughput scenarios.

---

### 13. `tests/e2e_pipeline.rs`

**Purpose**: Full end-to-end integration tests.

**Test helper** `run_pipeline_test()`:
1. Generates test data
2. Creates metrics collector
3. Spawns strategy runners
4. Connects emulator
5. Sets up tick broadcast callback
6. Runs with timeout
7. Collects and returns results

**Test cases**:
| Test | Profile | Purpose |
|------|---------|---------|
| `test_lite_pipeline` | Lite | Quick validation (~15s) |
| `test_normal_pipeline` | Normal | Standard stress test |
| `test_heavy_pipeline` | Heavy | Full stress (ignored by default) |
| `test_custom_config` | Custom | Validates custom configuration |
| `test_concurrent_runs` | Multiple | 3 parallel pipeline runs |
| `test_early_shutdown` | N/A | Graceful shutdown mid-replay |
| `test_report_formats` | Lite | Validates all report formats |
| `test_websocket_transport` | Lite | WebSocket transport validation |
| `test_transport_comparison` | Lite | Direct vs WebSocket latency comparison |

---

### 14. `tests/generator_tests.rs`

**Purpose**: Validates test data generator determinism and correctness.

**Key tests**:
| Test | Validates |
|------|-----------|
| `test_generator_determinism_same_seed` | Same seed → identical output |
| `test_generator_different_seeds` | Different seeds → different output |
| `test_volume_profile_counts` | Each profile generates ~expected ticks |
| `test_ticks_sorted` | Output sorted by timestamp |
| `test_size_monotonic_per_instrument` | Size increases per symbol |
| `test_sequence_monotonic_per_instrument` | Sequence increases per symbol |
| `test_side_alternation` | ~50/50 buy/sell ratio |
| `test_price_positive` | Price never goes ≤ 0 |

---

### 15. `tests/latency_tests.rs`

**Purpose**: Validates latency measurement accuracy.

**Key tests**:
| Test | Validates |
|------|-----------|
| `test_latency_extraction_basic` | Extracts reasonable latency from ts_in_delta |
| `test_latency_extraction_invalid` | Rejects 0 and negative inputs |
| `test_latency_extraction_wraparound` | Handles 31-bit wraparound |
| `test_latency_stats_*` | Mean, median, percentiles, std dev |
| `test_welford_accuracy` | Welford's algorithm matches expected |
| `test_concurrent_latency_recording` | Thread-safe recording |

---

## Data Flow Summary

```
1. DataGenConfig → TestDataGenerator.generate() → TestDataBundle
                                                      │
2. TestDataBundle → TestDataEmulator.subscribe()      │
                         │                            │
3. For each TradeMsg:    │                            │
   ├─ Rewrite timestamps to now                       │
   ├─ Embed send_time in ts_in_delta                  │
   └─ callback(StreamEvent::Tick(NormalizedTick))     │
                         │                            │
4. StrategyRunnerManager.broadcast_tick()             │
   ├─ RustStrategyRunner.process_tick()               │
   │   ├─ Extract latency from ts_recv                │
   │   └─ StrategyMetrics.record_tick(latency_us)     │
   └─ PythonStrategyRunner.process_tick()             │
                         │                            │
5. MetricsCollector.build_results()                   │
   ├─ Aggregate all StrategyMetrics                   │
   ├─ Validate against thresholds                     │
   └─ TestResults { passed, failures, latency_aggregate }
                         │
6. generate_report(results, format) → String
```

---

## Running the Tests

```bash
# Quick validation (Lite profile)
cargo test -p integration-tests test_lite_pipeline

# All generator tests
cargo test -p integration-tests --test generator_tests

# All latency tests
cargo test -p integration-tests --test latency_tests

# Normal stress test
cargo test -p integration-tests test_normal_pipeline -- --nocapture

# Heavy stress test (takes ~5 minutes)
cargo test -p integration-tests test_heavy_pipeline -- --ignored --nocapture
```

---

## Volume Profile Reference

| Profile | Ticks/sec/symbol | Typical Use Case |
|---------|------------------|------------------|
| Lite | 25 | Quick validation, CI/CD pipelines |
| Normal | 250 | Standard stress testing |
| Heavy | 1000 | Full stress test, performance benchmarking |

**Expected tick counts** (60 second window):
- Lite (10 symbols): ~15,000 ticks
- Normal (10 symbols): ~150,000 ticks
- Heavy (100 symbols): ~6,000,000 ticks

---

## Configuration File Format

Configuration files use TOML format (see `config/` directory):

```toml
[data_gen]
symbol_count = 10           # TEST0000 - TEST0009
time_window_secs = 60       # 1 minute
profile = "Normal"          # Heavy/Normal/Lite
seed = 12345                # Deterministic seed
exchange = "TEST"
base_price = 50000.0

[emulator]
replay_speed = 1.0          # Real-time (1.0x)
embed_send_time = true
min_delay_us = 10

[strategies]
rust_count = 5
python_count = 5
strategy_type = "tick_counter"
track_latency = true

[metrics]
latency_sample_limit = 100000
tick_loss_tolerance = 0.001  # 0.1%
max_avg_latency_us = 1000    # 1ms
max_p99_latency_us = 10000   # 10ms

[test]
timeout_secs = 120
settling_time_secs = 5
verify_db_persistence = false
```
