//! IPC consumer - re-exported from shared_memory module
//!
//! This module is kept for backwards compatibility.
//! Use `SharedMemoryChannel::consumer()` or `IpcConsumer` from the parent module.

// Re-export from shared_memory for backwards compatibility
pub use super::shared_memory::IpcConsumer;

/// Builder for creating IPC consumers (convenience wrapper)
pub struct IpcConsumerBuilder {
    path_prefix: String,
    symbol: Option<String>,
    exchange: Option<String>,
}

impl IpcConsumerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            path_prefix: "/data_manager_".to_string(),
            symbol: None,
            exchange: None,
        }
    }

    /// Set the shared memory path prefix
    pub fn path_prefix(mut self, prefix: &str) -> Self {
        self.path_prefix = prefix.to_string();
        self
    }

    /// Set the symbol to consume
    pub fn symbol(mut self, symbol: &str) -> Self {
        self.symbol = Some(symbol.to_string());
        self
    }

    /// Set the exchange
    pub fn exchange(mut self, exchange: &str) -> Self {
        self.exchange = Some(exchange.to_string());
        self
    }

    /// Build the consumer by opening existing shared memory
    pub fn build(self) -> crate::transport::TransportResult<IpcConsumer> {
        use super::shared_memory::SharedMemoryChannel;

        let symbol = self
            .symbol
            .ok_or_else(|| crate::transport::TransportError::Connection("Symbol not specified".to_string()))?;
        let exchange = self
            .exchange
            .ok_or_else(|| crate::transport::TransportError::Connection("Exchange not specified".to_string()))?;

        let channel = SharedMemoryChannel::open(&symbol, &exchange, &self.path_prefix)?;
        Ok(channel.consumer())
    }
}

impl Default for IpcConsumerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_builder() {
        let builder = IpcConsumerBuilder::new()
            .path_prefix("/test_")
            .symbol("ES")
            .exchange("CME");

        // Build will fail since we can't actually open shared memory in tests
        // (no producer has created it)
        let result = builder.build();
        assert!(result.is_err());
    }
}
