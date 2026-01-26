//! REST client for Binance USDT-M Futures API.
//!
//! This module provides typed methods for all Futures trading operations,
//! including leverage, margin type, and position management.

use std::sync::Arc;

use tracing::debug;

use crate::execution::venue::binance::common::{BinanceMarginType, BinanceTimeInForce};
use crate::execution::venue::binance::endpoints::{futures, weights};
use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::http::HttpClient;
use crate::orders::{Order, OrderSide};

use super::types::{
    FuturesAccountInfo, FuturesCancelOrderResponse, FuturesLeverageResponse,
    FuturesMarginTypeResponse, FuturesNewOrderResponse, FuturesOrderQueryResponse,
    FuturesOrderType, FuturesPositionModeResponse, FuturesPositionRisk,
};

/// REST client for Binance USDT-M Futures API.
pub struct FuturesRestClient {
    /// HTTP client
    http_client: Arc<HttpClient>,
}

impl FuturesRestClient {
    /// Create a new Futures REST client.
    pub fn new(http_client: Arc<HttpClient>) -> Self {
        Self { http_client }
    }

    /// Submit a new order.
    pub async fn submit_order(&self, order: &Order) -> VenueResult<FuturesNewOrderResponse> {
        // Build order parameters
        let symbol = &order.instrument_id.symbol;
        let side_str = match order.side {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        };
        let client_order_id = order.client_order_id.as_str();

        let mut params = vec![
            ("symbol", symbol.as_str()),
            ("side", side_str),
            ("newClientOrderId", client_order_id),
        ];

        // Determine order type
        let order_type = FuturesOrderType::from_order_type(order.order_type).ok_or_else(|| {
            VenueError::InvalidOrder(format!("Unsupported order type: {:?}", order.order_type))
        })?;

        let order_type_str = match order_type {
            FuturesOrderType::Market => "MARKET",
            FuturesOrderType::Limit => "LIMIT",
            FuturesOrderType::Stop => "STOP",
            FuturesOrderType::StopMarket => "STOP_MARKET",
            FuturesOrderType::TakeProfit => "TAKE_PROFIT",
            FuturesOrderType::TakeProfitMarket => "TAKE_PROFIT_MARKET",
            FuturesOrderType::TrailingStopMarket => "TRAILING_STOP_MARKET",
        };
        params.push(("type", order_type_str));

        // Add quantity
        let qty_str = order.quantity.to_string();
        params.push(("quantity", &qty_str));

        // Add price for limit orders
        let price_str;
        if order_type.requires_price() {
            if let Some(price) = order.price {
                price_str = price.to_string();
                params.push(("price", &price_str));
            } else {
                return Err(VenueError::InvalidOrder(
                    "Limit order requires price".to_string(),
                ));
            }
        }

        // Add stop price for stop orders
        let stop_price_str;
        if order_type.requires_stop_price() {
            if let Some(trigger_price) = order.trigger_price {
                stop_price_str = trigger_price.to_string();
                params.push(("stopPrice", &stop_price_str));
            } else {
                return Err(VenueError::InvalidOrder(
                    "Stop order requires trigger price".to_string(),
                ));
            }
        }

        // Add time in force (for limit orders)
        if matches!(order_type, FuturesOrderType::Limit | FuturesOrderType::Stop | FuturesOrderType::TakeProfit) {
            let tif = BinanceTimeInForce::from_time_in_force(order.time_in_force);
            let tif_str = match tif {
                BinanceTimeInForce::Gtc => "GTC",
                BinanceTimeInForce::Ioc => "IOC",
                BinanceTimeInForce::Fok => "FOK",
                BinanceTimeInForce::Gtx => "GTX",
                _ => "GTC",
            };
            params.push(("timeInForce", tif_str));
        }

        // Position side is only needed in hedge mode.
        // For one-way mode (default), positionSide should be omitted.
        // Users can specify position side via order tags if needed.
        if let Some(position_side) = order.get_tag("position_side") {
            let position_side_str = match position_side.as_str() {
                "LONG" | "long" | "Long" => "LONG",
                "SHORT" | "short" | "Short" => "SHORT",
                _ => "BOTH",
            };
            params.push(("positionSide", position_side_str));
        }

        // Add reduce only flag
        if order.is_reduce_only {
            params.push(("reduceOnly", "true"));
        }

        debug!("Submitting futures order: {:?}", params);

        self.http_client
            .post_signed(futures::ORDER, &params, weights::ORDER_SUBMIT)
            .await
    }

    /// Cancel an order.
    pub async fn cancel_order(
        &self,
        symbol: &str,
        client_order_id: Option<&str>,
        order_id: Option<i64>,
    ) -> VenueResult<FuturesCancelOrderResponse> {
        let mut params = vec![("symbol", symbol)];

        let order_id_str;
        if let Some(id) = client_order_id {
            params.push(("origClientOrderId", id));
        } else if let Some(id) = order_id {
            order_id_str = id.to_string();
            params.push(("orderId", &order_id_str));
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or order_id is required".to_string(),
            ));
        }

        debug!("Cancelling futures order: {:?}", params);

        self.http_client
            .delete_signed(futures::ORDER_CANCEL, &params, weights::ORDER_CANCEL)
            .await
    }

    /// Query an order.
    pub async fn query_order(
        &self,
        symbol: &str,
        client_order_id: Option<&str>,
        order_id: Option<i64>,
    ) -> VenueResult<FuturesOrderQueryResponse> {
        let mut params = vec![("symbol", symbol)];

        let order_id_str;
        if let Some(id) = client_order_id {
            params.push(("origClientOrderId", id));
        } else if let Some(id) = order_id {
            order_id_str = id.to_string();
            params.push(("orderId", &order_id_str));
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or order_id is required".to_string(),
            ));
        }

        self.http_client
            .get_signed(futures::ORDER_QUERY, &params, weights::QUERY_DEFAULT)
            .await
    }

    /// Query all open orders.
    pub async fn query_open_orders(
        &self,
        symbol: Option<&str>,
    ) -> VenueResult<Vec<FuturesOrderQueryResponse>> {
        let params: Vec<(&str, &str)> = match symbol {
            Some(s) => vec![("symbol", s)],
            None => vec![],
        };

        self.http_client
            .get_signed(futures::OPEN_ORDERS, &params, weights::OPEN_ORDERS)
            .await
    }

    /// Cancel all open orders for a symbol.
    pub async fn cancel_all_orders(
        &self,
        symbol: &str,
    ) -> VenueResult<serde_json::Value> {
        let params = vec![("symbol", symbol)];

        self.http_client
            .delete_signed(futures::ORDER_CANCEL_ALL, &params, weights::ORDER_CANCEL_ALL)
            .await
    }

    /// Query account information.
    pub async fn query_account(&self) -> VenueResult<FuturesAccountInfo> {
        self.http_client
            .get_signed(futures::ACCOUNT, &[], weights::ACCOUNT_INFO)
            .await
    }

    /// Query position risk for all positions.
    pub async fn query_positions(&self) -> VenueResult<Vec<FuturesPositionRisk>> {
        self.http_client
            .get_signed(futures::POSITION_RISK, &[], weights::POSITION_RISK)
            .await
    }

    /// Query position risk for a specific symbol.
    pub async fn query_position(&self, symbol: &str) -> VenueResult<FuturesPositionRisk> {
        let params = vec![("symbol", symbol)];
        let positions: Vec<FuturesPositionRisk> = self
            .http_client
            .get_signed(futures::POSITION_RISK, &params, weights::POSITION_RISK)
            .await?;

        positions.into_iter().next().ok_or_else(|| {
            VenueError::SymbolNotFound(format!("Position not found for {}", symbol))
        })
    }

    /// Set leverage for a symbol.
    pub async fn set_leverage(
        &self,
        symbol: &str,
        leverage: u32,
    ) -> VenueResult<FuturesLeverageResponse> {
        let leverage_str = leverage.to_string();
        let params = vec![("symbol", symbol), ("leverage", &leverage_str)];

        debug!("Setting leverage: {} -> {}x", symbol, leverage);

        self.http_client
            .post_signed(futures::LEVERAGE, &params, weights::QUERY_DEFAULT)
            .await
    }

    /// Set margin type for a symbol.
    pub async fn set_margin_type(
        &self,
        symbol: &str,
        margin_type: BinanceMarginType,
    ) -> VenueResult<()> {
        let margin_type_str = match margin_type {
            BinanceMarginType::Cross => "CROSSED",
            BinanceMarginType::Isolated => "ISOLATED",
        };
        let params = vec![("symbol", symbol), ("marginType", margin_type_str)];

        debug!("Setting margin type: {} -> {:?}", symbol, margin_type);

        let response: FuturesMarginTypeResponse = self
            .http_client
            .post_signed(futures::MARGIN_TYPE, &params, weights::QUERY_DEFAULT)
            .await?;

        if response.code != 200 && response.code != 0 {
            // Code -4046 means margin type already set
            if response.code == -4046 {
                return Ok(());
            }
            return Err(VenueError::VenueSpecific {
                code: response.code,
                message: response.msg,
            });
        }

        Ok(())
    }

    /// Get current position mode (hedge or one-way).
    pub async fn get_position_mode(&self) -> VenueResult<bool> {
        let response: FuturesPositionModeResponse = self
            .http_client
            .get_signed(futures::POSITION_MODE_GET, &[], weights::QUERY_DEFAULT)
            .await?;

        Ok(response.dual_side_position)
    }

    /// Set position mode (hedge or one-way).
    pub async fn set_position_mode(&self, hedge_mode: bool) -> VenueResult<()> {
        let dual_side = if hedge_mode { "true" } else { "false" };
        let params = vec![("dualSidePosition", dual_side)];

        debug!("Setting position mode: hedge={}", hedge_mode);

        let _: serde_json::Value = self
            .http_client
            .post_signed(futures::POSITION_MODE, &params, weights::QUERY_DEFAULT)
            .await?;

        Ok(())
    }

    /// Test connectivity (ping).
    pub async fn ping(&self) -> VenueResult<()> {
        let _: serde_json::Value = self
            .http_client
            .get_public(futures::PING, &[], weights::QUERY_DEFAULT)
            .await?;
        Ok(())
    }

    /// Get the HTTP client.
    pub fn http_client(&self) -> &Arc<HttpClient> {
        &self.http_client
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_type_string() {
        assert_eq!(
            FuturesOrderType::from_order_type(crate::orders::OrderType::Market),
            Some(FuturesOrderType::Market)
        );
        assert_eq!(
            FuturesOrderType::from_order_type(crate::orders::OrderType::StopLimit),
            Some(FuturesOrderType::Stop)
        );
    }
}
