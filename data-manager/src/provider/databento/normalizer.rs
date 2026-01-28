//! Databento message normalizer
//!
//! Converts Databento-specific message types to normalized data structures.
//!
//! Supports:
//! - **Trades (Last Tick)**: Individual trade executions via `normalize_trade()`
//! - **L1 (BBO)**: Best bid/offer quotes via `normalize_bbo()`
//! - **L2 (MBP)**: Market by price (aggregated levels) via `normalize_mbp_snapshot()` / `normalize_mbp_update()`
//! - **L3 (MBO)**: Market by order (individual orders) via `normalize_mbo()`
//! - **OHLC**: Candlestick bars via `normalize_ohlc()`

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::provider::ProviderError;
use crate::schema::NormalizedOHLC;
use trading_common::data::orderbook::{BookAction, BookSide, OrderBook, OrderBookDelta};
use trading_common::data::quotes::QuoteTick;
use trading_common::data::types::{TickData, TradeSide};
use trading_common::data::SequenceGenerator;

/// Normalizer for Databento messages
#[derive(Debug)]
pub struct DatabentoNormalizer {
    /// Counter for generating sequence numbers
    sequence: SequenceGenerator,
}

impl DatabentoNormalizer {
    /// Create a new normalizer
    pub fn new() -> Self {
        Self {
            sequence: SequenceGenerator::new(),
        }
    }

    /// Convert nanosecond timestamp to DateTime
    pub fn nanos_to_datetime(nanos: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(nanos)
    }

    /// Convert Databento price (fixed-point) to Decimal
    pub fn price_to_decimal(price: i64, scale: u32) -> Decimal {
        let divisor = 10i64.pow(scale);
        Decimal::from(price) / Decimal::from(divisor)
    }

    /// Normalize a trade message
    ///
    /// In production this would take a dbn::TradeMsg as input.
    /// For now, we provide a generic function that can be called with
    /// the raw field values.
    pub fn normalize_trade(
        &mut self,
        ts_event: i64, // nanoseconds since epoch
        ts_recv: i64,  // nanoseconds since epoch
        symbol: &str,
        exchange: &str,
        price: i64,       // fixed-point price
        price_scale: u32, // price decimal places
        size: i64,        // fixed-point size
        size_scale: u32,  // size decimal places
        side: char,       // 'B' or 'S'
        trade_id: Option<&str>,
    ) -> Result<TickData, ProviderError> {
        let ts_event_dt = Self::nanos_to_datetime(ts_event);
        let ts_recv_dt = Self::nanos_to_datetime(ts_recv);
        let price_dec = Self::price_to_decimal(price, price_scale);
        let quantity_dec = Self::price_to_decimal(size, size_scale);

        let side = TradeSide::from_db_char(side)
            .ok_or_else(|| ProviderError::Parse(format!("Invalid trade side: {}", side)))?;

        let sequence = self.sequence.next();

        Ok(TickData::with_details(
            ts_event_dt,
            ts_recv_dt,
            symbol.to_string(),
            exchange.to_string(),
            price_dec,
            quantity_dec,
            side,
            "databento".to_string(),
            trade_id.map(|s| s.to_string()).unwrap_or_default(),
            false, // is_buyer_maker not directly available from Databento
            sequence,
        ))
    }

    /// Normalize an OHLC bar
    pub fn normalize_ohlc(
        &mut self,
        ts_event: i64,
        symbol: &str,
        exchange: &str,
        timeframe: &str,
        open: i64,
        high: i64,
        low: i64,
        close: i64,
        price_scale: u32,
        volume: i64,
        volume_scale: u32,
        trade_count: u64,
    ) -> Result<NormalizedOHLC, ProviderError> {
        let timestamp = Self::nanos_to_datetime(ts_event);
        let open_dec = Self::price_to_decimal(open, price_scale);
        let high_dec = Self::price_to_decimal(high, price_scale);
        let low_dec = Self::price_to_decimal(low, price_scale);
        let close_dec = Self::price_to_decimal(close, price_scale);
        let volume_dec = Self::price_to_decimal(volume, volume_scale);

        Ok(NormalizedOHLC::new(
            timestamp,
            symbol.to_string(),
            exchange.to_string(),
            timeframe.to_string(),
            open_dec,
            high_dec,
            low_dec,
            close_dec,
            volume_dec,
            trade_count,
            "databento".to_string(),
        ))
    }

    /// Map Databento dataset to exchange name
    pub fn dataset_to_exchange(dataset: &str) -> &str {
        match dataset {
            "GLBX.MDP3" => "CME",
            "XNAS.ITCH" => "NASDAQ",
            "XNYS.TRADES" => "NYSE",
            "XBTS.PITCH" => "BATS",
            "IEXG.TOPS" => "IEX",
            _ => "UNKNOWN",
        }
    }

    // =========================================================================
    // L1 (BBO) Normalization
    // =========================================================================

    /// Normalize a BBO (Best Bid/Offer) message to QuoteTick
    ///
    /// Used for L1 top-of-book data from Databento's BBO schemas.
    /// This covers Bbo1SMsg (1-second snapshots), Bbo1MMsg (1-minute), etc.
    ///
    /// # Arguments
    /// * `ts_event` - Event timestamp in nanoseconds since epoch
    /// * `ts_recv` - Receive timestamp in nanoseconds since epoch
    /// * `symbol` - Trading symbol (e.g., "ESH4")
    /// * `exchange` - Exchange/venue name (e.g., "CME")
    /// * `bid_px` - Bid price (fixed-point)
    /// * `ask_px` - Ask price (fixed-point)
    /// * `bid_sz` - Bid size (fixed-point)
    /// * `ask_sz` - Ask size (fixed-point)
    /// * `price_scale` - Decimal places for prices
    /// * `size_scale` - Decimal places for sizes
    pub fn normalize_bbo(
        &mut self,
        ts_event: i64,
        ts_recv: i64,
        symbol: &str,
        exchange: &str,
        bid_px: i64,
        ask_px: i64,
        bid_sz: i64,
        ask_sz: i64,
        price_scale: u32,
        size_scale: u32,
    ) -> Result<QuoteTick, ProviderError> {
        let ts_event_dt = Self::nanos_to_datetime(ts_event);
        let ts_recv_dt = Self::nanos_to_datetime(ts_recv);

        let bid_price = Self::price_to_decimal(bid_px, price_scale);
        let ask_price = Self::price_to_decimal(ask_px, price_scale);
        let bid_size = Self::price_to_decimal(bid_sz, size_scale);
        let ask_size = Self::price_to_decimal(ask_sz, size_scale);

        // Validate prices
        if bid_price < Decimal::ZERO || ask_price < Decimal::ZERO {
            return Err(ProviderError::Parse(
                "BBO prices must be non-negative".to_string(),
            ));
        }

        let sequence = self.sequence.next() as u64;

        Ok(QuoteTick::with_details(
            ts_event_dt,
            ts_recv_dt,
            symbol.to_string(),
            exchange.to_string(),
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            sequence,
        ))
    }

    // =========================================================================
    // L2 (MBP - Market by Price) Normalization
    // =========================================================================

    /// Normalize an MBP (Market by Price) snapshot to OrderBook
    ///
    /// Used for L2 order book data with aggregated price levels.
    /// Supports Mbp1Msg (1 level) through Mbp10Msg (10 levels).
    ///
    /// # Arguments
    /// * `ts_event` - Event timestamp in nanoseconds since epoch
    /// * `symbol` - Trading symbol
    /// * `exchange` - Exchange/venue name
    /// * `bid_levels` - Array of (price, size, order_count) for bids
    /// * `ask_levels` - Array of (price, size, order_count) for asks
    /// * `price_scale` - Decimal places for prices
    /// * `size_scale` - Decimal places for sizes
    pub fn normalize_mbp_snapshot(
        &mut self,
        ts_event: i64,
        symbol: &str,
        exchange: &str,
        bid_levels: &[(i64, i64, u32)], // (price, size, order_count)
        ask_levels: &[(i64, i64, u32)],
        price_scale: u32,
        size_scale: u32,
    ) -> Result<OrderBook, ProviderError> {
        let ts = Self::nanos_to_datetime(ts_event);
        let sequence = self.sequence.next() as u64;

        let mut order_book = OrderBook::with_exchange(symbol, exchange);
        order_book.set_timestamp(ts, sequence);

        // Add bid levels (sorted descending by price)
        for (price, size, order_count) in bid_levels {
            let price_dec = Self::price_to_decimal(*price, price_scale);
            let size_dec = Self::price_to_decimal(*size, size_scale);
            if size_dec > Decimal::ZERO {
                order_book.update_bid(price_dec, size_dec, *order_count);
            }
        }

        // Add ask levels (sorted ascending by price)
        for (price, size, order_count) in ask_levels {
            let price_dec = Self::price_to_decimal(*price, price_scale);
            let size_dec = Self::price_to_decimal(*size, size_scale);
            if size_dec > Decimal::ZERO {
                order_book.update_ask(price_dec, size_dec, *order_count);
            }
        }

        Ok(order_book)
    }

    /// Normalize an MBP level update to OrderBookDelta
    ///
    /// Used for incremental L2 order book updates.
    ///
    /// # Arguments
    /// * `ts_event` - Event timestamp in nanoseconds since epoch
    /// * `symbol` - Trading symbol
    /// * `exchange` - Exchange/venue name
    /// * `side` - 'B' for bid, 'A' for ask
    /// * `action` - 'A' (add), 'M' (modify), 'D' (delete), 'C' (clear)
    /// * `price` - Price level (fixed-point)
    /// * `size` - Size at level (fixed-point)
    /// * `order_count` - Number of orders at level
    /// * `price_scale` - Decimal places for prices
    /// * `size_scale` - Decimal places for sizes
    #[allow(clippy::too_many_arguments)]
    pub fn normalize_mbp_update(
        &mut self,
        ts_event: i64,
        symbol: &str,
        exchange: &str,
        side: char,
        action: char,
        price: i64,
        size: i64,
        order_count: u32,
        price_scale: u32,
        size_scale: u32,
    ) -> Result<OrderBookDelta, ProviderError> {
        let ts = Self::nanos_to_datetime(ts_event);
        let price_dec = Self::price_to_decimal(price, price_scale);
        let size_dec = Self::price_to_decimal(size, size_scale);

        let book_side = match side {
            'B' | 'b' => BookSide::Bid,
            'A' | 'a' | 'S' | 's' => BookSide::Ask,
            _ => {
                return Err(ProviderError::Parse(format!(
                    "Invalid MBP side: {}",
                    side
                )))
            }
        };

        let book_action = match action {
            'A' | 'a' => BookAction::Add,
            'M' | 'm' | 'U' | 'u' => BookAction::Update,
            'D' | 'd' => BookAction::Delete,
            'C' | 'c' => BookAction::Clear,
            _ => {
                return Err(ProviderError::Parse(format!(
                    "Invalid MBP action: {}",
                    action
                )))
            }
        };

        let sequence = self.sequence.next() as u64;

        let mut delta = OrderBookDelta::new(
            symbol.to_string(),
            ts,
            book_side,
            book_action,
            price_dec,
            size_dec,
        );
        delta.exchange = exchange.to_string();
        delta.sequence = sequence;
        delta.order_count = order_count;

        Ok(delta)
    }

    // =========================================================================
    // L3 (MBO - Market by Order) Normalization
    // =========================================================================

    /// Normalize an MBO (Market by Order) message to OrderBookDelta
    ///
    /// Used for L3 full order book data with individual order IDs.
    /// Each message represents a single order action (add, modify, cancel, trade).
    ///
    /// # Arguments
    /// * `ts_event` - Event timestamp in nanoseconds since epoch
    /// * `symbol` - Trading symbol
    /// * `exchange` - Exchange/venue name
    /// * `order_id` - Unique order identifier
    /// * `side` - 'B' for bid, 'A' for ask
    /// * `action` - 'A' (add), 'M' (modify), 'C' (cancel), 'T' (trade)
    /// * `price` - Order price (fixed-point)
    /// * `size` - Order size (fixed-point)
    /// * `price_scale` - Decimal places for prices
    /// * `size_scale` - Decimal places for sizes
    #[allow(clippy::too_many_arguments)]
    pub fn normalize_mbo(
        &mut self,
        ts_event: i64,
        symbol: &str,
        exchange: &str,
        order_id: u64,
        side: char,
        action: char,
        price: i64,
        size: i64,
        price_scale: u32,
        size_scale: u32,
    ) -> Result<OrderBookDelta, ProviderError> {
        let ts = Self::nanos_to_datetime(ts_event);
        let price_dec = Self::price_to_decimal(price, price_scale);
        let size_dec = Self::price_to_decimal(size, size_scale);

        let book_side = match side {
            'B' | 'b' => BookSide::Bid,
            'A' | 'a' | 'S' | 's' => BookSide::Ask,
            _ => {
                return Err(ProviderError::Parse(format!(
                    "Invalid MBO side: {}",
                    side
                )))
            }
        };

        // Map MBO actions to BookAction
        // A = Add, M = Modify, C = Cancel, T = Trade (filled/partial)
        let book_action = match action {
            'A' | 'a' => BookAction::Add,
            'M' | 'm' => BookAction::Update,
            'C' | 'c' | 'D' | 'd' => BookAction::Delete,
            'T' | 't' => BookAction::Update, // Trade reduces/removes size
            'R' | 'r' => BookAction::Clear,  // Reset/clear side
            _ => {
                return Err(ProviderError::Parse(format!(
                    "Invalid MBO action: {}",
                    action
                )))
            }
        };

        let sequence = self.sequence.next() as u64;

        let mut delta = OrderBookDelta::new(
            symbol.to_string(),
            ts,
            book_side,
            book_action,
            price_dec,
            size_dec,
        );
        delta.exchange = exchange.to_string();
        delta.sequence = sequence;
        delta.order_id = Some(order_id);

        Ok(delta)
    }

    /// Build an L2 snapshot from a stream of MBO (L3) deltas
    ///
    /// This aggregates individual orders into price levels to create
    /// an L2 view from L3 data. Multiple orders at the same price are
    /// summed together.
    pub fn aggregate_mbo_to_mbp(
        symbol: &str,
        exchange: &str,
        ts: DateTime<Utc>,
        sequence: u64,
        mbo_deltas: &[OrderBookDelta],
    ) -> OrderBook {
        use std::collections::HashMap;

        let mut book = OrderBook::with_exchange(symbol, exchange);
        book.set_timestamp(ts, sequence);

        // Aggregate sizes by price level
        let mut bid_levels: HashMap<Decimal, (Decimal, u32)> = HashMap::new();
        let mut ask_levels: HashMap<Decimal, (Decimal, u32)> = HashMap::new();

        for delta in mbo_deltas {
            match delta.action {
                BookAction::Add | BookAction::Update => {
                    let levels = match delta.side {
                        BookSide::Bid => &mut bid_levels,
                        BookSide::Ask => &mut ask_levels,
                    };
                    let entry = levels.entry(delta.price).or_insert((Decimal::ZERO, 0));
                    entry.0 += delta.size;
                    entry.1 += 1; // Count orders
                }
                BookAction::Delete => {
                    let levels = match delta.side {
                        BookSide::Bid => &mut bid_levels,
                        BookSide::Ask => &mut ask_levels,
                    };
                    // For delete, we'd need to track order IDs to know which to remove
                    // For simplicity, we subtract the size
                    if let Some(entry) = levels.get_mut(&delta.price) {
                        entry.0 -= delta.size;
                        if entry.1 > 0 {
                            entry.1 -= 1;
                        }
                        if entry.0 <= Decimal::ZERO || entry.1 == 0 {
                            levels.remove(&delta.price);
                        }
                    }
                }
                BookAction::Clear => {
                    match delta.side {
                        BookSide::Bid => bid_levels.clear(),
                        BookSide::Ask => ask_levels.clear(),
                    }
                }
            }
        }

        // Build the order book from aggregated levels
        for (price, (size, order_count)) in bid_levels {
            if size > Decimal::ZERO {
                book.update_bid(price, size, order_count);
            }
        }
        for (price, (size, order_count)) in ask_levels {
            if size > Decimal::ZERO {
                book.update_ask(price, size, order_count);
            }
        }

        book
    }
}

impl Default for DatabentoNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_nanos_to_datetime() {
        let nanos = 1704067200000000000i64; // 2024-01-01 00:00:00 UTC
        let dt = DatabentoNormalizer::nanos_to_datetime(nanos);
        assert_eq!(dt.timestamp(), 1704067200);
    }

    #[test]
    fn test_price_to_decimal() {
        // 5025.50 with 2 decimal places = 502550
        let price = DatabentoNormalizer::price_to_decimal(502550, 2);
        assert_eq!(price, dec!(5025.50));

        // 1.23456789 with 8 decimal places = 123456789
        let price = DatabentoNormalizer::price_to_decimal(123456789, 8);
        assert_eq!(price, dec!(1.23456789));
    }

    #[test]
    fn test_normalize_trade() {
        let mut normalizer = DatabentoNormalizer::new();
        let tick = normalizer
            .normalize_trade(
                1704067200000000000, // 2024-01-01 00:00:00 UTC
                1704067200000001000, // 1 microsecond later
                "ESH4",
                "CME",
                502550,
                2,
                10,
                0,
                'B',
                Some("trade123"),
            )
            .unwrap();

        assert_eq!(tick.symbol, "ESH4");
        assert_eq!(tick.exchange, "CME");
        assert_eq!(tick.price, dec!(5025.50));
        assert_eq!(tick.quantity, dec!(10));
        assert_eq!(tick.side, TradeSide::Buy);
        assert_eq!(tick.trade_id, "trade123");
    }

    #[test]
    fn test_dataset_to_exchange() {
        assert_eq!(DatabentoNormalizer::dataset_to_exchange("GLBX.MDP3"), "CME");
        assert_eq!(
            DatabentoNormalizer::dataset_to_exchange("XNAS.ITCH"),
            "NASDAQ"
        );
        assert_eq!(
            DatabentoNormalizer::dataset_to_exchange("unknown"),
            "UNKNOWN"
        );
    }

    // =========================================================================
    // L1 (BBO) Tests
    // =========================================================================

    #[test]
    fn test_normalize_bbo() {
        let mut normalizer = DatabentoNormalizer::new();
        let quote = normalizer
            .normalize_bbo(
                1704067200000000000, // ts_event
                1704067200000001000, // ts_recv
                "ESH4",
                "CME",
                502550, // bid_px = 5025.50
                502575, // ask_px = 5025.75
                100,    // bid_sz
                50,     // ask_sz
                2,      // price_scale
                0,      // size_scale
            )
            .unwrap();

        assert_eq!(quote.symbol, "ESH4");
        assert_eq!(quote.exchange, "CME");
        assert_eq!(quote.bid_price, dec!(5025.50));
        assert_eq!(quote.ask_price, dec!(5025.75));
        assert_eq!(quote.bid_size, dec!(100));
        assert_eq!(quote.ask_size, dec!(50));
        // Sequence starts at 0 and increments
        // The first normalized message gets sequence 0
    }

    #[test]
    fn test_normalize_bbo_spread() {
        let mut normalizer = DatabentoNormalizer::new();
        let quote = normalizer
            .normalize_bbo(
                1704067200000000000,
                1704067200000001000,
                "AAPL",
                "NASDAQ",
                17525, // bid = 175.25
                17527, // ask = 175.27
                1000,
                500,
                2,
                0,
            )
            .unwrap();

        // Spread = ask - bid = 175.27 - 175.25 = 0.02
        assert_eq!(quote.spread(), dec!(0.02));
        // Mid price = (175.25 + 175.27) / 2 = 175.26
        assert_eq!(quote.mid_price(), dec!(175.26));
    }

    // =========================================================================
    // L2 (MBP) Tests
    // =========================================================================

    #[test]
    fn test_normalize_mbp_snapshot() {
        let mut normalizer = DatabentoNormalizer::new();

        // 3 bid levels, 3 ask levels
        let bid_levels = vec![
            (502550, 100, 5), // 5025.50 x 100, 5 orders
            (502525, 200, 8), // 5025.25 x 200, 8 orders
            (502500, 300, 12),
        ];
        let ask_levels = vec![
            (502575, 50, 3),  // 5025.75 x 50, 3 orders
            (502600, 150, 6), // 5026.00 x 150, 6 orders
            (502625, 250, 10),
        ];

        let book = normalizer
            .normalize_mbp_snapshot(
                1704067200000000000,
                "ESH4",
                "CME",
                &bid_levels,
                &ask_levels,
                2, // price_scale
                0, // size_scale
            )
            .unwrap();

        assert_eq!(book.symbol, "ESH4");
        assert_eq!(book.exchange, "CME");

        // Check best bid/ask
        let best_bid = book.best_bid().unwrap();
        assert_eq!(best_bid.price, dec!(5025.50));
        assert_eq!(best_bid.size, dec!(100));
        assert_eq!(best_bid.order_count, 5);

        let best_ask = book.best_ask().unwrap();
        assert_eq!(best_ask.price, dec!(5025.75));
        assert_eq!(best_ask.size, dec!(50));
        assert_eq!(best_ask.order_count, 3);

        // Check spread
        assert_eq!(book.spread().unwrap(), dec!(0.25));

        // Check depth (3 levels on each side)
        assert_eq!(book.bids(10).len(), 3);
        assert_eq!(book.asks(10).len(), 3);
    }

    #[test]
    fn test_normalize_mbp_update_add() {
        let mut normalizer = DatabentoNormalizer::new();

        let delta = normalizer
            .normalize_mbp_update(
                1704067200000000000,
                "ESH4",
                "CME",
                'B',    // bid side
                'A',    // add action
                502550, // price = 5025.50
                100,    // size
                5,      // order_count
                2,
                0,
            )
            .unwrap();

        assert_eq!(delta.symbol, "ESH4");
        assert_eq!(delta.side, BookSide::Bid);
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.price, dec!(5025.50));
        assert_eq!(delta.size, dec!(100));
        assert_eq!(delta.order_count, 5);
    }

    #[test]
    fn test_normalize_mbp_update_delete() {
        let mut normalizer = DatabentoNormalizer::new();

        let delta = normalizer
            .normalize_mbp_update(
                1704067200000000000,
                "ESH4",
                "CME",
                'A', // ask side
                'D', // delete action
                502575,
                0, // size = 0 for delete
                0,
                2,
                0,
            )
            .unwrap();

        assert_eq!(delta.side, BookSide::Ask);
        assert_eq!(delta.action, BookAction::Delete);
        assert_eq!(delta.price, dec!(5025.75));
        assert_eq!(delta.size, dec!(0));
    }

    // =========================================================================
    // L3 (MBO) Tests
    // =========================================================================

    #[test]
    fn test_normalize_mbo_add() {
        let mut normalizer = DatabentoNormalizer::new();

        let delta = normalizer
            .normalize_mbo(
                1704067200000000000,
                "ESH4",
                "CME",
                12345678, // order_id
                'B',      // bid side
                'A',      // add action
                502550,   // price
                10,       // size
                2,
                0,
            )
            .unwrap();

        assert_eq!(delta.symbol, "ESH4");
        assert_eq!(delta.side, BookSide::Bid);
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.price, dec!(5025.50));
        assert_eq!(delta.size, dec!(10));
        assert_eq!(delta.order_id, Some(12345678));
    }

    #[test]
    fn test_normalize_mbo_cancel() {
        let mut normalizer = DatabentoNormalizer::new();

        let delta = normalizer
            .normalize_mbo(
                1704067200000000000,
                "ESH4",
                "CME",
                12345678,
                'A', // ask side
                'C', // cancel action
                502575,
                0, // size after cancel = 0
                2,
                0,
            )
            .unwrap();

        assert_eq!(delta.side, BookSide::Ask);
        assert_eq!(delta.action, BookAction::Delete);
        assert_eq!(delta.order_id, Some(12345678));
    }

    #[test]
    fn test_normalize_mbo_trade() {
        let mut normalizer = DatabentoNormalizer::new();

        let delta = normalizer
            .normalize_mbo(
                1704067200000000000,
                "ESH4",
                "CME",
                12345678,
                'B', // bid side
                'T', // trade action (partial/full fill)
                502550,
                5, // remaining size after trade
                2,
                0,
            )
            .unwrap();

        assert_eq!(delta.side, BookSide::Bid);
        assert_eq!(delta.action, BookAction::Update); // Trade updates size
        assert_eq!(delta.size, dec!(5));
    }

    #[test]
    fn test_aggregate_mbo_to_mbp() {
        let ts = DatabentoNormalizer::nanos_to_datetime(1704067200000000000);

        // Simulate L3 orders at different prices
        let mbo_deltas = vec![
            // Bid orders at 5025.50: 3 orders totaling 30
            OrderBookDelta {
                symbol: "ESH4".to_string(),
                exchange: "CME".to_string(),
                ts_event: ts,
                ts_recv: ts,
                side: BookSide::Bid,
                action: BookAction::Add,
                price: dec!(5025.50),
                size: dec!(10),
                sequence: 1,
                order_count: 0,
                flags: 0,
                order_id: Some(1),
            },
            OrderBookDelta {
                symbol: "ESH4".to_string(),
                exchange: "CME".to_string(),
                ts_event: ts,
                ts_recv: ts,
                side: BookSide::Bid,
                action: BookAction::Add,
                price: dec!(5025.50),
                size: dec!(15),
                sequence: 2,
                order_count: 0,
                flags: 0,
                order_id: Some(2),
            },
            // Ask orders at 5025.75
            OrderBookDelta {
                symbol: "ESH4".to_string(),
                exchange: "CME".to_string(),
                ts_event: ts,
                ts_recv: ts,
                side: BookSide::Ask,
                action: BookAction::Add,
                price: dec!(5025.75),
                size: dec!(20),
                sequence: 3,
                order_count: 0,
                flags: 0,
                order_id: Some(3),
            },
        ];

        let book =
            DatabentoNormalizer::aggregate_mbo_to_mbp("ESH4", "CME", ts, 100, &mbo_deltas);

        // Aggregated bid at 5025.50 should be 10 + 15 = 25
        let best_bid = book.best_bid().unwrap();
        assert_eq!(best_bid.price, dec!(5025.50));
        assert_eq!(best_bid.size, dec!(25));

        let best_ask = book.best_ask().unwrap();
        assert_eq!(best_ask.price, dec!(5025.75));
        assert_eq!(best_ask.size, dec!(20));
    }

    #[test]
    fn test_invalid_mbo_side() {
        let mut normalizer = DatabentoNormalizer::new();

        let result = normalizer.normalize_mbo(
            1704067200000000000,
            "ESH4",
            "CME",
            12345678,
            'X', // invalid side
            'A',
            502550,
            10,
            2,
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid MBO side"));
    }

    #[test]
    fn test_invalid_mbp_action() {
        let mut normalizer = DatabentoNormalizer::new();

        let result = normalizer.normalize_mbp_update(
            1704067200000000000,
            "ESH4",
            "CME",
            'B',
            'X', // invalid action
            502550,
            10,
            5,
            2,
            0,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid MBP action"));
    }
}
