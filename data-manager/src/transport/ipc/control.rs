//! Control Channel for Dynamic IPC Subscription Management
//!
//! Provides a request-response protocol for trading-core to request
//! new symbol subscriptions from data-manager at runtime.
//!
//! # Protocol
//!
//! ```text
//! trading-core                         data-manager
//!      │                                    │
//!      │  SubscribeRequest(ETHUSD)          │
//!      │───────────────────────────────────►│
//!      │                                    │
//!      │                    [check if subscribed]
//!      │                    [if not, subscribe to provider]
//!      │                    [create IPC channel]
//!      │                                    │
//!      │  SubscribeResponse(Success)        │
//!      │◄───────────────────────────────────│
//!      │                                    │
//!      │  [open IPC channel]                │
//!      │                                    │
//! ```
//!
//! # Memory Layout
//!
//! The control channel uses two ring buffers in shared memory:
//! - Request buffer: trading-core writes, data-manager reads
//! - Response buffer: data-manager writes, trading-core reads

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use shared_memory::{Shmem, ShmemConf};
use tracing::{debug, info, warn};

use crate::transport::{TransportError, TransportResult};

/// Control channel shared memory name
pub const CONTROL_CHANNEL_NAME: &str = "/data_manager_control";

/// Maximum symbol length in bytes
const MAX_SYMBOL_LEN: usize = 32;
/// Maximum exchange length in bytes
const MAX_EXCHANGE_LEN: usize = 32;
/// Maximum provider length in bytes
const MAX_PROVIDER_LEN: usize = 32;
/// Maximum error message length
const MAX_ERROR_MSG_LEN: usize = 96;

/// Request types for the control channel
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestType {
    /// Subscribe to a symbol (creates IPC channel if needed)
    Subscribe = 1,
    /// Unsubscribe from a symbol (closes IPC channel)
    Unsubscribe = 2,
    /// Ping to check if data-manager is alive
    Ping = 3,
    /// List currently subscribed symbols
    ListSymbols = 4,
}

impl TryFrom<u8> for RequestType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(RequestType::Subscribe),
            2 => Ok(RequestType::Unsubscribe),
            3 => Ok(RequestType::Ping),
            4 => Ok(RequestType::ListSymbols),
            _ => Err(()),
        }
    }
}

/// Response types for the control channel
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseType {
    /// Request succeeded
    Success = 1,
    /// Request failed with error
    Failed = 2,
    /// Pong response to ping
    Pong = 3,
}

impl TryFrom<u8> for ResponseType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ResponseType::Success),
            2 => Ok(ResponseType::Failed),
            3 => Ok(ResponseType::Pong),
            _ => Err(()),
        }
    }
}

/// Error codes for failed requests
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// No error
    None = 0,
    /// Symbol is not valid on the provider
    InvalidSymbol = 1,
    /// Provider connection error
    ProviderError = 2,
    /// Symbol is already subscribed
    AlreadySubscribed = 3,
    /// Symbol is not subscribed (for unsubscribe)
    NotSubscribed = 4,
    /// Provider not available
    ProviderNotAvailable = 5,
    /// Internal error
    InternalError = 6,
}

impl TryFrom<u8> for ErrorCode {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ErrorCode::None),
            1 => Ok(ErrorCode::InvalidSymbol),
            2 => Ok(ErrorCode::ProviderError),
            3 => Ok(ErrorCode::AlreadySubscribed),
            4 => Ok(ErrorCode::NotSubscribed),
            5 => Ok(ErrorCode::ProviderNotAvailable),
            6 => Ok(ErrorCode::InternalError),
            _ => Err(()),
        }
    }
}

/// Control request message (128 bytes, fixed size)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ControlRequest {
    /// Unique message ID for matching responses
    pub message_id: u64,
    /// Request type
    pub request_type: u8,
    /// Symbol in DBT canonical format (null-terminated)
    pub symbol: [u8; MAX_SYMBOL_LEN],
    /// Exchange name (null-terminated)
    pub exchange: [u8; MAX_EXCHANGE_LEN],
    /// Provider name (null-terminated)
    pub provider: [u8; MAX_PROVIDER_LEN],
    /// Padding to 128 bytes
    _padding: [u8; 23],
}

impl ControlRequest {
    /// Size of the request message in bytes
    pub const SIZE: usize = 128;

    /// Create a new subscribe request
    pub fn subscribe(message_id: u64, symbol: &str, exchange: &str, provider: &str) -> Self {
        Self::new(message_id, RequestType::Subscribe, symbol, exchange, provider)
    }

    /// Create a new unsubscribe request
    pub fn unsubscribe(message_id: u64, symbol: &str, exchange: &str) -> Self {
        Self::new(message_id, RequestType::Unsubscribe, symbol, exchange, "")
    }

    /// Create a new ping request
    pub fn ping(message_id: u64) -> Self {
        Self::new(message_id, RequestType::Ping, "", "", "")
    }

    fn new(
        message_id: u64,
        request_type: RequestType,
        symbol: &str,
        exchange: &str,
        provider: &str,
    ) -> Self {
        let mut req = Self {
            message_id,
            request_type: request_type as u8,
            symbol: [0u8; MAX_SYMBOL_LEN],
            exchange: [0u8; MAX_EXCHANGE_LEN],
            provider: [0u8; MAX_PROVIDER_LEN],
            _padding: [0u8; 23],
        };

        // Copy strings (null-terminated)
        let symbol_bytes = symbol.as_bytes();
        let len = symbol_bytes.len().min(MAX_SYMBOL_LEN - 1);
        req.symbol[..len].copy_from_slice(&symbol_bytes[..len]);

        let exchange_bytes = exchange.as_bytes();
        let len = exchange_bytes.len().min(MAX_EXCHANGE_LEN - 1);
        req.exchange[..len].copy_from_slice(&exchange_bytes[..len]);

        let provider_bytes = provider.as_bytes();
        let len = provider_bytes.len().min(MAX_PROVIDER_LEN - 1);
        req.provider[..len].copy_from_slice(&provider_bytes[..len]);

        req
    }

    /// Get the symbol as a string
    pub fn symbol_str(&self) -> &str {
        let end = self.symbol.iter().position(|&b| b == 0).unwrap_or(MAX_SYMBOL_LEN);
        std::str::from_utf8(&self.symbol[..end]).unwrap_or("")
    }

    /// Get the exchange as a string
    pub fn exchange_str(&self) -> &str {
        let end = self.exchange.iter().position(|&b| b == 0).unwrap_or(MAX_EXCHANGE_LEN);
        std::str::from_utf8(&self.exchange[..end]).unwrap_or("")
    }

    /// Get the provider as a string
    pub fn provider_str(&self) -> &str {
        let end = self.provider.iter().position(|&b| b == 0).unwrap_or(MAX_PROVIDER_LEN);
        std::str::from_utf8(&self.provider[..end]).unwrap_or("")
    }

    /// Get the request type
    pub fn request_type(&self) -> Option<RequestType> {
        RequestType::try_from(self.request_type).ok()
    }
}

/// Control response message (128 bytes, fixed size)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ControlResponse {
    /// Message ID matching the request
    pub message_id: u64,
    /// Response type
    pub response_type: u8,
    /// Error code (if response_type is Error)
    pub error_code: u8,
    /// Error message (null-terminated)
    pub error_message: [u8; MAX_ERROR_MSG_LEN],
    /// Padding to 128 bytes
    _padding: [u8; 22],
}

impl ControlResponse {
    /// Size of the response message in bytes
    pub const SIZE: usize = 128;

    /// Create a success response
    pub fn success(message_id: u64) -> Self {
        Self {
            message_id,
            response_type: ResponseType::Success as u8,
            error_code: ErrorCode::None as u8,
            error_message: [0u8; MAX_ERROR_MSG_LEN],
            _padding: [0u8; 22],
        }
    }

    /// Create an error response
    pub fn error(message_id: u64, error_code: ErrorCode, message: &str) -> Self {
        let mut resp = Self {
            message_id,
            response_type: ResponseType::Failed as u8,
            error_code: error_code as u8,
            error_message: [0u8; MAX_ERROR_MSG_LEN],
            _padding: [0u8; 22],
        };

        let msg_bytes = message.as_bytes();
        let len = msg_bytes.len().min(MAX_ERROR_MSG_LEN - 1);
        resp.error_message[..len].copy_from_slice(&msg_bytes[..len]);

        resp
    }

    /// Create a pong response
    pub fn pong(message_id: u64) -> Self {
        Self {
            message_id,
            response_type: ResponseType::Pong as u8,
            error_code: ErrorCode::None as u8,
            error_message: [0u8; MAX_ERROR_MSG_LEN],
            _padding: [0u8; 22],
        }
    }

    /// Get the response type
    pub fn response_type(&self) -> Option<ResponseType> {
        ResponseType::try_from(self.response_type).ok()
    }

    /// Get the error code
    pub fn error_code(&self) -> Option<ErrorCode> {
        ErrorCode::try_from(self.error_code).ok()
    }

    /// Get the error message as a string
    pub fn error_message_str(&self) -> &str {
        let end = self
            .error_message
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(MAX_ERROR_MSG_LEN);
        std::str::from_utf8(&self.error_message[..end]).unwrap_or("")
    }

    /// Check if this is a success response
    pub fn is_success(&self) -> bool {
        self.response_type().map(|t| t == ResponseType::Success).unwrap_or(false)
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        self.response_type().map(|t| t == ResponseType::Failed).unwrap_or(false)
    }
}

/// Control channel header in shared memory
#[repr(C)]
pub struct ControlChannelHeader {
    /// Request write position (client writes)
    pub request_write_pos: AtomicU64,
    /// Request read position (server reads)
    pub request_read_pos: AtomicU64,
    /// Response write position (server writes)
    pub response_write_pos: AtomicU64,
    /// Response read position (client reads)
    pub response_read_pos: AtomicU64,
    /// Buffer capacity (number of entries per direction)
    pub capacity: u64,
    /// Flags (bit 0: server active)
    pub flags: AtomicU64,
    /// Padding to cache line
    _padding: [u8; 16],
}

impl ControlChannelHeader {
    /// Header size in bytes (64 bytes, cache-line aligned)
    pub const SIZE: usize = 64;

    /// Flag indicating server is active
    pub const FLAG_SERVER_ACTIVE: u64 = 1;

    /// Initialize the header
    pub fn init(&mut self, capacity: u64) {
        self.request_write_pos = AtomicU64::new(0);
        self.request_read_pos = AtomicU64::new(0);
        self.response_write_pos = AtomicU64::new(0);
        self.response_read_pos = AtomicU64::new(0);
        self.capacity = capacity;
        self.flags = AtomicU64::new(0);
        self._padding = [0u8; 16];
    }

    /// Check if server is active
    pub fn is_server_active(&self) -> bool {
        self.flags.load(Ordering::Acquire) & Self::FLAG_SERVER_ACTIVE != 0
    }

    /// Set server active flag
    pub fn set_server_active(&self, active: bool) {
        if active {
            self.flags.fetch_or(Self::FLAG_SERVER_ACTIVE, Ordering::Release);
        } else {
            self.flags.fetch_and(!Self::FLAG_SERVER_ACTIVE, Ordering::Release);
        }
    }
}

/// Configuration for the control channel
#[derive(Debug, Clone)]
pub struct ControlChannelConfig {
    /// Shared memory name
    pub name: String,
    /// Number of request/response entries
    pub capacity: usize,
}

impl Default for ControlChannelConfig {
    fn default() -> Self {
        Self {
            name: CONTROL_CHANNEL_NAME.to_string(),
            capacity: 64, // 64 pending requests should be plenty
        }
    }
}

/// Control channel server (used by data-manager)
pub struct ControlChannelServer {
    shmem: Shmem,
    config: ControlChannelConfig,
}

// Safety: Uses atomic operations for synchronization
unsafe impl Send for ControlChannelServer {}
unsafe impl Sync for ControlChannelServer {}

impl ControlChannelServer {
    /// Create a new control channel server
    pub fn create(config: ControlChannelConfig) -> TransportResult<Self> {
        // Calculate total size: header + request buffer + response buffer
        let buffer_size = config.capacity * ControlRequest::SIZE;
        let total_size = ControlChannelHeader::SIZE + buffer_size * 2;

        // Try to unlink any existing segment
        let _ = Self::unlink(&config.name);

        // Create shared memory
        let shmem = ShmemConf::new()
            .size(total_size)
            .os_id(&config.name)
            .create()
            .map_err(|e| TransportError::Connection(format!("Failed to create control channel: {}", e)))?;

        // Initialize header
        let header_ptr = shmem.as_ptr() as *mut ControlChannelHeader;
        unsafe {
            (*header_ptr).init(config.capacity as u64);
            (*header_ptr).set_server_active(true);
        }

        info!(
            "Created control channel {} ({} entries, {} bytes)",
            config.name, config.capacity, total_size
        );

        Ok(Self { shmem, config })
    }

    /// Try to receive a request (non-blocking)
    pub fn try_recv(&self) -> Option<ControlRequest> {
        let header = self.header();
        let read_pos = header.request_read_pos.load(Ordering::Relaxed);
        let write_pos = header.request_write_pos.load(Ordering::Acquire);

        if read_pos == write_pos {
            return None; // No pending requests
        }

        // Read the request
        let index = (read_pos % self.config.capacity as u64) as usize;
        let request_buffer = self.request_buffer();
        let offset = index * ControlRequest::SIZE;
        let request_ptr = unsafe { request_buffer.add(offset) } as *const ControlRequest;
        let request = unsafe { std::ptr::read(request_ptr) };

        // Advance read position
        header.request_read_pos.store(read_pos + 1, Ordering::Release);

        Some(request)
    }

    /// Send a response
    pub fn send(&self, response: &ControlResponse) -> TransportResult<()> {
        let header = self.header();
        let write_pos = header.response_write_pos.load(Ordering::Relaxed);
        let read_pos = header.response_read_pos.load(Ordering::Acquire);

        // Check if buffer is full
        if write_pos.wrapping_sub(read_pos) >= self.config.capacity as u64 {
            return Err(TransportError::BufferFull);
        }

        // Write the response
        let index = (write_pos % self.config.capacity as u64) as usize;
        let response_buffer = self.response_buffer();
        let offset = index * ControlResponse::SIZE;
        let response_ptr = unsafe { response_buffer.add(offset) } as *mut ControlResponse;
        unsafe {
            std::ptr::write(response_ptr, *response);
        }

        // Advance write position
        std::sync::atomic::fence(Ordering::Release);
        header.response_write_pos.store(write_pos + 1, Ordering::Release);

        Ok(())
    }

    /// Unlink the shared memory segment
    pub fn unlink(name: &str) -> TransportResult<()> {
        #[cfg(unix)]
        {
            use std::ffi::CString;
            let c_name = CString::new(name)
                .map_err(|e| TransportError::Connection(format!("Invalid name: {}", e)))?;
            unsafe {
                libc::shm_unlink(c_name.as_ptr());
            }
        }
        Ok(())
    }

    fn header(&self) -> &ControlChannelHeader {
        unsafe { &*(self.shmem.as_ptr() as *const ControlChannelHeader) }
    }

    fn request_buffer(&self) -> *const u8 {
        unsafe { self.shmem.as_ptr().add(ControlChannelHeader::SIZE) }
    }

    fn response_buffer(&self) -> *mut u8 {
        let offset = ControlChannelHeader::SIZE + self.config.capacity * ControlRequest::SIZE;
        unsafe { self.shmem.as_ptr().add(offset) as *mut u8 }
    }
}

impl Drop for ControlChannelServer {
    fn drop(&mut self) {
        // Mark server as inactive
        let header = self.header();
        header.set_server_active(false);
        debug!("Control channel server shutting down");
    }
}

/// Control channel client (used by trading-core)
pub struct ControlChannelClient {
    shmem: Shmem,
    config: ControlChannelConfig,
    next_message_id: AtomicU64,
}

// Safety: Uses atomic operations for synchronization
unsafe impl Send for ControlChannelClient {}
unsafe impl Sync for ControlChannelClient {}

impl ControlChannelClient {
    /// Open an existing control channel
    pub fn open(config: ControlChannelConfig) -> TransportResult<Self> {
        let shmem = ShmemConf::new()
            .os_id(&config.name)
            .open()
            .map_err(|e| TransportError::Connection(format!("Failed to open control channel: {}", e)))?;

        // Read capacity from header
        let header = unsafe { &*(shmem.as_ptr() as *const ControlChannelHeader) };
        let capacity = header.capacity as usize;

        debug!("Opened control channel {} ({} entries)", config.name, capacity);

        Ok(Self {
            shmem,
            config: ControlChannelConfig {
                name: config.name,
                capacity,
            },
            next_message_id: AtomicU64::new(1),
        })
    }

    /// Check if the server is active
    pub fn is_server_active(&self) -> bool {
        self.header().is_server_active()
    }

    /// Send a request and wait for response
    pub fn request(&self, request: &ControlRequest, timeout: Duration) -> TransportResult<ControlResponse> {
        // Send the request
        self.send_request(request)?;

        // Wait for response with matching message_id
        let start = Instant::now();
        loop {
            if let Some(response) = self.try_recv() {
                if response.message_id == request.message_id {
                    return Ok(response);
                }
                // Not our response, put it back? For now, log and continue
                warn!(
                    "Received response for different message_id: {} vs {}",
                    response.message_id, request.message_id
                );
            }

            if start.elapsed() > timeout {
                return Err(TransportError::Timeout);
            }

            std::thread::sleep(Duration::from_micros(100));
        }
    }

    /// Subscribe to a symbol
    pub fn subscribe(
        &self,
        symbol: &str,
        exchange: &str,
        provider: &str,
        timeout: Duration,
    ) -> TransportResult<()> {
        let message_id = self.next_message_id.fetch_add(1, Ordering::Relaxed);
        let request = ControlRequest::subscribe(message_id, symbol, exchange, provider);
        let response = self.request(&request, timeout)?;

        if response.is_success() {
            Ok(())
        } else {
            Err(TransportError::Connection(format!(
                "Subscribe failed: {} (code: {:?})",
                response.error_message_str(),
                response.error_code()
            )))
        }
    }

    /// Ping the server
    pub fn ping(&self, timeout: Duration) -> TransportResult<Duration> {
        let start = Instant::now();
        let message_id = self.next_message_id.fetch_add(1, Ordering::Relaxed);
        let request = ControlRequest::ping(message_id);
        let response = self.request(&request, timeout)?;

        if response.response_type() == Some(ResponseType::Pong) {
            Ok(start.elapsed())
        } else {
            Err(TransportError::Connection("Unexpected response to ping".to_string()))
        }
    }

    fn send_request(&self, request: &ControlRequest) -> TransportResult<()> {
        let header = self.header();
        let write_pos = header.request_write_pos.load(Ordering::Relaxed);
        let read_pos = header.request_read_pos.load(Ordering::Acquire);

        // Check if buffer is full
        if write_pos.wrapping_sub(read_pos) >= self.config.capacity as u64 {
            return Err(TransportError::BufferFull);
        }

        // Write the request
        let index = (write_pos % self.config.capacity as u64) as usize;
        let request_buffer = self.request_buffer();
        let offset = index * ControlRequest::SIZE;
        let request_ptr = unsafe { request_buffer.add(offset) } as *mut ControlRequest;
        unsafe {
            std::ptr::write(request_ptr, *request);
        }

        // Advance write position
        std::sync::atomic::fence(Ordering::Release);
        header.request_write_pos.store(write_pos + 1, Ordering::Release);

        Ok(())
    }

    fn try_recv(&self) -> Option<ControlResponse> {
        let header = self.header();
        let read_pos = header.response_read_pos.load(Ordering::Relaxed);
        let write_pos = header.response_write_pos.load(Ordering::Acquire);

        if read_pos == write_pos {
            return None;
        }

        // Read the response
        let index = (read_pos % self.config.capacity as u64) as usize;
        let response_buffer = self.response_buffer();
        let offset = index * ControlResponse::SIZE;
        let response_ptr = unsafe { response_buffer.add(offset) } as *const ControlResponse;
        let response = unsafe { std::ptr::read(response_ptr) };

        // Advance read position
        header.response_read_pos.store(read_pos + 1, Ordering::Release);

        Some(response)
    }

    fn header(&self) -> &ControlChannelHeader {
        unsafe { &*(self.shmem.as_ptr() as *const ControlChannelHeader) }
    }

    fn request_buffer(&self) -> *mut u8 {
        unsafe { self.shmem.as_ptr().add(ControlChannelHeader::SIZE) as *mut u8 }
    }

    fn response_buffer(&self) -> *const u8 {
        let offset = ControlChannelHeader::SIZE + self.config.capacity * ControlRequest::SIZE;
        unsafe { self.shmem.as_ptr().add(offset) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_response_sizes() {
        assert_eq!(std::mem::size_of::<ControlRequest>(), ControlRequest::SIZE);
        assert_eq!(std::mem::size_of::<ControlResponse>(), ControlResponse::SIZE);
        assert_eq!(std::mem::size_of::<ControlChannelHeader>(), ControlChannelHeader::SIZE);
    }

    #[test]
    fn test_control_request_creation() {
        let req = ControlRequest::subscribe(42, "BTCUSD", "KRAKEN", "kraken");
        assert_eq!(req.message_id, 42);
        assert_eq!(req.request_type(), Some(RequestType::Subscribe));
        assert_eq!(req.symbol_str(), "BTCUSD");
        assert_eq!(req.exchange_str(), "KRAKEN");
        assert_eq!(req.provider_str(), "kraken");
    }

    #[test]
    fn test_control_response_creation() {
        let resp = ControlResponse::success(42);
        assert_eq!(resp.message_id, 42);
        assert!(resp.is_success());
        assert!(!resp.is_error());

        let resp = ControlResponse::error(42, ErrorCode::InvalidSymbol, "Symbol not found");
        assert_eq!(resp.message_id, 42);
        assert!(!resp.is_success());
        assert!(resp.is_error());
        assert_eq!(resp.error_code(), Some(ErrorCode::InvalidSymbol));
        assert_eq!(resp.error_message_str(), "Symbol not found");
    }

    #[test]
    fn test_control_channel_basic() {
        let config = ControlChannelConfig {
            name: "/test_control_basic".to_string(),
            capacity: 16,
        };

        // Create server
        let server = ControlChannelServer::create(config.clone()).unwrap();
        assert!(server.header().is_server_active());

        // Create client
        let client = ControlChannelClient::open(config.clone()).unwrap();
        assert!(client.is_server_active());

        // Send request from client
        let req = ControlRequest::ping(1);
        client.send_request(&req).unwrap();

        // Receive on server
        let recv_req = server.try_recv().unwrap();
        assert_eq!(recv_req.message_id, 1);
        assert_eq!(recv_req.request_type(), Some(RequestType::Ping));

        // Send response from server
        let resp = ControlResponse::pong(1);
        server.send(&resp).unwrap();

        // Receive on client
        let recv_resp = client.try_recv().unwrap();
        assert_eq!(recv_resp.message_id, 1);
        assert_eq!(recv_resp.response_type(), Some(ResponseType::Pong));

        // Cleanup
        drop(server);
        drop(client);
        ControlChannelServer::unlink(&config.name).ok();
    }
}
