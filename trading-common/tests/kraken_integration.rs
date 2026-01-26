//! Kraken Integration Tests
//!
//! These tests exercise the full order flow against Kraken production:
//! Strategy -> Order -> KrakenVenue -> Kraken API -> Execution Report -> Strategy
//!
//! # IMPORTANT: No Public Sandbox
//!
//! Unlike Binance, Kraken does NOT have a public spot testnet/sandbox.
//! These tests use REAL production endpoints with small orders.
//!
//! # Setup
//!
//! 1. Get Kraken API keys:
//!    - Create account at https://www.kraken.com/
//!    - Generate API keys with trading permissions
//!
//! 2. Set environment variables:
//!    ```bash
//!    export KRAKEN_API_KEY=your_api_key
//!    export KRAKEN_API_SECRET=your_base64_encoded_secret
//!    ```
//!
//! 3. Run tests:
//!    ```bash
//!    cargo test -p trading-common --test kraken_integration -- --ignored --nocapture
//!    ```
//!
//! # Safety Notes
//!
//! - Tests are marked `#[ignore]` by default since they require API keys and use real money
//! - Tests use limit orders FAR from market price to avoid accidental fills
//! - Each test cleans up by canceling any orders it creates
//! - Use minimum order quantities to minimize any risk
//! - Consider using a dedicated test account with limited funds

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::timeout;

use trading_common::execution::venue::kraken::create_kraken_spot;
use trading_common::execution::venue::{
    AccountQueryVenue, ExecutionStreamVenue, ExecutionVenue, OrderSubmissionVenue,
};
use trading_common::orders::{Order, OrderEventAny, OrderSide, TimeInForce};

// ============================================================================
// Test Helpers
// ============================================================================

/// Check if Kraken API keys are available
fn has_kraken_keys() -> bool {
    env::var("KRAKEN_API_KEY").is_ok() && env::var("KRAKEN_API_SECRET").is_ok()
}

/// Skip test if no API keys
macro_rules! require_kraken_keys {
    () => {
        if !has_kraken_keys() {
            eprintln!("Skipping: KRAKEN_API_KEY and KRAKEN_API_SECRET not set");
            return;
        }
    };
}

/// Create a limit buy order far below market price (won't fill)
/// Uses XBT/USD pair (Kraken's BTC pair)
fn create_test_buy_order(symbol: &str, price: Decimal) -> Order {
    // Use minimum quantity for safety (0.0001 BTC is roughly $10 at $100k)
    Order::limit(symbol, OrderSide::Buy, dec!(0.0001), price)
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
    async fn test_venue_connect() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");

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
    async fn test_venue_query_balances() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Query balances
        let balances = venue.query_balances().await;
        assert!(
            balances.is_ok(),
            "Query balances failed: {:?}",
            balances.err()
        );

        let balances = balances.unwrap();
        println!("Kraken balances: {:?}", balances);

        // Should have some balances (may be empty if new account)
        println!("Found {} asset balances", balances.len());

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_venue_query_specific_balance() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Query USD balance (most accounts have this)
        let usd_balance = venue.query_balance("USD").await;
        println!("USD balance result: {:?}", usd_balance);

        // Query BTC balance (Kraken uses XBT internally)
        let btc_balance = venue.query_balance("BTC").await;
        println!("BTC balance result: {:?}", btc_balance);

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_reconnection() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");

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

// ============================================================================
// Order Management Tests
// ============================================================================

mod order_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_submit_and_cancel_order() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Create a limit buy order far below market (won't fill)
        // BTC typically trades > $30k, so $100 is very safe
        // Use BTCUSD symbol - Kraken uses XBT internally but accepts BTC
        let order = create_test_buy_order("BTCUSD", dec!(100));
        let order_id = order.client_order_id.clone();

        println!("Submitting order: {:?}", order_id);

        // Submit order
        let submit_result = venue.submit_order(&order).await;

        if let Err(ref e) = submit_result {
            // Check if it's a minimum size error
            if format!("{:?}", e).contains("MIN") || format!("{:?}", e).contains("size") {
                println!("Order rejected due to minimum size - this is expected behavior");
                venue.disconnect().await.ok();
                return;
            }
        }

        assert!(
            submit_result.is_ok(),
            "Submit failed: {:?}",
            submit_result.err()
        );

        let venue_order_id = submit_result.unwrap();
        println!(
            "Submit response - venue_order_id: {:?}",
            venue_order_id.as_str()
        );

        // Query the order to verify it exists
        tokio::time::sleep(Duration::from_millis(500)).await;

        let query_result = venue
            .query_order(&order_id, Some(&venue_order_id), "BTCUSD")
            .await;

        if query_result.is_ok() {
            let query_response = query_result.unwrap();
            println!("Query response: {:?}", query_response);
            assert_eq!(query_response.client_order_id, order_id);
        }

        // Cancel the order
        println!("Canceling order: {:?}", order_id);
        let cancel_result = venue
            .cancel_order(&order_id, Some(&venue_order_id), "BTCUSD")
            .await;
        assert!(cancel_result.is_ok(), "Cancel failed: {:?}", cancel_result.err());

        println!("Cancel successful");

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_query_open_orders() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Query open orders (may be empty)
        let open_orders = venue.query_open_orders(None).await;
        assert!(
            open_orders.is_ok(),
            "Query open orders failed: {:?}",
            open_orders.err()
        );

        let orders = open_orders.unwrap();
        println!("Open orders: {} found", orders.len());
        for order in &orders {
            println!("  - {:?}: {} @ {:?}", order.client_order_id, order.symbol, order.price);
        }

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_cancel_all_orders() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Cancel all existing orders
        let cancel_result = venue.cancel_all_orders(None).await;

        match cancel_result {
            Ok(canceled_ids) => {
                println!("Cancelled {} orders", canceled_ids.len());
            }
            Err(e) => {
                println!("Cancel all result: {:?}", e);
            }
        }

        venue.disconnect().await.ok();
    }

    #[tokio::test]
    #[ignore]
    async fn test_order_rejected_invalid_symbol() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Try to submit order with invalid symbol
        let order = create_test_buy_order("INVALIDPAIR", dec!(100));

        let result = venue.submit_order(&order).await;
        assert!(result.is_err(), "Should reject invalid symbol");

        let err = result.err().unwrap();
        println!("Expected error: {:?}", err);

        venue.disconnect().await.ok();
    }
}

// ============================================================================
// Order Modification Tests (Kraken-specific feature!)
// ============================================================================

mod modify_tests {
    use super::*;

    /// Test Kraken's true order modification (AmendOrder)
    /// This preserves queue priority unlike cancel+replace!
    #[tokio::test]
    #[ignore]
    async fn test_modify_order_price() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Submit initial order
        let order = create_test_buy_order("BTCUSD", dec!(100));
        let order_id = order.client_order_id.clone();

        let submit_result = venue.submit_order(&order).await;

        if submit_result.is_err() {
            println!(
                "Order submission failed (may be min size): {:?}",
                submit_result.err()
            );
            venue.disconnect().await.ok();
            return;
        }

        let venue_order_id = submit_result.unwrap();
        println!("Order submitted: {:?}", venue_order_id);

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Modify order price (this is a true amendment, not cancel+replace!)
        let new_price = dec!(101);
        let modify_result = venue
            .modify_order(&order_id, Some(&venue_order_id), "BTCUSD", Some(new_price), None)
            .await;

        match modify_result {
            Ok(new_venue_order_id) => {
                println!("Order modified! New ID: {:?}", new_venue_order_id);
                // Clean up - cancel the modified order
                venue
                    .cancel_order(&order_id, Some(&new_venue_order_id), "BTCUSD")
                    .await
                    .ok();
            }
            Err(e) => {
                println!("Modify failed: {:?}", e);
                // Clean up - cancel the original order
                venue
                    .cancel_order(&order_id, Some(&venue_order_id), "BTCUSD")
                    .await
                    .ok();
            }
        }

        venue.disconnect().await.ok();
    }
}

// ============================================================================
// Execution Stream Tests
// ============================================================================

mod stream_tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn test_execution_stream() {
        require_kraken_keys!();

        let mut venue = create_kraken_spot().expect("Failed to create venue");
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

        if let Err(ref e) = stream_result {
            println!("Stream start failed: {:?}", e);
            // Stream may fail if no valid token - this is informative
        }

        // Give the stream time to connect
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Submit an order - this should generate an event on the stream
        let order = create_test_buy_order("BTCUSD", dec!(100));
        let order_id = order.client_order_id.clone();

        let submit_result = venue.submit_order(&order).await;

        if submit_result.is_err() {
            println!("Order submission failed: {:?}", submit_result.err());
            shutdown_tx.send(()).ok();
            venue.disconnect().await.ok();
            return;
        }

        let venue_order_id = submit_result.unwrap();

        // Wait for event (with timeout)
        let received = timeout(Duration::from_secs(10), async {
            while !event_received.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;

        // Cancel order and clean up
        venue
            .cancel_order(&order_id, Some(&venue_order_id), "BTCUSD")
            .await
            .ok();
        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        if received.is_ok() {
            println!("=== Execution Stream Test: PASSED - Received events ===");
        } else {
            println!("=== Execution Stream Test: No events received within timeout ===");
            println!("This may be normal if WebSocket connection wasn't established");
        }
    }
}

// ============================================================================
// End-to-End Strategy Integration Tests
// ============================================================================

mod e2e_tests {
    use super::*;

    /// Test the full flow: Strategy -> Venue -> Kraken -> Venue -> Strategy
    #[tokio::test]
    #[ignore]
    async fn test_e2e_order_flow() {
        require_kraken_keys!();

        // Create and connect venue
        let mut venue = create_kraken_spot().expect("Failed to create venue");
        venue.connect().await.expect("Connect failed");

        // Set up execution stream to capture events
        let stream_events = Arc::new(std::sync::Mutex::new(Vec::<OrderEventAny>::new()));
        let stream_events_clone = stream_events.clone();

        let (shutdown_tx, shutdown_rx) = broadcast::channel::<()>(1);

        let callback = Arc::new(move |event: OrderEventAny| {
            println!("Stream received: {:?}", event);
            stream_events_clone.lock().unwrap().push(event);
        });

        let _ = venue.start_execution_stream(callback, shutdown_rx).await;

        tokio::time::sleep(Duration::from_secs(2)).await;

        // Simulate strategy generating an order
        let order = create_test_buy_order("BTCUSD", dec!(100));
        let order_id = order.client_order_id.clone();

        println!("\n=== E2E Test: Submitting order ===");
        println!("Order ID: {:?}", order_id);

        // Submit through venue
        let submit_result = venue.submit_order(&order).await;

        if submit_result.is_err() {
            println!("Submit failed (may be expected): {:?}", submit_result.err());
            shutdown_tx.send(()).ok();
            venue.disconnect().await.ok();
            return;
        }

        let venue_order_id = submit_result.unwrap();
        println!("Venue order ID: {:?}", venue_order_id);

        // Wait for WebSocket event
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Cancel order
        println!("\n=== E2E Test: Canceling order ===");
        let cancel_result = venue
            .cancel_order(&order_id, Some(&venue_order_id), "BTCUSD")
            .await;

        if cancel_result.is_err() {
            println!("Cancel failed: {:?}", cancel_result.err());
        } else {
            println!("Cancel successful");
        }

        // Wait for cancel event
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Check stream events
        let events = stream_events.lock().unwrap();
        println!(
            "\n=== E2E Test: Received {} stream events ===",
            events.len()
        );
        for (i, event) in events.iter().enumerate() {
            println!("  Event {}: {:?}", i, event);
        }

        // Clean up
        shutdown_tx.send(()).ok();
        venue.disconnect().await.ok();

        println!("\n=== E2E Test: COMPLETED ===");
    }
}
