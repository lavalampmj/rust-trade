//! User Data Stream for Binance execution reports.
//!
//! The User Data Stream WebSocket provides real-time updates for:
//! - Order execution reports (fills, cancels, etc.)
//! - Account balance updates
//! - Position updates (futures)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::execution::venue::config::StreamConfig;
use crate::execution::venue::error::VenueResult;
use crate::execution::venue::http::HttpClient;
use crate::execution::venue::websocket::{MessageCallback, WebSocketClient};

use super::listen_key::ListenKeyManager;

/// Callback for raw JSON messages from the user data stream.
pub type RawMessageCallback = Arc<dyn Fn(String) + Send + Sync>;

/// User data stream for Binance.
///
/// This manages the WebSocket connection to the User Data Stream,
/// including listen key management and reconnection.
///
/// # Example
///
/// ```ignore
/// let stream = BinanceUserDataStream::new(
///     http_client,
///     stream_config,
///     endpoints.user_data_ws_url.clone(),
///     "/api/v3/userDataStream",
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
pub struct BinanceUserDataStream {
    /// HTTP client for listen key management
    http_client: Arc<HttpClient>,
    /// Stream configuration
    stream_config: StreamConfig,
    /// Base WebSocket URL
    base_ws_url: String,
    /// Listen key endpoint
    listen_key_endpoint: String,
    /// Whether the stream is active
    is_active: Arc<AtomicBool>,
}

impl BinanceUserDataStream {
    /// Create a new user data stream.
    ///
    /// # Arguments
    ///
    /// * `http_client` - HTTP client for listen key API calls
    /// * `stream_config` - WebSocket stream configuration
    /// * `base_ws_url` - Base WebSocket URL (without listen key)
    /// * `listen_key_endpoint` - REST endpoint for listen key operations
    pub fn new(
        http_client: Arc<HttpClient>,
        stream_config: StreamConfig,
        base_ws_url: impl Into<String>,
        listen_key_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            http_client,
            stream_config,
            base_ws_url: base_ws_url.into(),
            listen_key_endpoint: listen_key_endpoint.into(),
            is_active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the user data stream.
    ///
    /// This method:
    /// 1. Creates a listen key
    /// 2. Starts the listen key keepalive task
    /// 3. Connects to the WebSocket
    /// 4. Runs until shutdown
    ///
    /// # Arguments
    ///
    /// * `callback` - Called for each message received
    /// * `shutdown_rx` - Receiver for shutdown signal
    pub async fn start(
        &self,
        callback: RawMessageCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        info!("Starting Binance user data stream");

        // Create listen key manager
        let listen_key_manager = ListenKeyManager::new(
            self.http_client.clone(),
            self.listen_key_endpoint.clone(),
        );

        // Create a new broadcast sender for internal shutdown coordination
        let (internal_shutdown_tx, _) = broadcast::channel::<()>(1);

        // Start listen key manager
        let listen_key = listen_key_manager
            .start(internal_shutdown_tx.subscribe())
            .await?;

        // Build WebSocket URL with listen key
        let _ws_url = format!("{}/{}", self.base_ws_url, listen_key);
        info!("Connecting to user data stream: {}", self.base_ws_url);

        // Create WebSocket client
        let ws_client = WebSocketClient::new(self.stream_config.clone());

        // Mark as active
        self.is_active.store(true, Ordering::SeqCst);

        // Wrap callback to handle listen key expiry
        let _listen_key_endpoint = self.listen_key_endpoint.clone();
        let wrapped_callback: MessageCallback = {
            let callback = callback.clone();
            Arc::new(move |msg: String| {
                // Check for listen key expiry
                if msg.contains("listenKeyExpired") {
                    warn!("Listen key expired, stream will reconnect");
                }
                callback(msg);
            })
        };

        // Run WebSocket with reconnection
        let result = self
            .run_with_reconnect(
                ws_client,
                listen_key_manager,
                wrapped_callback,
                &mut shutdown_rx,
            )
            .await;

        // Mark as inactive
        self.is_active.store(false, Ordering::SeqCst);

        result
    }

    /// Run WebSocket with automatic reconnection on listen key expiry.
    async fn run_with_reconnect(
        &self,
        ws_client: WebSocketClient,
        listen_key_manager: ListenKeyManager,
        callback: MessageCallback,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        let (internal_shutdown_tx, _) = broadcast::channel::<()>(1);

        loop {
            // Check for shutdown
            if shutdown_rx.try_recv().is_ok() {
                info!("Shutdown requested, stopping user data stream");
                let _ = listen_key_manager.stop().await;
                return Ok(());
            }

            // Get current listen key or create new one
            let listen_key = match listen_key_manager.get_listen_key().await {
                Some(key) => key,
                None => {
                    // Need to create a new listen key
                    listen_key_manager
                        .start(internal_shutdown_tx.subscribe())
                        .await?
                }
            };

            let ws_url = format!("{}/{}", self.base_ws_url, listen_key);

            // Run WebSocket
            let internal_shutdown_rx = internal_shutdown_tx.subscribe();
            let result = ws_client
                .run_with_url(&ws_url, callback.clone(), internal_shutdown_rx)
                .await;

            match result {
                Ok(()) => {
                    // Graceful shutdown
                    let _ = listen_key_manager.stop().await;
                    return Ok(());
                }
                Err(e) => {
                    warn!("User data stream error: {}. Reconnecting...", e);

                    // Check if it's a shutdown
                    if shutdown_rx.try_recv().is_ok() {
                        info!("Shutdown requested during reconnect");
                        let _ = listen_key_manager.stop().await;
                        return Ok(());
                    }

                    // Wait before reconnecting
                    tokio::time::sleep(self.stream_config.reconnect_initial_delay()).await;
                }
            }
        }
    }

    /// Check if the stream is active.
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
    }
}

/// Parse user data event type from JSON message.
pub fn parse_event_type(json: &str) -> Option<String> {
    // Quick parse to find event type
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
        // Spot uses "e" field
        if let Some(e) = value.get("e").and_then(|v| v.as_str()) {
            return Some(e.to_string());
        }
        // Futures uses "E" for event time and "e" for event type, but also has specific event names
        if let Some(e) = value.get("e").and_then(|v| v.as_str()) {
            return Some(e.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_event_type_execution_report() {
        let json = r#"{"e":"executionReport","E":1234567890,"s":"BTCUSDT"}"#;
        let event_type = parse_event_type(json);
        assert_eq!(event_type, Some("executionReport".to_string()));
    }

    #[test]
    fn test_parse_event_type_account_update() {
        let json = r#"{"e":"outboundAccountPosition","E":1234567890}"#;
        let event_type = parse_event_type(json);
        assert_eq!(event_type, Some("outboundAccountPosition".to_string()));
    }

    #[test]
    fn test_parse_event_type_listen_key_expired() {
        let json = r#"{"e":"listenKeyExpired","E":1234567890}"#;
        let event_type = parse_event_type(json);
        assert_eq!(event_type, Some("listenKeyExpired".to_string()));
    }

    #[test]
    fn test_parse_event_type_futures() {
        let json = r#"{"e":"ORDER_TRADE_UPDATE","E":1234567890}"#;
        let event_type = parse_event_type(json);
        assert_eq!(event_type, Some("ORDER_TRADE_UPDATE".to_string()));
    }

    #[test]
    fn test_parse_event_type_invalid_json() {
        let json = "not valid json";
        let event_type = parse_event_type(json);
        assert_eq!(event_type, None);
    }
}
