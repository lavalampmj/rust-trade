# N-Tick Bar Usage Guide

This guide shows how to use N-tick OHLC bars in your trading strategies.

## What are N-Tick Bars?

Unlike time-based bars (e.g., 1-minute, 5-minute candles) that represent a fixed time period, **N-tick bars** represent a fixed number of trades. For example:
- **100-tick bar**: Each bar contains exactly 100 trades
- **500-tick bar**: Each bar contains exactly 500 trades

### Advantages of Tick Bars

1. **Activity-Based Sampling**: More bars during high market activity, fewer during quiet periods
2. **Consistent Statistics**: Each bar has the same number of data points (trades)
3. **Eliminate Time Bias**: No assumptions about time-based patterns
4. **Better for Volume Analysis**: Volume per bar is more meaningful
5. **High-Frequency Friendly**: Better for short-term trading strategies

## Using N-Tick Bars in Rust

### 1. Generate N-Tick Bars from Repository

```rust
use trading_common::data::repository::TickDataRepository;
use chrono::{Utc, Duration};

async fn generate_tick_bars(
    repository: &TickDataRepository,
) -> Result<Vec<OHLCData>, DataError> {
    let symbol = "BTCUSDT";
    let tick_count = 100; // 100 ticks per bar
    let start_time = Utc::now() - Duration::hours(24);
    let end_time = Utc::now();

    // Generate 100-tick bars for the last 24 hours
    let tick_bars = repository
        .generate_n_tick_ohlc(
            symbol,
            tick_count,
            start_time,
            end_time,
            None, // No limit on ticks fetched
        )
        .await?;

    println!("Generated {} 100-tick bars", tick_bars.len());

    // Each bar (except possibly the last) will have trade_count = 100
    for (i, bar) in tick_bars.iter().enumerate() {
        println!(
            "Bar {}: O={} H={} L={} C={} V={} Ticks={}",
            i, bar.open, bar.high, bar.low, bar.close,
            bar.volume, bar.trade_count
        );
    }

    Ok(tick_bars)
}
```

### 2. Use with In-Memory Tick Data

```rust
use trading_common::data::types::{OHLCData, TickData};

fn create_tick_bars_from_ticks(ticks: &[TickData], tick_count: u32) -> Vec<OHLCData> {
    // Convert raw tick data to N-tick bars
    let bars = OHLCData::from_ticks_n_tick(ticks, tick_count);

    println!(
        "Created {} bars from {} ticks ({}T bars)",
        bars.len(),
        ticks.len(),
        tick_count
    );

    bars
}

// Example: Create 50-tick bars
let ticks: Vec<TickData> = /* your tick data */;
let tick_bars_50 = create_tick_bars_from_ticks(&ticks, 50);

// Example: Create 500-tick bars
let tick_bars_500 = create_tick_bars_from_ticks(&ticks, 500);
```

### 3. Backtest with Python Strategy

```rust
use trading_common::backtest::strategy::python_bridge::PythonStrategy;
use trading_common::backtest::engine::BacktestEngine;
use trading_common::backtest::portfolio::Portfolio;
use std::collections::HashMap;
use rust_decimal::Decimal;

async fn run_tick_bar_backtest(
    repository: &TickDataRepository,
) -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Load the Python tick bar strategy
    let mut strategy = PythonStrategy::from_file(
        "strategies/example_tick_bar_strategy.py",
        "TickBarStrategy",
    )?;

    // Step 2: Initialize strategy with parameters
    let mut params = HashMap::new();
    params.insert("tick_count".to_string(), "100".to_string());
    params.insert("lookback_bars".to_string(), "20".to_string());
    params.insert("momentum_threshold".to_string(), "0.002".to_string());
    params.insert("volume_multiplier".to_string(), "1.5".to_string());

    strategy.initialize(params)?;

    // Step 3: Generate 100-tick bars
    let symbol = "BTCUSDT";
    let start_time = Utc::now() - Duration::days(7);
    let end_time = Utc::now();

    let tick_bars = repository
        .generate_n_tick_ohlc(symbol, 100, start_time, end_time, None)
        .await?;

    println!("Running backtest on {} 100-tick bars", tick_bars.len());

    // Step 4: Create backtest engine
    let initial_capital = Decimal::from_str("10000.0")?;
    let mut portfolio = Portfolio::new(initial_capital);

    // Step 5: Process each bar through strategy
    for (i, bar) in tick_bars.iter().enumerate() {
        // Update portfolio with latest price
        portfolio.update_price(&bar.symbol, bar.close);

        // Generate signal
        let signal = strategy.on_ohlc(bar);

        // Execute signal
        match signal {
            Signal::Buy { symbol, quantity } => {
                println!("Bar {}: BUY {} @ {}", i, symbol, bar.close);
                portfolio.execute_buy(&symbol, quantity, bar.close)?;
            }
            Signal::Sell { symbol, quantity } => {
                println!("Bar {}: SELL {} @ {}", i, symbol, bar.close);
                portfolio.execute_sell(&symbol, quantity, bar.close)?;
            }
            Signal::Hold => {}
        }
    }

    // Step 6: Print results
    let metrics = portfolio.calculate_metrics();
    println!("\n=== Backtest Results ===");
    println!("Initial Capital: ${}", initial_capital);
    println!("Final Value: ${}", portfolio.total_value());
    println!("Total P&L: ${}", metrics.total_pnl);
    println!("Return: {:.2}%", metrics.return_pct);
    println!("Sharpe Ratio: {:.2}", metrics.sharpe_ratio);
    println!("Max Drawdown: {:.2}%", metrics.max_drawdown_pct);
    println!("Win Rate: {:.2}%", metrics.win_rate);

    Ok(())
}
```

## Using N-Tick Bars in Python Strategies

See the complete example in `strategies/example_tick_bar_strategy.py`.

### Key Points for Python Developers

1. **Implement `on_ohlc()` method** - N-tick bars are delivered as OHLC data
2. **Use `trade_count` field** - This tells you how many ticks are in each bar
3. **Set `supports_ohlc()` to `True`** - Required for OHLC-based strategies

```python
from base_strategy import BaseStrategy, Signal

class MyTickBarStrategy(BaseStrategy):
    def on_ohlc(self, ohlc):
        """
        ohlc dictionary contains:
        - timestamp: First tick's timestamp in the bar
        - symbol: Trading pair
        - open: First tick's price
        - high: Highest price in N ticks
        - low: Lowest price in N ticks
        - close: Last tick's price
        - volume: Total volume of N ticks
        - trade_count: Number of ticks (e.g., 100 for 100-tick bars)
        """
        trade_count = ohlc["trade_count"]

        # Verify you're getting N-tick bars
        print(f"Processing bar with {trade_count} ticks")

        # Your strategy logic here...
        return Signal.hold()

    def supports_ohlc(self):
        return True
```

## Common N-Tick Bar Configurations

### High-Frequency Trading
```rust
// 50-tick bars: Very active, many bars per day
let hft_bars = repository.generate_n_tick_ohlc(
    "BTCUSDT", 50, start, end, None
).await?;
```

### Scalping
```rust
// 100-tick bars: Good balance for scalping strategies
let scalp_bars = repository.generate_n_tick_ohlc(
    "BTCUSDT", 100, start, end, None
).await?;
```

### Day Trading
```rust
// 500-tick bars: Medium frequency
let day_bars = repository.generate_n_tick_ohlc(
    "BTCUSDT", 500, start, end, None
).await?;
```

### Swing Trading
```rust
// 1000-tick bars: Lower frequency, fewer bars
let swing_bars = repository.generate_n_tick_ohlc(
    "BTCUSDT", 1000, start, end, None
).await?;
```

## Comparing Tick Bars vs Time Bars

```rust
// Time-based: 1-minute bars (fixed time intervals)
let time_bars = repository.generate_ohlc_from_ticks(
    "BTCUSDT",
    Timeframe::OneMinute,
    start,
    end,
    None,
).await?;

// Tick-based: 100-tick bars (fixed number of trades)
let tick_bars = repository.generate_n_tick_ohlc(
    "BTCUSDT",
    100,
    start,
    end,
    None,
).await?;

println!("Time-based bars: {}", time_bars.len());
println!("Tick-based bars: {}", tick_bars.len());

// During high volatility: tick_bars.len() > time_bars.len()
// During low volatility: tick_bars.len() < time_bars.len()
```

## Performance Considerations

### Database Query Optimization

```rust
// Limit the number of ticks fetched if needed
let limit = Some(100_000); // Fetch max 100k ticks

let tick_bars = repository.generate_n_tick_ohlc(
    "BTCUSDT",
    100,
    start,
    end,
    limit,
).await?;

// This will create up to 1000 bars (100k ticks / 100 ticks per bar)
```

### Memory Efficiency

```rust
// For large datasets, process in chunks
let chunk_duration = Duration::hours(1);
let mut all_bars = Vec::new();

let mut current_start = start_time;
while current_start < end_time {
    let chunk_end = (current_start + chunk_duration).min(end_time);

    let chunk_bars = repository.generate_n_tick_ohlc(
        "BTCUSDT",
        100,
        current_start,
        chunk_end,
        Some(10_000),
    ).await?;

    all_bars.extend(chunk_bars);
    current_start = chunk_end;
}
```

## Testing Your Tick Bar Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tick_bar_generation() {
        let repo = create_test_repository().await;

        // Generate 10-tick bars for testing
        let bars = repo.generate_n_tick_ohlc(
            "TESTUSDT",
            10,
            test_start_time(),
            test_end_time(),
            None,
        ).await.unwrap();

        // Verify bar properties
        for (i, bar) in bars.iter().enumerate() {
            // Each bar should have exactly 10 ticks (except possibly last)
            if i < bars.len() - 1 {
                assert_eq!(bar.trade_count, 10);
            }

            // OHLC relationships should hold
            assert!(bar.high >= bar.open);
            assert!(bar.high >= bar.close);
            assert!(bar.low <= bar.open);
            assert!(bar.low <= bar.close);
        }
    }
}
```

## Next Steps: Volume-Based Bars (Future Enhancement)

The architecture supports future implementation of volume-based bars:

```rust
// Future: Volume-based bars (not yet implemented)
// let volume_bars = repository.generate_volume_based_ohlc(
//     "BTCUSDT",
//     Decimal::from_str("1000.0")?, // 1000 units of volume per bar
//     start,
//     end,
//     None,
// ).await?;
```

## References

- N-Tick Bar Implementation: `trading-common/src/data/types.rs:462-524`
- Repository Method: `trading-common/src/data/repository.rs:652-706`
- Python Example Strategy: `strategies/example_tick_bar_strategy.py`
- Base Strategy Interface: `strategies/base_strategy.py`

For questions or issues, see the main README.md or check the test suite in `trading-common/src/data/types.rs`.
