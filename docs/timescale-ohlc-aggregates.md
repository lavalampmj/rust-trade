# Plan: UTC-Aligned 1-Minute Continuous Aggregates with Session-Aware Queries

**Status**: Implementation Plan
**Created**: 2026-01-24

## Objective

Create TimescaleDB continuous aggregates for 1-minute OHLC bars using standard UTC alignment. Session boundary handling stays in application code, leveraging the existing `SessionSchedule` infrastructure.

## Approach

**Keep TimescaleDB simple (UTC-aligned)**:
- Continuous aggregates use default `time_bucket()` aligned to UTC
- No `origin` parameter complexity or DST concerns in database layer

**Handle sessions in application code**:
- Query with expanded time range (session start - 1 chunk padding)
- Trim results using `SessionSchedule.is_open()` or session boundaries
- Existing session infrastructure already supports CME Globex (17:00 CT → 16:00 CT)

## Why This Is Better

| Aspect | Session-Aligned Origin | UTC + Application Trim |
|--------|----------------------|------------------------|
| TimescaleDB complexity | High (origin, DST) | Low (standard config) |
| Multi-exchange support | Separate aggregates per exchange | Single aggregate, app handles sessions |
| DST handling | In database (complex) | In app (already exists) |
| Query overhead | 2 chunks for CME session | 2 chunks + trim (same) |
| Flexibility | Locked to one session schedule | Any session schedule |

## Files to Modify

| File | Change |
|------|--------|
| `data-manager/src/storage/timescale.rs` | Add `create_ohlc_1m()` (UTC-aligned) |
| `data-manager/src/cli/db.rs` | Add CLI commands for aggregate management |
| `data-manager/src/storage/mod.rs` | Add session-aware query helpers |

## Implementation Steps

### Step 1: Create UTC-Aligned 1-Minute Aggregate

In `data-manager/src/storage/timescale.rs`:

```rust
/// Create a 1-minute OHLC continuous aggregate (UTC-aligned)
pub async fn create_ohlc_1m(&self) -> RepositoryResult<()> {
    info!("Creating OHLC 1-minute continuous aggregate...");

    let query = r#"
        CREATE MATERIALIZED VIEW IF NOT EXISTS ohlc_1m
        WITH (timescaledb.continuous) AS
        SELECT
            time_bucket(INTERVAL '1 minute', ts_event) AS bucket,
            symbol,
            exchange,
            FIRST(price, ts_event) AS open,
            MAX(price) AS high,
            MIN(price) AS low,
            LAST(price, ts_event) AS close,
            SUM(size) AS volume,
            COUNT(*) AS trade_count
        FROM market_ticks
        GROUP BY bucket, symbol, exchange
        WITH NO DATA
    "#;

    sqlx::query(query).execute(&self.pool).await?;
    info!("Created OHLC 1m continuous aggregate");
    Ok(())
}
```

### Step 2: Add Refresh Policy

```rust
pub async fn add_ohlc_1m_refresh_policy(&self) -> RepositoryResult<()> {
    let query = r#"
        SELECT add_continuous_aggregate_policy(
            'ohlc_1m',
            start_offset => INTERVAL '1 hour',
            end_offset => INTERVAL '1 minute',
            schedule_interval => INTERVAL '1 minute',
            if_not_exists => TRUE
        )
    "#;

    sqlx::query(query).execute(&self.pool).await?;
    info!("Added refresh policy for ohlc_1m");
    Ok(())
}
```

### Step 3: Session-Aware Query Helper

Add to `data-manager/src/storage/queries.rs` (new file or existing module):

```rust
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use trading_common::instruments::SessionSchedule;

/// Query OHLC bars for a trading session with proper boundary handling
///
/// Fetches 1 extra day of data before session start to ensure coverage,
/// then trims to exact session boundaries.
pub async fn get_session_ohlc_bars(
    pool: &PgPool,
    symbol: &str,
    session_date: NaiveDate,      // The session's trading date (e.g., Monday for Mon session)
    session_schedule: &SessionSchedule,
) -> RepositoryResult<Vec<OhlcBar>> {
    // Calculate session boundaries in UTC
    let (session_start, session_end) = calculate_session_bounds(
        session_date,
        session_schedule,
    )?;

    // Query with 1-day padding before session start (covers chunk boundary)
    let query_start = session_start - chrono::Duration::days(1);

    let rows = sqlx::query_as::<_, OhlcBar>(
        r#"
        SELECT bucket, symbol, exchange, open, high, low, close, volume, trade_count
        FROM ohlc_1m
        WHERE symbol = $1
          AND bucket >= $2
          AND bucket < $3
        ORDER BY bucket ASC
        "#,
    )
    .bind(symbol)
    .bind(query_start)
    .bind(session_end)
    .fetch_all(pool)
    .await?;

    // Trim to exact session boundaries
    let trimmed: Vec<OhlcBar> = rows
        .into_iter()
        .filter(|bar| bar.bucket >= session_start && bar.bucket < session_end)
        .collect();

    Ok(trimmed)
}

/// Calculate UTC session boundaries from local session schedule
fn calculate_session_bounds(
    session_date: NaiveDate,
    schedule: &SessionSchedule,
) -> RepositoryResult<(DateTime<Utc>, DateTime<Utc>)> {
    // Get session open/close times in local timezone
    let open_time = schedule.session_open_time(session_date)
        .ok_or_else(|| RepositoryError::InvalidData("No session on this date".into()))?;

    // For CME-style overnight sessions, close is next day
    let close_date = if schedule.regular_sessions.first()
        .map(|s| s.end_time < s.start_time)
        .unwrap_or(false)
    {
        session_date + chrono::Duration::days(1)
    } else {
        session_date
    };

    let close_time = schedule.session_close_time(close_date)
        .ok_or_else(|| RepositoryError::InvalidData("No session close time".into()))?;

    // Convert to UTC
    let session_start = schedule.timezone
        .from_local_datetime(&session_date.and_time(open_time))
        .single()
        .ok_or_else(|| RepositoryError::InvalidData("Ambiguous local time".into()))?
        .with_timezone(&Utc);

    let session_end = schedule.timezone
        .from_local_datetime(&close_date.and_time(close_time))
        .single()
        .ok_or_else(|| RepositoryError::InvalidData("Ambiguous local time".into()))?
        .with_timezone(&Utc);

    Ok((session_start, session_end))
}
```

### Step 4: CLI Commands

In `data-manager/src/cli/db.rs`, add:

```rust
#[derive(Subcommand)]
pub enum AggregateCmd {
    /// Create 1-minute OHLC aggregate
    Create {
        /// Enable auto-refresh policy
        #[arg(long)]
        with_refresh: bool,
    },
    /// Refresh aggregate for a time range
    Refresh {
        #[arg(long)]
        start: String,
        #[arg(long)]
        end: String,
    },
    /// Show aggregate stats
    Stats,
}
```

Usage:
```bash
# Create aggregate
cargo run db aggregate create --with-refresh

# Refresh historical data
cargo run db aggregate refresh --start "2024-01-01" --end "2024-01-31"

# Show stats
cargo run db aggregate stats
```

## Verification

### 1. Verify Aggregate Created
```sql
SELECT * FROM timescaledb_information.continuous_aggregates
WHERE view_name = 'ohlc_1m';
```

### 2. Verify Data Population
```sql
-- Check bar count
SELECT COUNT(*) FROM ohlc_1m;

-- Sample bars
SELECT bucket, symbol, open, close, volume
FROM ohlc_1m
WHERE symbol LIKE 'ES%'
ORDER BY bucket DESC
LIMIT 10;
```

### 3. Test Session Query
```rust
// In test or CLI
let schedule = trading_common::instruments::session::presets::cme_globex();
let bars = get_session_ohlc_bars(
    &pool,
    "ESH6",
    NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),  // Monday
    &schedule,
).await?;

// Verify first bar is at session open (17:00 CT)
// Verify last bar is before session close (16:00 CT next day)
```

### 4. Verify Chunk Coverage
```sql
-- Check which chunks are hit for a session query
EXPLAIN ANALYZE
SELECT * FROM ohlc_1m
WHERE symbol = 'ESH6'
  AND bucket >= '2024-01-14 23:00:00+00'  -- Sunday 17:00 CT
  AND bucket < '2024-01-15 22:00:00+00';  -- Monday 16:00 CT
```

## Integration with Existing Code

The session-aware query uses existing infrastructure:
- `SessionSchedule` from `trading-common/src/instruments/session.rs`
- `presets::cme_globex()` for CME session times
- `session_open_time()` / `session_close_time()` methods
- Timezone handling via `chrono_tz`

## Future Extensions (Out of Scope)

- Larger timeframes (5m, 15m, 1h) - can aggregate from 1m in queries
- Real-time OHLC streaming - separate from historical aggregates
- Multiple exchange-specific aggregates - single aggregate handles all
