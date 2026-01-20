use chrono::{Duration, Timelike, Utc};
use rust_decimal::Decimal;
use std::str::FromStr;
use trading_common::backtest::bar_generator::HistoricalOHLCGenerator;
use trading_common::data::types::{BarDataMode, BarType, TickData, Timeframe, TradeSide};

fn create_tick(price: &str, timestamp_offset_seconds: i64) -> TickData {
    let base_time = Utc::now()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap();

    TickData {
        timestamp: base_time + Duration::seconds(timestamp_offset_seconds),
        symbol: "BTCUSDT".to_string(),
        price: Decimal::from_str(price).unwrap(),
        quantity: Decimal::from_str("1.0").unwrap(),
        side: TradeSide::Buy,
        trade_id: format!("test_{}", timestamp_offset_seconds),
        is_buyer_maker: false,
    }
}

#[test]
fn test_synthetic_bars_in_historical_backtest() {
    // Create generator for 1-minute bars in OnCloseBar mode
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnCloseBar,
    );

    // Create ticks with a gap:
    // Tick at 0s (minute 0)
    // Tick at 30s (minute 0)
    // GAP: no ticks in minute 1, 2, 3
    // Tick at 240s (minute 4)
    let ticks = vec![
        create_tick("50000", 0),    // 00:00
        create_tick("50100", 30),   // 00:30
        create_tick("50200", 240),  // 04:00 (4 minutes later!)
    ];

    let bars = gen.generate_from_ticks(&ticks);

    // Expected bars:
    // Bar 0: minute 0 (real) - has ticks at 0s and 30s
    // Bar 1: minute 1 (synthetic) - no ticks, O=H=L=C=50100
    // Bar 2: minute 2 (synthetic) - no ticks, O=H=L=C=50100
    // Bar 3: minute 3 (synthetic) - no ticks, O=H=L=C=50100
    // Bar 4: minute 4 (real) - has tick at 240s

    assert_eq!(bars.len(), 5, "Expected 5 bars: 1 real + 3 synthetic + 1 real");

    // Bar 0: Real bar from minute 0
    assert_eq!(bars[0].metadata.is_synthetic, false);
    assert_eq!(bars[0].metadata.is_bar_closed, true);
    assert_eq!(bars[0].ohlc_bar.open, Decimal::from_str("50000").unwrap());
    assert_eq!(bars[0].ohlc_bar.close, Decimal::from_str("50100").unwrap());

    // Bar 1: Synthetic bar for minute 1
    assert_eq!(bars[1].metadata.is_synthetic, true);
    assert_eq!(bars[1].metadata.tick_count_in_bar, 0);
    assert_eq!(bars[1].ohlc_bar.open, Decimal::from_str("50100").unwrap());
    assert_eq!(bars[1].ohlc_bar.high, Decimal::from_str("50100").unwrap());
    assert_eq!(bars[1].ohlc_bar.low, Decimal::from_str("50100").unwrap());
    assert_eq!(bars[1].ohlc_bar.close, Decimal::from_str("50100").unwrap());

    // Bar 2: Synthetic bar for minute 2
    assert_eq!(bars[2].metadata.is_synthetic, true);
    assert_eq!(bars[2].metadata.tick_count_in_bar, 0);
    assert_eq!(bars[2].ohlc_bar.open, Decimal::from_str("50100").unwrap());

    // Bar 3: Synthetic bar for minute 3
    assert_eq!(bars[3].metadata.is_synthetic, true);
    assert_eq!(bars[3].metadata.tick_count_in_bar, 0);
    assert_eq!(bars[3].ohlc_bar.open, Decimal::from_str("50100").unwrap());

    // Bar 4: Real bar from minute 4
    assert_eq!(bars[4].metadata.is_synthetic, false);
    assert_eq!(bars[4].metadata.is_bar_closed, true);
    assert_eq!(bars[4].ohlc_bar.open, Decimal::from_str("50200").unwrap());
    assert_eq!(bars[4].ohlc_bar.close, Decimal::from_str("50200").unwrap());
}

#[test]
fn test_no_synthetic_bars_without_gaps() {
    let gen = HistoricalOHLCGenerator::new(
        BarType::TimeBased(Timeframe::OneMinute),
        BarDataMode::OnCloseBar,
    );

    // Create ticks with NO gaps (consecutive minutes)
    let ticks = vec![
        create_tick("50000", 0),    // minute 0
        create_tick("50100", 60),   // minute 1
        create_tick("50200", 120),  // minute 2
    ];

    let bars = gen.generate_from_ticks(&ticks);

    // Should have exactly 3 real bars, no synthetic
    assert_eq!(bars.len(), 3);
    assert_eq!(bars[0].metadata.is_synthetic, false);
    assert_eq!(bars[1].metadata.is_synthetic, false);
    assert_eq!(bars[2].metadata.is_synthetic, false);
}
