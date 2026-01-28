//! Kraken data provider
//!
//! This module provides live trade streaming from Kraken WebSocket APIs.
//! Supports both Spot (V2 API) and Futures (V1 API) markets.
//!
//! # Usage
//!
//! ## Spot Market
//! ```no_run
//! use data_manager::provider::kraken::KrakenProvider;
//!
//! let provider = KrakenProvider::spot();
//! ```
//!
//! ## Futures Market
//! ```no_run
//! use data_manager::provider::kraken::KrakenProvider;
//!
//! // Production
//! let provider = KrakenProvider::futures(false);
//!
//! // Demo/Testnet
//! let demo_provider = KrakenProvider::futures(true);
//! ```
//!
//! # Symbol Formats
//!
//! The provider accepts symbols in various formats and converts them internally:
//!
//! | Input | Spot Output | Futures Output |
//! |-------|-------------|----------------|
//! | BTCUSD | XBT/USD | PI_XBTUSD |
//! | ETHUSD | ETH/USD | PI_ETHUSD |
//! | XBT/USD | XBT/USD | PI_XBTUSD |
//!
//! Note: Kraken uses "XBT" instead of "BTC" for Bitcoin.

mod client;
pub mod csv_parser;
mod normalizer;
pub mod symbol;
mod types;

pub use client::{KrakenMarketType, KrakenProvider, KrakenSettings};
pub use csv_parser::{CsvParseError, CsvTradeIterator, KrakenCsvTrade, SideInferrer};
pub use normalizer::KrakenNormalizer;
pub use types::{
    KrakenFuturesSubscribeMessage, KrakenFuturesTradeMessage, KrakenSpotSubscribeMessage,
    KrakenSpotTrade, KrakenSpotTradeMessage,
};
