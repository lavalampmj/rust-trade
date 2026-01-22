//! Binance message normalizer
//!
//! Converts Binance-specific message formats to TickData.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::provider::ProviderError;
use trading_common::data::types::{TickData, TradeSide};

use super::types::BinanceTradeMessage;

/// Normalizer for Binance trade data
pub struct BinanceNormalizer {
    /// Sequence counter for ordering
    sequence: AtomicI64,
}

impl BinanceNormalizer {
    /// Create a new normalizer
    pub fn new() -> Self {
        Self {
            sequence: AtomicI64::new(0),
        }
    }

    /// Normalize a Binance trade message to TickData
    pub fn normalize(&self, msg: BinanceTradeMessage) -> Result<TickData, ProviderError> {
        // Parse timestamp
        let ts_event = DateTime::from_timestamp_millis(msg.trade_time as i64)
            .ok_or_else(|| ProviderError::Parse("Invalid timestamp".to_string()))?;

        // Parse price
        let price = Decimal::from_str(&msg.price)
            .map_err(|e| ProviderError::Parse(format!("Invalid price '{}': {}", msg.price, e)))?;

        // Parse quantity
        let quantity = Decimal::from_str(&msg.quantity)
            .map_err(|e| ProviderError::Parse(format!("Invalid quantity '{}': {}", msg.quantity, e)))?;

        // Validate values
        if price <= Decimal::ZERO {
            return Err(ProviderError::Parse("Price must be positive".to_string()));
        }
        if quantity <= Decimal::ZERO {
            return Err(ProviderError::Parse("Quantity must be positive".to_string()));
        }

        // Determine trade side
        // If buyer is maker, seller is taker (SELL)
        // If buyer is not maker, buyer is taker (BUY)
        let side = if msg.is_buyer_maker {
            TradeSide::Sell
        } else {
            TradeSide::Buy
        };

        // Get next sequence number
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);

        Ok(TickData::with_details(
            ts_event,
            Utc::now(),
            msg.symbol,
            "BINANCE".to_string(),
            price,
            quantity,
            side,
            "binance".to_string(),
            msg.trade_id.to_string(),
            msg.is_buyer_maker,
            sequence,
        ))
    }

    /// Reset the sequence counter (for testing)
    #[cfg(test)]
    pub fn reset_sequence(&self) {
        self.sequence.store(0, Ordering::Relaxed);
    }
}

impl Default for BinanceNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate symbol format for Binance
pub fn validate_symbol(symbol: &str) -> Result<String, ProviderError> {
    if symbol.is_empty() {
        return Err(ProviderError::Configuration("Symbol cannot be empty".to_string()));
    }

    let symbol = symbol.to_uppercase();

    // Basic validation: should be alphanumeric and reasonable length
    if !symbol.chars().all(char::is_alphanumeric) {
        return Err(ProviderError::Configuration(format!(
            "Symbol '{}' contains invalid characters",
            symbol
        )));
    }

    if symbol.len() < 3 || symbol.len() > 20 {
        return Err(ProviderError::Configuration(format!(
            "Symbol '{}' has invalid length",
            symbol
        )));
    }

    Ok(symbol)
}

/// Build WebSocket subscription streams for Binance
pub fn build_trade_streams(symbols: &[String]) -> Result<Vec<String>, ProviderError> {
    if symbols.is_empty() {
        return Err(ProviderError::Configuration("No symbols provided".to_string()));
    }

    let mut streams = Vec::with_capacity(symbols.len());

    for symbol in symbols {
        let validated_symbol = validate_symbol(symbol)?;
        streams.push(format!("{}@trade", validated_symbol.to_lowercase()));
    }

    Ok(streams)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_normalize_trade() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "BTCUSDT".to_string(),
            trade_id: 12345,
            price: "50000.00".to_string(),
            quantity: "0.001".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        let tick = normalizer.normalize(msg).unwrap();

        assert_eq!(tick.symbol, "BTCUSDT");
        assert_eq!(tick.exchange, "BINANCE");
        assert_eq!(tick.price, dec!(50000.00));
        assert_eq!(tick.quantity, dec!(0.001));
        assert_eq!(tick.side, TradeSide::Buy);
        assert_eq!(tick.provider, "binance");
        assert_eq!(tick.trade_id, "12345");
        assert!(!tick.is_buyer_maker);
        assert_eq!(tick.sequence, 0);
    }

    #[test]
    fn test_normalize_sell_side() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "ETHUSDT".to_string(),
            trade_id: 67890,
            price: "3000.50".to_string(),
            quantity: "0.1".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: true, // Buyer is maker = Sell
        };

        let tick = normalizer.normalize(msg).unwrap();
        assert_eq!(tick.side, TradeSide::Sell);
        assert!(tick.is_buyer_maker);
    }

    #[test]
    fn test_sequence_increment() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "BTCUSDT".to_string(),
            trade_id: 1,
            price: "50000.00".to_string(),
            quantity: "0.001".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        let tick1 = normalizer.normalize(msg.clone()).unwrap();
        let tick2 = normalizer.normalize(msg).unwrap();

        assert_eq!(tick1.sequence, 0);
        assert_eq!(tick2.sequence, 1);
    }

    #[test]
    fn test_validate_symbol() {
        assert!(validate_symbol("BTCUSDT").is_ok());
        assert!(validate_symbol("btcusdt").is_ok());
        assert!(validate_symbol("").is_err());
        assert!(validate_symbol("BTC-USDT").is_err());
    }

    #[test]
    fn test_build_streams() {
        let symbols = vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        let streams = build_trade_streams(&symbols).unwrap();

        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0], "btcusdt@trade");
        assert_eq!(streams[1], "ethusdt@trade");
    }

    #[test]
    fn test_invalid_price() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "BTCUSDT".to_string(),
            trade_id: 1,
            price: "-100.00".to_string(),
            quantity: "0.001".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        assert!(normalizer.normalize(msg).is_err());
    }
}
