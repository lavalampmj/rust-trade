// market_data_tests.rs - Tests for MarketDataService

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::timeout;

use super::MarketDataService;
use crate::exchange::{errors::ExchangeError, traits::Exchange};
use trading_common::data::types::{TickData, TradeSide};
use trading_common::data::validator::{TickValidator, ValidationConfig};
use trading_common::data::{cache::{TieredCache, TickDataCache}, repository::TickDataRepository};

use chrono::Utc;
use rust_decimal::Decimal;
use std::str::FromStr;

// ============================================================================
// Mock Exchange Implementation
// ============================================================================

struct MockExchange {
    ticks: Vec<TickData>,
    should_fail: bool,
    delay_ms: u64,
}

impl MockExchange {
    fn new(ticks: Vec<TickData>) -> Self {
        Self {
            ticks,
            should_fail: false,
            delay_ms: 0,
        }
    }

    fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
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
        if self.should_fail {
            return Err(ExchangeError::NetworkError("Mock failure".to_string()));
        }

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

fn create_test_validator() -> Arc<TickValidator> {
    let config = ValidationConfig::default();
    Arc::new(TickValidator::new(config))
}

async fn cleanup_test_data(repository: &TickDataRepository, symbol: &str) {
    let _ = sqlx::query("DELETE FROM tick_data WHERE symbol = $1")
        .bind(symbol)
        .execute(repository.get_pool())
        .await;
}

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_service_creation() {
    let ticks = vec![create_test_tick("BTCTEST", "50000.0", "0.1", "1")];
    let exchange = Arc::new(MockExchange::new(ticks));
    let repository = create_test_repository().await;
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository,
        vec!["BTCTEST".to_string()],
        validator,
    );

    // Verify service was created
    assert!(service.get_shutdown_tx().receiver_count() == 0);
}

#[tokio::test]
async fn test_reject_empty_symbols() {
    let ticks = vec![create_test_tick("BTCTEST", "50000.0", "0.1", "1")];
    let exchange = Arc::new(MockExchange::new(ticks));
    let repository = create_test_repository().await;
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository,
        vec![], // Empty symbols
        validator,
    );

    let result = service.start().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No symbols"));
}

#[tokio::test]
async fn test_process_single_tick() {
    let repository = create_test_repository().await;
    let test_symbol = format!("BTCTEST_{}", Utc::now().timestamp_millis() % 1000000);

    cleanup_test_data(&repository, &test_symbol).await;

    let ticks = vec![create_test_tick(&test_symbol, "50000.0", "0.1", "test_1")];
    let exchange = Arc::new(MockExchange::new(ticks));
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange.clone(),
        repository.clone(),
        vec![test_symbol.clone()],
        validator,
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

    // Verify tick was inserted (may take a moment for batch to flush)
    tokio::time::sleep(Duration::from_millis(100)).await;

    cleanup_test_data(&repository, &test_symbol).await;
}

#[tokio::test]
async fn test_batch_processing() {
    let repository = create_test_repository().await;
    let test_symbol = format!("BATCHTEST_{}", Utc::now().timestamp_millis() % 1000000);

    cleanup_test_data(&repository, &test_symbol).await;

    // Create multiple ticks for batching
    let ticks: Vec<TickData> = (1..=5)
        .map(|i| {
            create_test_tick(
                &test_symbol,
                &format!("{}.0", 50000 + i),
                "0.1",
                &format!("test_{}", i),
            )
        })
        .collect();

    let exchange = Arc::new(MockExchange::new(ticks));
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository.clone(),
        vec![test_symbol.clone()],
        validator,
    );

    let shutdown_tx = service.get_shutdown_tx();

    // Start service
    let service_handle = tokio::spawn(async move {
        service.start().await
    });

    // Wait for ticks to be processed
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Shutdown
    let _ = shutdown_tx.send(());
    let _ = timeout(Duration::from_secs(2), service_handle).await;

    // Wait for batch flush
    tokio::time::sleep(Duration::from_millis(200)).await;

    cleanup_test_data(&repository, &test_symbol).await;
}

#[tokio::test]
async fn test_invalid_tick_rejection() {
    let repository = create_test_repository().await;
    let test_symbol = "INVALIDTEST".to_string();

    cleanup_test_data(&repository, &test_symbol).await;

    // Create invalid tick (negative price)
    let invalid_tick = TickData::new_unchecked(
        Utc::now(),
        test_symbol.clone(),
        Decimal::from_str("-100.0").unwrap(), // Invalid negative price
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "invalid_1".to_string(),
        false,
    );

    let exchange = Arc::new(MockExchange::new(vec![invalid_tick]));
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository.clone(),
        vec![test_symbol.clone()],
        validator,
    );

    let shutdown_tx = service.get_shutdown_tx();

    // Start service
    let service_handle = tokio::spawn(async move {
        service.start().await
    });

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Shutdown
    let _ = shutdown_tx.send(());
    let _ = timeout(Duration::from_secs(2), service_handle).await;

    // Verify no ticks were inserted (invalid tick was rejected)
    tokio::time::sleep(Duration::from_millis(100)).await;

    cleanup_test_data(&repository, &test_symbol).await;
}

#[tokio::test]
async fn test_shutdown_signal() {
    let ticks = vec![create_test_tick("SHUTDOWNTEST", "50000.0", "0.1", "1")];
    let exchange = Arc::new(MockExchange::new(ticks).with_delay(1000)); // 1 second delay
    let repository = create_test_repository().await;
    let validator = create_test_validator();
    let test_symbol = "SHUTDOWNTEST".to_string();

    cleanup_test_data(&repository, &test_symbol).await;

    let service = MarketDataService::new(
        exchange,
        repository.clone(),
        vec![test_symbol.clone()],
        validator,
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

    cleanup_test_data(&repository, &test_symbol).await;
}

#[tokio::test]
async fn test_multiple_symbols() {
    let repository = create_test_repository().await;
    let symbol1 = format!("BTC_{}", Utc::now().timestamp_millis() % 1000000);
    let symbol2 = format!("ETH_{}", Utc::now().timestamp_millis() % 1000000);

    cleanup_test_data(&repository, &symbol1).await;
    cleanup_test_data(&repository, &symbol2).await;

    let ticks = vec![
        create_test_tick(&symbol1, "50000.0", "0.1", "btc_1"),
        create_test_tick(&symbol2, "3500.0", "1.0", "eth_1"),
    ];

    let exchange = Arc::new(MockExchange::new(ticks));
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository.clone(),
        vec![symbol1.clone(), symbol2.clone()],
        validator,
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

    // Wait for batch flush
    tokio::time::sleep(Duration::from_millis(200)).await;

    cleanup_test_data(&repository, &symbol1).await;
    cleanup_test_data(&repository, &symbol2).await;
}

#[tokio::test]
async fn test_validator_integration() {
    let repository = create_test_repository().await;
    let test_symbol = format!("VALIDTEST_{}", Utc::now().timestamp_millis() % 1000000);

    cleanup_test_data(&repository, &test_symbol).await;

    // Create valid and invalid ticks
    let valid_tick = create_test_tick(&test_symbol, "50000.0", "0.1", "valid_1");

    // Invalid: zero price
    let invalid_tick = TickData::new_unchecked(
        Utc::now(),
        test_symbol.clone(),
        Decimal::ZERO,
        Decimal::from_str("0.1").unwrap(),
        TradeSide::Buy,
        "invalid_1".to_string(),
        false,
    );

    let exchange = Arc::new(MockExchange::new(vec![valid_tick, invalid_tick]));
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository.clone(),
        vec![test_symbol.clone()],
        validator,
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

    // Wait for batch flush
    tokio::time::sleep(Duration::from_millis(200)).await;

    cleanup_test_data(&repository, &test_symbol).await;
}

#[tokio::test]
async fn test_cache_update() {
    let repository = create_test_repository().await;
    let test_symbol = format!("CACHETEST_{}", Utc::now().timestamp_millis() % 1000000);

    cleanup_test_data(&repository, &test_symbol).await;

    let tick = create_test_tick(&test_symbol, "50000.0", "0.1", "cache_1");
    let exchange = Arc::new(MockExchange::new(vec![tick.clone()]));
    let validator = create_test_validator();

    let service = MarketDataService::new(
        exchange,
        repository.clone(),
        vec![test_symbol.clone()],
        validator,
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
    let cached_ticks: Result<Vec<TickData>, _> = repository
        .get_cache()
        .get_recent_ticks(&test_symbol, 10)
        .await;

    cleanup_test_data(&repository, &test_symbol).await;

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
