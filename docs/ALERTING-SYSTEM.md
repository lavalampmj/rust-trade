# Alerting System Documentation

**Status**: Production Ready
**Last Updated**: 2026-01-17
**Test Coverage**: 25/25 tests passing

## Overview

The alerting system monitors critical metrics in the trading system and triggers alerts when thresholds are exceeded. It integrates with Prometheus metrics and provides configurable rules for monitoring:

1. **Database Connection Pool Saturation**
2. **Batch Processing Failures**
3. **WebSocket Disconnections**
4. **WebSocket Reconnection Storms**
5. **Channel Backpressure**

## Architecture

### Components

```
┌─────────────────┐
│  Prometheus     │ ← Metrics Collection
│  Metrics        │
└────────┬────────┘
         │
         ↓
┌─────────────────┐
│  Alert Rules    │ ← Threshold Definitions
└────────┬────────┘
         │
         ↓
┌─────────────────┐
│  Alert          │ ← Periodic Evaluation
│  Evaluator      │   (Every 30s by default)
└────────┬────────┘
         │
         ↓
┌─────────────────┐
│  Alert Handler  │ ← Log to stdout/stderr
└─────────────────┘
```

### Key Modules

1. **`alerting/rules.rs`**: Alert rule definitions and conditions
2. **`alerting/evaluator.rs`**: Periodic evaluation engine with cooldown management
3. **`alerting/handler.rs`**: Alert handler implementations (logging, multi-handler support)

## Configuration

Configuration is managed in `config/development.toml`:

```toml
[alerting]
# Enable the alerting system
enabled = true

# How often to evaluate alert rules (in seconds)
interval_secs = 30

# Cooldown period between repeated alerts (in seconds)
cooldown_secs = 300  # 5 minutes

# Connection pool saturation threshold (0.0-1.0)
pool_saturation_threshold = 0.8  # 80%

# Connection pool critical threshold (0.0-1.0)
pool_critical_threshold = 0.95  # 95%

# Batch failure rate threshold (0.0-1.0)
batch_failure_threshold = 0.2  # 20%

# Channel backpressure threshold (0.0-100.0)
channel_backpressure_threshold = 80.0  # 80%

# WebSocket reconnection storm threshold
reconnection_storm_threshold = 10
```

## Alert Rules

### 1. Connection Pool Saturation (WARNING)

**Trigger**: Active database connections ≥ `pool_saturation_threshold` * max_connections
**Default**: 80% utilization (6 out of 8 connections)
**Action Required**: Consider increasing connection pool size or optimizing queries

**Example Alert**:
```
[ALERT:WARNING] Database connection pool saturation: 7 of 8 connections active
(87.5% utilization, threshold: 80.0%)
```

### 2. Connection Pool Critical (CRITICAL)

**Trigger**: Active database connections ≥ `pool_critical_threshold` * max_connections
**Default**: 95% utilization (8 out of 8 connections)
**Action Required**: Immediate investigation - queries may be timing out

**Example Alert**:
```
[ALERT:CRITICAL] Database connection pool saturation: 8 of 8 connections active
(100.0% utilization, threshold: 95.0%)
```

### 3. Batch Failure Rate (WARNING)

**Trigger**: (failed_batches / total_batches) ≥ `batch_failure_threshold`
**Default**: 20% failure rate
**Action Required**: Check database connectivity and query errors

**Example Alert**:
```
[ALERT:WARNING] High batch failure rate: 3 failures out of 10 total batches
(30.0% failure rate, threshold: 20.0%)
```

### 4. WebSocket Disconnected (CRITICAL)

**Trigger**: WebSocket connection status = 0 (disconnected)
**Default**: Always critical
**Action Required**: Data collection has stopped - investigate exchange connection

**Example Alert**:
```
[ALERT:CRITICAL] WebSocket connection lost (total disconnections: 5)
```

### 5. WebSocket Reconnection Storm (WARNING)

**Trigger**: Total reconnection attempts > `reconnection_storm_threshold`
**Default**: More than 10 reconnections
**Action Required**: May indicate network issues or exchange problems

**Example Alert**:
```
[ALERT:WARNING] Excessive WebSocket reconnection attempts: 12 reconnects
(threshold: 10)
```

### 6. Channel Backpressure (WARNING)

**Trigger**: Channel buffer utilization ≥ `channel_backpressure_threshold`%
**Default**: 80% utilization
**Action Required**: Data processing is falling behind - check downstream bottlenecks

**Example Alert**:
```
[ALERT:WARNING] Channel backpressure detected: 85.0% utilization
(425 ticks in buffer, threshold: 80.0%)
```

## Alert Severity Levels

Alerts are categorized by severity (ascending order):

1. **INFO**: Informational messages (not currently used)
2. **WARNING**: Requires attention but not immediately critical
3. **CRITICAL**: Immediate action required - system functionality degraded

## Cooldown Mechanism

To prevent alert spam, the system implements a cooldown period (default: 5 minutes). Once an alert fires:

1. Alert is logged immediately
2. Cooldown timer starts for that specific rule
3. If the same condition persists, no additional alerts are sent during cooldown
4. After cooldown expires, if condition still exists, alert fires again

**Example**:
```
12:00:00 - [ALERT:CRITICAL] Connection pool saturated (first alert)
12:02:00 - Still saturated, but suppressed (within 5-min cooldown)
12:05:01 - [ALERT:CRITICAL] Connection pool saturated (cooldown expired, alert fires again)
```

## Integration

### Startup Integration

The alerting system is initialized in `main.rs` during application startup:

```rust
async fn init_alerting_system() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::new()?;

    if !settings.alerting.enabled {
        return Ok(());
    }

    let handler = LogAlertHandler::new();
    let mut evaluator = AlertEvaluator::new(handler);
    evaluator.set_cooldown(Duration::from_secs(settings.alerting.cooldown_secs));

    // Add rules
    evaluator.add_rule(AlertRule::connection_pool_saturation(...));
    // ... more rules

    // Start background monitoring
    evaluator.start_monitoring(settings.alerting.interval_secs);

    Ok(())
}
```

### Metrics Integration

Alerting relies on Prometheus metrics defined in `metrics.rs`:

- `trading_db_connections_active` - Active database connections
- `trading_batches_failed_total` - Total failed batches
- `trading_batches_flushed_total` - Total successful batches
- `trading_ws_connection_status` - WebSocket connection status (0/1)
- `trading_ws_reconnects_total` - Total reconnection attempts
- `trading_channel_utilization_percent` - Channel buffer utilization

## Testing

### Test Coverage

All 25 tests pass with comprehensive coverage:

- Rule evaluation logic (6 tests)
- Alert handlers (3 tests)
- Alert evaluator (5 tests)
- Integration tests (11 tests)

### Key Test Scenarios

1. **Threshold validation**: Alerts trigger at correct utilization levels
2. **Cooldown enforcement**: Repeated alerts are suppressed during cooldown period
3. **Multi-rule evaluation**: Multiple rules evaluated independently
4. **Zero-division safety**: Batch failure rate handles zero batches gracefully
5. **Background monitoring**: Async task evaluates rules periodically

### Running Tests

```bash
# Run all alerting tests
cargo test alerting --lib

# Run specific test
cargo test alerting::tests::test_connection_pool_saturation_alert
```

## Operational Guidelines

### Monitoring Alerts

Alerts are logged to stdout/stderr with structured fields:

```
2026-01-17T12:00:00.000Z ERROR [ALERT:CRITICAL]
  metric=websocket_disconnected
  message="WebSocket connection lost (total disconnections: 5)"
```

Use `grep` or log aggregation tools to filter alerts:

```bash
# View all alerts
cargo run live 2>&1 | grep '\[ALERT'

# View only critical alerts
cargo run live 2>&1 | grep '\[ALERT:CRITICAL\]'
```

### Tuning Thresholds

Adjust thresholds based on your environment:

1. **High-volume environments**: Increase `pool_saturation_threshold` to 0.9 (90%)
2. **Development**: Set `batch_failure_threshold` to 0.5 (50%) for noisier environments
3. **Production**: Keep defaults or reduce to be more sensitive

### Disabling Alerting

Set `alerting.enabled = false` in configuration or via environment variable:

```bash
# Disable in config
[alerting]
enabled = false

# Or override at runtime (if implemented)
ALERTING_ENABLED=false cargo run live
```

## Future Enhancements

Potential improvements for the alerting system:

1. **Multiple Handlers**:
   - Email notifications
   - Slack/Discord webhooks
   - PagerDuty integration
   - SMS alerts

2. **Alert Persistence**:
   - Store alerts in database for historical analysis
   - Alert dashboard in web UI

3. **Advanced Rules**:
   - Rate-of-change detection (e.g., "connections increased by 50% in 5 minutes")
   - Composite rules (e.g., "high CPU AND high memory")
   - Time-based rules (e.g., "alert only during business hours")

4. **Alert Acknowledgement**:
   - Manual acknowledgement to suppress known issues
   - Auto-resolution when condition clears

5. **Metrics Export**:
   - Export alert metrics to Prometheus
   - Grafana dashboard for alert visualization

## Troubleshooting

### Alerts Not Firing

1. Check if alerting is enabled: `grep "Alerting system started" logs`
2. Verify metrics are being updated: `curl http://localhost:9090/metrics | grep trading_`
3. Confirm thresholds are set correctly in configuration
4. Check evaluation interval: alerts evaluate every 30s by default

### Too Many Alerts

1. Increase cooldown period: `cooldown_secs = 600` (10 minutes)
2. Adjust thresholds to be less sensitive
3. Investigate root cause of repeated threshold violations

### Missing Dependencies

Alerting requires:
- Prometheus metrics system initialized
- Configuration file with `[alerting]` section
- Metrics HTTP server running on port 9090

## References

- [Prometheus Metrics Documentation](../PROMETHEUS-METRICS.md) (if exists)
- [Configuration Guide](../CLAUDE.md#configuration-system)
- [Architecture Overview](../CLAUDE.md#architecture-overview)

---

**Notes**:
- Alerting system is production-ready and fully tested
- Integrates seamlessly with existing Prometheus metrics
- Designed for extensibility (easy to add new alert handlers and rules)
