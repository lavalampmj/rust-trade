//! REST client for Kraken Futures API.
//!
//! This module provides typed methods for Kraken Futures (derivatives) trading.
//!
//! Key differences from Spot:
//! - Different API version (v3)
//! - Different authentication (challenge-based for WebSocket)
//! - Different endpoints (`/derivatives/api/v3/...`)
//! - Different response format (result: "success" instead of error: [])

use std::sync::Arc;

use rust_decimal::Decimal;
use tracing::debug;

use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::kraken::common::{
    KrakenFuturesSigner, KrakenOrderType, KrakenTimeInForce,
};
use crate::execution::venue::kraken::endpoints::futures as endpoints;
use crate::orders::{Order, OrderSide};

use super::types::{
    to_futures_symbol, FuturesAccountsData, FuturesCancelAllData, FuturesCancelOrderData,
    FuturesEditOrderData, FuturesFillsData, FuturesInstrumentsData, FuturesLeverageData,
    FuturesOpenOrdersData, FuturesOpenPositionsData, FuturesResponse, FuturesSendOrderData,
    FuturesTickersData, FuturesWsChallengeData,
};

/// REST client for Kraken Futures API.
pub struct FuturesRestClient {
    /// HTTP client
    http_client: reqwest::Client,
    /// Request signer (Futures-specific)
    signer: Arc<KrakenFuturesSigner>,
    /// Base URL
    base_url: String,
}

impl FuturesRestClient {
    /// Create a new Futures REST client.
    pub fn new(
        http_client: reqwest::Client,
        signer: Arc<KrakenFuturesSigner>,
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

    /// Get all tradeable instruments.
    pub async fn instruments(&self) -> VenueResult<FuturesInstrumentsData> {
        self.get_public(endpoints::INSTRUMENTS).await
    }

    /// Get tickers for all instruments.
    pub async fn tickers(&self) -> VenueResult<FuturesTickersData> {
        self.get_public(endpoints::TICKERS).await
    }

    // =========================================================================
    // ACCOUNT ENDPOINTS
    // =========================================================================

    /// Query account information.
    pub async fn accounts(&self) -> VenueResult<FuturesAccountsData> {
        self.get_private(endpoints::ACCOUNTS).await
    }

    /// Get open orders.
    pub async fn open_orders(&self) -> VenueResult<FuturesOpenOrdersData> {
        self.get_private(endpoints::OPEN_ORDERS).await
    }

    /// Get open positions.
    pub async fn open_positions(&self) -> VenueResult<FuturesOpenPositionsData> {
        self.get_private(endpoints::OPEN_POSITIONS).await
    }

    /// Get fill history.
    pub async fn fills(&self) -> VenueResult<FuturesFillsData> {
        self.get_private(endpoints::FILLS).await
    }

    /// Get leverage preferences.
    pub async fn leverage(&self) -> VenueResult<FuturesLeverageData> {
        self.get_private(endpoints::LEVERAGE).await
    }

    // =========================================================================
    // ORDER ENDPOINTS
    // =========================================================================

    /// Send a new order.
    pub async fn send_order(&self, order: &Order) -> VenueResult<FuturesSendOrderData> {
        let symbol = to_futures_symbol(&order.instrument_id.symbol);
        let side = match order.side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };

        let order_type = KrakenOrderType::from_order_type(order.order_type, order.is_post_only)
            .ok_or_else(|| {
                VenueError::InvalidOrder(format!("Unsupported order type: {:?}", order.order_type))
            })?;

        // Futures uses different order type names
        let order_type_str = match order_type {
            KrakenOrderType::Market => "mkt",
            KrakenOrderType::Limit => "lmt",
            KrakenOrderType::StopLoss => "stp",
            KrakenOrderType::TakeProfit | KrakenOrderType::TakeProfitLimit => "take_profit",
            _ => "lmt",
        };

        let mut params = vec![
            ("symbol", symbol.as_str()),
            ("side", side),
            ("orderType", order_type_str),
        ];

        // Add quantity
        let size_str = order.quantity.to_string();
        params.push(("size", &size_str));

        // Add client order ID
        let client_order_id = order.client_order_id.as_str();
        params.push(("cliOrdId", client_order_id));

        // Add price for limit orders
        let price_str = order.price.map(|p| p.to_string());
        if order_type.requires_price() {
            if let Some(ref price) = price_str {
                params.push(("limitPrice", price.as_str()));
            } else {
                return Err(VenueError::InvalidOrder(
                    "Limit order requires price".to_string(),
                ));
            }
        }

        // Add stop price for stop orders
        let stop_price_str = order.trigger_price.map(|p| p.to_string());
        if order_type.requires_trigger_price() {
            if let Some(ref stop_price) = stop_price_str {
                params.push(("stopPrice", stop_price.as_str()));
            }
        }

        // Add time in force
        let tif = KrakenTimeInForce::from_time_in_force(order.time_in_force);
        let tif_str = match tif {
            KrakenTimeInForce::Gtc => "GTC",
            KrakenTimeInForce::Ioc => "IOC",
            KrakenTimeInForce::Gtd => "GTD",
        };
        params.push(("timeInForce", tif_str));

        // Reduce only flag
        if order.is_reduce_only {
            params.push(("reduceOnly", "true"));
        }

        // Post only flag
        if order.is_post_only {
            params.push(("postOnly", "true"));
        }

        debug!("Sending Kraken Futures order: {:?}", params);

        self.post_private(endpoints::SEND_ORDER, &params).await
    }

    /// Edit an existing order.
    pub async fn edit_order(
        &self,
        client_order_id: Option<&str>,
        venue_order_id: Option<&str>,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> VenueResult<FuturesEditOrderData> {
        let mut params = Vec::new();

        // Identify the order
        if let Some(cl_ord_id) = client_order_id {
            params.push(("cliOrdId".to_string(), cl_ord_id.to_string()));
        } else if let Some(order_id) = venue_order_id {
            params.push(("orderId".to_string(), order_id.to_string()));
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or venue_order_id is required".to_string(),
            ));
        }

        // Add new price
        if let Some(price) = new_price {
            params.push(("limitPrice".to_string(), price.to_string()));
        }

        // Add new quantity
        if let Some(qty) = new_quantity {
            params.push(("size".to_string(), qty.to_string()));
        }

        if new_price.is_none() && new_quantity.is_none() {
            return Err(VenueError::InvalidOrder(
                "At least one of new_price or new_quantity is required".to_string(),
            ));
        }

        debug!("Editing Kraken Futures order: {:?}", params);

        let params_ref: Vec<(&str, &str)> = params
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        self.post_private(endpoints::EDIT_ORDER, &params_ref).await
    }

    /// Cancel an order.
    pub async fn cancel_order(
        &self,
        client_order_id: Option<&str>,
        venue_order_id: Option<&str>,
    ) -> VenueResult<FuturesCancelOrderData> {
        let params = if let Some(cl_ord_id) = client_order_id {
            vec![("cliOrdId", cl_ord_id)]
        } else if let Some(order_id) = venue_order_id {
            vec![("order_id", order_id)]
        } else {
            return Err(VenueError::InvalidOrder(
                "Either client_order_id or venue_order_id is required".to_string(),
            ));
        };

        debug!("Cancelling Kraken Futures order: {:?}", params);

        self.post_private(endpoints::CANCEL_ORDER, &params).await
    }

    /// Cancel all orders.
    pub async fn cancel_all(&self, symbol: Option<&str>) -> VenueResult<FuturesCancelAllData> {
        let params = if let Some(sym) = symbol {
            let kraken_symbol = to_futures_symbol(sym);
            vec![("symbol", kraken_symbol)]
        } else {
            vec![]
        };

        debug!("Cancelling all Kraken Futures orders: {:?}", params);

        let params_ref: Vec<(&str, &str)> = params
            .iter()
            .map(|(k, v)| (k.as_ref(), v.as_ref()))
            .collect();

        self.post_private(endpoints::CANCEL_ALL, &params_ref).await
    }

    // =========================================================================
    // WEBSOCKET AUTHENTICATION
    // =========================================================================

    /// Get WebSocket authentication challenge.
    pub async fn ws_challenge(&self) -> VenueResult<FuturesWsChallengeData> {
        self.get_private(endpoints::WS_CHALLENGE).await
    }

    /// Sign the WebSocket challenge.
    pub fn sign_challenge(&self, challenge: &str) -> String {
        self.signer.sign_challenge(challenge)
    }

    // =========================================================================
    // INTERNAL METHODS
    // =========================================================================

    /// Make a public GET request.
    async fn get_public<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> VenueResult<T> {
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

        // Parse Futures response wrapper
        let api_response: FuturesResponse<T> = serde_json::from_str(&body).map_err(|e| {
            VenueError::Parse(format!("Failed to parse response: {} - {}", e, body))
        })?;

        if !api_response.is_success() {
            return Err(
                self.map_futures_error(api_response.error_message().unwrap_or("Unknown error"))
            );
        }

        api_response
            .data
            .ok_or_else(|| VenueError::Parse("No data in response".to_string()))
    }

    /// Make a private GET request.
    async fn get_private<T: serde::de::DeserializeOwned>(&self, endpoint: &str) -> VenueResult<T> {
        let nonce = KrakenFuturesSigner::generate_nonce();
        let nonce_str = nonce.to_string();

        // For GET requests, post_data is empty string
        let post_data = "";
        let signature = self.signer.sign(endpoint, post_data, nonce);

        let url = format!("{}{}", self.base_url, endpoint);

        debug!("GET (private) {} nonce={} api_key={}", endpoint, nonce, &self.signer.api_key()[..8]);
        debug!("Signature: {}", &signature[..20]);

        let response = self
            .http_client
            .get(&url)
            .header(self.signer.api_key_header(), self.signer.api_key())
            .header("Nonce", &nonce_str)
            .header(self.signer.signature_header(), signature)
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

        let api_response: FuturesResponse<T> = serde_json::from_str(&body).map_err(|e| {
            VenueError::Parse(format!("Failed to parse response: {} - {}", e, body))
        })?;

        if !api_response.is_success() {
            return Err(
                self.map_futures_error(api_response.error_message().unwrap_or("Unknown error"))
            );
        }

        api_response
            .data
            .ok_or_else(|| VenueError::Parse("No data in response".to_string()))
    }

    /// Make a private POST request.
    async fn post_private<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> VenueResult<T> {
        let nonce = KrakenFuturesSigner::generate_nonce();
        let nonce_str = nonce.to_string();

        // Build POST body (WITHOUT nonce in body - nonce goes in header for Futures)
        let post_data: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        // Sign the request
        let signature = self.signer.sign(endpoint, &post_data, nonce);

        let url = format!("{}{}", self.base_url, endpoint);

        debug!("POST (private) {} nonce={}", endpoint, nonce);

        let response = self
            .http_client
            .post(&url)
            .header(self.signer.api_key_header(), self.signer.api_key())
            .header("Nonce", &nonce_str)
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

        let api_response: FuturesResponse<T> = serde_json::from_str(&body).map_err(|e| {
            VenueError::Parse(format!("Failed to parse response: {} - {}", e, body))
        })?;

        if !api_response.is_success() {
            return Err(
                self.map_futures_error(api_response.error_message().unwrap_or("Unknown error"))
            );
        }

        api_response
            .data
            .ok_or_else(|| VenueError::Parse("No data in response".to_string()))
    }

    /// Map Kraken Futures error string to VenueError.
    fn map_futures_error(&self, error: &str) -> VenueError {
        let error_lower = error.to_lowercase();

        if error_lower.contains("rate limit") {
            VenueError::RateLimit {
                retry_after: Some(std::time::Duration::from_secs(60)),
            }
        } else if error_lower.contains("insufficient") || error_lower.contains("margin") {
            VenueError::InsufficientBalance(error.to_string())
        } else if error_lower.contains("not found") || error_lower.contains("unknown order") {
            VenueError::OrderNotFound(error.to_string())
        } else if error_lower.contains("invalid") && error_lower.contains("key") {
            VenueError::Authentication(error.to_string())
        } else if error_lower.contains("invalid") && error_lower.contains("nonce") {
            VenueError::Authentication(error.to_string())
        } else if error_lower.contains("invalid") || error_lower.contains("argument") {
            VenueError::InvalidOrder(error.to_string())
        } else if error_lower.contains("symbol") {
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
        let client = FuturesRestClient::new(
            reqwest::Client::new(),
            Arc::new(KrakenFuturesSigner::new("key", "dGVzdF9zZWNyZXRfa2V5XzEyMzQ1").unwrap()),
            "https://demo-futures.kraken.com",
        );

        let error = client.map_futures_error("Rate limit exceeded");
        assert!(matches!(error, VenueError::RateLimit { .. }));

        let error = client.map_futures_error("Insufficient margin");
        assert!(matches!(error, VenueError::InsufficientBalance(_)));

        let error = client.map_futures_error("Invalid API key");
        assert!(matches!(error, VenueError::Authentication(_)));

        let error = client.map_futures_error("Order not found");
        assert!(matches!(error, VenueError::OrderNotFound(_)));

        let error = client.map_futures_error("Invalid argument");
        assert!(matches!(error, VenueError::InvalidOrder(_)));
    }
}
