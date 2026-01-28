//! Execution venue traits for order management and account queries.
//!
//! This module defines the traits for venues that provide order execution:
//!
//! - [`OrderSubmissionVenue`]: Order lifecycle management via REST API
//! - [`ExecutionStreamVenue`]: Real-time execution reports via WebSocket
//! - [`AccountQueryVenue`]: Account balance and position queries
//! - [`BatchOrderVenue`]: Batch order operations
//! - [`FullExecutionVenue`]: Combined trait for full-featured venues
//!
//! # Example
//!
//! ```ignore
//! use trading_common::venue::execution::{OrderSubmissionVenue, AccountQueryVenue};
//!
//! async fn trade<V: OrderSubmissionVenue + AccountQueryVenue>(
//!     venue: &V,
//!     order: &Order,
//! ) -> VenueResult<VenueOrderId> {
//!     // Check balance first
//!     let balance = venue.query_balance("USDT").await?;
//!     if balance.free < required_amount {
//!         return Err(VenueError::InsufficientBalance("Not enough USDT".into()));
//!     }
//!
//!     // Submit the order
//!     venue.submit_order(order).await
//! }
//! ```

mod normalizer;
mod traits;
mod types;

pub use normalizer::ExecutionNormalizer;
pub use traits::{
    AccountQueryVenue, BatchOrderVenue, ExecutionCallback, ExecutionStreamVenue,
    FullExecutionVenue, OrderSubmissionVenue,
};
pub use types::{
    BalanceInfo, BatchCancelResult, BatchOrderResult, CancelRequest, ExecutionReport,
    OrderQueryResponse,
};
