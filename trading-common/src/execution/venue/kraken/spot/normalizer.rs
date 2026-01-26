//! Execution normalizer for Kraken Spot.
//!
//! Converts Kraken-specific API responses to normalized execution reports.

use rust_decimal::Decimal;
use tracing::debug;

use crate::execution::venue::kraken::common::{
    parse_kraken_decimal, timestamp_to_datetime, KrakenExecutionType, KrakenLiquiditySide,
};
use crate::execution::venue::types::{ExecutionReport, OrderQueryResponse};
use crate::orders::{
    AccountId, ClientOrderId, InstrumentId, LiquiditySide, OrderAccepted, OrderCanceled,
    OrderEventAny, OrderExpired, OrderFilled, OrderRejected, OrderSide, OrderStatus, OrderType,
    StrategyId, TimeInForce, TradeId, VenueOrderId,
};

use super::types::{from_kraken_symbol, SpotAddOrderResponse, SpotOrderInfo, WsExecutionData};

/// Execution normalizer for Kraken Spot.
pub struct SpotExecutionNormalizer {
    /// Venue name for reports
    _venue_name: String,
}

impl SpotExecutionNormalizer {
    /// Create a new normalizer.
    pub fn new(venue_name: &str) -> Self {
        Self {
            _venue_name: venue_name.to_string(),
        }
    }

    /// Normalize a new order response.
    pub fn normalize_new_order(&self, response: &SpotAddOrderResponse, client_order_id: &ClientOrderId, symbol: &str, side: OrderSide, order_type: OrderType, quantity: Decimal, price: Option<Decimal>) -> ExecutionReport {
        let venue_order_id = response
            .txid
            .first()
            .map(|id| VenueOrderId::new(id.clone()))
            .unwrap_or_else(|| VenueOrderId::new("unknown"));

        ExecutionReport {
            client_order_id: client_order_id.clone(),
            venue_order_id,
            symbol: symbol.to_string(),
            side,
            order_type,
            status: OrderStatus::Accepted,
            price,
            quantity,
            last_qty: None,
            last_price: None,
            cum_qty: Decimal::ZERO,
            leaves_qty: quantity,
            avg_price: None,
            commission: None,
            commission_asset: None,
            liquidity_side: LiquiditySide::Taker,
            trade_id: None,
            event_time: chrono::Utc::now(),
            transaction_time: chrono::Utc::now(),
            reject_reason: None,
        }
    }

    /// Normalize an order query response.
    pub fn normalize_order_query(&self, txid: &str, info: &SpotOrderInfo) -> OrderQueryResponse {
        let status = info.parsed_status().to_order_status();
        let filled_qty = parse_kraken_decimal(&info.vol_exec);
        let total_qty = parse_kraken_decimal(&info.vol);

        OrderQueryResponse {
            venue_order_id: VenueOrderId::new(txid),
            client_order_id: info
                .cl_ord_id
                .as_ref()
                .map(ClientOrderId::new)
                .unwrap_or_else(|| ClientOrderId::new("unknown")),
            symbol: from_kraken_symbol(&info.descr.pair),
            side: info.side().to_order_side(),
            order_type: info.order_type().to_order_type(),
            status,
            price: info.descr.price.parse().ok(),
            quantity: total_qty,
            filled_qty,
            avg_price: info.price.as_ref().and_then(|p| p.parse().ok()),
            time_in_force: TimeInForce::GTC,
            created_at: info
                .opentm
                .map(|t| timestamp_to_datetime(&t.to_string()))
                .unwrap_or_else(chrono::Utc::now),
            updated_at: info
                .closetm
                .map(|t| timestamp_to_datetime(&t.to_string()))
                .unwrap_or_else(chrono::Utc::now),
        }
    }

    /// Parse a WebSocket execution message.
    pub fn parse_ws_message(&self, json: &str) -> Result<Option<ExecutionReport>, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("Parse error: {}", e))?;

        // Check if this is an executions channel message
        if value.get("channel").and_then(|v| v.as_str()) != Some("executions") {
            return Ok(None);
        }

        // Parse the data array
        let data = value
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "No data array in message".to_string())?;

        if let Some(first) = data.first() {
            let exec_data: WsExecutionData =
                serde_json::from_value(first.clone()).map_err(|e| format!("Parse error: {}", e))?;

            let report = self.normalize_ws_execution(&exec_data);
            return Ok(Some(report));
        }

        Ok(None)
    }

    /// Normalize a WebSocket execution data entry.
    pub fn normalize_ws_execution(&self, data: &WsExecutionData) -> ExecutionReport {
        let exec_type = match data.exec_type.as_str() {
            "pending_new" => KrakenExecutionType::PendingNew,
            "new" => KrakenExecutionType::New,
            "trade" => KrakenExecutionType::Trade,
            "filled" => KrakenExecutionType::Filled,
            "canceled" | "cancelled" => KrakenExecutionType::Canceled,
            "expired" => KrakenExecutionType::Expired,
            "restated" => KrakenExecutionType::Restated,
            _ => KrakenExecutionType::New,
        };

        let status = match exec_type {
            KrakenExecutionType::PendingNew => OrderStatus::Submitted,
            KrakenExecutionType::New | KrakenExecutionType::Restated => OrderStatus::Accepted,
            KrakenExecutionType::Trade => {
                // Check if fully filled
                let cum_qty = data
                    .cum_qty
                    .as_ref()
                    .map(|s| parse_kraken_decimal(s))
                    .unwrap_or(Decimal::ZERO);
                let order_qty = parse_kraken_decimal(&data.order_qty);
                if cum_qty >= order_qty {
                    OrderStatus::Filled
                } else {
                    OrderStatus::PartiallyFilled
                }
            }
            KrakenExecutionType::Filled => OrderStatus::Filled,
            KrakenExecutionType::Canceled => OrderStatus::Canceled,
            KrakenExecutionType::Expired => OrderStatus::Expired,
        };

        let side = if data.side.to_lowercase() == "buy" {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let order_type = match data.order_type.as_str() {
            "market" => OrderType::Market,
            "limit" => OrderType::Limit,
            "stop-loss" => OrderType::Stop,
            "stop-loss-limit" => OrderType::StopLimit,
            "take-profit" | "take-profit-limit" => OrderType::LimitIfTouched,
            _ => OrderType::Limit,
        };

        let cum_qty = data
            .cum_qty
            .as_ref()
            .map(|s| parse_kraken_decimal(s))
            .unwrap_or(Decimal::ZERO);

        let order_qty = parse_kraken_decimal(&data.order_qty);
        let leaves_qty = order_qty - cum_qty;

        let avg_price = data
            .avg_price
            .as_ref()
            .map(|s| parse_kraken_decimal(s));

        let last_price = data
            .last_price
            .as_ref()
            .map(|s| parse_kraken_decimal(s));

        let last_qty = data
            .last_qty
            .as_ref()
            .map(|s| parse_kraken_decimal(s));

        // Parse fees
        let (commission, commission_asset) = if let Some(fees) = &data.fees {
            if let Some(first_fee) = fees.first() {
                (
                    Some(parse_kraken_decimal(&first_fee.qty)),
                    Some(first_fee.asset.clone()),
                )
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let liquidity_side = data
            .liquidity_ind
            .as_ref()
            .map(|s| KrakenLiquiditySide::from_liquidity_ind(s).to_liquidity_side())
            .unwrap_or(LiquiditySide::Taker);

        let price = data.limit_price.as_ref().map(|s| parse_kraken_decimal(s));

        ExecutionReport {
            client_order_id: data
                .cl_ord_id
                .as_ref()
                .map(ClientOrderId::new)
                .unwrap_or_else(|| ClientOrderId::new("unknown")),
            venue_order_id: VenueOrderId::new(&data.order_id),
            symbol: from_kraken_symbol(&data.symbol),
            side,
            order_type,
            status,
            price,
            quantity: order_qty,
            last_qty,
            last_price,
            cum_qty,
            leaves_qty,
            avg_price,
            commission,
            commission_asset,
            liquidity_side,
            trade_id: data.exec_id.clone(),
            event_time: timestamp_to_datetime(&data.timestamp),
            transaction_time: timestamp_to_datetime(&data.timestamp),
            reject_reason: None,
        }
    }

    /// Convert execution report to order event.
    pub fn to_order_event(&self, report: &ExecutionReport) -> Option<OrderEventAny> {
        match report.status {
            OrderStatus::Accepted | OrderStatus::Submitted => {
                let accepted = OrderAccepted::new(
                    report.client_order_id.clone(),
                    report.venue_order_id.clone(),
                    AccountId::default(),
                );
                Some(OrderEventAny::Accepted(accepted))
            }
            OrderStatus::PartiallyFilled | OrderStatus::Filled => {
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
                            "USD".to_string(), // Kraken uses USD as quote currency
                            report.commission.unwrap_or(Decimal::ZERO),
                            report.commission_asset.clone().unwrap_or_default(),
                            report.liquidity_side,
                        );
                        Some(OrderEventAny::Filled(filled))
                    } else {
                        None
                    }
                } else {
                    debug!(
                        "Fill event without fill details: {:?}",
                        report.venue_order_id
                    );
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
                    report.reject_reason.clone().unwrap_or_else(|| "Order rejected".to_string()),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_ws_execution_new() {
        let normalizer = SpotExecutionNormalizer::new("KRAKEN");

        let data = WsExecutionData {
            exec_type: "new".to_string(),
            order_id: "OXXXXX-XXXXX-XXXXXX".to_string(),
            cl_ord_id: Some("client123".to_string()),
            symbol: "BTC/USD".to_string(),
            side: "buy".to_string(),
            order_type: "limit".to_string(),
            order_qty: "1.0".to_string(),
            order_status: "open".to_string(),
            limit_price: Some("50000".to_string()),
            stop_price: None,
            time_in_force: Some("GTC".to_string()),
            exec_id: None,
            last_qty: None,
            last_price: None,
            cum_qty: Some("0".to_string()),
            avg_price: None,
            liquidity_ind: None,
            fees: None,
            timestamp: "2024-01-15T10:30:00.000000Z".to_string(),
        };

        let report = normalizer.normalize_ws_execution(&data);

        assert_eq!(report.status, OrderStatus::Accepted);
        assert_eq!(report.venue_order_id.as_str(), "OXXXXX-XXXXX-XXXXXX");
        assert_eq!(report.client_order_id.as_str(), "client123");
        assert_eq!(report.symbol, "BTCUSDT");
    }

    #[test]
    fn test_normalize_ws_execution_trade() {
        let normalizer = SpotExecutionNormalizer::new("KRAKEN");

        let data = WsExecutionData {
            exec_type: "trade".to_string(),
            order_id: "OXXXXX-XXXXX-XXXXXX".to_string(),
            cl_ord_id: Some("client123".to_string()),
            symbol: "BTC/USD".to_string(),
            side: "buy".to_string(),
            order_type: "limit".to_string(),
            order_qty: "1.0".to_string(),
            order_status: "open".to_string(),
            limit_price: Some("50000".to_string()),
            stop_price: None,
            time_in_force: Some("GTC".to_string()),
            exec_id: Some("TXXXXX-XXXXX-XXXXXX".to_string()),
            last_qty: Some("0.5".to_string()),
            last_price: Some("49999".to_string()),
            cum_qty: Some("0.5".to_string()),
            avg_price: Some("49999".to_string()),
            liquidity_ind: Some("maker".to_string()),
            fees: Some(vec![super::super::types::WsFeeInfo {
                asset: "USD".to_string(),
                qty: "0.25".to_string(),
            }]),
            timestamp: "2024-01-15T10:30:00.000000Z".to_string(),
        };

        let report = normalizer.normalize_ws_execution(&data);

        assert_eq!(report.status, OrderStatus::PartiallyFilled);
        assert_eq!(report.cum_qty, Decimal::new(5, 1));
        assert_eq!(report.last_qty, Some(Decimal::new(5, 1)));
        assert_eq!(report.last_price, Some(Decimal::new(49999, 0)));
        assert_eq!(report.commission, Some(Decimal::new(25, 2)));
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
    }

    #[test]
    fn test_to_order_event_accepted() {
        let normalizer = SpotExecutionNormalizer::new("KRAKEN");

        let report = ExecutionReport {
            client_order_id: ClientOrderId::new("client123"),
            venue_order_id: VenueOrderId::new("OXXXXX"),
            symbol: "BTCUSDT".to_string(),
            side: OrderSide::Buy,
            order_type: OrderType::Limit,
            status: OrderStatus::Accepted,
            price: Some(Decimal::new(50000, 0)),
            quantity: Decimal::ONE,
            last_qty: None,
            last_price: None,
            cum_qty: Decimal::ZERO,
            leaves_qty: Decimal::ONE,
            avg_price: None,
            commission: None,
            commission_asset: None,
            liquidity_side: LiquiditySide::Taker,
            trade_id: None,
            event_time: chrono::Utc::now(),
            transaction_time: chrono::Utc::now(),
            reject_reason: None,
        };

        let event = normalizer.to_order_event(&report);

        assert!(matches!(event, Some(OrderEventAny::Accepted { .. })));
    }
}
