//! Execution venue types.
//!
//! Types for order management and account queries.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::orders::{
    ClientOrderId, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, VenueOrderId,
};

/// Response from an order status query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderQueryResponse {
    /// Client-assigned order ID
    pub client_order_id: ClientOrderId,
    /// Venue-assigned order ID
    pub venue_order_id: VenueOrderId,
    /// Trading symbol
    pub symbol: String,
    /// Order side
    pub side: OrderSide,
    /// Order type
    pub order_type: OrderType,
    /// Current order status
    pub status: OrderStatus,
    /// Limit price (for limit orders)
    pub price: Option<Decimal>,
    /// Original order quantity
    pub quantity: Decimal,
    /// Filled quantity
    pub filled_qty: Decimal,
    /// Average fill price
    pub avg_price: Option<Decimal>,
    /// Time in force
    pub time_in_force: TimeInForce,
    /// When the order was created
    pub created_at: DateTime<Utc>,
    /// When the order was last updated
    pub updated_at: DateTime<Utc>,
}

impl OrderQueryResponse {
    /// Returns the remaining quantity to be filled.
    pub fn leaves_qty(&self) -> Decimal {
        (self.quantity - self.filled_qty).max(Decimal::ZERO)
    }

    /// Returns true if the order is fully filled.
    pub fn is_filled(&self) -> bool {
        self.status == OrderStatus::Filled || self.leaves_qty().is_zero()
    }

    /// Returns true if the order is still open.
    pub fn is_open(&self) -> bool {
        self.status.is_open()
    }
}

/// Balance information for an asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceInfo {
    /// Asset symbol (e.g., "BTC", "USDT")
    pub asset: String,
    /// Available balance (not locked in orders)
    pub free: Decimal,
    /// Locked balance (in open orders)
    pub locked: Decimal,
}

impl BalanceInfo {
    /// Create a new balance info.
    pub fn new(asset: impl Into<String>, free: Decimal, locked: Decimal) -> Self {
        Self {
            asset: asset.into(),
            free,
            locked,
        }
    }

    /// Returns the total balance (free + locked).
    pub fn total(&self) -> Decimal {
        self.free + self.locked
    }

    /// Returns true if the balance has any funds.
    pub fn has_balance(&self) -> bool {
        self.total() > Decimal::ZERO
    }

    /// Returns true if there's available balance for trading.
    pub fn has_free_balance(&self) -> bool {
        self.free > Decimal::ZERO
    }
}

/// Request to cancel an order.
#[derive(Debug, Clone)]
pub struct CancelRequest {
    /// Client-assigned order ID
    pub client_order_id: ClientOrderId,
    /// Venue-assigned order ID (optional, but helps with faster lookups)
    pub venue_order_id: Option<VenueOrderId>,
    /// Trading symbol
    pub symbol: String,
}

impl CancelRequest {
    /// Create a new cancel request.
    pub fn new(client_order_id: ClientOrderId, symbol: impl Into<String>) -> Self {
        Self {
            client_order_id,
            venue_order_id: None,
            symbol: symbol.into(),
        }
    }

    /// Add a venue order ID to the request.
    pub fn with_venue_order_id(mut self, venue_order_id: VenueOrderId) -> Self {
        self.venue_order_id = Some(venue_order_id);
        self
    }
}

/// Result of a batch order submission.
#[derive(Debug, Clone)]
pub struct BatchOrderResult {
    /// Client-assigned order ID
    pub client_order_id: ClientOrderId,
    /// Result of the submission
    pub result: Result<VenueOrderId, String>,
}

impl BatchOrderResult {
    /// Create a successful batch result.
    pub fn success(client_order_id: ClientOrderId, venue_order_id: VenueOrderId) -> Self {
        Self {
            client_order_id,
            result: Ok(venue_order_id),
        }
    }

    /// Create a failed batch result.
    pub fn failure(client_order_id: ClientOrderId, error: impl Into<String>) -> Self {
        Self {
            client_order_id,
            result: Err(error.into()),
        }
    }

    /// Returns true if the order was successfully submitted.
    pub fn is_success(&self) -> bool {
        self.result.is_ok()
    }
}

/// Result of a batch cancel operation.
#[derive(Debug, Clone)]
pub struct BatchCancelResult {
    /// Client-assigned order ID
    pub client_order_id: ClientOrderId,
    /// Whether the cancellation was successful
    pub success: bool,
    /// Error message if cancellation failed
    pub error: Option<String>,
}

impl BatchCancelResult {
    /// Create a successful cancel result.
    pub fn success(client_order_id: ClientOrderId) -> Self {
        Self {
            client_order_id,
            success: true,
            error: None,
        }
    }

    /// Create a failed cancel result.
    pub fn failure(client_order_id: ClientOrderId, error: impl Into<String>) -> Self {
        Self {
            client_order_id,
            success: false,
            error: Some(error.into()),
        }
    }
}

/// Normalized execution report from a venue.
///
/// This is a venue-agnostic representation of an execution report
/// that can be converted to `OrderEventAny`.
#[derive(Debug, Clone)]
pub struct ExecutionReport {
    /// Client-assigned order ID
    pub client_order_id: ClientOrderId,
    /// Venue-assigned order ID
    pub venue_order_id: VenueOrderId,
    /// Trading symbol
    pub symbol: String,
    /// Order side
    pub side: OrderSide,
    /// Order type
    pub order_type: OrderType,
    /// Current order status
    pub status: OrderStatus,
    /// Limit price (for limit orders)
    pub price: Option<Decimal>,
    /// Original order quantity
    pub quantity: Decimal,
    /// Quantity filled in this execution
    pub last_qty: Option<Decimal>,
    /// Price of this execution
    pub last_price: Option<Decimal>,
    /// Cumulative filled quantity
    pub cum_qty: Decimal,
    /// Remaining quantity
    pub leaves_qty: Decimal,
    /// Average fill price
    pub avg_price: Option<Decimal>,
    /// Commission amount
    pub commission: Option<Decimal>,
    /// Commission asset
    pub commission_asset: Option<String>,
    /// Whether this was a maker or taker fill
    pub liquidity_side: LiquiditySide,
    /// Trade ID (if this is a fill)
    pub trade_id: Option<String>,
    /// Time of the event on the exchange
    pub event_time: DateTime<Utc>,
    /// Transaction time
    pub transaction_time: DateTime<Utc>,
    /// Rejection reason (if rejected)
    pub reject_reason: Option<String>,
}

impl ExecutionReport {
    /// Returns true if this is a fill report.
    pub fn is_fill(&self) -> bool {
        self.last_qty
            .map(|qty| qty > Decimal::ZERO)
            .unwrap_or(false)
    }

    /// Returns true if the order is fully filled.
    pub fn is_fully_filled(&self) -> bool {
        self.status == OrderStatus::Filled || self.leaves_qty.is_zero()
    }

    /// Returns true if this is a rejection report.
    pub fn is_rejected(&self) -> bool {
        self.status == OrderStatus::Rejected
    }

    /// Returns true if this is a cancellation report.
    pub fn is_cancelled(&self) -> bool {
        self.status == OrderStatus::Canceled
    }

    /// Returns true if this is an expiration report.
    pub fn is_expired(&self) -> bool {
        self.status == OrderStatus::Expired
    }

    /// Check if this report represents a new fill that should be processed.
    pub fn has_new_fill(&self) -> bool {
        self.last_qty
            .map(|qty| qty > Decimal::ZERO)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_balance_info() {
        let balance = BalanceInfo::new("BTC", dec!(1.5), dec!(0.5));
        assert_eq!(balance.total(), dec!(2.0));
        assert!(balance.has_balance());
        assert!(balance.has_free_balance());

        let empty_balance = BalanceInfo::new("ETH", Decimal::ZERO, Decimal::ZERO);
        assert!(!empty_balance.has_balance());
        assert!(!empty_balance.has_free_balance());
    }

    #[test]
    fn test_batch_results() {
        let success = BatchOrderResult::success(
            ClientOrderId::new("order-1"),
            VenueOrderId::new("venue-1"),
        );
        assert!(success.is_success());

        let failure =
            BatchOrderResult::failure(ClientOrderId::new("order-2"), "Insufficient balance");
        assert!(!failure.is_success());
    }

    #[test]
    fn test_order_query_response() {
        let response = OrderQueryResponse {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: VenueOrderId::new("venue-test"),
            symbol: "BTCUSDT".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            status: OrderStatus::PartiallyFilled,
            price: Some(dec!(50000)),
            quantity: dec!(1.0),
            filled_qty: dec!(0.5),
            avg_price: Some(dec!(49999)),
            time_in_force: TimeInForce::GTC,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        assert_eq!(response.leaves_qty(), dec!(0.5));
        assert!(!response.is_filled());
        assert!(response.is_open());
    }
}
