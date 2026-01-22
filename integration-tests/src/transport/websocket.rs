//! WebSocket transport layer
//!
//! This module provides a WebSocket-based transport for tick delivery.
//! It adds realistic network latency and serialization overhead for
//! more accurate end-to-end latency measurements.
//!
//! # Serialization
//!
//! Uses MessagePack (`rmp-serde`) for binary serialization instead of JSON
//! to maintain the efficiency benefits of DBN format. MessagePack is ~5x
//! faster than JSON with much smaller payloads and has full serde compatibility.
//!
//! # Architecture
//!
//! ```text
//! Emulator
//!    |
//!    v
//! WebSocketServer (send ticks via broadcast channel)
//!    |
//!    | TCP/WebSocket (binary frames)
//!    v
//! WebSocketClient (receive, deserialize, invoke callback)
//!    |
//!    v
//! StrategyRunner callback
//! ```

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};

use data_manager::provider::{
    ConnectionStatus, ProviderError, ProviderResult, StreamCallback, StreamEvent,
};

use super::{
    StatusPayload, TickPayload, TickTransport, TransportMessage, TransportMode, WebSocketConfig,
};

/// WebSocket server that accepts connections and broadcasts ticks
pub struct WebSocketServer {
    /// Configuration
    config: WebSocketConfig,
    /// Channel to send messages to connected clients
    tx: broadcast::Sender<TransportMessage>,
    /// Whether the server is running
    running: Arc<AtomicBool>,
    /// Server task handle
    server_handle: Option<tokio::task::JoinHandle<()>>,
    /// Count of messages sent
    sent_count: Arc<AtomicU64>,
    /// Count of connected clients
    client_count: Arc<AtomicU64>,
}

impl WebSocketServer {
    /// Create a new WebSocket server
    pub fn new(config: WebSocketConfig) -> Self {
        let (tx, _) = broadcast::channel(200_000); // Large buffer for high-throughput tests

        Self {
            config,
            tx,
            running: Arc::new(AtomicBool::new(false)),
            server_handle: None,
            sent_count: Arc::new(AtomicU64::new(0)),
            client_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start the WebSocket server
    pub async fn start(&mut self) -> ProviderResult<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let addr: SocketAddr = self.config.server_url().parse().map_err(|e| {
            ProviderError::Connection(format!("Invalid server address: {}", e))
        })?;

        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            ProviderError::Connection(format!("Failed to bind WebSocket server: {}", e))
        })?;

        tracing::info!("WebSocket server listening on {}", addr);

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let tx = self.tx.clone();
        let client_count = self.client_count.clone();

        let handle = tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                // Accept with timeout to allow shutdown checks
                let accept_result =
                    timeout(Duration::from_millis(100), listener.accept()).await;

                match accept_result {
                    Ok(Ok((stream, peer_addr))) => {
                        tracing::debug!("WebSocket client connected: {}", peer_addr);
                        let rx = tx.subscribe();
                        let running_clone = running.clone();
                        let client_count_clone = client_count.clone();

                        tokio::spawn(async move {
                            client_count_clone.fetch_add(1, Ordering::SeqCst);
                            if let Err(e) =
                                Self::handle_client(stream, rx, running_clone).await
                            {
                                tracing::debug!("Client {} disconnected: {}", peer_addr, e);
                            }
                            client_count_clone.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Failed to accept connection: {}", e);
                    }
                    Err(_) => {
                        // Timeout - check running flag
                        continue;
                    }
                }
            }
            tracing::info!("WebSocket server stopped");
        });

        self.server_handle = Some(handle);
        Ok(())
    }

    /// Handle a connected client
    async fn handle_client(
        stream: TcpStream,
        mut rx: broadcast::Receiver<TransportMessage>,
        running: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let ws_stream = accept_async(stream)
            .await
            .map_err(|e| format!("WebSocket handshake failed: {}", e))?;

        let (mut write, mut read) = ws_stream.split();

        // Spawn a task to handle incoming messages (for future bidirectional communication)
        let read_running = running.clone();
        tokio::spawn(async move {
            while read_running.load(Ordering::SeqCst) {
                match read.next().await {
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        });

        // Send messages to client
        while running.load(Ordering::SeqCst) {
            match rx.recv().await {
                Ok(msg) => {
                    // Use MessagePack for efficient binary serialization (faster than JSON)
                    let bytes = rmp_serde::to_vec(&msg)
                        .map_err(|e| format!("Serialization error: {}", e))?;

                    write
                        .send(Message::Binary(bytes))
                        .await
                        .map_err(|e| format!("Send error: {}", e))?;

                    if matches!(msg, TransportMessage::Shutdown) {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("Client lagged by {} messages", n);
                }
            }
        }

        Ok(())
    }

    /// Stop the WebSocket server
    pub async fn stop(&mut self) -> ProviderResult<()> {
        self.running.store(false, Ordering::SeqCst);

        // Send shutdown message to all clients
        let _ = self.tx.send(TransportMessage::Shutdown);

        if let Some(handle) = self.server_handle.take() {
            let _ = timeout(Duration::from_secs(5), handle).await;
        }

        Ok(())
    }

    /// Send a message to all connected clients
    pub fn send(&self, msg: TransportMessage) -> ProviderResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(ProviderError::NotConnected);
        }

        if matches!(msg, TransportMessage::Tick(_)) {
            self.sent_count.fetch_add(1, Ordering::SeqCst);
        } else if let TransportMessage::Batch(ref batch) = msg {
            self.sent_count
                .fetch_add(batch.len() as u64, Ordering::SeqCst);
        }

        self.tx.send(msg).map_err(|_| {
            ProviderError::Connection("No clients connected".to_string())
        })?;

        Ok(())
    }

    /// Get the number of ticks sent
    pub fn sent_count(&self) -> u64 {
        self.sent_count.load(Ordering::SeqCst)
    }

    /// Get the number of connected clients
    pub fn client_count(&self) -> u64 {
        self.client_count.load(Ordering::SeqCst)
    }
}

/// WebSocket client that receives ticks and invokes callbacks
pub struct WebSocketClient {
    /// Configuration
    config: WebSocketConfig,
    /// Callback to invoke for each received tick
    callback: StreamCallback,
    /// Whether the client is running
    running: Arc<AtomicBool>,
    /// Client task handle
    client_handle: Option<tokio::task::JoinHandle<()>>,
    /// Shutdown signal
    #[allow(dead_code)]
    shutdown_rx: broadcast::Receiver<()>,
    /// Count of ticks received
    received_count: Arc<AtomicU64>,
}

impl WebSocketClient {
    /// Create a new WebSocket client
    pub fn new(
        config: WebSocketConfig,
        callback: StreamCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            config,
            callback,
            running: Arc::new(AtomicBool::new(false)),
            client_handle: None,
            shutdown_rx,
            received_count: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Connect to the WebSocket server
    pub async fn connect(&mut self) -> ProviderResult<()> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }

        let url = self.config.client_url();
        let connect_timeout = Duration::from_millis(self.config.connect_timeout_ms);

        // Retry connection a few times
        let mut attempts = 0;
        let max_attempts = 10;
        let retry_delay = Duration::from_millis(100);

        let ws_stream = loop {
            match timeout(connect_timeout, connect_async(&url)).await {
                Ok(Ok((stream, _))) => break stream,
                Ok(Err(e)) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(ProviderError::Connection(format!(
                            "Failed to connect to WebSocket server after {} attempts: {}",
                            attempts, e
                        )));
                    }
                    tracing::debug!(
                        "Connection attempt {} failed, retrying: {}",
                        attempts,
                        e
                    );
                    tokio::time::sleep(retry_delay).await;
                }
                Err(_) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(ProviderError::Connection(
                            "Connection timeout".to_string(),
                        ));
                    }
                    tokio::time::sleep(retry_delay).await;
                }
            }
        };

        tracing::info!("WebSocket client connected to {}", url);

        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let callback = self.callback.clone();
        let received_count = self.received_count.clone();

        let handle = tokio::spawn(async move {
            let (_, mut read) = ws_stream.split();

            while running.load(Ordering::SeqCst) {
                match read.next().await {
                    Some(Ok(Message::Binary(bytes))) => {
                        // Use MessagePack for efficient binary deserialization
                        match rmp_serde::from_slice::<TransportMessage>(&bytes) {
                            Ok(msg) => {
                                Self::handle_message(msg, &callback, &received_count);
                            }
                            Err(e) => {
                                tracing::warn!("Failed to deserialize message: {}", e);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::debug!("WebSocket connection closed by server");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::warn!("WebSocket receive error: {}", e);
                        break;
                    }
                    None => break,
                    _ => {}
                }
            }

            running.store(false, Ordering::SeqCst);
            tracing::info!("WebSocket client disconnected");
        });

        self.client_handle = Some(handle);
        Ok(())
    }

    /// Handle a received message
    fn handle_message(
        msg: TransportMessage,
        callback: &StreamCallback,
        received_count: &Arc<AtomicU64>,
    ) {
        match msg {
            TransportMessage::Tick(payload) => {
                received_count.fetch_add(1, Ordering::SeqCst);
                callback(StreamEvent::Tick(payload.tick));
            }
            TransportMessage::Batch(batch) => {
                for payload in batch {
                    received_count.fetch_add(1, Ordering::SeqCst);
                    callback(StreamEvent::Tick(payload.tick));
                }
            }
            TransportMessage::Status(status) => {
                match status {
                    StatusPayload::Connected => {
                        callback(StreamEvent::Status(ConnectionStatus::Connected));
                    }
                    StatusPayload::Disconnected => {
                        callback(StreamEvent::Status(ConnectionStatus::Disconnected));
                    }
                    StatusPayload::Error(e) => {
                        callback(StreamEvent::Error(e));
                    }
                }
            }
            TransportMessage::Shutdown => {
                callback(StreamEvent::Status(ConnectionStatus::Disconnected));
            }
        }
    }

    /// Disconnect from the server
    pub async fn disconnect(&mut self) -> ProviderResult<()> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.client_handle.take() {
            let _ = timeout(Duration::from_secs(5), handle).await;
        }

        Ok(())
    }

    /// Get the number of ticks received
    pub fn received_count(&self) -> u64 {
        self.received_count.load(Ordering::SeqCst)
    }
}

/// Combined WebSocket transport (server + client)
///
/// This provides a unified interface that starts both server and client,
/// handling the full network round-trip for realistic latency measurement.
pub struct WebSocketTransport {
    /// Server component
    server: WebSocketServer,
    /// Client component
    client: WebSocketClient,
    /// Whether the transport is running
    running: AtomicBool,
    /// Batch buffer for batching mode
    batch_buffer: RwLock<Vec<TickPayload>>,
    /// Batching configuration
    batch_size: usize,
    /// Whether batching is enabled
    batching_enabled: bool,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub fn new(
        config: WebSocketConfig,
        callback: StreamCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        let batch_size = config.batch_size;
        let batching_enabled = config.enable_batching;

        Self {
            server: WebSocketServer::new(config.clone()),
            client: WebSocketClient::new(config, callback, shutdown_rx),
            running: AtomicBool::new(false),
            batch_buffer: RwLock::new(Vec::with_capacity(batch_size)),
            batch_size,
            batching_enabled,
        }
    }

    /// Flush the batch buffer
    async fn flush_batch(&self) -> ProviderResult<()> {
        let batch = {
            let mut buffer = self.batch_buffer.write().await;
            if buffer.is_empty() {
                return Ok(());
            }
            std::mem::take(&mut *buffer)
        };

        self.server.send(TransportMessage::Batch(batch))
    }
}

#[async_trait]
impl TickTransport for WebSocketTransport {
    async fn start(&mut self) -> ProviderResult<()> {
        // Start server first
        self.server.start().await?;

        // Small delay to ensure server is ready
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Connect client
        self.client.connect().await?;

        self.running.store(true, Ordering::SeqCst);

        tracing::info!("WebSocket transport started (server + client)");
        Ok(())
    }

    async fn stop(&mut self) -> ProviderResult<()> {
        self.running.store(false, Ordering::SeqCst);

        // Flush any remaining batch
        if self.batching_enabled {
            let _ = self.flush_batch().await;
        }

        // Stop client first, then server
        self.client.disconnect().await?;
        self.server.stop().await?;

        tracing::info!("WebSocket transport stopped");
        Ok(())
    }

    async fn send(&self, event: StreamEvent) -> ProviderResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(ProviderError::NotConnected);
        }

        match event {
            StreamEvent::Tick(tick) => {
                let payload = TickPayload {
                    tick,
                    ts_in_delta: 0, // TODO: could embed here if needed
                };

                if self.batching_enabled {
                    let mut buffer = self.batch_buffer.write().await;
                    buffer.push(payload);

                    if buffer.len() >= self.batch_size {
                        let batch = std::mem::take(&mut *buffer);
                        drop(buffer); // Release lock before sending
                        self.server.send(TransportMessage::Batch(batch))?;
                    }
                } else {
                    self.server.send(TransportMessage::Tick(payload))?;
                }
            }
            StreamEvent::Status(status) => {
                self.server
                    .send(TransportMessage::Status(status.into()))?;
            }
            StreamEvent::Error(e) => {
                self.server
                    .send(TransportMessage::Status(StatusPayload::Error(e)))?;
            }
            StreamEvent::TickBatch(ticks) => {
                // Convert batch to payloads
                let batch: Vec<TickPayload> = ticks
                    .into_iter()
                    .map(|tick| TickPayload {
                        tick,
                        ts_in_delta: 0,
                    })
                    .collect();
                self.server.send(TransportMessage::Batch(batch))?;
            }
        }

        Ok(())
    }

    fn sent_count(&self) -> u64 {
        self.server.sent_count()
    }

    fn mode(&self) -> TransportMode {
        TransportMode::WebSocket
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_manager::schema::{NormalizedTick, TradeSide};
    use rust_decimal::Decimal;
    use std::sync::atomic::AtomicU64;

    fn create_test_tick() -> NormalizedTick {
        NormalizedTick::new(
            chrono::Utc::now(),
            "TEST0000".to_string(),
            "TEST".to_string(),
            Decimal::new(50000, 0),
            Decimal::new(1, 0),
            TradeSide::Buy,
            "test".to_string(),
            0, // sequence
        )
    }

    #[tokio::test]
    async fn test_websocket_server_start_stop() {
        let config = WebSocketConfig {
            port: 19990,
            ..Default::default()
        };

        let mut server = WebSocketServer::new(config);
        server.start().await.unwrap();
        assert!(server.running.load(Ordering::SeqCst));

        server.stop().await.unwrap();
        assert!(!server.running.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_websocket_transport_full_roundtrip() {
        let received = Arc::new(AtomicU64::new(0));
        let received_clone = received.clone();

        let callback: StreamCallback = Arc::new(move |event| {
            if matches!(event, StreamEvent::Tick(_)) {
                received_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        let (_tx, rx) = broadcast::channel(1);
        let config = WebSocketConfig {
            port: 19991,
            ..Default::default()
        };

        let mut transport = WebSocketTransport::new(config, callback, rx);

        // Start transport
        transport.start().await.unwrap();

        // Small delay for connection establishment
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send ticks
        let tick = create_test_tick();
        for _ in 0..10 {
            transport.send(StreamEvent::Tick(tick.clone())).await.unwrap();
        }

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Verify received
        assert_eq!(received.load(Ordering::SeqCst), 10);
        assert_eq!(transport.sent_count(), 10);

        // Stop transport
        transport.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_websocket_transport_batching() {
        let received = Arc::new(AtomicU64::new(0));
        let received_clone = received.clone();

        let callback: StreamCallback = Arc::new(move |event| {
            if matches!(event, StreamEvent::Tick(_)) {
                received_clone.fetch_add(1, Ordering::SeqCst);
            }
        });

        let (_tx, rx) = broadcast::channel(1);
        let config = WebSocketConfig {
            port: 19992,
            enable_batching: true,
            batch_size: 5,
            ..Default::default()
        };

        let mut transport = WebSocketTransport::new(config, callback, rx);
        transport.start().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Send 12 ticks (should trigger 2 batches of 5, with 2 remaining)
        let tick = create_test_tick();
        for _ in 0..12 {
            transport.send(StreamEvent::Tick(tick.clone())).await.unwrap();
        }

        tokio::time::sleep(Duration::from_millis(200)).await;

        // 10 should be delivered (2 batches), 2 remain buffered
        assert_eq!(received.load(Ordering::SeqCst), 10);

        // Stop will flush remaining
        transport.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // All 12 should now be received
        assert_eq!(received.load(Ordering::SeqCst), 12);
    }

    #[test]
    fn test_transport_message_serialization() {
        let tick = create_test_tick();
        let payload = TickPayload {
            tick,
            ts_in_delta: 12345,
        };

        let msg = TransportMessage::Tick(payload);
        // Use MessagePack for binary serialization (faster than JSON)
        let bytes = rmp_serde::to_vec(&msg).unwrap();
        let deserialized: TransportMessage = rmp_serde::from_slice(&bytes).unwrap();

        match deserialized {
            TransportMessage::Tick(p) => {
                assert_eq!(p.ts_in_delta, 12345);
                assert_eq!(p.tick.symbol, "TEST0000");
            }
            _ => panic!("Wrong message type"),
        }
    }
}
