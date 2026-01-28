//! Binance Testnet Integration Tests
//!
//! These tests exercise the full order flow against Binance testnet:
//! Strategy → Order → BinanceVenue → Binance Testnet → Execution Report → Strategy
//!
//! # Setup
//!
//! 1. Get testnet API keys:
//!    - Spot: https://testnet.binance.vision/
//!    - Futures: https://testnet.binancefuture.com/
//!
//! 2. Set environment variables:
//!    ```bash
//!    export BINANCE_TESTNET_API_KEY=your_testnet_key
//!    export BINANCE_TESTNET_API_SECRET=your_testnet_secret
//!    ```
//!
//! 3. Run tests:
//!    ```bash
//!    cargo test -p trading-common --test binance_testnet_integration -- --ignored --nocapture
//!    ```
//!
//! # Notes
//!
//! - Tests are marked `#[ignore]` by default since they require API keys
//! - Tests use limit orders far from market price to avoid accidental fills
//! - Each test cleans up by canceling any orders it creates

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::timeout;

use trading_common::execution::venue::binance::{
    create_binance_futures_testnet, create_binance_spot_testnet, MarginType,
};
use trading_common::execution::venue::{
    AccountQueryVenue, ExecutionStreamVenue, OrderSubmissionVenue,
};
use trading_common::venue::VenueConnection;
use trading_common::orders::{Order, OrderEventAny, OrderSide, TimeInForce};

// ============================================================================
// Test Helpers
// ============================================================================

/// Check if testnet API keys are available
fn has_testnet_keys() -> bool {
    env::var("BINANCE_TESTNET_API_KEY").is_ok()
        && env::var("BINANCE_TESTNET_API_SECRET").is_ok()
}

/// Skip test if no API keys
macro_rules! require_testnet_keys {
    () => {
        if !has_testnet_keys() {
            eprintln!("Skipping: BINANCE_TESTNET_API_KEY and BINANCE_TESTNET_API_SECRET not set");
            return;
        }
    };
}

/// Create a limit buy order far below market price (won't fill)
fn create_test_buy_order(symbol: &str, price: Decimal) -> Order {
    Order::limit(symbol, OrderSide::Buy, dec!(0.001), price)
        .with_time_in_force(TimeInForce::GTC)
        .build()
        .expect("Failed to create test order")
}

// ============================================================================
// Spot Venue Integration Tests
// ============================================================================

mod spot_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_connect() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");

        // Connect should succeed
        let result = venue.connect().await;
        assert!(result.is_ok(), "Connect failed: {:?}", result.err());
        assert!(venue.is_connected());

        // Disconnect
        venue.disconnect().await.expect("Disconnect failed");
        assert!(!venue.is_connected());
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_query_balances() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Query balances
        let balances = venue.query_balances().await;
        assert!(balances.is_ok(), "Query balances failed: {:?}", balances.err());

        let balances = balances.unwrap();
        println!("Spot balances: {:?}", balances);

        // Testnet should have some balances
        assert!(!balances.is_empty(), "Expected some testnet balances");

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_submit_and_cancel_order() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Create a limit buy order far below market (won't fill)
        // BTCUSDT typically trades around $40k-$70k, so $1000 is very safe
        let order = create_test_buy_order("BTCUSDT", dec!(1000));
        let order_id = order.client_order_id.clone();

        println!("Submitting order: {:?}", order_id);

        // Submit order - returns VenueOrderId on success
        let submit_result = venue.submit_order(&order).await;
        assert!(submit_result.is_ok(), "Submit failed: {:?}", submit_result.err());

        let venue_order_id = submit_result.unwrap();
        println!("Submit response - venue_order_id: {:?}", venue_order_id);

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
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_query_open_orders() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Submit a test order
        let order = create_test_buy_order("BTCUSDT", dec!(1000));
        let order_id = order.client_order_id.clone();

        let venue_order_id = venue.submit_order(&order).await.expect("Submit failed");

        // Query open orders
        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await;
        assert!(open_orders.is_ok(), "Query open orders failed: {:?}", open_orders.err());

        let orders = open_orders.unwrap();
        println!("Open orders: {:?}", orders);

        // Should find our order
        let found = orders.iter().any(|o| o.client_order_id == order_id);
        assert!(found, "Our order should be in open orders list");

        // Clean up
        venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await.ok();
        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_batch_cancel() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Submit multiple orders
        let order1 = create_test_buy_order("BTCUSDT", dec!(1000));
        let order2 = create_test_buy_order("BTCUSDT", dec!(1001));
        let id1 = order1.client_order_id.clone();
        let id2 = order2.client_order_id.clone();

        venue.submit_order(&order1).await.expect("Submit 1 failed");
        venue.submit_order(&order2).await.expect("Submit 2 failed");

        // Cancel all orders for symbol
        let cancel_result = venue.cancel_all_orders(Some("BTCUSDT")).await;
        assert!(cancel_result.is_ok(), "Batch cancel failed: {:?}", cancel_result.err());

        // Verify orders are gone
        tokio::time::sleep(Duration::from_millis(500)).await;

        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await.unwrap();
        let found1 = open_orders.iter().any(|o| o.client_order_id == id1);
        let found2 = open_orders.iter().any(|o| o.client_order_id == id2);

        assert!(!found1 && !found2, "Orders should have been canceled");

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_execution_stream() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Set up event tracking
        let event_received = Arc::new(AtomicBool::new(false));
        let event_received_clone = event_received.clone();

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        // Start execution stream
        let callback = Arc::new(move |event: OrderEventAny| {
            println!("Received event via stream: {:?}", event);
            event_received_clone.store(true, Ordering::SeqCst);
        });

        let stream_result = venue.start_execution_stream(callback, shutdown_rx).await;
        assert!(stream_result.is_ok(), "Start stream failed: {:?}", stream_result.err());

        // Give the stream time to connect
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Submit an order - this should generate an event on the stream
        let order = create_test_buy_order("BTCUSDT", dec!(1000));
        let order_id = order.client_order_id.clone();

        let venue_order_id = venue.submit_order(&order).await.expect("Submit failed");

        // Wait for event (with timeout)
        let received = timeout(Duration::from_secs(10), async {
            while !event_received.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        // Cancel order and clean up
        venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await.ok();
        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        assert!(
            received.is_ok(),
            "Should have received execution event within timeout"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_order_rejected_invalid_symbol() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Try to submit order with invalid symbol
        let order = create_test_buy_order("INVALIDUSDT", dec!(1000));

        let result = venue.submit_order(&order).await;
        assert!(result.is_err(), "Should reject invalid symbol");

        let err = result.err().unwrap();
        println!("Expected error: {:?}", err);

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_spot_venue_order_rejected_quantity_too_small() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Try to submit order with quantity below minimum
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.00000001), dec!(1000))
            .with_time_in_force(TimeInForce::GTC)
            .build()
            .expect("Failed to create order");

        let result = venue.submit_order(&order).await;
        assert!(result.is_err(), "Should reject quantity too small");

        let err = result.err().unwrap();
        println!("Expected error: {:?}", err);

        venue.disconnect().await.ok();
    }
}

// ============================================================================
// Futures Venue Integration Tests
// ============================================================================

mod futures_tests {
    use super::*;
    // Note: Futures-specific methods (set_leverage, query_positions, etc.)
    // are directly on BinanceFuturesVenue, not via a separate trait

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_connect() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");

        let result = venue.connect().await;
        assert!(result.is_ok(), "Connect failed: {:?}", result.err());
        assert!(venue.is_connected());

        venue.disconnect().await.expect("Disconnect failed");
        assert!(!venue.is_connected());
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_query_balances() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        let balances = venue.query_balances().await;
        assert!(balances.is_ok(), "Query balances failed: {:?}", balances.err());

        let balances = balances.unwrap();
        println!("Futures balances: {:?}", balances);

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_set_leverage() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        // Set leverage to 10x
        let result = venue.set_leverage("BTCUSDT", 10).await;
        assert!(result.is_ok(), "Set leverage failed: {:?}", result.err());

        // Query leverage to verify
        let leverage = venue.get_leverage("BTCUSDT").await;
        assert!(leverage.is_ok(), "Get leverage failed: {:?}", leverage.err());
        assert_eq!(leverage.unwrap(), 10);

        // Set to different value
        venue.set_leverage("BTCUSDT", 5).await.expect("Set leverage failed");
        let leverage = venue.get_leverage("BTCUSDT").await.unwrap();
        assert_eq!(leverage, 5);

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_set_margin_type() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        // Try setting margin type (may fail if position exists)
        let result = venue.set_margin_type("BTCUSDT", MarginType::Isolated).await;
        println!("Set margin type result: {:?}", result);

        // Query positions to verify margin type
        let position = venue.query_position("BTCUSDT").await;
        println!("Position info: {:?}", position);

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_query_positions() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        let positions = venue.query_positions().await;
        assert!(positions.is_ok(), "Query positions failed: {:?}", positions.err());

        let positions = positions.unwrap();
        println!("Positions: {:?}", positions);

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_submit_and_cancel_order() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        // Set low leverage for safety
        venue.set_leverage("BTCUSDT", 1).await.ok();

        // Create limit buy far below market
        let order = create_test_buy_order("BTCUSDT", dec!(1000));
        let order_id = order.client_order_id.clone();

        println!("Submitting futures order: {:?}", order_id);

        let submit_result = venue.submit_order(&order).await;
        assert!(submit_result.is_ok(), "Submit failed: {:?}", submit_result.err());

        let venue_order_id = submit_result.unwrap();
        println!("Submit response - venue_order_id: {:?}", venue_order_id);

        // Query the order
        let query_result = venue.query_order(&order_id, Some(&venue_order_id), "BTCUSDT").await;
        assert!(query_result.is_ok(), "Query failed: {:?}", query_result.err());

        // Cancel the order
        let cancel_result = venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await;
        assert!(cancel_result.is_ok(), "Cancel failed: {:?}", cancel_result.err());

        println!("Cancel successful");

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_execution_stream() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        let event_received = Arc::new(AtomicBool::new(false));
        let event_received_clone = event_received.clone();

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let callback = Arc::new(move |event: OrderEventAny| {
            println!("Received futures event: {:?}", event);
            event_received_clone.store(true, Ordering::SeqCst);
        });

        venue
            .start_execution_stream(callback, shutdown_rx)
            .await
            .expect("Start stream failed");

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Submit order to trigger event
        let order = create_test_buy_order("BTCUSDT", dec!(1000));
        let order_id = order.client_order_id.clone();

        let venue_order_id = venue.submit_order(&order).await.expect("Submit failed");

        let received = timeout(Duration::from_secs(10), async {
            while !event_received.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        venue.cancel_order(&order_id, Some(&venue_order_id), "BTCUSDT").await.ok();
        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        assert!(
            received.is_ok(),
            "Should have received execution event within timeout"
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_futures_venue_position_mode() {
        require_testnet_keys!();

        let mut venue = create_binance_futures_testnet().expect("Failed to create futures venue");
        venue.connect().await.expect("Connect failed");

        // Try to set position mode (may fail if positions exist)
        // One-way mode (hedge_mode = false)
        let result = venue.set_position_mode(false).await;
        println!("Set position mode (one-way) result: {:?}", result);

        venue.disconnect().await.ok();
    }
}

// ============================================================================
// End-to-End Strategy Integration Tests
// ============================================================================

mod e2e_tests {
    use super::*;

    /// Test the full flow: Strategy → Venue → Testnet → Venue → Strategy
    #[tokio::test]
    #[ignore]
    async fn test_e2e_spot_order_flow() {
        require_testnet_keys!();

        // Create and connect venue
        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
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

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Simulate strategy generating an order
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), dec!(1000))
            .with_time_in_force(TimeInForce::GTC)
            .build()
            .expect("Failed to create order");
        let order_id = order.client_order_id.clone();

        println!("\n=== E2E Test: Submitting order ===");
        println!("Order ID: {:?}", order_id);

        // Submit through venue (simulating framework)
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

        // Should have received at least NEW and CANCELED events
        assert!(
            events.len() >= 2,
            "Expected at least 2 events (new + cancel), got {}",
            events.len()
        );

        // Verify event types
        let has_accepted = events.iter().any(|e| matches!(e, OrderEventAny::Accepted(_)));
        let has_canceled = events.iter().any(|e| matches!(e, OrderEventAny::Canceled(_)));

        assert!(has_accepted, "Should have received Accepted event");
        assert!(has_canceled, "Should have received Canceled event");

        // Clean up
        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        println!("\n=== E2E Test: PASSED ===");
    }

    /// Test order rejection flow
    #[tokio::test]
    #[ignore]
    async fn test_e2e_order_rejection_flow() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        // Submit invalid order (quantity too small)
        let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.00000001), dec!(1000))
            .with_time_in_force(TimeInForce::GTC)
            .build()
            .expect("Failed to create order");

        println!("\n=== E2E Rejection Test: Submitting invalid order ===");

        let result = venue.submit_order(&order).await;

        // Should be rejected
        assert!(result.is_err(), "Invalid order should be rejected");

        let err = result.err().unwrap();
        println!("Rejection error: {:?}", err);

        venue.disconnect().await.ok();

        println!("\n=== E2E Rejection Test: PASSED ===");
    }

    /// Test multiple orders in sequence
    #[tokio::test]
    #[ignore]
    async fn test_e2e_multiple_orders_flow() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        let mut order_ids = Vec::new();

        println!("\n=== E2E Multi-Order Test: Submitting 3 orders ===");

        // Submit multiple orders
        for i in 0..3 {
            let price = dec!(1000) + Decimal::from(i);
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), price)
                .with_time_in_force(TimeInForce::GTC)
                .build()
                .expect("Failed to create order");

            let order_id = order.client_order_id.clone();
            println!("  Submitting order {}: {:?} @ {}", i, order_id, price);

            let result = venue.submit_order(&order).await;
            if result.is_ok() {
                println!("  Order {} submitted successfully", i);
                order_ids.push(order_id);
            } else {
                println!("  Order {} failed: {:?}", i, result.err());
            }
        }

        // Query all open orders
        tokio::time::sleep(Duration::from_millis(500)).await;
        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await.unwrap();
        println!("\n  Open orders count: {}", open_orders.len());

        // Verify all orders exist
        for order_id in &order_ids {
            let found = open_orders.iter().any(|o| &o.client_order_id == order_id);
            assert!(found, "Order {:?} should be in open orders", order_id);
        }

        // Cancel all orders
        println!("\n=== E2E Multi-Order Test: Canceling all orders ===");
        let cancel_result = venue.cancel_all_orders(Some("BTCUSDT")).await;
        assert!(cancel_result.is_ok(), "Batch cancel failed: {:?}", cancel_result.err());

        // Verify all canceled
        tokio::time::sleep(Duration::from_millis(500)).await;
        let open_orders = venue.query_open_orders(Some("BTCUSDT")).await.unwrap();
        assert!(
            open_orders.is_empty(),
            "All orders should be canceled, but {} remain",
            open_orders.len()
        );

        venue.disconnect().await.ok();

        println!("\n=== E2E Multi-Order Test: PASSED ===");
    }
}

// ============================================================================
// Stress / Rate Limit Tests
// ============================================================================

mod stress_tests {
    use super::*;

    /// Test rapid order submission (within rate limits)
    #[tokio::test]
    #[ignore]
    async fn test_rapid_order_submission() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");
        venue.connect().await.expect("Connect failed");

        println!("\n=== Rapid Submission Test: Submitting 5 orders quickly ===");

        let mut order_ids = Vec::new();

        for i in 0..5 {
            let price = dec!(1000) + Decimal::from(i);
            let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), price)
                .with_time_in_force(TimeInForce::GTC)
                .build()
                .expect("Failed to create order");

            let order_id = order.client_order_id.clone();

            let result = venue.submit_order(&order).await;
            if result.is_ok() {
                println!("  Order {} submitted successfully", i);
                order_ids.push(order_id);
            } else {
                println!("  Order {} failed: {:?}", i, result.err());
            }
        }

        println!("  Successfully submitted {} orders", order_ids.len());
        assert!(order_ids.len() >= 3, "Should submit most orders successfully");

        // Clean up
        venue.cancel_all_orders(Some("BTCUSDT")).await.ok();
        venue.disconnect().await.ok();

        println!("\n=== Rapid Submission Test: PASSED ===");
    }

    /// Test reconnection after disconnect
    #[tokio::test]
    #[ignore]
    async fn test_reconnection() {
        require_testnet_keys!();

        let mut venue = create_binance_spot_testnet().expect("Failed to create spot venue");

        // Connect
        venue.connect().await.expect("First connect failed");
        assert!(venue.is_connected());

        // Query to verify connection works
        let balances = venue.query_balances().await;
        assert!(balances.is_ok(), "Query should work when connected");

        // Disconnect
        venue.disconnect().await.expect("Disconnect failed");
        assert!(!venue.is_connected());

        // Reconnect
        venue.connect().await.expect("Reconnect failed");
        assert!(venue.is_connected());

        // Query again
        let balances = venue.query_balances().await;
        assert!(balances.is_ok(), "Query should work after reconnect");

        venue.disconnect().await.ok();

        println!("=== Reconnection Test: PASSED ===");
    }
}
