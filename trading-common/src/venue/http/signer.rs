//! Request signing traits for authenticated API calls.
//!
//! This module provides a venue-agnostic abstraction for request signing.
//! Each venue implements its own signing algorithm (HMAC-SHA256 for Binance,
//! SHA512 for Kraken, ECDSA for Coinbase, etc.).

/// Trait for signing HTTP requests.
///
/// Implementations of this trait handle venue-specific authentication:
/// - Adding timestamp parameters
/// - Computing signatures
/// - Providing API key headers
///
/// # Example
///
/// ```ignore
/// struct HmacSigner {
///     api_key: String,
///     api_secret: String,
/// }
///
/// impl RequestSigner for HmacSigner {
///     fn sign(&self, params: &mut Vec<(String, String)>, timestamp: u64) {
///         params.push(("timestamp".to_string(), timestamp.to_string()));
///         let query = build_query_string(params);
///         let signature = hmac_sha256(&self.api_secret, &query);
///         params.push(("signature".to_string(), signature));
///     }
///
///     fn api_key_header(&self) -> &str {
///         "X-MBX-APIKEY"
///     }
///
///     fn api_key(&self) -> &str {
///         &self.api_key
///     }
/// }
/// ```
pub trait RequestSigner: Send + Sync {
    /// Sign the request parameters.
    ///
    /// This method should:
    /// 1. Add any required timestamp parameters
    /// 2. Compute the signature from existing parameters
    /// 3. Append the signature to the parameters
    ///
    /// # Arguments
    ///
    /// * `params` - Mutable reference to the request parameters
    /// * `timestamp` - Current timestamp in milliseconds
    fn sign(&self, params: &mut Vec<(String, String)>, timestamp: u64);

    /// Returns the header name for the API key.
    ///
    /// For Binance: "X-MBX-APIKEY"
    /// For Kraken: "API-Key"
    /// For Coinbase: "CB-ACCESS-KEY"
    fn api_key_header(&self) -> &str;

    /// Returns the API key value.
    fn api_key(&self) -> &str;

    /// Returns the header name for the signature (if used in headers).
    ///
    /// Some venues like Coinbase put the signature in a header.
    /// Returns None if signature is in query parameters.
    fn signature_header(&self) -> Option<&str> {
        None
    }

    /// Returns additional headers required for authentication.
    ///
    /// Some venues require multiple headers (e.g., timestamp, passphrase).
    fn additional_headers(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// Build a query string from parameters.
///
/// This is a helper function for creating the string to sign.
pub fn build_query_string(params: &[(String, String)]) -> String {
    params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSigner {
        api_key: String,
    }

    impl RequestSigner for MockSigner {
        fn sign(&self, params: &mut Vec<(String, String)>, timestamp: u64) {
            params.push(("timestamp".to_string(), timestamp.to_string()));
            params.push(("signature".to_string(), "mock_signature".to_string()));
        }

        fn api_key_header(&self) -> &str {
            "X-API-KEY"
        }

        fn api_key(&self) -> &str {
            &self.api_key
        }
    }

    #[test]
    fn test_build_query_string() {
        let params = vec![
            ("symbol".to_string(), "BTCUSDT".to_string()),
            ("side".to_string(), "BUY".to_string()),
        ];
        let query = build_query_string(&params);
        assert_eq!(query, "symbol=BTCUSDT&side=BUY");
    }

    #[test]
    fn test_mock_signer() {
        let signer = MockSigner {
            api_key: "test_key".to_string(),
        };

        let mut params = vec![("symbol".to_string(), "BTCUSDT".to_string())];
        signer.sign(&mut params, 1234567890);

        assert_eq!(params.len(), 3);
        assert_eq!(
            params[1],
            ("timestamp".to_string(), "1234567890".to_string())
        );
        assert_eq!(
            params[2],
            ("signature".to_string(), "mock_signature".to_string())
        );
        assert_eq!(signer.api_key(), "test_key");
        assert_eq!(signer.api_key_header(), "X-API-KEY");
    }
}
