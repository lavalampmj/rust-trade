//! Symbol Resolver for translating between different symbol formats.
//!
//! This module provides services for resolving symbols between different
//! representations, including:
//!
//! - Continuous symbols to underlying contracts (ES.c.0 -> ESH6)
//! - Raw symbols to instrument IDs
//! - Provider-specific symbol mappings
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::SymbolResolver;
//!
//! let resolver = SymbolResolver::new(roll_manager.clone());
//!
//! // Resolve continuous symbol to current underlying
//! let underlying = resolver.resolve_continuous("ES.c.0.GLBX")?;
//! println!("ES.c.0.GLBX -> {}", underlying); // e.g., "ESH6.GLBX"
//! ```

use std::sync::Arc;

use super::continuous::ContinuousSymbol;
use super::roll_manager::RollManager;
use crate::orders::InstrumentId;

/// Symbol resolver for translating between different symbol formats.
///
/// Uses cached data from RollManager for continuous contract resolution.
/// For database lookups, use SymbolRegistry directly.
pub struct SymbolResolver {
    /// Roll manager for continuous contract resolution
    roll_manager: Option<Arc<RollManager>>,
}

impl SymbolResolver {
    /// Create a new symbol resolver without roll manager
    pub fn new() -> Self {
        Self { roll_manager: None }
    }

    /// Create with roll manager for continuous contract support
    pub fn with_roll_manager(roll_manager: Arc<RollManager>) -> Self {
        Self {
            roll_manager: Some(roll_manager),
        }
    }

    /// Resolve a symbol string to an InstrumentId
    ///
    /// Handles multiple formats:
    /// - Direct instrument ID: "BTCUSDT.BINANCE"
    /// - Continuous symbol: "ES.c.0.GLBX" -> resolves via roll manager
    pub fn resolve(&self, symbol: &str) -> Result<InstrumentId, ResolverError> {
        // Check if it's a continuous symbol
        if ContinuousSymbol::is_continuous(symbol) {
            return self.resolve_continuous(symbol);
        }

        // Try to parse as instrument ID directly
        if let Some(id) = InstrumentId::from_str(symbol) {
            return Ok(id);
        }

        Err(ResolverError::SymbolNotFound(symbol.to_string()))
    }

    /// Resolve a continuous symbol to its current underlying contract
    ///
    /// # Arguments
    ///
    /// * `continuous_symbol` - The continuous symbol notation (e.g., "ES.c.0.GLBX")
    pub fn resolve_continuous(&self, continuous_symbol: &str) -> Result<InstrumentId, ResolverError> {
        // Parse the continuous symbol to validate format
        let _parsed = ContinuousSymbol::parse(continuous_symbol)
            .map_err(|_| ResolverError::InvalidContinuousSymbol(continuous_symbol.to_string()))?;

        // Try to get current contract from roll manager
        if let Some(ref roll_manager) = self.roll_manager {
            if let Some(contract_id) = roll_manager.get_current_contract(continuous_symbol) {
                return Ok(contract_id);
            }

            // The continuous symbol is valid but not currently tracked
            return Err(ResolverError::ContinuousNotTracked(continuous_symbol.to_string()));
        }

        Err(ResolverError::NoRollManager)
    }

    /// Check if a symbol is a continuous contract notation
    pub fn is_continuous(&self, symbol: &str) -> bool {
        ContinuousSymbol::is_continuous(symbol)
    }

    /// Parse a continuous symbol string
    pub fn parse_continuous(&self, symbol: &str) -> Option<ContinuousSymbol> {
        ContinuousSymbol::parse(symbol).ok()
    }

    /// Get the current underlying contract for a continuous symbol
    ///
    /// Returns None if not tracked or no roll manager configured
    pub fn get_current_contract(&self, continuous_symbol: &str) -> Option<InstrumentId> {
        self.roll_manager
            .as_ref()
            .and_then(|rm| rm.get_current_contract(continuous_symbol))
    }
}

impl Default for SymbolResolver {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors that can occur during symbol resolution
#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolverError {
    #[error("Invalid continuous symbol notation: {0}")]
    InvalidContinuousSymbol(String),

    #[error("Symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("Continuous contract not tracked: {0}")]
    ContinuousNotTracked(String),

    #[error("No roll manager configured for continuous contract resolution")]
    NoRollManager,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_continuous() {
        let resolver = SymbolResolver::new();

        assert!(resolver.is_continuous("ES.c.0.GLBX"));
        assert!(resolver.is_continuous("NQ.v.0.GLBX"));
        assert!(resolver.is_continuous("CL.n.1.NYMX"));
        assert!(!resolver.is_continuous("BTCUSDT"));
        assert!(!resolver.is_continuous("ESH6.GLBX"));
    }

    #[test]
    fn test_resolve_direct_instrument_id() {
        let resolver = SymbolResolver::new();

        let result = resolver.resolve("BTCUSDT.BINANCE");
        assert!(result.is_ok());
        let id = result.unwrap();
        assert_eq!(id.symbol, "BTCUSDT");
        assert_eq!(id.venue, "BINANCE");
    }

    #[test]
    fn test_resolve_continuous_without_roll_manager() {
        let resolver = SymbolResolver::new();

        let result = resolver.resolve("ES.c.0.GLBX");
        assert!(matches!(result, Err(ResolverError::NoRollManager)));
    }

    #[test]
    fn test_parse_continuous() {
        let resolver = SymbolResolver::new();

        let parsed = resolver.parse_continuous("ES.c.0.GLBX");
        assert!(parsed.is_some());
        let symbol = parsed.unwrap();
        assert_eq!(symbol.base_symbol, "ES");
        assert_eq!(symbol.venue, Some("GLBX".to_string()));

        let invalid = resolver.parse_continuous("BTCUSDT");
        assert!(invalid.is_none());
    }

    #[test]
    fn test_resolver_error_display() {
        let invalid = ResolverError::InvalidContinuousSymbol("bad.symbol".to_string());
        assert!(invalid.to_string().contains("Invalid continuous symbol"));

        let not_found = ResolverError::SymbolNotFound("UNKNOWN".to_string());
        assert!(not_found.to_string().contains("Symbol not found"));

        let no_rm = ResolverError::NoRollManager;
        assert!(no_rm.to_string().contains("No roll manager"));
    }
}
