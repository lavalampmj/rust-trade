//! Kraken Futures API types.
//!
//! These types represent Kraken Futures REST API requests and responses.
//! Futures uses a different API structure than Spot (v3 derivatives API).

use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;

use crate::execution::venue::kraken::common::{
    KrakenOrderSide, KrakenOrderStatus, KrakenOrderType,
};

// =============================================================================
// COMMON RESPONSE WRAPPER
// =============================================================================

/// Kraken Futures API response wrapper.
///
/// Futures uses `result: "success"` instead of `error: []`.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesResponse<T> {
    /// Result status ("success" or "error")
    pub result: String,
    /// Server time
    #[serde(rename = "serverTime")]
    #[serde(default)]
    pub server_time: Option<String>,
    /// Error message (if result is "error")
    #[serde(default)]
    pub error: Option<String>,
    /// Response data (flattened into response)
    #[serde(flatten)]
    pub data: Option<T>,
}

impl<T> FuturesResponse<T> {
    /// Check if the response is successful.
    pub fn is_success(&self) -> bool {
        self.result == "success"
    }

    /// Get the error message if present.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

// =============================================================================
// INSTRUMENT/TICKER TYPES
// =============================================================================

/// Futures instrument info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesInstrument {
    /// Instrument symbol (e.g., "PI_XBTUSD")
    pub symbol: String,
    /// Instrument type (e.g., "perpetual", "futures_inverse")
    #[serde(rename = "type")]
    pub instrument_type: String,
    /// Underlying asset
    #[serde(default)]
    pub underlying: Option<String>,
    /// Tick size
    #[serde(rename = "tickSize")]
    #[serde(default)]
    pub tick_size: Option<Decimal>,
    /// Contract size
    #[serde(rename = "contractSize")]
    #[serde(default)]
    pub contract_size: Option<Decimal>,
    /// Maximum leverage
    #[serde(rename = "maxLeverage")]
    #[serde(default)]
    pub max_leverage: Option<i32>,
    /// Is tradeable
    #[serde(default)]
    pub tradeable: Option<bool>,
}

/// Instruments response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesInstrumentsData {
    /// List of instruments
    pub instruments: Vec<FuturesInstrument>,
}

/// Ticker info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesTicker {
    /// Symbol
    pub symbol: String,
    /// Last trade price
    #[serde(default)]
    pub last: Option<Decimal>,
    /// Best bid price
    #[serde(default)]
    pub bid: Option<Decimal>,
    /// Best ask price
    #[serde(default)]
    pub ask: Option<Decimal>,
    /// Mark price
    #[serde(rename = "markPrice")]
    #[serde(default)]
    pub mark_price: Option<Decimal>,
    /// Index price
    #[serde(rename = "indexPrice")]
    #[serde(default)]
    pub index_price: Option<Decimal>,
    /// 24h volume
    #[serde(default)]
    pub vol24h: Option<Decimal>,
    /// Open interest
    #[serde(rename = "openInterest")]
    #[serde(default)]
    pub open_interest: Option<Decimal>,
    /// Funding rate
    #[serde(rename = "fundingRate")]
    #[serde(default)]
    pub funding_rate: Option<Decimal>,
}

/// Tickers response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesTickersData {
    /// List of tickers
    pub tickers: Vec<FuturesTicker>,
}

// =============================================================================
// ACCOUNT TYPES
// =============================================================================

/// Futures account info.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesAccountInfo {
    /// Account type
    #[serde(rename = "type")]
    #[serde(default)]
    pub account_type: Option<String>,
    /// Currency
    #[serde(default)]
    pub currency: Option<String>,
    /// Total balance
    #[serde(default)]
    pub balance: Option<Decimal>,
    /// Available balance
    #[serde(default)]
    pub available: Option<Decimal>,
    /// Margin info
    #[serde(default)]
    pub margin: Option<FuturesMarginInfo>,
}

/// Futures margin info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesMarginInfo {
    /// Initial margin
    #[serde(rename = "initialMargin")]
    #[serde(default)]
    pub initial_margin: Option<Decimal>,
    /// Maintenance margin
    #[serde(rename = "maintenanceMargin")]
    #[serde(default)]
    pub maintenance_margin: Option<Decimal>,
    /// Unrealized PnL
    #[serde(rename = "unrealizedFunding")]
    #[serde(default)]
    pub unrealized_funding: Option<Decimal>,
    /// Portfolio value
    #[serde(rename = "portfolioValue")]
    #[serde(default)]
    pub portfolio_value: Option<Decimal>,
}

/// Accounts response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesAccountsData {
    /// List of accounts (cash, margin accounts)
    pub accounts: HashMap<String, FuturesAccountInfo>,
}

// =============================================================================
// ORDER TYPES
// =============================================================================

/// Send order response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesSendOrderData {
    /// Order ID
    #[serde(rename = "orderId")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cliOrdId")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// Send status
    #[serde(rename = "sendStatus")]
    #[serde(default)]
    pub send_status: Option<FuturesSendStatus>,
}

/// Order send status details.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesSendStatus {
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Status
    #[serde(default)]
    pub status: Option<String>,
    /// Received time
    #[serde(rename = "receivedTime")]
    #[serde(default)]
    pub received_time: Option<String>,
    /// Order events
    #[serde(rename = "orderEvents")]
    #[serde(default)]
    pub order_events: Option<Vec<FuturesOrderEvent>>,
}

/// Futures order event from send/edit.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesOrderEvent {
    /// Old order (for edit)
    #[serde(default)]
    pub old: Option<FuturesOrderDetails>,
    /// New order
    #[serde(default)]
    pub new: Option<FuturesOrderDetails>,
    /// Execution (for fill)
    #[serde(default)]
    pub execution: Option<FuturesExecution>,
    /// Event type
    #[serde(rename = "type")]
    #[serde(default)]
    pub event_type: Option<String>,
}

/// Futures order details.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesOrderDetails {
    /// Order ID
    #[serde(rename = "orderId")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cliOrdId")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// Symbol
    #[serde(default)]
    pub symbol: Option<String>,
    /// Side
    #[serde(default)]
    pub side: Option<String>,
    /// Order type
    #[serde(rename = "orderType")]
    #[serde(default)]
    pub order_type: Option<String>,
    /// Limit price
    #[serde(rename = "limitPrice")]
    #[serde(default)]
    pub limit_price: Option<Decimal>,
    /// Stop price
    #[serde(rename = "stopPrice")]
    #[serde(default)]
    pub stop_price: Option<Decimal>,
    /// Quantity
    #[serde(default)]
    pub quantity: Option<Decimal>,
    /// Filled quantity
    #[serde(default)]
    pub filled: Option<Decimal>,
    /// Reduce only
    #[serde(rename = "reduceOnly")]
    #[serde(default)]
    pub reduce_only: Option<bool>,
}

/// Futures execution (fill) info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesExecution {
    /// Execution ID
    #[serde(rename = "uid")]
    #[serde(default)]
    pub uid: Option<String>,
    /// Price
    #[serde(default)]
    pub price: Option<Decimal>,
    /// Quantity
    #[serde(default)]
    pub quantity: Option<Decimal>,
    /// Taker/maker
    #[serde(rename = "takerReducedQuantity")]
    #[serde(default)]
    pub taker_reduced_quantity: Option<Decimal>,
}

/// Edit order response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesEditOrderData {
    /// Edit status
    #[serde(rename = "editStatus")]
    #[serde(default)]
    pub edit_status: Option<FuturesSendStatus>,
}

/// Cancel order response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesCancelOrderData {
    /// Cancel status
    #[serde(rename = "cancelStatus")]
    #[serde(default)]
    pub cancel_status: Option<FuturesCancelStatus>,
}

/// Cancel status details.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesCancelStatus {
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Status
    #[serde(default)]
    pub status: Option<String>,
    /// Client order ID
    #[serde(rename = "cliOrdId")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
}

/// Cancel all orders response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesCancelAllData {
    /// Cancel status
    #[serde(rename = "cancelStatus")]
    #[serde(default)]
    pub cancel_status: Option<FuturesCancelAllStatus>,
}

/// Cancel all status details.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesCancelAllStatus {
    /// Cancelled order IDs
    #[serde(rename = "cancelledOrders")]
    #[serde(default)]
    pub cancelled_orders: Option<Vec<FuturesCancelledOrder>>,
    /// Cancel count
    #[serde(rename = "cancelCount")]
    #[serde(default)]
    pub cancel_count: Option<i32>,
}

/// Cancelled order info.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesCancelledOrder {
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cliOrdId")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
}

// =============================================================================
// OPEN ORDERS/POSITIONS TYPES
// =============================================================================

/// Futures order info for queries.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesOrderInfo {
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cliOrdId")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// Symbol
    #[serde(default)]
    pub symbol: Option<String>,
    /// Side
    #[serde(default)]
    pub side: Option<String>,
    /// Order type
    #[serde(rename = "orderType")]
    #[serde(default)]
    pub order_type: Option<String>,
    /// Limit price
    #[serde(rename = "limitPrice")]
    #[serde(default)]
    pub limit_price: Option<Decimal>,
    /// Stop price
    #[serde(rename = "stopPrice")]
    #[serde(default)]
    pub stop_price: Option<Decimal>,
    /// Quantity
    #[serde(default)]
    pub quantity: Option<Decimal>,
    /// Filled quantity
    #[serde(default)]
    pub filled: Option<Decimal>,
    /// Unfilled quantity
    #[serde(rename = "unfilledSize")]
    #[serde(default)]
    pub unfilled_size: Option<Decimal>,
    /// Reduce only flag
    #[serde(rename = "reduceOnly")]
    #[serde(default)]
    pub reduce_only: Option<bool>,
    /// Last update time
    #[serde(rename = "lastUpdateTime")]
    #[serde(default)]
    pub last_update_time: Option<String>,
}

impl FuturesOrderInfo {
    /// Parse the order side.
    pub fn parsed_side(&self) -> KrakenOrderSide {
        match self.side.as_deref() {
            Some("buy") => KrakenOrderSide::Buy,
            Some("sell") => KrakenOrderSide::Sell,
            _ => KrakenOrderSide::Buy,
        }
    }

    /// Parse the order type.
    pub fn parsed_order_type(&self) -> KrakenOrderType {
        match self.order_type.as_deref() {
            Some("lmt") => KrakenOrderType::Limit,
            Some("mkt") => KrakenOrderType::Market,
            Some("stp") => KrakenOrderType::StopLoss,
            Some("take_profit") => KrakenOrderType::TakeProfit,
            _ => KrakenOrderType::Limit,
        }
    }

    /// Get order status based on filled amount.
    pub fn computed_status(&self) -> KrakenOrderStatus {
        let qty = self.quantity.unwrap_or_default();
        let filled = self.filled.unwrap_or_default();

        if filled >= qty && qty > Decimal::ZERO {
            KrakenOrderStatus::Closed
        } else if filled > Decimal::ZERO {
            KrakenOrderStatus::Open // Partially filled
        } else {
            KrakenOrderStatus::Open
        }
    }
}

/// Open orders response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesOpenOrdersData {
    /// List of open orders
    #[serde(rename = "openOrders")]
    pub open_orders: Vec<FuturesOrderInfo>,
}

/// Open positions response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesOpenPositionsData {
    /// List of open positions
    #[serde(rename = "openPositions")]
    pub open_positions: Vec<FuturesPositionInfo>,
}

/// Position info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesPositionInfo {
    /// Symbol
    pub symbol: String,
    /// Side (long/short)
    pub side: String,
    /// Position size
    pub size: Decimal,
    /// Entry price
    #[serde(rename = "price")]
    #[serde(default)]
    pub entry_price: Option<Decimal>,
    /// Mark price
    #[serde(rename = "markPrice")]
    #[serde(default)]
    pub mark_price: Option<Decimal>,
    /// PnL
    #[serde(default)]
    pub pnl: Option<Decimal>,
    /// Liquidation price
    #[serde(rename = "liquidationThreshold")]
    #[serde(default)]
    pub liquidation_threshold: Option<Decimal>,
}

// =============================================================================
// FILLS TYPES
// =============================================================================

/// Fills history response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesFillsData {
    /// List of fills
    pub fills: Vec<FuturesFillInfo>,
}

/// Fill info.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesFillInfo {
    /// Fill ID
    #[serde(rename = "fill_id")]
    #[serde(default)]
    pub fill_id: Option<String>,
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cliOrdId")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// Symbol
    pub symbol: String,
    /// Side
    pub side: String,
    /// Price
    pub price: Decimal,
    /// Size
    pub size: Decimal,
    /// Fill time
    #[serde(rename = "fillTime")]
    #[serde(default)]
    pub fill_time: Option<String>,
    /// Fill type (maker/taker)
    #[serde(rename = "fillType")]
    #[serde(default)]
    pub fill_type: Option<String>,
    /// Fee paid
    #[serde(rename = "feePaid")]
    #[serde(default)]
    pub fee_paid: Option<Decimal>,
    /// Fee currency
    #[serde(rename = "feeCurrency")]
    #[serde(default)]
    pub fee_currency: Option<String>,
}

// =============================================================================
// LEVERAGE TYPES
// =============================================================================

/// Leverage preferences response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesLeverageData {
    /// Leverage preferences per symbol
    #[serde(rename = "leveragePreferences")]
    #[serde(default)]
    pub leverage_preferences: HashMap<String, FuturesLeveragePreference>,
}

/// Leverage preference for a symbol.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct FuturesLeveragePreference {
    /// Maximum leverage
    #[serde(rename = "maxLeverage")]
    #[serde(default)]
    pub max_leverage: Option<Decimal>,
}

// =============================================================================
// WEBSOCKET TYPES
// =============================================================================

/// WebSocket authentication challenge response.
#[derive(Debug, Clone, Deserialize)]
pub struct FuturesWsChallengeData {
    /// Challenge string
    pub challenge: String,
}

/// WebSocket fill event.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WsFuturesFillEvent {
    /// Feed type
    pub feed: String,
    /// Account
    #[serde(default)]
    pub account: Option<String>,
    /// Fills
    #[serde(default)]
    pub fills: Option<Vec<WsFuturesFill>>,
}

/// WebSocket fill data.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WsFuturesFill {
    /// Instrument
    pub instrument: String,
    /// Time
    pub time: i64,
    /// Price
    pub price: Decimal,
    /// Sequence number
    pub seq: i64,
    /// Is buy side
    pub buy: bool,
    /// Quantity
    pub qty: Decimal,
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cli_ord_id")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// Fill ID
    #[serde(rename = "fill_id")]
    #[serde(default)]
    pub fill_id: Option<String>,
    /// Fill type
    #[serde(rename = "fill_type")]
    #[serde(default)]
    pub fill_type: Option<String>,
    /// Fee paid
    #[serde(rename = "fee_paid")]
    #[serde(default)]
    pub fee_paid: Option<Decimal>,
    /// Fee currency
    #[serde(rename = "fee_currency")]
    #[serde(default)]
    pub fee_currency: Option<String>,
}

/// WebSocket open orders event.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WsFuturesOpenOrdersEvent {
    /// Feed type
    pub feed: String,
    /// Account
    #[serde(default)]
    pub account: Option<String>,
    /// Orders
    #[serde(default)]
    pub orders: Option<Vec<WsFuturesOrder>>,
}

/// WebSocket order data.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct WsFuturesOrder {
    /// Instrument
    pub instrument: String,
    /// Time
    pub time: i64,
    /// Last update time
    #[serde(rename = "last_update_time")]
    #[serde(default)]
    pub last_update_time: Option<i64>,
    /// Quantity
    pub qty: Decimal,
    /// Filled quantity
    #[serde(default)]
    pub filled: Option<Decimal>,
    /// Limit price
    #[serde(rename = "limit_price")]
    #[serde(default)]
    pub limit_price: Option<Decimal>,
    /// Stop price
    #[serde(rename = "stop_price")]
    #[serde(default)]
    pub stop_price: Option<Decimal>,
    /// Order type
    #[serde(rename = "type")]
    #[serde(default)]
    pub order_type: Option<String>,
    /// Order ID
    #[serde(rename = "order_id")]
    #[serde(default)]
    pub order_id: Option<String>,
    /// Client order ID
    #[serde(rename = "cli_ord_id")]
    #[serde(default)]
    pub cli_ord_id: Option<String>,
    /// Direction
    #[serde(default)]
    pub direction: Option<i32>, // 1 = buy, -1 = sell
    /// Reduce only
    #[serde(rename = "reduce_only")]
    #[serde(default)]
    pub reduce_only: Option<bool>,
    /// Is cancelled
    #[serde(rename = "is_cancel")]
    #[serde(default)]
    pub is_cancel: Option<bool>,
    /// Reason
    #[serde(default)]
    pub reason: Option<String>,
}

// =============================================================================
// SYMBOL MAPPING
// =============================================================================

/// Convert internal symbol format to Kraken Futures format.
///
/// Kraken Futures uses specific prefixes:
/// - Perpetuals: PI_ prefix (e.g., PI_XBTUSD)
/// - Fixed maturity: FI_ prefix (e.g., FI_XBTUSD_220325)
pub fn to_futures_symbol(symbol: &str) -> String {
    let symbol = symbol.to_uppercase();

    // Already has Kraken futures prefix
    if symbol.starts_with("PI_") || symbol.starts_with("FI_") || symbol.starts_with("PF_") {
        return symbol;
    }

    // Convert common formats
    let symbol = symbol.replace("USDT", "USD");
    let symbol = symbol.replace("BTC", "XBT");

    // Add perpetual prefix by default
    format!("PI_{}", symbol)
}

/// Convert Kraken Futures symbol to internal format.
pub fn from_futures_symbol(kraken_symbol: &str) -> String {
    let symbol = kraken_symbol.to_uppercase();

    // Remove prefix
    let symbol = symbol
        .strip_prefix("PI_")
        .or_else(|| symbol.strip_prefix("FI_"))
        .or_else(|| symbol.strip_prefix("PF_"))
        .unwrap_or(&symbol);

    // Convert XBT to BTC
    let symbol = symbol.replace("XBT", "BTC");

    // Convert USD to USDT - handle date suffixes like _220325
    let symbol = if symbol.ends_with("USD") && !symbol.ends_with("USDT") {
        format!("{}T", symbol)
    } else if symbol.contains("USD_") && !symbol.contains("USDT_") {
        // Handle futures with date suffix: ETHUSD_220325 -> ETHUSDT_220325
        symbol.replace("USD_", "USDT_")
    } else {
        symbol.to_string()
    };

    symbol
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_futures_symbol() {
        assert_eq!(to_futures_symbol("BTCUSDT"), "PI_XBTUSD");
        assert_eq!(to_futures_symbol("ETHUSDT"), "PI_ETHUSD");
        assert_eq!(to_futures_symbol("PI_XBTUSD"), "PI_XBTUSD");
    }

    #[test]
    fn test_from_futures_symbol() {
        assert_eq!(from_futures_symbol("PI_XBTUSD"), "BTCUSDT");
        assert_eq!(from_futures_symbol("FI_ETHUSD_220325"), "ETHUSDT_220325");
        assert_eq!(from_futures_symbol("XBTUSD"), "BTCUSDT");
    }

    #[test]
    fn test_futures_response_success() {
        let json = r#"{"result": "success", "serverTime": "2024-01-15T10:30:00.000Z"}"#;
        let resp: FuturesResponse<()> = serde_json::from_str(json).unwrap();
        assert!(resp.is_success());
        assert!(resp.error_message().is_none());
    }

    #[test]
    fn test_futures_response_error() {
        let json = r#"{"result": "error", "error": "invalidArgument"}"#;
        let resp: FuturesResponse<()> = serde_json::from_str(json).unwrap();
        assert!(!resp.is_success());
        assert_eq!(resp.error_message(), Some("invalidArgument"));
    }

    #[test]
    fn test_order_info_parsing() {
        let json = r#"{
            "order_id": "12345",
            "cliOrdId": "my-order",
            "symbol": "PI_XBTUSD",
            "side": "buy",
            "orderType": "lmt",
            "limitPrice": "50000",
            "quantity": "1",
            "filled": "0.5"
        }"#;

        let info: FuturesOrderInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.parsed_side(), KrakenOrderSide::Buy);
        assert_eq!(info.parsed_order_type(), KrakenOrderType::Limit);
        assert_eq!(info.computed_status(), KrakenOrderStatus::Open); // Partially filled
    }

    #[test]
    fn test_order_fully_filled() {
        let json = r#"{
            "order_id": "12345",
            "symbol": "PI_XBTUSD",
            "side": "buy",
            "orderType": "lmt",
            "quantity": "1",
            "filled": "1"
        }"#;

        let info: FuturesOrderInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.computed_status(), KrakenOrderStatus::Closed);
    }
}
