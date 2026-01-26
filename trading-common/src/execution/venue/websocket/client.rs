//! WebSocket client for venue streaming connections.
//!
//! This module provides a generic WebSocket client that handles:
//! - Automatic reconnection with exponential backoff
//! - Ping/pong keepalive
//! - Graceful shutdown
//! - Message parsing

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::time::interval;
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
    MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, info, warn};

use crate::execution::venue::config::StreamConfig;
use crate::execution::venue::error::{VenueError, VenueResult};

/// Type alias for WebSocket connection.
pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Callback for processing WebSocket messages.
pub type MessageCallback = Arc<dyn Fn(String) + Send + Sync>;

/// WebSocket client with automatic reconnection.
///
/// This client provides a robust WebSocket connection that:
/// - Automatically reconnects on disconnect with exponential backoff
/// - Handles ping/pong keepalive messages
/// - Supports graceful shutdown via broadcast channel
///
/// # Example
///
/// ```ignore
/// let config = StreamConfig {
///     ws_url: "wss://stream.binance.com:9443/ws".to_string(),
///     reconnect_max_attempts: 10,
///     ..Default::default()
/// };
///
/// let client = WebSocketClient::new(config);
///
/// let callback = Arc::new(|msg: String| {
///     println!("Received: {}", msg);
/// });
///
/// // Run the WebSocket (blocks until shutdown)
/// client.run(callback, shutdown_rx).await?;
/// ```
pub struct WebSocketClient {
    /// Configuration
    config: StreamConfig,
    /// Whether the client is connected
    connected: Arc<AtomicBool>,
    /// Current reconnection attempt count
    reconnect_attempts: Arc<AtomicU32>,
}

impl WebSocketClient {
    /// Create a new WebSocket client.
    pub fn new(config: StreamConfig) -> Self {
        Self {
            config,
            connected: Arc::new(AtomicBool::new(false)),
            reconnect_attempts: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Check if the client is connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// Get the current reconnection attempt count.
    pub fn reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts.load(Ordering::SeqCst)
    }

    /// Calculate reconnection delay with exponential backoff.
    fn reconnect_delay(&self, attempt: u32) -> Duration {
        let base = self.config.reconnect_initial_delay();
        let max = self.config.reconnect_max_delay();

        let delay = base.as_millis() as u64 * 2u64.pow(attempt.min(10));
        let delay = Duration::from_millis(delay);

        std::cmp::min(delay, max)
    }

    /// Connect to the WebSocket server.
    async fn connect(&self) -> VenueResult<WsStream> {
        debug!("Connecting to WebSocket: {}", self.config.ws_url);

        // Validate URL format
        let _url = url::Url::parse(&self.config.ws_url)
            .map_err(|e| VenueError::Configuration(format!("Invalid WebSocket URL: {}", e)))?;

        // tokio-tungstenite's connect_async accepts &str directly
        match connect_async(&self.config.ws_url).await {
            Ok((ws_stream, _response)) => {
                info!("WebSocket connected to {}", self.config.ws_url);
                self.connected.store(true, Ordering::SeqCst);
                self.reconnect_attempts.store(0, Ordering::SeqCst);
                Ok(ws_stream)
            }
            Err(e) => {
                error!("Failed to connect to WebSocket: {}", e);
                self.connected.store(false, Ordering::SeqCst);
                Err(VenueError::Connection(e.to_string()))
            }
        }
    }

    /// Run the WebSocket client with automatic reconnection.
    ///
    /// This method blocks until shutdown is signaled or max reconnection
    /// attempts are exceeded.
    ///
    /// # Arguments
    ///
    /// * `callback` - Called for each text message received
    /// * `shutdown_rx` - Receiver for shutdown signal
    pub async fn run(
        &self,
        callback: MessageCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        loop {
            // Check for shutdown before connecting
            if shutdown_rx.try_recv().is_ok() {
                info!("WebSocket shutdown requested");
                return Ok(());
            }

            // Check if we've exceeded max reconnection attempts
            let attempts = self.reconnect_attempts.load(Ordering::SeqCst);
            if attempts >= self.config.reconnect_max_attempts {
                error!(
                    "Max reconnection attempts ({}) exceeded",
                    self.config.reconnect_max_attempts
                );
                return Err(VenueError::Connection(
                    "Max reconnection attempts exceeded".to_string(),
                ));
            }

            // Try to connect
            let ws_stream = match self.connect().await {
                Ok(stream) => stream,
                Err(e) => {
                    self.reconnect_attempts.fetch_add(1, Ordering::SeqCst);
                    let delay = self.reconnect_delay(attempts);
                    warn!(
                        "Connection failed (attempt {}), retrying in {:?}: {}",
                        attempts + 1,
                        delay,
                        e
                    );
                    tokio::time::sleep(delay).await;
                    continue;
                }
            };

            // Run the connection
            let result = self
                .run_connection(ws_stream, callback.clone(), &mut shutdown_rx)
                .await;

            match result {
                Ok(()) => {
                    // Graceful shutdown
                    info!("WebSocket connection closed gracefully");
                    return Ok(());
                }
                Err(e) => {
                    self.connected.store(false, Ordering::SeqCst);

                    // Check if it's a shutdown
                    if shutdown_rx.try_recv().is_ok() {
                        info!("WebSocket shutdown requested during error handling");
                        return Ok(());
                    }

                    // Increment attempts and wait before reconnecting
                    self.reconnect_attempts.fetch_add(1, Ordering::SeqCst);
                    let delay = self.reconnect_delay(attempts);
                    warn!(
                        "Connection error (attempt {}), reconnecting in {:?}: {}",
                        attempts + 1,
                        delay,
                        e
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    /// Run a single WebSocket connection.
    async fn run_connection(
        &self,
        ws_stream: WsStream,
        callback: MessageCallback,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        let (mut write, mut read) = ws_stream.split();

        // Set up ping interval if configured
        let ping_interval = if self.config.ping_interval_secs > 0 {
            Some(interval(self.config.ping_interval()))
        } else {
            None
        };

        let mut ping_interval = ping_interval;

        loop {
            tokio::select! {
                // Check for shutdown signal
                _ = shutdown_rx.recv() => {
                    debug!("Shutdown signal received, closing WebSocket");
                    let _ = write.close().await;
                    return Ok(());
                }

                // Handle ping interval
                _ = async {
                    if let Some(ref mut interval) = ping_interval {
                        let _ = interval.tick().await;
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {
                    debug!("Sending ping");
                    if let Err(e) = write.send(Message::Ping(vec![])).await {
                        warn!("Failed to send ping: {}", e);
                        return Err(VenueError::Stream(format!("Ping failed: {}", e)));
                    }
                }

                // Handle incoming messages
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            debug!("Received text message: {} bytes", text.len());
                            callback(text);
                        }
                        Some(Ok(Message::Binary(data))) => {
                            // Some venues send binary data, convert to string
                            if let Ok(text) = String::from_utf8(data) {
                                debug!("Received binary message: {} bytes", text.len());
                                callback(text);
                            }
                        }
                        Some(Ok(Message::Ping(data))) => {
                            debug!("Received ping, sending pong");
                            if let Err(e) = write.send(Message::Pong(data)).await {
                                warn!("Failed to send pong: {}", e);
                            }
                        }
                        Some(Ok(Message::Pong(_))) => {
                            debug!("Received pong");
                        }
                        Some(Ok(Message::Close(frame))) => {
                            info!("WebSocket closed by server: {:?}", frame);
                            return Err(VenueError::Stream("Server closed connection".to_string()));
                        }
                        Some(Ok(Message::Frame(_))) => {
                            // Raw frame, ignore
                        }
                        Some(Err(e)) => {
                            error!("WebSocket error: {}", e);
                            return Err(VenueError::Stream(e.to_string()));
                        }
                        None => {
                            info!("WebSocket stream ended");
                            return Err(VenueError::Stream("Stream ended".to_string()));
                        }
                    }
                }
            }
        }
    }

    /// Connect and run with a custom URL (useful for dynamic URLs like listen keys).
    ///
    /// # Arguments
    ///
    /// * `url` - The WebSocket URL to connect to
    /// * `callback` - Called for each text message received
    /// * `shutdown_rx` - Receiver for shutdown signal
    pub async fn run_with_url(
        &self,
        url: &str,
        callback: MessageCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        let mut config = self.config.clone();
        config.ws_url = url.to_string();

        let client = WebSocketClient::new(config);
        client.run(callback, shutdown_rx).await
    }
}

impl Clone for WebSocketClient {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            connected: Arc::new(AtomicBool::new(false)),
            reconnect_attempts: Arc::new(AtomicU32::new(0)),
        }
    }
}

/// Builder for WebSocket client with custom configuration.
pub struct WebSocketClientBuilder {
    config: StreamConfig,
}

impl WebSocketClientBuilder {
    /// Create a new builder with the given URL.
    pub fn new(ws_url: impl Into<String>) -> Self {
        let mut config = StreamConfig::default();
        config.ws_url = ws_url.into();
        Self { config }
    }

    /// Set the maximum reconnection attempts.
    pub fn max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.config.reconnect_max_attempts = attempts;
        self
    }

    /// Set the initial reconnection delay.
    pub fn reconnect_initial_delay(mut self, delay: Duration) -> Self {
        self.config.reconnect_initial_delay_ms = delay.as_millis() as u64;
        self
    }

    /// Set the maximum reconnection delay.
    pub fn reconnect_max_delay(mut self, delay: Duration) -> Self {
        self.config.reconnect_max_delay_ms = delay.as_millis() as u64;
        self
    }

    /// Set the ping interval.
    pub fn ping_interval(mut self, interval: Duration) -> Self {
        self.config.ping_interval_secs = interval.as_secs();
        self
    }

    /// Disable ping.
    pub fn no_ping(mut self) -> Self {
        self.config.ping_interval_secs = 0;
        self
    }

    /// Build the WebSocket client.
    pub fn build(self) -> WebSocketClient {
        WebSocketClient::new(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconnect_delay() {
        let mut config = StreamConfig::default();
        config.reconnect_initial_delay_ms = 1000;
        config.reconnect_max_delay_ms = 60000;

        let client = WebSocketClient::new(config);

        // First attempt: 1s
        assert_eq!(client.reconnect_delay(0), Duration::from_millis(1000));

        // Second attempt: 2s
        assert_eq!(client.reconnect_delay(1), Duration::from_millis(2000));

        // Third attempt: 4s
        assert_eq!(client.reconnect_delay(2), Duration::from_millis(4000));

        // High attempt should be capped at max
        assert_eq!(client.reconnect_delay(20), Duration::from_millis(60000));
    }

    #[test]
    fn test_builder() {
        let client = WebSocketClientBuilder::new("wss://example.com/ws")
            .max_reconnect_attempts(5)
            .reconnect_initial_delay(Duration::from_secs(2))
            .ping_interval(Duration::from_secs(30))
            .build();

        assert_eq!(client.config.ws_url, "wss://example.com/ws");
        assert_eq!(client.config.reconnect_max_attempts, 5);
        assert_eq!(client.config.reconnect_initial_delay_ms, 2000);
        assert_eq!(client.config.ping_interval_secs, 30);
    }

    #[test]
    fn test_client_initial_state() {
        let client = WebSocketClientBuilder::new("wss://example.com/ws").build();

        assert!(!client.is_connected());
        assert_eq!(client.reconnect_attempts(), 0);
    }
}
