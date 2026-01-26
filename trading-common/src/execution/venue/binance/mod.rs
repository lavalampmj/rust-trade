//! Binance execution venue implementation.
//!
//! This module provides execution venue implementations for Binance:
//!
//! - **Spot**: [`spot::BinanceSpotVenue`] for Binance.com and Binance.US spot trading
//! - **Futures**: [`futures::BinanceFuturesVenue`] for USDT-M perpetual futures
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                       BinanceVenueConfig                         │
//! │    platform: Com/Us    market_type: Spot/Futures    testnet     │
//! └─────────────────────────────┬───────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      BinanceEndpoints                            │
//! │    REST URL    WS URL    User Data Stream URL                   │
//! └─────────────────────────────┬───────────────────────────────────┘
//!                               │
//!          ┌────────────────────┴────────────────────┐
//!          │                                         │
//!          ▼                                         ▼
//! ┌────────────────────┐                ┌────────────────────┐
//! │  BinanceSpotVenue  │                │BinanceFuturesVenue │
//! │                    │                │                    │
//! │ - ExecutionVenue   │                │ - ExecutionVenue   │
//! │ - OrderSubmission  │                │ - OrderSubmission  │
//! │ - ExecutionStream  │                │ - ExecutionStream  │
//! │ - AccountQuery     │                │ - AccountQuery     │
//! │                    │                │ - Leverage         │
//! │                    │                │ - Positions        │
//! └────────────────────┘                └────────────────────┘
//! ```
//!
//! # Usage
//!
//! ## Quick Start with Factory
//!
//! ```ignore
//! use trading_common::execution::venue::binance::{
//!     create_binance_spot_us, create_binance_usdt_futures,
//! };
//!
//! // Create a Binance.US Spot venue
//! let mut spot_venue = create_binance_spot_us()?;
//! spot_venue.connect().await?;
//!
//! // Create a Binance Futures venue
//! let mut futures_venue = create_binance_usdt_futures()?;
//! futures_venue.connect().await?;
//!
//! // Set leverage before trading
//! futures_venue.set_leverage("BTCUSDT", 10).await?;
//! ```
//!
//! ## With Custom Configuration
//!
//! ```ignore
//! use trading_common::execution::venue::binance::{
//!     BinanceVenueConfig, BinanceSpotVenue,
//! };
//!
//! let mut config = BinanceVenueConfig::spot_com();
//! config.base.rest.timeout_ms = 5000;
//! config.base.auth.api_key_env = "MY_BINANCE_KEY".to_string();
//! config.base.auth.api_secret_env = "MY_BINANCE_SECRET".to_string();
//!
//! let mut venue = BinanceSpotVenue::new(config)?;
//! venue.connect().await?;
//! ```
//!
//! ## Testnet / Sandbox Testing
//!
//! ```ignore
//! use trading_common::execution::venue::binance::create_binance_spot_testnet;
//!
//! let mut venue = create_binance_spot_testnet()?;
//! venue.connect().await?;
//! ```
//!
//! ### Available Testnets
//!
//! | Platform | Testnet Available | URL |
//! |----------|-------------------|-----|
//! | Binance.com Spot | Yes | <https://testnet.binance.vision> |
//! | Binance.com Futures | Yes | <https://testnet.binancefuture.com> |
//! | **Binance.US Spot** | **No** | Uses Binance.com testnet |
//!
//! **Important**: Binance.US does NOT offer its own testnet/sandbox environment.
//! For testing Binance.US integrations, the implementation falls back to the
//! Binance.com Spot testnet. Note that production Binance.US may have different:
//! - Available trading pairs
//! - Rate limits
//! - API restrictions (some endpoints unavailable)
//!
//! For final validation of Binance.US integrations, consider testing with
//! small real orders in production.
//!
//! ### Testnet API Keys
//!
//! Testnet requires separate API keys from production:
//! - Binance.com Spot testnet: <https://testnet.binance.vision/>
//! - Binance Futures testnet: <https://testnet.binancefuture.com/>
//!
//! Set environment variables:
//! ```bash
//! export BINANCE_TESTNET_API_KEY=your_testnet_key
//! export BINANCE_TESTNET_API_SECRET=your_testnet_secret
//! ```

pub mod common;
pub mod config;
pub mod endpoints;
pub mod factory;
pub mod futures;
pub mod spot;

// Re-export commonly used types
pub use config::{BinanceMarketType, BinancePlatform, BinanceVenueConfig};
pub use endpoints::BinanceEndpoints;
pub use factory::{
    create_binance_futures_testnet, create_binance_spot_com, create_binance_spot_testnet,
    create_binance_spot_us, create_binance_usdt_futures, create_binance_venue,
};
pub use futures::{BinanceFuturesVenue, FuturesPositionInfo, MarginType};
pub use spot::BinanceSpotVenue;

// Re-export common types that users might need
pub use common::{BinanceHmacSigner, BinanceUserDataStream};
