//! Kraken WebSocket message types
//!
//! Types for deserializing Kraken WebSocket messages for both Spot (V2) and Futures (V1).

use serde::{Deserialize, Serialize};

// =============================================================================
// KRAKEN SPOT V2 WEBSOCKET TYPES
// =============================================================================

/// Kraken Spot V2 WebSocket trade message wrapper
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotTradeMessage {
    /// Channel name ("trade")
    pub channel: String,
    /// Message type ("update" or "snapshot")
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Trade data array
    pub data: Vec<KrakenSpotTrade>,
}

/// Individual trade in Kraken Spot V2 format
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotTrade {
    /// Trading pair (e.g., "XBT/USD")
    pub symbol: String,
    /// Trade side ("buy" or "sell")
    pub side: String,
    /// Trade price
    pub price: f64,
    /// Trade quantity
    pub qty: f64,
    /// Order type ("market" or "limit")
    pub ord_type: String,
    /// Trade ID
    pub trade_id: i64,
    /// Trade timestamp (RFC3339 format)
    pub timestamp: String,
}

/// Kraken Spot V2 subscription request
#[derive(Debug, Serialize, Clone)]
pub struct KrakenSpotSubscribeMessage {
    /// Method ("subscribe")
    pub method: String,
    /// Subscription parameters
    pub params: KrakenSpotSubscribeParams,
}

/// Kraken Spot V2 subscription parameters
#[derive(Debug, Serialize, Clone)]
pub struct KrakenSpotSubscribeParams {
    /// Channel to subscribe to
    pub channel: String,
    /// Symbols to subscribe to (e.g., ["XBT/USD", "ETH/USD"])
    pub symbol: Vec<String>,
    /// Whether to receive snapshot on subscribe (default: true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
}

/// Kraken Spot V2 subscription parameters with depth (for book channel)
#[derive(Debug, Serialize, Clone)]
pub struct KrakenSpotBookSubscribeParams {
    /// Channel to subscribe to
    pub channel: String,
    /// Symbols to subscribe to
    pub symbol: Vec<String>,
    /// Book depth (10, 25, 100, 500, 1000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Whether to receive snapshot on subscribe
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<bool>,
}

impl KrakenSpotSubscribeMessage {
    /// Create a new trade subscription message
    pub fn trades(symbols: Vec<String>) -> Self {
        Self {
            method: "subscribe".to_string(),
            params: KrakenSpotSubscribeParams {
                channel: "trade".to_string(),
                symbol: symbols,
                snapshot: Some(false), // Don't need historical trades on connect
            },
        }
    }

    /// Create a new ticker subscription message (L1 BBO)
    pub fn ticker(symbols: Vec<String>) -> Self {
        Self {
            method: "subscribe".to_string(),
            params: KrakenSpotSubscribeParams {
                channel: "ticker".to_string(),
                symbol: symbols,
                snapshot: Some(true), // Get initial ticker state
            },
        }
    }

    /// Create a new book subscription message (L2 order book)
    pub fn book(symbols: Vec<String>, depth: u32) -> KrakenSpotBookSubscribeMessage {
        KrakenSpotBookSubscribeMessage {
            method: "subscribe".to_string(),
            params: KrakenSpotBookSubscribeParams {
                channel: "book".to_string(),
                symbol: symbols,
                depth: Some(depth),
                snapshot: Some(true), // Get initial book snapshot
            },
        }
    }
}

/// Kraken Spot V2 book subscription message (needs depth param)
#[derive(Debug, Serialize, Clone)]
pub struct KrakenSpotBookSubscribeMessage {
    /// Method ("subscribe")
    pub method: String,
    /// Subscription parameters with depth
    pub params: KrakenSpotBookSubscribeParams,
}

// =============================================================================
// KRAKEN SPOT V2 TICKER (L1) TYPES
// =============================================================================

/// Kraken Spot V2 ticker message wrapper
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotTickerMessage {
    /// Channel name ("ticker")
    pub channel: String,
    /// Message type ("update" or "snapshot") - kept for completeness
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub msg_type: String,
    /// Ticker data array
    pub data: Vec<KrakenSpotTicker>,
}

/// Individual ticker in Kraken Spot V2 format
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotTicker {
    /// Trading pair (e.g., "XBT/USD")
    pub symbol: String,
    /// Best bid price
    pub bid: f64,
    /// Best bid quantity
    pub bid_qty: f64,
    /// Best ask price
    pub ask: f64,
    /// Best ask quantity
    pub ask_qty: f64,
    /// Last trade price
    pub last: f64,
    /// 24h volume
    pub volume: f64,
    /// 24h VWAP
    pub vwap: f64,
    /// 24h low
    pub low: f64,
    /// 24h high
    pub high: f64,
    /// 24h price change
    pub change: f64,
    /// 24h price change percentage
    pub change_pct: f64,
}

// =============================================================================
// KRAKEN SPOT V2 BOOK (L2) TYPES
// =============================================================================

/// Kraken Spot V2 book message wrapper
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotBookMessage {
    /// Channel name ("book")
    pub channel: String,
    /// Message type ("snapshot" or "update")
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Book data array
    pub data: Vec<KrakenSpotBook>,
}

/// Individual book update in Kraken Spot V2 format
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotBook {
    /// Trading pair (e.g., "XBT/USD")
    pub symbol: String,
    /// Bid levels (price, qty)
    #[serde(default)]
    pub bids: Vec<KrakenSpotBookLevel>,
    /// Ask levels (price, qty)
    #[serde(default)]
    pub asks: Vec<KrakenSpotBookLevel>,
    /// Checksum for validation
    #[serde(default)]
    pub checksum: u32,
    /// Timestamp (RFC3339)
    #[serde(default)]
    pub timestamp: Option<String>,
}

/// Book level in Kraken Spot V2 format
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotBookLevel {
    /// Price
    pub price: f64,
    /// Quantity (0 means delete level)
    pub qty: f64,
}

/// Kraken Spot V2 generic response for subscription confirmations
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenSpotResponse {
    /// Method that was called
    #[allow(dead_code)]
    pub method: Option<String>,
    /// Result of the subscription
    #[allow(dead_code)]
    pub result: Option<KrakenSpotSubscribeResult>,
    /// Success indicator
    pub success: Option<bool>,
    /// Error message if any
    pub error: Option<String>,
    /// Time in millis
    #[allow(dead_code)]
    pub time_in: Option<String>,
    /// Time out millis
    #[allow(dead_code)]
    pub time_out: Option<String>,
}

/// Result of a Kraken Spot subscription
#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct KrakenSpotSubscribeResult {
    /// Channel subscribed to
    pub channel: Option<String>,
    /// Symbols subscribed to
    pub symbol: Option<String>,
    /// Snapshot setting
    pub snapshot: Option<bool>,
}

// =============================================================================
// KRAKEN FUTURES V1 WEBSOCKET TYPES
// =============================================================================

/// Kraken Futures V1 WebSocket trade message
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenFuturesTradeMessage {
    /// Feed name ("trade")
    pub feed: String,
    /// Product ID (e.g., "PI_XBTUSD")
    pub product_id: String,
    /// Trade side ("buy" or "sell")
    pub side: String,
    /// Trade price
    pub price: f64,
    /// Trade quantity (contracts)
    pub qty: f64,
    /// Trade sequence number (used as trade ID)
    pub seq: i64,
    /// Unix timestamp in milliseconds
    pub time: i64,
    /// Trade type (e.g., "fill")
    #[serde(rename = "type")]
    pub trade_type: Option<String>,
}

/// Kraken Futures V1 subscription request
#[derive(Debug, Serialize, Clone)]
pub struct KrakenFuturesSubscribeMessage {
    /// Event type ("subscribe")
    pub event: String,
    /// Feed to subscribe to ("trade")
    pub feed: String,
    /// Product IDs to subscribe to
    pub product_ids: Vec<String>,
}

impl KrakenFuturesSubscribeMessage {
    /// Create a new trade subscription message
    pub fn trades(product_ids: Vec<String>) -> Self {
        Self {
            event: "subscribe".to_string(),
            feed: "trade".to_string(),
            product_ids,
        }
    }

    /// Create a new ticker subscription message (L1 BBO)
    pub fn ticker(product_ids: Vec<String>) -> Self {
        Self {
            event: "subscribe".to_string(),
            feed: "ticker".to_string(),
            product_ids,
        }
    }

    /// Create a new book subscription message (L2 order book)
    pub fn book(product_ids: Vec<String>) -> Self {
        Self {
            event: "subscribe".to_string(),
            feed: "book".to_string(),
            product_ids,
        }
    }
}

// =============================================================================
// KRAKEN FUTURES V1 TICKER (L1) TYPES
// =============================================================================

/// Kraken Futures V1 ticker message
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenFuturesTickerMessage {
    /// Feed name ("ticker")
    pub feed: String,
    /// Product ID (e.g., "PI_XBTUSD")
    pub product_id: String,
    /// Best bid price
    pub bid: f64,
    /// Best ask price
    pub ask: f64,
    /// Best bid size
    pub bid_size: f64,
    /// Best ask size
    pub ask_size: f64,
    /// Last trade price
    #[serde(default)]
    pub last: f64,
    /// Last trade size
    #[serde(default)]
    pub last_size: f64,
    /// 24h volume
    #[serde(default)]
    pub volume: f64,
    /// Open interest
    #[serde(default)]
    pub open_interest: f64,
    /// Mark price
    #[serde(default)]
    pub mark_price: f64,
    /// Unix timestamp in milliseconds
    pub time: i64,
}

// =============================================================================
// KRAKEN FUTURES V1 BOOK (L2) TYPES
// =============================================================================

/// Kraken Futures V1 book snapshot message
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenFuturesBookSnapshotMessage {
    /// Feed name ("book_snapshot")
    pub feed: String,
    /// Product ID (e.g., "PI_XBTUSD")
    pub product_id: String,
    /// Unix timestamp in milliseconds
    pub timestamp: i64,
    /// Sequence number
    pub seq: i64,
    /// Bid levels
    pub bids: Vec<KrakenFuturesBookLevel>,
    /// Ask levels
    pub asks: Vec<KrakenFuturesBookLevel>,
}

/// Kraken Futures V1 book update message (delta)
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenFuturesBookUpdateMessage {
    /// Feed name ("book")
    pub feed: String,
    /// Product ID (e.g., "PI_XBTUSD")
    pub product_id: String,
    /// Sequence number
    pub seq: i64,
    /// Unix timestamp in milliseconds
    pub timestamp: i64,
    /// Side ("buy" or "sell")
    pub side: String,
    /// Price level
    pub price: f64,
    /// Quantity (0 means delete)
    pub qty: f64,
}

/// Book level in Kraken Futures V1 format
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenFuturesBookLevel {
    /// Price
    pub price: f64,
    /// Quantity
    pub qty: f64,
}

/// Kraken Futures V1 subscription response
#[derive(Debug, Deserialize, Clone)]
pub struct KrakenFuturesResponse {
    /// Event type
    pub event: Option<String>,
    /// Feed name
    #[allow(dead_code)]
    pub feed: Option<String>,
    /// Product IDs subscribed
    pub product_ids: Option<Vec<String>>,
    /// Error message if any
    pub error: Option<String>,
    /// Message (for info events)
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spot_trade_message() {
        let json = r#"{
            "channel": "trade",
            "type": "update",
            "data": [{
                "symbol": "XBT/USD",
                "side": "buy",
                "price": 50000.0,
                "qty": 0.001,
                "ord_type": "market",
                "trade_id": 12345,
                "timestamp": "2024-01-15T10:30:00.123456Z"
            }]
        }"#;

        let msg: KrakenSpotTradeMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.channel, "trade");
        assert_eq!(msg.msg_type, "update");
        assert_eq!(msg.data.len(), 1);
        assert_eq!(msg.data[0].symbol, "XBT/USD");
        assert_eq!(msg.data[0].side, "buy");
        assert_eq!(msg.data[0].price, 50000.0);
        assert_eq!(msg.data[0].trade_id, 12345);
    }

    #[test]
    fn test_parse_futures_trade_message() {
        let json = r#"{
            "feed": "trade",
            "product_id": "PI_XBTUSD",
            "side": "sell",
            "price": 50100.5,
            "qty": 1.0,
            "seq": 67890,
            "time": 1705315800123,
            "type": "fill"
        }"#;

        let msg: KrakenFuturesTradeMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.feed, "trade");
        assert_eq!(msg.product_id, "PI_XBTUSD");
        assert_eq!(msg.side, "sell");
        assert_eq!(msg.price, 50100.5);
        assert_eq!(msg.qty, 1.0);
        assert_eq!(msg.seq, 67890);
        assert_eq!(msg.time, 1705315800123);
    }

    #[test]
    fn test_spot_subscribe_message() {
        let msg = KrakenSpotSubscribeMessage::trades(vec![
            "XBT/USD".to_string(),
            "ETH/USD".to_string(),
        ]);

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"method\":\"subscribe\""));
        assert!(json.contains("\"channel\":\"trade\""));
        assert!(json.contains("XBT/USD"));
    }

    #[test]
    fn test_futures_subscribe_message() {
        let msg = KrakenFuturesSubscribeMessage::trades(vec![
            "PI_XBTUSD".to_string(),
            "PI_ETHUSD".to_string(),
        ]);

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"event\":\"subscribe\""));
        assert!(json.contains("\"feed\":\"trade\""));
        assert!(json.contains("PI_XBTUSD"));
    }

    #[test]
    fn test_parse_spot_response() {
        let json = r#"{
            "method": "subscribe",
            "result": {
                "channel": "trade",
                "symbol": "XBT/USD",
                "snapshot": false
            },
            "success": true,
            "time_in": "2024-01-15T10:30:00.000000Z",
            "time_out": "2024-01-15T10:30:00.001000Z"
        }"#;

        let resp: KrakenSpotResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.success, Some(true));
        assert!(resp.result.is_some());
    }

    #[test]
    fn test_parse_futures_response() {
        let json = r#"{
            "event": "subscribed",
            "feed": "trade",
            "product_ids": ["PI_XBTUSD", "PI_ETHUSD"]
        }"#;

        let resp: KrakenFuturesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.event, Some("subscribed".to_string()));
        assert_eq!(resp.feed, Some("trade".to_string()));
    }

    #[test]
    fn test_spot_ticker_subscribe() {
        let msg = KrakenSpotSubscribeMessage::ticker(vec!["XBT/USD".to_string()]);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"channel\":\"ticker\""));
    }

    #[test]
    fn test_spot_book_subscribe() {
        let msg = KrakenSpotSubscribeMessage::book(vec!["XBT/USD".to_string()], 10);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"channel\":\"book\""));
        assert!(json.contains("\"depth\":10"));
    }

    #[test]
    fn test_parse_spot_ticker_message() {
        let json = r#"{
            "channel": "ticker",
            "type": "update",
            "data": [{
                "symbol": "XBT/USD",
                "bid": 50000.0,
                "bid_qty": 1.5,
                "ask": 50001.0,
                "ask_qty": 2.0,
                "last": 50000.5,
                "volume": 1000.0,
                "vwap": 49500.0,
                "low": 49000.0,
                "high": 51000.0,
                "change": 500.0,
                "change_pct": 1.0
            }]
        }"#;

        let msg: KrakenSpotTickerMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.channel, "ticker");
        assert_eq!(msg.data.len(), 1);
        assert_eq!(msg.data[0].bid, 50000.0);
        assert_eq!(msg.data[0].ask, 50001.0);
    }

    #[test]
    fn test_parse_spot_book_message() {
        let json = r#"{
            "channel": "book",
            "type": "snapshot",
            "data": [{
                "symbol": "XBT/USD",
                "bids": [
                    {"price": 50000.0, "qty": 1.5},
                    {"price": 49999.0, "qty": 2.0}
                ],
                "asks": [
                    {"price": 50001.0, "qty": 1.0},
                    {"price": 50002.0, "qty": 3.0}
                ],
                "checksum": 123456,
                "timestamp": "2024-01-15T10:30:00.123456Z"
            }]
        }"#;

        let msg: KrakenSpotBookMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.channel, "book");
        assert_eq!(msg.msg_type, "snapshot");
        assert_eq!(msg.data[0].bids.len(), 2);
        assert_eq!(msg.data[0].asks.len(), 2);
        assert_eq!(msg.data[0].bids[0].price, 50000.0);
    }

    #[test]
    fn test_futures_ticker_subscribe() {
        let msg = KrakenFuturesSubscribeMessage::ticker(vec!["PI_XBTUSD".to_string()]);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"feed\":\"ticker\""));
    }

    #[test]
    fn test_futures_book_subscribe() {
        let msg = KrakenFuturesSubscribeMessage::book(vec!["PI_XBTUSD".to_string()]);
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"feed\":\"book\""));
    }

    #[test]
    fn test_parse_futures_ticker_message() {
        let json = r#"{
            "feed": "ticker",
            "product_id": "PI_XBTUSD",
            "bid": 50000.0,
            "ask": 50001.0,
            "bid_size": 100.0,
            "ask_size": 150.0,
            "last": 50000.5,
            "last_size": 10.0,
            "volume": 50000.0,
            "open_interest": 100000.0,
            "mark_price": 50000.25,
            "time": 1705315800123
        }"#;

        let msg: KrakenFuturesTickerMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.feed, "ticker");
        assert_eq!(msg.product_id, "PI_XBTUSD");
        assert_eq!(msg.bid, 50000.0);
        assert_eq!(msg.ask, 50001.0);
    }

    #[test]
    fn test_parse_futures_book_snapshot() {
        let json = r#"{
            "feed": "book_snapshot",
            "product_id": "PI_XBTUSD",
            "timestamp": 1705315800123,
            "seq": 12345,
            "bids": [
                {"price": 50000.0, "qty": 100.0},
                {"price": 49999.0, "qty": 200.0}
            ],
            "asks": [
                {"price": 50001.0, "qty": 150.0},
                {"price": 50002.0, "qty": 250.0}
            ]
        }"#;

        let msg: KrakenFuturesBookSnapshotMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.feed, "book_snapshot");
        assert_eq!(msg.bids.len(), 2);
        assert_eq!(msg.asks.len(), 2);
    }

    #[test]
    fn test_parse_futures_book_update() {
        let json = r#"{
            "feed": "book",
            "product_id": "PI_XBTUSD",
            "seq": 12346,
            "timestamp": 1705315800124,
            "side": "buy",
            "price": 50000.0,
            "qty": 150.0
        }"#;

        let msg: KrakenFuturesBookUpdateMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.feed, "book");
        assert_eq!(msg.side, "buy");
        assert_eq!(msg.price, 50000.0);
    }
}
