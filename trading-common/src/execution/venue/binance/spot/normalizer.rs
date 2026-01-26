//! Execution report normalizer for Binance Spot.
//!
//! Converts Binance Spot-specific types to venue-agnostic types.

use rust_decimal::Decimal;
use tracing::{debug, warn};

use crate::execution::venue::binance::common::timestamp_to_datetime;
use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::types::ExecutionReport;
use crate::orders::{
    ClientOrderId, LiquiditySide, OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired,
    OrderFilled, OrderRejected, OrderStatus, TradeId, VenueOrderId,
};

use super::types::{SpotExecutionReport, SpotNewOrderResponse, SpotOrderQueryResponse};

/// Normalizer for Binance Spot execution reports.
pub struct SpotExecutionNormalizer {
    /// Venue name for events
    venue_name: String,
}

impl SpotExecutionNormalizer {
    /// Create a new normalizer.
    pub fn new(venue_name: impl Into<String>) -> Self {
        Self {
            venue_name: venue_name.into(),
        }
    }

    /// Convert a new order response to an execution report.
    pub fn normalize_new_order(&self, response: &SpotNewOrderResponse) -> ExecutionReport {
        // Calculate average price from fills
        let (avg_price, total_commission, commission_asset) = if !response.fills.is_empty() {
            let total_qty: Decimal = response.fills.iter().map(|f| f.qty).sum();
            let weighted_price: Decimal = response
                .fills
                .iter()
                .map(|f| f.price * f.qty)
                .sum::<Decimal>();
            let avg = if total_qty > Decimal::ZERO {
                Some(weighted_price / total_qty)
            } else {
                None
            };
            let commission: Decimal = response.fills.iter().map(|f| f.commission).sum();
            let asset = response.fills.first().map(|f| f.commission_asset.clone());
            (avg, Some(commission), asset)
        } else {
            (None, None, None)
        };

        let leaves_qty = response.orig_qty - response.executed_qty;

        ExecutionReport {
            client_order_id: ClientOrderId::new(&response.client_order_id),
            venue_order_id: VenueOrderId::new(response.order_id.to_string()),
            symbol: response.symbol.clone(),
            side: response.side.to_order_side(),
            order_type: response.order_type.to_order_type(),
            status: response.status.to_order_status(),
            price: response.price,
            quantity: response.orig_qty,
            last_qty: response.fills.first().map(|f| f.qty),
            last_price: response.fills.first().map(|f| f.price),
            cum_qty: response.executed_qty,
            leaves_qty,
            avg_price,
            commission: total_commission,
            commission_asset,
            liquidity_side: LiquiditySide::Taker, // New orders are takers by default
            trade_id: response.fills.first().map(|f| f.trade_id.to_string()),
            event_time: timestamp_to_datetime(response.transact_time),
            transaction_time: timestamp_to_datetime(response.transact_time),
            reject_reason: None,
        }
    }

    /// Convert a WebSocket execution report to our normalized format.
    pub fn normalize_ws_report(&self, report: &SpotExecutionReport) -> ExecutionReport {
        let leaves_qty = report.quantity - report.cum_qty;

        // Calculate average price
        let avg_price = if report.cum_qty > Decimal::ZERO && report.cum_quote_qty > Decimal::ZERO {
            Some(report.cum_quote_qty / report.cum_qty)
        } else {
            None
        };

        ExecutionReport {
            client_order_id: ClientOrderId::new(&report.client_order_id),
            venue_order_id: VenueOrderId::new(report.order_id.to_string()),
            symbol: report.symbol.clone(),
            side: report.side.to_order_side(),
            order_type: report.order_type.to_order_type(),
            status: report.order_status.to_order_status(),
            price: if report.price > Decimal::ZERO {
                Some(report.price)
            } else {
                None
            },
            quantity: report.quantity,
            last_qty: if report.last_qty > Decimal::ZERO {
                Some(report.last_qty)
            } else {
                None
            },
            last_price: if report.last_price > Decimal::ZERO {
                Some(report.last_price)
            } else {
                None
            },
            cum_qty: report.cum_qty,
            leaves_qty,
            avg_price,
            commission: report.commission,
            commission_asset: report.commission_asset.clone(),
            liquidity_side: report.liquidity_side().to_liquidity_side(),
            trade_id: if report.trade_id > 0 {
                Some(report.trade_id.to_string())
            } else {
                None
            },
            event_time: timestamp_to_datetime(report.event_time),
            transaction_time: timestamp_to_datetime(report.transaction_time),
            reject_reason: if report.reject_reason != "NONE" && !report.reject_reason.is_empty() {
                Some(report.reject_reason.clone())
            } else {
                None
            },
        }
    }

    /// Convert an execution report to an order event.
    pub fn to_order_event(&self, report: &ExecutionReport) -> Option<OrderEventAny> {
        use crate::orders::{AccountId, InstrumentId, StrategyId};

        let event = match report.status {
            OrderStatus::Accepted => {
                let accepted = OrderAccepted::new(
                    report.client_order_id.clone(),
                    report.venue_order_id.clone(),
                    AccountId::default(),
                );
                Some(OrderEventAny::Accepted(accepted))
            }
            OrderStatus::Filled | OrderStatus::PartiallyFilled => {
                // Only emit fill event if there was actually a fill
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
        };

        event
    }

    /// Convert an order query response to our normalized format.
    pub fn normalize_order_query(
        &self,
        response: &SpotOrderQueryResponse,
    ) -> crate::execution::venue::types::OrderQueryResponse {
        let avg_price = if response.executed_qty > Decimal::ZERO
            && response.cummulative_quote_qty > Decimal::ZERO
        {
            Some(response.cummulative_quote_qty / response.executed_qty)
        } else {
            None
        };

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
            avg_price,
            time_in_force: response.time_in_force.to_time_in_force(),
            created_at: timestamp_to_datetime(response.time),
            updated_at: timestamp_to_datetime(response.update_time),
        }
    }

    /// Parse a WebSocket message and convert to execution report.
    pub fn parse_ws_message(&self, json: &str) -> VenueResult<Option<ExecutionReport>> {
        // First check if this is an execution report
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| VenueError::Parse(e.to_string()))?;

        let event_type = value
            .get("e")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        match event_type {
            "executionReport" => {
                let report: SpotExecutionReport = serde_json::from_str(json)
                    .map_err(|e| VenueError::Parse(format!("Failed to parse execution report: {}", e)))?;

                debug!(
                    "Parsed spot execution report: {} {} {:?}",
                    report.symbol, report.client_order_id, report.execution_type
                );

                Ok(Some(self.normalize_ws_report(&report)))
            }
            "outboundAccountPosition" | "balanceUpdate" => {
                // Account/balance updates - not execution reports
                debug!("Received account update: {}", event_type);
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
    use crate::orders::OrderSide;
    use rust_decimal_macros::dec;

    #[test]
    fn test_normalize_ws_report() {
        let normalizer = SpotExecutionNormalizer::new("BINANCE_SPOT");

        let json = r#"{
            "e": "executionReport",
            "E": 1499405658658,
            "s": "ETHBTC",
            "c": "test_order_1",
            "S": "BUY",
            "o": "LIMIT",
            "f": "GTC",
            "q": "1.00000000",
            "p": "0.10000000",
            "P": "0.00000000",
            "F": "0.00000000",
            "g": -1,
            "C": "",
            "x": "TRADE",
            "X": "FILLED",
            "r": "NONE",
            "i": 4293153,
            "l": "1.00000000",
            "z": "1.00000000",
            "L": "0.10000000",
            "n": "0.00010000",
            "N": "BTC",
            "T": 1499405658657,
            "t": 123456,
            "I": 8641984,
            "w": false,
            "m": true,
            "M": false,
            "O": 1499405658657,
            "Z": "0.10000000",
            "Y": "0.10000000",
            "Q": "0.00000000"
        }"#;

        let result = normalizer.parse_ws_message(json).unwrap();
        assert!(result.is_some());

        let report = result.unwrap();
        assert_eq!(report.symbol, "ETHBTC");
        assert_eq!(report.client_order_id.as_str(), "test_order_1");
        assert_eq!(report.status, OrderStatus::Filled);
        assert_eq!(report.cum_qty, dec!(1));
        assert_eq!(report.last_qty, Some(dec!(1)));
        assert_eq!(report.commission, Some(dec!(0.0001)));
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
    }

    #[test]
    fn test_to_order_event_fill() {
        let normalizer = SpotExecutionNormalizer::new("BINANCE_SPOT");

        let report = ExecutionReport {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: VenueOrderId::new("123"),
            symbol: "BTCUSDT".to_string(),
            side: OrderSide::Buy,
            order_type: crate::orders::OrderType::Market,
            status: OrderStatus::Filled,
            price: None,
            quantity: dec!(1),
            last_qty: Some(dec!(1)),
            last_price: Some(dec!(50000)),
            cum_qty: dec!(1),
            leaves_qty: dec!(0),
            avg_price: Some(dec!(50000)),
            commission: Some(dec!(0.001)),
            commission_asset: Some("BTC".to_string()),
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
