//! gRPC transport (placeholder for future implementation)
//!
//! This module will provide gRPC-based data distribution for
//! cross-machine communication.
//!
//! Uses `dbn::TradeMsg` as the canonical message format.

use dbn::TradeMsg;

use crate::transport::{Transport, TransportResult, TransportStats};

/// Placeholder gRPC transport
#[allow(dead_code)] // Placeholder for future implementation
pub struct GrpcTransport {
    /// Bind address
    bind_address: String,
    /// Whether the service is running
    running: bool,
}

impl GrpcTransport {
    /// Create a new gRPC transport
    pub fn new(bind_address: String) -> Self {
        Self {
            bind_address,
            running: false,
        }
    }

    /// Start the gRPC server
    pub async fn start(&mut self) -> TransportResult<()> {
        // Placeholder - would start tonic server
        self.running = true;
        Ok(())
    }

    /// Stop the gRPC server
    pub async fn stop(&mut self) -> TransportResult<()> {
        self.running = false;
        Ok(())
    }
}

impl Transport for GrpcTransport {
    fn send_msg(&self, _msg: &TradeMsg, _symbol: &str, _exchange: &str) -> TransportResult<()> {
        // Placeholder
        Ok(())
    }

    fn send_batch(&self, _msgs: &[TradeMsg], _symbol: &str, _exchange: &str) -> TransportResult<usize> {
        // Placeholder
        Ok(0)
    }

    fn is_ready(&self) -> bool {
        self.running
    }

    fn stats(&self) -> TransportStats {
        TransportStats::default()
    }
}
