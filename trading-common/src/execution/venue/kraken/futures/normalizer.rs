//! Execution report normalizer for Kraken Futures.
//!
//! This module handles normalization of Kraken Futures execution reports
//! from both REST API and WebSocket streams to a unified format.
//!
//! # Symbol Conversion (DBT Symbology)
//!
//! The normalizer converts Kraken Futures raw symbols (e.g., "PI_XBTUSD") to canonical
//! DBT parent format (e.g., "BTC.PERP") when a SymbolRegistry is configured.
//! This enables strategies to use venue-agnostic symbols.

use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::debug;

use crate::execution::venue::error::VenueResult;
use crate::execution::venue::kraken::common::KrakenOrderStatus;
use crate::execution::venue::router::SymbolRegistry;
use crate::execution::venue::types::{ExecutionReport, OrderQueryResponse};
use crate::orders::{
    AccountId, ClientOrderId, InstrumentId, LiquiditySide, OrderAccepted, OrderCanceled,
    OrderEventAny, OrderExpired, OrderFilled, OrderRejected, OrderSide, OrderStatus, OrderType,
    StrategyId, TimeInForce, TradeId, VenueOrderId,
};

use chrono::Utc;

use super::types::{
    from_futures_symbol, FuturesOrderInfo, WsFuturesFill, WsFuturesFillEvent, WsFuturesOrder,
    WsFuturesOpenOrdersEvent,
};

/// Normalizer for Kraken Futures execution reports.
///
/// Converts Kraken-specific execution reports to venue-agnostic types.
/// When a SymbolRegistry is configured, raw Kraken Futures symbols are converted
/// to canonical DBT parent format.
pub struct FuturesExecutionNormalizer {
    /// Venue identifier for symbol lookups
    venue_id: String,
    /// Symbol registry for raw → canonical conversion (optional)
    symbol_registry: Option<Arc<RwLock<SymbolRegistry>>>,
}

impl FuturesExecutionNormalizer {
    /// Create a new normalizer.
    pub fn new(venue_id: &str) -> Self {
        Self {
            venue_id: venue_id.to_string(),
            symbol_registry: None,
        }
    }

    /// Create a new normalizer with symbol registry for raw → canonical conversion.
    pub fn with_symbol_registry(venue_id: &str, registry: Arc<RwLock<SymbolRegistry>>) -> Self {
        Self {
            venue_id: venue_id.to_string(),
            symbol_registry: Some(registry),
        }
    }

    /// Set the symbol registry for raw → canonical conversion.
    pub fn set_symbol_registry(&mut self, registry: Arc<RwLock<SymbolRegistry>>) {
        self.symbol_registry = Some(registry);
    }

    /// Convert a raw Kraken Futures symbol to canonical format.
    ///
    /// If a registry is configured and has a mapping, uses the canonical symbol.
    /// Otherwise falls back to the legacy `from_futures_symbol` conversion.
    fn to_canonical(&self, raw_symbol: &str) -> String {
        if let Some(ref registry) = self.symbol_registry {
            if let Some(canonical) = registry.read().to_canonical(raw_symbol, &self.venue_id) {
                return canonical.to_string();
            }
        }
        // Fallback to legacy conversion
        from_futures_symbol(raw_symbol)
    }

    // =========================================================================
    // REST API NORMALIZATION
    // =========================================================================

    /// Normalize an order info response from REST API.
    ///
    /// The symbol is converted to canonical format if a SymbolRegistry is configured.
    pub fn normalize_order_query(&self, info: &FuturesOrderInfo) -> OrderQueryResponse {
        let status = match info.computed_status() {
            KrakenOrderStatus::Pending => OrderStatus::Submitted,
            KrakenOrderStatus::Open => OrderStatus::Accepted,
            KrakenOrderStatus::Closed => OrderStatus::Filled,
            KrakenOrderStatus::Canceled => OrderStatus::Canceled,
            KrakenOrderStatus::Expired => OrderStatus::Expired,
        };

        let order_type = match info.parsed_order_type() {
            crate::execution::venue::kraken::common::KrakenOrderType::Market => OrderType::Market,
            crate::execution::venue::kraken::common::KrakenOrderType::Limit => OrderType::Limit,
            crate::execution::venue::kraken::common::KrakenOrderType::StopLoss => OrderType::Stop,
            crate::execution::venue::kraken::common::KrakenOrderType::StopLossLimit => {
                OrderType::StopLimit
            }
            crate::execution::venue::kraken::common::KrakenOrderType::TakeProfit => {
                OrderType::LimitIfTouched
            }
            crate::execution::venue::kraken::common::KrakenOrderType::TakeProfitLimit => {
                OrderType::LimitIfTouched
            }
            crate::execution::venue::kraken::common::KrakenOrderType::TrailingStop
            | crate::execution::venue::kraken::common::KrakenOrderType::TrailingStopLimit => {
                OrderType::TrailingStop
            }
            crate::execution::venue::kraken::common::KrakenOrderType::SettlePosition => {
                OrderType::Market
            }
        };

        let side = match info.parsed_side() {
            crate::execution::venue::kraken::common::KrakenOrderSide::Buy => OrderSide::Buy,
            crate::execution::venue::kraken::common::KrakenOrderSide::Sell => OrderSide::Sell,
        };

        // Convert raw symbol to canonical format
        let symbol = info
            .symbol
            .as_ref()
            .map(|s| self.to_canonical(s))
            .unwrap_or_default();

        let timestamp = info
            .last_update_time
            .as_ref()
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now);

        OrderQueryResponse {
            client_order_id: ClientOrderId::new(
                info.cli_ord_id.clone().unwrap_or_else(|| "unknown".to_string()),
            ),
            venue_order_id: info
                .order_id
                .clone()
                .map(VenueOrderId::new)
                .unwrap_or_else(|| VenueOrderId::new("unknown")),
            symbol,
            side,
            order_type,
            status,
            price: info.limit_price,
            quantity: info.quantity.unwrap_or_default(),
            filled_qty: info.filled.unwrap_or_default(),
            avg_price: info.limit_price, // Futures doesn't always provide avg price
            time_in_force: TimeInForce::GTC,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }

    // =========================================================================
    // WEBSOCKET NORMALIZATION
    // =========================================================================

    /// Parse a WebSocket message and return an execution report if applicable.
    pub fn parse_ws_message(&self, msg: &str) -> VenueResult<Option<ExecutionReport>> {
        // Try to parse as fill event
        if let Ok(fill_event) = serde_json::from_str::<WsFuturesFillEvent>(msg) {
            if fill_event.feed == "fills" || fill_event.feed == "fills_snapshot" {
                if let Some(fills) = fill_event.fills {
                    if let Some(fill) = fills.first() {
                        return Ok(Some(self.normalize_ws_fill(fill)));
                    }
                }
            }
        }

        // Try to parse as open orders event
        if let Ok(orders_event) = serde_json::from_str::<WsFuturesOpenOrdersEvent>(msg) {
            if orders_event.feed == "open_orders" || orders_event.feed == "open_orders_snapshot" {
                if let Some(orders) = orders_event.orders {
                    if let Some(order) = orders.first() {
                        return Ok(Some(self.normalize_ws_order(order)));
                    }
                }
            }
        }

        // Not an execution-related message
        debug!("Unhandled Futures WebSocket message: {}", msg);
        Ok(None)
    }

    /// Normalize a WebSocket fill message.
    ///
    /// The symbol is converted to canonical format if a SymbolRegistry is configured.
    fn normalize_ws_fill(&self, fill: &WsFuturesFill) -> ExecutionReport {
        // Convert raw symbol to canonical format
        let symbol = self.to_canonical(&fill.instrument);
        let side = if fill.buy {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };

        let liquidity = fill
            .fill_type
            .as_ref()
            .map(|t| {
                if t == "maker" {
                    LiquiditySide::Maker
                } else {
                    LiquiditySide::Taker
                }
            })
            .unwrap_or(LiquiditySide::Taker);

        let now = Utc::now();

        ExecutionReport {
            client_order_id: ClientOrderId::new(
                fill.cli_ord_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
            venue_order_id: VenueOrderId::new(
                fill.order_id.clone().unwrap_or_else(|| "unknown".to_string()),
            ),
            symbol,
            side,
            order_type: OrderType::Limit, // Not always provided
            status: OrderStatus::PartiallyFilled, // Individual fill, may be partial
            price: Some(fill.price),
            quantity: fill.qty,
            last_qty: Some(fill.qty),
            last_price: Some(fill.price),
            cum_qty: fill.qty, // Single fill, cumulative = this fill
            leaves_qty: Decimal::ZERO, // Not provided in fill
            avg_price: Some(fill.price),
            commission: fill.fee_paid,
            commission_asset: fill.fee_currency.clone(),
            liquidity_side: liquidity,
            trade_id: fill.fill_id.clone(),
            event_time: now,
            transaction_time: now,
            reject_reason: None,
        }
    }

    /// Normalize a WebSocket order update message.
    ///
    /// The symbol is converted to canonical format if a SymbolRegistry is configured.
    fn normalize_ws_order(&self, order: &WsFuturesOrder) -> ExecutionReport {
        // Convert raw symbol to canonical format
        let symbol = self.to_canonical(&order.instrument);
        let side = match order.direction {
            Some(1) => OrderSide::Buy,
            Some(-1) => OrderSide::Sell,
            _ => OrderSide::Buy,
        };

        let status = if order.is_cancel.unwrap_or(false) {
            OrderStatus::Canceled
        } else {
            let filled = order.filled.unwrap_or_default();
            if filled >= order.qty {
                OrderStatus::Filled
            } else if filled > Decimal::ZERO {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Accepted
            }
        };

        let order_type = match order.order_type.as_deref() {
            Some("mkt") => OrderType::Market,
            Some("lmt") => OrderType::Limit,
            Some("stp") => OrderType::Stop,
            Some("take_profit") => OrderType::LimitIfTouched,
            _ => OrderType::Limit,
        };

        let filled = order.filled.unwrap_or_default();
        let remaining = order.qty - filled;
        let now = Utc::now();

        ExecutionReport {
            client_order_id: ClientOrderId::new(
                order
                    .cli_ord_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
            venue_order_id: VenueOrderId::new(
                order
                    .order_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            ),
            symbol,
            side,
            order_type,
            status,
            price: order.limit_price,
            quantity: order.qty,
            last_qty: None, // No individual fill info
            last_price: None,
            cum_qty: filled,
            leaves_qty: remaining,
            avg_price: order.limit_price,
            commission: None,
            commission_asset: None,
            liquidity_side: LiquiditySide::Taker, // Default
            trade_id: None,
            event_time: now,
            transaction_time: now,
            reject_reason: order.reason.clone(),
        }
    }

    // =========================================================================
    // ORDER EVENT CONVERSION
    // =========================================================================

    /// Convert an execution report to an order event.
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
            OrderStatus::Filled | OrderStatus::PartiallyFilled => {
                // Create a fill event
                let last_qty = report.last_qty.unwrap_or(report.cum_qty);
                let last_px = report.last_price.unwrap_or_else(|| report.avg_price.unwrap_or_default());

                let filled = OrderFilled::new(
                    report.client_order_id.clone(),
                    report.venue_order_id.clone(),
                    AccountId::default(),
                    InstrumentId::new(&report.symbol, &self.venue_id),
                    TradeId::new(report.trade_id.clone().unwrap_or_else(|| "unknown".to_string())),
                    StrategyId::default(),
                    report.side,
                    report.order_type,
                    last_qty,
                    last_px,
                    report.cum_qty,
                    report.leaves_qty,
                    "USD".to_string(), // Futures typically in USD
                    report.commission.unwrap_or_default(),
                    report.commission_asset.clone().unwrap_or_else(|| "USD".to_string()),
                    report.liquidity_side,
                );
                Some(OrderEventAny::Filled(filled))
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
    fn test_normalize_order_query() {
        let normalizer = FuturesExecutionNormalizer::new("KRAKEN_FUTURES");

        let info = FuturesOrderInfo {
            order_id: Some("12345".to_string()),
            cli_ord_id: Some("my-order".to_string()),
            symbol: Some("PI_XBTUSD".to_string()),
            side: Some("buy".to_string()),
            order_type: Some("lmt".to_string()),
            limit_price: Some(Decimal::new(50000, 0)),
            stop_price: None,
            quantity: Some(Decimal::ONE),
            filled: Some(Decimal::ZERO),
            unfilled_size: Some(Decimal::ONE),
            reduce_only: None,
            last_update_time: None,
        };

        let response = normalizer.normalize_order_query(&info);

        assert_eq!(response.client_order_id.as_str(), "my-order");
        assert_eq!(response.venue_order_id.as_str(), "12345");
        assert_eq!(response.symbol, "BTCUSDT");
        assert_eq!(response.side, OrderSide::Buy);
        assert_eq!(response.status, OrderStatus::Accepted);
    }

    #[test]
    fn test_normalize_ws_fill() {
        let normalizer = FuturesExecutionNormalizer::new("KRAKEN_FUTURES");

        let fill = WsFuturesFill {
            instrument: "PI_XBTUSD".to_string(),
            time: 1705320600000,
            price: Decimal::new(50000, 0),
            seq: 123,
            buy: true,
            qty: Decimal::ONE,
            order_id: Some("order123".to_string()),
            cli_ord_id: Some("my-order".to_string()),
            fill_id: Some("fill456".to_string()),
            fill_type: Some("maker".to_string()),
            fee_paid: Some(Decimal::new(1, 2)),
            fee_currency: Some("USD".to_string()),
        };

        let report = normalizer.normalize_ws_fill(&fill);

        assert_eq!(report.client_order_id.as_str(), "my-order");
        assert_eq!(report.symbol, "BTCUSDT");
        assert_eq!(report.side, OrderSide::Buy);
        assert_eq!(report.price, Some(Decimal::new(50000, 0)));
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
    }

    #[test]
    fn test_normalize_ws_order() {
        let normalizer = FuturesExecutionNormalizer::new("KRAKEN_FUTURES");

        let order = WsFuturesOrder {
            instrument: "PI_ETHUSD".to_string(),
            time: 1705320600000,
            last_update_time: None,
            qty: Decimal::new(10, 0),
            filled: Some(Decimal::new(5, 0)),
            limit_price: Some(Decimal::new(2500, 0)),
            stop_price: None,
            order_type: Some("lmt".to_string()),
            order_id: Some("order789".to_string()),
            cli_ord_id: Some("client-order".to_string()),
            direction: Some(1),
            reduce_only: None,
            is_cancel: None,
            reason: None,
        };

        let report = normalizer.normalize_ws_order(&order);

        assert_eq!(report.symbol, "ETHUSDT");
        assert_eq!(report.side, OrderSide::Buy);
        assert_eq!(report.status, OrderStatus::PartiallyFilled);
        assert_eq!(report.cum_qty, Decimal::new(5, 0));
    }

    #[test]
    fn test_to_order_event_accepted() {
        let normalizer = FuturesExecutionNormalizer::new("KRAKEN_FUTURES");

        let now = Utc::now();
        let report = ExecutionReport {
            client_order_id: ClientOrderId::new("test-order"),
            venue_order_id: VenueOrderId::new("venue-123"),
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
            event_time: now,
            transaction_time: now,
            reject_reason: None,
        };

        let event = normalizer.to_order_event(&report);
        assert!(matches!(event, Some(OrderEventAny::Accepted(_))));
    }

    #[test]
    fn test_to_order_event_filled() {
        let normalizer = FuturesExecutionNormalizer::new("KRAKEN_FUTURES");

        let now = Utc::now();
        let report = ExecutionReport {
            client_order_id: ClientOrderId::new("test-order"),
            venue_order_id: VenueOrderId::new("venue-123"),
            symbol: "BTCUSDT".to_string(),
            side: OrderSide::Sell,
            order_type: OrderType::Limit,
            status: OrderStatus::Filled,
            price: Some(Decimal::new(50000, 0)),
            quantity: Decimal::ONE,
            last_qty: Some(Decimal::ONE),
            last_price: Some(Decimal::new(50000, 0)),
            cum_qty: Decimal::ONE,
            leaves_qty: Decimal::ZERO,
            avg_price: Some(Decimal::new(50000, 0)),
            commission: Some(Decimal::new(1, 2)),
            commission_asset: Some("USD".to_string()),
            liquidity_side: LiquiditySide::Taker,
            trade_id: Some("trade-456".to_string()),
            event_time: now,
            transaction_time: now,
            reject_reason: None,
        };

        let event = normalizer.to_order_event(&report);
        assert!(matches!(event, Some(OrderEventAny::Filled(_))));
    }
}
