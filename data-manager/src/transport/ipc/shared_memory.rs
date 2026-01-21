//! Shared memory segment management
//!
//! Creates and manages POSIX shared memory segments for IPC.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::debug;

use crate::schema::{CompactTick, NormalizedTick};
use crate::transport::{Transport, TransportError, TransportResult, TransportStats};

use super::ring_buffer::{RingBufferHeader, RingBufferProducer};

/// Shared memory transport for distributing ticks via IPC
pub struct SharedMemoryTransport {
    /// Ring buffers by symbol
    buffers: Arc<RwLock<HashMap<String, SymbolBuffer>>>,
    /// Configuration
    config: SharedMemoryConfig,
    /// Statistics
    stats: Arc<RwLock<TransportStats>>,
}

/// Configuration for shared memory transport
#[derive(Debug, Clone)]
pub struct SharedMemoryConfig {
    /// Shared memory path prefix
    pub path_prefix: String,
    /// Number of entries per ring buffer
    pub buffer_entries: usize,
    /// Entry size in bytes
    pub entry_size: usize,
}

impl Default for SharedMemoryConfig {
    fn default() -> Self {
        Self {
            path_prefix: "/data_manager_".to_string(),
            buffer_entries: 65536, // 64K entries
            entry_size: std::mem::size_of::<CompactTick>(),
        }
    }
}

/// Per-symbol buffer
struct SymbolBuffer {
    /// The memory allocation
    memory: Vec<u8>,
    /// Producer handle
    producer: RingBufferProducer,
}

impl SharedMemoryTransport {
    /// Create a new shared memory transport
    pub fn new(config: SharedMemoryConfig) -> Self {
        Self {
            buffers: Arc::new(RwLock::new(HashMap::new())),
            config,
            stats: Arc::new(RwLock::new(TransportStats::default())),
        }
    }

    /// Get or create a buffer for a symbol
    fn get_or_create_buffer(&self, symbol: &str, exchange: &str) -> TransportResult<()> {
        let key = format!("{}@{}", symbol, exchange);

        let mut buffers = self.buffers.write();
        if buffers.contains_key(&key) {
            return Ok(());
        }

        // Calculate buffer size
        let header_size = RingBufferHeader::SIZE;
        let data_size = self.config.buffer_entries * self.config.entry_size;
        let total_size = header_size + data_size;

        // Allocate memory
        // In production, this would use shared_memory crate:
        // let shmem = ShmemConf::new()
        //     .size(total_size)
        //     .flink(&format!("{}{}", self.config.path_prefix, key))
        //     .create()?;

        // For now, use local memory (works for single-process testing)
        let mut memory = vec![0u8; total_size];

        let header_ptr = memory.as_mut_ptr() as *mut RingBufferHeader;
        let data_ptr = unsafe { memory.as_mut_ptr().add(header_size) };

        // Initialize header
        unsafe {
            (*header_ptr).init(
                self.config.buffer_entries as u64,
                self.config.entry_size as u64,
            );
        }

        // Create producer
        let producer = unsafe { RingBufferProducer::new(header_ptr, data_ptr) };

        buffers.insert(
            key.clone(),
            SymbolBuffer { memory, producer },
        );

        debug!("Created buffer for {}", key);
        Ok(())
    }

    /// Send a tick to the appropriate buffer
    fn send_to_buffer(&self, tick: &NormalizedTick) -> TransportResult<()> {
        let key = tick.full_symbol();

        // Ensure buffer exists
        self.get_or_create_buffer(&tick.symbol, &tick.exchange)?;

        let mut buffers = self.buffers.write();
        let buffer = buffers
            .get_mut(&key)
            .ok_or_else(|| TransportError::ConsumerNotFound(key))?;

        // Convert to compact format
        let compact = CompactTick::from_normalized(tick);

        // Push to ring buffer
        buffer.producer.push(&compact)?;

        // Update stats
        let mut stats = self.stats.write();
        stats.messages_sent += 1;
        stats.bytes_sent += std::mem::size_of::<CompactTick>() as u64;

        Ok(())
    }

    /// Get all active symbol keys
    pub fn active_symbols(&self) -> Vec<String> {
        self.buffers.read().keys().cloned().collect()
    }

    /// Get buffer utilization for a symbol
    pub fn buffer_utilization(&self, symbol: &str, exchange: &str) -> Option<f64> {
        let key = format!("{}@{}", symbol, exchange);
        self.buffers
            .read()
            .get(&key)
            .map(|b| b.producer.utilization())
    }

    /// Remove a buffer
    pub fn remove_buffer(&self, symbol: &str, exchange: &str) {
        let key = format!("{}@{}", symbol, exchange);
        self.buffers.write().remove(&key);
        debug!("Removed buffer for {}", key);
    }
}

impl Transport for SharedMemoryTransport {
    fn send_tick(&self, tick: &NormalizedTick) -> TransportResult<()> {
        self.send_to_buffer(tick)
    }

    fn send_batch(&self, ticks: &[NormalizedTick]) -> TransportResult<usize> {
        let mut count = 0;
        for tick in ticks {
            self.send_to_buffer(tick)?;
            count += 1;
        }
        Ok(count)
    }

    fn is_ready(&self) -> bool {
        true
    }

    fn stats(&self) -> TransportStats {
        let mut stats = self.stats.read().clone();

        // Calculate average buffer utilization
        let buffers = self.buffers.read();
        if !buffers.is_empty() {
            let total_util: f64 = buffers.values().map(|b| b.producer.utilization()).sum();
            stats.buffer_utilization = total_util / buffers.len() as f64;
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::TradeSide;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn create_test_tick(symbol: &str, seq: i64) -> NormalizedTick {
        NormalizedTick::new(
            Utc::now(),
            symbol.to_string(),
            "CME".to_string(),
            dec!(5025.50),
            dec!(10),
            TradeSide::Buy,
            "test".to_string(),
            seq,
        )
    }

    #[test]
    fn test_shared_memory_transport() {
        let transport = SharedMemoryTransport::new(SharedMemoryConfig::default());

        // Send ticks
        for i in 0..100 {
            let tick = create_test_tick("ES", i);
            transport.send_tick(&tick).unwrap();
        }

        // Check stats
        let stats = transport.stats();
        assert_eq!(stats.messages_sent, 100);

        // Check symbols
        let symbols = transport.active_symbols();
        assert_eq!(symbols.len(), 1);
        assert!(symbols.contains(&"ES@CME".to_string()));
    }

    #[test]
    fn test_multiple_symbols() {
        let transport = SharedMemoryTransport::new(SharedMemoryConfig::default());

        transport.send_tick(&create_test_tick("ES", 1)).unwrap();
        transport.send_tick(&create_test_tick("NQ", 1)).unwrap();
        transport.send_tick(&create_test_tick("CL", 1)).unwrap();

        let symbols = transport.active_symbols();
        assert_eq!(symbols.len(), 3);
    }
}
