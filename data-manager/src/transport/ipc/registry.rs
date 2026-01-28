//! Service Registry for Data Manager Instances
//!
//! Provides service discovery and health tracking for multiple data-manager instances.
//! Trading-core uses this to find which instance serves a particular symbol.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Shared Memory Registry                    │
//! │  /data_manager_registry                                     │
//! │                                                              │
//! │  Header (64 bytes):                                         │
//! │  - magic, version, max_entries, active_count, lock          │
//! │                                                              │
//! │  Entries (256 bytes each):                                  │
//! │  ┌──────────────────────────────────────────────────────┐   │
//! │  │ Slot 0: kraken-prod-1 | KRAKEN | BTCUSD,ETHUSD | ♥   │   │
//! │  ├──────────────────────────────────────────────────────┤   │
//! │  │ Slot 1: binance-prod | BINANCE | BTCUSD,BNBUSD | ♥   │   │
//! │  ├──────────────────────────────────────────────────────┤   │
//! │  │ Slot 2: (inactive)                                    │   │
//! │  └──────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//!          ▲              ▲              ▲
//!          │              │              │
//!    data-manager-1  data-manager-2  trading-core
//!    (registers)     (registers)     (discovers)
//! ```
//!
//! # Heartbeat Protocol
//!
//! Each data-manager sends a heartbeat every 5 seconds.
//! Entries with heartbeat older than 30 seconds are considered stale.
//! Trading-core can use this for failover.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use shared_memory::{Shmem, ShmemConf};
use tracing::{debug, info, warn};

use crate::transport::{TransportError, TransportResult};

// =============================================================================
// CONSTANTS
// =============================================================================

/// Registry shared memory name
pub const REGISTRY_NAME: &str = "/data_manager_registry";

/// Magic number to identify valid registry
const REGISTRY_MAGIC: u64 = 0x444D5245_47495354; // "DMREGIST"

/// Current protocol version
const REGISTRY_VERSION: u32 = 1;

/// Maximum number of instances that can register
const MAX_REGISTRY_ENTRIES: usize = 64;

/// Maximum length of instance ID
const MAX_INSTANCE_ID_LEN: usize = 32;

/// Maximum length of provider name
const MAX_PROVIDER_LEN: usize = 32;

/// Maximum length of exchange name
const MAX_EXCHANGE_LEN: usize = 32;

/// Maximum length of channel prefix
const MAX_CHANNEL_PREFIX_LEN: usize = 64;

/// Maximum length of symbol list (comma-separated)
const MAX_SYMBOLS_LEN: usize = 256;

/// Heartbeat interval (how often instances should heartbeat)
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Stale threshold (entries older than this are considered dead)
pub const STALE_THRESHOLD: Duration = Duration::from_secs(30);

// =============================================================================
// ENTRY STATUS
// =============================================================================

/// Status of a registry entry
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryStatus {
    /// Slot is empty/available
    Inactive = 0,
    /// Instance is active and healthy
    Active = 1,
    /// Instance is shutting down (draining)
    Draining = 2,
}

impl From<u8> for EntryStatus {
    fn from(value: u8) -> Self {
        match value {
            1 => EntryStatus::Active,
            2 => EntryStatus::Draining,
            _ => EntryStatus::Inactive,
        }
    }
}

// =============================================================================
// REGISTRY ENTRY
// =============================================================================

/// A single entry in the registry (fixed size)
#[repr(C)]
#[derive(Clone)]
pub struct RegistryEntry {
    /// Unique instance identifier
    pub instance_id: [u8; MAX_INSTANCE_ID_LEN],
    /// Provider name (kraken, binance, etc.)
    pub provider: [u8; MAX_PROVIDER_LEN],
    /// Exchange name (KRAKEN, BINANCE, etc.)
    pub exchange: [u8; MAX_EXCHANGE_LEN],
    /// IPC channel prefix for this instance
    pub channel_prefix: [u8; MAX_CHANNEL_PREFIX_LEN],
    /// Comma-separated list of symbols
    pub symbols: [u8; MAX_SYMBOLS_LEN],
    /// Last heartbeat timestamp (milliseconds since epoch) - placed before smaller fields for alignment
    pub last_heartbeat: u64,
    /// Number of symbols
    pub symbol_count: u16,
    /// Entry status
    pub status: u8,
    /// Padding to round size
    _padding: [u8; 5],
}

impl RegistryEntry {
    /// Size of a registry entry in bytes (432 with natural alignment)
    pub const SIZE: usize = 432;

    /// Create a new empty entry
    pub fn empty() -> Self {
        Self {
            instance_id: [0; MAX_INSTANCE_ID_LEN],
            provider: [0; MAX_PROVIDER_LEN],
            exchange: [0; MAX_EXCHANGE_LEN],
            channel_prefix: [0; MAX_CHANNEL_PREFIX_LEN],
            symbols: [0; MAX_SYMBOLS_LEN],
            status: EntryStatus::Inactive as u8,
            symbol_count: 0,
            last_heartbeat: 0,
            _padding: [0; 5],
        }
    }

    /// Create a new active entry
    pub fn new(
        instance_id: &str,
        provider: &str,
        exchange: &str,
        channel_prefix: &str,
        symbols: &[String],
    ) -> Self {
        let mut entry = Self::empty();

        // Copy instance ID
        let id_bytes = instance_id.as_bytes();
        let len = id_bytes.len().min(MAX_INSTANCE_ID_LEN - 1);
        entry.instance_id[..len].copy_from_slice(&id_bytes[..len]);

        // Copy provider
        let provider_bytes = provider.as_bytes();
        let len = provider_bytes.len().min(MAX_PROVIDER_LEN - 1);
        entry.provider[..len].copy_from_slice(&provider_bytes[..len]);

        // Copy exchange
        let exchange_bytes = exchange.as_bytes();
        let len = exchange_bytes.len().min(MAX_EXCHANGE_LEN - 1);
        entry.exchange[..len].copy_from_slice(&exchange_bytes[..len]);

        // Copy channel prefix
        let prefix_bytes = channel_prefix.as_bytes();
        let len = prefix_bytes.len().min(MAX_CHANNEL_PREFIX_LEN - 1);
        entry.channel_prefix[..len].copy_from_slice(&prefix_bytes[..len]);

        // Copy symbols (comma-separated)
        let symbols_str = symbols.join(",");
        let symbols_bytes = symbols_str.as_bytes();
        let len = symbols_bytes.len().min(MAX_SYMBOLS_LEN - 1);
        entry.symbols[..len].copy_from_slice(&symbols_bytes[..len]);

        entry.symbol_count = symbols.len() as u16;
        entry.status = EntryStatus::Active as u8;
        entry.last_heartbeat = current_time_millis();

        entry
    }

    /// Get instance ID as string
    pub fn instance_id_str(&self) -> &str {
        let end = self.instance_id.iter().position(|&b| b == 0).unwrap_or(MAX_INSTANCE_ID_LEN);
        std::str::from_utf8(&self.instance_id[..end]).unwrap_or("")
    }

    /// Get provider as string
    pub fn provider_str(&self) -> &str {
        let end = self.provider.iter().position(|&b| b == 0).unwrap_or(MAX_PROVIDER_LEN);
        std::str::from_utf8(&self.provider[..end]).unwrap_or("")
    }

    /// Get exchange as string
    pub fn exchange_str(&self) -> &str {
        let end = self.exchange.iter().position(|&b| b == 0).unwrap_or(MAX_EXCHANGE_LEN);
        std::str::from_utf8(&self.exchange[..end]).unwrap_or("")
    }

    /// Get channel prefix as string
    pub fn channel_prefix_str(&self) -> &str {
        let end = self.channel_prefix.iter().position(|&b| b == 0).unwrap_or(MAX_CHANNEL_PREFIX_LEN);
        std::str::from_utf8(&self.channel_prefix[..end]).unwrap_or("")
    }

    /// Get symbols as a list
    pub fn symbols_list(&self) -> Vec<String> {
        let end = self.symbols.iter().position(|&b| b == 0).unwrap_or(MAX_SYMBOLS_LEN);
        let symbols_str = std::str::from_utf8(&self.symbols[..end]).unwrap_or("");
        symbols_str
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    }

    /// Check if this entry serves a particular symbol
    pub fn has_symbol(&self, symbol: &str) -> bool {
        self.symbols_list().iter().any(|s| s == symbol)
    }

    /// Get entry status
    pub fn status(&self) -> EntryStatus {
        EntryStatus::from(self.status)
    }

    /// Check if entry is active
    pub fn is_active(&self) -> bool {
        self.status() == EntryStatus::Active
    }

    /// Check if entry is stale (no heartbeat for too long)
    pub fn is_stale(&self) -> bool {
        let now = current_time_millis();
        let age = now.saturating_sub(self.last_heartbeat);
        age > STALE_THRESHOLD.as_millis() as u64
    }

    /// Check if entry is healthy (active and not stale)
    pub fn is_healthy(&self) -> bool {
        self.is_active() && !self.is_stale()
    }

    /// Update the heartbeat timestamp
    pub fn touch(&mut self) {
        self.last_heartbeat = current_time_millis();
    }

    /// Get age since last heartbeat
    pub fn age(&self) -> Duration {
        let now = current_time_millis();
        let age_ms = now.saturating_sub(self.last_heartbeat);
        Duration::from_millis(age_ms)
    }
}

impl std::fmt::Debug for RegistryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegistryEntry")
            .field("instance_id", &self.instance_id_str())
            .field("provider", &self.provider_str())
            .field("exchange", &self.exchange_str())
            .field("channel_prefix", &self.channel_prefix_str())
            .field("symbol_count", &self.symbol_count)
            .field("status", &self.status())
            .field("age", &self.age())
            .finish()
    }
}

// Verify size at compile time
const _: () = assert!(std::mem::size_of::<RegistryEntry>() == RegistryEntry::SIZE);

// =============================================================================
// REGISTRY HEADER
// =============================================================================

/// Registry header in shared memory
#[repr(C)]
pub struct RegistryHeader {
    /// Magic number to identify valid registry
    pub magic: u64,
    /// Simple spinlock for write operations (placed early for 8-byte alignment)
    pub write_lock: AtomicU64,
    /// Protocol version
    pub version: u32,
    /// Maximum number of entries
    pub max_entries: u32,
    /// Number of active entries
    pub active_count: AtomicU32,
    /// Padding to 64 bytes
    _padding: [u8; 36],
}

impl RegistryHeader {
    /// Size of the header in bytes
    pub const SIZE: usize = 64;

    /// Initialize a new header
    pub fn init(&mut self, max_entries: u32) {
        self.magic = REGISTRY_MAGIC;
        self.version = REGISTRY_VERSION;
        self.max_entries = max_entries;
        self.active_count = AtomicU32::new(0);
        self.write_lock = AtomicU64::new(0);
        self._padding = [0; 36];
    }

    /// Check if this is a valid registry
    pub fn is_valid(&self) -> bool {
        self.magic == REGISTRY_MAGIC && self.version == REGISTRY_VERSION
    }

    /// Acquire the write lock (simple spinlock)
    pub fn lock(&self) {
        loop {
            if self.write_lock.compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed).is_ok() {
                return;
            }
            std::hint::spin_loop();
        }
    }

    /// Release the write lock
    pub fn unlock(&self) {
        self.write_lock.store(0, Ordering::Release);
    }
}

// Verify size at compile time
const _: () = assert!(std::mem::size_of::<RegistryHeader>() == RegistryHeader::SIZE);

// =============================================================================
// REGISTRY
// =============================================================================

/// Service registry for data-manager instances
pub struct Registry {
    shmem: Shmem,
    is_owner: bool,
}

// Safety: Uses atomic operations for synchronization
unsafe impl Send for Registry {}
unsafe impl Sync for Registry {}

impl Registry {
    /// Total size of the registry in shared memory
    pub const TOTAL_SIZE: usize = RegistryHeader::SIZE + (MAX_REGISTRY_ENTRIES * RegistryEntry::SIZE);

    /// Create a new registry (or open existing)
    pub fn open_or_create() -> TransportResult<Self> {
        // Try to open existing first
        match ShmemConf::new().os_id(REGISTRY_NAME).open() {
            Ok(shmem) => {
                // Verify it's a valid registry
                let header = unsafe { &*(shmem.as_ptr() as *const RegistryHeader) };
                if header.is_valid() {
                    debug!("Opened existing registry with {} entries", header.max_entries);
                    return Ok(Self { shmem, is_owner: false });
                }
                warn!("Existing registry is invalid, recreating");
            }
            Err(_) => {
                debug!("Registry doesn't exist, creating new one");
            }
        }

        // Try to create new registry, but if it fails because it already exists,
        // try opening again (handles race condition)
        match Self::create() {
            Ok(registry) => Ok(registry),
            Err(_) => {
                // Another process may have created it, try opening
                match ShmemConf::new().os_id(REGISTRY_NAME).open() {
                    Ok(shmem) => {
                        let header = unsafe { &*(shmem.as_ptr() as *const RegistryHeader) };
                        if header.is_valid() {
                            debug!("Opened registry after create race");
                            return Ok(Self { shmem, is_owner: false });
                        }
                        Err(TransportError::Connection("Invalid registry after race".to_string()))
                    }
                    Err(e) => Err(TransportError::Connection(format!("Failed to open registry: {}", e))),
                }
            }
        }
    }

    /// Create a new registry
    pub fn create() -> TransportResult<Self> {
        // Try to unlink any existing
        let _ = Self::unlink();

        let shmem = ShmemConf::new()
            .size(Self::TOTAL_SIZE)
            .os_id(REGISTRY_NAME)
            .create()
            .map_err(|e| TransportError::Connection(format!("Failed to create registry: {}", e)))?;

        // Initialize header
        let header = unsafe { &mut *(shmem.as_ptr() as *mut RegistryHeader) };
        header.init(MAX_REGISTRY_ENTRIES as u32);

        // Initialize all entries as inactive
        let entries_ptr = unsafe { shmem.as_ptr().add(RegistryHeader::SIZE) as *mut RegistryEntry };
        for i in 0..MAX_REGISTRY_ENTRIES {
            unsafe {
                std::ptr::write(entries_ptr.add(i), RegistryEntry::empty());
            }
        }

        info!("Created new registry with {} slots ({} KB)", MAX_REGISTRY_ENTRIES, Self::TOTAL_SIZE / 1024);

        Ok(Self { shmem, is_owner: true })
    }

    /// Open an existing registry (read-only for discovery)
    pub fn open() -> TransportResult<Self> {
        let shmem = ShmemConf::new()
            .os_id(REGISTRY_NAME)
            .open()
            .map_err(|e| TransportError::Connection(format!("Failed to open registry: {}", e)))?;

        // Verify it's a valid registry
        let header = unsafe { &*(shmem.as_ptr() as *const RegistryHeader) };
        if !header.is_valid() {
            return Err(TransportError::Connection("Invalid registry".to_string()));
        }

        Ok(Self { shmem, is_owner: false })
    }

    /// Register a new instance
    pub fn register(&self, entry: RegistryEntry) -> TransportResult<usize> {
        let header = self.header();
        header.lock();

        // Find an empty slot
        let slot = {
            let entries = self.entries();
            let mut found_slot = None;

            // First, try to find an existing entry with the same instance_id (re-registration)
            for (i, e) in entries.iter().enumerate() {
                if e.instance_id_str() == entry.instance_id_str() {
                    found_slot = Some(i);
                    break;
                }
            }

            // If not found, find an empty slot
            if found_slot.is_none() {
                for (i, e) in entries.iter().enumerate() {
                    if !e.is_active() {
                        found_slot = Some(i);
                        break;
                    }
                }
            }

            found_slot
        };

        match slot {
            Some(i) => {
                // Write the entry
                let entries_ptr = self.entries_ptr();
                unsafe {
                    std::ptr::write(entries_ptr.add(i), entry);
                }
                header.active_count.fetch_add(1, Ordering::Relaxed);
                header.unlock();

                info!("Registered instance in slot {}", i);
                Ok(i)
            }
            None => {
                header.unlock();
                Err(TransportError::BufferFull)
            }
        }
    }

    /// Update heartbeat for a slot
    pub fn heartbeat(&self, slot: usize) -> TransportResult<()> {
        if slot >= MAX_REGISTRY_ENTRIES {
            return Err(TransportError::Connection("Invalid slot".to_string()));
        }

        let entries_ptr = self.entries_ptr();
        unsafe {
            let entry = &mut *entries_ptr.add(slot);
            entry.touch();
        }

        Ok(())
    }

    /// Deregister an instance
    pub fn deregister(&self, slot: usize) -> TransportResult<()> {
        if slot >= MAX_REGISTRY_ENTRIES {
            return Err(TransportError::Connection("Invalid slot".to_string()));
        }

        let header = self.header();
        header.lock();

        let entries_ptr = self.entries_ptr();
        unsafe {
            let entry = &mut *entries_ptr.add(slot);
            entry.status = EntryStatus::Inactive as u8;
        }

        header.active_count.fetch_sub(1, Ordering::Relaxed);
        header.unlock();

        info!("Deregistered slot {}", slot);
        Ok(())
    }

    /// Find instances serving a particular symbol
    pub fn find_by_symbol(&self, symbol: &str) -> Vec<(usize, RegistryEntry)> {
        self.entries()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_healthy() && e.has_symbol(symbol))
            .map(|(i, e)| (i, e.clone()))
            .collect()
    }

    /// Find instances by provider
    pub fn find_by_provider(&self, provider: &str) -> Vec<(usize, RegistryEntry)> {
        self.entries()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_healthy() && e.provider_str() == provider)
            .map(|(i, e)| (i, e.clone()))
            .collect()
    }

    /// List all active instances
    pub fn list_active(&self) -> Vec<(usize, RegistryEntry)> {
        self.entries()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_active())
            .map(|(i, e)| (i, e.clone()))
            .collect()
    }

    /// List all healthy instances (active and not stale)
    pub fn list_healthy(&self) -> Vec<(usize, RegistryEntry)> {
        self.entries()
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_healthy())
            .map(|(i, e)| (i, e.clone()))
            .collect()
    }

    /// Get a specific entry
    pub fn get(&self, slot: usize) -> Option<RegistryEntry> {
        if slot >= MAX_REGISTRY_ENTRIES {
            return None;
        }
        let entry = self.entries()[slot].clone();
        if entry.is_active() {
            Some(entry)
        } else {
            None
        }
    }

    /// Cleanup stale entries
    pub fn cleanup_stale(&self) -> usize {
        let header = self.header();
        header.lock();

        let entries_ptr = self.entries_ptr();
        let mut cleaned = 0;

        for i in 0..MAX_REGISTRY_ENTRIES {
            unsafe {
                let entry = &mut *entries_ptr.add(i);
                if entry.is_active() && entry.is_stale() {
                    warn!(
                        "Cleaning up stale instance: {} (age: {:?})",
                        entry.instance_id_str(),
                        entry.age()
                    );
                    entry.status = EntryStatus::Inactive as u8;
                    header.active_count.fetch_sub(1, Ordering::Relaxed);
                    cleaned += 1;
                }
            }
        }

        header.unlock();
        cleaned
    }

    /// Unlink the registry shared memory
    pub fn unlink() -> TransportResult<()> {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            let c_name = CString::new(REGISTRY_NAME)
                .map_err(|e| TransportError::Connection(format!("Invalid name: {}", e)))?;
            unsafe {
                libc::shm_unlink(c_name.as_ptr());
            }
        }
        Ok(())
    }

    fn header(&self) -> &RegistryHeader {
        unsafe { &*(self.shmem.as_ptr() as *const RegistryHeader) }
    }

    fn entries(&self) -> &[RegistryEntry] {
        let entries_ptr = unsafe { self.shmem.as_ptr().add(RegistryHeader::SIZE) as *const RegistryEntry };
        unsafe { std::slice::from_raw_parts(entries_ptr, MAX_REGISTRY_ENTRIES) }
    }

    fn entries_ptr(&self) -> *mut RegistryEntry {
        unsafe { self.shmem.as_ptr().add(RegistryHeader::SIZE) as *mut RegistryEntry }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        if self.is_owner {
            debug!("Registry owner dropping - shared memory will persist for other readers");
        }
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Get current time in milliseconds since epoch
fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Generate a unique instance ID
pub fn generate_instance_id(provider: &str) -> String {
    use std::process;
    let pid = process::id();
    let timestamp = current_time_millis() % 100000; // Last 5 digits
    format!("{}-{}-{}", provider, pid, timestamp)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_size() {
        assert_eq!(std::mem::size_of::<RegistryEntry>(), RegistryEntry::SIZE);
        assert_eq!(std::mem::size_of::<RegistryHeader>(), RegistryHeader::SIZE);
    }

    #[test]
    fn test_registry_entry_creation() {
        let symbols = vec!["BTCUSD".to_string(), "ETHUSD".to_string()];
        let entry = RegistryEntry::new(
            "test-instance-1",
            "kraken",
            "KRAKEN",
            "/dm_kraken_",
            &symbols,
        );

        assert_eq!(entry.instance_id_str(), "test-instance-1");
        assert_eq!(entry.provider_str(), "kraken");
        assert_eq!(entry.exchange_str(), "KRAKEN");
        assert_eq!(entry.channel_prefix_str(), "/dm_kraken_");
        assert_eq!(entry.symbols_list(), symbols);
        assert_eq!(entry.symbol_count, 2);
        assert!(entry.is_active());
        assert!(!entry.is_stale());
    }

    #[test]
    fn test_registry_basic() {
        // Use open_or_create to handle concurrent test execution
        let registry = Registry::open_or_create().unwrap();

        let symbols = vec!["BTCUSD".to_string(), "ETHUSD".to_string()];
        let entry = RegistryEntry::new(
            "test-basic",
            "kraken",
            "KRAKEN",
            "/dm_test_",
            &symbols,
        );

        let slot = registry.register(entry).unwrap();
        assert!(slot < MAX_REGISTRY_ENTRIES);

        // Find by symbol
        let found = registry.find_by_symbol("BTCUSD");
        assert!(!found.is_empty());
        assert!(found.iter().any(|(_, e)| e.instance_id_str() == "test-basic"));

        // Heartbeat
        registry.heartbeat(slot).unwrap();

        // Deregister
        registry.deregister(slot).unwrap();

        // Note: other tests may have entries, so just check our entry is gone
        let found = registry.find_by_symbol("BTCUSD");
        assert!(!found.iter().any(|(_, e)| e.instance_id_str() == "test-basic"));
    }

    #[test]
    fn test_multiple_instances() {
        // Use open_or_create to handle concurrent test execution
        let registry = Registry::open_or_create().unwrap();

        // Register two instances
        let entry1 = RegistryEntry::new(
            "kraken-1",
            "kraken",
            "KRAKEN",
            "/dm_kraken1_",
            &["BTCUSD".to_string()],
        );
        let entry2 = RegistryEntry::new(
            "binance-1",
            "binance",
            "BINANCE",
            "/dm_binance1_",
            &["BTCUSD".to_string(), "BNBUSD".to_string()],
        );

        let slot1 = registry.register(entry1).unwrap();
        let slot2 = registry.register(entry2).unwrap();

        assert_ne!(slot1, slot2);

        // Both serve BTCUSD (may have more from other tests)
        let found = registry.find_by_symbol("BTCUSD");
        assert!(found.len() >= 2);
        assert!(found.iter().any(|(_, e)| e.instance_id_str() == "kraken-1"));
        assert!(found.iter().any(|(_, e)| e.instance_id_str() == "binance-1"));

        // Only binance-1 serves BNBUSD (from our entries)
        let found = registry.find_by_symbol("BNBUSD");
        assert!(found.iter().any(|(_, e)| e.instance_id_str() == "binance-1"));

        // Cleanup
        registry.deregister(slot1).unwrap();
        registry.deregister(slot2).unwrap();
    }

    #[test]
    fn test_generate_instance_id() {
        let id1 = generate_instance_id("kraken");
        let id2 = generate_instance_id("kraken");

        assert!(id1.starts_with("kraken-"));
        assert!(id2.starts_with("kraken-"));
        // They might be the same if generated in the same millisecond
    }
}
