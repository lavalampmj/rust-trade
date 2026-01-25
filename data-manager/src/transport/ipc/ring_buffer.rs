//! Lock-free SPSC ring buffer for IPC.
//!
//! Implements a single-producer single-consumer ring buffer using
//! atomic operations for lock-free access across process boundaries.
//!
//! Uses `dbn::TradeMsg` (48 bytes) as the canonical entry format.
//! This is more compact than the previous TradeMsg (128 bytes)
//! and uses the industry-standard DBN format.
//!
//! # Memory Layout
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                    Shared Memory Region                     │
//! ├────────────────────────────────────────────────────────────┤
//! │ Header (64 bytes, cache-line aligned)                      │
//! │  ├─ write_pos: AtomicU64  (producer writes, consumer reads)│
//! │  ├─ read_pos:  AtomicU64  (consumer writes, producer reads)│
//! │  ├─ capacity:  u64        (immutable after init)           │
//! │  ├─ entry_size: u64       (48 bytes for TradeMsg)          │
//! │  ├─ flags:     AtomicU64  (active bit, overflow bit)       │
//! │  └─ padding:   [u8; 24]   (fills to 64 bytes)              │
//! ├────────────────────────────────────────────────────────────┤
//! │ Data Area (capacity * entry_size bytes)                    │
//! │  ├─ Entry[0]: TradeMsg (48 bytes)                          │
//! │  ├─ Entry[1]: TradeMsg (48 bytes)                          │
//! │  ├─ ...                                                    │
//! │  └─ Entry[capacity-1]: TradeMsg (48 bytes)                 │
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Memory Ordering
//!
//! The ring buffer uses carefully chosen memory orderings for correctness:
//!
//! ## Producer (push)
//! 1. Load `write_pos` with `Relaxed` - only producer modifies this
//! 2. Load `read_pos` with `Acquire` - synchronize with consumer's Release store
//! 3. Write data to buffer (non-atomic, safe due to SPSC guarantee)
//! 4. `Release` fence - ensure data write completes before position update
//! 5. Store `write_pos` with `Release` - make data visible to consumer
//!
//! ## Consumer (pop)
//! 1. Load `read_pos` with `Relaxed` - only consumer modifies this
//! 2. Load `write_pos` with `Acquire` - synchronize with producer's Release store
//! 3. Read data from buffer (non-atomic, safe due to SPSC guarantee)
//! 4. Store `read_pos` with `Release` - allow producer to reclaim slot
//!
//! ## Why these orderings work:
//! - `Acquire` on loads ensures we see all writes that happened before the
//!   corresponding `Release` store
//! - The fence before `write_pos` update ensures the data is fully written
//!   before the consumer sees the new position
//! - `wrapping_sub` handles position wraparound correctly (positions are
//!   monotonically increasing, wrap at u64::MAX)
//!
//! # Overflow Handling
//!
//! When the buffer is full and a new entry arrives:
//! 1. Mark overflow flag (for monitoring)
//! 2. Advance `read_pos` to drop oldest entry
//! 3. Write new entry at the freed slot
//!
//! This prioritizes real-time data freshness over completeness - newer
//! market data is more valuable than historical data in live trading.
//!
//! # Performance
//!
//! - Typical latency: ~10µs per entry
//! - Cache-line aligned header reduces false sharing
//! - No locks, no syscalls in hot path
//! - Batch operations amortize overhead

use std::sync::atomic::{AtomicU64, Ordering};

use dbn::TradeMsg;

use crate::transport::TransportResult;

/// Ring buffer header in shared memory
///
/// Must be at the start of the shared memory segment.
/// Using repr(C) for stable memory layout.
#[repr(C)]
#[derive(Debug)]
pub struct RingBufferHeader {
    /// Write position (producer)
    pub write_pos: AtomicU64,
    /// Read position (consumer)
    pub read_pos: AtomicU64,
    /// Buffer capacity (number of entries)
    pub capacity: u64,
    /// Size of each entry in bytes
    pub entry_size: u64,
    /// Flags (bit 0: active, bit 1: overflow occurred)
    pub flags: AtomicU64,
    /// Padding to cache line (64 bytes total)
    _padding: [u8; 24],
}

impl RingBufferHeader {
    /// Header size in bytes
    pub const SIZE: usize = 64;

    /// Flag: buffer is active
    pub const FLAG_ACTIVE: u64 = 0x01;
    /// Flag: overflow occurred
    pub const FLAG_OVERFLOW: u64 = 0x02;

    /// Initialize a new header
    pub fn init(&mut self, capacity: u64, entry_size: u64) {
        self.write_pos.store(0, Ordering::Release);
        self.read_pos.store(0, Ordering::Release);
        self.capacity = capacity;
        self.entry_size = entry_size;
        self.flags.store(Self::FLAG_ACTIVE, Ordering::Release);
    }

    /// Check if buffer is active
    pub fn is_active(&self) -> bool {
        self.flags.load(Ordering::Acquire) & Self::FLAG_ACTIVE != 0
    }

    /// Mark overflow occurred
    pub fn mark_overflow(&self) {
        self.flags.fetch_or(Self::FLAG_OVERFLOW, Ordering::Release);
    }

    /// Check if overflow occurred
    pub fn had_overflow(&self) -> bool {
        self.flags.load(Ordering::Acquire) & Self::FLAG_OVERFLOW != 0
    }

    /// Get current fill level
    pub fn fill_level(&self) -> u64 {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }

    /// Get buffer utilization (0.0 - 1.0)
    pub fn utilization(&self) -> f64 {
        self.fill_level() as f64 / self.capacity as f64
    }
}

/// SPSC Ring buffer producer
pub struct RingBufferProducer {
    /// Pointer to header in shared memory
    header: *mut RingBufferHeader,
    /// Pointer to data area
    data: *mut u8,
    /// Cached write position for batch operations (reserved for future optimization)
    #[allow(dead_code)]
    cached_write_pos: u64,
}

// Safety: RingBufferProducer is Send+Sync because it owns exclusive write access
// to the producer side of the ring buffer. The raw pointers point to shared memory
// that is thread-safe through atomic operations.
unsafe impl Send for RingBufferProducer {}
unsafe impl Sync for RingBufferProducer {}

impl RingBufferProducer {
    /// Create from shared memory pointers
    ///
    /// # Safety
    /// - `header` must point to valid, properly aligned RingBufferHeader
    /// - `data` must point to valid memory of size capacity * entry_size
    /// - Only one producer should exist per buffer
    pub unsafe fn new(header: *mut RingBufferHeader, data: *mut u8) -> Self {
        Self {
            header,
            data,
            cached_write_pos: (*header).write_pos.load(Ordering::Acquire),
        }
    }

    /// Push a tick to the buffer
    ///
    /// Returns true if successful, false if buffer is full.
    /// When buffer is full, overwrites oldest entry (real-time priority).
    pub fn push(&mut self, tick: &TradeMsg) -> TransportResult<()> {
        unsafe {
            let header = &*self.header;
            let capacity = header.capacity;
            let entry_size = header.entry_size as usize;

            let write_pos = header.write_pos.load(Ordering::Relaxed);
            let read_pos = header.read_pos.load(Ordering::Acquire);

            // Check if buffer is full
            if write_pos.wrapping_sub(read_pos) >= capacity {
                // Overwrite oldest - advance read position
                header.read_pos.fetch_add(1, Ordering::Release);
                header.mark_overflow();
            }

            // Calculate write offset
            let index = (write_pos % capacity) as usize;
            let offset = index * entry_size;

            // Write data - copy bytes directly (TradeMsg is repr(C))
            let dest = self.data.add(offset);
            let src = tick as *const TradeMsg as *const u8;
            std::ptr::copy_nonoverlapping(src, dest, entry_size);

            // Memory fence before updating write position
            std::sync::atomic::fence(Ordering::Release);

            // Update write position
            header
                .write_pos
                .store(write_pos.wrapping_add(1), Ordering::Release);

            Ok(())
        }
    }

    /// Push multiple ticks
    pub fn push_batch(&mut self, ticks: &[TradeMsg]) -> TransportResult<usize> {
        let mut count = 0;
        for tick in ticks {
            self.push(tick)?;
            count += 1;
        }
        Ok(count)
    }

    /// Get buffer utilization
    pub fn utilization(&self) -> f64 {
        unsafe { (*self.header).utilization() }
    }

    /// Get number of items in buffer
    pub fn len(&self) -> u64 {
        unsafe { (*self.header).fill_level() }
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// SPSC Ring buffer consumer
pub struct RingBufferConsumer {
    /// Pointer to header in shared memory (mutable for atomic updates to read_pos)
    header: *mut RingBufferHeader,
    /// Pointer to data area
    data: *const u8,
    /// Cached read position (reserved for future optimization)
    #[allow(dead_code)]
    cached_read_pos: u64,
}

// Safety: RingBufferConsumer is Send+Sync because it owns exclusive read access
// to the consumer side of the ring buffer. The raw pointers point to shared memory
// that is thread-safe through atomic operations.
unsafe impl Send for RingBufferConsumer {}
unsafe impl Sync for RingBufferConsumer {}

impl RingBufferConsumer {
    /// Create from shared memory pointers
    ///
    /// # Safety
    /// - `header` must point to valid, properly aligned RingBufferHeader
    /// - `data` must point to valid memory of size capacity * entry_size
    /// - Only one consumer should exist per buffer
    pub unsafe fn new(header: *const RingBufferHeader, data: *const u8) -> Self {
        Self {
            header: header as *mut RingBufferHeader,
            data,
            cached_read_pos: (*header).read_pos.load(Ordering::Acquire),
        }
    }

    /// Try to pop a tick from the buffer (non-blocking)
    pub fn try_pop(&mut self) -> Option<TradeMsg> {
        unsafe {
            let header = &*self.header;
            let capacity = header.capacity;
            let entry_size = header.entry_size as usize;

            let read_pos = header.read_pos.load(Ordering::Relaxed);
            let write_pos = header.write_pos.load(Ordering::Acquire);

            // Check if buffer is empty
            if read_pos >= write_pos {
                return None;
            }

            // Calculate read offset
            let index = (read_pos % capacity) as usize;
            let offset = index * entry_size;

            // Read data
            let src = self.data.add(offset) as *const TradeMsg;
            let tick = std::ptr::read_volatile(src);

            // Memory fence before updating read position
            std::sync::atomic::fence(Ordering::Acquire);

            // Update read position
            (*self.header)
                .read_pos
                .store(read_pos.wrapping_add(1), Ordering::Release);

            Some(tick)
        }
    }

    /// Pop multiple ticks (non-blocking)
    pub fn pop_batch(&mut self, max_count: usize) -> Vec<TradeMsg> {
        let mut ticks = Vec::with_capacity(max_count);
        for _ in 0..max_count {
            match self.try_pop() {
                Some(tick) => ticks.push(tick),
                None => break,
            }
        }
        ticks
    }

    /// Get number of items available
    pub fn available(&self) -> u64 {
        unsafe { (*self.header).fill_level() }
    }

    /// Check if buffer has data
    pub fn has_data(&self) -> bool {
        self.available() > 0
    }

    /// Check if overflow occurred
    pub fn had_overflow(&self) -> bool {
        unsafe { (*self.header).had_overflow() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use trading_common::data::{create_trade_msg_from_decimals, TradeSideCompat};

    fn create_test_tick(seq: i64) -> TradeMsg {
        let now_nanos = Utc::now().timestamp_nanos_opt().unwrap() as u64;
        create_trade_msg_from_decimals(
            now_nanos,
            now_nanos,
            "ES",
            "CME",
            dec!(5025.50),
            dec!(10),
            TradeSideCompat::Buy,
            seq as u32,
        )
    }

    #[test]
    fn test_header_size() {
        assert_eq!(
            std::mem::size_of::<RingBufferHeader>(),
            RingBufferHeader::SIZE
        );
    }

    #[test]
    fn test_ring_buffer_basic() {
        // Allocate buffer
        let capacity = 16u64;
        let entry_size = std::mem::size_of::<TradeMsg>() as u64;
        let data_size = (capacity as usize) * (entry_size as usize);

        let mut memory = vec![0u8; RingBufferHeader::SIZE + data_size];
        let header_ptr = memory.as_mut_ptr() as *mut RingBufferHeader;
        let data_ptr = unsafe { memory.as_mut_ptr().add(RingBufferHeader::SIZE) };

        // Initialize header
        unsafe {
            (*header_ptr).init(capacity, entry_size);
        }

        // Create producer and consumer
        let mut producer = unsafe { RingBufferProducer::new(header_ptr, data_ptr) };
        let mut consumer = unsafe { RingBufferConsumer::new(header_ptr, data_ptr) };

        // Push and pop
        let tick1 = create_test_tick(1);
        producer.push(&tick1).unwrap();

        assert!(consumer.has_data());
        let popped = consumer.try_pop().unwrap();
        assert_eq!(popped.sequence, 1);
        assert!(!consumer.has_data());
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let capacity = 4u64;
        let entry_size = std::mem::size_of::<TradeMsg>() as u64;
        let data_size = (capacity as usize) * (entry_size as usize);

        let mut memory = vec![0u8; RingBufferHeader::SIZE + data_size];
        let header_ptr = memory.as_mut_ptr() as *mut RingBufferHeader;
        let data_ptr = unsafe { memory.as_mut_ptr().add(RingBufferHeader::SIZE) };

        unsafe {
            (*header_ptr).init(capacity, entry_size);
        }

        let mut producer = unsafe { RingBufferProducer::new(header_ptr, data_ptr) };
        let mut consumer = unsafe { RingBufferConsumer::new(header_ptr, data_ptr) };

        // Fill beyond capacity (should overwrite oldest)
        for i in 0..6 {
            producer.push(&create_test_tick(i)).unwrap();
        }

        // Should have had overflow
        assert!(consumer.had_overflow());

        // Should only be able to read the last 4
        let batch = consumer.pop_batch(10);
        assert_eq!(batch.len(), 4);
        assert_eq!(batch[0].sequence, 2); // First two were overwritten
    }
}
