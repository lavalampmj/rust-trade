//! Execution report normalizer for Binance Futures.
//!
//! Converts Binance Futures-specific types to venue-agnostic types.

use rust_decimal::Decimal;
use tracing::{debug, warn};

use crate::execution::venue::binance::common::timestamp_to_datetime;
use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::types::ExecutionReport;
use crate::orders::{
    ClientOrderId, LiquiditySide, OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired,
    OrderFilled, OrderRejected, OrderStatus, TradeId, VenueOrderId,
};

use super::types::{
    FuturesNewOrderResponse, FuturesOrderInfo, FuturesOrderQueryResponse, FuturesOrderTradeUpdate,
};

/// Normalizer for Binance Futures execution reports.
pub struct FuturesExecutionNormalizer {
    /// Venue name for events
    venue_name: String,
}

impl FuturesExecutionNormalizer {
    /// Create a new normalizer.
    pub fn new(venue_name: impl Into<String>) -> Self {
        Self {
            venue_name: venue_name.into(),
        }
    }

    /// Convert a new order response to an execution report.
    pub fn normalize_new_order(&self, response: &FuturesNewOrderResponse) -> ExecutionReport {
        let leaves_qty = response.orig_qty - response.executed_qty;

        ExecutionReport {
            client_order_id: ClientOrderId::new(&response.client_order_id),
            venue_order_id: VenueOrderId::new(response.order_id.to_string()),
            symbol: response.symbol.clone(),
            side: response.side.to_order_side(),
            order_type: response.order_type.to_order_type(),
            status: response.status.to_order_status(),
            price: if response.price > Decimal::ZERO {
                Some(response.price)
            } else {
                None
            },
            quantity: response.orig_qty,
            last_qty: None,
            last_price: None,
            cum_qty: response.executed_qty,
            leaves_qty,
            avg_price: if response.avg_price > Decimal::ZERO {
                Some(response.avg_price)
            } else {
                None
            },
            commission: None,
            commission_asset: None,
            liquidity_side: LiquiditySide::Taker,
            trade_id: None,
            event_time: timestamp_to_datetime(response.update_time),
            transaction_time: timestamp_to_datetime(response.update_time),
            reject_reason: None,
        }
    }

    /// Convert a WebSocket order trade update to our normalized format.
    pub fn normalize_ws_report(&self, order: &FuturesOrderInfo) -> ExecutionReport {
        let leaves_qty = order.quantity - order.cum_qty;

        ExecutionReport {
            client_order_id: ClientOrderId::new(&order.client_order_id),
            venue_order_id: VenueOrderId::new(order.order_id.to_string()),
            symbol: order.symbol.clone(),
            side: order.side.to_order_side(),
            order_type: order.order_type.to_order_type(),
            status: order.order_status.to_order_status(),
            price: if order.price > Decimal::ZERO {
                Some(order.price)
            } else {
                None
            },
            quantity: order.quantity,
            last_qty: if order.last_qty > Decimal::ZERO {
                Some(order.last_qty)
            } else {
                None
            },
            last_price: if order.last_price > Decimal::ZERO {
                Some(order.last_price)
            } else {
                None
            },
            cum_qty: order.cum_qty,
            leaves_qty,
            avg_price: if order.avg_price > Decimal::ZERO {
                Some(order.avg_price)
            } else {
                None
            },
            commission: order.commission,
            commission_asset: order.commission_asset.clone(),
            liquidity_side: order.liquidity_side().to_liquidity_side(),
            trade_id: if order.trade_id > 0 {
                Some(order.trade_id.to_string())
            } else {
                None
            },
            event_time: timestamp_to_datetime(order.order_trade_time),
            transaction_time: timestamp_to_datetime(order.order_trade_time),
            reject_reason: None,
        }
    }

    /// Convert an execution report to an order event.
    pub fn to_order_event(&self, report: &ExecutionReport) -> Option<OrderEventAny> {
        use crate::orders::{AccountId, InstrumentId, StrategyId};

        match report.status {
            OrderStatus::Accepted => {
                let accepted = OrderAccepted::new(
                    report.client_order_id.clone(),
                    report.venue_order_id.clone(),
                    AccountId::default(),
                );
                Some(OrderEventAny::Accepted(accepted))
            }
            OrderStatus::Filled | OrderStatus::PartiallyFilled => {
                if let (Some(last_qty), Some(last_price)) = (report.last_qty, report.last_price) {
                    if last_qty > Decimal::ZERO {
                        let trade_id = report
                            .trade_id
                            .as_ref()
                            .map(|id| TradeId::new(id.clone()))
                            .unwrap_or_else(TradeId::generate);

                        let filled = OrderFilled::new(
                            report.client_order_id.clone(),
                            report.venue_order_id.clone(),
                            AccountId::default(),
                            InstrumentId::from_symbol(&report.symbol),
                            trade_id,
                            StrategyId::default(),
                            report.side,
                            report.order_type,
                            last_qty,
                            last_price,
                            report.cum_qty,
                            report.leaves_qty,
                            "USDT".to_string(),
                            report.commission.unwrap_or(Decimal::ZERO),
                            report.commission_asset.clone().unwrap_or_default(),
                            report.liquidity_side,
                        );
                        Some(OrderEventAny::Filled(filled))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            OrderStatus::Canceled => {
                let canceled = OrderCanceled::new(
                    report.client_order_id.clone(),
                    Some(report.venue_order_id.clone()),
                    AccountId::default(),
                );
                Some(OrderEventAny::Canceled(canceled))
            }
            OrderStatus::Rejected => {
                let rejected = OrderRejected::new(
                    report.client_order_id.clone(),
                    AccountId::default(),
                    report.reject_reason.clone().unwrap_or_default(),
                );
                Some(OrderEventAny::Rejected(rejected))
            }
            OrderStatus::Expired => {
                let expired = OrderExpired::new(
                    report.client_order_id.clone(),
                    Some(report.venue_order_id.clone()),
                    AccountId::default(),
                );
                Some(OrderEventAny::Expired(expired))
            }
            _ => None,
        }
    }

    /// Convert an order query response to our normalized format.
    pub fn normalize_order_query(
        &self,
        response: &FuturesOrderQueryResponse,
    ) -> crate::execution::venue::types::OrderQueryResponse {
        crate::execution::venue::types::OrderQueryResponse {
            client_order_id: ClientOrderId::new(&response.client_order_id),
            venue_order_id: VenueOrderId::new(response.order_id.to_string()),
            symbol: response.symbol.clone(),
            side: response.side.to_order_side(),
            order_type: response.order_type.to_order_type(),
            status: response.status.to_order_status(),
            price: if response.price > Decimal::ZERO {
                Some(response.price)
            } else {
                None
            },
            quantity: response.orig_qty,
            filled_qty: response.executed_qty,
            avg_price: if response.avg_price > Decimal::ZERO {
                Some(response.avg_price)
            } else {
                None
            },
            time_in_force: response.time_in_force.to_time_in_force(),
            created_at: timestamp_to_datetime(response.time),
            updated_at: timestamp_to_datetime(response.update_time),
        }
    }

    /// Parse a WebSocket message and convert to execution report.
    pub fn parse_ws_message(&self, json: &str) -> VenueResult<Option<ExecutionReport>> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| VenueError::Parse(e.to_string()))?;

        let event_type = value
            .get("e")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        match event_type {
            "ORDER_TRADE_UPDATE" => {
                let update: FuturesOrderTradeUpdate = serde_json::from_str(json).map_err(|e| {
                    VenueError::Parse(format!("Failed to parse order trade update: {}", e))
                })?;

                debug!(
                    "Parsed futures order update: {} {} {:?}",
                    update.order.symbol, update.order.client_order_id, update.order.execution_type
                );

                Ok(Some(self.normalize_ws_report(&update.order)))
            }
            "ACCOUNT_UPDATE" => {
                debug!("Received account update");
                Ok(None)
            }
            "MARGIN_CALL" => {
                warn!("Received margin call!");
                Ok(None)
            }
            "ACCOUNT_CONFIG_UPDATE" => {
                debug!("Received account config update");
                Ok(None)
            }
            "listenKeyExpired" => {
                warn!("Listen key expired");
                Err(VenueError::Stream("Listen key expired".to_string()))
            }
            _ => {
                debug!("Unknown event type: {}", event_type);
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_normalize_ws_report() {
        let normalizer = FuturesExecutionNormalizer::new("BINANCE_FUTURES");

        let json = r#"{
            "e": "ORDER_TRADE_UPDATE",
            "E": 1568879465651,
            "T": 1568879465650,
            "o": {
                "s": "BTCUSDT",
                "c": "test_order_1",
                "S": "BUY",
                "o": "LIMIT",
                "f": "GTC",
                "q": "1.000",
                "p": "50000.00",
                "ap": "50000.00",
                "sp": "0",
                "x": "TRADE",
                "X": "FILLED",
                "i": 8886774,
                "l": "1.000",
                "z": "1.000",
                "L": "50000.00",
                "n": "0.005",
                "N": "USDT",
                "T": 1568879465651,
                "t": 123456,
                "b": "0",
                "a": "0",
                "m": true,
                "R": false,
                "wt": "CONTRACT_PRICE",
                "ot": "LIMIT",
                "ps": "BOTH",
                "cp": false,
                "pP": false,
                "rp": "0"
            }
        }"#;

        let result = normalizer.parse_ws_message(json).unwrap();
        assert!(result.is_some());

        let report = result.unwrap();
        assert_eq!(report.symbol, "BTCUSDT");
        assert_eq!(report.client_order_id.as_str(), "test_order_1");
        assert_eq!(report.status, OrderStatus::Filled);
        assert_eq!(report.cum_qty, dec!(1));
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
    }

    #[test]
    fn test_to_order_event_fill() {
        let normalizer = FuturesExecutionNormalizer::new("BINANCE_FUTURES");

        let report = ExecutionReport {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: VenueOrderId::new("123"),
            symbol: "BTCUSDT".to_string(),
            side: crate::orders::OrderSide::Buy,
            order_type: crate::orders::OrderType::Market,
            status: OrderStatus::Filled,
            price: None,
            quantity: dec!(1),
            last_qty: Some(dec!(1)),
            last_price: Some(dec!(50000)),
            cum_qty: dec!(1),
            leaves_qty: dec!(0),
            avg_price: Some(dec!(50000)),
            commission: Some(dec!(0.005)),
            commission_asset: Some("USDT".to_string()),
            liquidity_side: LiquiditySide::Taker,
            trade_id: Some("456".to_string()),
            event_time: chrono::Utc::now(),
            transaction_time: chrono::Utc::now(),
            reject_reason: None,
        };

        let event = normalizer.to_order_event(&report);
        assert!(event.is_some());

        if let Some(OrderEventAny::Filled(filled)) = event {
            assert_eq!(filled.last_qty, dec!(1));
            assert_eq!(filled.last_px, dec!(50000));
        } else {
            panic!("Expected Filled event");
        }
    }
}
