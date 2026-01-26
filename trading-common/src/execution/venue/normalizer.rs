//! Execution normalizer trait for venue implementations.
//!
//! This module provides a trait for normalizing venue-specific execution reports
//! to venue-agnostic types.

use crate::execution::venue::error::VenueResult;
use crate::execution::venue::types::ExecutionReport;
use crate::orders::OrderEventAny;

/// Trait for normalizing venue-specific execution reports.
///
/// Each venue implements this trait to convert its raw WebSocket messages
/// into standardized `ExecutionReport` and `OrderEventAny` types.
///
/// # Example
///
/// ```ignore
/// struct MyVenueNormalizer;
///
/// impl ExecutionNormalizer for MyVenueNormalizer {
///     type RawReport = MyVenueExecutionReport;
///
///     fn normalize(&self, raw: &Self::RawReport) -> VenueResult<ExecutionReport> {
///         Ok(ExecutionReport {
///             client_order_id: raw.client_id.into(),
///             // ... map other fields
///         })
///     }
///
///     fn to_order_event(&self, report: &ExecutionReport) -> Option<OrderEventAny> {
///         match report.status {
///             OrderStatus::Filled => Some(OrderEventAny::Filled(...)),
///             // ... handle other statuses
///         }
///     }
///
///     fn parse_message(&self, json: &str) -> VenueResult<Option<ExecutionReport>> {
///         let raw: Self::RawReport = serde_json::from_str(json)?;
///         self.normalize(&raw).map(Some)
///     }
/// }
/// ```
pub trait ExecutionNormalizer: Send + Sync {
    /// The raw execution report type from the venue.
    type RawReport;

    /// Normalize a raw execution report to the venue-agnostic format.
    fn normalize(&self, raw: &Self::RawReport) -> VenueResult<ExecutionReport>;

    /// Convert an execution report to an order event.
    ///
    /// Returns None if the report doesn't warrant an event (e.g., duplicate).
    fn to_order_event(&self, report: &ExecutionReport) -> Option<OrderEventAny>;

    /// Parse a JSON message and normalize to an execution report.
    ///
    /// Returns None if the message is not an execution report (e.g., account update).
    fn parse_message(&self, json: &str) -> VenueResult<Option<ExecutionReport>>;
}

/// Extension methods for ExecutionReport.
impl ExecutionReport {
    /// Check if this report represents a new fill that should be processed.
    pub fn has_new_fill(&self) -> bool {
        self.last_qty
            .map(|qty| qty > rust_decimal::Decimal::ZERO)
            .unwrap_or(false)
    }
}
