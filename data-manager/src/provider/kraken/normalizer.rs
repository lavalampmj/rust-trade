//! Kraken message normalizer
//!
//! Converts Kraken-specific message formats to TickData, QuoteTick, OrderBook,
//! and DBN-native types (TradeMsg, BboMsg) for both Spot and Futures markets.
//!
//! # Instrument ID Assignment
//!
//! When an `InstrumentRegistry` is provided, the normalizer uses persistent
//! instrument_ids from the database. Call `register_symbols()` before processing
//! to pre-cache the IDs for all subscribed symbols.
//!
//! # DBN-Native Methods
//!
//! This normalizer provides DBN-native output methods that produce `dbn::TradeMsg`
//! and `dbn::BboMsg` directly, avoiding intermediate type conversions:
//!
//! - `normalize_spot_to_dbn()` - Spot trade → TradeMsg
//! - `normalize_futures_to_dbn()` - Futures trade → TradeMsg
//! - `normalize_spot_ticker_to_dbn()` - Spot ticker → BboMsg
//! - `normalize_futures_ticker_to_dbn()` - Futures ticker → BboMsg

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;

use crate::instruments::InstrumentRegistry;
use crate::provider::ProviderError;
use trading_common::data::orderbook::{BookAction, BookSide, OrderBook, OrderBookDelta};
use trading_common::data::quotes::QuoteTick;
use trading_common::data::types::{TickData, TradeSide};
use trading_common::data::SequenceGenerator;

// DBN types for native output
use trading_common::data::{
    create_bbo_msg_from_decimals, create_trade_msg_with_instrument_id, datetime_to_nanos, BboMsg,
    TradeMsg, TradeSideCompat,
};

use super::symbol::{get_exchange_name, to_canonical, to_kraken_futures, to_kraken_spot};
use super::types::{
    KrakenFuturesBookSnapshotMessage, KrakenFuturesBookUpdateMessage, KrakenFuturesTickerMessage,
    KrakenFuturesTradeMessage, KrakenSpotBook, KrakenSpotTicker, KrakenSpotTrade,
};
use crate::provider::traits::SymbolNormalizer;

/// Normalizer for Kraken trade data
pub struct KrakenNormalizer {
    /// Sequence counter for ordering
    sequence: SequenceGenerator,

    /// Whether this is a futures normalizer
    is_futures: bool,

    /// Cached instrument_id mappings: canonical_symbol -> instrument_id
    instrument_ids: DashMap<String, u32>,

    /// Optional registry reference for on-demand lookups
    registry: Option<Arc<InstrumentRegistry>>,
}

impl KrakenNormalizer {
    /// Create a new normalizer for Spot
    pub fn spot() -> Self {
        Self {
            sequence: SequenceGenerator::new(),
            is_futures: false,
            instrument_ids: DashMap::new(),
            registry: None,
        }
    }

    /// Create a new normalizer for Futures
    pub fn futures() -> Self {
        Self {
            sequence: SequenceGenerator::new(),
            is_futures: true,
            instrument_ids: DashMap::new(),
            registry: None,
        }
    }

    /// Create a new Spot normalizer with an instrument registry
    pub fn spot_with_registry(registry: Arc<InstrumentRegistry>) -> Self {
        Self {
            sequence: SequenceGenerator::new(),
            is_futures: false,
            instrument_ids: DashMap::new(),
            registry: Some(registry),
        }
    }

    /// Create a new Futures normalizer with an instrument registry
    pub fn futures_with_registry(registry: Arc<InstrumentRegistry>) -> Self {
        Self {
            sequence: SequenceGenerator::new(),
            is_futures: true,
            instrument_ids: DashMap::new(),
            registry: Some(registry),
        }
    }

    /// Set the instrument registry
    pub fn set_registry(&mut self, registry: Arc<InstrumentRegistry>) {
        self.registry = Some(registry);
    }

    /// Get the exchange name for this normalizer
    fn exchange_name(&self) -> &'static str {
        get_exchange_name(self.is_futures)
    }

    /// Pre-register symbols with the registry and cache their IDs
    ///
    /// Call this before processing messages to ensure all instrument_ids
    /// are cached for fast synchronous lookup during normalization.
    ///
    /// # Arguments
    /// * `symbols` - Kraken symbols to register (Spot: "BTC/USD", Futures: "PI_XBTUSD")
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

        let exchange = self.exchange_name();
        let mut result = HashMap::new();

        for symbol in symbols {
            // Convert to canonical format
            let canonical = to_canonical(symbol)?;

            // Get or create instrument_id from registry
            let id = registry
                .get_or_create(&canonical, exchange)
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
        self.exchange_name().hash(&mut hasher);
        (hasher.finish() >> 32) as u32
    }

    /// Check if registry is configured
    pub fn has_registry(&self) -> bool {
        self.registry.is_some()
    }

    /// Get the number of cached instrument IDs
    pub fn cached_instruments(&self) -> usize {
        self.instrument_ids.len()
    }

    /// Normalize a Kraken Spot trade to TickData
    pub fn normalize_spot(&self, trade: KrakenSpotTrade) -> Result<TickData, ProviderError> {
        // Parse timestamp (RFC3339 format, e.g., "2024-01-15T10:30:00.123456Z")
        let ts_event = DateTime::parse_from_rfc3339(&trade.timestamp)
            .map_err(|e| {
                ProviderError::Parse(format!(
                    "Invalid timestamp '{}': {}",
                    trade.timestamp, e
                ))
            })?
            .with_timezone(&Utc);

        // Convert price from f64 to Decimal
        let price = Decimal::try_from(trade.price).map_err(|e| {
            ProviderError::Parse(format!("Invalid price '{}': {}", trade.price, e))
        })?;

        // Convert quantity from f64 to Decimal
        let quantity = Decimal::try_from(trade.qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid quantity '{}': {}", trade.qty, e))
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
        let side = parse_side(&trade.side)?;

        // For Kraken, if side is "sell", the seller is the taker (is_buyer_maker = true)
        // If side is "buy", the buyer is the taker (is_buyer_maker = false)
        let is_buyer_maker = side == TradeSide::Sell;

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&trade.symbol)?;

        // Get next sequence number
        let sequence = self.sequence.next();

        Ok(TickData::with_details(
            ts_event,
            Utc::now(),
            canonical_symbol,
            get_exchange_name(false).to_string(),
            price,
            quantity,
            side,
            "kraken".to_string(),
            trade.trade_id.to_string(),
            is_buyer_maker,
            sequence,
        ))
    }

    /// Normalize a Kraken Futures trade to TickData
    pub fn normalize_futures(
        &self,
        trade: KrakenFuturesTradeMessage,
    ) -> Result<TickData, ProviderError> {
        // Parse timestamp (Unix milliseconds)
        let ts_event = DateTime::from_timestamp_millis(trade.time).ok_or_else(|| {
            ProviderError::Parse(format!("Invalid timestamp: {}", trade.time))
        })?;

        // Convert price from f64 to Decimal
        let price = Decimal::try_from(trade.price).map_err(|e| {
            ProviderError::Parse(format!("Invalid price '{}': {}", trade.price, e))
        })?;

        // Convert quantity from f64 to Decimal
        let quantity = Decimal::try_from(trade.qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid quantity '{}': {}", trade.qty, e))
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
        let side = parse_side(&trade.side)?;

        // For Kraken Futures, if side is "sell", the seller is the taker
        let is_buyer_maker = side == TradeSide::Sell;

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&trade.product_id)?;

        // Get next sequence number
        let sequence = self.sequence.next();

        Ok(TickData::with_details(
            ts_event,
            Utc::now(),
            canonical_symbol,
            get_exchange_name(true).to_string(),
            price,
            quantity,
            side,
            "kraken_futures".to_string(),
            trade.seq.to_string(), // Use sequence number as trade ID
            is_buyer_maker,
            sequence,
        ))
    }

    /// Reset the sequence counter (for testing)
    #[cfg(test)]
    pub fn reset_sequence(&self) {
        self.sequence.reset();
    }

    // ========================================================================
    // L1 Quote (Ticker) Normalization
    // ========================================================================

    /// Normalize a Kraken Spot ticker to QuoteTick
    pub fn normalize_spot_ticker(
        &self,
        ticker: KrakenSpotTicker,
        ts_event: Option<DateTime<Utc>>,
    ) -> Result<QuoteTick, ProviderError> {
        let ts = ts_event.unwrap_or_else(Utc::now);

        // Convert prices from f64 to Decimal
        let bid_price = Decimal::try_from(ticker.bid).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid price '{}': {}", ticker.bid, e))
        })?;
        let ask_price = Decimal::try_from(ticker.ask).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask price '{}': {}", ticker.ask, e))
        })?;
        let bid_size = Decimal::try_from(ticker.bid_qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid qty '{}': {}", ticker.bid_qty, e))
        })?;
        let ask_size = Decimal::try_from(ticker.ask_qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask qty '{}': {}", ticker.ask_qty, e))
        })?;

        // Validate prices
        if bid_price <= Decimal::ZERO || ask_price <= Decimal::ZERO {
            return Err(ProviderError::Parse(
                "Bid and ask prices must be positive".to_string(),
            ));
        }

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&ticker.symbol)?;
        let sequence = self.sequence.next() as u64;

        Ok(QuoteTick::with_details(
            ts,
            Utc::now(),
            canonical_symbol,
            get_exchange_name(false).to_string(),
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence,
        ))
    }

    /// Normalize a Kraken Futures ticker to QuoteTick
    pub fn normalize_futures_ticker(
        &self,
        ticker: KrakenFuturesTickerMessage,
    ) -> Result<QuoteTick, ProviderError> {
        // Parse timestamp (Unix milliseconds)
        let ts_event = DateTime::from_timestamp_millis(ticker.time).ok_or_else(|| {
            ProviderError::Parse(format!("Invalid timestamp: {}", ticker.time))
        })?;

        // Convert prices from f64 to Decimal
        let bid_price = Decimal::try_from(ticker.bid).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid price '{}': {}", ticker.bid, e))
        })?;
        let ask_price = Decimal::try_from(ticker.ask).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask price '{}': {}", ticker.ask, e))
        })?;
        let bid_size = Decimal::try_from(ticker.bid_size).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid size '{}': {}", ticker.bid_size, e))
        })?;
        let ask_size = Decimal::try_from(ticker.ask_size).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask size '{}': {}", ticker.ask_size, e))
        })?;

        // Validate prices
        if bid_price <= Decimal::ZERO || ask_price <= Decimal::ZERO {
            return Err(ProviderError::Parse(
                "Bid and ask prices must be positive".to_string(),
            ));
        }

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&ticker.product_id)?;
        let sequence = self.sequence.next() as u64;

        Ok(QuoteTick::with_details(
            ts_event,
            Utc::now(),
            canonical_symbol,
            get_exchange_name(true).to_string(),
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence,
        ))
    }

    // ========================================================================
    // DBN-Native Trade Normalization
    // ========================================================================

    /// Normalize a Kraken Spot trade to DBN TradeMsg
    ///
    /// This produces a native `dbn::TradeMsg` without intermediate TickData conversion,
    /// which is more efficient for DBN-native pipelines.
    ///
    /// Uses persistent instrument_id from InstrumentRegistry if available.
    pub fn normalize_spot_to_dbn(&self, trade: KrakenSpotTrade) -> Result<TradeMsg, ProviderError> {
        // Parse timestamp (RFC3339 format)
        let ts_event = DateTime::parse_from_rfc3339(&trade.timestamp)
            .map_err(|e| {
                ProviderError::Parse(format!(
                    "Invalid timestamp '{}': {}",
                    trade.timestamp, e
                ))
            })?
            .with_timezone(&Utc);

        // Convert price and quantity
        let price = Decimal::try_from(trade.price).map_err(|e| {
            ProviderError::Parse(format!("Invalid price '{}': {}", trade.price, e))
        })?;
        let quantity = Decimal::try_from(trade.qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid quantity '{}': {}", trade.qty, e))
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
        let side = parse_side_dbn(&trade.side)?;

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&trade.symbol)?;

        // Get instrument_id (cached or hash-based fallback)
        let instrument_id = self.get_instrument_id(&canonical_symbol);

        // Get next sequence number
        let sequence = self.sequence.next();

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

    /// Normalize a Kraken Futures trade to DBN TradeMsg
    ///
    /// This produces a native `dbn::TradeMsg` without intermediate TickData conversion.
    ///
    /// Uses persistent instrument_id from InstrumentRegistry if available.
    pub fn normalize_futures_to_dbn(
        &self,
        trade: KrakenFuturesTradeMessage,
    ) -> Result<TradeMsg, ProviderError> {
        // Parse timestamp (Unix milliseconds)
        let ts_event = DateTime::from_timestamp_millis(trade.time).ok_or_else(|| {
            ProviderError::Parse(format!("Invalid timestamp: {}", trade.time))
        })?;

        // Convert price and quantity
        let price = Decimal::try_from(trade.price).map_err(|e| {
            ProviderError::Parse(format!("Invalid price '{}': {}", trade.price, e))
        })?;
        let quantity = Decimal::try_from(trade.qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid quantity '{}': {}", trade.qty, e))
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
        let side = parse_side_dbn(&trade.side)?;

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&trade.product_id)?;

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
            trade.seq as u32, // Use Kraken sequence as DBN sequence
        ))
    }

    // ========================================================================
    // DBN-Native Quote (Ticker) Normalization
    // ========================================================================

    /// Normalize a Kraken Spot ticker to DBN BboMsg
    ///
    /// This produces a native `dbn::BboMsg` without intermediate QuoteTick conversion.
    pub fn normalize_spot_ticker_to_dbn(
        &self,
        ticker: KrakenSpotTicker,
        ts_event: Option<DateTime<Utc>>,
    ) -> Result<BboMsg, ProviderError> {
        let ts = ts_event.unwrap_or_else(Utc::now);

        // Convert prices
        let bid_price = Decimal::try_from(ticker.bid).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid price '{}': {}", ticker.bid, e))
        })?;
        let ask_price = Decimal::try_from(ticker.ask).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask price '{}': {}", ticker.ask, e))
        })?;
        let bid_size = Decimal::try_from(ticker.bid_qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid qty '{}': {}", ticker.bid_qty, e))
        })?;
        let ask_size = Decimal::try_from(ticker.ask_qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask qty '{}': {}", ticker.ask_qty, e))
        })?;

        // Validate prices
        if bid_price <= Decimal::ZERO || ask_price <= Decimal::ZERO {
            return Err(ProviderError::Parse(
                "Bid and ask prices must be positive".to_string(),
            ));
        }

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&ticker.symbol)?;
        let exchange = get_exchange_name(false);
        let sequence = self.sequence.next();

        let ts_nanos = datetime_to_nanos(ts);
        let ts_recv_nanos = datetime_to_nanos(Utc::now());

        Ok(create_bbo_msg_from_decimals(
            ts_nanos,
            ts_recv_nanos,
            &canonical_symbol,
            exchange,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence as u32,
        ))
    }

    /// Normalize a Kraken Futures ticker to DBN BboMsg
    ///
    /// This produces a native `dbn::BboMsg` without intermediate QuoteTick conversion.
    pub fn normalize_futures_ticker_to_dbn(
        &self,
        ticker: KrakenFuturesTickerMessage,
    ) -> Result<BboMsg, ProviderError> {
        // Parse timestamp (Unix milliseconds)
        let ts_event = DateTime::from_timestamp_millis(ticker.time).ok_or_else(|| {
            ProviderError::Parse(format!("Invalid timestamp: {}", ticker.time))
        })?;

        // Convert prices
        let bid_price = Decimal::try_from(ticker.bid).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid price '{}': {}", ticker.bid, e))
        })?;
        let ask_price = Decimal::try_from(ticker.ask).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask price '{}': {}", ticker.ask, e))
        })?;
        let bid_size = Decimal::try_from(ticker.bid_size).map_err(|e| {
            ProviderError::Parse(format!("Invalid bid size '{}': {}", ticker.bid_size, e))
        })?;
        let ask_size = Decimal::try_from(ticker.ask_size).map_err(|e| {
            ProviderError::Parse(format!("Invalid ask size '{}': {}", ticker.ask_size, e))
        })?;

        // Validate prices
        if bid_price <= Decimal::ZERO || ask_price <= Decimal::ZERO {
            return Err(ProviderError::Parse(
                "Bid and ask prices must be positive".to_string(),
            ));
        }

        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&ticker.product_id)?;
        let exchange = get_exchange_name(true);
        let sequence = self.sequence.next();

        let ts_nanos = datetime_to_nanos(ts_event);
        let ts_recv_nanos = datetime_to_nanos(Utc::now());

        Ok(create_bbo_msg_from_decimals(
            ts_nanos,
            ts_recv_nanos,
            &canonical_symbol,
            exchange,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence as u32,
        ))
    }

    // ========================================================================
    // L2 Order Book Normalization
    // ========================================================================

    /// Normalize a Kraken Spot book snapshot to OrderBook
    pub fn normalize_spot_book_snapshot(
        &self,
        book_data: KrakenSpotBook,
    ) -> Result<OrderBook, ProviderError> {
        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&book_data.symbol)?;
        let exchange = get_exchange_name(false).to_string();

        let mut order_book = OrderBook::with_exchange(&canonical_symbol, &exchange);

        // Parse timestamp if available
        let ts = if let Some(ref ts_str) = book_data.timestamp {
            DateTime::parse_from_rfc3339(ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now())
        } else {
            Utc::now()
        };

        let sequence = self.sequence.next() as u64;
        order_book.set_timestamp(ts, sequence);

        // Add bid levels
        for level in &book_data.bids {
            let price = Decimal::try_from(level.price).map_err(|e| {
                ProviderError::Parse(format!("Invalid bid price '{}': {}", level.price, e))
            })?;
            let size = Decimal::try_from(level.qty).map_err(|e| {
                ProviderError::Parse(format!("Invalid bid qty '{}': {}", level.qty, e))
            })?;
            order_book.update_bid(price, size, 0);
        }

        // Add ask levels
        for level in &book_data.asks {
            let price = Decimal::try_from(level.price).map_err(|e| {
                ProviderError::Parse(format!("Invalid ask price '{}': {}", level.price, e))
            })?;
            let size = Decimal::try_from(level.qty).map_err(|e| {
                ProviderError::Parse(format!("Invalid ask qty '{}': {}", level.qty, e))
            })?;
            order_book.update_ask(price, size, 0);
        }

        Ok(order_book)
    }

    /// Normalize a Kraken Spot book update to OrderBookDelta
    pub fn normalize_spot_book_update(
        &self,
        book_data: &KrakenSpotBook,
    ) -> Result<Vec<OrderBookDelta>, ProviderError> {
        let canonical_symbol = to_canonical(&book_data.symbol)?;
        let exchange = get_exchange_name(false).to_string();

        // Parse timestamp if available
        let ts = if let Some(ref ts_str) = book_data.timestamp {
            DateTime::parse_from_rfc3339(ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now())
        } else {
            Utc::now()
        };

        let mut deltas = Vec::new();

        // Process bid updates
        for level in &book_data.bids {
            let price = Decimal::try_from(level.price).map_err(|e| {
                ProviderError::Parse(format!("Invalid bid price '{}': {}", level.price, e))
            })?;
            let size = Decimal::try_from(level.qty).map_err(|e| {
                ProviderError::Parse(format!("Invalid bid qty '{}': {}", level.qty, e))
            })?;

            let action = if size.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let mut delta = OrderBookDelta::new(
                canonical_symbol.clone(),
                ts,
                BookSide::Bid,
                action,
                price,
                size,
            );
            delta.exchange = exchange.clone();
            delta.sequence = self.sequence.next() as u64;
            deltas.push(delta);
        }

        // Process ask updates
        for level in &book_data.asks {
            let price = Decimal::try_from(level.price).map_err(|e| {
                ProviderError::Parse(format!("Invalid ask price '{}': {}", level.price, e))
            })?;
            let size = Decimal::try_from(level.qty).map_err(|e| {
                ProviderError::Parse(format!("Invalid ask qty '{}': {}", level.qty, e))
            })?;

            let action = if size.is_zero() {
                BookAction::Delete
            } else {
                BookAction::Update
            };

            let mut delta = OrderBookDelta::new(
                canonical_symbol.clone(),
                ts,
                BookSide::Ask,
                action,
                price,
                size,
            );
            delta.exchange = exchange.clone();
            delta.sequence = self.sequence.next() as u64;
            deltas.push(delta);
        }

        Ok(deltas)
    }

    /// Normalize a Kraken Futures book snapshot to OrderBook
    pub fn normalize_futures_book_snapshot(
        &self,
        snapshot: KrakenFuturesBookSnapshotMessage,
    ) -> Result<OrderBook, ProviderError> {
        // Convert symbol to canonical format
        let canonical_symbol = to_canonical(&snapshot.product_id)?;
        let exchange = get_exchange_name(true).to_string();

        let mut order_book = OrderBook::with_exchange(&canonical_symbol, &exchange);

        // Parse timestamp (Unix milliseconds)
        let ts = DateTime::from_timestamp_millis(snapshot.timestamp).ok_or_else(|| {
            ProviderError::Parse(format!("Invalid timestamp: {}", snapshot.timestamp))
        })?;

        order_book.set_timestamp(ts, snapshot.seq as u64);

        // Add bid levels
        for level in &snapshot.bids {
            let price = Decimal::try_from(level.price).map_err(|e| {
                ProviderError::Parse(format!("Invalid bid price '{}': {}", level.price, e))
            })?;
            let size = Decimal::try_from(level.qty).map_err(|e| {
                ProviderError::Parse(format!("Invalid bid qty '{}': {}", level.qty, e))
            })?;
            order_book.update_bid(price, size, 0);
        }

        // Add ask levels
        for level in &snapshot.asks {
            let price = Decimal::try_from(level.price).map_err(|e| {
                ProviderError::Parse(format!("Invalid ask price '{}': {}", level.price, e))
            })?;
            let size = Decimal::try_from(level.qty).map_err(|e| {
                ProviderError::Parse(format!("Invalid ask qty '{}': {}", level.qty, e))
            })?;
            order_book.update_ask(price, size, 0);
        }

        Ok(order_book)
    }

    /// Normalize a Kraken Futures book update to OrderBookDelta
    pub fn normalize_futures_book_update(
        &self,
        update: KrakenFuturesBookUpdateMessage,
    ) -> Result<OrderBookDelta, ProviderError> {
        let canonical_symbol = to_canonical(&update.product_id)?;
        let exchange = get_exchange_name(true).to_string();

        // Parse timestamp (Unix milliseconds)
        let ts = DateTime::from_timestamp_millis(update.timestamp).ok_or_else(|| {
            ProviderError::Parse(format!("Invalid timestamp: {}", update.timestamp))
        })?;

        let price = Decimal::try_from(update.price).map_err(|e| {
            ProviderError::Parse(format!("Invalid price '{}': {}", update.price, e))
        })?;
        let size = Decimal::try_from(update.qty).map_err(|e| {
            ProviderError::Parse(format!("Invalid qty '{}': {}", update.qty, e))
        })?;

        let side = match update.side.to_lowercase().as_str() {
            "buy" => BookSide::Bid,
            "sell" => BookSide::Ask,
            _ => {
                return Err(ProviderError::Parse(format!(
                    "Unknown book side: {}",
                    update.side
                )))
            }
        };

        let action = if size.is_zero() {
            BookAction::Delete
        } else {
            BookAction::Update
        };

        let mut delta = OrderBookDelta::new(canonical_symbol, ts, side, action, price, size);
        delta.exchange = exchange;
        delta.sequence = update.seq as u64;

        Ok(delta)
    }
}

// =============================================================================
// SymbolNormalizer Trait Implementation
// =============================================================================

use async_trait::async_trait;

#[async_trait]
impl SymbolNormalizer for KrakenNormalizer {
    fn to_canonical(&self, venue_symbol: &str) -> Result<String, ProviderError> {
        to_canonical(venue_symbol)
    }

    fn to_venue(&self, canonical_symbol: &str) -> Result<String, ProviderError> {
        if self.is_futures {
            to_kraken_futures(canonical_symbol)
        } else {
            to_kraken_spot(canonical_symbol)
        }
    }

    fn exchange_name(&self) -> &str {
        get_exchange_name(self.is_futures)
    }

    async fn register_symbols(
        &self,
        symbols: &[String],
        registry: &Arc<InstrumentRegistry>,
    ) -> Result<HashMap<String, u32>, ProviderError> {
        let exchange = get_exchange_name(self.is_futures);
        let mut result = HashMap::new();

        for symbol in symbols {
            // Convert to canonical format
            let canonical = to_canonical(symbol)?;

            // Get or create instrument_id from registry
            let id = registry
                .get_or_create(&canonical, exchange)
                .await
                .map_err(|e| ProviderError::Internal(format!("Registry error: {}", e)))?;

            // Cache for fast lookup
            self.instrument_ids.insert(canonical.clone(), id);
            result.insert(canonical, id);
        }

        Ok(result)
    }
}

/// Parse trade side from Kraken string format
fn parse_side(side: &str) -> Result<TradeSide, ProviderError> {
    match side.to_lowercase().as_str() {
        "buy" | "b" => Ok(TradeSide::Buy),
        "sell" | "s" => Ok(TradeSide::Sell),
        _ => Err(ProviderError::Parse(format!("Unknown trade side: {}", side))),
    }
}

/// Parse trade side from Kraken string format to DBN-compatible TradeSideCompat
fn parse_side_dbn(side: &str) -> Result<TradeSideCompat, ProviderError> {
    match side.to_lowercase().as_str() {
        "buy" | "b" => Ok(TradeSideCompat::Buy),
        "sell" | "s" => Ok(TradeSideCompat::Sell),
        _ => Err(ProviderError::Parse(format!("Unknown trade side: {}", side))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_normalize_spot_trade() {
        let normalizer = KrakenNormalizer::spot();

        let trade = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: 50000.0,
            qty: 0.001,
            ord_type: "market".to_string(),
            trade_id: 12345,
            timestamp: "2024-01-15T10:30:00.123456Z".to_string(),
        };

        let tick = normalizer.normalize_spot(trade).unwrap();

        assert_eq!(tick.symbol, "BTCUSD");
        assert_eq!(tick.exchange, "KRAKEN");
        assert_eq!(tick.price, dec!(50000));
        assert_eq!(tick.quantity, dec!(0.001));
        assert_eq!(tick.side, TradeSide::Buy);
        assert_eq!(tick.provider, "kraken");
        assert_eq!(tick.trade_id, "12345");
        assert!(!tick.is_buyer_maker); // Buy side = buyer is taker
        assert_eq!(tick.sequence, 0);
    }

    #[test]
    fn test_normalize_spot_sell_trade() {
        let normalizer = KrakenNormalizer::spot();

        let trade = KrakenSpotTrade {
            symbol: "ETH/USD".to_string(),
            side: "sell".to_string(),
            price: 3000.50,
            qty: 0.1,
            ord_type: "limit".to_string(),
            trade_id: 67890,
            timestamp: "2024-01-15T10:31:00.000000Z".to_string(),
        };

        let tick = normalizer.normalize_spot(trade).unwrap();

        assert_eq!(tick.symbol, "ETHUSD");
        assert_eq!(tick.side, TradeSide::Sell);
        assert!(tick.is_buyer_maker); // Sell side = buyer is maker
    }

    #[test]
    fn test_normalize_futures_trade() {
        let normalizer = KrakenNormalizer::futures();

        let trade = KrakenFuturesTradeMessage {
            feed: "trade".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            side: "buy".to_string(),
            price: 50100.5,
            qty: 1.0,
            seq: 123456,
            time: 1705315800123,
            trade_type: Some("fill".to_string()),
        };

        let tick = normalizer.normalize_futures(trade).unwrap();

        assert_eq!(tick.symbol, "BTCUSD");
        assert_eq!(tick.exchange, "KRAKEN_FUTURES");
        assert_eq!(tick.price, dec!(50100.5));
        assert_eq!(tick.quantity, dec!(1));
        assert_eq!(tick.side, TradeSide::Buy);
        assert_eq!(tick.provider, "kraken_futures");
        assert_eq!(tick.trade_id, "123456");
        assert!(!tick.is_buyer_maker);
    }

    #[test]
    fn test_sequence_increment() {
        let normalizer = KrakenNormalizer::spot();

        let trade1 = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: 50000.0,
            qty: 0.001,
            ord_type: "market".to_string(),
            trade_id: 1,
            timestamp: "2024-01-15T10:30:00.000000Z".to_string(),
        };

        let trade2 = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "sell".to_string(),
            price: 50001.0,
            qty: 0.002,
            ord_type: "market".to_string(),
            trade_id: 2,
            timestamp: "2024-01-15T10:30:01.000000Z".to_string(),
        };

        let tick1 = normalizer.normalize_spot(trade1).unwrap();
        let tick2 = normalizer.normalize_spot(trade2).unwrap();

        assert_eq!(tick1.sequence, 0);
        assert_eq!(tick2.sequence, 1);
    }

    #[test]
    fn test_invalid_price() {
        let normalizer = KrakenNormalizer::spot();

        let trade = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: -100.0,
            qty: 0.001,
            ord_type: "market".to_string(),
            trade_id: 1,
            timestamp: "2024-01-15T10:30:00.000000Z".to_string(),
        };

        assert!(normalizer.normalize_spot(trade).is_err());
    }

    #[test]
    fn test_invalid_quantity() {
        let normalizer = KrakenNormalizer::spot();

        let trade = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: 50000.0,
            qty: 0.0,
            ord_type: "market".to_string(),
            trade_id: 1,
            timestamp: "2024-01-15T10:30:00.000000Z".to_string(),
        };

        assert!(normalizer.normalize_spot(trade).is_err());
    }

    #[test]
    fn test_invalid_side() {
        assert!(parse_side("buy").is_ok());
        assert!(parse_side("sell").is_ok());
        assert!(parse_side("BUY").is_ok());
        assert!(parse_side("SELL").is_ok());
        assert!(parse_side("b").is_ok());
        assert!(parse_side("s").is_ok());
        assert!(parse_side("unknown").is_err());
    }

    #[test]
    fn test_invalid_timestamp() {
        let normalizer = KrakenNormalizer::spot();

        let trade = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: 50000.0,
            qty: 0.001,
            ord_type: "market".to_string(),
            trade_id: 1,
            timestamp: "invalid-timestamp".to_string(),
        };

        assert!(normalizer.normalize_spot(trade).is_err());
    }

    // ========================================================================
    // L1 Quote (Ticker) Tests
    // ========================================================================

    #[test]
    fn test_normalize_spot_ticker() {
        let normalizer = KrakenNormalizer::spot();

        let ticker = KrakenSpotTicker {
            symbol: "XBT/USD".to_string(),
            bid: 50000.0,
            bid_qty: 1.5,
            ask: 50001.0,
            ask_qty: 2.0,
            last: 50000.5,
            volume: 1000.0,
            vwap: 49500.0,
            low: 49000.0,
            high: 51000.0,
            change: 500.0,
            change_pct: 1.0,
        };

        let quote = normalizer.normalize_spot_ticker(ticker, None).unwrap();

        assert_eq!(quote.symbol, "BTCUSD");
        assert_eq!(quote.exchange, "KRAKEN");
        assert_eq!(quote.bid_price, dec!(50000));
        assert_eq!(quote.ask_price, dec!(50001));
        assert_eq!(quote.bid_size, dec!(1.5));
        assert_eq!(quote.ask_size, dec!(2));
    }

    #[test]
    fn test_normalize_futures_ticker() {
        let normalizer = KrakenNormalizer::futures();

        let ticker = KrakenFuturesTickerMessage {
            feed: "ticker".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            bid: 50000.0,
            ask: 50001.0,
            bid_size: 100.0,
            ask_size: 150.0,
            last: 50000.5,
            last_size: 10.0,
            volume: 50000.0,
            open_interest: 100000.0,
            mark_price: 50000.25,
            time: 1705315800123,
        };

        let quote = normalizer.normalize_futures_ticker(ticker).unwrap();

        assert_eq!(quote.symbol, "BTCUSD");
        assert_eq!(quote.exchange, "KRAKEN_FUTURES");
        assert_eq!(quote.bid_price, dec!(50000));
        assert_eq!(quote.ask_price, dec!(50001));
        assert_eq!(quote.bid_size, dec!(100));
        assert_eq!(quote.ask_size, dec!(150));
    }

    // ========================================================================
    // L2 Order Book Tests
    // ========================================================================

    #[test]
    fn test_normalize_spot_book_snapshot() {
        use super::super::types::KrakenSpotBookLevel;

        let normalizer = KrakenNormalizer::spot();

        let book_data = KrakenSpotBook {
            symbol: "XBT/USD".to_string(),
            bids: vec![
                KrakenSpotBookLevel { price: 50000.0, qty: 1.5 },
                KrakenSpotBookLevel { price: 49999.0, qty: 2.0 },
            ],
            asks: vec![
                KrakenSpotBookLevel { price: 50001.0, qty: 1.0 },
                KrakenSpotBookLevel { price: 50002.0, qty: 3.0 },
            ],
            checksum: 0,
            timestamp: Some("2024-01-15T10:30:00.123456Z".to_string()),
        };

        let book = normalizer.normalize_spot_book_snapshot(book_data).unwrap();

        assert_eq!(book.symbol, "BTCUSD");
        assert_eq!(book.exchange, "KRAKEN");
        assert_eq!(book.bid_depth(), 2);
        assert_eq!(book.ask_depth(), 2);
        assert_eq!(book.best_bid().unwrap().price, dec!(50000));
        assert_eq!(book.best_ask().unwrap().price, dec!(50001));
    }

    #[test]
    fn test_normalize_spot_book_update() {
        use super::super::types::KrakenSpotBookLevel;

        let normalizer = KrakenNormalizer::spot();

        let book_data = KrakenSpotBook {
            symbol: "XBT/USD".to_string(),
            bids: vec![
                KrakenSpotBookLevel { price: 50000.0, qty: 2.0 }, // Update
                KrakenSpotBookLevel { price: 49998.0, qty: 0.0 }, // Delete
            ],
            asks: vec![
                KrakenSpotBookLevel { price: 50001.0, qty: 1.5 }, // Update
            ],
            checksum: 0,
            timestamp: None,
        };

        let deltas = normalizer.normalize_spot_book_update(&book_data).unwrap();

        assert_eq!(deltas.len(), 3);
        assert_eq!(deltas[0].side, BookSide::Bid);
        assert_eq!(deltas[0].price, dec!(50000));
        assert_eq!(deltas[0].action, BookAction::Update);

        assert_eq!(deltas[1].side, BookSide::Bid);
        assert_eq!(deltas[1].action, BookAction::Delete);

        assert_eq!(deltas[2].side, BookSide::Ask);
    }

    #[test]
    fn test_normalize_futures_book_snapshot() {
        use super::super::types::KrakenFuturesBookLevel;

        let normalizer = KrakenNormalizer::futures();

        let snapshot = KrakenFuturesBookSnapshotMessage {
            feed: "book_snapshot".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            timestamp: 1705315800123,
            seq: 12345,
            bids: vec![
                KrakenFuturesBookLevel { price: 50000.0, qty: 100.0 },
                KrakenFuturesBookLevel { price: 49999.0, qty: 200.0 },
            ],
            asks: vec![
                KrakenFuturesBookLevel { price: 50001.0, qty: 150.0 },
            ],
        };

        let book = normalizer.normalize_futures_book_snapshot(snapshot).unwrap();

        assert_eq!(book.symbol, "BTCUSD");
        assert_eq!(book.exchange, "KRAKEN_FUTURES");
        assert_eq!(book.bid_depth(), 2);
        assert_eq!(book.ask_depth(), 1);
        assert_eq!(book.sequence, 12345);
    }

    #[test]
    fn test_normalize_futures_book_update() {
        let normalizer = KrakenNormalizer::futures();

        let update = KrakenFuturesBookUpdateMessage {
            feed: "book".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            seq: 12346,
            timestamp: 1705315800124,
            side: "buy".to_string(),
            price: 50000.0,
            qty: 150.0,
        };

        let delta = normalizer.normalize_futures_book_update(update).unwrap();

        assert_eq!(delta.symbol, "BTCUSD");
        assert_eq!(delta.side, BookSide::Bid);
        assert_eq!(delta.action, BookAction::Update);
        assert_eq!(delta.price, dec!(50000));
        assert_eq!(delta.size, dec!(150));
    }

    #[test]
    fn test_futures_book_update_delete() {
        let normalizer = KrakenNormalizer::futures();

        let update = KrakenFuturesBookUpdateMessage {
            feed: "book".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            seq: 12347,
            timestamp: 1705315800125,
            side: "sell".to_string(),
            price: 50001.0,
            qty: 0.0, // Zero quantity means delete
        };

        let delta = normalizer.normalize_futures_book_update(update).unwrap();

        assert_eq!(delta.side, BookSide::Ask);
        assert_eq!(delta.action, BookAction::Delete);
    }

    // ========================================================================
    // DBN-Native Trade Tests
    // ========================================================================

    #[test]
    fn test_normalize_spot_to_dbn() {
        use trading_common::data::TradeMsgExt;

        let normalizer = KrakenNormalizer::spot();

        let trade = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: 50000.0,
            qty: 0.001,
            ord_type: "market".to_string(),
            trade_id: 12345,
            timestamp: "2024-01-15T10:30:00.123456Z".to_string(),
        };

        let msg = normalizer.normalize_spot_to_dbn(trade).unwrap();

        // Verify price conversion (50000.0 -> 50000 * 1e9)
        assert_eq!(msg.price_decimal(), dec!(50000));
        assert!(msg.is_buy());
        assert!(msg.hd.ts_event > 0);
        assert!(msg.ts_recv > 0);
    }

    #[test]
    fn test_normalize_futures_to_dbn() {
        use trading_common::data::TradeMsgExt;

        let normalizer = KrakenNormalizer::futures();

        let trade = KrakenFuturesTradeMessage {
            feed: "trade".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            side: "sell".to_string(),
            price: 50100.5,
            qty: 1.0,
            seq: 123456,
            time: 1705315800123,
            trade_type: Some("fill".to_string()),
        };

        let msg = normalizer.normalize_futures_to_dbn(trade).unwrap();

        assert_eq!(msg.price_decimal(), dec!(50100.5));
        assert!(msg.is_sell());
        assert_eq!(msg.sequence, 123456);
    }

    // ========================================================================
    // DBN-Native Quote Tests
    // ========================================================================

    #[test]
    fn test_normalize_spot_ticker_to_dbn() {
        use trading_common::data::BboMsgExt;

        let normalizer = KrakenNormalizer::spot();

        let ticker = KrakenSpotTicker {
            symbol: "XBT/USD".to_string(),
            bid: 50000.0,
            bid_qty: 1.5,
            ask: 50001.0,
            ask_qty: 2.0,
            last: 50000.5,
            volume: 1000.0,
            vwap: 49500.0,
            low: 49000.0,
            high: 51000.0,
            change: 500.0,
            change_pct: 1.0,
        };

        let msg = normalizer.normalize_spot_ticker_to_dbn(ticker, None).unwrap();

        assert_eq!(msg.bid_price(), dec!(50000));
        assert_eq!(msg.ask_price(), dec!(50001));
        assert_eq!(msg.spread(), dec!(1)); // 50001 - 50000
        assert_eq!(msg.mid_price(), dec!(50000.5));
    }

    #[test]
    fn test_normalize_futures_ticker_to_dbn() {
        use trading_common::data::BboMsgExt;

        let normalizer = KrakenNormalizer::futures();

        let ticker = KrakenFuturesTickerMessage {
            feed: "ticker".to_string(),
            product_id: "PI_XBTUSD".to_string(),
            bid: 50000.0,
            ask: 50001.0,
            bid_size: 100.0,
            ask_size: 150.0,
            last: 50000.5,
            last_size: 10.0,
            volume: 50000.0,
            open_interest: 100000.0,
            mark_price: 50000.25,
            time: 1705315800123,
        };

        let msg = normalizer.normalize_futures_ticker_to_dbn(ticker).unwrap();

        assert_eq!(msg.bid_price(), dec!(50000));
        assert_eq!(msg.ask_price(), dec!(50001));
        assert_eq!(msg.bid_size(), dec!(100));
        assert_eq!(msg.ask_size(), dec!(150));
    }

    #[test]
    fn test_dbn_parity_with_tick_data() {
        use trading_common::data::TradeMsgExt;

        let normalizer = KrakenNormalizer::spot();
        normalizer.reset_sequence();

        let trade = KrakenSpotTrade {
            symbol: "XBT/USD".to_string(),
            side: "buy".to_string(),
            price: 50000.0,
            qty: 0.001,
            ord_type: "market".to_string(),
            trade_id: 12345,
            timestamp: "2024-01-15T10:30:00.123456Z".to_string(),
        };

        // Normalize to TickData
        let tick = normalizer.normalize_spot(trade.clone()).unwrap();

        // Reset and normalize to DBN
        normalizer.reset_sequence();
        let msg = normalizer.normalize_spot_to_dbn(trade).unwrap();

        // Verify parity: same price, size, side
        assert_eq!(tick.price, msg.price_decimal());
        assert_eq!(tick.side == TradeSide::Buy, msg.is_buy());
    }
}
