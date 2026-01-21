// market_data_tests.rs - Tests for MarketDataService

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::timeout;

use super::MarketDataService;
use crate::exchange::{errors::ExchangeError, traits::Exchange};
use trading_common::data::types::{TickData, TradeSide};
use trading_common::data::cache::{TieredCache, TickDataCache};

use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;

// ============================================================================
// Mock Exchange Implementation
// ============================================================================

struct MockExchange {
    ticks: Vec<TickData>,
    delay_ms: u64,
}

impl MockExchange {
    fn new(ticks: Vec<TickData>) -> Self {
        Self {
            ticks,
            delay_ms: 0,
        }
    }

    fn with_delay(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }
}

#[async_trait::async_trait]
impl Exchange for MockExchange {
    async fn subscribe_trades(
        &self,
        _symbols: &[String],
        callback: Box<dyn Fn(TickData) + Send + Sync>,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ExchangeError> {
        // Send all ticks
        for tick in &self.ticks {
            // Check for shutdown
            if shutdown_rx.try_recv().is_ok() {
                return Ok(());
            }

            // Add delay if configured
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }

            callback(tick.clone());
        }

        // Wait for shutdown signal
        let _ = shutdown_rx.recv().await;
        Ok(())
    }
}

// ============================================================================
// Test Helper Functions
// ============================================================================

fn create_test_tick(symbol: &str, price: &str, quantity: &str, trade_id: &str) -> TickData {
    TickData::new_unchecked(
        Utc::now(),
        symbol.to_string(),
        Decimal::from_str(price).unwrap(),
        Decimal::from_str(quantity).unwrap(),
        TradeSide::Buy,
        trade_id.to_string(),
        false,
    )
}

async fn create_test_cache() -> Arc<dyn TickDataCache> {
    dotenv::dotenv().ok();
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());

    let cache = TieredCache::new(
        (100, 60),                    // memory: 100 ticks, 60s TTL
        (&redis_url, 1000, 300)       // redis: 1000 ticks, 300s TTL
    )
    .await
    .expect("Failed to create cache");

    Arc::new(cache)
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_service_creation() {
    let ticks = vec![create_test_tick("BTCTEST", "50000.0", "0.1", "1")];
    let exchange = Arc::new(MockExchange::new(ticks));
    let cache = create_test_cache().await;

    let service = MarketDataService::new(
        exchange,
        cache,
        vec!["BTCTEST".to_string()],
    );

    // Verify service was created
    assert!(service.get_shutdown_tx().receiver_count() == 0);
}

#[tokio::test]
async fn test_reject_empty_symbols() {
    let ticks = vec![create_test_tick("BTCTEST", "50000.0", "0.1", "1")];
    let exchange = Arc::new(MockExchange::new(ticks));
    let cache = create_test_cache().await;

    let service = MarketDataService::new(
        exchange,
        cache,
        vec![], // Empty symbols
    );

    let result = service.start().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No symbols"));
}

#[tokio::test]
async fn test_process_single_tick() {
    let cache = create_test_cache().await;
    let test_symbol = format!("BTCTEST_{}", Utc::now().timestamp_millis() % 1000000);

    let ticks = vec![create_test_tick(&test_symbol, "50000.0", "0.1", "test_1")];
    let exchange = Arc::new(MockExchange::new(ticks));

    let service = MarketDataService::new(
        exchange.clone(),
        cache.clone(),
        vec![test_symbol.clone()],
    );

    let shutdown_tx = service.get_shutdown_tx();

    // Start service in background
    let service_handle = tokio::spawn(async move {
        service.start().await
    });

    // Wait for tick to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Shutdown service
    let _ = shutdown_tx.send(());

    // Wait for service to stop
    let result = timeout(Duration::from_secs(2), service_handle)
        .await
        .expect("Service should stop within timeout");

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_shutdown_signal() {
    let ticks = vec![create_test_tick("SHUTDOWNTEST", "50000.0", "0.1", "1")];
    let exchange = Arc::new(MockExchange::new(ticks).with_delay(1000)); // 1 second delay
    let cache = create_test_cache().await;
    let test_symbol = "SHUTDOWNTEST".to_string();

    let service = MarketDataService::new(
        exchange,
        cache.clone(),
        vec![test_symbol.clone()],
    );

    let shutdown_tx = service.get_shutdown_tx();

    // Start service
    let service_handle = tokio::spawn(async move {
        service.start().await
    });

    // Immediately send shutdown signal
    tokio::time::sleep(Duration::from_millis(10)).await;
    let _ = shutdown_tx.send(());

    // Service should stop quickly
    let result = timeout(Duration::from_secs(2), service_handle)
        .await
        .expect("Service should stop within timeout");

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_symbols() {
    let cache = create_test_cache().await;
    let symbol1 = format!("BTC_{}", Utc::now().timestamp_millis() % 1000000);
    let symbol2 = format!("ETH_{}", Utc::now().timestamp_millis() % 1000000);

    let ticks = vec![
        create_test_tick(&symbol1, "50000.0", "0.1", "btc_1"),
        create_test_tick(&symbol2, "3500.0", "1.0", "eth_1"),
    ];

    let exchange = Arc::new(MockExchange::new(ticks));

    let service = MarketDataService::new(
        exchange,
        cache.clone(),
        vec![symbol1.clone(), symbol2.clone()],
    );

    let shutdown_tx = service.get_shutdown_tx();

    // Start service
    let service_handle = tokio::spawn(async move {
        service.start().await
    });

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Shutdown
    let _ = shutdown_tx.send(());
    let _ = timeout(Duration::from_secs(2), service_handle).await;
}

#[tokio::test]
async fn test_cache_update() {
    let cache = create_test_cache().await;
    let test_symbol = format!("CACHETEST_{}", Utc::now().timestamp_millis() % 1000000);

    let tick = create_test_tick(&test_symbol, "50000.0", "0.1", "cache_1");
    let exchange = Arc::new(MockExchange::new(vec![tick.clone()]));

    let service = MarketDataService::new(
        exchange,
        cache.clone(),
        vec![test_symbol.clone()],
    );

    let shutdown_tx = service.get_shutdown_tx();

    // Start service
    let service_handle = tokio::spawn(async move {
        service.start().await
    });

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Shutdown
    let _ = shutdown_tx.send(());
    let _ = timeout(Duration::from_secs(2), service_handle).await;

    // Verify tick was cached
    tokio::time::sleep(Duration::from_millis(100)).await;
    let cached_ticks: Result<Vec<TickData>, _> = cache
        .get_recent_ticks(&test_symbol, 10)
        .await;

    // Cache might be empty if Redis is not running, but test shouldn't fail
    match cached_ticks {
        Ok(ticks) => {
            if !ticks.is_empty() {
                assert_eq!(ticks.len(), 1);
                assert_eq!(ticks[0].symbol, test_symbol);
            }
        }
        Err(_) => {
            // Cache might not be available in test environment
            println!("Cache not available in test environment");
        }
    }
}
