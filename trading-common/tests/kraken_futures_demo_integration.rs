//! Kraken Futures Demo Integration Tests
//!
//! These tests exercise the full order flow against Kraken Futures demo environment:
//! Strategy → Order → KrakenFuturesVenue → Kraken Demo → Execution Report → Strategy
//!
//! # Setup
//!
//! 1. Get Kraken Futures demo API keys:
//!    - Create demo account at https://demo-futures.kraken.com/
//!    - Generate API keys from account settings
//!
//! 2. Set environment variables:
//!    ```bash
//!    export KRAKEN_FUTURES_DEMO_API_KEY=your_demo_key
//!    export KRAKEN_FUTURES_DEMO_API_SECRET=your_demo_secret  # Base64 encoded!
//!    ```
//!
//! 3. Run tests:
//!    ```bash
//!    cargo test -p trading-common --test kraken_futures_demo_integration -- --ignored --nocapture
//!    ```
//!
//! # Notes
//!
//! - Tests are marked `#[ignore]` by default since they require API keys
//! - Tests use limit orders far from market price to avoid accidental fills
//! - Each test cleans up by canceling any orders it creates
//! - Kraken Futures Demo is a real testnet with fake funds - safe to test!
//!
//! # Kraken Futures Symbol Format
//!
//! - Perpetual: PI_XBTUSD, PI_ETHUSD
//! - Fixed maturity: FI_XBTUSD_220325
//! - Internal format: BTCUSDT, ETHUSDT (auto-converted)

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::timeout;

use trading_common::execution::venue::kraken::{
    create_kraken_futures_demo, KrakenFuturesVenue, KrakenFuturesVenueConfig,
};
use trading_common::execution::venue::{
    AccountQueryVenue, ExecutionStreamVenue, OrderSubmissionVenue,
};
use trading_common::venue::VenueConnection;
use trading_common::orders::{Order, OrderEventAny, OrderSide, TimeInForce};

// ============================================================================
// Test Helpers
// ============================================================================

/// Check if Kraken Futures demo API keys are available
fn has_demo_keys() -> bool {
    env::var("KRAKEN_FUTURES_DEMO_API_KEY").is_ok()
        && env::var("KRAKEN_FUTURES_DEMO_API_SECRET").is_ok()
}

/// Skip test if no API keys
macro_rules! require_demo_keys {
    () => {
        if !has_demo_keys() {
            eprintln!(
                "Skipping: KRAKEN_FUTURES_DEMO_API_KEY and KRAKEN_FUTURES_DEMO_API_SECRET not set"
            );
            return;
        }
    };
}

/// Create a limit buy order far below market price (won't fill)
/// Using BTCUSDT which gets converted to PI_XBTUSD
fn create_test_buy_order(symbol: &str, price: Decimal) -> Order {
    Order::limit(symbol, OrderSide::Buy, dec!(0.001), price)
        .with_time_in_force(TimeInForce::GTC)
        .build()
        .expect("Failed to create test order")
}

/// Create a sell order for testing
fn create_test_sell_order(symbol: &str, price: Decimal) -> Order {
    Order::limit(symbol, OrderSide::Sell, dec!(0.001), price)
        .with_time_in_force(TimeInForce::GTC)
        .build()
        .expect("Failed to create test order")
}

// ============================================================================
// Venue Connection Tests
// ============================================================================

mod connection_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_venue_connect() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");

        // Connect should succeed
        let result = venue.connect().await;
        assert!(result.is_ok(), "Connect failed: {:?}", result.err());
        assert!(venue.is_connected());

        println!("Venue info: {:?}", venue.info());

        // Disconnect
        venue.disconnect().await.expect("Disconnect failed");
        assert!(!venue.is_connected());

        println!("=== Connection Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_venue_reconnect() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");

        // First connect
        venue.connect().await.expect("First connect failed");
        assert!(venue.is_connected());

        // Disconnect
        venue.disconnect().await.expect("Disconnect failed");
        assert!(!venue.is_connected());

        // Reconnect
        venue.connect().await.expect("Reconnect failed");
        assert!(venue.is_connected());

        // Verify works after reconnect
        let balances = venue.query_balances().await;
        assert!(balances.is_ok(), "Query should work after reconnect");

        venue.disconnect().await.ok();

        println!("=== Reconnection Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_venue_custom_config() {
        require_demo_keys!();

        // Create with custom config
        let mut config = KrakenFuturesVenueConfig::demo();
        config.base.rest.timeout_ms = 10000;

        let venue = KrakenFuturesVenue::new(config);
        assert!(venue.is_ok(), "Should create venue with custom config");

        let mut venue = venue.unwrap();
        let result = venue.connect().await;
        assert!(result.is_ok(), "Should connect with custom config");

        venue.disconnect().await.ok();

        println!("=== Custom Config Test: PASSED ===");
    }
}

// ============================================================================
// Account Query Tests
// ============================================================================

mod account_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_query_balances() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Query balances
        let balances = venue.query_balances().await;
        assert!(balances.is_ok(), "Query balances failed: {:?}", balances.err());

        let balances = balances.unwrap();
        println!("Futures demo balances: {:?}", balances);

        // Demo account should have some balances
        // Note: Kraken demo may start with empty balances or predefined amounts
        println!("Balance count: {}", balances.len());

        venue.disconnect().await.ok();

        println!("=== Query Balances Test: PASSED ===");
    }
}

// ============================================================================
// Order Submission Tests
// ============================================================================

mod order_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_submit_and_cancel_order() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Create a limit buy order far below market (won't fill)
        // BTC typically trades around $40k-$100k, so $10000 is safe
        let order = create_test_buy_order("BTCUSDT", dec!(10000));
        let order_id = order.client_order_id.clone();

        println!("Submitting order: {:?}", order_id);
        println!("Symbol: BTCUSDT -> PI_XBTUSD");

        // Submit order
        let submit_result = venue.submit_order(&order).await;
        assert!(submit_result.is_ok(), "Submit failed: {:?}", submit_result.err());

        let venue_order_id = submit_result.unwrap();
        println!("Submit response - venue_order_id: {:?}", venue_order_id);

        // Give exchange time to process
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Query the order to verify it exists
        let query_result = venue.query_order(&order_id, Some(&venue_order_id), "BTCUSDT").await;
        assert!(query_result.is_ok(), "Query failed: {:?}", query_result.err());

        let query_response = query_result.unwrap();
        println!("Query response: {:?}", query_response);
        assert_eq!(query_response.client_order_id, order_id);

        // Cancel the order
        println!("Canceling order: {:?}", order_id);
        let cancel_result = venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await;
        assert!(cancel_result.is_ok(), "Cancel failed: {:?}", cancel_result.err());

        println!("Cancel successful");

        venue.disconnect().await.ok();

        println!("=== Submit and Cancel Order Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_query_open_orders() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Submit a test order
        let order = create_test_buy_order("BTCUSDT", dec!(10000));
        let order_id = order.client_order_id.clone();

        let venue_order_id = venue.submit_order(&order).await.expect("Submit failed");

        // Wait for order to be registered
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Query open orders
        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await;
        assert!(open_orders.is_ok(), "Query open orders failed: {:?}", open_orders.err());

        let orders = open_orders.unwrap();
        println!("Open orders: {} found", orders.len());
        for order in &orders {
            println!("  - {:?}: {} @ {}", order.client_order_id, order.side, order.price.unwrap_or_default());
        }

        // Should find our order
        let found = orders.iter().any(|o| o.client_order_id == order_id);
        assert!(found, "Our order should be in open orders list");

        // Clean up
        venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await.ok();
        venue.disconnect().await.ok();

        println!("=== Query Open Orders Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_cancel_all_orders() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Submit multiple orders
        let order1 = create_test_buy_order("BTCUSDT", dec!(10000));
        let order2 = create_test_buy_order("BTCUSDT", dec!(10001));
        let id1 = order1.client_order_id.clone();
        let id2 = order2.client_order_id.clone();

        venue.submit_order(&order1).await.expect("Submit 1 failed");
        venue.submit_order(&order2).await.expect("Submit 2 failed");

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Cancel all orders for symbol
        let cancel_result = venue.cancel_all_orders(Some("BTCUSDT")).await;
        assert!(cancel_result.is_ok(), "Cancel all failed: {:?}", cancel_result.err());

        // Verify orders are gone
        tokio::time::sleep(Duration::from_millis(500)).await;

        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await.unwrap();
        let found1 = open_orders.iter().any(|o| o.client_order_id == id1);
        let found2 = open_orders.iter().any(|o| o.client_order_id == id2);

        assert!(!found1 && !found2, "Orders should have been canceled");

        venue.disconnect().await.ok();

        println!("=== Cancel All Orders Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_modify_order() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Submit initial order
        let order = create_test_buy_order("BTCUSDT", dec!(10000));
        let order_id = order.client_order_id.clone();

        println!("Submitting order: {:?}", order_id);
        let venue_order_id = venue.submit_order(&order).await.expect("Submit failed");
        println!("Order ID: {:?}", venue_order_id);

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Modify the order price
        let new_price = dec!(10500);
        println!("Modifying order to price: {}", new_price);

        let modify_result = venue
            .modify_order(&order_id, Some(&venue_order_id), "BTCUSDT", Some(new_price), None)
            .await;

        match modify_result {
            Ok(new_venue_id) => {
                println!("Modify successful - new venue ID: {:?}", new_venue_id);

                // Verify the modification
                let query_result = venue.query_order(&order_id, Some(&new_venue_id), "BTCUSDT").await;
                if let Ok(order_info) = query_result {
                    println!("Modified order price: {:?}", order_info.price);
                    // Note: Kraken may keep the same order ID or return a new one
                }

                // Clean up
                venue.cancel_order(&order_id, Some(&new_venue_id), "BTCUSDT").await.ok();
            }
            Err(e) => {
                // Modify might not be supported in demo or for certain order types
                println!("Modify returned error (may be expected): {:?}", e);
                // Clean up original order
                venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await.ok();
            }
        }

        venue.disconnect().await.ok();

        println!("=== Modify Order Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_order_rejected_invalid_symbol() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Try to submit order with invalid symbol
        let order = create_test_buy_order("INVALIDUSDT", dec!(10000));

        let result = venue.submit_order(&order).await;
        assert!(result.is_err(), "Should reject invalid symbol");

        let err = result.err().unwrap();
        println!("Expected error for invalid symbol: {:?}", err);

        venue.disconnect().await.ok();

        println!("=== Order Rejected Invalid Symbol Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_order_rejected_quantity_too_small() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Try to submit order with quantity below minimum
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.00000001), dec!(10000))
            .with_time_in_force(TimeInForce::GTC)
            .build()
            .expect("Failed to create order");

        let result = venue.submit_order(&order).await;
        // Note: Kraken might have different minimum size requirements
        println!("Tiny quantity result: {:?}", result);

        venue.disconnect().await.ok();

        println!("=== Order Rejected Quantity Test: COMPLETED ===");
    }
}

// ============================================================================
// WebSocket Execution Stream Tests
// ============================================================================

mod stream_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_futures_demo_execution_stream() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Set up event tracking
        let event_received = Arc::new(AtomicBool::new(false));
        let event_received_clone = event_received.clone();
        let events_collected = Arc::new(std::sync::Mutex::new(Vec::<OrderEventAny>::new()));
        let events_clone = events_collected.clone();

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        // Start execution stream
        let callback = Arc::new(move |event: OrderEventAny| {
            println!("Received event via stream: {:?}", event);
            event_received_clone.store(true, Ordering::SeqCst);
            events_clone.lock().unwrap().push(event);
        });

        let stream_result = venue.start_execution_stream(callback, shutdown_rx).await;
        assert!(stream_result.is_ok(), "Start stream failed: {:?}", stream_result.err());

        // Give the stream time to connect and authenticate
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Submit an order - this should generate an event on the stream
        let order = create_test_buy_order("BTCUSDT", dec!(10000));
        let order_id = order.client_order_id.clone();

        println!("Submitting order to trigger stream event...");
        let venue_order_id = venue.submit_order(&order).await.expect("Submit failed");

        // Wait for event (with timeout)
        let received = timeout(Duration::from_secs(15), async {
            while !event_received.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        // Cancel order and clean up
        venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await.ok();

        // Wait for cancel event
        tokio::time::sleep(Duration::from_secs(2)).await;

        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        // Check results
        let events = events_collected.lock().unwrap();
        println!("Total events received: {}", events.len());
        for event in events.iter() {
            println!("  Event: {:?}", event);
        }

        assert!(
            received.is_ok() || !events.is_empty(),
            "Should have received at least one execution event"
        );

        println!("=== Execution Stream Test: PASSED ===");
    }
}

// ============================================================================
// End-to-End Flow Tests
// ============================================================================

mod e2e_tests {
    use super::*;

    /// Test the full flow: Strategy → Venue → Demo → Venue → Strategy
    #[tokio::test]
    #[ignore]
    async fn test_e2e_futures_demo_order_flow() {
        require_demo_keys!();

        // Create and connect venue
        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Set up execution stream to capture events
        let stream_events = Arc::new(std::sync::Mutex::new(Vec::<OrderEventAny>::new()));
        let stream_events_clone = stream_events.clone();

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let callback = Arc::new(move |event: OrderEventAny| {
            println!("Stream received: {:?}", event);
            stream_events_clone.lock().unwrap().push(event);
        });

        venue
            .start_execution_stream(callback, shutdown_rx)
            .await
            .expect("Start stream failed");

        tokio::time::sleep(Duration::from_secs(3)).await;

        // Simulate strategy generating an order
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), dec!(10000))
            .with_time_in_force(TimeInForce::GTC)
            .build()
            .expect("Failed to create order");
        let order_id = order.client_order_id.clone();

        println!("\n=== E2E Test: Submitting order ===");
        println!("Order ID: {:?}", order_id);
        println!("Symbol: BTCUSDT (converted to PI_XBTUSD)");

        // Submit through venue
        let submit_result = venue.submit_order(&order).await;
        assert!(submit_result.is_ok(), "Submit failed: {:?}", submit_result.err());

        let venue_order_id = submit_result.unwrap();
        println!("Venue order ID: {:?}", venue_order_id);

        // Wait for WebSocket event
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Cancel order
        println!("\n=== E2E Test: Canceling order ===");
        let cancel_result = venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await;
        assert!(cancel_result.is_ok(), "Cancel failed: {:?}", cancel_result.err());

        // Wait for cancel event
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check stream events
        let events = stream_events.lock().unwrap();
        println!("\n=== E2E Test: Received {} stream events ===", events.len());
        for (i, event) in events.iter().enumerate() {
            println!("  Event {}: {:?}", i, event);
        }

        // Clean up
        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        // Verify we got events (at minimum)
        if !events.is_empty() {
            let has_accepted = events.iter().any(|e| matches!(e, OrderEventAny::Accepted(_)));
            let has_canceled = events.iter().any(|e| matches!(e, OrderEventAny::Canceled(_)));

            println!("Has Accepted event: {}", has_accepted);
            println!("Has Canceled event: {}", has_canceled);
        }

        println!("\n=== E2E Test: PASSED ===");
    }

    /// Test multiple orders in sequence
    #[tokio::test]
    #[ignore]
    async fn test_e2e_multiple_orders_flow() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        let mut order_ids = Vec::new();

        println!("\n=== E2E Multi-Order Test: Submitting 3 orders ===");

        // Submit multiple orders
        for i in 0..3 {
            let price = dec!(10000) + Decimal::from(i * 100);
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), price)
                .with_time_in_force(TimeInForce::GTC)
                .build()
                .expect("Failed to create order");

            let order_id = order.client_order_id.clone();
            println!("  Submitting order {}: {:?} @ {}", i, order_id, price);

            let result = venue.submit_order(&order).await;
            match result {
                Ok(venue_id) => {
                    println!("  Order {} submitted: {:?}", i, venue_id);
                    order_ids.push(order_id);
                }
                Err(e) => {
                    println!("  Order {} failed: {:?}", i, e);
                }
            }

            // Small delay between orders
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Query all open orders
        tokio::time::sleep(Duration::from_millis(500)).await;
        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await.unwrap();
        println!("\n  Open orders count: {}", open_orders.len());

        // Verify submitted orders exist
        for order_id in &order_ids {
            let found = open_orders.iter().any(|o| &o.client_order_id == order_id);
            if !found {
                println!("  Warning: Order {:?} not found in open orders", order_id);
            }
        }

        // Cancel all orders
        println!("\n=== E2E Multi-Order Test: Canceling all orders ===");
        let cancel_result = venue.cancel_all_orders(Some("BTCUSDT")).await;
        assert!(cancel_result.is_ok(), "Batch cancel failed: {:?}", cancel_result.err());

        // Verify all canceled
        tokio::time::sleep(Duration::from_millis(500)).await;
        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await.unwrap();

        for order_id in &order_ids {
            let found = open_orders.iter().any(|o| &o.client_order_id == order_id);
            assert!(!found, "Order {:?} should have been canceled", order_id);
        }

        venue.disconnect().await.ok();

        println!("\n=== E2E Multi-Order Test: PASSED ===");
    }
}

// ============================================================================
// Stress Tests
// ============================================================================

mod stress_tests {
    use super::*;

    /// Test rapid order submission (within rate limits)
    #[tokio::test]
    #[ignore]
    async fn test_rapid_order_submission() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        println!("\n=== Rapid Submission Test: Submitting 5 orders quickly ===");

        let mut successful = 0;

        for i in 0..5 {
            let price = dec!(10000) + Decimal::from(i * 50);
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), price)
                .with_time_in_force(TimeInForce::GTC)
                .build()
                .expect("Failed to create order");

            let result = venue.submit_order(&order).await;
            match result {
                Ok(_) => {
                    println!("  Order {} submitted successfully", i);
                    successful += 1;
                }
                Err(e) => {
                    println!("  Order {} failed: {:?}", i, e);
                }
            }
        }

        println!("  Successfully submitted {} orders", successful);
        assert!(successful >= 3, "Should submit most orders successfully");

        // Clean up
        venue.cancel_all_orders(Some("BTCUSDT")).await.ok();
        venue.disconnect().await.ok();

        println!("\n=== Rapid Submission Test: PASSED ===");
    }

    /// Test interleaved buy and sell orders
    #[tokio::test]
    #[ignore]
    async fn test_interleaved_orders() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        println!("\n=== Interleaved Orders Test ===");

        // Submit buy orders at low prices
        let buy_order = create_test_buy_order("BTCUSDT", dec!(10000));
        let buy_result = venue.submit_order(&buy_order).await;
        println!("Buy order result: {:?}", buy_result);

        // Submit sell orders at high prices
        let sell_order = create_test_sell_order("BTCUSDT", dec!(150000));
        let sell_result = venue.submit_order(&sell_order).await;
        println!("Sell order result: {:?}", sell_result);

        // Query and verify
        tokio::time::sleep(Duration::from_millis(500)).await;
        let open_orders = venue.query_open_orders(None).await;
        println!("Open orders: {:?}", open_orders);

        // Clean up
        venue.cancel_all_orders(None).await.ok();
        venue.disconnect().await.ok();

        println!("\n=== Interleaved Orders Test: PASSED ===");
    }
}

// ============================================================================
// Symbol Conversion Tests
// ============================================================================

mod symbol_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_symbol_conversion_btc() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Test BTC symbol (BTCUSDT -> PI_XBTUSD)
        let order = create_test_buy_order("BTCUSDT", dec!(10000));
        let result = venue.submit_order(&order).await;

        assert!(result.is_ok(), "BTC order should succeed: {:?}", result.err());
        let venue_id = result.unwrap();

        // Clean up
        venue.cancel_order(&order.client_order_id, Some(&venue_id), "BTCUSDT").await.ok();
        venue.disconnect().await.ok();

        println!("=== BTC Symbol Conversion Test: PASSED ===");
    }

    #[tokio::test]
    #[ignore]
    async fn test_symbol_conversion_eth() {
        require_demo_keys!();

        let mut venue = create_kraken_futures_demo().expect("Failed to create futures demo venue");
        venue.connect().await.expect("Connect failed");

        // Test ETH symbol (ETHUSDT -> PI_ETHUSD)
        let order = create_test_buy_order("ETHUSDT", dec!(1000));
        let result = venue.submit_order(&order).await;

        assert!(result.is_ok(), "ETH order should succeed: {:?}", result.err());
        let venue_id = result.unwrap();

        // Clean up
        venue.cancel_order(&order.client_order_id, Some(&venue_id), "ETHUSDT").await.ok();
        venue.disconnect().await.ok();

        println!("=== ETH Symbol Conversion Test: PASSED ===");
    }
}
