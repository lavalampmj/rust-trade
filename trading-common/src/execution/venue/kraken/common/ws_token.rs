//! WebSocket authentication token management for Kraken.
//!
//! Kraken requires a token for authenticated WebSocket streams.
//! Key differences from Binance:
//! - Token is obtained via POST /0/private/GetWebSocketsToken
//! - Token is valid for ~15 minutes before connection
//! - Token does NOT expire once WebSocket is connected (no keepalive needed!)
//! - Token is sent in the subscription message, not the URL

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::execution::venue::error::{VenueError, VenueResult};

use super::KrakenHmacSigner;

/// Token validity duration (conservative: 10 minutes)
const TOKEN_VALIDITY: Duration = Duration::from_secs(600);

/// WebSocket token response from Kraken API.
#[derive(Debug, Clone, serde::Deserialize)]
#[allow(dead_code)]
struct WsTokenResponse {
    token: String,
    #[serde(default)]
    expires: Option<i64>,
}

/// Manages WebSocket authentication tokens for Kraken.
///
/// Unlike Binance's listen key which requires periodic keepalive,
/// Kraken's token doesn't expire once the WebSocket is connected.
///
/// # Example
///
/// ```ignore
/// let token_manager = KrakenWsTokenManager::new(
///     reqwest::Client::new(),
///     signer,
///     "https://api.kraken.com",
/// );
///
/// // Get a token for WebSocket connection
/// let token = token_manager.get_token().await?;
///
/// // Use token in subscription message
/// let sub_msg = json!({
///     "method": "subscribe",
///     "params": {
///         "channel": "executions",
///         "token": token
///     }
/// });
/// ```
pub struct KrakenWsTokenManager {
    /// HTTP client for API calls
    http_client: reqwest::Client,
    /// Request signer
    signer: Arc<KrakenHmacSigner>,
    /// REST API base URL
    rest_url: String,
    /// Cached token with expiry
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

#[derive(Clone)]
struct CachedToken {
    token: String,
    obtained_at: Instant,
}

impl KrakenWsTokenManager {
    /// Create a new token manager.
    ///
    /// # Arguments
    ///
    /// * `http_client` - HTTP client for API calls
    /// * `signer` - Request signer for authentication
    /// * `rest_url` - REST API base URL
    pub fn new(
        http_client: reqwest::Client,
        signer: Arc<KrakenHmacSigner>,
        rest_url: impl Into<String>,
    ) -> Self {
        Self {
            http_client,
            signer,
            rest_url: rest_url.into(),
            cached_token: Arc::new(RwLock::new(None)),
        }
    }

    /// Get a valid WebSocket token.
    ///
    /// Returns a cached token if still valid, otherwise requests a new one.
    pub async fn get_token(&self) -> VenueResult<String> {
        // Check cache first
        {
            let cache = self.cached_token.read().await;
            if let Some(ref cached) = *cache {
                if cached.obtained_at.elapsed() < TOKEN_VALIDITY {
                    debug!("Using cached WebSocket token");
                    return Ok(cached.token.clone());
                }
            }
        }

        // Request new token
        self.request_token().await
    }

    /// Force request a new token (ignores cache).
    pub async fn request_token(&self) -> VenueResult<String> {
        info!("Requesting new Kraken WebSocket token");

        let endpoint = "/0/private/GetWebSocketsToken";
        let nonce = KrakenHmacSigner::generate_nonce();
        let post_data = format!("nonce={}", nonce);

        let signature = self.signer.sign(endpoint, &post_data, nonce);

        let url = format!("{}{}", self.rest_url, endpoint);

        let response = self
            .http_client
            .post(&url)
            .header(self.signer.api_key_header(), self.signer.api_key())
            .header(self.signer.signature_header(), signature)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(post_data)
            .send()
            .await
            .map_err(|e| VenueError::Request(format!("Failed to request WS token: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| VenueError::Request(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            return Err(VenueError::Request(format!(
                "WS token request failed ({}): {}",
                status, body
            )));
        }

        // Parse response
        let api_response: super::types::KrakenResponse<WsTokenResponse> =
            serde_json::from_str(&body).map_err(|e| {
                VenueError::Parse(format!("Failed to parse WS token response: {} - {}", e, body))
            })?;

        if api_response.has_error() {
            return Err(VenueError::Authentication(
                api_response
                    .first_error()
                    .unwrap_or("Unknown error")
                    .to_string(),
            ));
        }

        let token_data = api_response
            .result
            .ok_or_else(|| VenueError::Parse("No token in response".to_string()))?;

        // Cache the token
        {
            let mut cache = self.cached_token.write().await;
            *cache = Some(CachedToken {
                token: token_data.token.clone(),
                obtained_at: Instant::now(),
            });
        }

        info!("Obtained new Kraken WebSocket token");
        Ok(token_data.token)
    }

    /// Clear the cached token.
    pub async fn clear_cache(&self) {
        let mut cache = self.cached_token.write().await;
        *cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_validity_duration() {
        // Token should be valid for 10 minutes (conservative)
        assert_eq!(TOKEN_VALIDITY, Duration::from_secs(600));
    }
}
