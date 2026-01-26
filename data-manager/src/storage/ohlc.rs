//! OHLC data types and session-aware query helpers
//!
//! Provides structures and query helpers for working with OHLC continuous aggregates.
//! Session boundary handling is done in application code using `SessionSchedule`.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgPool, Row};
use trading_common::instruments::SessionSchedule;
use tracing::debug;

use super::{RepositoryError, RepositoryResult};

/// OHLC bar data from continuous aggregate
#[derive(Debug, Clone, FromRow)]
pub struct OhlcBar {
    pub bucket: DateTime<Utc>,
    pub symbol: String,
    pub exchange: String,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub trade_count: i64,
}

/// Query helper for OHLC continuous aggregates
pub struct OhlcQueryHelper {
    pool: PgPool,
}

impl OhlcQueryHelper {
    /// Create a new OHLC query helper
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Query OHLC bars for a time range (UTC-aligned)
    ///
    /// Returns bars from the specified continuous aggregate view.
    pub async fn get_ohlc_bars(
        &self,
        view_name: &str,
        symbol: &str,
        exchange: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> RepositoryResult<Vec<OhlcBar>> {
        let query = format!(
            r#"
            SELECT bucket, symbol, exchange, open, high, low, close, volume, trade_count
            FROM {}
            WHERE symbol = $1
              AND exchange = $2
              AND bucket >= $3
              AND bucket < $4
            ORDER BY bucket ASC
            "#,
            view_name
        );

        let bars = sqlx::query_as::<_, OhlcBar>(&query)
            .bind(symbol)
            .bind(exchange)
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await?;

        debug!(
            "Fetched {} bars from {} for {}@{} ({} to {})",
            bars.len(),
            view_name,
            symbol,
            exchange,
            start,
            end
        );

        Ok(bars)
    }

    /// Query OHLC bars for a trading session with proper boundary handling
    ///
    /// Fetches bars with padding before session start to ensure coverage,
    /// then trims to exact session boundaries.
    ///
    /// # Arguments
    /// * `view_name` - The continuous aggregate view name (e.g., "ohlc_1m")
    /// * `symbol` - Trading symbol
    /// * `exchange` - Exchange name
    /// * `session_date` - The session's trading date (e.g., Monday for Mon session)
    /// * `session_schedule` - Session schedule defining open/close times
    ///
    /// # Example
    /// ```ignore
    /// use trading_common::instruments::session::presets::cme_globex;
    ///
    /// let schedule = cme_globex();
    /// let bars = helper.get_session_ohlc_bars(
    ///     "ohlc_1m",
    ///     "ESH6",
    ///     "CME",
    ///     NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(),  // Monday
    ///     &schedule,
    /// ).await?;
    /// ```
    pub async fn get_session_ohlc_bars(
        &self,
        view_name: &str,
        symbol: &str,
        exchange: &str,
        session_date: NaiveDate,
        session_schedule: &SessionSchedule,
    ) -> RepositoryResult<Vec<OhlcBar>> {
        // Calculate session boundaries in UTC
        let (session_start, session_end) =
            calculate_session_bounds(session_date, session_schedule)?;

        debug!(
            "Session boundaries for {} on {}: {} to {}",
            symbol, session_date, session_start, session_end
        );

        // Query with 1-day padding before session start (covers chunk boundary)
        let query_start = session_start - chrono::Duration::days(1);

        let query = format!(
            r#"
            SELECT bucket, symbol, exchange, open, high, low, close, volume, trade_count
            FROM {}
            WHERE symbol = $1
              AND exchange = $2
              AND bucket >= $3
              AND bucket < $4
            ORDER BY bucket ASC
            "#,
            view_name
        );

        let rows = sqlx::query_as::<_, OhlcBar>(&query)
            .bind(symbol)
            .bind(exchange)
            .bind(query_start)
            .bind(session_end)
            .fetch_all(&self.pool)
            .await?;

        // Trim to exact session boundaries
        let trimmed: Vec<OhlcBar> = rows
            .into_iter()
            .filter(|bar| bar.bucket >= session_start && bar.bucket < session_end)
            .collect();

        debug!(
            "Fetched {} bars for session {} (trimmed from query range)",
            trimmed.len(),
            session_date
        );

        Ok(trimmed)
    }

    /// Get the latest bar from a continuous aggregate
    pub async fn get_latest_bar(
        &self,
        view_name: &str,
        symbol: &str,
        exchange: &str,
    ) -> RepositoryResult<Option<OhlcBar>> {
        let query = format!(
            r#"
            SELECT bucket, symbol, exchange, open, high, low, close, volume, trade_count
            FROM {}
            WHERE symbol = $1
              AND exchange = $2
            ORDER BY bucket DESC
            LIMIT 1
            "#,
            view_name
        );

        let bar = sqlx::query_as::<_, OhlcBar>(&query)
            .bind(symbol)
            .bind(exchange)
            .fetch_optional(&self.pool)
            .await?;

        Ok(bar)
    }

    /// Get aggregate statistics for a symbol
    pub async fn get_aggregate_stats(
        &self,
        view_name: &str,
        symbol: &str,
        exchange: &str,
    ) -> RepositoryResult<AggregateStats> {
        let query = format!(
            r#"
            SELECT
                COUNT(*)::BIGINT as bar_count,
                MIN(bucket) as earliest_bucket,
                MAX(bucket) as latest_bucket,
                SUM(volume) as total_volume,
                SUM(trade_count)::BIGINT as total_trades
            FROM {}
            WHERE symbol = $1 AND exchange = $2
            "#,
            view_name
        );

        let row = sqlx::query(&query)
            .bind(symbol)
            .bind(exchange)
            .fetch_one(&self.pool)
            .await?;

        Ok(AggregateStats {
            bar_count: row.get::<Option<i64>, _>("bar_count").unwrap_or(0) as u64,
            earliest_bucket: row.get("earliest_bucket"),
            latest_bucket: row.get("latest_bucket"),
            total_volume: row.get::<Option<Decimal>, _>("total_volume"),
            total_trades: row.get::<Option<i64>, _>("total_trades").unwrap_or(0) as u64,
        })
    }
}

/// Statistics for an OHLC aggregate
#[derive(Debug, Clone)]
pub struct AggregateStats {
    pub bar_count: u64,
    pub earliest_bucket: Option<DateTime<Utc>>,
    pub latest_bucket: Option<DateTime<Utc>>,
    pub total_volume: Option<Decimal>,
    pub total_trades: u64,
}

/// Calculate UTC session boundaries from local session schedule
fn calculate_session_bounds(
    session_date: NaiveDate,
    schedule: &SessionSchedule,
) -> RepositoryResult<(DateTime<Utc>, DateTime<Utc>)> {
    // Get session open time in local timezone
    let open_time = schedule
        .session_open_time(session_date)
        .ok_or_else(|| RepositoryError::InvalidData("No session on this date".into()))?;

    // For overnight sessions (CME-style), close is on the next day
    let close_date = if is_overnight_session(schedule) {
        session_date + chrono::Duration::days(1)
    } else {
        session_date
    };

    let close_time = schedule
        .session_close_time(close_date)
        .ok_or_else(|| RepositoryError::InvalidData("No session close time".into()))?;

    // Convert to UTC
    let session_start = schedule
        .timezone
        .from_local_datetime(&session_date.and_time(open_time))
        .single()
        .ok_or_else(|| RepositoryError::InvalidData("Ambiguous local time for session start".into()))?
        .with_timezone(&Utc);

    let session_end = schedule
        .timezone
        .from_local_datetime(&close_date.and_time(close_time))
        .single()
        .ok_or_else(|| RepositoryError::InvalidData("Ambiguous local time for session end".into()))?
        .with_timezone(&Utc);

    Ok((session_start, session_end))
}

/// Check if a session schedule represents an overnight session
/// (where end time is less than start time)
fn is_overnight_session(schedule: &SessionSchedule) -> bool {
    schedule
        .regular_sessions
        .first()
        .map(|s| s.end_time < s.start_time)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Timelike;
    use trading_common::instruments::session::presets::cme_globex;

    #[test]
    fn test_is_overnight_session() {
        let schedule = cme_globex();
        assert!(is_overnight_session(&schedule));
    }

    #[test]
    fn test_calculate_session_bounds_cme() {
        let schedule = cme_globex();
        let date = NaiveDate::from_ymd_opt(2024, 1, 15).unwrap(); // Monday

        let (start, end) = calculate_session_bounds(date, &schedule).unwrap();

        // CME opens at 17:00 CT on Monday = 23:00 UTC on Monday
        // CME closes at 16:00 CT on Tuesday = 22:00 UTC on Tuesday
        assert_eq!(start.hour(), 23); // 5 PM CT in winter = 23:00 UTC
        assert_eq!(end.hour(), 22); // 4 PM CT = 22:00 UTC
        assert!(end > start);
    }
}
