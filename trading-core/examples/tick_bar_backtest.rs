//! Example: N-Tick Bar Backtesting
//!
//! This example demonstrates:
//! - Generating N-tick OHLC bars from historical tick data
//! - Loading a Python strategy that uses tick bars
//! - Running a backtest with tick bars
//! - Analyzing results
//!
//! Usage:
//!   cargo run --example tick_bar_backtest

use chrono::{Duration, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use trading_common::backtest::Portfolio;
use trading_common::data::cache::{TickDataCache, TieredCache};
use trading_common::data::repository::TickDataRepository;
use trading_common::data::types::{BarData, BarMetadata, BarType, Timeframe};
use trading_common::orders::OrderSide;
use trading_common::series::bars_context::BarsContext;

#[tokio::main]
async fn main() -> Result<(), String> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== N-Tick Bar Backtest Example ===\n");

    // Load environment variables
    dotenv::dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set");

    // Connect to database
    println!("Connecting to database...");
    let pool = sqlx::PgPool::connect(&database_url)
        .await
        .map_err(|e| format!("Database connection failed: {}", e))?;

    // Initialize cache
    let cache = TieredCache::new((100, 300), (&redis_url, 1000, 3600))
        .await
        .map_err(|e| format!("Cache initialization failed: {}", e))?;

    // Create repository
    let cache: Arc<dyn TickDataCache> = Arc::new(cache);
    let repository = TickDataRepository::new(pool, cache);

    // Check available data
    println!("\nChecking available data...");
    let data_info = repository
        .get_backtest_data_info()
        .await
        .map_err(|e| format!("Failed to get backtest data info: {}", e))?;
    println!("Total records: {}", data_info.total_records);
    println!("Available symbols: {}", data_info.symbols_count);

    if data_info.symbols_count == 0 {
        println!("\n⚠️  No data available. Please run live data collection first:");
        println!("   cargo run live");
        return Ok(());
    }

    // Pick a symbol with data
    let symbol = data_info
        .symbol_info
        .first()
        .map(|s| s.symbol.clone())
        .unwrap_or_else(|| "BTCUSDT".to_string());

    println!("Using symbol: {}", symbol);

    // Define backtest time range
    let end_time = Utc::now();
    let start_time = end_time - Duration::hours(24); // Last 24 hours

    println!("\nBacktest period:");
    println!("  Start: {}", start_time);
    println!("  End: {}", end_time);

    // === Compare Time-Based vs Tick-Based Bars ===

    println!("\n--- Generating Bars ---");

    // 1. Generate time-based bars (1-minute)
    println!("\n1. Time-based bars (1-minute):");
    let time_bars = repository
        .generate_ohlc_from_ticks(&symbol, Timeframe::OneMinute, start_time, end_time, None)
        .await
        .map_err(|e| format!("Failed to generate time bars: {}", e))?;
    println!("   Generated {} 1-minute bars", time_bars.len());

    // 2. Generate 100-tick bars
    println!("\n2. Tick-based bars (100-tick):");
    let tick_bars_100 = repository
        .generate_n_tick_ohlc(&symbol, 100, start_time, end_time, None)
        .await
        .map_err(|e| format!("Failed to generate 100-tick bars: {}", e))?;
    println!("   Generated {} 100-tick bars", tick_bars_100.len());

    // 3. Generate 500-tick bars
    println!("\n3. Tick-based bars (500-tick):");
    let tick_bars_500 = repository
        .generate_n_tick_ohlc(&symbol, 500, start_time, end_time, None)
        .await
        .map_err(|e| format!("Failed to generate 500-tick bars: {}", e))?;
    println!("   Generated {} 500-tick bars", tick_bars_500.len());

    // Show some sample bars
    if !tick_bars_100.is_empty() {
        println!("\n--- Sample 100-Tick Bars ---");
        for (i, bar) in tick_bars_100.iter().take(5).enumerate() {
            println!(
                "Bar {}: O={:.2} H={:.2} L={:.2} C={:.2} V={:.4} Ticks={}",
                i, bar.open, bar.high, bar.low, bar.close, bar.volume, bar.trade_count
            );
        }
    }

    // === Run Backtest with Python Tick Bar Strategy ===

    println!("\n--- Loading Python Tick Bar Strategy ---");

    let strategy_name = "example_tick_bar_strategy";

    // Check if strategy file exists
    let strategy_path = format!("strategies/examples/{}.py", strategy_name);
    if !std::path::Path::new(&strategy_path).exists() {
        println!("\n⚠️  Strategy file not found: {}", strategy_path);
        println!("The example strategy should be in the strategies/examples/ directory.");
        return Ok(());
    }

    let mut strategy = trading_common::backtest::create_strategy(strategy_name).map_err(|e| {
        format!(
            "Failed to load strategy: {}\n\
            Make sure:\n\
              1. RestrictedPython is installed: pip install RestrictedPython==7.0\n\
              2. strategies/_lib/restricted_compiler.py exists\n\
              3. strategies/_lib/base_strategy.py exists\n\
              4. Strategy file exists at: {}",
            e, strategy_path
        )
    })?;

    println!("✓ Loaded strategy: {}", strategy.name());

    // Initialize strategy with parameters
    println!("\nInitializing strategy with parameters...");
    let mut params = HashMap::new();
    params.insert("tick_count".to_string(), "100".to_string());
    params.insert("lookback_bars".to_string(), "20".to_string());
    params.insert("momentum_threshold".to_string(), "0.002".to_string());
    params.insert("volume_multiplier".to_string(), "1.5".to_string());

    strategy
        .initialize(params)
        .map_err(|e| format!("Strategy initialization failed: {}", e))?;

    println!("✓ Strategy initialized");

    // Run backtest if we have enough bars
    if tick_bars_100.len() < 20 {
        println!(
            "\n⚠️  Not enough bars for backtest (have: {}, need: 20)",
            tick_bars_100.len()
        );
        println!("Please collect more data by running: cargo run live");
        return Ok(());
    }

    println!("\n--- Running Backtest on {} Bars ---", tick_bars_100.len());

    let initial_capital = Decimal::from_str("10000.0").unwrap();
    let mut portfolio = Portfolio::new(initial_capital);
    let mut trade_count = 0;

    // Create BarsContext for the strategy
    let mut bars_context = BarsContext::new(&symbol);

    for (i, bar) in tick_bars_100.iter().enumerate() {
        // Update portfolio with latest price
        portfolio.update_price(&bar.symbol, bar.close);

        // Create BarData for strategy
        let bar_data = BarData {
            current_tick: None,
            ohlc_bar: bar.clone(),
            metadata: BarMetadata {
                bar_type: BarType::TickBased(100),
                is_first_tick_of_bar: false,
                is_bar_closed: true,
                tick_count_in_bar: bar.trade_count as u64,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        };

        // Update BarsContext with new bar data
        bars_context.on_bar_update(&bar_data);

        // Process bar data with strategy
        strategy.on_bar_data(&bar_data, &mut bars_context);

        // Get orders from strategy
        let orders = strategy.get_orders(&bar_data, &mut bars_context);

        // Execute orders
        for order in orders {
            match order.side {
                OrderSide::Buy => {
                    println!(
                        "\n📈 Bar {}: BUY {} @ {} (Volume: {:.4}, Ticks: {})",
                        i,
                        order.symbol(),
                        bar.close,
                        bar.volume,
                        bar.trade_count
                    );
                    if let Err(e) =
                        portfolio.execute_buy(order.symbol().to_string(), order.quantity, bar.close)
                    {
                        println!("   ⚠️  Buy failed: {}", e);
                    } else {
                        trade_count += 1;
                    }
                }
                OrderSide::Sell => {
                    println!(
                        "\n📉 Bar {}: SELL {} @ {} (Volume: {:.4}, Ticks: {})",
                        i,
                        order.symbol(),
                        bar.close,
                        bar.volume,
                        bar.trade_count
                    );
                    if let Err(e) = portfolio.execute_sell(
                        order.symbol().to_string(),
                        order.quantity,
                        bar.close,
                    ) {
                        println!("   ⚠️  Sell failed: {}", e);
                    } else {
                        trade_count += 1;
                    }
                }
            }
        }

        // Print portfolio value every 50 bars
        if i > 0 && i % 50 == 0 {
            println!(
                "Bar {}: Portfolio Value = ${:.2}",
                i,
                portfolio.total_value()
            );
        }
    }

    // Calculate and display results
    println!("\n=== Backtest Results ===");
    println!("Symbol: {}", symbol);
    println!("Bars Processed: {}", tick_bars_100.len());
    println!("Trades Executed: {}", trade_count);
    println!();
    println!("Initial Capital: ${:.2}", initial_capital);
    println!("Final Value: ${:.2}", portfolio.total_value());
    println!("Total P&L: ${:.2}", portfolio.total_pnl());
    println!("Total Realized P&L: ${:.2}", portfolio.total_realized_pnl());
    println!(
        "Total Unrealized P&L: ${:.2}",
        portfolio.total_unrealized_pnl()
    );
    println!("Total Commission: ${:.2}", portfolio.total_commission());
    println!(
        "Return: {:.2}%",
        ((portfolio.total_value() - initial_capital) / initial_capital) * Decimal::from(100)
    );

    println!("\n✓ Backtest complete!");

    Ok(())
}
