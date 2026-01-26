//! Kraken Spot API types.
//!
//! These types represent Kraken Spot REST API requests and responses.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::execution::venue::kraken::common::{
    KrakenOrderSide, KrakenOrderStatus, KrakenOrderType,
};

// =============================================================================
// PUBLIC API RESPONSES
// =============================================================================

/// System status response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotSystemStatus {
    /// System status (online, maintenance, etc.)
    pub status: String,
    /// Timestamp
    pub timestamp: String,
}

/// Server time response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotServerTime {
    /// Unix timestamp
    pub unixtime: i64,
    /// RFC 1123 formatted time
    pub rfc1123: String,
}

// =============================================================================
// ACCOUNT API RESPONSES
// =============================================================================

/// Account balance response.
///
/// Returns a map of asset -> balance.
/// Example: {"ZUSD": "1000.0000", "XXBT": "0.5000"}
#[derive(Debug, Clone, Deserialize)]
pub struct SpotAccountBalance(pub HashMap<String, String>);

impl SpotAccountBalance {
    /// Get balance for an asset.
    pub fn get(&self, asset: &str) -> Option<Decimal> {
        self.0.get(asset).and_then(|s| s.parse().ok())
    }

    /// Get all balances as (asset, amount) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, Decimal)> {
        self.0
            .iter()
            .filter_map(|(k, v)| v.parse::<Decimal>().ok().map(|d| (k, d)))
    }

    /// Get assets with non-zero balance.
    pub fn non_zero(&self) -> Vec<(String, Decimal)> {
        self.iter()
            .filter(|(_, v)| *v != Decimal::ZERO)
            .map(|(k, v)| (k.clone(), v))
            .collect()
    }
}

/// Extended balance entry with hold amounts.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SpotBalanceEntry {
    /// Available balance
    pub balance: String,
    /// Credit amount (if applicable)
    #[serde(default)]
    pub credit: Option<String>,
    /// Credit used
    #[serde(default)]
    pub credit_used: Option<String>,
    /// Amount on hold (pending orders)
    #[serde(default)]
    pub hold_trade: Option<String>,
}

/// Extended account balance response.
#[allow(dead_code)]
pub type SpotAccountBalanceEx = HashMap<String, SpotBalanceEntry>;

// =============================================================================
// ORDER API TYPES
// =============================================================================

/// Add order request parameters.
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)]
pub struct AddOrderRequest {
    /// Trading pair (e.g., "XBTUSD")
    pub pair: String,
    /// Order side
    #[serde(rename = "type")]
    pub side: String,
    /// Order type
    pub ordertype: String,
    /// Order volume
    pub volume: String,
    /// Price (for limit orders)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Secondary price (for stop-limit orders)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price2: Option<String>,
    /// Trigger type for stop orders
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Leverage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leverage: Option<String>,
    /// Start time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starttm: Option<String>,
    /// Expire time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expiretm: Option<String>,
    /// User reference ID (max 32 bit signed int)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub userref: Option<i32>,
    /// Client order ID (max 45 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cl_ord_id: Option<String>,
    /// Order flags (comma-delimited list)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oflags: Option<String>,
    /// Time in force
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeinforce: Option<String>,
    /// Validate only (don't submit)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validate: Option<bool>,
}

/// Add order response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotAddOrderResponse {
    /// Order description
    pub descr: OrderDescription,
    /// Transaction IDs (order IDs)
    pub txid: Vec<String>,
}

/// Order description from add order response.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderDescription {
    /// Order description string
    pub order: String,
    /// Close order description (if applicable)
    #[serde(default)]
    pub close: Option<String>,
}

/// Amend order response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotAmendOrderResponse {
    /// Original transaction ID
    #[serde(default)]
    pub txid: Option<String>,
    /// Amended order ID
    #[serde(default)]
    pub amend_id: Option<String>,
    /// Order description
    #[serde(default)]
    pub descr: Option<OrderDescription>,
}

/// Cancel order response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotCancelOrderResponse {
    /// Number of orders cancelled
    pub count: i32,
    /// Whether pending orders remain
    #[serde(default)]
    pub pending: Option<bool>,
}

/// Cancel all orders response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotCancelAllResponse {
    /// Number of orders cancelled
    pub count: i32,
}

// =============================================================================
// ORDER QUERY TYPES
// =============================================================================

/// Order info from query.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotOrderInfo {
    /// Reference ID
    #[serde(default)]
    pub refid: Option<String>,
    /// User reference ID
    #[serde(default)]
    pub userref: Option<i32>,
    /// Client order ID
    #[serde(default)]
    pub cl_ord_id: Option<String>,
    /// Order status
    pub status: String,
    /// Open timestamp
    #[serde(default)]
    pub opentm: Option<f64>,
    /// Start timestamp
    #[serde(default)]
    pub starttm: Option<f64>,
    /// Expire timestamp
    #[serde(default)]
    pub expiretm: Option<f64>,
    /// Close timestamp
    #[serde(default)]
    pub closetm: Option<f64>,
    /// Order description
    pub descr: SpotOrderDescr,
    /// Volume ordered
    pub vol: String,
    /// Volume executed
    pub vol_exec: String,
    /// Total cost (quote currency)
    #[serde(default)]
    pub cost: Option<String>,
    /// Fee amount
    #[serde(default)]
    pub fee: Option<String>,
    /// Average price
    #[serde(default)]
    pub price: Option<String>,
    /// Stop price
    #[serde(default)]
    pub stopprice: Option<String>,
    /// Limit price
    #[serde(default)]
    pub limitprice: Option<String>,
    /// Triggered price
    #[serde(default)]
    pub trigger: Option<String>,
    /// Miscellaneous info
    #[serde(default)]
    pub misc: Option<String>,
    /// Order flags
    #[serde(default)]
    pub oflags: Option<String>,
    /// Reason for status
    #[serde(default)]
    pub reason: Option<String>,
    /// Trades (if closed)
    #[serde(default)]
    pub trades: Option<Vec<String>>,
}

impl SpotOrderInfo {
    /// Parse the order status.
    pub fn parsed_status(&self) -> KrakenOrderStatus {
        match self.status.as_str() {
            "pending" => KrakenOrderStatus::Pending,
            "open" => KrakenOrderStatus::Open,
            "closed" => KrakenOrderStatus::Closed,
            "canceled" | "cancelled" => KrakenOrderStatus::Canceled,
            "expired" => KrakenOrderStatus::Expired,
            _ => KrakenOrderStatus::Open,
        }
    }

    /// Get the order side.
    pub fn side(&self) -> KrakenOrderSide {
        if self.descr.type_field.to_lowercase() == "buy" {
            KrakenOrderSide::Buy
        } else {
            KrakenOrderSide::Sell
        }
    }

    /// Get the order type.
    pub fn order_type(&self) -> KrakenOrderType {
        match self.descr.ordertype.as_str() {
            "market" => KrakenOrderType::Market,
            "limit" => KrakenOrderType::Limit,
            "stop-loss" => KrakenOrderType::StopLoss,
            "stop-loss-limit" => KrakenOrderType::StopLossLimit,
            "take-profit" => KrakenOrderType::TakeProfit,
            "take-profit-limit" => KrakenOrderType::TakeProfitLimit,
            _ => KrakenOrderType::Limit,
        }
    }
}

/// Order description from order info.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotOrderDescr {
    /// Trading pair
    pub pair: String,
    /// Order side ("buy" or "sell")
    #[serde(rename = "type")]
    pub type_field: String,
    /// Order type
    pub ordertype: String,
    /// Price
    pub price: String,
    /// Secondary price
    #[serde(default)]
    pub price2: Option<String>,
    /// Leverage
    #[serde(default)]
    pub leverage: Option<String>,
    /// Order description
    pub order: String,
    /// Close description
    #[serde(default)]
    pub close: Option<String>,
}

/// Open orders response.
#[derive(Debug, Clone, Deserialize)]
pub struct SpotOpenOrdersResponse {
    /// Map of txid -> order info
    pub open: HashMap<String, SpotOrderInfo>,
}

/// Query orders response.
pub type SpotQueryOrdersResponse = HashMap<String, SpotOrderInfo>;

// =============================================================================
// WEBSOCKET EXECUTION TYPES
// =============================================================================

/// WebSocket V2 execution message.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WsExecutionMessage {
    /// Channel name
    pub channel: String,
    /// Message type (snapshot or update)
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Execution data
    pub data: Vec<WsExecutionData>,
}

/// WebSocket execution data entry.
#[derive(Debug, Clone, Deserialize)]
pub struct WsExecutionData {
    /// Execution type
    pub exec_type: String,
    /// Order ID
    pub order_id: String,
    /// Client order ID
    #[serde(default)]
    pub cl_ord_id: Option<String>,
    /// Symbol
    pub symbol: String,
    /// Order side
    pub side: String,
    /// Order type
    pub order_type: String,
    /// Order quantity
    pub order_qty: String,
    /// Order status
    pub order_status: String,
    /// Limit price
    #[serde(default)]
    pub limit_price: Option<String>,
    /// Stop price
    #[serde(default)]
    pub stop_price: Option<String>,
    /// Time in force
    #[serde(default)]
    pub time_in_force: Option<String>,
    /// Execution ID (for trades)
    #[serde(default)]
    pub exec_id: Option<String>,
    /// Last quantity (for trades)
    #[serde(default)]
    pub last_qty: Option<String>,
    /// Last price (for trades)
    #[serde(default)]
    pub last_price: Option<String>,
    /// Cumulative quantity
    #[serde(default)]
    pub cum_qty: Option<String>,
    /// Average price
    #[serde(default)]
    pub avg_price: Option<String>,
    /// Liquidity indicator (maker/taker)
    #[serde(default)]
    pub liquidity_ind: Option<String>,
    /// Fees
    #[serde(default)]
    pub fees: Option<Vec<WsFeeInfo>>,
    /// Timestamp
    pub timestamp: String,
}

/// Fee information from WebSocket.
#[derive(Debug, Clone, Deserialize)]
pub struct WsFeeInfo {
    /// Fee asset
    pub asset: String,
    /// Fee quantity
    pub qty: String,
}

// =============================================================================
// SYMBOL MAPPING
// =============================================================================

/// Convert internal symbol format to Kraken format.
///
/// Examples:
/// - "BTCUSDT" -> "XBTUSD" or "BTC/USD"
/// - "ETHUSDT" -> "ETHUSD" or "ETH/USD"
pub fn to_kraken_symbol(symbol: &str) -> String {
    // Simple conversion - may need a more complete mapping table
    let symbol = symbol.to_uppercase();

    // Handle common transformations
    let symbol = symbol.replace("USDT", "USD");

    // Kraken uses XBT for Bitcoin
    let symbol = symbol.replace("BTC", "XBT");

    symbol
}

/// Convert Kraken symbol format to internal format.
///
/// Examples:
/// - "XBTUSD" -> "BTCUSDT"
/// - "BTC/USD" -> "BTCUSDT"
pub fn from_kraken_symbol(kraken_symbol: &str) -> String {
    let symbol = kraken_symbol.to_uppercase();

    // Remove slash if present
    let symbol = symbol.replace('/', "");

    // Convert XBT to BTC
    let symbol = symbol.replace("XBT", "BTC");

    // Convert USD to USDT (most common use case)
    let symbol = if symbol.ends_with("USD") && !symbol.ends_with("USDT") {
        format!("{}T", symbol)
    } else {
        symbol
    };

    symbol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_balance() {
        let mut map = HashMap::new();
        map.insert("ZUSD".to_string(), "1000.0000".to_string());
        map.insert("XXBT".to_string(), "0.5000".to_string());
        map.insert("XETH".to_string(), "0.0000".to_string());

        let balance = SpotAccountBalance(map);

        assert_eq!(balance.get("ZUSD"), Some(Decimal::new(10000000, 4)));
        assert_eq!(balance.get("XXBT"), Some(Decimal::new(5000, 4)));
        assert_eq!(balance.non_zero().len(), 2);
    }

    #[test]
    fn test_to_kraken_symbol() {
        assert_eq!(to_kraken_symbol("BTCUSDT"), "XBTUSD");
        assert_eq!(to_kraken_symbol("ETHUSDT"), "ETHUSD");
        assert_eq!(to_kraken_symbol("XRPUSD"), "XRPUSD");
    }

    #[test]
    fn test_from_kraken_symbol() {
        assert_eq!(from_kraken_symbol("XBTUSD"), "BTCUSDT");
        assert_eq!(from_kraken_symbol("BTC/USD"), "BTCUSDT");
        assert_eq!(from_kraken_symbol("ETHUSD"), "ETHUSDT");
    }

    #[test]
    fn test_order_info_parsing() {
        let json = r#"{
            "status": "open",
            "vol": "1.0",
            "vol_exec": "0.5",
            "descr": {
                "pair": "XBTUSD",
                "type": "buy",
                "ordertype": "limit",
                "price": "50000",
                "order": "buy 1.0 XBTUSD @ limit 50000"
            }
        }"#;

        let info: SpotOrderInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.parsed_status(), KrakenOrderStatus::Open);
        assert_eq!(info.side(), KrakenOrderSide::Buy);
        assert_eq!(info.order_type(), KrakenOrderType::Limit);
    }
}
