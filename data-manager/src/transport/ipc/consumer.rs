//! IPC consumer for receiving data from shared memory

use std::time::Duration;

use crate::schema::NormalizedTick;
use crate::transport::{Consumer, TransportError, TransportResult};

use super::ring_buffer::{RingBufferConsumer, RingBufferHeader};

/// Consumer for reading from a shared memory ring buffer
pub struct IpcConsumer {
    /// Ring buffer consumer
    consumer: RingBufferConsumer,
    /// Symbol being consumed
    symbol: String,
    /// Exchange
    exchange: String,
}

impl IpcConsumer {
    /// Create a new IPC consumer
    ///
    /// # Safety
    /// The memory pointers must be valid for the lifetime of the consumer.
    pub unsafe fn new(
        header: *const RingBufferHeader,
        data: *const u8,
        symbol: String,
        exchange: String,
    ) -> Self {
        Self {
            consumer: RingBufferConsumer::new(header, data),
            symbol,
            exchange,
        }
    }

    /// Create from shared memory path
    ///
    /// In production, this would open an existing shared memory segment:
    /// ```ignore
    /// let shmem = ShmemConf::new()
    ///     .flink(&format!("{}{}", path_prefix, key))
    ///     .open()?;
    ///
    /// let header = shmem.as_ptr() as *const RingBufferHeader;
    /// let data = unsafe { shmem.as_ptr().add(RingBufferHeader::SIZE) };
    /// ```
    pub fn open(_path: &str, _symbol: String, _exchange: String) -> TransportResult<Self> {
        // Placeholder - would open shared memory in production
        Err(TransportError::NotInitialized)
    }

    /// Get the full symbol key
    pub fn full_symbol(&self) -> String {
        format!("{}@{}", self.symbol, self.exchange)
    }

    /// Check if overflow occurred (data was lost)
    pub fn had_overflow(&self) -> bool {
        self.consumer.had_overflow()
    }
}

impl Consumer for IpcConsumer {
    fn try_recv(&mut self) -> TransportResult<Option<NormalizedTick>> {
        match self.consumer.try_pop() {
            Some(compact) => Ok(Some(compact.to_normalized())),
            None => Ok(None),
        }
    }

    fn recv(&mut self) -> TransportResult<NormalizedTick> {
        // Spin wait with backoff
        let mut backoff = 1;
        loop {
            if let Some(compact) = self.consumer.try_pop() {
                return Ok(compact.to_normalized());
            }

            // Exponential backoff
            std::thread::sleep(Duration::from_micros(backoff));
            backoff = (backoff * 2).min(1000); // Max 1ms sleep
        }
    }

    fn recv_batch(&mut self, max_count: usize) -> TransportResult<Vec<NormalizedTick>> {
        let compact_ticks = self.consumer.pop_batch(max_count);
        Ok(compact_ticks.iter().map(|c| c.to_normalized()).collect())
    }

    fn has_pending(&self) -> bool {
        self.consumer.has_data()
    }

    fn pending_count(&self) -> usize {
        self.consumer.available() as usize
    }
}

/// Builder for creating IPC consumers
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

    /// Build the consumer
    pub fn build(self) -> TransportResult<IpcConsumer> {
        let symbol = self
            .symbol
            .ok_or_else(|| TransportError::Connection("Symbol not specified".to_string()))?;
        let exchange = self
            .exchange
            .ok_or_else(|| TransportError::Connection("Exchange not specified".to_string()))?;

        let path = format!("{}{}", self.path_prefix, format!("{}@{}", symbol, exchange));
        IpcConsumer::open(&path, symbol, exchange)
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
        let result = builder.build();
        assert!(result.is_err());
    }
}
