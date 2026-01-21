//! Transport layer for tick delivery
//!
//! This module provides configurable transport mechanisms for delivering ticks
//! from the emulator to strategy runners. Two modes are supported:
//!
//! - **Direct**: In-memory callback (lowest latency, no serialization)
//! - **WebSocket**: Network transport (realistic latency, serialization overhead)
//!
//! # Architecture
//!
//! ```text
//! Direct Mode:
//!   Emulator → callback(StreamEvent::Tick) → StrategyRunner
//!
//! WebSocket Mode:
//!   Emulator → WsServer → serialize → network → WsClient → deserialize → callback
//! ```

mod direct;
mod websocket;

pub use direct::DirectTransport;
pub use websocket::{WebSocketClient, WebSocketServer, WebSocketTransport};

use async_trait::async_trait;
use data_manager::provider::{ProviderResult, StreamCallback, StreamEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Transport mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TransportMode {
    /// Direct in-memory callback (lowest latency, for baseline measurements)
    Direct,
    /// WebSocket transport (default - realistic network latency and serialization)
    #[default]
    WebSocket,
}

/// WebSocket transport configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// Host to bind/connect to
    #[serde(default = "default_ws_host")]
    pub host: String,
    /// Port to bind/connect to
    #[serde(default = "default_ws_port")]
    pub port: u16,
    /// Connection timeout in milliseconds
    #[serde(default = "default_ws_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// Whether to enable message batching
    #[serde(default)]
    pub enable_batching: bool,
    /// Batch size (messages before flush)
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
    /// Batch timeout in milliseconds
    #[serde(default = "default_batch_timeout_ms")]
    pub batch_timeout_ms: u64,
}

fn default_ws_host() -> String {
    "127.0.0.1".to_string()
}

fn default_ws_port() -> u16 {
    9999
}

fn default_ws_connect_timeout_ms() -> u64 {
    5000
}

fn default_batch_size() -> usize {
    100
}

fn default_batch_timeout_ms() -> u64 {
    10
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            host: default_ws_host(),
            port: default_ws_port(),
            connect_timeout_ms: default_ws_connect_timeout_ms(),
            enable_batching: false,
            batch_size: default_batch_size(),
            batch_timeout_ms: default_batch_timeout_ms(),
        }
    }
}

impl WebSocketConfig {
    /// Get the WebSocket URL for the server
    pub fn server_url(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Get the WebSocket URL for client connection
    pub fn client_url(&self) -> String {
        format!("ws://{}:{}/ws", self.host, self.port)
    }
}

/// Complete transport configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransportConfig {
    /// Transport mode to use
    #[serde(default)]
    pub mode: TransportMode,
    /// WebSocket-specific configuration (used when mode is WebSocket)
    #[serde(default)]
    pub websocket: WebSocketConfig,
}

/// Trait for tick transport mechanisms
#[async_trait]
pub trait TickTransport: Send + Sync {
    /// Start the transport layer (server side)
    async fn start(&mut self) -> ProviderResult<()>;

    /// Stop the transport layer
    async fn stop(&mut self) -> ProviderResult<()>;

    /// Send a tick through the transport
    async fn send(&self, event: StreamEvent) -> ProviderResult<()>;

    /// Get the number of ticks sent
    fn sent_count(&self) -> u64;

    /// Get the transport mode
    fn mode(&self) -> TransportMode;
}

/// Create a transport instance based on configuration
pub fn create_transport(
    config: &TransportConfig,
    callback: StreamCallback,
    shutdown_rx: broadcast::Receiver<()>,
) -> Box<dyn TickTransport> {
    match config.mode {
        TransportMode::Direct => {
            Box::new(DirectTransport::new(callback, shutdown_rx))
        }
        TransportMode::WebSocket => {
            Box::new(WebSocketTransport::new(
                config.websocket.clone(),
                callback,
                shutdown_rx,
            ))
        }
    }
}

/// Serializable message format for WebSocket transport
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportMessage {
    /// Single tick event
    Tick(TickPayload),
    /// Status update
    Status(StatusPayload),
    /// Batch of ticks
    Batch(Vec<TickPayload>),
    /// Shutdown signal
    Shutdown,
}

/// Tick payload for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickPayload {
    /// The normalized tick data
    pub tick: data_manager::schema::NormalizedTick,
    /// Original ts_in_delta for latency tracking (microseconds embedded)
    pub ts_in_delta: i32,
}

/// Status payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StatusPayload {
    Connected,
    Disconnected,
    Error(String),
}

impl From<data_manager::provider::ConnectionStatus> for StatusPayload {
    fn from(status: data_manager::provider::ConnectionStatus) -> Self {
        match status {
            data_manager::provider::ConnectionStatus::Connected => StatusPayload::Connected,
            data_manager::provider::ConnectionStatus::Disconnected => StatusPayload::Disconnected,
            data_manager::provider::ConnectionStatus::Reconnecting => {
                StatusPayload::Error("Reconnecting".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_mode_default() {
        let mode = TransportMode::default();
        assert_eq!(mode, TransportMode::WebSocket);
    }

    #[test]
    fn test_websocket_config_default() {
        let config = WebSocketConfig::default();
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 9999);
        assert_eq!(config.client_url(), "ws://127.0.0.1:9999/ws");
    }

    #[test]
    fn test_transport_config_serialization() {
        let config = TransportConfig {
            mode: TransportMode::WebSocket,
            websocket: WebSocketConfig {
                port: 8080,
                ..Default::default()
            },
        };
        let serialized = toml::to_string(&config).unwrap();
        assert!(serialized.contains("mode = \"WebSocket\""));
        assert!(serialized.contains("port = 8080"));
    }
}
