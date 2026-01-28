//! Control Channel Handler
//!
//! Processes dynamic subscription requests from trading-core via the control channel.
//! This allows trading-core to request IPC channels at runtime without restarting data-manager.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::transport::ipc::{
    ControlChannelConfig, ControlChannelServer, ControlRequest, ControlResponse, ErrorCode,
    RequestType, SharedMemoryTransport,
};

/// State shared between the control handler and the main streaming loop
pub struct ControlHandlerState {
    /// All symbols currently subscribed to the provider
    pub subscribed_symbols: HashSet<String>,
    /// Symbols currently being streamed via IPC
    pub ipc_symbols: RwLock<HashSet<String>>,
    /// Exchange name for IPC channel creation
    pub exchange: String,
}

impl ControlHandlerState {
    pub fn new(subscribed: HashSet<String>, ipc_symbols: HashSet<String>, exchange: &str) -> Self {
        Self {
            subscribed_symbols: subscribed,
            ipc_symbols: RwLock::new(ipc_symbols),
            exchange: exchange.to_string(),
        }
    }

    /// Check if a symbol is subscribed on the provider
    pub fn is_subscribed(&self, symbol: &str) -> bool {
        self.subscribed_symbols.contains(symbol)
    }

    /// Check if a symbol has an IPC channel
    pub fn has_ipc_channel(&self, symbol: &str) -> bool {
        self.ipc_symbols.read().contains(symbol)
    }

    /// Add a symbol to the IPC set
    pub fn add_ipc_symbol(&self, symbol: &str) {
        self.ipc_symbols.write().insert(symbol.to_string());
    }

    /// Get all IPC symbols as a list (for registry)
    pub fn ipc_symbols(&self) -> Vec<String> {
        self.ipc_symbols.read().iter().cloned().collect()
    }
}

/// Control channel handler
///
/// Runs as a background task, processing requests from trading-core.
pub struct ControlHandler {
    server: ControlChannelServer,
    state: Arc<ControlHandlerState>,
    transport: Arc<SharedMemoryTransport>,
}

impl ControlHandler {
    /// Create a new control handler
    pub fn new(
        state: Arc<ControlHandlerState>,
        transport: Arc<SharedMemoryTransport>,
    ) -> Result<Self, anyhow::Error> {
        let config = ControlChannelConfig::default();
        let server = ControlChannelServer::create(config)
            .map_err(|e| anyhow::anyhow!("Failed to create control channel: {}", e))?;

        info!("Control channel handler started");

        Ok(Self {
            server,
            state,
            transport,
        })
    }

    /// Start the control handler as a background task
    pub fn spawn(
        state: Arc<ControlHandlerState>,
        transport: Arc<SharedMemoryTransport>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            match ControlHandler::new(state, transport) {
                Ok(handler) => {
                    handler.run(shutdown_rx).await;
                }
                Err(e) => {
                    error!("Failed to start control handler: {}", e);
                }
            }
        })
    }

    /// Run the control handler loop
    async fn run(self, mut shutdown_rx: broadcast::Receiver<()>) {
        let poll_interval = Duration::from_millis(10);

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Control handler shutting down");
                    break;
                }
                _ = async {
                    // Check for pending requests
                    if let Some(request) = self.server.try_recv() {
                        let response = self.handle_request(&request);
                        if let Err(e) = self.server.send(&response) {
                            error!("Failed to send control response: {}", e);
                        }
                    } else {
                        tokio::time::sleep(poll_interval).await;
                    }
                } => {}
            }
        }
    }

    /// Handle a single control request
    fn handle_request(&self, request: &ControlRequest) -> ControlResponse {
        let message_id = request.message_id;

        match request.request_type() {
            Some(RequestType::Ping) => {
                debug!("Received ping request");
                ControlResponse::pong(message_id)
            }
            Some(RequestType::Subscribe) => {
                self.handle_subscribe(request)
            }
            Some(RequestType::Unsubscribe) => {
                self.handle_unsubscribe(request)
            }
            Some(RequestType::ListSymbols) => {
                // For now, just return success - could extend to return symbol list
                ControlResponse::success(message_id)
            }
            None => {
                warn!("Received unknown request type: {}", request.request_type);
                ControlResponse::error(
                    message_id,
                    ErrorCode::InternalError,
                    "Unknown request type",
                )
            }
        }
    }

    /// Handle a subscribe request
    fn handle_subscribe(&self, request: &ControlRequest) -> ControlResponse {
        let message_id = request.message_id;
        let symbol = request.symbol_str();
        let exchange = request.exchange_str();

        info!("Subscribe request for {}@{}", symbol, exchange);

        // Check if symbol is subscribed on the provider
        if !self.state.is_subscribed(symbol) {
            warn!(
                "Symbol {} not subscribed. Available symbols: {:?}",
                symbol, self.state.subscribed_symbols
            );
            return ControlResponse::error(
                message_id,
                ErrorCode::NotSubscribed,
                &format!(
                    "Symbol '{}' not subscribed on provider. Restart data-manager with --symbols {}",
                    symbol, symbol
                ),
            );
        }

        // Check if IPC channel already exists
        if self.state.has_ipc_channel(symbol) {
            debug!("IPC channel already exists for {}", symbol);
            return ControlResponse::success(message_id);
        }

        // Create IPC channel for this symbol
        let channel_exchange = if exchange.is_empty() {
            &self.state.exchange
        } else {
            exchange
        };

        match self.transport.ensure_channel(symbol, channel_exchange) {
            Ok(created) => {
                if created {
                    info!("Created IPC channel for {}@{}", symbol, channel_exchange);
                }
                // Add to IPC symbol set
                self.state.add_ipc_symbol(symbol);
                ControlResponse::success(message_id)
            }
            Err(e) => {
                error!("Failed to create IPC channel for {}: {}", symbol, e);
                ControlResponse::error(
                    message_id,
                    ErrorCode::InternalError,
                    &format!("Failed to create IPC channel: {}", e),
                )
            }
        }
    }

    /// Handle an unsubscribe request
    fn handle_unsubscribe(&self, request: &ControlRequest) -> ControlResponse {
        let message_id = request.message_id;
        let symbol = request.symbol_str();

        info!("Unsubscribe request for {}", symbol);

        // Remove from IPC symbol set (channel cleanup is optional)
        if self.state.has_ipc_channel(symbol) {
            self.state.ipc_symbols.write().remove(symbol);
            info!("Removed {} from IPC streaming", symbol);
        }

        ControlResponse::success(message_id)
    }
}
