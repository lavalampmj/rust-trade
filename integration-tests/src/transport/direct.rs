//! Direct in-memory transport
//!
//! This transport delivers ticks directly to the callback without any
//! network overhead or serialization. It's the fastest option and provides
//! baseline latency measurements.

use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

use data_manager::provider::{ProviderError, ProviderResult, StreamCallback, StreamEvent};

use super::{TickTransport, TransportMode};

/// Direct in-memory transport
///
/// Passes ticks directly to the callback with no serialization or network hop.
/// This is the baseline for measuring "pure" processing latency.
pub struct DirectTransport {
    /// Callback to invoke for each event
    callback: StreamCallback,
    /// Shutdown receiver
    #[allow(dead_code)]
    shutdown_rx: broadcast::Receiver<()>,
    /// Whether the transport is running
    running: AtomicBool,
    /// Count of ticks sent
    sent_count: Arc<AtomicU64>,
}

impl DirectTransport {
    /// Create a new direct transport
    pub fn new(callback: StreamCallback, shutdown_rx: broadcast::Receiver<()>) -> Self {
        Self {
            callback,
            shutdown_rx,
            running: AtomicBool::new(false),
            sent_count: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[async_trait]
impl TickTransport for DirectTransport {
    async fn start(&mut self) -> ProviderResult<()> {
        self.running.store(true, Ordering::SeqCst);
        tracing::debug!("Direct transport started");
        Ok(())
    }

    async fn stop(&mut self) -> ProviderResult<()> {
        self.running.store(false, Ordering::SeqCst);
        tracing::debug!("Direct transport stopped");
        Ok(())
    }

    async fn send(&self, event: StreamEvent) -> ProviderResult<()> {
        if !self.running.load(Ordering::SeqCst) {
            return Err(ProviderError::NotConnected);
        }

        // Track tick count
        if matches!(event, StreamEvent::Tick(_)) {
            self.sent_count.fetch_add(1, Ordering::SeqCst);
        }

        // Direct callback invocation - no serialization, no network
        (self.callback)(event);
        Ok(())
    }

    fn sent_count(&self) -> u64 {
        self.sent_count.load(Ordering::SeqCst)
    }

    fn mode(&self) -> TransportMode {
        TransportMode::Direct
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_manager::provider::ConnectionStatus;
    use std::sync::atomic::AtomicU64;

    #[tokio::test]
    async fn test_direct_transport_start_stop() {
        let received = Arc::new(AtomicU64::new(0));
        let received_clone = received.clone();

        let callback: StreamCallback = Arc::new(move |_event| {
            received_clone.fetch_add(1, Ordering::SeqCst);
        });

        let (_tx, rx) = broadcast::channel(1);
        let mut transport = DirectTransport::new(callback, rx);

        // Should fail before start
        assert!(transport.send(StreamEvent::Status(ConnectionStatus::Connected)).await.is_err());

        // Start and send
        transport.start().await.unwrap();
        transport.send(StreamEvent::Status(ConnectionStatus::Connected)).await.unwrap();
        assert_eq!(received.load(Ordering::SeqCst), 1);

        // Stop
        transport.stop().await.unwrap();
        assert!(transport.send(StreamEvent::Status(ConnectionStatus::Connected)).await.is_err());
    }

    #[tokio::test]
    async fn test_direct_transport_tick_counting() {
        let callback: StreamCallback = Arc::new(|_| {});
        let (_tx, rx) = broadcast::channel(1);
        let mut transport = DirectTransport::new(callback, rx);

        transport.start().await.unwrap();

        // Send multiple status events (not ticks)
        for _ in 0..5 {
            transport.send(StreamEvent::Status(ConnectionStatus::Connected)).await.unwrap();
        }
        assert_eq!(transport.sent_count(), 0);

        // Send ticks
        let tick = data_manager::schema::NormalizedTick::new(
            chrono::Utc::now(),
            "TEST".to_string(),
            "TEST".to_string(),
            rust_decimal::Decimal::new(100, 0),
            rust_decimal::Decimal::new(1, 0),
            data_manager::schema::TradeSide::Buy,
            "test".to_string(),
            0, // sequence
        );

        for _ in 0..10 {
            // Convert NormalizedTick to TickData for StreamEvent
            transport.send(StreamEvent::Tick(tick.clone().into())).await.unwrap();
        }
        assert_eq!(transport.sent_count(), 10);
    }
}
