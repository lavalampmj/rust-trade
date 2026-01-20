// paper_trading_tests.rs - Tests for PaperTradingProcessor

use std::collections::HashMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use std::str::FromStr;

use super::PaperTradingProcessor;
use trading_common::backtest::strategy::{Signal, Strategy};
use trading_common::data::types::{BarData, TickData, TradeSide};
use trading_common::data::{cache::TieredCache, repository::TickDataRepository};
use trading_common::series::bars_context::BarsContext;

use chrono::Utc;

// ============================================================================
// Mock Strategy Implementation
// ============================================================================

struct MockStrategy {
    name: String,
    signals: Vec<Signal>,
    current_index: usize,
    last_tick_id: Option<String>, // Track last processed tick to avoid duplicate signals
}

impl MockStrategy {
    fn new(name: &str, signals: Vec<Signal>) -> Self {
        Self {
            name: name.to_string(),
            signals,
            current_index: 0,
            last_tick_id: None,
        }
    }

    fn always_hold() -> Self {
        Self::new("AlwaysHold", vec![Signal::Hold])
    }

    fn buy_once(symbol: &str, quantity: Decimal) -> Self {
        Self::new(
            "BuyOnce",
            vec![Signal::Buy {
                symbol: symbol.to_string(),
                quantity,
            }],
        )
    }

    fn sell_once(symbol: &str, quantity: Decimal) -> Self {
        Self::new(
            "SellOnce",
            vec![Signal::Sell {
                symbol: symbol.to_string(),
                quantity,
            }],
        )
    }

    fn buy_then_sell(symbol: &str, buy_qty: Decimal, sell_qty: Decimal) -> Self {
        Self::new(
            "BuyThenSell",
            vec![
                Signal::Buy {
                    symbol: symbol.to_string(),
                    quantity: buy_qty,
                },
                Signal::Sell {
                    symbol: symbol.to_string(),
                    quantity: sell_qty,
                },
            ],
        )
    }
}

impl Strategy for MockStrategy {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_ready(&self, _bars: &BarsContext) -> bool {
        // Mock strategy is always ready (no warmup needed)
        true
    }

    fn warmup_period(&self) -> usize {
        0
    }

    fn on_bar_data(&mut self, bar_data: &BarData, _bars: &mut BarsContext) -> Signal {
        // Only advance signal on new ticks (not on bar close events or duplicate tick events)
        if let Some(ref tick) = bar_data.current_tick {
            // Check if this is a new tick
            let is_new_tick = self.last_tick_id.as_ref() != Some(&tick.trade_id);

            if is_new_tick {
                self.last_tick_id = Some(tick.trade_id.clone());

                if self.current_index < self.signals.len() {
                    let signal = self.signals[self.current_index].clone();
                    self.current_index += 1;
                    return signal;
                } else {
                    // Exhausted signals, return last one
                    return self.signals
                        .last()
                        .cloned()
                        .unwrap_or(Signal::Hold);
                }
            }
        }

        // For duplicate ticks or bar close events, return Hold
        Signal::Hold
    }

    fn initialize(&mut self, _params: HashMap<String, String>) -> Result<(), String> {
        Ok(())
    }
}

// ============================================================================
// Test Helper Functions
// ============================================================================

fn create_test_tick(symbol: &str, price: &str, quantity: &str) -> TickData {
    TickData::new_unchecked(
        Utc::now(),
        symbol.to_string(),
        Decimal::from_str(price).unwrap(),
        Decimal::from_str(quantity).unwrap(),
        TradeSide::Buy,
        format!("test_{}", Utc::now().timestamp_nanos_opt().unwrap_or(0)),
        false,
    )
}

async fn create_test_repository() -> Arc<TickDataRepository> {
    dotenv::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool");

    let cache = TieredCache::new(
        (100, 60),                    // memory: 100 ticks, 60s TTL
        (&redis_url, 1000, 300)       // redis: 1000 ticks, 300s TTL
    )
    .await
    .expect("Failed to create cache");

    Arc::new(TickDataRepository::new(pool, cache))
}

async fn cleanup_test_logs(repository: &TickDataRepository, strategy_id: &str) {
    let _ = sqlx::query("DELETE FROM live_strategy_log WHERE strategy_id = $1")
        .bind(strategy_id)
        .execute(repository.get_pool())
        .await;
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_processor_initialization() {
    let repository = create_test_repository().await;
    let strategy = Box::new(MockStrategy::always_hold());
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    let processor = PaperTradingProcessor::new(strategy, repository, initial_capital);

    // Verify initial state through public interface (process_tick will reveal internal state)
    assert_eq!(processor.initial_capital, initial_capital);
}

#[tokio::test]
async fn test_hold_signal() {
    let repository = create_test_repository().await;
    let strategy = Box::new(MockStrategy::always_hold());
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "AlwaysHold").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    let tick = create_test_tick("TESTUSDT", "100.0", "1.0");
    let result = processor.process_tick(&tick).await;

    assert!(result.is_ok());

    // Verify portfolio value = initial capital (no trades)
    assert_eq!(processor.cash, initial_capital);
    assert_eq!(processor.position, Decimal::ZERO);
    assert_eq!(processor.total_trades, 0);

    cleanup_test_logs(&repository, "AlwaysHold").await;
}

#[tokio::test]
async fn test_buy_signal_execution() {
    let repository = create_test_repository().await;
    let buy_qty = Decimal::from_str("10.0").unwrap();
    let strategy = Box::new(MockStrategy::buy_once("TESTUSDT", buy_qty));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyOnce").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    let tick = create_test_tick("TESTUSDT", "100.0", "1.0");
    let result = processor.process_tick(&tick).await;

    assert!(result.is_ok());

    // Verify trade executed
    let cost = buy_qty * Decimal::from_str("100.0").unwrap(); // 10 * 100 = 1000
    assert_eq!(processor.cash, initial_capital - cost);
    assert_eq!(processor.position, buy_qty);
    assert_eq!(processor.avg_cost, Decimal::from_str("100.0").unwrap());
    assert_eq!(processor.total_trades, 1);

    cleanup_test_logs(&repository, "BuyOnce").await;
}

#[tokio::test]
async fn test_buy_insufficient_cash() {
    let repository = create_test_repository().await;
    let buy_qty = Decimal::from_str("200.0").unwrap(); // Need 20,000
    let strategy = Box::new(MockStrategy::buy_once("TESTUSDT", buy_qty));
    let initial_capital = Decimal::from_str("10000.0").unwrap(); // Only have 10,000

    cleanup_test_logs(&repository, "BuyOnce").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    let tick = create_test_tick("TESTUSDT", "100.0", "1.0");
    let result = processor.process_tick(&tick).await;

    assert!(result.is_ok());

    // Verify trade NOT executed (insufficient cash)
    assert_eq!(processor.cash, initial_capital);
    assert_eq!(processor.position, Decimal::ZERO);
    assert_eq!(processor.total_trades, 0);

    cleanup_test_logs(&repository, "BuyOnce").await;
}

#[tokio::test]
async fn test_sell_signal_execution() {
    let repository = create_test_repository().await;
    let buy_qty = Decimal::from_str("10.0").unwrap();
    let sell_qty = Decimal::from_str("5.0").unwrap();
    let strategy = Box::new(MockStrategy::buy_then_sell("TESTUSDT", buy_qty, sell_qty));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyThenSell").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // First tick: BUY 10 @ 100
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    let result1 = processor.process_tick(&tick1).await;
    assert!(result1.is_ok());

    // Verify buy executed
    assert_eq!(processor.position, buy_qty);
    assert_eq!(processor.total_trades, 1);

    // Second tick: SELL 5 @ 120
    let tick2 = create_test_tick("TESTUSDT", "120.0", "1.0");
    let result2 = processor.process_tick(&tick2).await;
    assert!(result2.is_ok());

    // Verify sell executed
    assert_eq!(processor.position, Decimal::from_str("5.0").unwrap()); // 10 - 5
    assert_eq!(processor.total_trades, 2);

    // Cash should be: initial - buy_cost + sell_proceeds
    // 10000 - (10*100) + (5*120) = 10000 - 1000 + 600 = 9600
    let expected_cash = Decimal::from_str("9600.0").unwrap();
    assert_eq!(processor.cash, expected_cash);

    cleanup_test_logs(&repository, "BuyThenSell").await;
}

#[tokio::test]
async fn test_sell_insufficient_position() {
    let repository = create_test_repository().await;
    let sell_qty = Decimal::from_str("10.0").unwrap(); // Try to sell 10
    let strategy = Box::new(MockStrategy::sell_once("TESTUSDT", sell_qty));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "SellOnce").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // Try to sell without position
    let tick = create_test_tick("TESTUSDT", "100.0", "1.0");
    let result = processor.process_tick(&tick).await;

    assert!(result.is_ok());

    // Verify trade NOT executed (no position)
    assert_eq!(processor.cash, initial_capital);
    assert_eq!(processor.position, Decimal::ZERO);
    assert_eq!(processor.total_trades, 0);

    cleanup_test_logs(&repository, "SellOnce").await;
}

#[tokio::test]
async fn test_sell_all_position_resets_avg_cost() {
    let repository = create_test_repository().await;
    let buy_qty = Decimal::from_str("10.0").unwrap();
    let strategy = Box::new(MockStrategy::buy_then_sell("TESTUSDT", buy_qty, buy_qty));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyThenSell").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // BUY 10 @ 100
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    processor.process_tick(&tick1).await.unwrap();

    assert_eq!(processor.avg_cost, Decimal::from_str("100.0").unwrap());

    // SELL 10 @ 120 (sell entire position)
    let tick2 = create_test_tick("TESTUSDT", "120.0", "1.0");
    processor.process_tick(&tick2).await.unwrap();

    // Verify avg_cost reset to zero
    assert_eq!(processor.position, Decimal::ZERO);
    assert_eq!(processor.avg_cost, Decimal::ZERO);

    cleanup_test_logs(&repository, "BuyThenSell").await;
}

#[tokio::test]
async fn test_average_cost_calculation() {
    let repository = create_test_repository().await;

    // Create strategy that buys twice
    let strategy = Box::new(MockStrategy::new(
        "DoubleBuy",
        vec![
            Signal::Buy {
                symbol: "TESTUSDT".to_string(),
                quantity: Decimal::from_str("10.0").unwrap(),
            },
            Signal::Buy {
                symbol: "TESTUSDT".to_string(),
                quantity: Decimal::from_str("5.0").unwrap(),
            },
        ],
    ));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "DoubleBuy").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // First BUY: 10 @ 100
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    processor.process_tick(&tick1).await.unwrap();

    assert_eq!(processor.position, Decimal::from_str("10.0").unwrap());
    assert_eq!(processor.avg_cost, Decimal::from_str("100.0").unwrap());

    // Second BUY: 5 @ 120
    let tick2 = create_test_tick("TESTUSDT", "120.0", "1.0");
    processor.process_tick(&tick2).await.unwrap();

    // Position should be 15
    assert_eq!(processor.position, Decimal::from_str("15.0").unwrap());

    // Average cost: (10*100 + 5*120) / 15 = (1000 + 600) / 15 = 1600 / 15 = 106.666...
    let expected_avg_cost = Decimal::from_str("106.666666666666666666666666667").unwrap();
    assert_eq!(processor.avg_cost, expected_avg_cost);

    cleanup_test_logs(&repository, "DoubleBuy").await;
}

#[tokio::test]
async fn test_portfolio_value_calculation() {
    let repository = create_test_repository().await;
    let buy_qty = Decimal::from_str("10.0").unwrap();
    let strategy = Box::new(MockStrategy::buy_once("TESTUSDT", buy_qty));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyOnce").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // BUY 10 @ 100, cash becomes 9000
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    processor.process_tick(&tick1).await.unwrap();

    // Portfolio value with price at 100: 9000 + (10 * 100) = 10000
    let portfolio_value = processor.calculate_portfolio_value(Decimal::from_str("100.0").unwrap());
    assert_eq!(portfolio_value, Decimal::from_str("10000.0").unwrap());

    // Portfolio value with price at 120: 9000 + (10 * 120) = 10200
    let portfolio_value = processor.calculate_portfolio_value(Decimal::from_str("120.0").unwrap());
    assert_eq!(portfolio_value, Decimal::from_str("10200.0").unwrap());

    cleanup_test_logs(&repository, "BuyOnce").await;
}

#[tokio::test]
async fn test_database_logging() {
    let repository = create_test_repository().await;
    let strategy = Box::new(MockStrategy::always_hold());
    let initial_capital = Decimal::from_str("10000.0").unwrap();
    let strategy_id = "AlwaysHold";

    cleanup_test_logs(&repository, strategy_id).await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    let tick = create_test_tick("TESTUSDT", "100.0", "1.0");
    let result = processor.process_tick(&tick).await;

    assert!(result.is_ok());

    // Verify log was inserted into database
    let logs = sqlx::query("SELECT * FROM live_strategy_log WHERE strategy_id = $1")
        .bind(strategy_id)
        .fetch_all(repository.get_pool())
        .await
        .unwrap();

    // With OHLC generator, we might generate multiple bar events per tick,
    // but we only log once per process_tick call
    assert_eq!(logs.len(), 1, "Expected 1 log entry per processed tick");

    cleanup_test_logs(&repository, strategy_id).await;
}

#[tokio::test]
async fn test_multiple_ticks_sequential() {
    let repository = create_test_repository().await;

    // Strategy: Buy, Hold, Sell
    let strategy = Box::new(MockStrategy::new(
        "BuyHoldSell",
        vec![
            Signal::Buy {
                symbol: "TESTUSDT".to_string(),
                quantity: Decimal::from_str("10.0").unwrap(),
            },
            Signal::Hold,
            Signal::Sell {
                symbol: "TESTUSDT".to_string(),
                quantity: Decimal::from_str("10.0").unwrap(),
            },
        ],
    ));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyHoldSell").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // Tick 1: BUY 10 @ 100
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    processor.process_tick(&tick1).await.unwrap();
    assert_eq!(processor.total_trades, 1);

    // Tick 2: HOLD @ 110
    let tick2 = create_test_tick("TESTUSDT", "110.0", "1.0");
    processor.process_tick(&tick2).await.unwrap();
    assert_eq!(processor.total_trades, 1); // No new trade

    // Tick 3: SELL 10 @ 120
    let tick3 = create_test_tick("TESTUSDT", "120.0", "1.0");
    processor.process_tick(&tick3).await.unwrap();
    assert_eq!(processor.total_trades, 2);

    // Final state: cash = 10000 - 1000 + 1200 = 10200, position = 0
    assert_eq!(processor.cash, Decimal::from_str("10200.0").unwrap());
    assert_eq!(processor.position, Decimal::ZERO);

    cleanup_test_logs(&repository, "BuyHoldSell").await;
}

#[tokio::test]
async fn test_profit_scenario() {
    let repository = create_test_repository().await;
    let strategy = Box::new(MockStrategy::buy_then_sell(
        "TESTUSDT",
        Decimal::from_str("10.0").unwrap(),
        Decimal::from_str("10.0").unwrap(),
    ));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyThenSell").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // BUY 10 @ 100
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    processor.process_tick(&tick1).await.unwrap();

    // SELL 10 @ 150 (profit scenario)
    let tick2 = create_test_tick("TESTUSDT", "150.0", "1.0");
    processor.process_tick(&tick2).await.unwrap();

    // Profit: (150 - 100) * 10 = 500
    // Final cash: 10000 - 1000 + 1500 = 10500
    assert_eq!(processor.cash, Decimal::from_str("10500.0").unwrap());

    cleanup_test_logs(&repository, "BuyThenSell").await;
}

#[tokio::test]
async fn test_loss_scenario() {
    let repository = create_test_repository().await;
    let strategy = Box::new(MockStrategy::buy_then_sell(
        "TESTUSDT",
        Decimal::from_str("10.0").unwrap(),
        Decimal::from_str("10.0").unwrap(),
    ));
    let initial_capital = Decimal::from_str("10000.0").unwrap();

    cleanup_test_logs(&repository, "BuyThenSell").await;

    let mut processor = PaperTradingProcessor::new(strategy, repository.clone(), initial_capital);

    // BUY 10 @ 100
    let tick1 = create_test_tick("TESTUSDT", "100.0", "1.0");
    processor.process_tick(&tick1).await.unwrap();

    // SELL 10 @ 80 (loss scenario)
    let tick2 = create_test_tick("TESTUSDT", "80.0", "1.0");
    processor.process_tick(&tick2).await.unwrap();

    // Loss: (80 - 100) * 10 = -200
    // Final cash: 10000 - 1000 + 800 = 9800
    assert_eq!(processor.cash, Decimal::from_str("9800.0").unwrap());

    cleanup_test_logs(&repository, "BuyThenSell").await;
}
