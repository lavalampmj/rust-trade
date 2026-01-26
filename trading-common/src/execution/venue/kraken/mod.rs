//! Kraken execution venue implementation.
//!
//! This module provides execution venue implementations for:
//! - **Kraken Spot**: Cryptocurrency trading (no public testnet)
//! - **Kraken Futures**: Perpetual and fixed-maturity futures (has public demo!)
//!
//! # Key Differences from Binance
//!
//! | Aspect | Binance | Kraken |
//! |--------|---------|--------|
//! | **Signing** | HMAC-SHA256 | HMAC-SHA512 with SHA256 pre-hash |
//! | **Secret Format** | Plain string | Base64-encoded |
//! | **Timestamp** | `timestamp` query param | `nonce` in POST body |
//! | **API Key Header** | `x-mbx-apikey` | `API-Key` |
//! | **Signature** | Query param `signature` | Header `API-Sign` |
//! | **Request Method** | GET/POST | POST (all private endpoints) |
//! | **Order ID** | Number | String (OXXXXX-XXXXX-XXXXXX) |
//! | **Symbol Format** | `BTCUSDT` | `XBT/USD` (Spot) or `PI_XBTUSD` (Futures) |
//! | **Order Modify** | Cancel + new | True in-place amendment |
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         Kraken Module                           │
//! ├───────────────────────┬─────────────────────────────────────────┤
//! │        Spot           │              Futures                    │
//! │  KrakenSpotVenue      │        KrakenFuturesVenue              │
//! │  KrakenVenueConfig    │      KrakenFuturesVenueConfig          │
//! │  (no public testnet)  │        (has public demo!)              │
//! └───────────────────────┴─────────────────────────────────────────┘
//!                              │
//!                   ┌──────────┴──────────┐
//!                   │   Common Module     │
//!                   │ - KrakenHmacSigner  │
//!                   │ - KrakenUserStream  │
//!                   │ - Shared types      │
//!                   └─────────────────────┘
//! ```
//!
//! # Quick Start
//!
//! ## Spot Trading
//!
//! ```ignore
//! use trading_common::execution::venue::kraken::{
//!     create_kraken_spot, create_kraken_sandbox,
//! };
//!
//! // Create a Kraken Spot venue
//! let mut venue = create_kraken_spot()?;
//! venue.connect().await?;
//!
//! // Create a sandbox venue for testing (uses production API!)
//! let mut sandbox = create_kraken_sandbox()?;
//! sandbox.connect().await?;
//! ```
//!
//! ## Futures Trading (Recommended for Testing!)
//!
//! ```ignore
//! use trading_common::execution::venue::kraken::{
//!     create_kraken_futures, create_kraken_futures_demo,
//! };
//!
//! // Create a Futures demo venue (REAL testnet - safe to test!)
//! let mut demo = create_kraken_futures_demo()?;
//! demo.connect().await?;
//!
//! // Create a Futures production venue
//! let mut futures = create_kraken_futures()?;
//! futures.connect().await?;
//! ```
//!
//! ## Custom Configuration
//!
//! ```ignore
//! use trading_common::execution::venue::kraken::{
//!     KrakenVenueConfig, KrakenSpotVenue,
//!     KrakenFuturesVenueConfig, KrakenFuturesVenue,
//! };
//!
//! // Spot with custom config
//! let mut config = KrakenVenueConfig::production();
//! config.base.rest.timeout_ms = 5000;
//! let mut spot = KrakenSpotVenue::new(config)?;
//!
//! // Futures with custom config
//! let mut futures_config = KrakenFuturesVenueConfig::demo();
//! futures_config.base.rest.timeout_ms = 5000;
//! let mut futures = KrakenFuturesVenue::new(futures_config)?;
//! ```
//!
//! # Testing Environments
//!
//! | Market | Environment | URL | Notes |
//! |--------|-------------|-----|-------|
//! | **Spot** | Production | `api.kraken.com` | No public testnet |
//! | **Spot** | "Sandbox" | `api.kraken.com` | Uses production API! |
//! | **Futures** | Production | `futures.kraken.com` | Live trading |
//! | **Futures** | Demo | `demo-futures.kraken.com` | Safe testnet! |
//!
//! **Recommendation**: Use Futures Demo for integration testing, then test
//! Spot with small orders at prices far from market.
//!
//! # Environment Variables
//!
//! ```bash
//! # Spot Production
//! export KRAKEN_API_KEY=your_api_key
//! export KRAKEN_API_SECRET=your_base64_secret  # Note: Base64!
//!
//! # Spot "Sandbox" (still uses production)
//! export KRAKEN_SANDBOX_API_KEY=sandbox_key
//! export KRAKEN_SANDBOX_API_SECRET=sandbox_secret
//!
//! # Futures Production
//! export KRAKEN_FUTURES_API_KEY=futures_key
//! export KRAKEN_FUTURES_API_SECRET=futures_secret
//!
//! # Futures Demo (real testnet!)
//! export KRAKEN_FUTURES_DEMO_API_KEY=demo_key
//! export KRAKEN_FUTURES_DEMO_API_SECRET=demo_secret
//! ```
//!
//! # True Order Modification
//!
//! Unlike Binance, Kraken supports true order modification via `AmendOrder`:
//!
//! ```ignore
//! // Modify order price while preserving queue priority!
//! let new_venue_id = venue.modify_order(
//!     &client_order_id,
//!     Some(&venue_order_id),
//!     "BTC/USD",
//!     Some(dec!(50000.00)), // New price
//!     None, // Keep quantity unchanged
//! ).await?;
//! ```

pub mod common;
pub mod config;
pub mod endpoints;
pub mod factory;
pub mod futures;
pub mod spot;

// Re-export commonly used types
pub use config::{KrakenFuturesVenueConfig, KrakenTier, KrakenVenueConfig};
pub use endpoints::{KrakenEndpoints, KrakenFuturesEndpoints};
pub use factory::{
    create_kraken_futures, create_kraken_futures_demo, create_kraken_futures_venue,
    create_kraken_sandbox, create_kraken_spot, create_kraken_venue,
};
pub use futures::KrakenFuturesVenue;
pub use spot::KrakenSpotVenue;

// Re-export common types that users might need
pub use common::{KrakenHmacSigner, KrakenUserStream};
