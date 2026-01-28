//! Binance symbol conversion utilities
//!
//! Handles conversion between canonical symbols (e.g., "BTCUSD") and
//! Binance-specific formats (e.g., "BTCUSDT").
//!
//! # Binance Symbol Format
//!
//! Binance uses pairs like `BTCUSDT`, `ETHBUSD`, `BTCBUSD`:
//! - USDT (Tether) is the most common quote currency
//! - BUSD (Binance USD) is another option
//! - Some pairs use USDC, TUSD, etc.
//!
//! # Canonical (DBT) Format
//!
//! The framework uses DBT canonical format internally: `BTCUSD`, `ETHUSD`.
//! This normalizes all USD-like stablecoins to `USD`.

use crate::provider::ProviderError;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Mapping from canonical quote currencies to Binance format
/// Default mapping uses USDT as the standard stablecoin
static TO_BINANCE_QUOTE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("USD", "USDT"), // Canonical USD maps to USDT by default
    ])
});

/// Quote currencies that map to canonical USD
/// All USD-pegged stablecoins normalize to "USD" in canonical format
static USD_STABLECOINS: &[&str] = &["USDT", "BUSD", "USDC", "TUSD", "USDP", "GUSD", "DAI"];

/// Other common quote currencies (non-USD) that we preserve as-is
static OTHER_QUOTE_CURRENCIES: &[&str] = &["BTC", "ETH", "BNB", "EUR", "GBP", "AUD", "TRY"];

/// Convert a canonical symbol to Binance format.
///
/// # Examples
/// - "BTCUSD" -> "BTCUSDT"
/// - "ETHUSD" -> "ETHUSDT"
/// - "BNBUSD" -> "BNBUSDT"
/// - "ETHBTC" -> "ETHBTC" (BTC pairs preserved)
pub fn to_binance(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim().to_uppercase();

    if symbol.is_empty() {
        return Err(ProviderError::Configuration(
            "Symbol cannot be empty".to_string(),
        ));
    }

    // If already in Binance format (ends with stablecoin), return as-is
    for stable in USD_STABLECOINS.iter() {
        if symbol.ends_with(stable) {
            return Ok(symbol);
        }
    }

    // Try to split and convert
    let (base, quote) = split_symbol(&symbol)?;

    // Convert quote currency if it's USD -> USDT
    let binance_quote = TO_BINANCE_QUOTE
        .get(quote.as_str())
        .map(|s| s.to_string())
        .unwrap_or(quote);

    Ok(format!("{}{}", base, binance_quote))
}

/// Convert a Binance symbol to canonical format.
///
/// # Examples
/// - "BTCUSDT" -> "BTCUSD"
/// - "ETHBUSD" -> "ETHUSD"
/// - "SOLUSDC" -> "SOLUSD"
/// - "ETHBTC" -> "ETHBTC" (non-USD pairs preserved)
pub fn to_canonical(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim().to_uppercase();

    if symbol.is_empty() {
        return Err(ProviderError::Configuration(
            "Symbol cannot be empty".to_string(),
        ));
    }

    // Check if symbol ends with a USD stablecoin
    for stable in USD_STABLECOINS.iter() {
        if symbol.ends_with(stable) && symbol.len() > stable.len() {
            let base = &symbol[..symbol.len() - stable.len()];
            return Ok(format!("{}USD", base));
        }
    }

    // Check for other quote currencies - preserve as-is
    for quote in OTHER_QUOTE_CURRENCIES.iter() {
        if symbol.ends_with(quote) && symbol.len() > quote.len() {
            // Already in a format we recognize, return as-is
            return Ok(symbol);
        }
    }

    // Fallback: assume it's already canonical or unknown format
    Ok(symbol)
}

/// Split a symbol into base and quote currencies.
fn split_symbol(symbol: &str) -> Result<(String, String), ProviderError> {
    // Try USD stablecoins first (these map to USD)
    for stable in USD_STABLECOINS.iter() {
        if symbol.ends_with(stable) && symbol.len() > stable.len() {
            let base = &symbol[..symbol.len() - stable.len()];
            return Ok((base.to_string(), stable.to_string()));
        }
    }

    // Try other quote currencies
    for quote in OTHER_QUOTE_CURRENCIES.iter() {
        if symbol.ends_with(quote) && symbol.len() > quote.len() {
            let base = &symbol[..symbol.len() - quote.len()];
            return Ok((base.to_string(), quote.to_string()));
        }
    }

    // Try canonical USD suffix
    if symbol.ends_with("USD") && symbol.len() > 3 {
        let base = &symbol[..symbol.len() - 3];
        return Ok((base.to_string(), "USD".to_string()));
    }

    Err(ProviderError::Configuration(format!(
        "Unable to determine quote currency for symbol: {}",
        symbol
    )))
}

/// Validate a symbol for use with Binance.
///
/// Returns the symbol in uppercase if valid, or an error if invalid.
pub fn validate_symbol(symbol: &str) -> Result<String, ProviderError> {
    let symbol = symbol.trim();

    if symbol.is_empty() {
        return Err(ProviderError::Configuration(
            "Symbol cannot be empty".to_string(),
        ));
    }

    if symbol.len() < 3 {
        return Err(ProviderError::Configuration(format!(
            "Symbol too short (min 3 chars): {}",
            symbol
        )));
    }

    if symbol.len() > 20 {
        return Err(ProviderError::Configuration(format!(
            "Symbol too long (max 20 chars): {}",
            symbol
        )));
    }

    // Binance symbols are alphanumeric only
    for c in symbol.chars() {
        if !c.is_ascii_alphanumeric() {
            return Err(ProviderError::Configuration(format!(
                "Invalid character '{}' in symbol: {}",
                c, symbol
            )));
        }
    }

    Ok(symbol.to_uppercase())
}

/// Get the exchange identifier for Binance.
pub fn get_exchange_name() -> &'static str {
    "BINANCE"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_binance() {
        // Canonical to Binance
        assert_eq!(to_binance("BTCUSD").unwrap(), "BTCUSDT");
        assert_eq!(to_binance("ETHUSD").unwrap(), "ETHUSDT");
        assert_eq!(to_binance("SOLUSD").unwrap(), "SOLUSDT");
        assert_eq!(to_binance("btcusd").unwrap(), "BTCUSDT");
    }

    #[test]
    fn test_to_binance_passthrough() {
        // Already in Binance format
        assert_eq!(to_binance("BTCUSDT").unwrap(), "BTCUSDT");
        assert_eq!(to_binance("ETHBUSD").unwrap(), "ETHBUSD");
        assert_eq!(to_binance("SOLUSDC").unwrap(), "SOLUSDC");
    }

    #[test]
    fn test_to_binance_non_usd_pairs() {
        // Non-USD pairs preserved
        assert_eq!(to_binance("ETHBTC").unwrap(), "ETHBTC");
        assert_eq!(to_binance("SOLETH").unwrap(), "SOLETH");
        assert_eq!(to_binance("BNBBTC").unwrap(), "BNBBTC");
    }

    #[test]
    fn test_to_canonical() {
        // Binance to canonical
        assert_eq!(to_canonical("BTCUSDT").unwrap(), "BTCUSD");
        assert_eq!(to_canonical("ETHUSDT").unwrap(), "ETHUSD");
        assert_eq!(to_canonical("SOLUSDT").unwrap(), "SOLUSD");
    }

    #[test]
    fn test_to_canonical_other_stables() {
        // Other stablecoins also map to USD
        assert_eq!(to_canonical("BTCBUSD").unwrap(), "BTCUSD");
        assert_eq!(to_canonical("ETHUSDC").unwrap(), "ETHUSD");
        assert_eq!(to_canonical("SOLTUSD").unwrap(), "SOLUSD");
    }

    #[test]
    fn test_to_canonical_non_usd() {
        // Non-USD pairs preserved
        assert_eq!(to_canonical("ETHBTC").unwrap(), "ETHBTC");
        assert_eq!(to_canonical("SOLETH").unwrap(), "SOLETH");
        assert_eq!(to_canonical("BNBBTC").unwrap(), "BNBBTC");
    }

    #[test]
    fn test_to_canonical_already_canonical() {
        // Already in canonical format
        assert_eq!(to_canonical("BTCUSD").unwrap(), "BTCUSD");
        assert_eq!(to_canonical("ETHUSD").unwrap(), "ETHUSD");
    }

    #[test]
    fn test_roundtrip() {
        // Canonical -> Binance -> Canonical
        let original = "BTCUSD";
        let binance = to_binance(original).unwrap();
        let back = to_canonical(&binance).unwrap();
        assert_eq!(original, back);

        let original = "ETHUSD";
        let binance = to_binance(original).unwrap();
        let back = to_canonical(&binance).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn test_validate_symbol() {
        assert!(validate_symbol("BTCUSDT").is_ok());
        assert!(validate_symbol("btcusd").is_ok());
        assert!(validate_symbol("ETH").is_ok());
        assert!(validate_symbol("").is_err());
        assert!(validate_symbol("BT").is_err()); // Too short
        assert!(validate_symbol("BTC-USDT").is_err()); // Dash not allowed
        assert!(validate_symbol("BTC/USDT").is_err()); // Slash not allowed
    }

    #[test]
    fn test_split_symbol() {
        let (base, quote) = split_symbol("BTCUSDT").unwrap();
        assert_eq!(base, "BTC");
        assert_eq!(quote, "USDT");

        let (base, quote) = split_symbol("ETHBUSD").unwrap();
        assert_eq!(base, "ETH");
        assert_eq!(quote, "BUSD");

        let (base, quote) = split_symbol("BTCUSD").unwrap();
        assert_eq!(base, "BTC");
        assert_eq!(quote, "USD");

        let (base, quote) = split_symbol("ETHBTC").unwrap();
        assert_eq!(base, "ETH");
        assert_eq!(quote, "BTC");
    }

    #[test]
    fn test_exchange_name() {
        assert_eq!(get_exchange_name(), "BINANCE");
    }

    #[test]
    fn test_case_insensitivity() {
        assert_eq!(to_binance("btcusd").unwrap(), "BTCUSDT");
        assert_eq!(to_binance("BtCuSd").unwrap(), "BTCUSDT");
        assert_eq!(to_canonical("btcusdt").unwrap(), "BTCUSD");
        assert_eq!(to_canonical("BtCuSdT").unwrap(), "BTCUSD");
    }

    #[test]
    fn test_empty_symbol() {
        assert!(to_binance("").is_err());
        assert!(to_canonical("").is_err());
        assert!(validate_symbol("").is_err());
    }
}
