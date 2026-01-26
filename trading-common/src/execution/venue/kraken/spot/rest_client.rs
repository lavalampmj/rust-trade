//! REST client for Kraken Spot API.
//!
//! This module provides typed methods for all Spot trading operations.
//!
//! Key differences from Binance:
//! - All private endpoints use POST
//! - Authentication via API-Key and API-Sign headers
//! - Uses nonce instead of timestamp
//! - Responses wrapped in {"error": [], "result": {...}}

use std::sync::Arc;

use rust_decimal::Decimal;
use tracing::debug;

use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::kraken::common::{
    KrakenHmacSigner, KrakenOrderType, KrakenTimeInForce,
};
use crate::execution::venue::kraken::endpoints::spot as endpoints;
use crate::orders::{Order, OrderSide};

use super::types::{
    to_kraken_symbol, SpotAccountBalance, SpotAddOrderResponse, SpotAmendOrderResponse,
    SpotCancelAllResponse, SpotCancelOrderResponse, SpotOpenOrdersResponse, SpotOrderInfo,
    SpotQueryOrdersResponse, SpotServerTime, SpotSystemStatus,
};

/// REST client for Kraken Spot API.
pub struct SpotRestClient {
    /// HTTP client
    http_client: reqwest::Client,
    /// Request signer
    signer: Arc<KrakenHmacSigner>,
    /// Base URL
    base_url: String,
}

impl SpotRestClient {
    /// Create a new Spot REST client.
    pub fn new(
        http_client: reqwest::Client,
        signer: Arc<KrakenHmacSigner>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            http_client,
            signer,
            base_url: base_url.into(),
        }
    }

    // =========================================================================
    // PUBLIC ENDPOINTS
    // =========================================================================

    /// Test connectivity - get server time.
    pub async fn ping(&self) -> VenueResult<SpotServerTime> {
        self.get_public(endpoints::SERVER_TIME).await
    }

    /// Get system status.
    pub async fn system_status(&self) -> VenueResult<SpotSystemStatus> {
        self.get_public(endpoints::SYSTEM_STATUS).await
    }

    // =========================================================================
    // ACCOUNT ENDPOINTS
    // =========================================================================

    /// Query account balances.
    pub async fn query_balance(&self) -> VenueResult<SpotAccountBalance> {
        self.post_private(endpoints::BALANCE, &[]).await
    }

    // =========================================================================
    // ORDER ENDPOINTS
    // =========================================================================

    /// Submit a new order.
    pub async fn submit_order(&self, order: &Order) -> VenueResult<SpotAddOrderResponse> {
        let symbol = to_kraken_symbol(&order.instrument_id.symbol);
        let side = match order.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        let order_type = KrakenOrderType::from_order_type(order.order_type, order.is_post_only)
            .ok_or_else(|| {
                VenueError::InvalidOrder(format!("Unsupported order type: {:?}", order.order_type))
            })?;

        let mut params = vec![
            ("pair", symbol.as_str()),
            ("type", side),
            ("ordertype", order_type.as_str()),
        ];

        // Add quantity
        let qty_str = order.quantity.to_string();
        params.push(("volume", &qty_str));

        // Add client order ID
        let client_order_id = order.client_order_id.as_str();
        params.push(("cl_ord_id", client_order_id));

        // Add price for limit orders
        let price_str = order.price.map(|p| p.to_string());
        if order_type.requires_price() {
            if let Some(ref price) = price_str {
                params.push(("price", price.as_str()));
            } else {
                return Err(VenueError::InvalidOrder(
                    "Limit order requires price".to_string(),
                ));
            }
        }

        // Add trigger price for stop orders
        let trigger_price_str = order.trigger_price.map(|p| p.to_string());
        let price2_str = order.price.map(|p| p.to_string());
        if order_type.requires_trigger_price() {
            if let Some(ref trigger_price) = trigger_price_str {
                params.push(("price", trigger_price.as_str())); // Stop price goes in "price"
                // Limit price goes in "price2" for stop-limit orders
                if order_type.requires_price() {
                    if let Some(ref price2) = price2_str {
                        params.push(("price2", price2.as_str()));
                    }
                }
            } else {
                return Err(VenueError::InvalidOrder(
                    "Stop order requires trigger price".to_string(),
                ));
            }
        }

        // Add time in force
        let tif = KrakenTimeInForce::from_time_in_force(order.time_in_force);
        params.push(("timeinforce", tif.as_str()));

        // Add order flags
        let mut flags = Vec::new();
        if order.is_post_only {
            flags.push("post");
        }
        if order.is_reduce_only {
            flags.push("fciq"); // Fees in quote currency (closest to reduce-only behavior)
        }
        let flags_str;
        if !flags.is_empty() {
            flags_str = flags.join(",");
            params.push(("oflags", &flags_str));
        }

        debug!("Submitting Kraken order: {:?}", params);

        self.post_private(endpoints::ADD_ORDER, &params).await
    }

    /// Cancel an order.
    pub async fn cancel_order(
        &self,
        client_order_id: Option<&str>,
        venue_order_id: Option<&str>,
    ) -> VenueResult<SpotCancelOrderResponse> {
        let params = if let Some(cl_ord_id) = client_order_id {
            vec![("cl_ord_id", cl_ord_id)]
        } else if let Some(txid) = venue_order_id {
            vec![("txid", txid)]
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or venue_order_id is required".to_string(),
            ));
        };

        debug!("Cancelling Kraken order: {:?}", params);

        self.post_private(endpoints::CANCEL_ORDER, &params).await
    }

    /// Amend an order (true order modification - Kraken supports this!).
    pub async fn amend_order(
        &self,
        client_order_id: Option<&str>,
        venue_order_id: Option<&str>,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> VenueResult<SpotAmendOrderResponse> {
        let mut params = Vec::new();

        // Identify the order
        if let Some(cl_ord_id) = client_order_id {
            params.push(("cl_ord_id".to_string(), cl_ord_id.to_string()));
        } else if let Some(txid) = venue_order_id {
            params.push(("txid".to_string(), txid.to_string()));
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or venue_order_id is required".to_string(),
            ));
        }

        // Add new price
        if let Some(price) = new_price {
            params.push(("limit_price".to_string(), price.to_string()));
        }

        // Add new quantity
        if let Some(qty) = new_quantity {
            params.push(("order_qty".to_string(), qty.to_string()));
        }

        if new_price.is_none() && new_quantity.is_none() {
            return Err(VenueError::InvalidOrder(
                "At least one of new_price or new_quantity is required".to_string(),
            ));
        }

        debug!("Amending Kraken order: {:?}", params);

        let params_ref: Vec<(&str, &str)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        self.post_private(endpoints::AMEND_ORDER, &params_ref).await
    }

    /// Cancel all orders.
    pub async fn cancel_all(&self) -> VenueResult<SpotCancelAllResponse> {
        self.post_private(endpoints::CANCEL_ALL, &[]).await
    }

    /// Query open orders.
    pub async fn query_open_orders(&self) -> VenueResult<SpotOpenOrdersResponse> {
        self.post_private(endpoints::OPEN_ORDERS, &[]).await
    }

    /// Query specific orders by txid.
    pub async fn query_orders(&self, txids: &[&str]) -> VenueResult<SpotQueryOrdersResponse> {
        let txids_str = txids.join(",");
        let params = vec![("txid", txids_str.as_str())];

        self.post_private(endpoints::QUERY_ORDERS, &params).await
    }

    /// Query a single order.
    pub async fn query_order(
        &self,
        client_order_id: Option<&str>,
        venue_order_id: Option<&str>,
    ) -> VenueResult<SpotOrderInfo> {
        let txid = if let Some(txid) = venue_order_id {
            txid.to_string()
        } else if let Some(cl_ord_id) = client_order_id {
            // For client order ID, we need to search in open orders
            let open_orders = self.query_open_orders().await?;
            let found = open_orders.open.iter().find(|(_, info)| {
                info.cl_ord_id
                    .as_ref()
                    .map(|id| id == cl_ord_id)
                    .unwrap_or(false)
            });

            if let Some((txid, _)) = found {
                txid.clone()
            } else {
                return Err(VenueError::OrderNotFound(format!(
                    "Order with client_order_id {} not found",
                    cl_ord_id
                )));
            }
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or venue_order_id is required".to_string(),
            ));
        };

        let orders = self.query_orders(&[&txid]).await?;

        orders
            .into_iter()
            .next()
            .map(|(_, info)| info)
            .ok_or_else(|| VenueError::OrderNotFound(format!("Order {} not found", txid)))
    }

    // =========================================================================
    // INTERNAL METHODS
    // =========================================================================

    /// Make a public GET request.
    async fn get_public<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
    ) -> VenueResult<T> {
        let url = format!("{}{}", self.base_url, endpoint);

        debug!("GET (public) {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| VenueError::Request(format!("Request failed: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| VenueError::Request(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(VenueError::Request(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        // Parse Kraken response wrapper
        let api_response: crate::execution::venue::kraken::common::types::KrakenResponse<T> =
            serde_json::from_str(&body).map_err(|e| {
                VenueError::Parse(format!("Failed to parse response: {} - {}", e, body))
            })?;

        if api_response.has_error() {
            return Err(self.map_kraken_error(api_response.first_error().unwrap_or("Unknown error")));
        }

        api_response
            .result
            .ok_or_else(|| VenueError::Parse("No result in response".to_string()))
    }

    /// Make a private POST request.
    async fn post_private<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> VenueResult<T> {
        let nonce = KrakenHmacSigner::generate_nonce();

        // Build POST body with nonce
        let mut post_params: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        post_params.insert(0, ("nonce".to_string(), nonce.to_string()));

        let post_data: String = post_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        // Sign the request
        let signature = self.signer.sign(endpoint, &post_data, nonce);

        let url = format!("{}{}", self.base_url, endpoint);

        debug!("POST (private) {}", endpoint);

        let response = self
            .http_client
            .post(&url)
            .header(self.signer.api_key_header(), self.signer.api_key())
            .header(self.signer.signature_header(), signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| VenueError::Request(format!("Request failed: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| VenueError::Request(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(VenueError::Request(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        // Parse Kraken response wrapper
        let api_response: crate::execution::venue::kraken::common::types::KrakenResponse<T> =
            serde_json::from_str(&body).map_err(|e| {
                VenueError::Parse(format!("Failed to parse response: {} - {}", e, body))
            })?;

        if api_response.has_error() {
            return Err(self.map_kraken_error(api_response.first_error().unwrap_or("Unknown error")));
        }

        api_response
            .result
            .ok_or_else(|| VenueError::Parse("No result in response".to_string()))
    }

    /// Map Kraken error string to VenueError.
    fn map_kraken_error(&self, error: &str) -> VenueError {
        // Kraken errors are formatted like "ECategory:Specific error"
        if error.contains("Rate limit exceeded") || error.starts_with("EOrder:Rate limit") {
            VenueError::RateLimit {
                retry_after: Some(std::time::Duration::from_secs(60)),
            }
        } else if error.contains("Orders limit exceeded") {
            VenueError::OrderRejected {
                reason: error.to_string(),
                code: None,
            }
        } else if error.contains("Insufficient") {
            VenueError::InsufficientBalance(error.to_string())
        } else if error.contains("Unknown order") || error.contains("Order not found") {
            VenueError::OrderNotFound(error.to_string())
        } else if error.starts_with("EAPI:Invalid nonce")
            || error.starts_with("EAPI:Invalid key")
            || error.starts_with("EAPI:Invalid signature")
        {
            VenueError::Authentication(error.to_string())
        } else if error.starts_with("EGeneral:Invalid arguments")
            || error.starts_with("EOrder:Invalid")
        {
            VenueError::InvalidOrder(error.to_string())
        } else if error.contains("Unknown pair") || error.contains("Invalid asset pair") {
            VenueError::SymbolNotFound(error.to_string())
        } else {
            VenueError::VenueSpecific {
                code: 0,
                message: error.to_string(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_mapping() {
        let client = SpotRestClient::new(
            reqwest::Client::new(),
            Arc::new(
                KrakenHmacSigner::new("key", "dGVzdF9zZWNyZXRfa2V5XzEyMzQ1").unwrap(),
            ),
            "https://api.kraken.com",
        );

        let error = client.map_kraken_error("EOrder:Rate limit exceeded");
        assert!(matches!(error, VenueError::RateLimit { .. }));

        let error = client.map_kraken_error("EOrder:Insufficient funds");
        assert!(matches!(error, VenueError::InsufficientBalance(_)));

        let error = client.map_kraken_error("EAPI:Invalid key");
        assert!(matches!(error, VenueError::Authentication(_)));

        let error = client.map_kraken_error("EOrder:Unknown order");
        assert!(matches!(error, VenueError::OrderNotFound(_)));
    }
}
