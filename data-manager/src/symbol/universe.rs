//! Symbol universe manager
//!
//! Manages the active set of symbols being tracked by the data manager.

use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

use super::SymbolSpec;

/// Symbol universe manager
///
/// Maintains the set of symbols currently being tracked.
/// Thread-safe via RwLock.
pub struct SymbolUniverse {
    /// Active symbols
    symbols: Arc<RwLock<HashSet<SymbolSpec>>>,
    /// Default symbols loaded from config
    default_symbols: Vec<SymbolSpec>,
}

impl SymbolUniverse {
    /// Create a new empty universe
    pub fn new() -> Self {
        Self {
            symbols: Arc::new(RwLock::new(HashSet::new())),
            default_symbols: Vec::new(),
        }
    }

    /// Create with default symbols from config
    pub fn with_defaults(default_symbols: Vec<SymbolSpec>) -> Self {
        let symbols: HashSet<SymbolSpec> = default_symbols.iter().cloned().collect();
        Self {
            symbols: Arc::new(RwLock::new(symbols)),
            default_symbols,
        }
    }

    /// Add a symbol to the universe
    pub fn add(&self, symbol: SymbolSpec) -> bool {
        let mut symbols = self.symbols.write();
        let added = symbols.insert(symbol.clone());
        if added {
            debug!("Added symbol to universe: {}", symbol);
        }
        added
    }

    /// Add multiple symbols
    pub fn add_many(&self, symbols: impl IntoIterator<Item = SymbolSpec>) -> usize {
        let mut universe = self.symbols.write();
        let mut count = 0;
        for symbol in symbols {
            if universe.insert(symbol) {
                count += 1;
            }
        }
        count
    }

    /// Remove a symbol from the universe
    pub fn remove(&self, symbol: &SymbolSpec) -> bool {
        let mut symbols = self.symbols.write();
        let removed = symbols.remove(symbol);
        if removed {
            debug!("Removed symbol from universe: {}", symbol);
        }
        removed
    }

    /// Check if a symbol is in the universe
    pub fn contains(&self, symbol: &SymbolSpec) -> bool {
        self.symbols.read().contains(symbol)
    }

    /// Get all symbols in the universe
    pub fn all(&self) -> Vec<SymbolSpec> {
        self.symbols.read().iter().cloned().collect()
    }

    /// Get symbols for a specific exchange
    pub fn by_exchange(&self, exchange: &str) -> Vec<SymbolSpec> {
        self.symbols
            .read()
            .iter()
            .filter(|s| s.exchange == exchange)
            .cloned()
            .collect()
    }

    /// Get the number of symbols
    pub fn len(&self) -> usize {
        self.symbols.read().len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.symbols.read().is_empty()
    }

    /// Clear all symbols
    pub fn clear(&self) {
        self.symbols.write().clear();
    }

    /// Reset to default symbols
    pub fn reset_to_defaults(&self) {
        let mut symbols = self.symbols.write();
        symbols.clear();
        for symbol in &self.default_symbols {
            symbols.insert(symbol.clone());
        }
    }

    /// Get available exchanges
    pub fn exchanges(&self) -> Vec<String> {
        let mut exchanges: Vec<String> = self
            .symbols
            .read()
            .iter()
            .map(|s| s.exchange.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        exchanges.sort();
        exchanges
    }
}

impl Default for SymbolUniverse {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SymbolUniverse {
    fn clone(&self) -> Self {
        Self {
            symbols: Arc::new(RwLock::new(self.symbols.read().clone())),
            default_symbols: self.default_symbols.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_universe_operations() {
        let universe = SymbolUniverse::new();

        // Add symbols
        assert!(universe.add(SymbolSpec::new("ES", "CME")));
        assert!(universe.add(SymbolSpec::new("NQ", "CME")));
        assert!(!universe.add(SymbolSpec::new("ES", "CME"))); // Duplicate

        assert_eq!(universe.len(), 2);

        // Check containment
        assert!(universe.contains(&SymbolSpec::new("ES", "CME")));
        assert!(!universe.contains(&SymbolSpec::new("BTCUSDT", "BINANCE")));

        // Remove
        assert!(universe.remove(&SymbolSpec::new("ES", "CME")));
        assert!(!universe.remove(&SymbolSpec::new("ES", "CME"))); // Already removed
        assert_eq!(universe.len(), 1);
    }

    #[test]
    fn test_by_exchange() {
        let universe = SymbolUniverse::new();
        universe.add(SymbolSpec::new("ES", "CME"));
        universe.add(SymbolSpec::new("NQ", "CME"));
        universe.add(SymbolSpec::new("BTCUSDT", "BINANCE"));

        let cme_symbols = universe.by_exchange("CME");
        assert_eq!(cme_symbols.len(), 2);

        let binance_symbols = universe.by_exchange("BINANCE");
        assert_eq!(binance_symbols.len(), 1);
    }

    #[test]
    fn test_defaults() {
        let defaults = vec![SymbolSpec::new("ES", "CME"), SymbolSpec::new("NQ", "CME")];
        let universe = SymbolUniverse::with_defaults(defaults);

        assert_eq!(universe.len(), 2);

        // Add more
        universe.add(SymbolSpec::new("BTCUSDT", "BINANCE"));
        assert_eq!(universe.len(), 3);

        // Reset to defaults
        universe.reset_to_defaults();
        assert_eq!(universe.len(), 2);
        assert!(!universe.contains(&SymbolSpec::new("BTCUSDT", "BINANCE")));
    }
}
