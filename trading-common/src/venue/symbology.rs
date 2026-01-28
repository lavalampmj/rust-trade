//! Symbol normalization for venues.
//!
//! This module provides the [`SymbolNormalizer`] trait for converting between
//! venue-specific symbol formats and the canonical DBT format.
//!
//! # DBT Canonical Format
//!
//! The canonical format is `BASE + QUOTE` without separators:
//! - `BTCUSD`, `ETHUSD`, `SOLUSD`
//!
//! # Venue-Specific Formats
//!
//! Different venues use different formats:
//! - **Binance**: `BTCUSDT`, `ETHBUSD` (stablecoin suffixes)
//! - **Kraken Spot**: `BTC/USD`, `XBT/USD` (slash separator, legacy XBT)
//! - **Kraken Futures**: `PI_XBTUSD` (prefix + legacy XBT)
//! - **Databento**: Native DBT format (no conversion)
//!
//! # Example
//!
//! ```ignore
//! use trading_common::venue::SymbolNormalizer;
//!
//! struct MyVenueNormalizer;
//!
//! impl SymbolNormalizer for MyVenueNormalizer {
//!     fn to_canonical(&self, venue_symbol: &str) -> VenueResult<String> {
//!         // MY_BTC_USD -> BTCUSD
//!         Ok(venue_symbol.replace("MY_", "").replace("_", ""))
//!     }
//!
//!     fn to_venue(&self, canonical_symbol: &str) -> VenueResult<String> {
//!         // BTCUSD -> MY_BTC_USD
//!         let base = &canonical_symbol[..3];
//!         let quote = &canonical_symbol[3..];
//!         Ok(format!("MY_{}_{}", base, quote))
//!     }
//!
//!     fn venue_id(&self) -> &str {
//!         "MY_VENUE"
//!     }
//! }
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use super::error::VenueResult;
use crate::data::InstrumentRegistry;

/// Trait for converting between venue-specific and canonical symbol formats.
///
/// All venues that don't use DBT symbology natively must implement this trait
/// to ensure consistent symbol handling across the system.
///
/// # Symbol Registration
///
/// The [`register_symbols`] method pre-registers symbols with the
/// [`InstrumentRegistry`] and caches their IDs for fast synchronous lookup
/// during message processing. This should be called during subscription setup.
#[async_trait]
pub trait SymbolNormalizer: Send + Sync {
    /// Convert a venue-specific symbol to DBT canonical format.
    ///
    /// # Examples
    /// - Binance: `"BTCUSDT"` → `"BTCUSD"`
    /// - Kraken Spot: `"XBT/USD"` → `"BTCUSD"`
    /// - Kraken Futures: `"PI_XBTUSD"` → `"BTCUSD"`
    fn to_canonical(&self, venue_symbol: &str) -> VenueResult<String>;

    /// Convert a DBT canonical symbol to venue-specific format.
    ///
    /// # Examples
    /// - Binance: `"BTCUSD"` → `"BTCUSDT"`
    /// - Kraken Spot: `"BTCUSD"` → `"BTC/USD"`
    /// - Kraken Futures: `"BTCUSD"` → `"PI_XBTUSD"`
    fn to_venue(&self, canonical_symbol: &str) -> VenueResult<String>;

    /// Get the venue identifier for this normalizer.
    ///
    /// This is used for instrument registry lookups and should be consistent
    /// across the system (e.g., "BINANCE", "KRAKEN", "KRAKEN_FUTURES").
    fn venue_id(&self) -> &str;

    /// Pre-register symbols with the instrument registry and cache their IDs.
    ///
    /// Call this during subscription setup to ensure all instrument_ids are
    /// cached for fast synchronous lookup during message normalization.
    ///
    /// # Arguments
    /// * `symbols` - Venue-specific symbols to register (converted to canonical internally)
    /// * `registry` - The instrument registry for persistent ID storage
    ///
    /// # Returns
    /// Map of canonical symbol to instrument_id
    ///
    /// # Default Implementation
    /// Converts each symbol to canonical format, then registers with the registry.
    async fn register_symbols(
        &self,
        symbols: &[String],
        registry: &Arc<InstrumentRegistry>,
    ) -> VenueResult<HashMap<String, u32>> {
        let mut result = HashMap::new();

        for venue_symbol in symbols {
            let canonical = self.to_canonical(venue_symbol)?;
            let id = registry.get_or_register(&canonical, self.venue_id());
            result.insert(canonical, id);
        }

        Ok(result)
    }

    /// Check if this normalizer is for a DBT-native venue.
    ///
    /// DBT-native venues (like Databento) don't need symbol conversion.
    /// This returns `false` by default, meaning conversion is required.
    fn is_native_dbt(&self) -> bool {
        false
    }
}

/// Marker trait for DBT-native venues.
///
/// Venues that implement this trait use DBT symbology natively and
/// don't require symbol conversion. Examples include Databento.
///
/// This trait provides default no-op implementations for [`SymbolNormalizer`].
pub trait NativeDbVenue: Send + Sync {
    /// Get the venue identifier.
    fn venue_id(&self) -> &str;
}

/// Blanket implementation of SymbolNormalizer for DBT-native venues.
#[async_trait]
impl<T: NativeDbVenue> SymbolNormalizer for T {
    fn to_canonical(&self, venue_symbol: &str) -> VenueResult<String> {
        // No conversion needed - already canonical
        Ok(venue_symbol.to_uppercase())
    }

    fn to_venue(&self, canonical_symbol: &str) -> VenueResult<String> {
        // No conversion needed - already in venue format
        Ok(canonical_symbol.to_uppercase())
    }

    fn venue_id(&self) -> &str {
        NativeDbVenue::venue_id(self)
    }

    fn is_native_dbt(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestNormalizer;

    #[async_trait]
    impl SymbolNormalizer for TestNormalizer {
        fn to_canonical(&self, venue_symbol: &str) -> VenueResult<String> {
            // Simple test: remove underscores
            Ok(venue_symbol.replace('_', "").to_uppercase())
        }

        fn to_venue(&self, canonical_symbol: &str) -> VenueResult<String> {
            // Simple test: add underscore after first 3 chars
            if canonical_symbol.len() >= 6 {
                let base = &canonical_symbol[..3];
                let quote = &canonical_symbol[3..];
                Ok(format!("{}_{}", base, quote))
            } else {
                Ok(canonical_symbol.to_uppercase())
            }
        }

        fn venue_id(&self) -> &str {
            "TEST"
        }
    }

    struct TestNativeVenue;

    impl NativeDbVenue for TestNativeVenue {
        fn venue_id(&self) -> &str {
            "DATABENTO"
        }
    }

    #[test]
    fn test_to_canonical() {
        let normalizer = TestNormalizer;
        assert_eq!(normalizer.to_canonical("BTC_USD").unwrap(), "BTCUSD");
        assert!(!normalizer.is_native_dbt());
    }

    #[test]
    fn test_to_venue() {
        let normalizer = TestNormalizer;
        assert_eq!(normalizer.to_venue("BTCUSD").unwrap(), "BTC_USD");
    }

    #[test]
    fn test_native_db_venue() {
        let venue = TestNativeVenue;
        // Uses blanket impl
        assert_eq!(venue.to_canonical("btcusd").unwrap(), "BTCUSD");
        assert_eq!(venue.to_venue("btcusd").unwrap(), "BTCUSD");
        assert!(venue.is_native_dbt());
        assert_eq!(SymbolNormalizer::venue_id(&venue), "DATABENTO");
    }
}
