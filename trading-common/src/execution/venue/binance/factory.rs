//! Factory for creating Binance execution venues.
//!
//! This module provides a factory function for creating the appropriate
//! Binance venue based on configuration.

use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::traits::FullExecutionVenue;

use super::config::{BinanceMarketType, BinanceVenueConfig};
use super::futures::BinanceFuturesVenue;
use super::spot::BinanceSpotVenue;

/// Create a Binance venue from configuration.
///
/// This factory creates the appropriate venue type (spot or futures)
/// based on the configuration.
///
/// # Arguments
///
/// * `config` - The Binance venue configuration
///
/// # Returns
///
/// A boxed trait object implementing `FullExecutionVenue`.
///
/// # Example
///
/// ```ignore
/// let config = BinanceVenueConfig::spot_us();
/// let venue = create_binance_venue(config)?;
///
/// venue.connect().await?;
/// let balances = venue.query_balances().await?;
/// ```
pub fn create_binance_venue(
    config: BinanceVenueConfig,
) -> VenueResult<Box<dyn FullExecutionVenue>> {
    match config.market_type {
        BinanceMarketType::Spot => {
            let venue = BinanceSpotVenue::new(config)?;
            Ok(Box::new(venue))
        }
        BinanceMarketType::UsdtFutures => {
            let venue = BinanceFuturesVenue::new(config)?;
            Ok(Box::new(venue))
        }
        BinanceMarketType::CoinFutures => Err(VenueError::Configuration(
            "COIN-M Futures not yet supported".to_string(),
        )),
    }
}

/// Create a Binance Spot venue for the international platform.
pub fn create_binance_spot_com() -> VenueResult<BinanceSpotVenue> {
    BinanceSpotVenue::new(BinanceVenueConfig::spot_com())
}

/// Create a Binance Spot venue for the US platform.
pub fn create_binance_spot_us() -> VenueResult<BinanceSpotVenue> {
    BinanceSpotVenue::new(BinanceVenueConfig::spot_us())
}

/// Create a Binance USDT-M Futures venue.
pub fn create_binance_usdt_futures() -> VenueResult<BinanceFuturesVenue> {
    BinanceFuturesVenue::new(BinanceVenueConfig::usdt_futures())
}

/// Create a Binance Spot testnet venue.
pub fn create_binance_spot_testnet() -> VenueResult<BinanceSpotVenue> {
    BinanceSpotVenue::new(BinanceVenueConfig::spot_com().with_testnet(true))
}

/// Create a Binance Futures testnet venue.
pub fn create_binance_futures_testnet() -> VenueResult<BinanceFuturesVenue> {
    BinanceFuturesVenue::new(BinanceVenueConfig::usdt_futures().with_testnet(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_spot_venue() {
        let config = BinanceVenueConfig::spot_com();
        let venue = create_binance_venue(config);
        assert!(venue.is_ok());
    }

    #[test]
    fn test_create_futures_venue() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = create_binance_venue(config);
        assert!(venue.is_ok());
    }

    #[test]
    fn test_coin_futures_not_supported() {
        let mut config = BinanceVenueConfig::default();
        config.market_type = BinanceMarketType::CoinFutures;

        let result = create_binance_venue(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_convenience_functions() {
        assert!(create_binance_spot_com().is_ok());
        assert!(create_binance_spot_us().is_ok());
        assert!(create_binance_usdt_futures().is_ok());
        assert!(create_binance_spot_testnet().is_ok());
        assert!(create_binance_futures_testnet().is_ok());
    }
}
