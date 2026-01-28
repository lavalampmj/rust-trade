//! Kraken symbol conversion utilities
//!
//! Handles conversion between canonical symbols (e.g., "BTCUSD") and
//! Kraken-specific formats for Spot (e.g., "XBT/USD") and Futures (e.g., "PI_XBTUSD").

use crate::provider::ProviderError;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Mapping from common names to Kraken Spot names
/// Kraken uses XBT instead of BTC for Bitcoin
static TO_KRAKEN_SPOT: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("BTC", "XBT"),
        ("DOGE", "XDG"), // Kraken uses XDG for Dogecoin in some contexts
    ])
});

/// Reverse mapping from Kraken names to common names
static FROM_KRAKEN: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("XBT", "BTC"),
        ("XDG", "DOGE"),
    ])
});

/// Common quote currencies for Kraken
static QUOTE_CURRENCIES: &[&str] = &["USD", "EUR", "GBP", "CAD", "JPY", "AUD", "USDT", "USDC"];

/// Convert a canonical symbol to Kraken Spot format.
///
/// # Examples
/// - "BTCUSD" -> "XBT/USD"
/// - "ETHUSD" -> "ETH/USD"
/// - "SOLUSD" -> "SOL/USD"
/// - "XBT/USD" -> "XBT/USD" (already in Kraken format)
pub fn to_kraken_spot(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim().to_uppercase();

    // If already in Kraken format (contains /), validate and return
    if symbol.contains('/') {
        validate_kraken_spot_symbol(&symbol)?;
        return Ok(symbol);
    }

    // Try to split into base and quote
    let (base, quote) = split_symbol(&symbol)?;

    // Convert base currency if needed (BTC -> XBT)
    let kraken_base = TO_KRAKEN_SPOT
        .get(base.as_str())
        .map(|s| s.to_string())
        .unwrap_or(base);

    Ok(format!("{}/{}", kraken_base, quote))
}

/// Convert a canonical symbol to Kraken Futures format.
///
/// # Examples
/// - "BTCUSD" -> "PI_XBTUSD"
/// - "ETHUSD" -> "PI_ETHUSD"
/// - "PI_XBTUSD" -> "PI_XBTUSD" (already in Kraken format)
pub fn to_kraken_futures(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim().to_uppercase();

    // If already in Kraken Futures format (starts with PI_ or PF_), validate and return
    if symbol.starts_with("PI_") || symbol.starts_with("PF_") {
        validate_kraken_futures_symbol(&symbol)?;
        return Ok(symbol);
    }

    // Remove slash if present (convert from Spot format)
    let clean = symbol.replace('/', "");

    // Try to split into base and quote
    let (base, quote) = split_symbol(&clean)?;

    // Convert base currency if needed (BTC -> XBT)
    let kraken_base = TO_KRAKEN_SPOT
        .get(base.as_str())
        .map(|s| s.to_string())
        .unwrap_or(base);

    // PI_ prefix is for perpetual inverse contracts (most common)
    Ok(format!("PI_{}{}", kraken_base, quote))
}

/// Convert a Kraken symbol to canonical format.
///
/// # Examples
/// - "XBT/USD" -> "BTCUSD"
/// - "PI_XBTUSD" -> "BTCUSD"
/// - "ETH/USD" -> "ETHUSD"
pub fn to_canonical(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim().to_uppercase();

    // Handle Futures format (PI_XBTUSD, PF_XBTUSD)
    if symbol.starts_with("PI_") || symbol.starts_with("PF_") {
        let inner = &symbol[3..]; // Remove prefix
        return canonicalize_pair(inner);
    }

    // Handle Spot format (XBT/USD)
    if symbol.contains('/') {
        let parts: Vec<&str> = symbol.split('/').collect();
        if parts.len() != 2 {
            return Err(ProviderError::Configuration(format!(
                "Invalid Kraken symbol format: {}",
                symbol
            )));
        }
        let base = convert_from_kraken(parts[0]);
        let quote = convert_from_kraken(parts[1]);
        return Ok(format!("{}{}", base, quote));
    }

    // Assume it's already canonical or simple format
    canonicalize_pair(&symbol)
}

/// Validate a symbol for use with Kraken.
///
/// Returns the symbol unchanged if valid, or an error if invalid.
pub fn validate_symbol(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim();

    if symbol.is_empty() {
        return Err(ProviderError::Configuration(
            "Symbol cannot be empty".to_string(),
        ));
    }

    if symbol.len() > 20 {
        return Err(ProviderError::Configuration(format!(
            "Symbol too long (max 20 chars): {}",
            symbol
        )));
    }

    // Allow alphanumeric, slash, and underscore
    for c in symbol.chars() {
        if !c.is_ascii_alphanumeric() && c != '/' && c != '_' {
            return Err(ProviderError::Configuration(format!(
                "Invalid character '{}' in symbol: {}",
                c, symbol
            )));
        }
    }

    Ok(symbol.to_uppercase())
}

/// Validate Kraken Spot symbol format (e.g., "XBT/USD")
fn validate_kraken_spot_symbol(symbol: &str) -> Result<(), ProviderError> {
    if !symbol.contains('/') {
        return Err(ProviderError::Configuration(format!(
            "Kraken Spot symbol must contain '/': {}",
            symbol
        )));
    }

    let parts: Vec<&str> = symbol.split('/').collect();
    if parts.len() != 2 {
        return Err(ProviderError::Configuration(format!(
            "Kraken Spot symbol must have exactly one '/': {}",
            symbol
        )));
    }

    if parts[0].is_empty() || parts[1].is_empty() {
        return Err(ProviderError::Configuration(format!(
            "Kraken Spot symbol has empty base or quote: {}",
            symbol
        )));
    }

    Ok(())
}

/// Validate Kraken Futures symbol format (e.g., "PI_XBTUSD")
fn validate_kraken_futures_symbol(symbol: &str) -> Result<(), ProviderError> {
    if !symbol.starts_with("PI_") && !symbol.starts_with("PF_") {
        return Err(ProviderError::Configuration(format!(
            "Kraken Futures symbol must start with 'PI_' or 'PF_': {}",
            symbol
        )));
    }

    let inner = &symbol[3..];
    if inner.len() < 4 {
        return Err(ProviderError::Configuration(format!(
            "Kraken Futures symbol too short: {}",
            symbol
        )));
    }

    Ok(())
}

/// Split a symbol into base and quote currencies.
fn split_symbol(symbol: &str) -> Result<(String, String), ProviderError> {
    // Try each quote currency from longest to shortest
    let mut quotes: Vec<&&str> = QUOTE_CURRENCIES.iter().collect();
    quotes.sort_by(|a, b| b.len().cmp(&a.len()));

    for quote in quotes {
        if symbol.ends_with(*quote) && symbol.len() > quote.len() {
            let base = &symbol[..symbol.len() - quote.len()];
            return Ok((base.to_string(), quote.to_string()));
        }
    }

    Err(ProviderError::Configuration(format!(
        "Unable to determine quote currency for symbol: {}",
        symbol
    )))
}

/// Convert a single currency from Kraken format to common format.
fn convert_from_kraken(currency: &str) -> String {
    FROM_KRAKEN
        .get(currency)
        .map(|s| s.to_string())
        .unwrap_or_else(|| currency.to_string())
}

/// Canonicalize a pair string (XBTUSD -> BTCUSD)
fn canonicalize_pair(pair: &str) -> Result<String, ProviderError> {
    let (base, quote) = split_symbol(pair)?;
    let canonical_base = convert_from_kraken(&base);
    let canonical_quote = convert_from_kraken(&quote);
    Ok(format!("{}{}", canonical_base, canonical_quote))
}

/// Get the exchange identifier for Kraken based on market type.
pub fn get_exchange_name(is_futures: bool) -> &'static str {
    if is_futures {
        "KRAKEN_FUTURES"
    } else {
        "KRAKEN"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_kraken_spot() {
        assert_eq!(to_kraken_spot("BTCUSD").unwrap(), "XBT/USD");
        assert_eq!(to_kraken_spot("ETHUSD").unwrap(), "ETH/USD");
        assert_eq!(to_kraken_spot("SOLUSD").unwrap(), "SOL/USD");
        assert_eq!(to_kraken_spot("btcusd").unwrap(), "XBT/USD");
    }

    #[test]
    fn test_to_kraken_spot_passthrough() {
        // Already in Kraken format
        assert_eq!(to_kraken_spot("XBT/USD").unwrap(), "XBT/USD");
        assert_eq!(to_kraken_spot("ETH/USD").unwrap(), "ETH/USD");
    }

    #[test]
    fn test_to_kraken_futures() {
        assert_eq!(to_kraken_futures("BTCUSD").unwrap(), "PI_XBTUSD");
        assert_eq!(to_kraken_futures("ETHUSD").unwrap(), "PI_ETHUSD");
        assert_eq!(to_kraken_futures("btcusd").unwrap(), "PI_XBTUSD");
    }

    #[test]
    fn test_to_kraken_futures_passthrough() {
        // Already in Kraken format
        assert_eq!(to_kraken_futures("PI_XBTUSD").unwrap(), "PI_XBTUSD");
        assert_eq!(to_kraken_futures("PF_ETHUSD").unwrap(), "PF_ETHUSD");
    }

    #[test]
    fn test_to_canonical() {
        // From Spot format
        assert_eq!(to_canonical("XBT/USD").unwrap(), "BTCUSD");
        assert_eq!(to_canonical("ETH/USD").unwrap(), "ETHUSD");

        // From Futures format
        assert_eq!(to_canonical("PI_XBTUSD").unwrap(), "BTCUSD");
        assert_eq!(to_canonical("PI_ETHUSD").unwrap(), "ETHUSD");
        assert_eq!(to_canonical("PF_XBTUSD").unwrap(), "BTCUSD");
    }

    #[test]
    fn test_validate_symbol() {
        assert!(validate_symbol("BTCUSD").is_ok());
        assert!(validate_symbol("XBT/USD").is_ok());
        assert!(validate_symbol("PI_XBTUSD").is_ok());
        assert!(validate_symbol("").is_err());
        assert!(validate_symbol("BTC-USD").is_err()); // Dash not allowed
    }

    #[test]
    fn test_split_symbol() {
        let (base, quote) = split_symbol("BTCUSD").unwrap();
        assert_eq!(base, "BTC");
        assert_eq!(quote, "USD");

        let (base, quote) = split_symbol("ETHUSDT").unwrap();
        assert_eq!(base, "ETH");
        assert_eq!(quote, "USDT");

        let (base, quote) = split_symbol("XBTUSD").unwrap();
        assert_eq!(base, "XBT");
        assert_eq!(quote, "USD");
    }

    #[test]
    fn test_exchange_name() {
        assert_eq!(get_exchange_name(false), "KRAKEN");
        assert_eq!(get_exchange_name(true), "KRAKEN_FUTURES");
    }

    #[test]
    fn test_various_quote_currencies() {
        assert_eq!(to_kraken_spot("BTCEUR").unwrap(), "XBT/EUR");
        assert_eq!(to_kraken_spot("ETHGBP").unwrap(), "ETH/GBP");
        assert_eq!(to_kraken_spot("SOLJPY").unwrap(), "SOL/JPY");
    }
}
