//! User execution stream for Kraken WebSocket V2.
//!
//! The Kraken WebSocket V2 API provides real-time updates for:
//! - Order execution reports (fills, cancels, amendments)
//! - Position updates
//!
//! Key differences from Binance:
//! - Uses WebSocket V2 with JSON-RPC style messages
//! - Token is sent in the subscription message, not the URL
//! - Token doesn't expire once connected (no keepalive needed)
//! - Uses "executions" channel for order updates

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tracing::{debug, info, warn};

use crate::execution::venue::config::StreamConfig;
use crate::execution::venue::error::{VenueError, VenueResult};

use super::ws_token::KrakenWsTokenManager;
use super::RawMessageCallback;

/// User execution stream for Kraken.
///
/// Manages the WebSocket connection to the authenticated executions channel.
///
/// # Example
///
/// ```ignore
/// let stream = KrakenUserStream::new(
///     token_manager,
///     stream_config,
///     "wss://ws-auth.kraken.com/v2",
/// );
///
/// let callback = Arc::new(|msg: String| {
///     // Parse and handle execution report
///     println!("Received: {}", msg);
/// });
///
/// // Start the stream (blocks until shutdown)
/// stream.start(callback, shutdown_rx).await?;
/// ```
pub struct KrakenUserStream {
    /// Token manager for authentication
    token_manager: Arc<KrakenWsTokenManager>,
    /// Stream configuration
    stream_config: StreamConfig,
    /// WebSocket URL
    ws_url: String,
    /// Whether the stream is active
    is_active: Arc<AtomicBool>,
}

impl KrakenUserStream {
    /// Create a new user stream.
    ///
    /// # Arguments
    ///
    /// * `token_manager` - Token manager for authentication
    /// * `stream_config` - WebSocket stream configuration
    /// * `ws_url` - Authenticated WebSocket URL
    pub fn new(
        token_manager: Arc<KrakenWsTokenManager>,
        stream_config: StreamConfig,
        ws_url: impl Into<String>,
    ) -> Self {
        Self {
            token_manager,
            stream_config,
            ws_url: ws_url.into(),
            is_active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the user stream.
    ///
    /// This method:
    /// 1. Gets a WebSocket token
    /// 2. Connects to the WebSocket
    /// 3. Subscribes to the executions channel
    /// 4. Runs until shutdown
    ///
    /// # Arguments
    ///
    /// * `callback` - Called for each execution message received
    /// * `shutdown_rx` - Receiver for shutdown signal
    pub async fn start(
        &self,
        callback: RawMessageCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        info!("Starting Kraken user execution stream");

        self.is_active.store(true, Ordering::SeqCst);

        let result = self.run_with_reconnect(callback, &mut shutdown_rx).await;

        self.is_active.store(false, Ordering::SeqCst);

        result
    }

    /// Run WebSocket with automatic reconnection.
    async fn run_with_reconnect(
        &self,
        callback: RawMessageCallback,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        let mut reconnect_attempts = 0u32;
        let max_attempts = self.stream_config.reconnect_max_attempts;

        loop {
            // Check for shutdown
            if shutdown_rx.try_recv().is_ok() {
                info!("Shutdown requested, stopping user stream");
                return Ok(());
            }

            // Get token
            let token = match self.token_manager.get_token().await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Failed to get WebSocket token: {}", e);
                    if reconnect_attempts >= max_attempts {
                        return Err(e);
                    }
                    reconnect_attempts += 1;
                    tokio::time::sleep(self.stream_config.reconnect_initial_delay()).await;
                    continue;
                }
            };

            // Connect and run
            match self.run_once(&token, callback.clone(), shutdown_rx).await {
                Ok(()) => {
                    // Graceful shutdown
                    return Ok(());
                }
                Err(e) => {
                    warn!("User stream error: {}. Reconnecting...", e);

                    // Check for shutdown
                    if shutdown_rx.try_recv().is_ok() {
                        info!("Shutdown requested during reconnect");
                        return Ok(());
                    }

                    reconnect_attempts += 1;
                    if reconnect_attempts >= max_attempts {
                        return Err(VenueError::Stream(format!(
                            "Max reconnection attempts ({}) exceeded",
                            max_attempts
                        )));
                    }

                    // Clear token cache on error to get fresh token
                    self.token_manager.clear_cache().await;

                    // Exponential backoff
                    let delay = std::cmp::min(
                        self.stream_config.reconnect_initial_delay()
                            * 2u32.pow(reconnect_attempts),
                        self.stream_config.reconnect_max_delay(),
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Run a single WebSocket connection.
    async fn run_once(
        &self,
        token: &str,
        callback: RawMessageCallback,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        info!("Connecting to Kraken WebSocket: {}", self.ws_url);

        // Connect
        let (ws_stream, _) = connect_async(&self.ws_url)
            .await
            .map_err(|e| VenueError::Connection(format!("WebSocket connect failed: {}", e)))?;

        let (mut write, mut read) = ws_stream.split();

        info!("Connected to Kraken WebSocket");

        // Subscribe to executions channel with token
        let subscribe_msg = json!({
            "method": "subscribe",
            "params": {
                "channel": "executions",
                "token": token,
                "snap_orders": true,  // Get snapshot of open orders
                "snap_trades": false  // Don't need trade history
            }
        });

        write
            .send(tokio_tungstenite::tungstenite::Message::Text(
                subscribe_msg.to_string().into(),
            ))
            .await
            .map_err(|e| VenueError::Stream(format!("Failed to send subscribe: {}", e)))?;

        debug!("Sent subscription request for executions channel");

        // Ping interval
        let ping_interval = self.stream_config.ping_interval();
        let mut ping_timer = tokio::time::interval(ping_interval);
        ping_timer.tick().await; // Skip first tick

        loop {
            tokio::select! {
                // Check for shutdown
                _ = shutdown_rx.recv() => {
                    info!("Shutdown received, closing WebSocket");
                    let _ = write.close().await;
                    return Ok(());
                }

                // Ping timer
                _ = ping_timer.tick() => {
                    let ping_msg = json!({
                        "method": "ping"
                    });
                    if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(
                        ping_msg.to_string().into()
                    )).await {
                        warn!("Failed to send ping: {}", e);
                        return Err(VenueError::Stream(format!("Ping failed: {}", e)));
                    }
                }

                // Message received
                msg = read.next() => {
                    match msg {
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                            self.handle_message(&text, &callback);
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Ping(data))) => {
                            // Respond to ping
                            let _ = write.send(tokio_tungstenite::tungstenite::Message::Pong(data)).await;
                        }
                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                            info!("WebSocket closed by server");
                            return Err(VenueError::Stream("Server closed connection".to_string()));
                        }
                        Some(Err(e)) => {
                            return Err(VenueError::Stream(format!("WebSocket error: {}", e)));
                        }
                        None => {
                            return Err(VenueError::Stream("WebSocket stream ended".to_string()));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Handle a received message.
    fn handle_message(&self, text: &str, callback: &RawMessageCallback) {
        // Parse to check message type
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            // Check for channel
            if let Some(channel) = value.get("channel").and_then(|v| v.as_str()) {
                if channel == "executions" {
                    // This is an execution update, call callback
                    callback(text.to_string());
                } else if channel == "heartbeat" {
                    debug!("Received heartbeat");
                } else if channel == "status" {
                    debug!("Received status: {}", text);
                }
            } else if let Some(method) = value.get("method").and_then(|v| v.as_str()) {
                // Response to our request
                match method {
                    "pong" => {
                        debug!("Received pong");
                    }
                    "subscribe" => {
                        if value.get("success").and_then(|v| v.as_bool()) == Some(true) {
                            info!("Successfully subscribed to executions channel");
                        } else {
                            warn!("Subscription failed: {}", text);
                        }
                    }
                    _ => {
                        debug!("Received response: {}", text);
                    }
                }
            } else if value.get("error").is_some() {
                warn!("Received error: {}", text);
            }
        }
    }

    /// Check if the stream is active.
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }
}

/// Parse execution event type from JSON message.
#[allow(dead_code)]
pub fn parse_execution_type(json: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
        // Check if it's an executions channel update
        if value.get("channel").and_then(|v| v.as_str()) == Some("executions") {
            // Get exec_type from the first item in data array
            if let Some(data) = value.get("data").and_then(|v| v.as_array()) {
                if let Some(first) = data.first() {
                    return first
                        .get("exec_type")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_execution_type_new() {
        let json = r#"{
            "channel": "executions",
            "type": "update",
            "data": [{
                "exec_type": "new",
                "order_id": "OXXXXX-XXXXX-XXXXXX"
            }]
        }"#;
        let exec_type = parse_execution_type(json);
        assert_eq!(exec_type, Some("new".to_string()));
    }

    #[test]
    fn test_parse_execution_type_trade() {
        let json = r#"{
            "channel": "executions",
            "type": "update",
            "data": [{
                "exec_type": "trade",
                "order_id": "OXXXXX-XXXXX-XXXXXX",
                "last_qty": "0.5"
            }]
        }"#;
        let exec_type = parse_execution_type(json);
        assert_eq!(exec_type, Some("trade".to_string()));
    }

    #[test]
    fn test_parse_execution_type_non_execution() {
        let json = r#"{"channel": "heartbeat"}"#;
        let exec_type = parse_execution_type(json);
        assert_eq!(exec_type, None);
    }
}
