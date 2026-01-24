//! OHLC Generation Benchmark
//!
//! Benchmarks the full pipeline from database storage to OHLC generation:
//! - Store 1,000,000 ticks (100,000 ticks/day × 10 days)
//! - Load ticks from TimescaleDB
//! - Generate 1000 tick bars (1000 ticks per bar)
//! - Measure total time from request to OHLCV series creation

use chrono::{DateTime, Duration, Utc};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use dotenv::dotenv;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::time::Instant;
use tokio::runtime::Runtime;
use trading_common::data::types::{OHLCData, TickData, TradeSide};

const TICKS_PER_DAY: usize = 100_000;
const NUM_DAYS: usize = 10;
const TOTAL_TICKS: usize = TICKS_PER_DAY * NUM_DAYS; // 1,000,000
const TICKS_PER_BAR: u32 = 1000;
const EXPECTED_BARS: usize = TOTAL_TICKS / TICKS_PER_BAR as usize; // 1000 bars

const BENCH_SYMBOL: &str = "OHLC_BENCH_TEST";

/// Create a synthetic tick with realistic price movement
fn create_tick(
    symbol: &str,
    base_price: Decimal,
    tick_index: usize,
    day_offset: usize,
    intraday_offset: usize,
) -> TickData {
    // Simulate price movement: random walk with slight trend
    let price_offset = Decimal::from(((tick_index * 7 + day_offset * 13) % 1000) as i64 - 500)
        / Decimal::from(100);
    let price = base_price + price_offset;

    // Distribute ticks evenly across the day (86400 seconds)
    let seconds_per_tick = 86400.0 / TICKS_PER_DAY as f64;
    let timestamp = Utc::now() - Duration::days(NUM_DAYS as i64 - day_offset as i64)
        + Duration::milliseconds((intraday_offset as f64 * seconds_per_tick * 1000.0) as i64);

    // Alternate buy/sell
    let side = if tick_index % 2 == 0 {
        TradeSide::Buy
    } else {
        TradeSide::Sell
    };

    // Varying quantity
    let quantity = Decimal::from(1) + Decimal::from((tick_index % 10) as i64) / Decimal::from(10);

    TickData::with_details(
        timestamp,
        timestamp,
        symbol.to_string(),
        "BENCH".to_string(),
        price,
        quantity,
        side,
        "BENCHMARK".to_string(),
        format!("bench_{}_{}", day_offset, intraday_offset),
        tick_index % 2 == 0,
        (day_offset * TICKS_PER_DAY + intraday_offset) as i64,
    )
}

/// Generate all test ticks
fn generate_test_ticks(symbol: &str) -> Vec<TickData> {
    let base_price = Decimal::from(50000);
    let mut ticks = Vec::with_capacity(TOTAL_TICKS);

    for day in 0..NUM_DAYS {
        for tick_idx in 0..TICKS_PER_DAY {
            ticks.push(create_tick(symbol, base_price, tick_idx, day, tick_idx));
        }
    }

    // Sort by timestamp (important for OHLC generation)
    ticks.sort_by_key(|t| t.timestamp);
    ticks
}

async fn setup_pool() -> PgPool {
    dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to database")
}

async fn cleanup_test_data(pool: &PgPool, symbol: &str) {
    sqlx::query("DELETE FROM market_ticks WHERE symbol = $1")
        .bind(symbol)
        .execute(pool)
        .await
        .expect("Failed to cleanup test data");
}

async fn insert_ticks_batch(pool: &PgPool, ticks: &[TickData]) -> usize {
    const BATCH_SIZE: usize = 5000;
    let mut total_inserted = 0;

    for chunk in ticks.chunks(BATCH_SIZE) {
        let mut query = String::from(
            "INSERT INTO market_ticks (ts_event, ts_recv, symbol, exchange, price, size, side, provider, provider_trade_id, is_buyer_maker, sequence) VALUES ",
        );

        let mut params: Vec<String> = Vec::new();
        for (i, _) in chunk.iter().enumerate() {
            let base = i * 11;
            params.push(format!(
                "(${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${}, ${})",
                base + 1,
                base + 2,
                base + 3,
                base + 4,
                base + 5,
                base + 6,
                base + 7,
                base + 8,
                base + 9,
                base + 10,
                base + 11
            ));
        }
        query.push_str(&params.join(", "));
        query.push_str(" ON CONFLICT DO NOTHING");

        let mut sqlx_query = sqlx::query(&query);
        for tick in chunk {
            sqlx_query = sqlx_query
                .bind(tick.timestamp)
                .bind(tick.ts_recv)
                .bind(&tick.symbol)
                .bind(&tick.exchange)
                .bind(tick.price)
                .bind(tick.quantity)
                .bind(tick.side.as_db_str())
                .bind(&tick.provider)
                .bind(&tick.trade_id)
                .bind(tick.is_buyer_maker)
                .bind(tick.sequence);
        }

        let result = sqlx_query
            .execute(pool)
            .await
            .expect("Failed to insert batch");
        total_inserted += result.rows_affected() as usize;
    }

    total_inserted
}

async fn load_ticks_from_db(
    pool: &PgPool,
    symbol: &str,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
) -> Vec<TickData> {
    let rows = sqlx::query_as::<_, (DateTime<Utc>, DateTime<Utc>, String, String, Decimal, Decimal, String, String, String, bool, i64)>(
        r#"
        SELECT ts_event, ts_recv, symbol, exchange, price, size, side, provider, provider_trade_id, is_buyer_maker, sequence
        FROM market_ticks
        WHERE symbol = $1 AND ts_event >= $2 AND ts_event <= $3
        ORDER BY ts_event ASC
        "#,
    )
    .bind(symbol)
    .bind(start_time)
    .bind(end_time)
    .fetch_all(pool)
    .await
    .expect("Failed to load ticks");

    rows.into_iter()
        .map(
            |(
                ts_event,
                ts_recv,
                symbol,
                exchange,
                price,
                size,
                side,
                provider,
                trade_id,
                is_buyer_maker,
                sequence,
            )| {
                TickData::with_details(
                    ts_event,
                    ts_recv,
                    symbol,
                    exchange,
                    price,
                    size,
                    TradeSide::from_db_str(&side).unwrap_or(TradeSide::Buy),
                    provider,
                    trade_id,
                    is_buyer_maker,
                    sequence,
                )
            },
        )
        .collect()
}

/// Generate N-tick OHLC bars from ticks
fn generate_tick_bars(ticks: &[TickData], ticks_per_bar: u32) -> Vec<OHLCData> {
    OHLCData::from_ticks_n_tick(ticks, ticks_per_bar)
}

fn ohlc_pipeline_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    // Setup
    let pool = rt.block_on(setup_pool());
    rt.block_on(cleanup_test_data(&pool, BENCH_SYMBOL));

    println!("\n=== OHLC Pipeline Benchmark ===");
    println!("Configuration:");
    println!("  - Ticks per day: {}", TICKS_PER_DAY);
    println!("  - Number of days: {}", NUM_DAYS);
    println!("  - Total ticks: {}", TOTAL_TICKS);
    println!("  - Ticks per bar: {}", TICKS_PER_BAR);
    println!("  - Expected bars: {}", EXPECTED_BARS);
    println!();

    // Generate test data
    println!("Generating {} test ticks...", TOTAL_TICKS);
    let gen_start = Instant::now();
    let ticks = generate_test_ticks(BENCH_SYMBOL);
    let gen_duration = gen_start.elapsed();
    println!("  Generated in {:?}", gen_duration);

    // Get time range
    let start_time = ticks.first().unwrap().timestamp;
    let end_time = ticks.last().unwrap().timestamp;

    // Insert all ticks
    println!("\nInserting {} ticks into TimescaleDB...", TOTAL_TICKS);
    let insert_start = Instant::now();
    let inserted = rt.block_on(insert_ticks_batch(&pool, &ticks));
    let insert_duration = insert_start.elapsed();
    println!("  Inserted {} ticks in {:?}", inserted, insert_duration);
    println!(
        "  Insert rate: {:.0} ticks/sec",
        inserted as f64 / insert_duration.as_secs_f64()
    );

    // Benchmark: Full pipeline (load + generate OHLC)
    let mut group = c.benchmark_group("ohlc_pipeline");
    group.sample_size(10); // Fewer samples for this heavy benchmark

    group.bench_function(
        BenchmarkId::new(
            "full_pipeline",
            format!("{}ticks_{}bars", TOTAL_TICKS, EXPECTED_BARS),
        ),
        |b| {
            b.to_async(&rt).iter(|| async {
                // Step 1: Load ticks from database
                let loaded_ticks =
                    load_ticks_from_db(&pool, BENCH_SYMBOL, start_time, end_time).await;

                // Step 2: Generate OHLC bars
                let bars = generate_tick_bars(black_box(&loaded_ticks), TICKS_PER_BAR);

                black_box(bars)
            });
        },
    );

    // Benchmark: Load only
    group.bench_function(
        BenchmarkId::new("db_load_only", format!("{}ticks", TOTAL_TICKS)),
        |b| {
            b.to_async(&rt).iter(|| async {
                let loaded_ticks =
                    load_ticks_from_db(&pool, BENCH_SYMBOL, start_time, end_time).await;
                black_box(loaded_ticks)
            });
        },
    );

    // Benchmark: OHLC generation only (from pre-loaded ticks)
    let preloaded_ticks = rt.block_on(load_ticks_from_db(
        &pool,
        BENCH_SYMBOL,
        start_time,
        end_time,
    ));
    println!(
        "\nPre-loaded {} ticks for OHLC-only benchmark",
        preloaded_ticks.len()
    );

    group.bench_function(
        BenchmarkId::new(
            "ohlc_gen_only",
            format!("{}ticks_to_{}bars", TOTAL_TICKS, EXPECTED_BARS),
        ),
        |b| {
            b.iter(|| {
                let bars = generate_tick_bars(black_box(&preloaded_ticks), TICKS_PER_BAR);
                black_box(bars)
            });
        },
    );

    group.finish();

    // Manual timing for detailed report
    println!("\n=== Detailed Timing Report ===");

    // Time the full pipeline manually
    let pipeline_start = Instant::now();
    let loaded = rt.block_on(load_ticks_from_db(
        &pool,
        BENCH_SYMBOL,
        start_time,
        end_time,
    ));
    let load_time = pipeline_start.elapsed();

    let ohlc_start = Instant::now();
    let bars = generate_tick_bars(&loaded, TICKS_PER_BAR);
    let ohlc_time = ohlc_start.elapsed();

    let total_time = pipeline_start.elapsed();

    println!("1. Database Load: {:?}", load_time);
    println!("   - Ticks loaded: {}", loaded.len());
    println!(
        "   - Load rate: {:.0} ticks/sec",
        loaded.len() as f64 / load_time.as_secs_f64()
    );
    println!();
    println!("2. OHLC Generation: {:?}", ohlc_time);
    println!("   - Bars generated: {}", bars.len());
    println!(
        "   - Generation rate: {:.0} bars/sec",
        bars.len() as f64 / ohlc_time.as_secs_f64()
    );
    println!();
    println!("3. TOTAL PIPELINE: {:?}", total_time);
    println!(
        "   - End-to-end rate: {:.0} bars/sec",
        bars.len() as f64 / total_time.as_secs_f64()
    );

    // Verify bars
    if !bars.is_empty() {
        println!();
        println!("Sample bar (first):");
        println!("  - Timestamp: {}", bars[0].timestamp);
        println!("  - Open: {}", bars[0].open);
        println!("  - High: {}", bars[0].high);
        println!("  - Low: {}", bars[0].low);
        println!("  - Close: {}", bars[0].close);
        println!("  - Volume: {}", bars[0].volume);
        println!("  - Trade count: {}", bars[0].trade_count);
    }

    // Cleanup
    println!("\nCleaning up test data...");
    rt.block_on(cleanup_test_data(&pool, BENCH_SYMBOL));
    println!("Done!");
}

criterion_group! {
    name = benches;
    config = Criterion::default().measurement_time(std::time::Duration::from_secs(30));
    targets = ohlc_pipeline_benchmark
}
criterion_main!(benches);
