//! Market data repository
//!
//! Provides high-level data access operations for market data storage.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::time::Duration;
use thiserror::Error;
use tracing::debug;
use uuid::Uuid;

use crate::config::DatabaseSettings;
use trading_common::data::orderbook::{BookSide, OrderBook, OrderBookDelta};
use trading_common::data::quotes::QuoteTick;
use trading_common::data::types::{TickData, TradeSide};
use trading_common::error::{ErrorCategory, ErrorClassification};

/// Repository errors
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum RepositoryError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

impl ErrorClassification for RepositoryError {
    fn category(&self) -> ErrorCategory {
        match self {
            RepositoryError::Database(_) => ErrorCategory::Transient,
            RepositoryError::Configuration(_) => ErrorCategory::Configuration,
            RepositoryError::NotFound(_) => ErrorCategory::Permanent,
            RepositoryError::InvalidData(_) => ErrorCategory::Permanent,
        }
    }

    fn suggested_retry_delay(&self) -> Option<Duration> {
        match self {
            RepositoryError::Database(_) => Some(Duration::from_millis(500)),
            _ => None,
        }
    }
}

pub type RepositoryResult<T> = Result<T, RepositoryError>;

/// Market data repository
pub struct MarketDataRepository {
    pool: PgPool,
    batch_size: usize,
}

impl MarketDataRepository {
    /// Create a new repository with the given connection pool
    pub fn new(pool: PgPool, batch_size: usize) -> Self {
        Self { pool, batch_size }
    }

    /// Create a new repository from settings
    pub async fn from_settings(settings: &DatabaseSettings) -> RepositoryResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(settings.max_connections)
            .min_connections(settings.min_connections)
            .acquire_timeout(Duration::from_secs(30))
            .connect(&settings.url)
            .await?;

        Ok(Self::new(pool, 1000))
    }

    /// Get the database pool reference
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Insert a single tick
    pub async fn insert_tick(&self, tick: &TickData) -> RepositoryResult<()> {
        sqlx::query(
            r#"
            INSERT INTO market_ticks (
                ts_event, ts_recv, symbol, exchange, price, size, side,
                provider, provider_trade_id, is_buyer_maker, raw_dbn, sequence
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(tick.timestamp)
        .bind(tick.ts_recv)
        .bind(&tick.symbol)
        .bind(&tick.exchange)
        .bind(tick.price)
        .bind(tick.quantity)
        .bind(tick.side.as_db_char().to_string())
        .bind(&tick.provider)
        .bind(&tick.trade_id)
        .bind(tick.is_buyer_maker)
        .bind(&tick.raw_dbn)
        .bind(tick.sequence)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batch insert ticks
    pub async fn batch_insert_ticks(&self, ticks: &[TickData]) -> RepositoryResult<usize> {
        if ticks.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        // Process in chunks
        for chunk in ticks.chunks(self.batch_size) {
            let inserted = self.insert_tick_batch(chunk).await?;
            total_inserted += inserted;
        }

        debug!("Batch inserted {} ticks", total_inserted);
        Ok(total_inserted)
    }

    /// Insert a batch of ticks using COPY
    async fn insert_tick_batch(&self, ticks: &[TickData]) -> RepositoryResult<usize> {
        // Build bulk insert query
        let mut query = String::from(
            r#"
            INSERT INTO market_ticks (
                ts_event, ts_recv, symbol, exchange, price, size, side,
                provider, provider_trade_id, is_buyer_maker, sequence
            ) VALUES
            "#,
        );

        let _args: Vec<String> = Vec::with_capacity(ticks.len() * 11);
        let mut param_count = 1;

        for (i, _tick) in ticks.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }

            query.push_str(&format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                param_count,
                param_count + 1,
                param_count + 2,
                param_count + 3,
                param_count + 4,
                param_count + 5,
                param_count + 6,
                param_count + 7,
                param_count + 8,
                param_count + 9,
                param_count + 10,
            ));
            param_count += 11;
        }

        query.push_str(" ON CONFLICT DO NOTHING");

        // Build and execute query
        let mut sqlx_query = sqlx::query(&query);

        for tick in ticks {
            sqlx_query = sqlx_query
                .bind(tick.timestamp)
                .bind(tick.ts_recv)
                .bind(&tick.symbol)
                .bind(&tick.exchange)
                .bind(tick.price)
                .bind(tick.quantity)
                .bind(tick.side.as_db_char().to_string())
                .bind(&tick.provider)
                .bind(&tick.trade_id)
                .bind(tick.is_buyer_maker)
                .bind(tick.sequence);
        }

        let result = sqlx_query.execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    /// Query ticks by symbol and time range
    pub async fn get_ticks(
        &self,
        symbol: &str,
        exchange: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: Option<i64>,
    ) -> RepositoryResult<Vec<TickData>> {
        let limit = limit.unwrap_or(100_000);

        let rows = sqlx::query(
            r#"
            SELECT ts_event, ts_recv, symbol, exchange, price, size, side,
                   provider, provider_trade_id, is_buyer_maker, sequence
            FROM market_ticks
            WHERE symbol = $1 AND exchange = $2
              AND ts_event >= $3 AND ts_event < $4
            ORDER BY ts_event ASC, sequence ASC
            LIMIT $5
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .bind(start)
        .bind(end)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let ticks: Vec<TickData> = rows
            .iter()
            .map(|row| {
                let side_str: String = row.get("side");
                let side = TradeSide::from_db_char(side_str.chars().next().unwrap_or('B'))
                    .unwrap_or(TradeSide::Buy);

                TickData::with_details(
                    row.get("ts_event"),
                    row.get("ts_recv"),
                    row.get("symbol"),
                    row.get("exchange"),
                    row.get("price"),
                    row.get("size"),
                    side,
                    row.get("provider"),
                    row.get("provider_trade_id"),
                    row.get("is_buyer_maker"),
                    row.get("sequence"),
                )
            })
            .collect();

        Ok(ticks)
    }

    /// Get the latest tick for a symbol
    pub async fn get_latest_tick(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> RepositoryResult<Option<TickData>> {
        let ticks = self
            .get_ticks(
                symbol,
                exchange,
                Utc::now() - chrono::Duration::hours(24),
                Utc::now(),
                Some(1),
            )
            .await?;

        Ok(ticks.into_iter().last())
    }

    /// Get available symbols in the database
    pub async fn get_available_symbols(&self) -> RepositoryResult<Vec<(String, String)>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT symbol, exchange
            FROM market_ticks
            ORDER BY symbol, exchange
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let symbols: Vec<(String, String)> = rows
            .iter()
            .map(|row| (row.get("symbol"), row.get("exchange")))
            .collect();

        Ok(symbols)
    }

    /// Get data statistics for a symbol
    pub async fn get_symbol_stats(
        &self,
        symbol: &str,
        exchange: &str,
    ) -> RepositoryResult<SymbolStats> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total_records,
                MIN(ts_event) as earliest_time,
                MAX(ts_event) as latest_time,
                MIN(price) as min_price,
                MAX(price) as max_price
            FROM market_ticks
            WHERE symbol = $1 AND exchange = $2
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .fetch_one(&self.pool)
        .await?;

        Ok(SymbolStats {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            total_records: row.get::<i64, _>("total_records") as u64,
            earliest_time: row.get("earliest_time"),
            latest_time: row.get("latest_time"),
            min_price: row.get("min_price"),
            max_price: row.get("max_price"),
        })
    }

    // ========================================================================
    // L1 Quote Operations
    // ========================================================================

    /// Insert a single quote (L1 BBO)
    pub async fn insert_quote(&self, quote: &QuoteTick) -> RepositoryResult<()> {
        sqlx::query(
            r#"
            INSERT INTO quote_data (
                timestamp, symbol, exchange, bid_price, ask_price,
                bid_size, ask_size, sequence
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(quote.ts_event)
        .bind(&quote.symbol)
        .bind(&quote.exchange)
        .bind(quote.bid_price)
        .bind(quote.ask_price)
        .bind(quote.bid_size)
        .bind(quote.ask_size)
        .bind(quote.sequence as i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batch insert quotes
    pub async fn batch_insert_quotes(&self, quotes: &[QuoteTick]) -> RepositoryResult<usize> {
        if quotes.is_empty() {
            return Ok(0);
        }

        let mut total_inserted = 0;

        for chunk in quotes.chunks(self.batch_size) {
            let inserted = self.insert_quote_batch(chunk).await?;
            total_inserted += inserted;
        }

        debug!("Batch inserted {} quotes", total_inserted);
        Ok(total_inserted)
    }

    /// Insert a batch of quotes
    async fn insert_quote_batch(&self, quotes: &[QuoteTick]) -> RepositoryResult<usize> {
        let mut query = String::from(
            r#"
            INSERT INTO quote_data (
                timestamp, symbol, exchange, bid_price, ask_price,
                bid_size, ask_size, sequence
            ) VALUES
            "#,
        );

        let mut param_count = 1;

        for (i, _quote) in quotes.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }

            query.push_str(&format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                param_count,
                param_count + 1,
                param_count + 2,
                param_count + 3,
                param_count + 4,
                param_count + 5,
                param_count + 6,
                param_count + 7,
            ));
            param_count += 8;
        }

        query.push_str(" ON CONFLICT DO NOTHING");

        let mut sqlx_query = sqlx::query(&query);

        for quote in quotes {
            sqlx_query = sqlx_query
                .bind(quote.ts_event)
                .bind(&quote.symbol)
                .bind(&quote.exchange)
                .bind(quote.bid_price)
                .bind(quote.ask_price)
                .bind(quote.bid_size)
                .bind(quote.ask_size)
                .bind(quote.sequence as i64);
        }

        let result = sqlx_query.execute(&self.pool).await?;
        Ok(result.rows_affected() as usize)
    }

    /// Query quotes by symbol and time range
    pub async fn get_quotes(
        &self,
        symbol: &str,
        exchange: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: Option<i64>,
    ) -> RepositoryResult<Vec<QuoteTick>> {
        let limit = limit.unwrap_or(100_000);

        let rows = sqlx::query(
            r#"
            SELECT timestamp, symbol, exchange, bid_price, ask_price,
                   bid_size, ask_size, sequence
            FROM quote_data
            WHERE symbol = $1 AND exchange = $2
              AND timestamp >= $3 AND timestamp < $4
            ORDER BY timestamp ASC, sequence ASC
            LIMIT $5
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .bind(start)
        .bind(end)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let quotes: Vec<QuoteTick> = rows
            .iter()
            .map(|row| {
                QuoteTick::with_details(
                    row.get("timestamp"),
                    row.get("timestamp"),
                    row.get("symbol"),
                    row.get("exchange"),
                    row.get("bid_price"),
                    row.get("ask_price"),
                    row.get("bid_size"),
                    row.get("ask_size"),
                    row.get::<i64, _>("sequence") as u64,
                )
            })
            .collect();

        Ok(quotes)
    }

    // ========================================================================
    // L2 Order Book Operations
    // ========================================================================

    /// Insert order book snapshot (all levels)
    pub async fn insert_orderbook_snapshot(&self, book: &OrderBook) -> RepositoryResult<()> {
        let mut levels = Vec::new();

        // Collect bid levels
        for (depth, level) in book.all_bids().iter().enumerate() {
            levels.push((
                book.ts_event,
                &book.symbol,
                &book.exchange,
                "BID",
                level.price,
                level.size,
                level.order_count as i32,
                depth as i16,
                book.sequence as i64,
                true, // is_snapshot
            ));
        }

        // Collect ask levels
        for (depth, level) in book.all_asks().iter().enumerate() {
            levels.push((
                book.ts_event,
                &book.symbol,
                &book.exchange,
                "ASK",
                level.price,
                level.size,
                level.order_count as i32,
                depth as i16,
                book.sequence as i64,
                true,
            ));
        }

        if levels.is_empty() {
            return Ok(());
        }

        // Batch insert all levels
        self.insert_orderbook_levels(&levels).await
    }

    /// Insert order book delta (single level update)
    pub async fn insert_orderbook_delta(&self, delta: &OrderBookDelta) -> RepositoryResult<()> {
        let side = match delta.side {
            BookSide::Bid => "BID",
            BookSide::Ask => "ASK",
        };

        sqlx::query(
            r#"
            INSERT INTO orderbook_levels (
                timestamp, symbol, exchange, side, price, size,
                order_count, depth_position, sequence, is_snapshot
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
        )
        .bind(delta.ts_event)
        .bind(&delta.symbol)
        .bind(&delta.exchange)
        .bind(side)
        .bind(delta.price)
        .bind(delta.size)
        .bind(delta.order_count as i32)
        .bind(0i16) // depth_position not tracked for deltas
        .bind(delta.sequence as i64)
        .bind(false) // is_snapshot = false for deltas
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Batch insert order book levels
    async fn insert_orderbook_levels(
        &self,
        levels: &[(
            DateTime<Utc>,
            &String,
            &String,
            &str,
            Decimal,
            Decimal,
            i32,
            i16,
            i64,
            bool,
        )],
    ) -> RepositoryResult<()> {
        if levels.is_empty() {
            return Ok(());
        }

        // Build batch insert query
        let mut query = String::from(
            r#"
            INSERT INTO orderbook_levels (
                timestamp, symbol, exchange, side, price, size,
                order_count, depth_position, sequence, is_snapshot
            ) VALUES
            "#,
        );

        let mut param_count = 1;

        for (i, _) in levels.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }
            query.push_str(&format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                param_count,
                param_count + 1,
                param_count + 2,
                param_count + 3,
                param_count + 4,
                param_count + 5,
                param_count + 6,
                param_count + 7,
                param_count + 8,
                param_count + 9,
            ));
            param_count += 10;
        }

        let mut sqlx_query = sqlx::query(&query);

        for level in levels {
            sqlx_query = sqlx_query
                .bind(level.0)
                .bind(level.1)
                .bind(level.2)
                .bind(level.3)
                .bind(level.4)
                .bind(level.5)
                .bind(level.6)
                .bind(level.7)
                .bind(level.8)
                .bind(level.9);
        }

        sqlx_query.execute(&self.pool).await?;
        Ok(())
    }

    /// Get the latest order book snapshot for a symbol
    pub async fn get_latest_orderbook(
        &self,
        symbol: &str,
        exchange: &str,
        depth: usize,
    ) -> RepositoryResult<OrderBook> {
        // Find the latest snapshot timestamp
        let snapshot_row = sqlx::query(
            r#"
            SELECT MAX(timestamp) as latest_ts
            FROM orderbook_levels
            WHERE symbol = $1 AND exchange = $2 AND is_snapshot = TRUE
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .fetch_one(&self.pool)
        .await?;

        let latest_ts: Option<DateTime<Utc>> = snapshot_row.get("latest_ts");

        let Some(ts) = latest_ts else {
            return Ok(OrderBook::with_exchange(symbol, exchange));
        };

        // Fetch all levels for that snapshot
        let rows = sqlx::query(
            r#"
            SELECT side, price, size, order_count, depth_position, sequence
            FROM orderbook_levels
            WHERE symbol = $1 AND exchange = $2 AND timestamp = $3 AND is_snapshot = TRUE
            ORDER BY side, depth_position
            LIMIT $4
            "#,
        )
        .bind(symbol)
        .bind(exchange)
        .bind(ts)
        .bind((depth * 2) as i64) // Both sides
        .fetch_all(&self.pool)
        .await?;

        let mut book = OrderBook::with_exchange(symbol, exchange);
        book.set_timestamp(ts, rows.first().map(|r| r.get::<i64, _>("sequence") as u64).unwrap_or(0));

        for row in rows {
            let side: String = row.get("side");
            let price: Decimal = row.get("price");
            let size: Decimal = row.get("size");
            let order_count: i32 = row.get("order_count");

            match side.as_str() {
                "BID" => book.update_bid(price, size, order_count as u32),
                "ASK" => book.update_ask(price, size, order_count as u32),
                _ => {}
            }
        }

        Ok(book)
    }

    /// Get overall database statistics
    pub async fn get_database_stats(&self) -> RepositoryResult<DatabaseStats> {
        let row = sqlx::query(
            r#"
            SELECT
                COUNT(*) as total_records,
                COUNT(DISTINCT (symbol, exchange)) as total_symbols,
                MIN(ts_event) as earliest_time,
                MAX(ts_event) as latest_time
            FROM market_ticks
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        // Get hypertable size
        let size_row = sqlx::query(
            r#"
            SELECT pg_size_pretty(hypertable_size('market_ticks')) as size
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        let size_str = size_row
            .map(|r| r.get::<String, _>("size"))
            .unwrap_or_else(|| "unknown".to_string());

        Ok(DatabaseStats {
            total_records: row.get::<i64, _>("total_records") as u64,
            total_symbols: row.get::<i64, _>("total_symbols") as u64,
            earliest_time: row.get("earliest_time"),
            latest_time: row.get("latest_time"),
            total_size: size_str,
        })
    }
}

/// Statistics for a single symbol
#[derive(Debug, Clone)]
pub struct SymbolStats {
    pub symbol: String,
    pub exchange: String,
    pub total_records: u64,
    pub earliest_time: Option<DateTime<Utc>>,
    pub latest_time: Option<DateTime<Utc>>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
}

/// Overall database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    pub total_records: u64,
    pub total_symbols: u64,
    pub earliest_time: Option<DateTime<Utc>>,
    pub latest_time: Option<DateTime<Utc>>,
    pub total_size: String,
}

/// Data import job
#[derive(Debug, Clone)]
pub struct DataJob {
    pub id: Uuid,
    pub job_type: String,
    pub symbols: Vec<String>,
    pub status: JobStatus,
    pub progress_pct: i16,
    pub records_processed: i64,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Job status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(JobStatus::Pending),
            "running" => Some(JobStatus::Running),
            "completed" => Some(JobStatus::Completed),
            "failed" => Some(JobStatus::Failed),
            "cancelled" => Some(JobStatus::Cancelled),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_conversion() {
        assert_eq!(JobStatus::Pending.as_str(), "pending");
        assert_eq!(JobStatus::from_str("running"), Some(JobStatus::Running));
        assert_eq!(JobStatus::from_str("invalid"), None);
    }
}
