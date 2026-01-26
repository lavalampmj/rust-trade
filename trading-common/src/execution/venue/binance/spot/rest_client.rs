//! REST client for Binance Spot API.
//!
//! This module provides typed methods for all Spot trading operations.

use std::sync::Arc;

use tracing::debug;

use crate::execution::venue::binance::common::BinanceTimeInForce;
use crate::execution::venue::binance::endpoints::{spot, weights};
use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::http::HttpClient;
use crate::orders::{Order, OrderSide};

use super::types::{
    SpotAccountInfo, SpotCancelOrderResponse, SpotNewOrderResponse, SpotOrderQueryResponse,
    SpotOrderType,
};

/// REST client for Binance Spot API.
pub struct SpotRestClient {
    /// HTTP client
    http_client: Arc<HttpClient>,
}

impl SpotRestClient {
    /// Create a new Spot REST client.
    pub fn new(http_client: Arc<HttpClient>) -> Self {
        Self { http_client }
    }

    /// Submit a new order.
    pub async fn submit_order(&self, order: &Order) -> VenueResult<SpotNewOrderResponse> {
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
        let order_type = SpotOrderType::from_order_type(order.order_type, order.is_post_only)
            .ok_or_else(|| {
                VenueError::InvalidOrder(format!(
                    "Unsupported order type: {:?}",
                    order.order_type
                ))
            })?;

        let order_type_str = match order_type {
            SpotOrderType::Market => "MARKET",
            SpotOrderType::Limit => "LIMIT",
            SpotOrderType::LimitMaker => "LIMIT_MAKER",
            SpotOrderType::StopLoss => "STOP_LOSS",
            SpotOrderType::StopLossLimit => "STOP_LOSS_LIMIT",
            SpotOrderType::TakeProfit => "TAKE_PROFIT",
            SpotOrderType::TakeProfitLimit => "TAKE_PROFIT_LIMIT",
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
        if matches!(
            order_type,
            SpotOrderType::Limit | SpotOrderType::StopLossLimit | SpotOrderType::TakeProfitLimit
        ) {
            let tif = BinanceTimeInForce::from_time_in_force(order.time_in_force);
            let tif_str = match tif {
                BinanceTimeInForce::Gtc => "GTC",
                BinanceTimeInForce::Ioc => "IOC",
                BinanceTimeInForce::Fok => "FOK",
                _ => "GTC",
            };
            params.push(("timeInForce", tif_str));
        }

        // Request full response to get fills
        params.push(("newOrderRespType", "FULL"));

        debug!("Submitting spot order: {:?}", params);

        self.http_client
            .post_signed(spot::ORDER, &params, weights::ORDER_SUBMIT)
            .await
    }

    /// Cancel an order.
    pub async fn cancel_order(
        &self,
        symbol: &str,
        client_order_id: Option<&str>,
        order_id: Option<i64>,
    ) -> VenueResult<SpotCancelOrderResponse> {
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

        debug!("Cancelling spot order: {:?}", params);

        self.http_client
            .delete_signed(spot::ORDER_CANCEL, &params, weights::ORDER_CANCEL)
            .await
    }

    /// Query an order.
    pub async fn query_order(
        &self,
        symbol: &str,
        client_order_id: Option<&str>,
        order_id: Option<i64>,
    ) -> VenueResult<SpotOrderQueryResponse> {
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
            .get_signed(spot::ORDER_QUERY, &params, weights::QUERY_DEFAULT)
            .await
    }

    /// Query all open orders.
    pub async fn query_open_orders(
        &self,
        symbol: Option<&str>,
    ) -> VenueResult<Vec<SpotOrderQueryResponse>> {
        let params: Vec<(&str, &str)> = match symbol {
            Some(s) => vec![("symbol", s)],
            None => vec![],
        };

        self.http_client
            .get_signed(spot::OPEN_ORDERS, &params, weights::OPEN_ORDERS)
            .await
    }

    /// Cancel all open orders for a symbol.
    pub async fn cancel_all_orders(
        &self,
        symbol: &str,
    ) -> VenueResult<Vec<SpotCancelOrderResponse>> {
        let params = vec![("symbol", symbol)];

        self.http_client
            .delete_signed(spot::ORDER_CANCEL_ALL, &params, weights::ORDER_CANCEL_ALL)
            .await
    }

    /// Query account information.
    pub async fn query_account(&self) -> VenueResult<SpotAccountInfo> {
        self.http_client
            .get_signed(spot::ACCOUNT, &[], weights::ACCOUNT_INFO)
            .await
    }

    /// Test connectivity (ping).
    pub async fn ping(&self) -> VenueResult<()> {
        let _: serde_json::Value = self
            .http_client
            .get_public(spot::PING, &[], weights::QUERY_DEFAULT)
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
    use crate::orders::Order;
    use rust_decimal_macros::dec;

    #[test]
    fn test_order_type_string() {
        // Just test that the order type strings are correct
        assert_eq!(
            SpotOrderType::from_order_type(crate::orders::OrderType::Market, false),
            Some(SpotOrderType::Market)
        );
    }
}
