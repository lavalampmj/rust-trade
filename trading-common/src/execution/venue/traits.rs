//! Execution venue trait definitions.
//!
//! This module defines the trait hierarchy for execution venues:
//!
//! - [`ExecutionVenue`]: Base trait for all venues
//! - [`OrderSubmissionVenue`]: Trait for submitting orders via REST API
//! - [`ExecutionStreamVenue`]: Trait for receiving execution reports via WebSocket
//! - [`AccountQueryVenue`]: Trait for querying account balances
//! - [`FullExecutionVenue`]: Combined trait for full-featured venues
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::{ExecutionVenue, OrderSubmissionVenue};
//!
//! async fn submit_order<V: OrderSubmissionVenue>(venue: &V, order: &Order) {
//!     match venue.submit_order(order).await {
//!         Ok(venue_order_id) => println!("Order submitted: {}", venue_order_id),
//!         Err(e) => println!("Order failed: {}", e),
//!     }
//! }
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::broadcast;

use crate::orders::{ClientOrderId, Order, OrderEventAny, VenueOrderId};

use super::error::VenueResult;
use super::types::{
    BalanceInfo, BatchCancelResult, BatchOrderResult, CancelRequest, OrderQueryResponse,
    VenueConnectionStatus, VenueInfo,
};

/// Callback for execution reports from WebSocket streams.
///
/// This callback is invoked for each execution report received from the venue.
/// The callback should be lightweight and non-blocking - heavy processing
/// should be delegated to a separate task.
pub type ExecutionCallback = Arc<dyn Fn(OrderEventAny) + Send + Sync>;

/// Base trait for all execution venues.
///
/// This trait provides the common interface for connecting to and
/// managing the lifecycle of an execution venue connection.
#[async_trait]
pub trait ExecutionVenue: Send + Sync {
    /// Returns information about this venue's capabilities.
    fn info(&self) -> &VenueInfo;

    /// Connect to the venue.
    ///
    /// This establishes the initial connection (e.g., validates API credentials,
    /// creates HTTP client, etc.). WebSocket streams are started separately
    /// via [`ExecutionStreamVenue::start_execution_stream`].
    async fn connect(&mut self) -> VenueResult<()>;

    /// Disconnect from the venue.
    ///
    /// This closes all connections and releases resources.
    async fn disconnect(&mut self) -> VenueResult<()>;

    /// Returns true if the venue is connected and ready for operations.
    fn is_connected(&self) -> bool;

    /// Returns the current connection status.
    fn connection_status(&self) -> VenueConnectionStatus;
}

/// Trait for submitting and managing orders via REST API.
///
/// This trait provides methods for order lifecycle management
/// through synchronous REST API calls.
#[async_trait]
pub trait OrderSubmissionVenue: ExecutionVenue {
    /// Submit a new order to the venue.
    ///
    /// # Arguments
    ///
    /// * `order` - The order to submit
    ///
    /// # Returns
    ///
    /// The venue-assigned order ID on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the order is rejected or fails to submit.
    async fn submit_order(&self, order: &Order) -> VenueResult<VenueOrderId>;

    /// Cancel an existing order.
    ///
    /// # Arguments
    ///
    /// * `client_order_id` - The client-assigned order ID
    /// * `venue_order_id` - The venue-assigned order ID (optional, but recommended)
    /// * `symbol` - The trading symbol
    ///
    /// # Errors
    ///
    /// Returns an error if the order cannot be cancelled.
    async fn cancel_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
    ) -> VenueResult<()>;

    /// Modify an existing order.
    ///
    /// Not all venues support order modification. Check [`VenueInfo::supports_modify`]
    /// before calling this method.
    ///
    /// # Arguments
    ///
    /// * `client_order_id` - The client-assigned order ID
    /// * `venue_order_id` - The venue-assigned order ID (optional)
    /// * `symbol` - The trading symbol
    /// * `new_price` - New price (None to keep current)
    /// * `new_quantity` - New quantity (None to keep current)
    ///
    /// # Returns
    ///
    /// The (possibly new) venue-assigned order ID on success.
    ///
    /// # Errors
    ///
    /// Returns an error if modification is not supported or fails.
    async fn modify_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> VenueResult<VenueOrderId>;

    /// Query the current status of an order.
    ///
    /// # Arguments
    ///
    /// * `client_order_id` - The client-assigned order ID
    /// * `venue_order_id` - The venue-assigned order ID (optional)
    /// * `symbol` - The trading symbol
    ///
    /// # Returns
    ///
    /// The current order status.
    async fn query_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
    ) -> VenueResult<OrderQueryResponse>;

    /// Query all open orders.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Filter by symbol (None for all symbols)
    ///
    /// # Returns
    ///
    /// List of all open orders.
    async fn query_open_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<OrderQueryResponse>>;

    /// Cancel all open orders.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Filter by symbol (None for all symbols)
    ///
    /// # Returns
    ///
    /// List of cancelled order IDs.
    async fn cancel_all_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<ClientOrderId>>;
}

/// Trait for batch order operations.
///
/// This trait extends [`OrderSubmissionVenue`] with batch operations
/// for venues that support them.
#[async_trait]
pub trait BatchOrderVenue: OrderSubmissionVenue {
    /// Submit multiple orders in a single request.
    ///
    /// # Arguments
    ///
    /// * `orders` - The orders to submit
    ///
    /// # Returns
    ///
    /// Results for each order in the batch.
    async fn submit_orders_batch(&self, orders: &[Order]) -> VenueResult<Vec<BatchOrderResult>>;

    /// Cancel multiple orders in a single request.
    ///
    /// # Arguments
    ///
    /// * `cancels` - The cancel requests
    ///
    /// # Returns
    ///
    /// Results for each cancel in the batch.
    async fn cancel_orders_batch(
        &self,
        cancels: &[CancelRequest],
    ) -> VenueResult<Vec<BatchCancelResult>>;
}

/// Trait for receiving execution reports via WebSocket.
///
/// This trait provides methods for managing the WebSocket connection
/// that receives real-time execution reports.
#[async_trait]
pub trait ExecutionStreamVenue: ExecutionVenue {
    /// Start the execution report stream.
    ///
    /// This connects to the venue's WebSocket and begins receiving
    /// execution reports. Reports are delivered via the callback.
    ///
    /// # Arguments
    ///
    /// * `callback` - Called for each execution report received
    /// * `shutdown_rx` - Receiver for shutdown signal
    ///
    /// # Notes
    ///
    /// This method runs until a shutdown signal is received or an
    /// unrecoverable error occurs. It should be spawned as a task.
    async fn start_execution_stream(
        &mut self,
        callback: ExecutionCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()>;

    /// Stop the execution report stream.
    ///
    /// This gracefully closes the WebSocket connection.
    async fn stop_execution_stream(&mut self) -> VenueResult<()>;

    /// Returns true if the stream is currently active.
    fn is_stream_active(&self) -> bool;
}

/// Trait for querying account information.
#[async_trait]
pub trait AccountQueryVenue: ExecutionVenue {
    /// Query all account balances.
    ///
    /// # Returns
    ///
    /// List of all asset balances.
    async fn query_balances(&self) -> VenueResult<Vec<BalanceInfo>>;

    /// Query the balance of a specific asset.
    ///
    /// # Arguments
    ///
    /// * `asset` - The asset symbol (e.g., "BTC", "USDT")
    ///
    /// # Returns
    ///
    /// The balance information for the asset.
    async fn query_balance(&self, asset: &str) -> VenueResult<BalanceInfo>;
}

/// Combined trait for full-featured execution venues.
///
/// Venues that implement all of the execution traits can be used
/// interchangeably through this trait.
pub trait FullExecutionVenue:
    OrderSubmissionVenue + ExecutionStreamVenue + AccountQueryVenue
{
}

// Auto-implement FullExecutionVenue for types implementing all required traits
impl<T> FullExecutionVenue for T where
    T: OrderSubmissionVenue + ExecutionStreamVenue + AccountQueryVenue
{
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time check that traits have correct bounds
    fn _assert_send_sync<T: Send + Sync>() {}

    fn _check_trait_bounds() {
        _assert_send_sync::<Box<dyn ExecutionVenue>>();
        _assert_send_sync::<Box<dyn OrderSubmissionVenue>>();
        _assert_send_sync::<Box<dyn ExecutionStreamVenue>>();
        _assert_send_sync::<Box<dyn AccountQueryVenue>>();
    }
}
