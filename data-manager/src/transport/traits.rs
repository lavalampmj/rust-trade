//! Transport trait definitions

use thiserror::Error;

use crate::schema::NormalizedTick;

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

/// Transport trait for data distribution
pub trait Transport: Send + Sync {
    /// Send a single tick to the transport
    fn send_tick(&self, tick: &NormalizedTick) -> TransportResult<()>;

    /// Send a batch of ticks
    fn send_batch(&self, ticks: &[NormalizedTick]) -> TransportResult<usize>;

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

/// Consumer handle for receiving data
pub trait Consumer: Send + Sync {
    /// Receive a tick (non-blocking)
    fn try_recv(&mut self) -> TransportResult<Option<NormalizedTick>>;

    /// Receive a tick (blocking)
    fn recv(&mut self) -> TransportResult<NormalizedTick>;

    /// Receive a batch of ticks
    fn recv_batch(&mut self, max_count: usize) -> TransportResult<Vec<NormalizedTick>>;

    /// Check if there are pending messages
    fn has_pending(&self) -> bool;

    /// Get number of pending messages
    fn pending_count(&self) -> usize;
}
