//! Binance message types
//!
//! Types for deserializing Binance WebSocket messages.

use serde::{Deserialize, Serialize};

/// Binance specific trade message format
#[derive(Debug, Deserialize, Clone)]
pub struct BinanceTradeMessage {
    /// Symbol
    #[serde(rename = "s")]
    pub symbol: String,

    /// Trade ID
    #[serde(rename = "t")]
    pub trade_id: u64,

    /// Price
    #[serde(rename = "p")]
    pub price: String,

    /// Quantity
    #[serde(rename = "q")]
    pub quantity: String,

    /// Trade time
    #[serde(rename = "T")]
    pub trade_time: u64,

    /// Is the buyer the market maker?
    #[serde(rename = "m")]
    pub is_buyer_maker: bool,
}

/// Binance WebSocket stream wrapper for combined streams
#[derive(Debug, Deserialize)]
pub struct BinanceStreamMessage {
    /// Stream name (e.g., "btcusdt@trade")
    #[allow(dead_code)]
    pub stream: String,

    /// The actual trade data
    pub data: BinanceTradeMessage,
}

/// Binance subscription message format
#[derive(Debug, Serialize)]
pub struct BinanceSubscribeMessage {
    pub method: String,
    pub params: Vec<String>,
    pub id: u32,
}

impl BinanceSubscribeMessage {
    pub fn new(streams: Vec<String>) -> Self {
        Self {
            method: "SUBSCRIBE".to_string(),
            params: streams,
            id: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stream_message() {
        let json = r#"{
            "stream": "btcusdt@trade",
            "data": {
                "e": "trade",
                "E": 1672515782136,
                "s": "BTCUSDT",
                "t": 12345,
                "p": "50000.00",
                "q": "0.001",
                "b": 88,
                "a": 50,
                "T": 1672515782136,
                "m": false,
                "M": true
            }
        }"#;

        let msg: BinanceStreamMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.data.symbol, "BTCUSDT");
        assert_eq!(msg.data.trade_id, 12345);
        assert_eq!(msg.data.price, "50000.00");
        assert!(!msg.data.is_buyer_maker);
    }

    #[test]
    fn test_subscribe_message() {
        let msg = BinanceSubscribeMessage::new(vec![
            "btcusdt@trade".to_string(),
            "ethusdt@trade".to_string(),
        ]);

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("SUBSCRIBE"));
        assert!(json.contains("btcusdt@trade"));
    }
}
