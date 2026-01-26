//! WebSocket client infrastructure for execution venues.
//!
//! This module provides venue-agnostic WebSocket client components:
//!
//! - [`WebSocketClient`]: Reconnecting WebSocket client
//! - [`WebSocketClientBuilder`]: Builder for custom configuration
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use trading_common::execution::venue::websocket::WebSocketClientBuilder;
//!
//! // Create a WebSocket client
//! let client = WebSocketClientBuilder::new("wss://stream.binance.com:9443/ws")
//!     .max_reconnect_attempts(10)
//!     .ping_interval(std::time::Duration::from_secs(30))
//!     .build();
//!
//! // Define message handler
//! let callback = Arc::new(|msg: String| {
//!     println!("Received: {}", msg);
//! });
//!
//! // Run the client (blocks until shutdown)
//! client.run(callback, shutdown_rx).await?;
//! ```

mod client;

pub use client::{MessageCallback, WebSocketClient, WebSocketClientBuilder, WsStream};
