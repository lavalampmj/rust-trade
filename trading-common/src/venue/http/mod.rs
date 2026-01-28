//! HTTP client infrastructure for venues.
//!
//! This module provides venue-agnostic HTTP client components:
//!
//! - [`RequestSigner`]: Trait for request signing (HMAC, ECDSA, etc.)
//! - [`RateLimiter`]: Multi-bucket rate limiting
//! - [`HttpClient`]: Authenticated HTTP client with retry logic
//!
//! # Example
//!
//! ```ignore
//! use trading_common::venue::http::{HttpClient, RateLimiter, RequestSigner};
//!
//! // Create a custom signer for your venue
//! struct MySigner { api_key: String, api_secret: String }
//!
//! impl RequestSigner for MySigner {
//!     // ... implement signing logic
//! }
//!
//! // Create the HTTP client
//! let client = HttpClient::new(
//!     "https://api.example.com",
//!     Box::new(MySigner::new()),
//!     RateLimiter::from_config(&config),
//!     rest_config,
//! )?;
//!
//! // Make authenticated requests
//! let result: MyResponse = client.get_signed("/api/endpoint", &[], 1).await?;
//! ```

mod client;
mod rate_limiter;
mod signer;

pub use client::HttpClient;
pub use rate_limiter::RateLimiter;
pub use signer::{build_query_string, RequestSigner};
