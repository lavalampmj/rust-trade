//! HMAC-SHA256 request signing for Binance API.
//!
//! Binance uses HMAC-SHA256 for request authentication. This module
//! provides the signer implementation that:
//! 1. Adds timestamp parameter
//! 2. Computes HMAC-SHA256 signature of the query string
//! 3. Appends signature to parameters

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::execution::venue::http::{build_query_string, RequestSigner};

type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 request signer for Binance API.
///
/// # Example
///
/// ```ignore
/// let signer = BinanceHmacSigner::new("api_key", "api_secret");
///
/// let mut params = vec![
///     ("symbol".to_string(), "BTCUSDT".to_string()),
///     ("side".to_string(), "BUY".to_string()),
/// ];
///
/// signer.sign(&mut params, 1234567890);
///
/// // params now contains timestamp and signature
/// ```
#[derive(Clone)]
pub struct BinanceHmacSigner {
    api_key: String,
    api_secret: String,
}

impl BinanceHmacSigner {
    /// Create a new Binance HMAC signer.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The Binance API key
    /// * `api_secret` - The Binance API secret
    pub fn new(api_key: impl Into<String>, api_secret: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            api_secret: api_secret.into(),
        }
    }

    /// Create a signer from environment variables.
    ///
    /// # Arguments
    ///
    /// * `api_key_env` - Environment variable name for API key
    /// * `api_secret_env` - Environment variable name for API secret
    ///
    /// # Errors
    ///
    /// Returns None if the environment variables are not set.
    pub fn from_env(api_key_env: &str, api_secret_env: &str) -> Option<Self> {
        let api_key = std::env::var(api_key_env).ok()?;
        let api_secret = std::env::var(api_secret_env).ok()?;
        Some(Self::new(api_key, api_secret))
    }

    /// Compute HMAC-SHA256 signature.
    fn compute_signature(&self, data: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(self.api_secret.as_bytes()).expect("HMAC can take any size");
        mac.update(data.as_bytes());
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }
}

impl RequestSigner for BinanceHmacSigner {
    fn sign(&self, params: &mut Vec<(String, String)>, timestamp: u64) {
        // Add timestamp
        params.push(("timestamp".to_string(), timestamp.to_string()));

        // Build query string without signature
        let query = build_query_string(params);

        // Compute signature
        let signature = self.compute_signature(&query);

        // Append signature
        params.push(("signature".to_string(), signature));
    }

    fn api_key_header(&self) -> &str {
        "x-mbx-apikey"
    }

    fn api_key(&self) -> &str {
        &self.api_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_computation() {
        // Test vector from Binance documentation
        let signer = BinanceHmacSigner::new(
            "vmPUZE6mv9SD5VNHk4HlWFsOr6aKE2zvsw0MuIgwCIPy6utIco14y7Ju91duEh8A",
            "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j",
        );

        // Using the example from Binance docs
        let data = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let signature = signer.compute_signature(data);

        assert_eq!(
            signature,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }

    #[test]
    fn test_sign_params() {
        let signer = BinanceHmacSigner::new("test_key", "test_secret");

        let mut params = vec![
            ("symbol".to_string(), "BTCUSDT".to_string()),
            ("side".to_string(), "BUY".to_string()),
        ];

        signer.sign(&mut params, 1234567890);

        // Should have timestamp and signature added
        assert!(params.iter().any(|(k, v)| k == "timestamp" && v == "1234567890"));
        assert!(params.iter().any(|(k, _)| k == "signature"));

        // Signature should be 64 chars (SHA256 hex)
        let sig = params.iter().find(|(k, _)| k == "signature").unwrap();
        assert_eq!(sig.1.len(), 64);
    }

    #[test]
    fn test_api_key_header() {
        let signer = BinanceHmacSigner::new("key", "secret");
        assert_eq!(signer.api_key_header(), "x-mbx-apikey");
        assert_eq!(signer.api_key(), "key");
    }

    #[test]
    fn test_deterministic_signature() {
        let signer = BinanceHmacSigner::new("key", "secret");

        let mut params1 = vec![("a".to_string(), "1".to_string())];
        let mut params2 = vec![("a".to_string(), "1".to_string())];

        signer.sign(&mut params1, 1000);
        signer.sign(&mut params2, 1000);

        let sig1 = &params1.iter().find(|(k, _)| k == "signature").unwrap().1;
        let sig2 = &params2.iter().find(|(k, _)| k == "signature").unwrap().1;

        assert_eq!(sig1, sig2);
    }
}
