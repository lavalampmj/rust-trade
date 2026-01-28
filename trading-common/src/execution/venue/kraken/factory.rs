//! Factory for creating Kraken execution venues.
//!
//! This module provides factory functions for creating Kraken venues
//! with sensible defaults.
//!
//! # Spot vs Futures
//!
//! - **Spot**: No public testnet. "Sandbox" mode uses production APIs.
//! - **Futures**: Has a fully functional demo environment at `demo-futures.kraken.com`.

use crate::execution::venue::error::VenueResult;
use crate::execution::venue::traits::FullExecutionVenue;

use super::config::{KrakenFuturesVenueConfig, KrakenTier, KrakenVenueConfig};
use super::futures::KrakenFuturesVenue;
use super::spot::KrakenSpotVenue;

/// Create a Kraken venue from configuration.
///
/// This factory creates the appropriate venue based on the configuration.
///
/// # Arguments
///
/// * `config` - The Kraken venue configuration
///
/// # Returns
///
/// A boxed trait object implementing `FullExecutionVenue`.
///
/// # Example
///
/// ```ignore
/// let config = KrakenVenueConfig::production();
/// let venue = create_kraken_venue(config)?;
///
/// venue.connect().await?;
/// let balances = venue.query_balances().await?;
/// ```
pub fn create_kraken_venue(config: KrakenVenueConfig) -> VenueResult<Box<dyn FullExecutionVenue>> {
    let venue = KrakenSpotVenue::new(config)?;
    Ok(Box::new(venue))
}

/// Create a Kraken Spot production venue.
///
/// Uses environment variables:
/// - `KRAKEN_API_KEY`
/// - `KRAKEN_API_SECRET` (base64-encoded)
///
/// # Example
///
/// ```ignore
/// let mut venue = create_kraken_spot()?;
/// venue.connect().await?;
/// ```
pub fn create_kraken_spot() -> VenueResult<KrakenSpotVenue> {
    KrakenSpotVenue::new(KrakenVenueConfig::production())
}

/// Create a Kraken Spot sandbox venue.
///
/// Uses environment variables:
/// - `KRAKEN_SANDBOX_API_KEY`
/// - `KRAKEN_SANDBOX_API_SECRET` (base64-encoded)
///
/// **Note**: Kraken spot doesn't have a public sandbox. This uses the
/// Futures Demo environment for testing authentication patterns.
///
/// # Example
///
/// ```ignore
/// let mut venue = create_kraken_sandbox()?;
/// venue.connect().await?;
/// ```
pub fn create_kraken_sandbox() -> VenueResult<KrakenSpotVenue> {
    KrakenSpotVenue::new(KrakenVenueConfig::sandbox())
}

/// Create a Kraken Spot venue with a specific rate limit tier.
///
/// # Arguments
///
/// * `tier` - The rate limit tier (Starter, Intermediate, Pro)
///
/// # Example
///
/// ```ignore
/// let mut venue = create_kraken_spot_with_tier(KrakenTier::Pro)?;
/// venue.connect().await?;
/// ```
pub fn create_kraken_spot_with_tier(tier: KrakenTier) -> VenueResult<KrakenSpotVenue> {
    KrakenSpotVenue::new(KrakenVenueConfig::production().with_tier(tier))
}

// =============================================================================
// FUTURES FACTORY FUNCTIONS
// =============================================================================

/// Create a Kraken Futures venue from configuration.
///
/// # Example
///
/// ```ignore
/// let config = KrakenFuturesVenueConfig::demo();
/// let venue = create_kraken_futures_venue(config)?;
/// ```
pub fn create_kraken_futures_venue(
    config: KrakenFuturesVenueConfig,
) -> VenueResult<Box<dyn FullExecutionVenue>> {
    let venue = KrakenFuturesVenue::new(config)?;
    Ok(Box::new(venue))
}

/// Create a Kraken Futures production venue.
///
/// Uses environment variables:
/// - `KRAKEN_FUTURES_API_KEY`
/// - `KRAKEN_FUTURES_API_SECRET` (base64-encoded)
///
/// # Example
///
/// ```ignore
/// let mut venue = create_kraken_futures()?;
/// venue.connect().await?;
/// ```
pub fn create_kraken_futures() -> VenueResult<KrakenFuturesVenue> {
    KrakenFuturesVenue::new(KrakenFuturesVenueConfig::production())
}

/// Create a Kraken Futures demo venue.
///
/// **Unlike Spot, Futures has a public demo environment!**
///
/// Uses environment variables:
/// - `KRAKEN_FUTURES_DEMO_API_KEY`
/// - `KRAKEN_FUTURES_DEMO_API_SECRET` (base64-encoded)
///
/// # Example
///
/// ```ignore
/// let mut venue = create_kraken_futures_demo()?;
/// venue.connect().await?;
/// // Safe to test with real orders!
/// ```
pub fn create_kraken_futures_demo() -> VenueResult<KrakenFuturesVenue> {
    KrakenFuturesVenue::new(KrakenFuturesVenueConfig::demo())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::VenueConnection;

    #[test]
    fn test_create_spot_venue() {
        let config = KrakenVenueConfig::production();
        let venue = create_kraken_venue(config);
        assert!(venue.is_ok());
    }

    #[test]
    fn test_create_sandbox_venue() {
        let venue = create_kraken_sandbox();
        assert!(venue.is_ok());

        let venue = venue.unwrap();
        // Kraken has no public spot sandbox - "sandbox" mode uses production API
        assert!(venue.info().display_name.contains("Test Mode"));
    }

    #[test]
    fn test_convenience_functions() {
        assert!(create_kraken_spot().is_ok());
        assert!(create_kraken_sandbox().is_ok());
        assert!(create_kraken_spot_with_tier(KrakenTier::Pro).is_ok());
    }

    #[test]
    fn test_tier_configuration() {
        let venue = create_kraken_spot_with_tier(KrakenTier::Intermediate).unwrap();
        // Venue should be created successfully
        assert!(venue.info().venue_id.contains("kraken"));
    }

    // =========================================================================
    // FUTURES FACTORY TESTS
    // =========================================================================

    #[test]
    fn test_create_futures_venue() {
        let config = KrakenFuturesVenueConfig::production();
        let venue = create_kraken_futures_venue(config);
        assert!(venue.is_ok());
    }

    #[test]
    fn test_create_futures_production() {
        let venue = create_kraken_futures();
        assert!(venue.is_ok());
        let venue = venue.unwrap();
        assert!(venue.info().venue_id.contains("futures"));
        assert!(!venue.info().display_name.contains("Demo"));
    }

    #[test]
    fn test_create_futures_demo() {
        let venue = create_kraken_futures_demo();
        assert!(venue.is_ok());
        let venue = venue.unwrap();
        assert!(venue.info().venue_id.contains("demo"));
        assert!(venue.info().display_name.contains("Demo"));
    }

    #[test]
    fn test_futures_convenience_functions() {
        assert!(create_kraken_futures().is_ok());
        assert!(create_kraken_futures_demo().is_ok());
    }
}
