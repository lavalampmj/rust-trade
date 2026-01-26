//! HMAC-SHA512 request signing for Kraken API.
//!
//! Kraken uses a unique signing scheme:
//! 1. SHA256(nonce + POST data)
//! 2. HMAC-SHA512(uri_path + sha256_hash, base64_decoded_secret)
//! 3. Base64 encode the result
//!
//! Key differences from Binance:
//! - Secret is base64-encoded and must be decoded first
//! - Signature goes in the `API-Sign` header (not query params)
//! - Uses `nonce` instead of `timestamp`
//! - All private endpoints use POST

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::execution::venue::error::{VenueError, VenueResult};

type HmacSha512 = Hmac<Sha512>;

/// HMAC-SHA512 request signer for Kraken API.
///
/// # Example
///
/// ```ignore
/// let signer = KrakenHmacSigner::new("api_key", "base64_encoded_secret")?;
///
/// let nonce = signer.generate_nonce();
/// let post_data = format!("nonce={}&pair=XBTUSD", nonce);
/// let signature = signer.sign("/0/private/Balance", &post_data, nonce);
///
/// // Use signature in API-Sign header
/// ```
#[derive(Clone)]
pub struct KrakenHmacSigner {
    api_key: String,
    api_secret: Vec<u8>, // Decoded from base64
}

impl KrakenHmacSigner {
    /// Create a new Kraken HMAC signer.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The Kraken API key
    /// * `api_secret_b64` - The Kraken API secret (base64-encoded)
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be decoded from base64.
    pub fn new(
        api_key: impl Into<String>,
        api_secret_b64: impl AsRef<str>,
    ) -> VenueResult<Self> {
        let api_secret = BASE64
            .decode(api_secret_b64.as_ref())
            .map_err(|e| VenueError::Configuration(format!("Invalid base64 API secret: {}", e)))?;

        Ok(Self {
            api_key: api_key.into(),
            api_secret,
        })
    }

    /// Create a signer from environment variables.
    ///
    /// # Arguments
    ///
    /// * `api_key_env` - Environment variable name for API key
    /// * `api_secret_env` - Environment variable name for API secret (base64)
    ///
    /// # Returns
    ///
    /// Returns None if the environment variables are not set or invalid.
    pub fn from_env(api_key_env: &str, api_secret_env: &str) -> Option<Self> {
        let api_key = std::env::var(api_key_env).ok()?;
        let api_secret_b64 = std::env::var(api_secret_env).ok()?;
        Self::new(api_key, api_secret_b64).ok()
    }

    /// Generate a nonce for request signing.
    ///
    /// Kraken requires strictly increasing nonces. Using nanoseconds
    /// ensures uniqueness across requests.
    pub fn generate_nonce() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    /// Sign a request for the Kraken API.
    ///
    /// # Arguments
    ///
    /// * `uri_path` - The API endpoint path (e.g., "/0/private/Balance")
    /// * `post_data` - The POST body data (URL-encoded)
    /// * `nonce` - The nonce value (must also be in post_data)
    ///
    /// # Returns
    ///
    /// The base64-encoded signature for the `API-Sign` header.
    pub fn sign(&self, uri_path: &str, post_data: &str, nonce: u64) -> String {
        // Step 1: SHA256(nonce + post_data)
        let mut sha256 = Sha256::new();
        sha256.update(nonce.to_string().as_bytes());
        sha256.update(post_data.as_bytes());
        let sha256_digest = sha256.finalize();

        // Step 2: HMAC-SHA512(uri_path + sha256_digest, api_secret)
        let mut hmac = HmacSha512::new_from_slice(&self.api_secret)
            .expect("HMAC can take key of any size");
        hmac.update(uri_path.as_bytes());
        hmac.update(&sha256_digest);
        let hmac_result = hmac.finalize().into_bytes();

        // Step 3: Base64 encode
        BASE64.encode(hmac_result)
    }

    /// Get the API key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the API key header name.
    pub fn api_key_header(&self) -> &str {
        "API-Key"
    }

    /// Get the signature header name.
    pub fn signature_header(&self) -> &str {
        "API-Sign"
    }

    /// Sign a message directly (used for WebSocket challenge authentication).
    ///
    /// This is simpler than the full request signing - just HMAC-SHA512 of the message.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to sign (e.g., WebSocket challenge string)
    ///
    /// # Returns
    ///
    /// The base64-encoded HMAC-SHA512 signature.
    pub fn sign_message(&self, message: &str) -> String {
        let mut hmac = HmacSha512::new_from_slice(&self.api_secret)
            .expect("HMAC can take key of any size");
        hmac.update(message.as_bytes());
        let hmac_result = hmac.finalize().into_bytes();

        BASE64.encode(hmac_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test secret (not a real key, just for testing)
    const TEST_API_KEY: &str = "test_api_key";
    // Base64 encoded "test_secret_key_12345"
    const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzEyMzQ1";

    #[test]
    fn test_signer_creation() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64);
        assert!(signer.is_ok());

        let signer = signer.unwrap();
        assert_eq!(signer.api_key(), TEST_API_KEY);
    }

    #[test]
    fn test_invalid_base64_secret() {
        let result = KrakenHmacSigner::new(TEST_API_KEY, "not-valid-base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_nonce_generation() {
        let nonce1 = KrakenHmacSigner::generate_nonce();
        std::thread::sleep(std::time::Duration::from_micros(1));
        let nonce2 = KrakenHmacSigner::generate_nonce();

        // Nonces should be strictly increasing
        assert!(nonce2 > nonce1);
    }

    #[test]
    fn test_signature_determinism() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let uri_path = "/0/private/Balance";
        let nonce = 1234567890u64;
        let post_data = "nonce=1234567890";

        let sig1 = signer.sign(uri_path, post_data, nonce);
        let sig2 = signer.sign(uri_path, post_data, nonce);

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_signature_changes_with_data() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let uri_path = "/0/private/Balance";
        let nonce = 1234567890u64;

        let sig1 = signer.sign(uri_path, "nonce=1234567890", nonce);
        let sig2 = signer.sign(uri_path, "nonce=1234567890&extra=param", nonce);

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_signature_changes_with_path() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let nonce = 1234567890u64;
        let post_data = "nonce=1234567890";

        let sig1 = signer.sign("/0/private/Balance", post_data, nonce);
        let sig2 = signer.sign("/0/private/OpenOrders", post_data, nonce);

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_headers() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        assert_eq!(signer.api_key_header(), "API-Key");
        assert_eq!(signer.signature_header(), "API-Sign");
    }

    #[test]
    fn test_signature_is_base64() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let signature = signer.sign("/0/private/Balance", "nonce=123", 123);

        // Should be valid base64
        let decoded = BASE64.decode(&signature);
        assert!(decoded.is_ok());

        // HMAC-SHA512 produces 64 bytes, base64 encoded
        assert_eq!(decoded.unwrap().len(), 64);
    }

    #[test]
    fn test_sign_message() {
        let signer = KrakenHmacSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let challenge = "test_challenge_string_123";
        let signed = signer.sign_message(challenge);

        // Should be valid base64
        let decoded = BASE64.decode(&signed);
        assert!(decoded.is_ok());

        // HMAC-SHA512 produces 64 bytes
        assert_eq!(decoded.unwrap().len(), 64);

        // Same input should produce same output
        let signed2 = signer.sign_message(challenge);
        assert_eq!(signed, signed2);

        // Different input should produce different output
        let signed3 = signer.sign_message("different_challenge");
        assert_ne!(signed, signed3);
    }
}
