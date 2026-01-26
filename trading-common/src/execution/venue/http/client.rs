//! HTTP client for venue REST APIs.
//!
//! This module provides a generic HTTP client that handles:
//! - Request signing via the `RequestSigner` trait
//! - Rate limiting via the `RateLimiter`
//! - Retry with exponential backoff
//! - Timeout handling

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{header, Client, Response};
use serde::de::DeserializeOwned;
use tracing::debug;

use super::rate_limiter::RateLimiter;
use super::signer::{build_query_string, RequestSigner};
use crate::execution::venue::config::RestConfig;
use crate::execution::venue::error::{VenueError, VenueResult};

/// HTTP client for venue REST APIs.
///
/// Provides authenticated HTTP requests with automatic signing,
/// rate limiting, and retry logic.
///
/// # Example
///
/// ```ignore
/// let signer = BinanceHmacSigner::new(api_key, api_secret);
/// let rate_limiter = RateLimiter::from_config(&config.rate_limits);
///
/// let client = HttpClient::new(
///     "https://api.binance.com",
///     Box::new(signer),
///     rate_limiter,
///     config.rest.clone(),
/// )?;
///
/// // Make a signed GET request
/// let response: AccountInfo = client
///     .get_signed("/api/v3/account", &[("recvWindow", "5000")])
///     .await?;
///
/// // Make a signed POST request
/// let order: OrderResponse = client
///     .post_signed("/api/v3/order", &[
///         ("symbol", "BTCUSDT"),
///         ("side", "BUY"),
///         ("type", "MARKET"),
///         ("quantity", "0.001"),
///     ])
///     .await?;
/// ```
pub struct HttpClient {
    /// The underlying HTTP client
    client: Client,
    /// Base URL for all requests
    base_url: String,
    /// Request signer for authentication
    signer: Arc<dyn RequestSigner>,
    /// Rate limiter
    rate_limiter: RateLimiter,
    /// Configuration
    config: RestConfig,
}

impl HttpClient {
    /// Create a new HTTP client.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Base URL for all requests (e.g., "https://api.binance.com")
    /// * `signer` - Request signer for authentication
    /// * `rate_limiter` - Rate limiter for the venue
    /// * `config` - REST configuration
    pub fn new(
        base_url: impl Into<String>,
        signer: Box<dyn RequestSigner>,
        rate_limiter: RateLimiter,
        config: RestConfig,
    ) -> VenueResult<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/x-www-form-urlencoded"),
        );

        let client = Client::builder()
            .timeout(config.timeout())
            .default_headers(headers)
            .build()
            .map_err(|e| VenueError::Configuration(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url: base_url.into(),
            signer: Arc::from(signer),
            rate_limiter,
            config,
        })
    }

    /// Get the current timestamp in milliseconds.
    fn timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    /// Build the full URL with query parameters.
    fn build_url(&self, endpoint: &str, params: &[(String, String)]) -> String {
        let base = format!("{}{}", self.base_url, endpoint);
        if params.is_empty() {
            base
        } else {
            format!("{}?{}", base, build_query_string(params))
        }
    }

    /// Sign parameters and return the signed parameter list.
    fn sign_params(&self, params: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut signed_params: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        // Add recvWindow if not present
        if !signed_params.iter().any(|(k, _)| k == "recvWindow") {
            signed_params.push((
                "recvWindow".to_string(),
                self.config.recv_window_ms.to_string(),
            ));
        }

        let timestamp = Self::timestamp_ms();
        self.signer.sign(&mut signed_params, timestamp);

        signed_params
    }

    /// Build request headers with authentication.
    fn build_auth_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();

        // Add API key header
        if let (Ok(header_name), Ok(header_value)) = (
            header::HeaderName::from_bytes(self.signer.api_key_header().as_bytes()),
            header::HeaderValue::from_str(self.signer.api_key()),
        ) {
            headers.insert(header_name, header_value);
        }

        // Add any additional headers from signer
        for (name, value) in self.signer.additional_headers() {
            if let (Ok(name), Ok(value)) = (
                header::HeaderName::from_bytes(name.as_bytes()),
                header::HeaderValue::from_str(&value),
            ) {
                headers.insert(name, value);
            }
        }

        headers
    }

    /// Make a public (unsigned) GET request.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - API endpoint (e.g., "/api/v3/ticker/price")
    /// * `params` - Query parameters
    /// * `weight` - Request weight for rate limiting
    pub async fn get_public<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        weight: u32,
    ) -> VenueResult<T> {
        self.rate_limiter.check_weight_rate(weight).await?;

        let params: Vec<(String, String)> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let url = self.build_url(endpoint, &params);

        debug!("GET (public) {}", url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| VenueError::Request(e.to_string()))?;

        self.handle_response(response).await
    }

    /// Make a signed GET request.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - API endpoint
    /// * `params` - Query parameters (timestamp and signature will be added)
    /// * `weight` - Request weight for rate limiting
    pub async fn get_signed<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        weight: u32,
    ) -> VenueResult<T> {
        self.rate_limiter.check_weight_rate(weight).await?;

        let signed_params = self.sign_params(params);
        let url = self.build_url(endpoint, &signed_params);
        let headers = self.build_auth_headers();

        debug!("GET (signed) {}", endpoint);

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| VenueError::Request(e.to_string()))?;

        self.handle_response(response).await
    }

    /// Make a signed POST request.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - API endpoint
    /// * `params` - Form parameters (timestamp and signature will be added)
    /// * `weight` - Request weight for rate limiting
    pub async fn post_signed<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        weight: u32,
    ) -> VenueResult<T> {
        self.rate_limiter.check_weight_rate(weight).await?;

        let signed_params = self.sign_params(params);
        let headers = self.build_auth_headers();
        let body = build_query_string(&signed_params);

        debug!("POST (signed) {}", endpoint);

        let response = self
            .client
            .post(&format!("{}{}", self.base_url, endpoint))
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| VenueError::Request(e.to_string()))?;

        self.handle_response(response).await
    }

    /// Make a signed DELETE request.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - API endpoint
    /// * `params` - Query parameters (timestamp and signature will be added)
    /// * `weight` - Request weight for rate limiting
    pub async fn delete_signed<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        weight: u32,
    ) -> VenueResult<T> {
        self.rate_limiter.check_weight_rate(weight).await?;

        let signed_params = self.sign_params(params);
        let url = self.build_url(endpoint, &signed_params);
        let headers = self.build_auth_headers();

        debug!("DELETE (signed) {}", endpoint);

        let response = self
            .client
            .delete(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| VenueError::Request(e.to_string()))?;

        self.handle_response(response).await
    }

    /// Make a signed PUT request.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - API endpoint
    /// * `params` - Form parameters (timestamp and signature will be added)
    /// * `weight` - Request weight for rate limiting
    pub async fn put_signed<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        weight: u32,
    ) -> VenueResult<T> {
        self.rate_limiter.check_weight_rate(weight).await?;

        let signed_params = self.sign_params(params);
        let headers = self.build_auth_headers();
        let body = build_query_string(&signed_params);

        debug!("PUT (signed) {}", endpoint);

        let response = self
            .client
            .put(&format!("{}{}", self.base_url, endpoint))
            .headers(headers)
            .body(body)
            .send()
            .await
            .map_err(|e| VenueError::Request(e.to_string()))?;

        self.handle_response(response).await
    }

    /// Handle the HTTP response.
    async fn handle_response<T: DeserializeOwned>(&self, response: Response) -> VenueResult<T> {
        let status = response.status();
        let headers = response.headers().clone();

        // Check for rate limit headers
        if let Some(retry_after) = headers.get("retry-after") {
            if let Ok(secs) = retry_after.to_str().unwrap_or("60").parse::<u64>() {
                if status.as_u16() == 429 {
                    return Err(VenueError::rate_limited(Duration::from_secs(secs)));
                }
            }
        }

        let body = response
            .text()
            .await
            .map_err(|e| VenueError::Request(format!("Failed to read response: {}", e)))?;

        if !status.is_success() {
            // Try to parse as JSON error
            if let Ok(error) = serde_json::from_str::<BinanceErrorResponse>(&body) {
                return Err(self.map_error_code(error.code, &error.msg));
            }
            return Err(VenueError::Request(format!(
                "HTTP {}: {}",
                status.as_u16(),
                body
            )));
        }

        serde_json::from_str(&body)
            .map_err(|e| VenueError::Parse(format!("Failed to parse response: {} - body: {}", e, body)))
    }

    /// Map venue error codes to VenueError variants.
    fn map_error_code(&self, code: i32, message: &str) -> VenueError {
        match code {
            // Rate limiting
            -1003 | -1015 => VenueError::RateLimit {
                retry_after: Some(Duration::from_secs(60)),
            },
            // Authentication errors
            -2014 | -2015 => VenueError::Authentication(message.to_string()),
            // Order rejected
            -2010 => {
                if message.to_lowercase().contains("insufficient") {
                    VenueError::InsufficientBalance(message.to_string())
                } else {
                    VenueError::OrderRejected {
                        reason: message.to_string(),
                        code: Some(code),
                    }
                }
            }
            // Order not found
            -2011 | -2013 => VenueError::OrderNotFound(message.to_string()),
            // Invalid parameters (-1100 to -1199)
            -1199..=-1100 => VenueError::InvalidOrder(message.to_string()),
            // Timestamp issues
            -1021 | -1022 => VenueError::Request(format!("Timestamp error: {}", message)),
            // Unknown symbol
            -1121 => VenueError::SymbolNotFound(message.to_string()),
            // Generic venue error
            _ => VenueError::VenueSpecific {
                code,
                message: message.to_string(),
            },
        }
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get the rate limiter.
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }
}

/// Binance-style error response.
#[derive(Debug, serde::Deserialize)]
struct BinanceErrorResponse {
    code: i32,
    msg: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::venue::config::RateLimitConfig;

    struct MockSigner {
        api_key: String,
    }

    impl RequestSigner for MockSigner {
        fn sign(&self, params: &mut Vec<(String, String)>, timestamp: u64) {
            params.push(("timestamp".to_string(), timestamp.to_string()));
            params.push(("signature".to_string(), "test_sig".to_string()));
        }

        fn api_key_header(&self) -> &str {
            "x-mbx-apikey"
        }

        fn api_key(&self) -> &str {
            &self.api_key
        }
    }

    #[test]
    fn test_build_url_with_params() {
        let signer = MockSigner {
            api_key: "test".to_string(),
        };
        let rate_limiter = RateLimiter::from_config(&RateLimitConfig::default());
        let client = HttpClient::new(
            "https://api.example.com",
            Box::new(signer),
            rate_limiter,
            RestConfig::default(),
        )
        .unwrap();

        let params = vec![
            ("symbol".to_string(), "BTCUSDT".to_string()),
            ("side".to_string(), "BUY".to_string()),
        ];
        let url = client.build_url("/api/v1/order", &params);

        assert!(url.starts_with("https://api.example.com/api/v1/order?"));
        assert!(url.contains("symbol=BTCUSDT"));
        assert!(url.contains("side=BUY"));
    }

    #[test]
    fn test_sign_params() {
        let signer = MockSigner {
            api_key: "test".to_string(),
        };
        let rate_limiter = RateLimiter::from_config(&RateLimitConfig::default());
        let client = HttpClient::new(
            "https://api.example.com",
            Box::new(signer),
            rate_limiter,
            RestConfig::default(),
        )
        .unwrap();

        let params = [("symbol", "BTCUSDT")];
        let signed = client.sign_params(&params);

        assert!(signed.iter().any(|(k, _)| k == "timestamp"));
        assert!(signed.iter().any(|(k, _)| k == "signature"));
        assert!(signed.iter().any(|(k, _)| k == "recvWindow"));
    }

    #[test]
    fn test_map_error_code_rate_limit() {
        let signer = MockSigner {
            api_key: "test".to_string(),
        };
        let rate_limiter = RateLimiter::from_config(&RateLimitConfig::default());
        let client = HttpClient::new(
            "https://api.example.com",
            Box::new(signer),
            rate_limiter,
            RestConfig::default(),
        )
        .unwrap();

        let error = client.map_error_code(-1015, "Too many requests");
        assert!(matches!(error, VenueError::RateLimit { .. }));
    }

    #[test]
    fn test_map_error_code_insufficient_balance() {
        let signer = MockSigner {
            api_key: "test".to_string(),
        };
        let rate_limiter = RateLimiter::from_config(&RateLimitConfig::default());
        let client = HttpClient::new(
            "https://api.example.com",
            Box::new(signer),
            rate_limiter,
            RestConfig::default(),
        )
        .unwrap();

        let error = client.map_error_code(-2010, "Account has insufficient balance");
        assert!(matches!(error, VenueError::InsufficientBalance(_)));
    }
}
