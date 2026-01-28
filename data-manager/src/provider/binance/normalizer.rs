//! Binance message normalizer
//!
//! Converts Binance-specific message formats to TickData and DBN-native types.
//!
//! # Symbol Conversion
//!
//! Binance symbols (e.g., `BTCUSDT`) are automatically converted to canonical
//! DBT format (e.g., `BTCUSD`) during normalization. This ensures all internal
//! data uses the canonical symbology.
//!
//! # Instrument ID Assignment
//!
//! When an `InstrumentRegistry` is provided, the normalizer uses persistent
//! instrument_ids from the database. Call `register_symbols()` before processing
//! to pre-cache the IDs for all subscribed symbols.
//!
//! # DBN-Native Methods
//!
//! This normalizer provides DBN-native output methods for efficient processing:
//!
//! - `normalize_to_dbn()` - Trade → TradeMsg

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::instruments::InstrumentRegistry;
use crate::provider::ProviderError;
use trading_common::data::types::{TickData, TradeSide};
use trading_common::data::SequenceGenerator;
use trading_common::validation::SymbolValidator;

// DBN types for native output
use trading_common::data::{
    create_trade_msg_with_instrument_id, datetime_to_nanos, TradeMsg, TradeSideCompat,
};

use super::symbol::to_canonical;
use super::types::BinanceTradeMessage;

/// Exchange name for Binance
const BINANCE_EXCHANGE: &str = "BINANCE";

/// Shared symbol validator for Binance (3-20 chars, alphanumeric)
static BINANCE_SYMBOL_VALIDATOR: std::sync::LazyLock<SymbolValidator> =
    std::sync::LazyLock::new(SymbolValidator::binance);

/// Normalizer for Binance trade data
pub struct BinanceNormalizer {
    /// Sequence counter for ordering
    sequence: SequenceGenerator,

    /// Cached instrument_id mappings: canonical_symbol -> instrument_id
    /// Pre-populated by register_symbols() for fast lookup during normalization
    instrument_ids: DashMap<String, u32>,

    /// Optional registry reference for on-demand lookups
    registry: Option<Arc<InstrumentRegistry>>,
}

impl BinanceNormalizer {
    /// Create a new normalizer without registry (uses hash-based IDs)
    pub fn new() -> Self {
        Self {
            sequence: SequenceGenerator::new(),
            instrument_ids: DashMap::new(),
            registry: None,
        }
    }

    /// Create a new normalizer with an instrument registry
    pub fn with_registry(registry: Arc<InstrumentRegistry>) -> Self {
        Self {
            sequence: SequenceGenerator::new(),
            instrument_ids: DashMap::new(),
            registry: Some(registry),
        }
    }

    /// Set the instrument registry
    pub fn set_registry(&mut self, registry: Arc<InstrumentRegistry>) {
        self.registry = Some(registry);
    }

    /// Pre-register symbols with the registry and cache their IDs
    ///
    /// Call this before processing messages to ensure all instrument_ids
    /// are cached for fast synchronous lookup during normalization.
    ///
    /// # Arguments
    /// * `symbols` - Binance symbols to register (e.g., ["BTCUSDT", "ETHUSDT"])
    ///
    /// # Returns
    /// Map of canonical symbol to instrument_id
    pub async fn register_symbols(
        &self,
        symbols: &[String],
    ) -> Result<HashMap<String, u32>, ProviderError> {
        let registry = self.registry.as_ref().ok_or_else(|| {
            ProviderError::Configuration("No instrument registry configured".to_string())
        })?;

        let mut result = HashMap::new();

        for symbol in symbols {
            // Convert to canonical format
            let canonical = to_canonical(symbol)?;

            // Get or create instrument_id from registry
            let id = registry
                .get_or_create(&canonical, BINANCE_EXCHANGE)
                .await
                .map_err(|e| ProviderError::Internal(format!("Registry error: {}", e)))?;

            // Cache for fast lookup
            self.instrument_ids.insert(canonical.clone(), id);
            result.insert(canonical, id);
        }

        Ok(result)
    }

    /// Get cached instrument_id for a canonical symbol
    ///
    /// Falls back to hash-based ID if not in cache
    fn get_instrument_id(&self, canonical_symbol: &str) -> u32 {
        if let Some(id) = self.instrument_ids.get(canonical_symbol) {
            return *id;
        }

        // Fallback: generate hash-based ID (for backward compatibility)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        canonical_symbol.hash(&mut hasher);
        BINANCE_EXCHANGE.hash(&mut hasher);
        (hasher.finish() >> 32) as u32
    }

    /// Normalize a Binance trade message to TickData
    ///
    /// Automatically converts Binance symbol format (e.g., `BTCUSDT`) to
    /// canonical DBT format (e.g., `BTCUSD`).
    pub fn normalize(&self, msg: BinanceTradeMessage) -> Result<TickData, ProviderError> {
        // Parse timestamp
        let ts_event = DateTime::from_timestamp_millis(msg.trade_time as i64)
            .ok_or_else(|| ProviderError::Parse("Invalid timestamp".to_string()))?;

        // Parse price
        let price = Decimal::from_str(&msg.price)
            .map_err(|e| ProviderError::Parse(format!("Invalid price '{}': {}", msg.price, e)))?;

        // Parse quantity
        let quantity = Decimal::from_str(&msg.quantity).map_err(|e| {
            ProviderError::Parse(format!("Invalid quantity '{}': {}", msg.quantity, e))
        })?;

        // Validate values
        if price <= Decimal::ZERO {
            return Err(ProviderError::Parse("Price must be positive".to_string()));
        }
        if quantity <= Decimal::ZERO {
            return Err(ProviderError::Parse(
                "Quantity must be positive".to_string(),
            ));
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
        let sequence = self.sequence.next();

        // Convert Binance symbol to canonical format (BTCUSDT -> BTCUSD)
        let canonical_symbol = to_canonical(&msg.symbol)?;

        Ok(TickData::with_details(
            ts_event,
            Utc::now(),
            canonical_symbol,
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
        self.sequence.reset();
    }

    // ========================================================================
    // DBN-Native Trade Normalization
    // ========================================================================

    /// Normalize a Binance trade message to DBN TradeMsg
    ///
    /// This produces a native `dbn::TradeMsg` without intermediate TickData conversion,
    /// which is more efficient for DBN-native pipelines.
    ///
    /// Automatically converts Binance symbol format (e.g., `BTCUSDT`) to
    /// canonical DBT format (e.g., `BTCUSD`).
    ///
    /// Uses persistent instrument_id from InstrumentRegistry if available,
    /// otherwise falls back to hash-based ID generation.
    pub fn normalize_to_dbn(&self, msg: BinanceTradeMessage) -> Result<TradeMsg, ProviderError> {
        // Parse timestamp
        let ts_event = DateTime::from_timestamp_millis(msg.trade_time as i64)
            .ok_or_else(|| ProviderError::Parse("Invalid timestamp".to_string()))?;

        // Parse price
        let price = Decimal::from_str(&msg.price)
            .map_err(|e| ProviderError::Parse(format!("Invalid price '{}': {}", msg.price, e)))?;

        // Parse quantity
        let quantity = Decimal::from_str(&msg.quantity).map_err(|e| {
            ProviderError::Parse(format!("Invalid quantity '{}': {}", msg.quantity, e))
        })?;

        // Validate values
        if price <= Decimal::ZERO {
            return Err(ProviderError::Parse("Price must be positive".to_string()));
        }
        if quantity <= Decimal::ZERO {
            return Err(ProviderError::Parse(
                "Quantity must be positive".to_string(),
            ));
        }

        // Determine trade side
        // If buyer is maker, seller is taker (SELL)
        // If buyer is not maker, buyer is taker (BUY)
        let side = if msg.is_buyer_maker {
            TradeSideCompat::Sell
        } else {
            TradeSideCompat::Buy
        };

        // Get next sequence number
        let sequence = self.sequence.next();

        // Convert Binance symbol to canonical format (BTCUSDT -> BTCUSD)
        let canonical_symbol = to_canonical(&msg.symbol)?;

        // Get instrument_id (cached or hash-based fallback)
        let instrument_id = self.get_instrument_id(&canonical_symbol);

        let ts_nanos = datetime_to_nanos(ts_event);
        let ts_recv_nanos = datetime_to_nanos(Utc::now());

        Ok(create_trade_msg_with_instrument_id(
            ts_nanos,
            ts_recv_nanos,
            instrument_id,
            price,
            quantity,
            side,
            sequence as u32,
        ))
    }

    /// Check if registry is configured
    pub fn has_registry(&self) -> bool {
        self.registry.is_some()
    }

    /// Get the number of cached instrument IDs
    pub fn cached_instruments(&self) -> usize {
        self.instrument_ids.len()
    }
}

impl Default for BinanceNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate and normalize symbol format for Binance.
///
/// Converts to uppercase and validates (3-20 chars, alphanumeric).
pub fn validate_symbol(symbol: &str) -> Result<String, ProviderError> {
    BINANCE_SYMBOL_VALIDATOR
        .normalize(symbol)
        .map_err(|e| ProviderError::Configuration(e.to_string()))
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

        // Symbol is converted to canonical format (BTCUSDT -> BTCUSD)
        assert_eq!(tick.symbol, "BTCUSD");
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
        // Symbol is converted to canonical format (ETHUSDT -> ETHUSD)
        assert_eq!(tick.symbol, "ETHUSD");
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
    fn test_symbol_conversion_busd() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "BTCBUSD".to_string(), // BUSD stablecoin
            trade_id: 12345,
            price: "50000.00".to_string(),
            quantity: "0.001".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        let tick = normalizer.normalize(msg).unwrap();
        // BTCBUSD should be converted to BTCUSD (canonical)
        assert_eq!(tick.symbol, "BTCUSD");
    }

    #[test]
    fn test_symbol_conversion_usdc() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "ETHUSDC".to_string(), // USDC stablecoin
            trade_id: 12345,
            price: "3000.00".to_string(),
            quantity: "0.1".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        let tick = normalizer.normalize(msg).unwrap();
        // ETHUSDC should be converted to ETHUSD (canonical)
        assert_eq!(tick.symbol, "ETHUSD");
    }

    #[test]
    fn test_symbol_non_usd_pair() {
        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "ETHBTC".to_string(), // Non-USD pair
            trade_id: 12345,
            price: "0.05".to_string(),
            quantity: "0.1".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        let tick = normalizer.normalize(msg).unwrap();
        // ETHBTC should remain as-is (not a USD pair)
        assert_eq!(tick.symbol, "ETHBTC");
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

    // ========================================================================
    // DBN-Native Trade Tests
    // ========================================================================

    #[test]
    fn test_normalize_to_dbn() {
        use trading_common::data::TradeMsgExt;

        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "BTCUSDT".to_string(),
            trade_id: 12345,
            price: "50000.00".to_string(),
            quantity: "0.001".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        let trade_msg = normalizer.normalize_to_dbn(msg).unwrap();

        assert_eq!(trade_msg.price_decimal(), dec!(50000.00));
        assert!(trade_msg.is_buy());
        assert!(trade_msg.hd.ts_event > 0);
    }

    #[test]
    fn test_normalize_to_dbn_sell() {
        use trading_common::data::TradeMsgExt;

        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "ETHUSDT".to_string(),
            trade_id: 67890,
            price: "3000.50".to_string(),
            quantity: "0.1".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: true, // Buyer is maker = Sell
        };

        let trade_msg = normalizer.normalize_to_dbn(msg).unwrap();

        assert!(trade_msg.is_sell());
        assert_eq!(trade_msg.price_decimal(), dec!(3000.50));
    }

    #[test]
    fn test_dbn_parity_with_tick_data() {
        use trading_common::data::TradeMsgExt;

        let normalizer = BinanceNormalizer::new();

        let msg = BinanceTradeMessage {
            symbol: "BTCUSDT".to_string(),
            trade_id: 12345,
            price: "50000.00".to_string(),
            quantity: "0.001".to_string(),
            trade_time: 1672515782136,
            is_buyer_maker: false,
        };

        // Normalize to TickData
        let tick = normalizer.normalize(msg.clone()).unwrap();

        // Reset and normalize to DBN
        normalizer.reset_sequence();
        let trade_msg = normalizer.normalize_to_dbn(msg).unwrap();

        // Verify parity: same price, side
        assert_eq!(tick.price, trade_msg.price_decimal());
        assert_eq!(tick.side == TradeSide::Buy, trade_msg.is_buy());
    }
}
