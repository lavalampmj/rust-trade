//! VenueRouter implementation for multi-venue order management.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock as SyncRwLock;
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::traits::{
    AccountQueryVenue, ExecutionCallback, ExecutionStreamVenue, ExecutionVenue,
    FullExecutionVenue, OrderSubmissionVenue,
};
use crate::execution::venue::types::{
    BalanceInfo, OrderQueryResponse, VenueConnectionStatus, VenueInfo,
};
use crate::orders::{ClientOrderId, Order, OrderEventAny, OrderType, TimeInForce, VenueOrderId};
use rust_decimal::Decimal;

use super::config::VenueRouterConfig;
use super::symbol_registry::SymbolRegistry;

/// Unique identifier for a venue within the router.
pub type VenueId = String;

/// Information about an order's venue routing.
#[derive(Debug, Clone)]
struct OrderRouting {
    /// The venue this order was sent to
    venue_id: VenueId,
    /// The venue-assigned order ID
    venue_order_id: VenueOrderId,
    /// The symbol as sent to the venue
    venue_symbol: String,
}

/// VenueRouter manages multiple execution venues and routes orders based on
/// the instrument's venue specification.
pub struct VenueRouter {
    /// Configuration
    config: VenueRouterConfig,

    /// Registered venues (using tokio RwLock for async safety)
    venues: RwLock<HashMap<VenueId, Box<dyn FullExecutionVenue>>>,

    /// Order routing information: ClientOrderId -> routing details
    order_routing: SyncRwLock<HashMap<ClientOrderId, OrderRouting>>,

    /// Symbol registry for cross-venue symbol mapping
    symbol_registry: SyncRwLock<SymbolRegistry>,

    /// Aggregated venue info (combined capabilities)
    aggregated_info: SyncRwLock<VenueInfo>,

    /// Whether the router is connected (at least one venue connected)
    connected: AtomicBool,

    /// Whether execution streams are active
    streams_active: AtomicBool,

    /// Shutdown signal for streams
    shutdown_tx: SyncRwLock<Option<broadcast::Sender<()>>>,
}

impl VenueRouter {
    /// Create a new VenueRouter with the given configuration.
    pub fn new(config: VenueRouterConfig) -> Self {
        Self {
            config,
            venues: RwLock::new(HashMap::new()),
            order_routing: SyncRwLock::new(HashMap::new()),
            symbol_registry: SyncRwLock::new(SymbolRegistry::with_crypto_defaults()),
            aggregated_info: SyncRwLock::new(Self::default_info()),
            connected: AtomicBool::new(false),
            streams_active: AtomicBool::new(false),
            shutdown_tx: SyncRwLock::new(None),
        }
    }

    /// Create a new VenueRouter with default configuration.
    pub fn with_default() -> Self {
        Self::new(VenueRouterConfig::default())
    }

    fn default_info() -> VenueInfo {
        VenueInfo::new("venue_router", "Venue Router")
            .with_name("ROUTER")
            .with_order_types(vec![OrderType::Market, OrderType::Limit])
            .with_tif(vec![TimeInForce::GTC, TimeInForce::IOC, TimeInForce::FOK])
    }

    /// Register a venue with the router.
    pub async fn register_venue(
        &self,
        venue_id: impl Into<VenueId>,
        venue: impl FullExecutionVenue + 'static,
    ) {
        let venue_id = venue_id.into();
        info!("Registering venue: {}", venue_id);

        let boxed: Box<dyn FullExecutionVenue> = Box::new(venue);
        self.venues.write().await.insert(venue_id.clone(), boxed);

        // Update aggregated info
        self.update_aggregated_info().await;
    }

    /// Register a boxed venue (for dynamic registration).
    pub async fn register_venue_boxed(
        &self,
        venue_id: impl Into<VenueId>,
        venue: Box<dyn FullExecutionVenue>,
    ) {
        let venue_id = venue_id.into();
        info!("Registering venue (boxed): {}", venue_id);

        self.venues.write().await.insert(venue_id, venue);
        self.update_aggregated_info().await;
    }

    /// Unregister a venue from the router.
    pub async fn unregister_venue(&self, venue_id: &str) -> Option<Box<dyn FullExecutionVenue>> {
        let venue = self.venues.write().await.remove(venue_id);
        if venue.is_some() {
            info!("Unregistered venue: {}", venue_id);
            self.update_aggregated_info().await;
        }
        venue
    }

    /// Get the list of registered venue IDs.
    pub async fn venue_ids(&self) -> Vec<VenueId> {
        self.venues.read().await.keys().cloned().collect()
    }

    /// Check if a venue is registered.
    pub async fn has_venue(&self, venue_id: &str) -> bool {
        self.venues.read().await.contains_key(venue_id)
    }

    /// Get the number of registered venues.
    pub async fn venue_count(&self) -> usize {
        self.venues.read().await.len()
    }

    /// Get the symbol registry for configuration.
    pub fn symbol_registry(&self) -> &SyncRwLock<SymbolRegistry> {
        &self.symbol_registry
    }

    /// Set the symbol registry.
    pub fn set_symbol_registry(&self, registry: SymbolRegistry) {
        *self.symbol_registry.write() = registry;
    }

    /// Get connection status for all venues.
    pub async fn venue_statuses(&self) -> HashMap<VenueId, VenueConnectionStatus> {
        self.venues
            .read()
            .await
            .iter()
            .map(|(id, venue)| (id.clone(), venue.connection_status()))
            .collect()
    }

    /// Get list of connected venues.
    pub async fn connected_venues(&self) -> Vec<VenueId> {
        self.venues
            .read()
            .await
            .iter()
            .filter(|(_, venue)| venue.is_connected())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get list of disconnected venues.
    pub async fn disconnected_venues(&self) -> Vec<VenueId> {
        self.venues
            .read()
            .await
            .iter()
            .filter(|(_, venue)| !venue.is_connected())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Update aggregated venue info based on registered venues.
    async fn update_aggregated_info(&self) {
        let venues = self.venues.read().await;

        let mut order_types = Vec::new();
        let mut tif = Vec::new();
        let mut supports_modify = false;
        let mut supports_batch = false;
        let mut supports_stop = false;
        let mut max_batch = 0usize;

        for venue in venues.values() {
            let info = venue.info();

            for ot in &info.supported_order_types {
                if !order_types.contains(ot) {
                    order_types.push(*ot);
                }
            }

            for t in &info.supported_tif {
                if !tif.contains(t) {
                    tif.push(*t);
                }
            }

            supports_modify |= info.supports_modify;
            supports_batch |= info.supports_batch;
            supports_stop |= info.supports_stop_orders;
            max_batch = max_batch.max(info.max_orders_per_batch);
        }

        let mut info = VenueInfo::new("venue_router", "Venue Router")
            .with_name("ROUTER")
            .with_order_types(order_types)
            .with_tif(tif);

        if supports_stop {
            info = info.with_stop_orders();
        }
        if supports_modify {
            info = info.with_modify_support();
        }
        if supports_batch {
            info = info.with_batch_support(max_batch);
        }

        *self.aggregated_info.write() = info;
    }

    /// Resolve the venue for an order based on instrument_id.venue.
    async fn resolve_venue(&self, order: &Order) -> VenueResult<VenueId> {
        let venue_id = &order.instrument_id.venue;

        // If venue is "DEFAULT", use default from config
        if venue_id == "DEFAULT" {
            if let Some(ref default) = self.config.default_venue {
                if self.has_venue(default).await {
                    return Ok(default.clone());
                }
                return Err(VenueError::Configuration(format!(
                    "Default venue '{}' not registered",
                    default
                )));
            }
            return Err(VenueError::Configuration(
                "No default venue configured and order has no venue specified".to_string(),
            ));
        }

        // Check if venue is registered
        if self.has_venue(venue_id).await {
            return Ok(venue_id.clone());
        }

        // Check for route rules
        let symbol = &order.instrument_id.symbol;
        if let Some(rule) = self.config.route_rules.get(symbol) {
            if self.has_venue(&rule.primary_venue).await {
                return Ok(rule.primary_venue.clone());
            }
            for fallback in &rule.fallback_venues {
                if self.has_venue(fallback).await {
                    warn!(
                        "Primary venue '{}' unavailable, using fallback '{}'",
                        rule.primary_venue, fallback
                    );
                    return Ok(fallback.clone());
                }
            }
        }

        Err(VenueError::Configuration(format!(
            "Venue '{}' not registered",
            venue_id
        )))
    }

    /// Get routing info for an order.
    pub fn get_order_routing(
        &self,
        client_order_id: &ClientOrderId,
    ) -> Option<(VenueId, VenueOrderId)> {
        self.order_routing
            .read()
            .get(client_order_id)
            .map(|r| (r.venue_id.clone(), r.venue_order_id.clone()))
    }

    /// Get the venue ID for an existing order.
    pub fn get_order_venue(&self, client_order_id: &ClientOrderId) -> Option<VenueId> {
        self.order_routing
            .read()
            .get(client_order_id)
            .map(|r| r.venue_id.clone())
    }

    /// Get a cloned copy of the aggregated info.
    pub fn aggregated_info(&self) -> VenueInfo {
        self.aggregated_info.read().clone()
    }
}

#[async_trait]
impl ExecutionVenue for VenueRouter {
    fn info(&self) -> &VenueInfo {
        // Note: This is tricky with RwLock. We store a static reference.
        // In practice, the aggregated info rarely changes after initial setup.
        // For a truly correct implementation, we'd need interior mutability patterns.
        // For now, we leak a reference which is valid for the lifetime of the router.
        let info = self.aggregated_info.read().clone();
        Box::leak(Box::new(info))
    }

    async fn connect(&mut self) -> VenueResult<()> {
        info!("Connecting to all registered venues...");

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        if venue_ids.is_empty() {
            return Err(VenueError::Configuration("No venues registered".to_string()));
        }

        let mut connected_count = 0;
        let mut errors = Vec::new();

        for venue_id in venue_ids {
            let result = {
                let mut venues = self.venues.write().await;
                if let Some(venue) = venues.get_mut(&venue_id) {
                    match tokio::time::timeout(
                        self.config.connect_timeout_duration(),
                        venue.connect(),
                    )
                    .await
                    {
                        Ok(Ok(())) => {
                            info!("Connected to venue: {}", venue_id);
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            error!("Failed to connect to venue {}: {:?}", venue_id, e);
                            Err(e)
                        }
                        Err(_) => {
                            error!("Timeout connecting to venue: {}", venue_id);
                            Err(VenueError::Timeout(format!(
                                "Connection timeout for {}",
                                venue_id
                            )))
                        }
                    }
                } else {
                    Err(VenueError::Internal(format!(
                        "Venue {} disappeared during connect",
                        venue_id
                    )))
                }
            };

            match result {
                Ok(()) => connected_count += 1,
                Err(e) => errors.push((venue_id, e)),
            }
        }

        if self.config.require_all_connected && !errors.is_empty() {
            return Err(VenueError::Connection(format!(
                "Failed to connect to all venues: {:?}",
                errors
            )));
        }

        if connected_count == 0 {
            return Err(VenueError::Connection(
                "Failed to connect to any venue".to_string(),
            ));
        }

        self.connected.store(true, Ordering::SeqCst);
        info!(
            "VenueRouter connected: {}/{} venues",
            connected_count,
            connected_count + errors.len()
        );

        Ok(())
    }

    async fn disconnect(&mut self) -> VenueResult<()> {
        info!("Disconnecting from all venues...");

        // Stop streams first
        if self.streams_active.load(Ordering::SeqCst) {
            if let Some(ref tx) = *self.shutdown_tx.read() {
                let _ = tx.send(());
            }
            self.streams_active.store(false, Ordering::SeqCst);
        }

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for venue_id in venue_ids {
            let mut venues = self.venues.write().await;
            if let Some(venue) = venues.get_mut(&venue_id) {
                if let Err(e) = venue.disconnect().await {
                    warn!("Error disconnecting venue {}: {:?}", venue_id, e);
                }
            }
        }

        self.connected.store(false, Ordering::SeqCst);
        info!("VenueRouter disconnected");

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn connection_status(&self) -> VenueConnectionStatus {
        if self.connected.load(Ordering::SeqCst) {
            VenueConnectionStatus::Connected
        } else {
            VenueConnectionStatus::Disconnected
        }
    }
}

#[async_trait]
impl OrderSubmissionVenue for VenueRouter {
    async fn submit_order(&self, order: &Order) -> VenueResult<VenueOrderId> {
        let venue_id = self.resolve_venue(order).await?;

        debug!(
            "Routing order {} to venue {}",
            order.client_order_id, venue_id
        );

        // Get venue, check connected, and submit
        let venue_order_id = {
            let venues = self.venues.read().await;
            let venue = venues
                .get(&venue_id)
                .ok_or_else(|| VenueError::Internal(format!("Venue {} not found", venue_id)))?;

            if !venue.is_connected() {
                return Err(VenueError::NotConnected);
            }

            venue.submit_order(order).await?
        };

        // Store routing information
        self.order_routing.write().insert(
            order.client_order_id.clone(),
            OrderRouting {
                venue_id: venue_id.clone(),
                venue_order_id: venue_order_id.clone(),
                venue_symbol: order.instrument_id.symbol.clone(),
            },
        );

        info!(
            "Order {} submitted to {} -> {}",
            order.client_order_id, venue_id, venue_order_id
        );

        Ok(venue_order_id)
    }

    async fn cancel_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
    ) -> VenueResult<()> {
        // Look up routing info
        let routing = self.order_routing.read().get(client_order_id).cloned();

        let (venue_id, actual_venue_order_id) = if let Some(routing) = routing {
            (
                routing.venue_id,
                venue_order_id
                    .cloned()
                    .unwrap_or(routing.venue_order_id),
            )
        } else {
            // No routing info - need venue_order_id
            let vid = venue_order_id
                .ok_or_else(|| {
                    VenueError::OrderNotFound(format!(
                        "No routing info for order {} and no venue_order_id provided",
                        client_order_id
                    ))
                })?
                .clone();

            // Try to find which venue has this order by searching
            let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();
            let mut found_venue = None;

            for vid_check in venue_ids {
                let venues = self.venues.read().await;
                if let Some(venue) = venues.get(&vid_check) {
                    if venue.is_connected() {
                        // Try query - if it succeeds, this venue has the order
                        if venue
                            .query_order(client_order_id, Some(&vid), symbol)
                            .await
                            .is_ok()
                        {
                            found_venue = Some(vid_check);
                            break;
                        }
                    }
                }
            }

            let venue_id = found_venue.ok_or_else(|| {
                VenueError::OrderNotFound(format!(
                    "Order {} not found on any venue",
                    client_order_id
                ))
            })?;

            (venue_id, vid)
        };

        debug!("Canceling order {} on venue {}", client_order_id, venue_id);

        let result = {
            let venues = self.venues.read().await;
            let venue = venues
                .get(&venue_id)
                .ok_or_else(|| VenueError::Internal(format!("Venue {} not found", venue_id)))?;

            venue
                .cancel_order(client_order_id, Some(&actual_venue_order_id), symbol)
                .await
        };

        if result.is_ok() {
            self.order_routing.write().remove(client_order_id);
            info!("Order {} cancelled on venue {}", client_order_id, venue_id);
        }

        result
    }

    async fn modify_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> VenueResult<VenueOrderId> {
        let routing = self
            .order_routing
            .read()
            .get(client_order_id)
            .cloned()
            .ok_or_else(|| {
                VenueError::OrderNotFound(format!("No routing info for order {}", client_order_id))
            })?;

        let actual_venue_order_id = venue_order_id
            .cloned()
            .unwrap_or(routing.venue_order_id.clone());

        debug!(
            "Modifying order {} on venue {}",
            client_order_id, routing.venue_id
        );

        let new_venue_order_id = {
            let venues = self.venues.read().await;
            let venue = venues.get(&routing.venue_id).ok_or_else(|| {
                VenueError::Internal(format!("Venue {} not found", routing.venue_id))
            })?;

            venue
                .modify_order(
                    client_order_id,
                    Some(&actual_venue_order_id),
                    symbol,
                    new_price,
                    new_quantity,
                )
                .await?
        };

        // Update routing with new venue order ID
        self.order_routing.write().insert(
            client_order_id.clone(),
            OrderRouting {
                venue_id: routing.venue_id.clone(),
                venue_order_id: new_venue_order_id.clone(),
                venue_symbol: routing.venue_symbol,
            },
        );

        info!(
            "Order {} modified on {} -> {}",
            client_order_id, routing.venue_id, new_venue_order_id
        );

        Ok(new_venue_order_id)
    }

    async fn query_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
    ) -> VenueResult<OrderQueryResponse> {
        let routing = self.order_routing.read().get(client_order_id).cloned();

        if let Some(routing) = routing {
            let actual_venue_order_id = venue_order_id
                .cloned()
                .unwrap_or(routing.venue_order_id);

            let venues = self.venues.read().await;
            let venue = venues.get(&routing.venue_id).ok_or_else(|| {
                VenueError::Internal(format!("Venue {} not found", routing.venue_id))
            })?;

            return venue
                .query_order(client_order_id, Some(&actual_venue_order_id), symbol)
                .await;
        }

        // No routing info - search all venues
        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for vid in venue_ids {
            let venues = self.venues.read().await;
            if let Some(venue) = venues.get(&vid) {
                if venue.is_connected() {
                    if let Ok(response) = venue
                        .query_order(client_order_id, venue_order_id, symbol)
                        .await
                    {
                        return Ok(response);
                    }
                }
            }
        }

        Err(VenueError::OrderNotFound(format!(
            "Order {} not found on any venue",
            client_order_id
        )))
    }

    async fn query_open_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<OrderQueryResponse>> {
        let mut all_orders = Vec::new();

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for venue_id in venue_ids {
            let venues = self.venues.read().await;
            if let Some(venue) = venues.get(&venue_id) {
                if venue.is_connected() {
                    match venue.query_open_orders(symbol).await {
                        Ok(orders) => {
                            debug!("Got {} orders from venue {}", orders.len(), venue_id);
                            all_orders.extend(orders);
                        }
                        Err(e) => {
                            warn!("Failed to query orders from venue {}: {:?}", venue_id, e);
                        }
                    }
                }
            }
        }

        Ok(all_orders)
    }

    async fn cancel_all_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<ClientOrderId>> {
        let mut cancelled = Vec::new();

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for venue_id in venue_ids {
            let venues = self.venues.read().await;
            if let Some(venue) = venues.get(&venue_id) {
                if venue.is_connected() {
                    match venue.cancel_all_orders(symbol).await {
                        Ok(ids) => {
                            debug!("Cancelled {} orders on venue {}", ids.len(), venue_id);
                            cancelled.extend(ids);
                        }
                        Err(e) => {
                            warn!("Failed to cancel orders on venue {}: {:?}", venue_id, e);
                        }
                    }
                }
            }
        }

        // Clean up routing for cancelled orders
        {
            let mut routing = self.order_routing.write();
            for id in &cancelled {
                routing.remove(id);
            }
        }

        info!("Cancelled {} orders across all venues", cancelled.len());
        Ok(cancelled)
    }
}

#[async_trait]
impl ExecutionStreamVenue for VenueRouter {
    async fn start_execution_stream(
        &mut self,
        callback: ExecutionCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        if !self.config.aggregate_execution_streams {
            return Ok(());
        }

        info!("Starting aggregated execution streams...");

        // Create internal shutdown channel
        let (internal_shutdown_tx, _) = broadcast::channel::<()>(1);
        *self.shutdown_tx.write() = Some(internal_shutdown_tx.clone());

        // Get venue IDs first
        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();
        let callback = Arc::new(callback);

        for venue_id in venue_ids {
            let mut venues = self.venues.write().await;
            if let Some(venue) = venues.get_mut(&venue_id) {
                if venue.is_connected() {
                    let callback_clone = callback.clone();
                    let venue_id_clone = venue_id.clone();
                    let internal_rx = internal_shutdown_tx.subscribe();

                    // Wrap callback to add venue context
                    let wrapped_callback: ExecutionCallback =
                        Arc::new(move |event: OrderEventAny| {
                            debug!("Execution event from venue {}: {:?}", venue_id_clone, event);
                            callback_clone(event);
                        });

                    match venue
                        .start_execution_stream(wrapped_callback, internal_rx)
                        .await
                    {
                        Ok(()) => {
                            info!("Started execution stream for venue {}", venue_id);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to start execution stream for venue {}: {:?}",
                                venue_id, e
                            );
                        }
                    }
                }
            }
        }

        self.streams_active.store(true, Ordering::SeqCst);

        // Spawn task to listen for external shutdown
        let shutdown_tx_clone = internal_shutdown_tx.clone();
        tokio::spawn(async move {
            let mut rx = shutdown_rx;
            let _ = rx.recv().await;
            // Note: We can't update streams_active from here since we don't own it
            // The stop_execution_stream method will handle cleanup
            let _ = shutdown_tx_clone.send(());
        });

        Ok(())
    }

    async fn stop_execution_stream(&mut self) -> VenueResult<()> {
        info!("Stopping aggregated execution streams...");

        if let Some(ref tx) = *self.shutdown_tx.read() {
            let _ = tx.send(());
        }

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for venue_id in venue_ids {
            let mut venues = self.venues.write().await;
            if let Some(venue) = venues.get_mut(&venue_id) {
                if let Err(e) = venue.stop_execution_stream().await {
                    warn!(
                        "Error stopping execution stream for venue {}: {:?}",
                        venue_id, e
                    );
                }
            }
        }

        self.streams_active.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_stream_active(&self) -> bool {
        self.streams_active.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AccountQueryVenue for VenueRouter {
    async fn query_balances(&self) -> VenueResult<Vec<BalanceInfo>> {
        let mut all_balances = Vec::new();

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for venue_id in venue_ids {
            let venues = self.venues.read().await;
            if let Some(venue) = venues.get(&venue_id) {
                if venue.is_connected() {
                    match venue.query_balances().await {
                        Ok(balances) => {
                            // Add venue prefix to distinguish balances
                            for mut balance in balances {
                                balance.asset = format!("{}:{}", venue_id, balance.asset);
                                all_balances.push(balance);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to query balances from venue {}: {:?}", venue_id, e);
                        }
                    }
                }
            }
        }

        Ok(all_balances)
    }

    async fn query_balance(&self, asset: &str) -> VenueResult<BalanceInfo> {
        // Check if asset has venue prefix
        if let Some((venue_id, actual_asset)) = asset.split_once(':') {
            let venues = self.venues.read().await;
            if let Some(venue) = venues.get(venue_id) {
                return venue.query_balance(actual_asset).await;
            }
            return Err(VenueError::Configuration(format!(
                "Venue {} not found",
                venue_id
            )));
        }

        // No prefix - try all venues and sum
        let mut total_free = Decimal::ZERO;
        let mut total_locked = Decimal::ZERO;

        let venue_ids: Vec<_> = self.venues.read().await.keys().cloned().collect();

        for venue_id in venue_ids {
            let venues = self.venues.read().await;
            if let Some(venue) = venues.get(&venue_id) {
                if venue.is_connected() {
                    if let Ok(balance) = venue.query_balance(asset).await {
                        total_free += balance.free;
                        total_locked += balance.locked;
                    }
                }
            }
        }

        Ok(BalanceInfo {
            asset: asset.to_string(),
            free: total_free,
            locked: total_locked,
        })
    }
}

// Note: FullExecutionVenue is automatically implemented via blanket impl
// for any type implementing ExecutionVenue + OrderSubmissionVenue +
// ExecutionStreamVenue + AccountQueryVenue

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_creation() {
        let config = VenueRouterConfig::with_default_venue("BINANCE");
        let router = VenueRouter::new(config);

        assert!(!router.is_connected());
    }

    #[tokio::test]
    async fn test_venue_registration() {
        let router = VenueRouter::with_default();
        assert_eq!(router.venue_count().await, 0);
        assert!(router.venue_ids().await.is_empty());
    }
}
