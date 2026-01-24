//! Transport trait definitions
//!
//! Uses `dbn::TradeMsg` as the canonical format for transport.

use dbn::TradeMsg;
use thiserror::Error;

/// Transport errors
#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Send error: {0}")]
    Send(String),

    #[error("Receive error: {0}")]
    Receive(String),

    #[error("Buffer full")]
    BufferFull,

    #[error("Buffer empty")]
    BufferEmpty,

    #[error("Shared memory error: {0}")]
    SharedMemory(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Transport not initialized")]
    NotInitialized,

    #[error("Consumer not found: {0}")]
    ConsumerNotFound(String),
}

pub type TransportResult<T> = Result<T, TransportError>;

/// Transport trait for data distribution using DBN TradeMsg format
pub trait Transport: Send + Sync {
    /// Send a single TradeMsg to the transport
    ///
    /// Symbol and exchange are needed to route to the correct channel
    fn send_msg(&self, msg: &TradeMsg, symbol: &str, exchange: &str) -> TransportResult<()>;

    /// Send a batch of TradeMsg to the same channel
    fn send_batch(&self, msgs: &[TradeMsg], symbol: &str, exchange: &str)
        -> TransportResult<usize>;

    /// Check if the transport is ready
    fn is_ready(&self) -> bool;

    /// Get transport statistics
    fn stats(&self) -> TransportStats;
}

/// Transport statistics
#[derive(Debug, Clone, Default)]
pub struct TransportStats {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Number of send errors
    pub send_errors: u64,
    /// Number of buffer overflows (for ring buffers)
    pub buffer_overflows: u64,
    /// Current buffer utilization (0.0 - 1.0)
    pub buffer_utilization: f64,
}

/// Consumer handle for receiving TradeMsg data
pub trait Consumer: Send + Sync {
    /// Receive a TradeMsg (non-blocking)
    fn try_recv(&mut self) -> TransportResult<Option<TradeMsg>>;

    /// Receive a TradeMsg (blocking)
    fn recv(&mut self) -> TransportResult<TradeMsg>;

    /// Receive a batch of TradeMsg
    fn recv_batch(&mut self, max_count: usize) -> TransportResult<Vec<TradeMsg>>;

    /// Check if there are pending messages
    fn has_pending(&self) -> bool;

    /// Get number of pending messages
    fn pending_count(&self) -> usize;
}
