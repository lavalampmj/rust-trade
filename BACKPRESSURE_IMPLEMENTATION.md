# Backpressure Mechanism Implementation

**Date**: 2026-01-16
**Priority**: 🔴 **Critical** (Priority 1 from SESSION_REVIEW.md)
**Status**: ✅ **IMPLEMENTED**

---

## Overview

Implemented comprehensive backpressure handling in the MarketDataService to prevent the system from being overwhelmed during high-volume trading periods. This addresses the critical concern identified in the security review about fast exchanges overwhelming slow database writes.

---

## Problem Statement

### Original Issue
- **Channel capacity**: 1,000 ticks
- **No backpressure monitoring**: System could silently drop data or block
- **No visibility**: No metrics on pipeline health
- **Small batch size**: 100 ticks = more database round-trips

### Risk with 10 Symbols
With the expansion from 3 to 10 trading pairs:
- **Estimated throughput**: ~40 ticks/min → potentially 400+ ticks/min during volatile markets
- **Burst scenarios**: Flash crashes, major news events could cause 1000s of ticks/sec
- **Consequence**: Channel overflow → blocking → missed ticks → data loss

---

## Solution Implemented

### 1. Increased Channel Capacity

**File**: `trading-core/src/service/market_data.rs:73-80`

```rust
// Before
let (tick_tx, tick_rx) = mpsc::channel::<TickData>(1000);

// After
const CHANNEL_CAPACITY: usize = 10_000;  // 10x increase
let (tick_tx, tick_rx) = mpsc::channel::<TickData>(CHANNEL_CAPACITY);

info!(
    "Data pipeline initialized with buffer capacity: {} ticks",
    CHANNEL_CAPACITY
);
```

**Benefits**:
- ✅ Handles 10x more burst traffic
- ✅ Provides ~15 minutes of buffer at 10 ticks/sec
- ✅ Graceful degradation during database slowdowns

**Memory Impact**:
```
TickData size: ~200 bytes
Buffer: 10,000 × 200 bytes = 2 MB
Total system memory with 10 symbols: ~25 MB (acceptable)
```

### 2. Backpressure Monitoring & Warnings

**File**: `trading-core/src/service/market_data.rs:110-173`

```rust
// Check channel capacity for backpressure monitoring
let capacity = tx.capacity();
let max_capacity = tx.max_capacity();
let utilization_pct = if max_capacity > 0 {
    ((max_capacity - capacity) as f64 / max_capacity as f64) * 100.0
} else {
    0.0
};

// Warn if channel is getting full (>80% utilized)
if utilization_pct > 80.0 {
    warn!(
        "Processing backpressure detected: channel {}% full ({}/{})",
        utilization_pct as u32,
        max_capacity - capacity,
        max_capacity
    );
}
```

**Alert Levels**:
- **>80% utilization**: `WARN` - Processing is falling behind
- **>50% utilization**: `DEBUG` - Normal monitoring
- **Channel full**: `WARN` + blocking send

### 3. Graceful Degradation with try_send

**File**: `trading-core/src/service/market_data.rs:135-168`

```rust
// Use try_send for non-blocking check, fallback to send if needed
match tx.try_send(tick.clone()) {
    Ok(_) => {
        // Successfully sent without blocking
    }
    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
        // Channel is full, warn and use blocking send
        warn!(
            "Channel full! Blocking until space available. Symbol: {}, Price: {}",
            tick.symbol, tick.price
        );

        if let Err(e) = tx.send(tick).await {
            if !e.to_string().contains("channel closed") {
                error!("Failed to send tick data after blocking: {}", e);
            }
        }
    }
    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
        // Channel closed, likely during shutdown
        debug!("Tick channel closed, skipping tick");
    }
}
```

**Behavior**:
1. **Normal operation**: Non-blocking `try_send()` succeeds immediately
2. **High load**: If channel full, logs warning and uses blocking `send()`
3. **Graceful shutdown**: Detects closed channel and skips tick

### 4. Periodic Health Monitoring

**File**: `trading-core/src/service/market_data.rs:225-235, 303-330`

```rust
// Add periodic health monitoring (every 30 seconds)
let mut health_monitor = interval(Duration::from_secs(30));
let mut last_tick_count = 0u64;

// In select! loop:
_ = health_monitor.tick() => {
    let current_stats = stats.lock().await;
    let current_tick_count = current_stats.total_ticks_processed;
    let ticks_processed_since_last = current_tick_count - last_tick_count;
    last_tick_count = current_tick_count;

    info!(
        "Pipeline Health: {} ticks/30s | Total: {} | Batches: {} | Failed: {} | Retries: {} | Buffer: {}/{}",
        ticks_processed_since_last,
        current_stats.total_ticks_processed,
        current_stats.total_batches_flushed,
        current_stats.total_failed_batches,
        current_stats.total_retry_attempts,
        batch_buffer.len(),
        batch_config.max_batch_size
    );

    // Warn if batch buffer is getting full
    let buffer_utilization = (batch_buffer.len() as f64 / batch_config.max_batch_size as f64) * 100.0;
    if buffer_utilization > 80.0 {
        warn!(
            "Batch buffer {}% full ({}/{}), database writes may be slow",
            buffer_utilization as u32,
            batch_buffer.len(),
            batch_config.max_batch_size
        );
    }
}
```

**Metrics Logged Every 30s**:
- ✅ Throughput: Ticks processed in last 30 seconds
- ✅ Total ticks processed (cumulative)
- ✅ Batches flushed to database
- ✅ Failed batches (after retries)
- ✅ Retry attempts
- ✅ Current batch buffer utilization

### 5. Optimized Batch Configuration

**File**: `trading-core/src/service/types.rs:16-27`

```rust
impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            // Before: 100 ticks
            // After: 500 ticks (5x increase)
            max_batch_size: 500,

            // Before: 1 second
            // After: 5 seconds
            max_batch_time: 5,

            max_retry_attempts: 3,
            retry_delay_ms: 1000,
        }
    }
}
```

**Benefits**:
- ✅ 5x fewer database round-trips
- ✅ Better amortization of connection overhead
- ✅ Reduced database load
- ✅ Still maintains <5 second latency

**Trade-offs**:
- ⚠️ Slightly higher memory usage (500 vs 100 ticks)
- ⚠️ Up to 5 seconds before flush (vs 1 second)
- ✅ Overall better for throughput and database performance

---

## Monitoring Output Examples

### Normal Operation
```
INFO  Data pipeline initialized with buffer capacity: 10000 ticks
DEBUG Channel utilization: 12% (1200/10000)
INFO  Pipeline Health: 245 ticks/30s | Total: 12450 | Batches: 25 | Failed: 0 | Retries: 0 | Buffer: 120/500
```

### High Load Warning
```
WARN  Processing backpressure detected: channel 85% full (8500/10000), data processing may be falling behind
INFO  Pipeline Health: 3200 ticks/30s | Total: 45600 | Batches: 91 | Failed: 0 | Retries: 2 | Buffer: 420/500
```

### Critical Backpressure
```
WARN  Channel full! Blocking until space available. Symbol: BTCUSDT, Price: 95234.50
WARN  Batch buffer 92% full (460/500), database writes may be slow
INFO  Pipeline Health: 8900 ticks/30s | Total: 128700 | Batches: 257 | Failed: 1 | Retries: 12 | Buffer: 460/500
```

---

## Performance Impact Analysis

### Before Implementation
```
Channel: 1,000 ticks
Batch: 100 ticks every 1s
Throughput: ~100 inserts/sec
Memory: ~200 KB buffer
Visibility: ❌ None
```

### After Implementation
```
Channel: 10,000 ticks
Batch: 500 ticks every 5s
Throughput: ~100+ inserts/sec (same, but with headroom)
Memory: ~2 MB channel + 100 KB batch = ~2.1 MB
Visibility: ✅ Full metrics every 30s
```

### Capacity Calculation
```
Current: ~40 ticks/min = 0.67 ticks/sec
Buffer capacity: 10,000 ticks
Time to fill at current rate: 10,000 / 0.67 = 14,925 seconds = ~4.1 hours

Burst capacity: 10,000 ticks / 60 sec = 166 ticks/sec sustained for 1 min
Flash crash scenario: Can handle 1000 ticks/sec for 10 seconds
```

---

## Testing Recommendations

### 1. Load Testing
```bash
# Simulate high-volume trading
cd trading-core
cargo run --bin trading-core -- live --paper-trading

# Monitor logs for backpressure warnings
tail -f /var/log/trading-core.log | grep -E "WARN|backpressure|Pipeline Health"
```

### 2. Database Slowdown Simulation
```sql
-- Slow down database to test backpressure
-- Run in separate terminal
SELECT pg_sleep(0.1);  -- Add 100ms delay to simulate slow DB
```

Expected behavior:
- ✅ Warnings logged about backpressure
- ✅ Channel fills up
- ✅ System continues operating (blocking send)
- ✅ No data loss

### 3. Metrics Validation
Monitor for 10 minutes and verify:
- ✅ Health logs appear every 30 seconds
- ✅ Tick count increases
- ✅ Batch flushes occur
- ✅ No failed batches under normal conditions

---

## Configuration Tuning

### Environment Variables
No new environment variables required. All configuration is in code.

### Adjustable Parameters

**Channel Capacity** (market_data.rs:73):
```rust
const CHANNEL_CAPACITY: usize = 10_000;  // Adjust based on memory/needs
```

**Health Monitoring Interval** (market_data.rs:228):
```rust
let mut health_monitor = interval(Duration::from_secs(30));  // Adjust frequency
```

**Batch Configuration** (types.rs:19-26):
```rust
max_batch_size: 500,  // Trade-off: latency vs throughput
max_batch_time: 5,    // Trade-off: data freshness vs efficiency
```

### Tuning Guidelines

**If seeing frequent backpressure warnings**:
1. Increase `max_batch_size` to 1000
2. Increase database `max_connections` from 5 to 8-10
3. Consider adding database indexes (see SESSION_REVIEW.md)

**If memory constrained**:
1. Decrease `CHANNEL_CAPACITY` to 5000
2. Decrease `max_batch_size` to 250
3. Accept higher database load

**If latency is critical**:
1. Decrease `max_batch_time` to 2-3 seconds
2. Accept more database round-trips

---

## Files Modified

1. **trading-core/src/service/market_data.rs**
   - Increased channel capacity from 1,000 to 10,000
   - Added backpressure monitoring with capacity checks
   - Implemented try_send with fallback to blocking send
   - Added periodic health monitoring (every 30s)
   - Enhanced logging with detailed metrics

2. **trading-core/src/service/types.rs**
   - Increased `max_batch_size` from 100 to 500
   - Increased `max_batch_time` from 1s to 5s
   - Added detailed comments explaining trade-offs

---

## Integration with Other Priorities

This implementation complements other SESSION_REVIEW.md recommendations:

### ✅ Implemented
- **Backpressure mechanism** - This PR

### 🔜 Next Steps
- **Add Prometheus metrics** - Can expose channel utilization, throughput, etc.
- **Database indexes** - Will reduce batch insert time, reduce backpressure
- **Rate limiting** - Separate concern for exchange reconnections
- **Paper trading channel** - Decouple from hot path (mentioned in review)

---

## Verification Steps

### 1. Build & Test
```bash
cargo build --package trading-core
cargo test --workspace
# Expected: ✅ All tests pass
```

### 2. Start System
```bash
cd trading-core
cargo run --bin trading-core -- live --paper-trading
```

### 3. Verify Logs
Look for these log entries:
```
✅ "Data pipeline initialized with buffer capacity: 10000 ticks"
✅ "Pipeline Health: X ticks/30s | ..." (every 30 seconds)
⚠️ "Processing backpressure detected..." (during high load)
```

### 4. Monitor Database
```bash
watch -n 5 'psql postgresql://mj:mj@localhost/trading_core -c "SELECT symbol, COUNT(*) FROM tick_data GROUP BY symbol ORDER BY symbol;"'
```

Expected: Steady growth in tick counts across all 10 symbols

---

## Known Limitations

1. **No Prometheus metrics yet** - Logs only, not time-series metrics
2. **No automatic channel scaling** - Fixed 10k capacity
3. **Paper trading still on hot path** - Could benefit from separate channel
4. **No circuit breaker** - System will block indefinitely if database is down

See SESSION_REVIEW.md for planned improvements addressing these.

---

## Rollback Plan

If issues arise:

### Quick Rollback
```bash
git diff HEAD trading-core/src/service/market_data.rs
git checkout HEAD -- trading-core/src/service/market_data.rs
git checkout HEAD -- trading-core/src/service/types.rs
cargo build
```

### Partial Rollback (keep monitoring, revert capacity)
Edit `market_data.rs:73`:
```rust
const CHANNEL_CAPACITY: usize = 1_000;  // Back to original
```

---

## Success Metrics

After deployment, monitor for:
- ✅ Zero `"Channel full! Blocking..."` warnings under normal load
- ✅ Backpressure warnings <1% of health check intervals
- ✅ All 10 symbols collecting data
- ✅ Database batch insert times <100ms
- ✅ No failed batches
- ✅ Memory usage stable at ~25 MB

---

## References

- **SESSION_REVIEW.md** - Section 4.4, Priority 1
- **Tokio mpsc docs**: https://docs.rs/tokio/latest/tokio/sync/mpsc/
- **Backpressure patterns**: https://tokio.rs/tokio/topics/backpressure

---

**Implementation Completed By**: Claude Code Assistant
**Date**: 2026-01-16
**Review Status**: Ready for testing with 10 symbols
**Production Ready**: After 24h soak test ✅
