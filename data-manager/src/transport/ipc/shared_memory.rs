//! Shared memory segment management
//!
//! Creates and manages POSIX shared memory segments for IPC.
//! Supports both in-process testing and real cross-process communication.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use shared_memory::{Shmem, ShmemConf, ShmemError};
use tracing::debug;

use crate::schema::{CompactTick, NormalizedTick};
use crate::transport::{Transport, TransportError, TransportResult, TransportStats};

use super::ring_buffer::{RingBufferConsumer, RingBufferHeader, RingBufferProducer};

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

/// Shared memory channel for IPC communication
///
/// This is the main entry point for creating producer/consumer pairs.
/// It manages the shared memory segment and provides access to the ring buffer.
pub struct SharedMemoryChannel {
    /// The shared memory segment
    shmem: Shmem,
    /// Symbol
    symbol: String,
    /// Exchange
    exchange: String,
    /// Whether we own (created) the shared memory
    is_owner: bool,
    /// Configuration
    config: SharedMemoryConfig,
}

// Safety: SharedMemoryChannel uses atomic operations in the ring buffer
// and the Shmem type itself handles synchronization
unsafe impl Send for SharedMemoryChannel {}
unsafe impl Sync for SharedMemoryChannel {}

impl SharedMemoryChannel {
    /// Create a new shared memory channel with auto-generated name
    pub fn create(symbol: &str, exchange: &str, config: SharedMemoryConfig) -> TransportResult<Self> {
        let shm_name = format!("{}{}_{}", config.path_prefix, symbol, exchange);
        Self::create_named(&shm_name, symbol, exchange, config)
    }

    /// Create a new shared memory channel with explicit name
    pub fn create_named(
        shm_name: &str,
        symbol: &str,
        exchange: &str,
        config: SharedMemoryConfig,
    ) -> TransportResult<Self> {
        let header_size = RingBufferHeader::SIZE;
        let data_size = config.buffer_entries * config.entry_size;
        let total_size = header_size + data_size;

        // Try to unlink any existing segment first
        let _ = Self::unlink(shm_name);

        // Create new shared memory segment using OS-native identifier
        // This avoids file permission issues in containers/WSL
        let shmem = ShmemConf::new()
            .size(total_size)
            .os_id(shm_name)
            .create()
            .map_err(|e| TransportError::Connection(format!("Failed to create shmem: {}", e)))?;

        // Initialize the ring buffer header
        let header_ptr = shmem.as_ptr() as *mut RingBufferHeader;
        unsafe {
            (*header_ptr).init(config.buffer_entries as u64, config.entry_size as u64);
        }

        debug!(
            "Created shared memory channel {} for {}@{} ({} bytes)",
            shm_name, symbol, exchange, total_size
        );

        Ok(Self {
            shmem,
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            is_owner: true,
            config,
        })
    }

    /// Open an existing shared memory channel by name
    pub fn open_named(shm_name: &str, symbol: &str, exchange: &str) -> TransportResult<Self> {
        let shmem = ShmemConf::new()
            .os_id(shm_name)
            .open()
            .map_err(|e| TransportError::Connection(format!("Failed to open shmem: {}", e)))?;

        // Read config from header
        let header_ptr = shmem.as_ptr() as *const RingBufferHeader;
        let (buffer_entries, entry_size) = unsafe {
            let header = &*header_ptr;
            (header.capacity as usize, header.entry_size as usize)
        };

        debug!(
            "Opened shared memory channel {} for {}@{}",
            shm_name, symbol, exchange
        );

        Ok(Self {
            shmem,
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            is_owner: false,
            config: SharedMemoryConfig {
                path_prefix: String::new(),
                buffer_entries,
                entry_size,
            },
        })
    }

    /// Open an existing shared memory channel with auto-generated name
    pub fn open(
        symbol: &str,
        exchange: &str,
        path_prefix: &str,
    ) -> TransportResult<Self> {
        let shm_name = format!("{}{}_{}", path_prefix, symbol, exchange);
        Self::open_named(&shm_name, symbol, exchange)
    }

    /// Get a producer for this channel
    pub fn producer(&self) -> IpcProducer {
        let header_ptr = self.shmem.as_ptr() as *mut RingBufferHeader;
        let data_ptr = unsafe { self.shmem.as_ptr().add(RingBufferHeader::SIZE) as *mut u8 };

        IpcProducer {
            producer: unsafe { RingBufferProducer::new(header_ptr, data_ptr) },
            symbol: self.symbol.clone(),
            exchange: self.exchange.clone(),
        }
    }

    /// Get a consumer for this channel
    pub fn consumer(&self) -> IpcConsumer {
        let header_ptr = self.shmem.as_ptr() as *const RingBufferHeader;
        let data_ptr = unsafe { self.shmem.as_ptr().add(RingBufferHeader::SIZE) };

        IpcConsumer {
            consumer: unsafe { RingBufferConsumer::new(header_ptr, data_ptr) },
            symbol: self.symbol.clone(),
            exchange: self.exchange.clone(),
        }
    }

    /// Get the full symbol key
    pub fn full_symbol(&self) -> String {
        format!("{}@{}", self.symbol, self.exchange)
    }

    /// Get buffer utilization
    pub fn utilization(&self) -> f64 {
        let header_ptr = self.shmem.as_ptr() as *const RingBufferHeader;
        unsafe { (*header_ptr).utilization() }
    }

    /// Unlink (remove) a shared memory segment
    pub fn unlink(shm_name: &str) -> TransportResult<()> {
        // The shared_memory crate doesn't expose unlink directly,
        // so we use the raw syscall on Unix
        #[cfg(unix)]
        {
            use std::ffi::CString;
            let c_name = CString::new(shm_name)
                .map_err(|e| TransportError::Connection(format!("Invalid shm name: {}", e)))?;
            unsafe {
                libc::shm_unlink(c_name.as_ptr());
            }
        }
        Ok(())
    }
}

impl Drop for SharedMemoryChannel {
    fn drop(&mut self) {
        if self.is_owner {
            debug!("Dropping shared memory channel for {}@{}", self.symbol, self.exchange);
            // The Shmem will be dropped automatically
            // On owner drop, we could unlink but that might affect other consumers
        }
    }
}

/// IPC Producer for sending ticks to shared memory
pub struct IpcProducer {
    producer: RingBufferProducer,
    symbol: String,
    exchange: String,
}

// Safety: IpcProducer uses atomic operations internally
unsafe impl Send for IpcProducer {}
unsafe impl Sync for IpcProducer {}

impl IpcProducer {
    /// Send a tick to the ring buffer
    pub fn send(&mut self, tick: &NormalizedTick) -> TransportResult<()> {
        let compact = CompactTick::from_normalized(tick);
        self.producer.push(&compact)
    }

    /// Send a batch of ticks
    pub fn send_batch(&mut self, ticks: &[NormalizedTick]) -> TransportResult<usize> {
        let mut count = 0;
        for tick in ticks {
            self.send(tick)?;
            count += 1;
        }
        Ok(count)
    }

    /// Get buffer utilization
    pub fn utilization(&self) -> f64 {
        self.producer.utilization()
    }

    /// Get number of items in buffer
    pub fn len(&self) -> usize {
        self.producer.len() as usize
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.producer.is_empty()
    }
}

/// IPC Consumer for receiving ticks from shared memory
pub struct IpcConsumer {
    consumer: RingBufferConsumer,
    symbol: String,
    exchange: String,
}

// Safety: IpcConsumer uses atomic operations internally
unsafe impl Send for IpcConsumer {}
unsafe impl Sync for IpcConsumer {}

impl IpcConsumer {
    /// Try to receive a tick (non-blocking)
    pub fn try_recv(&mut self) -> TransportResult<Option<NormalizedTick>> {
        match self.consumer.try_pop() {
            Some(compact) => Ok(Some(compact.to_normalized())),
            None => Ok(None),
        }
    }

    /// Receive a tick (blocking with spin-wait)
    pub fn recv(&mut self) -> TransportResult<NormalizedTick> {
        let mut backoff = 1u64;
        loop {
            if let Some(compact) = self.consumer.try_pop() {
                return Ok(compact.to_normalized());
            }

            // Exponential backoff
            std::thread::sleep(std::time::Duration::from_micros(backoff));
            backoff = (backoff * 2).min(1000); // Max 1ms sleep
        }
    }

    /// Receive a batch of ticks (non-blocking)
    pub fn recv_batch(&mut self, max_count: usize) -> TransportResult<Vec<NormalizedTick>> {
        let compact_ticks = self.consumer.pop_batch(max_count);
        Ok(compact_ticks.iter().map(|c| c.to_normalized()).collect())
    }

    /// Check if data is available
    pub fn has_data(&self) -> bool {
        self.consumer.has_data()
    }

    /// Get number of available ticks
    pub fn available(&self) -> usize {
        self.consumer.available() as usize
    }

    /// Check if overflow occurred
    pub fn had_overflow(&self) -> bool {
        self.consumer.had_overflow()
    }

    /// Get the symbol
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Get the exchange
    pub fn exchange(&self) -> &str {
        &self.exchange
    }
}

/// Shared memory transport for distributing ticks via IPC (multi-symbol)
pub struct SharedMemoryTransport {
    /// Channels by symbol
    channels: Arc<RwLock<HashMap<String, SharedMemoryChannel>>>,
    /// Configuration
    config: SharedMemoryConfig,
    /// Statistics
    stats: Arc<RwLock<TransportStats>>,
}

impl SharedMemoryTransport {
    /// Create a new shared memory transport
    pub fn new(config: SharedMemoryConfig) -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            config,
            stats: Arc::new(RwLock::new(TransportStats::default())),
        }
    }

    /// Get or create a channel for a symbol
    fn get_or_create_channel(&self, symbol: &str, exchange: &str) -> TransportResult<()> {
        let key = format!("{}@{}", symbol, exchange);

        let mut channels = self.channels.write();
        if channels.contains_key(&key) {
            return Ok(());
        }

        let channel = SharedMemoryChannel::create(symbol, exchange, self.config.clone())?;
        channels.insert(key.clone(), channel);

        debug!("Created channel for {}", key);
        Ok(())
    }

    /// Send a tick to the appropriate channel
    fn send_to_channel(&self, tick: &NormalizedTick) -> TransportResult<()> {
        let key = tick.full_symbol();

        // Ensure channel exists
        self.get_or_create_channel(&tick.symbol, &tick.exchange)?;

        let channels = self.channels.read();
        let channel = channels
            .get(&key)
            .ok_or_else(|| TransportError::ConsumerNotFound(key.clone()))?;

        // Send via producer
        let mut producer = channel.producer();
        producer.send(tick)?;

        // Update stats
        let mut stats = self.stats.write();
        stats.messages_sent += 1;
        stats.bytes_sent += std::mem::size_of::<CompactTick>() as u64;

        Ok(())
    }

    /// Get all active symbol keys
    pub fn active_symbols(&self) -> Vec<String> {
        self.channels.read().keys().cloned().collect()
    }

    /// Get buffer utilization for a symbol
    pub fn buffer_utilization(&self, symbol: &str, exchange: &str) -> Option<f64> {
        let key = format!("{}@{}", symbol, exchange);
        self.channels.read().get(&key).map(|c| c.utilization())
    }

    /// Remove a channel
    pub fn remove_channel(&self, symbol: &str, exchange: &str) {
        let key = format!("{}@{}", symbol, exchange);
        self.channels.write().remove(&key);
        debug!("Removed channel for {}", key);
    }

    /// Get a consumer for a symbol
    pub fn consumer(&self, symbol: &str, exchange: &str) -> Option<IpcConsumer> {
        let key = format!("{}@{}", symbol, exchange);
        self.channels.read().get(&key).map(|c| c.consumer())
    }
}

impl Transport for SharedMemoryTransport {
    fn send_tick(&self, tick: &NormalizedTick) -> TransportResult<()> {
        self.send_to_channel(tick)
    }

    fn send_batch(&self, ticks: &[NormalizedTick]) -> TransportResult<usize> {
        let mut count = 0;
        for tick in ticks {
            self.send_to_channel(tick)?;
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
        let channels = self.channels.read();
        if !channels.is_empty() {
            let total_util: f64 = channels.values().map(|c| c.utilization()).sum();
            stats.buffer_utilization = total_util / channels.len() as f64;
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
    fn test_shared_memory_channel_basic() {
        let config = SharedMemoryConfig {
            path_prefix: "/test_channel_basic_".to_string(),
            buffer_entries: 1024,
            entry_size: std::mem::size_of::<CompactTick>(),
        };

        let channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
        let mut producer = channel.producer();
        let mut consumer = channel.consumer();

        // Send and receive
        let tick = create_test_tick("ES", 1);
        producer.send(&tick).unwrap();

        let received = consumer.try_recv().unwrap().unwrap();
        assert_eq!(received.sequence, 1);
        assert_eq!(received.symbol, "ES");
    }

    #[test]
    fn test_shared_memory_transport() {
        let transport = SharedMemoryTransport::new(SharedMemoryConfig {
            path_prefix: "/test_transport_".to_string(),
            ..Default::default()
        });

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
        let transport = SharedMemoryTransport::new(SharedMemoryConfig {
            path_prefix: "/test_multi_sym_".to_string(),
            ..Default::default()
        });

        transport.send_tick(&create_test_tick("ES", 1)).unwrap();
        transport.send_tick(&create_test_tick("NQ", 1)).unwrap();
        transport.send_tick(&create_test_tick("CL", 1)).unwrap();

        let symbols = transport.active_symbols();
        assert_eq!(symbols.len(), 3);
    }
}
