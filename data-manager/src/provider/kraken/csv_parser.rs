//! Kraken Time & Sales CSV parser
//!
//! Parses historical trade data from Kraken's CSV export format.
//! Supports side inference using the Lee-Ready tick rule.

use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;
use trading_common::data::types::TradeSide;

use super::symbol::{to_canonical, QUOTE_CURRENCIES};

/// Errors during CSV parsing
#[derive(Error, Debug)]
pub enum CsvParseError {
    #[error("Invalid line format: expected 3 fields, got {0}")]
    InvalidFieldCount(usize),

    #[error("Failed to parse timestamp '{0}': {1}")]
    InvalidTimestamp(String, String),

    #[error("Failed to parse price '{0}': {1}")]
    InvalidPrice(String, String),

    #[error("Failed to parse volume '{0}': {1}")]
    InvalidVolume(String, String),

    #[error("Failed to parse symbol from filename '{0}': {1}")]
    InvalidFilename(String, String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// A single trade record from Kraken CSV
#[derive(Debug, Clone)]
pub struct KrakenCsvTrade {
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// Trade price in quote currency
    pub price: Decimal,
    /// Trade volume in base currency
    pub volume: Decimal,
    /// Inferred trade side (if enabled)
    pub side: TradeSide,
}

/// Lee-Ready tick rule side inferrer
///
/// Infers trade side based on price movement:
/// - Price up → Buy (aggressor bought at higher price)
/// - Price down → Sell (aggressor sold at lower price)
/// - Price same → Inherit previous direction
#[derive(Debug, Clone)]
pub struct SideInferrer {
    last_price: Option<Decimal>,
    last_side: TradeSide,
}

impl Default for SideInferrer {
    fn default() -> Self {
        Self::new()
    }
}

impl SideInferrer {
    /// Create a new side inferrer
    pub fn new() -> Self {
        Self {
            last_price: None,
            last_side: TradeSide::Buy, // Default for first trade
        }
    }

    /// Infer trade side from price using Lee-Ready tick rule
    pub fn infer(&mut self, price: Decimal) -> TradeSide {
        let side = match self.last_price {
            None => TradeSide::Buy, // First trade defaults to Buy
            Some(prev) => {
                if price > prev {
                    TradeSide::Buy
                } else if price < prev {
                    TradeSide::Sell
                } else {
                    self.last_side // Same price, inherit direction
                }
            }
        };

        self.last_price = Some(price);
        self.last_side = side;
        side
    }

    /// Reset the inferrer state (e.g., for a new symbol)
    pub fn reset(&mut self) {
        self.last_price = None;
        self.last_side = TradeSide::Buy;
    }
}

/// Parse a symbol from a Kraken CSV filename
///
/// # Examples
/// - "XBTUSD.csv" → ("BTCUSD", "BTC", "USD")
/// - "ETHUSD.csv" → ("ETHUSD", "ETH", "USD")
/// - "AAVEEUR.csv" → ("AAVEEUR", "AAVE", "EUR")
pub fn parse_filename(filename: &str) -> Result<(String, String, String), CsvParseError> {
    // Remove .csv extension
    let name = filename
        .strip_suffix(".csv")
        .or_else(|| filename.strip_suffix(".CSV"))
        .unwrap_or(filename);

    // Split into base and quote using known quote currencies
    let (base, quote) = split_symbol(name).map_err(|e| {
        CsvParseError::InvalidFilename(filename.to_string(), e)
    })?;

    // Convert to canonical format (XBT → BTC, etc.)
    let canonical = to_canonical(&format!("{}{}", base, quote))
        .map_err(|e| CsvParseError::InvalidFilename(filename.to_string(), e.to_string()))?;

    // Re-split canonical to get normalized base
    let (canonical_base, canonical_quote) = split_symbol(&canonical)
        .map_err(|e| CsvParseError::InvalidFilename(filename.to_string(), e))?;

    Ok((canonical, canonical_base, canonical_quote))
}

/// Split a symbol into base and quote currencies
fn split_symbol(symbol: &str) -> Result<(String, String), String> {
    let symbol = symbol.to_uppercase();

    // Extended quote currencies for Kraken T&S data
    let mut quotes: Vec<&str> = QUOTE_CURRENCIES.to_vec();
    // Add additional quote currencies found in Kraken T&S
    quotes.extend(&["XBT", "ETH", "DOT", "DAI", "CHF", "AED"]);

    // Sort by length descending to match longest first (USDT before USD)
    let mut quotes: Vec<&str> = quotes.iter().copied().collect();
    quotes.sort_by(|a, b| b.len().cmp(&a.len()));

    for quote in quotes {
        if symbol.ends_with(quote) && symbol.len() > quote.len() {
            let base = &symbol[..symbol.len() - quote.len()];
            return Ok((base.to_string(), quote.to_string()));
        }
    }

    Err(format!("Unable to determine quote currency for: {}", symbol))
}

/// Parse a single CSV line into a trade record
///
/// Expected format: timestamp,price,volume (no header)
pub fn parse_line(line: &str, inferrer: &mut Option<SideInferrer>) -> Result<KrakenCsvTrade, CsvParseError> {
    let fields: Vec<&str> = line.split(',').map(|s| s.trim()).collect();

    if fields.len() != 3 {
        return Err(CsvParseError::InvalidFieldCount(fields.len()));
    }

    // Parse timestamp (Unix seconds)
    let timestamp_secs: i64 = fields[0]
        .parse()
        .map_err(|e: std::num::ParseIntError| {
            CsvParseError::InvalidTimestamp(fields[0].to_string(), e.to_string())
        })?;

    let timestamp = Utc
        .timestamp_opt(timestamp_secs, 0)
        .single()
        .ok_or_else(|| {
            CsvParseError::InvalidTimestamp(
                fields[0].to_string(),
                "Invalid Unix timestamp".to_string(),
            )
        })?;

    // Parse price
    let price = Decimal::from_str(fields[1]).map_err(|e| {
        CsvParseError::InvalidPrice(fields[1].to_string(), e.to_string())
    })?;

    // Parse volume
    let volume = Decimal::from_str(fields[2]).map_err(|e| {
        CsvParseError::InvalidVolume(fields[2].to_string(), e.to_string())
    })?;

    // Infer side using Lee-Ready rule (if inferrer provided)
    let side = match inferrer {
        Some(inf) => inf.infer(price),
        None => TradeSide::Unknown,
    };

    Ok(KrakenCsvTrade {
        timestamp,
        price,
        volume,
        side,
    })
}

/// Iterator over trades in a CSV file or reader
pub struct CsvTradeIterator<R: Read> {
    reader: BufReader<R>,
    inferrer: Option<SideInferrer>,
    line_number: usize,
    errors: usize,
    max_errors: usize,
}

impl<R: Read> CsvTradeIterator<R> {
    /// Create a new iterator from a reader
    pub fn new(reader: R, infer_side: bool) -> Self {
        Self {
            reader: BufReader::new(reader),
            inferrer: if infer_side { Some(SideInferrer::new()) } else { None },
            line_number: 0,
            errors: 0,
            max_errors: 100, // Stop after 100 consecutive errors
        }
    }

    /// Get the current line number
    pub fn line_number(&self) -> usize {
        self.line_number
    }

    /// Get the error count
    pub fn error_count(&self) -> usize {
        self.errors
    }
}

impl<R: Read> Iterator for CsvTradeIterator<R> {
    type Item = Result<KrakenCsvTrade, CsvParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = String::new();

        loop {
            line.clear();
            match self.reader.read_line(&mut line) {
                Ok(0) => return None, // EOF
                Ok(_) => {
                    self.line_number += 1;

                    // Skip empty lines
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match parse_line(trimmed, &mut self.inferrer) {
                        Ok(trade) => {
                            self.errors = 0; // Reset consecutive error count
                            return Some(Ok(trade));
                        }
                        Err(e) => {
                            self.errors += 1;
                            if self.errors >= self.max_errors {
                                return Some(Err(e));
                            }
                            // Skip bad lines and continue
                            continue;
                        }
                    }
                }
                Err(e) => return Some(Err(CsvParseError::Io(e))),
            }
        }
    }
}

/// Open a CSV file and return an iterator over trades
pub fn open_csv_file(path: &Path, infer_side: bool) -> Result<CsvTradeIterator<std::fs::File>, CsvParseError> {
    let file = std::fs::File::open(path)?;
    Ok(CsvTradeIterator::new(file, infer_side))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_parse_filename() {
        let (canonical, base, quote) = parse_filename("XBTUSD.csv").unwrap();
        assert_eq!(canonical, "BTCUSD");
        assert_eq!(base, "BTC");
        assert_eq!(quote, "USD");

        let (canonical, base, quote) = parse_filename("ETHUSD.csv").unwrap();
        assert_eq!(canonical, "ETHUSD");
        assert_eq!(base, "ETH");
        assert_eq!(quote, "USD");

        let (canonical, base, quote) = parse_filename("AAVEEUR.csv").unwrap();
        assert_eq!(canonical, "AAVEEUR");
        assert_eq!(base, "AAVE");
        assert_eq!(quote, "EUR");

        let (canonical, base, quote) = parse_filename("ETHXBT.csv").unwrap();
        assert_eq!(canonical, "ETHBTC");
        assert_eq!(base, "ETH");
        assert_eq!(quote, "BTC");
    }

    #[test]
    fn test_parse_line() {
        let mut inferrer = Some(SideInferrer::new());

        let trade = parse_line("1381095255,122.0,0.1", &mut inferrer).unwrap();
        assert_eq!(trade.timestamp.timestamp(), 1381095255);
        assert_eq!(trade.price, dec!(122.0));
        assert_eq!(trade.volume, dec!(0.1));
        assert_eq!(trade.side, TradeSide::Buy); // First trade defaults to Buy
    }

    #[test]
    fn test_side_inference() {
        let mut inferrer = SideInferrer::new();

        // First trade - default Buy
        assert_eq!(inferrer.infer(dec!(100)), TradeSide::Buy);

        // Price up - Buy
        assert_eq!(inferrer.infer(dec!(101)), TradeSide::Buy);

        // Price down - Sell
        assert_eq!(inferrer.infer(dec!(100)), TradeSide::Sell);

        // Price same - inherit (Sell)
        assert_eq!(inferrer.infer(dec!(100)), TradeSide::Sell);

        // Price up - Buy
        assert_eq!(inferrer.infer(dec!(102)), TradeSide::Buy);

        // Price same - inherit (Buy)
        assert_eq!(inferrer.infer(dec!(102)), TradeSide::Buy);
    }

    #[test]
    fn test_parse_line_no_inference() {
        let mut inferrer = None;

        let trade = parse_line("1381095255,122.0,0.1", &mut inferrer).unwrap();
        assert_eq!(trade.side, TradeSide::Unknown);
    }

    #[test]
    fn test_parse_line_invalid() {
        let mut inferrer = Some(SideInferrer::new());

        // Wrong field count
        assert!(parse_line("1381095255,122.0", &mut inferrer).is_err());
        assert!(parse_line("1381095255,122.0,0.1,extra", &mut inferrer).is_err());

        // Invalid timestamp
        assert!(parse_line("invalid,122.0,0.1", &mut inferrer).is_err());

        // Invalid price
        assert!(parse_line("1381095255,invalid,0.1", &mut inferrer).is_err());

        // Invalid volume
        assert!(parse_line("1381095255,122.0,invalid", &mut inferrer).is_err());
    }

    #[test]
    fn test_csv_iterator() {
        let data = "1381095255,122.0,0.1\n1381179030,123.61,0.1\n1381201115,123.9,0.9916\n";
        let iter = CsvTradeIterator::new(data.as_bytes(), true);

        let trades: Vec<_> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(trades.len(), 3);

        assert_eq!(trades[0].price, dec!(122.0));
        assert_eq!(trades[0].side, TradeSide::Buy); // First

        assert_eq!(trades[1].price, dec!(123.61));
        assert_eq!(trades[1].side, TradeSide::Buy); // Price up

        assert_eq!(trades[2].price, dec!(123.9));
        assert_eq!(trades[2].side, TradeSide::Buy); // Price up
    }
}
