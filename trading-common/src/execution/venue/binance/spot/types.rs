//! Binance Spot-specific API types.
//!
//! These types map to the Binance Spot REST API and WebSocket responses.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::execution::venue::binance::common::{
    BinanceExecutionType, BinanceLiquiditySide, BinanceOrderSide, BinanceOrderStatus,
    BinanceTimeInForce, parse_decimal,
};

/// Binance Spot order types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SpotOrderType {
    /// Market order
    Market,
    /// Limit order
    Limit,
    /// Stop loss (market)
    StopLoss,
    /// Stop loss limit
    StopLossLimit,
    /// Take profit (market)
    TakeProfit,
    /// Take profit limit
    TakeProfitLimit,
    /// Limit maker (post-only)
    LimitMaker,
}

impl SpotOrderType {
    /// Convert from our internal OrderType.
    ///
    /// Note: LimitIfTouched maps to TakeProfit/TakeProfitLimit since they behave
    /// similarly (order triggered when price reaches a level).
    pub fn from_order_type(
        order_type: crate::orders::OrderType,
        post_only: bool,
    ) -> Option<Self> {
        match order_type {
            crate::orders::OrderType::Market => Some(Self::Market),
            crate::orders::OrderType::Limit if post_only => Some(Self::LimitMaker),
            crate::orders::OrderType::Limit => Some(Self::Limit),
            crate::orders::OrderType::Stop => Some(Self::StopLoss),
            crate::orders::OrderType::StopLimit => Some(Self::StopLossLimit),
            // LimitIfTouched can map to TakeProfitLimit
            crate::orders::OrderType::LimitIfTouched => Some(Self::TakeProfitLimit),
            _ => None,
        }
    }

    /// Convert to our internal OrderType.
    ///
    /// Note: TakeProfit/TakeProfitLimit map to Stop/StopLimit since they behave
    /// similarly (order triggered when price reaches a level).
    pub fn to_order_type(self) -> crate::orders::OrderType {
        match self {
            Self::Market => crate::orders::OrderType::Market,
            Self::Limit | Self::LimitMaker => crate::orders::OrderType::Limit,
            Self::StopLoss | Self::TakeProfit => crate::orders::OrderType::Stop,
            Self::StopLossLimit | Self::TakeProfitLimit => crate::orders::OrderType::StopLimit,
        }
    }

    /// Check if this order type requires a price.
    pub fn requires_price(self) -> bool {
        matches!(
            self,
            Self::Limit | Self::StopLossLimit | Self::TakeProfitLimit | Self::LimitMaker
        )
    }

    /// Check if this order type requires a stop price.
    pub fn requires_stop_price(self) -> bool {
        matches!(
            self,
            Self::StopLoss | Self::StopLossLimit | Self::TakeProfit | Self::TakeProfitLimit
        )
    }
}

/// Response from submitting a new order.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotNewOrderResponse {
    /// Symbol
    pub symbol: String,
    /// Order ID assigned by Binance
    pub order_id: i64,
    /// Order list ID (for OCO orders)
    pub order_list_id: i64,
    /// Client order ID
    pub client_order_id: String,
    /// Transaction time
    pub transact_time: i64,
    /// Price (for limit orders)
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub price: Option<Decimal>,
    /// Original quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_qty: Decimal,
    /// Executed quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub executed_qty: Decimal,
    /// Cumulative quote quantity
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub cummulative_quote_qty: Option<Decimal>,
    /// Order status
    pub status: BinanceOrderStatus,
    /// Time in force
    pub time_in_force: BinanceTimeInForce,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: SpotOrderType,
    /// Order side
    pub side: BinanceOrderSide,
    /// Self-trade prevention mode
    pub self_trade_prevention_mode: Option<String>,
    /// Working time
    pub working_time: Option<i64>,
    /// Fills (for FULL response type)
    #[serde(default)]
    pub fills: Vec<SpotFill>,
}

/// Fill information in order response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotFill {
    /// Fill price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Fill quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub qty: Decimal,
    /// Commission
    #[serde(deserialize_with = "deserialize_decimal")]
    pub commission: Decimal,
    /// Commission asset
    pub commission_asset: String,
    /// Trade ID
    pub trade_id: i64,
}

/// Response from querying an order.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotOrderQueryResponse {
    /// Symbol
    pub symbol: String,
    /// Order ID
    pub order_id: i64,
    /// Order list ID (for OCO)
    pub order_list_id: i64,
    /// Client order ID
    pub client_order_id: String,
    /// Price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Original quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_qty: Decimal,
    /// Executed quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub executed_qty: Decimal,
    /// Cumulative quote quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cummulative_quote_qty: Decimal,
    /// Status
    pub status: BinanceOrderStatus,
    /// Time in force
    pub time_in_force: BinanceTimeInForce,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: SpotOrderType,
    /// Side
    pub side: BinanceOrderSide,
    /// Stop price
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub stop_price: Option<Decimal>,
    /// Iceberg quantity
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub iceberg_qty: Option<Decimal>,
    /// Order creation time
    pub time: i64,
    /// Order update time
    pub update_time: i64,
    /// Is working
    pub is_working: bool,
    /// Working time
    pub working_time: i64,
    /// Original quote order quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_quote_order_qty: Decimal,
    /// Self-trade prevention mode
    pub self_trade_prevention_mode: Option<String>,
}

/// Response from cancelling an order.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotCancelOrderResponse {
    /// Symbol
    pub symbol: String,
    /// Original client order ID
    pub orig_client_order_id: String,
    /// Order ID
    pub order_id: i64,
    /// Order list ID
    pub order_list_id: i64,
    /// Client order ID
    pub client_order_id: String,
    /// Transaction time
    pub transact_time: i64,
    /// Price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Original quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_qty: Decimal,
    /// Executed quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub executed_qty: Decimal,
    /// Cumulative quote quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cummulative_quote_qty: Decimal,
    /// Status
    pub status: BinanceOrderStatus,
    /// Time in force
    pub time_in_force: BinanceTimeInForce,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: SpotOrderType,
    /// Side
    pub side: BinanceOrderSide,
    /// Self-trade prevention mode
    pub self_trade_prevention_mode: Option<String>,
}

/// Account information response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotAccountInfo {
    /// Maker commission (basis points)
    pub maker_commission: i32,
    /// Taker commission (basis points)
    pub taker_commission: i32,
    /// Buyer commission (basis points)
    pub buyer_commission: i32,
    /// Seller commission (basis points)
    pub seller_commission: i32,
    /// Commission rates
    pub commission_rates: Option<SpotCommissionRates>,
    /// Can trade
    pub can_trade: bool,
    /// Can withdraw
    pub can_withdraw: bool,
    /// Can deposit
    pub can_deposit: bool,
    /// Brokered
    pub brokered: Option<bool>,
    /// Require self-trade prevention
    pub require_self_trade_prevention: Option<bool>,
    /// Prevent SOR
    pub prevent_sor: Option<bool>,
    /// Update time
    pub update_time: i64,
    /// Account type
    pub account_type: String,
    /// Balances
    pub balances: Vec<SpotBalance>,
    /// Permissions
    pub permissions: Vec<String>,
    /// UID
    pub uid: Option<i64>,
}

/// Commission rates.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotCommissionRates {
    /// Maker rate
    #[serde(deserialize_with = "deserialize_decimal")]
    pub maker: Decimal,
    /// Taker rate
    #[serde(deserialize_with = "deserialize_decimal")]
    pub taker: Decimal,
    /// Buyer rate
    #[serde(deserialize_with = "deserialize_decimal")]
    pub buyer: Decimal,
    /// Seller rate
    #[serde(deserialize_with = "deserialize_decimal")]
    pub seller: Decimal,
}

/// Balance information.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotBalance {
    /// Asset symbol
    pub asset: String,
    /// Free balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub free: Decimal,
    /// Locked balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub locked: Decimal,
}

/// Execution report from User Data Stream.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotExecutionReport {
    /// Event type (always "executionReport")
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Symbol
    #[serde(rename = "s")]
    pub symbol: String,
    /// Client order ID
    #[serde(rename = "c")]
    pub client_order_id: String,
    /// Side
    #[serde(rename = "S")]
    pub side: BinanceOrderSide,
    /// Order type
    #[serde(rename = "o")]
    pub order_type: SpotOrderType,
    /// Time in force
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Quantity
    #[serde(rename = "q", deserialize_with = "deserialize_decimal")]
    pub quantity: Decimal,
    /// Price
    #[serde(rename = "p", deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Stop price
    #[serde(rename = "P", deserialize_with = "deserialize_decimal")]
    pub stop_price: Decimal,
    /// Trailing delta
    #[serde(rename = "d", default)]
    pub trailing_delta: Option<i64>,
    /// Iceberg quantity
    #[serde(rename = "F", deserialize_with = "deserialize_decimal")]
    pub iceberg_quantity: Decimal,
    /// Order list ID
    #[serde(rename = "g")]
    pub order_list_id: i64,
    /// Original client order ID (for cancel/replace)
    #[serde(rename = "C")]
    pub original_client_order_id: String,
    /// Execution type
    #[serde(rename = "x")]
    pub execution_type: BinanceExecutionType,
    /// Order status
    #[serde(rename = "X")]
    pub order_status: BinanceOrderStatus,
    /// Rejection reason
    #[serde(rename = "r")]
    pub reject_reason: String,
    /// Order ID
    #[serde(rename = "i")]
    pub order_id: i64,
    /// Last executed quantity
    #[serde(rename = "l", deserialize_with = "deserialize_decimal")]
    pub last_qty: Decimal,
    /// Cumulative filled quantity
    #[serde(rename = "z", deserialize_with = "deserialize_decimal")]
    pub cum_qty: Decimal,
    /// Last executed price
    #[serde(rename = "L", deserialize_with = "deserialize_decimal")]
    pub last_price: Decimal,
    /// Commission amount
    #[serde(rename = "n", default, deserialize_with = "deserialize_decimal_opt")]
    pub commission: Option<Decimal>,
    /// Commission asset
    #[serde(rename = "N")]
    pub commission_asset: Option<String>,
    /// Transaction time
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Trade ID
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// Ignore
    #[serde(rename = "I")]
    pub ignore_field: i64,
    /// Is the order in the book?
    #[serde(rename = "w")]
    pub is_in_book: bool,
    /// Is this a maker order?
    #[serde(rename = "m")]
    pub is_maker: bool,
    /// Ignore
    #[serde(rename = "M")]
    pub ignore_field_2: bool,
    /// Order creation time
    #[serde(rename = "O")]
    pub order_creation_time: i64,
    /// Cumulative quote quantity
    #[serde(rename = "Z", deserialize_with = "deserialize_decimal")]
    pub cum_quote_qty: Decimal,
    /// Last quote quantity
    #[serde(rename = "Y", deserialize_with = "deserialize_decimal")]
    pub last_quote_qty: Decimal,
    /// Quote order quantity
    #[serde(rename = "Q", deserialize_with = "deserialize_decimal")]
    pub quote_order_qty: Decimal,
    /// Working time
    #[serde(rename = "W", default)]
    pub working_time: Option<i64>,
    /// Self-trade prevention mode
    #[serde(rename = "V", default)]
    pub stp_mode: Option<String>,
}

impl SpotExecutionReport {
    /// Get liquidity side (maker/taker).
    pub fn liquidity_side(&self) -> BinanceLiquiditySide {
        BinanceLiquiditySide::from_is_maker(self.is_maker)
    }
}

// Custom deserializer for decimal strings
fn deserialize_decimal<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    parse_decimal(&s)
        .try_into()
        .map_err(|_| serde::de::Error::custom(format!("Invalid decimal: {}", s)))
}

fn deserialize_decimal_opt<'de, D>(deserializer: D) -> Result<Option<Decimal>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Deserialize::deserialize(deserializer)?;
    match s {
        Some(s) if !s.is_empty() && s != "0" && s != "0.00000000" => {
            let d = parse_decimal(&s);
            if d.is_zero() {
                Ok(None)
            } else {
                Ok(Some(d))
            }
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spot_order_type_conversion() {
        assert_eq!(
            SpotOrderType::from_order_type(crate::orders::OrderType::Market, false),
            Some(SpotOrderType::Market)
        );
        assert_eq!(
            SpotOrderType::from_order_type(crate::orders::OrderType::Limit, true),
            Some(SpotOrderType::LimitMaker)
        );
    }

    #[test]
    fn test_spot_order_type_requires() {
        assert!(SpotOrderType::Limit.requires_price());
        assert!(!SpotOrderType::Market.requires_price());
        assert!(SpotOrderType::StopLoss.requires_stop_price());
        assert!(!SpotOrderType::Limit.requires_stop_price());
    }

    #[test]
    fn test_deserialize_execution_report() {
        let json = r#"{
            "e": "executionReport",
            "E": 1499405658658,
            "s": "ETHBTC",
            "c": "mUvoqJxFIILMdfAW5iGSOW",
            "S": "BUY",
            "o": "LIMIT",
            "f": "GTC",
            "q": "1.00000000",
            "p": "0.10264410",
            "P": "0.00000000",
            "F": "0.00000000",
            "g": -1,
            "C": "",
            "x": "NEW",
            "X": "NEW",
            "r": "NONE",
            "i": 4293153,
            "l": "0.00000000",
            "z": "0.00000000",
            "L": "0.00000000",
            "n": "0",
            "N": null,
            "T": 1499405658657,
            "t": -1,
            "I": 8641984,
            "w": true,
            "m": false,
            "M": false,
            "O": 1499405658657,
            "Z": "0.00000000",
            "Y": "0.00000000",
            "Q": "0.00000000"
        }"#;

        let report: SpotExecutionReport = serde_json::from_str(json).unwrap();
        assert_eq!(report.symbol, "ETHBTC");
        assert_eq!(report.side, BinanceOrderSide::Buy);
        assert_eq!(report.order_type, SpotOrderType::Limit);
        assert_eq!(report.order_status, BinanceOrderStatus::New);
    }

    #[test]
    fn test_deserialize_account_info() {
        let json = r#"{
            "makerCommission": 15,
            "takerCommission": 15,
            "buyerCommission": 0,
            "sellerCommission": 0,
            "canTrade": true,
            "canWithdraw": true,
            "canDeposit": true,
            "updateTime": 123456789,
            "accountType": "SPOT",
            "balances": [
                {"asset": "BTC", "free": "1.00000000", "locked": "0.50000000"},
                {"asset": "USDT", "free": "1000.00000000", "locked": "0.00000000"}
            ],
            "permissions": ["SPOT"]
        }"#;

        let info: SpotAccountInfo = serde_json::from_str(json).unwrap();
        assert!(info.can_trade);
        assert_eq!(info.balances.len(), 2);
        assert_eq!(info.balances[0].asset, "BTC");
    }
}
