//! HMAC-SHA256 request signing for Kraken Futures API.
//!
//! Kraken Futures uses a different signing scheme than Spot:
//! 1. SHA256(postData + nonce + endpointPath)
//! 2. HMAC-SHA512(sha256_hash, base64_decoded_secret)
//! 3. Base64 encode the result
//!
//! Key differences from Spot:
//! - Different header names: `APIKey` and `Authent`
//! - Different hash order: postData + nonce + path (vs nonce + postData)
//! - SHA256 hash of concatenated string (not just nonce + postData)

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::execution::venue::error::{VenueError, VenueResult};

type HmacSha512 = Hmac<Sha512>;

/// HMAC signer for Kraken Futures API.
///
/// # Example
///
/// ```ignore
/// let signer = KrakenFuturesSigner::new("api_key", "base64_encoded_secret")?;
///
/// let nonce = signer.generate_nonce();
/// let post_data = format!("nonce={}&symbol=PI_XBTUSD", nonce);
/// let signature = signer.sign("/derivatives/api/v3/accounts", &post_data, nonce);
///
/// // Use signature in Authent header, API key in APIKey header
/// ```
#[derive(Clone)]
pub struct KrakenFuturesSigner {
    api_key: String,
    api_secret: Vec<u8>, // Decoded from base64
}

impl KrakenFuturesSigner {
    /// Create a new Kraken Futures HMAC signer.
    ///
    /// # Arguments
    ///
    /// * `api_key` - The Kraken Futures API key
    /// * `api_secret_b64` - The Kraken Futures API secret (base64-encoded)
    ///
    /// # Errors
    ///
    /// Returns an error if the secret cannot be decoded from base64.
    pub fn new(api_key: impl Into<String>, api_secret_b64: impl AsRef<str>) -> VenueResult<Self> {
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
    /// Kraken Futures expects nonces in milliseconds (not nanoseconds like Spot).
    /// Using milliseconds ensures compatibility with Kraken's authentication.
    pub fn generate_nonce() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    /// Sign a request for the Kraken Futures API.
    ///
    /// # Arguments
    ///
    /// * `endpoint_path` - The API endpoint path (e.g., "/derivatives/api/v3/accounts")
    ///   Note: The `/derivatives` prefix is stripped for signing purposes.
    /// * `post_data` - The POST body data (URL-encoded)
    /// * `nonce` - The nonce value
    ///
    /// # Returns
    ///
    /// The base64-encoded signature for the `Authent` header.
    pub fn sign(&self, endpoint_path: &str, post_data: &str, nonce: u64) -> String {
        // Strip /derivatives prefix for signing - Kraken expects /api/v3/... in signature
        let sign_path = endpoint_path
            .strip_prefix("/derivatives")
            .unwrap_or(endpoint_path);

        // Step 1: SHA256(postData + nonce + endpointPath)
        // Note: Different order than Spot!
        let mut sha256 = Sha256::new();
        sha256.update(post_data.as_bytes());
        sha256.update(nonce.to_string().as_bytes());
        sha256.update(sign_path.as_bytes());
        let sha256_digest = sha256.finalize();

        // Step 2: HMAC-SHA512(sha256_digest, api_secret)
        let mut hmac =
            HmacSha512::new_from_slice(&self.api_secret).expect("HMAC can take key of any size");
        hmac.update(&sha256_digest);
        let hmac_result = hmac.finalize().into_bytes();

        // Step 3: Base64 encode
        BASE64.encode(hmac_result)
    }

    /// Get the API key.
    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Get the API key header name for Futures.
    pub fn api_key_header(&self) -> &str {
        "APIKey"
    }

    /// Get the signature header name for Futures.
    pub fn signature_header(&self) -> &str {
        "Authent"
    }

    /// Sign a message directly (used for WebSocket challenge authentication).
    ///
    /// For Futures WebSocket, the challenge is signed with SHA256 first.
    ///
    /// # Arguments
    ///
    /// * `message` - The message to sign (e.g., WebSocket challenge string)
    ///
    /// # Returns
    ///
    /// The base64-encoded HMAC-SHA512 signature.
    pub fn sign_challenge(&self, challenge: &str) -> String {
        // For Futures WebSocket challenge, hash the challenge first
        let mut sha256 = Sha256::new();
        sha256.update(challenge.as_bytes());
        let sha256_digest = sha256.finalize();

        let mut hmac =
            HmacSha512::new_from_slice(&self.api_secret).expect("HMAC can take key of any size");
        hmac.update(&sha256_digest);
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
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64);
        assert!(signer.is_ok());

        let signer = signer.unwrap();
        assert_eq!(signer.api_key(), TEST_API_KEY);
    }

    #[test]
    fn test_invalid_base64_secret() {
        let result = KrakenFuturesSigner::new(TEST_API_KEY, "not-valid-base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_nonce_generation() {
        let nonce1 = KrakenFuturesSigner::generate_nonce();
        std::thread::sleep(std::time::Duration::from_micros(1));
        let nonce2 = KrakenFuturesSigner::generate_nonce();

        // Nonces should be strictly increasing
        assert!(nonce2 > nonce1);
    }

    #[test]
    fn test_signature_determinism() {
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let endpoint = "/derivatives/api/v3/accounts";
        let nonce = 1234567890u64;
        let post_data = "nonce=1234567890";

        let sig1 = signer.sign(endpoint, post_data, nonce);
        let sig2 = signer.sign(endpoint, post_data, nonce);

        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_signature_changes_with_data() {
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let endpoint = "/derivatives/api/v3/accounts";
        let nonce = 1234567890u64;

        let sig1 = signer.sign(endpoint, "nonce=1234567890", nonce);
        let sig2 = signer.sign(endpoint, "nonce=1234567890&extra=param", nonce);

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_signature_changes_with_path() {
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let nonce = 1234567890u64;
        let post_data = "nonce=1234567890";

        let sig1 = signer.sign("/derivatives/api/v3/accounts", post_data, nonce);
        let sig2 = signer.sign("/derivatives/api/v3/openorders", post_data, nonce);

        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_futures_headers() {
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        // Futures uses different header names than Spot
        assert_eq!(signer.api_key_header(), "APIKey");
        assert_eq!(signer.signature_header(), "Authent");
    }

    #[test]
    fn test_signature_is_base64() {
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let signature = signer.sign("/derivatives/api/v3/accounts", "nonce=123", 123);

        // Should be valid base64
        let decoded = BASE64.decode(&signature);
        assert!(decoded.is_ok());

        // HMAC-SHA512 produces 64 bytes, base64 encoded
        assert_eq!(decoded.unwrap().len(), 64);
    }

    #[test]
    fn test_sign_challenge() {
        let signer = KrakenFuturesSigner::new(TEST_API_KEY, TEST_API_SECRET_B64).unwrap();

        let challenge = "test_challenge_string_123";
        let signed = signer.sign_challenge(challenge);

        // Should be valid base64
        let decoded = BASE64.decode(&signed);
        assert!(decoded.is_ok());

        // HMAC-SHA512 produces 64 bytes
        assert_eq!(decoded.unwrap().len(), 64);

        // Same input should produce same output
        let signed2 = signer.sign_challenge(challenge);
        assert_eq!(signed, signed2);
    }
}
