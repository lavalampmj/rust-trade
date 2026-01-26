//! Binance USDT-M Futures-specific API types.
//!
//! These types map to the Binance Futures REST API and WebSocket responses.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::execution::venue::binance::common::{
    BinanceExecutionType, BinanceLiquiditySide, BinanceMarginType, BinanceOrderSide,
    BinanceOrderStatus, BinancePositionSide, BinanceTimeInForce, parse_decimal,
};

/// Binance Futures order types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FuturesOrderType {
    /// Limit order
    Limit,
    /// Market order
    Market,
    /// Stop order (market when triggered)
    Stop,
    /// Stop market order
    StopMarket,
    /// Take profit order (market when triggered)
    TakeProfit,
    /// Take profit market
    TakeProfitMarket,
    /// Trailing stop market
    TrailingStopMarket,
}

impl FuturesOrderType {
    /// Convert from our internal OrderType.
    ///
    /// Note: Our OrderType doesn't have TakeProfit variants, so we only
    /// map the basic types. Users can specify TakeProfit via order tags.
    pub fn from_order_type(order_type: crate::orders::OrderType) -> Option<Self> {
        match order_type {
            crate::orders::OrderType::Market => Some(Self::Market),
            crate::orders::OrderType::Limit => Some(Self::Limit),
            crate::orders::OrderType::Stop => Some(Self::StopMarket),
            crate::orders::OrderType::StopLimit => Some(Self::Stop),
            crate::orders::OrderType::TrailingStop => Some(Self::TrailingStopMarket),
            // LimitIfTouched can map to TakeProfit (limit order triggered at price)
            crate::orders::OrderType::LimitIfTouched => Some(Self::TakeProfit),
            _ => None,
        }
    }

    /// Convert to our internal OrderType.
    ///
    /// Note: TakeProfit variants map to StopLimit/Stop since they behave
    /// similarly (order triggered when price reaches a level).
    pub fn to_order_type(self) -> crate::orders::OrderType {
        match self {
            Self::Market | Self::StopMarket | Self::TakeProfitMarket => {
                crate::orders::OrderType::Market
            }
            Self::Limit => crate::orders::OrderType::Limit,
            Self::Stop | Self::TakeProfit => crate::orders::OrderType::StopLimit,
            Self::TrailingStopMarket => crate::orders::OrderType::TrailingStop,
        }
    }

    /// Check if this order type requires a price.
    pub fn requires_price(self) -> bool {
        matches!(self, Self::Limit | Self::Stop | Self::TakeProfit)
    }

    /// Check if this order type requires a stop price.
    pub fn requires_stop_price(self) -> bool {
        matches!(
            self,
            Self::Stop | Self::StopMarket | Self::TakeProfit | Self::TakeProfitMarket
        )
    }
}

/// Response from submitting a new futures order.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesNewOrderResponse {
    /// Client order ID
    pub client_order_id: String,
    /// Cumulative quote quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cum_quote: Decimal,
    /// Executed quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub executed_qty: Decimal,
    /// Order ID
    pub order_id: i64,
    /// Average price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub avg_price: Decimal,
    /// Original quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_qty: Decimal,
    /// Price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Reduce only
    pub reduce_only: bool,
    /// Side
    pub side: BinanceOrderSide,
    /// Position side
    pub position_side: BinancePositionSide,
    /// Status
    pub status: BinanceOrderStatus,
    /// Stop price
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub stop_price: Option<Decimal>,
    /// Close position
    pub close_position: bool,
    /// Symbol
    pub symbol: String,
    /// Time in force
    pub time_in_force: BinanceTimeInForce,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: FuturesOrderType,
    /// Original type
    pub orig_type: Option<FuturesOrderType>,
    /// Activation price (for trailing stop)
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub activate_price: Option<Decimal>,
    /// Price rate (for trailing stop)
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub price_rate: Option<Decimal>,
    /// Update time
    pub update_time: i64,
    /// Working type
    pub working_type: Option<String>,
    /// Price protect
    pub price_protect: Option<bool>,
}

/// Response from querying a futures order.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderQueryResponse {
    /// Average price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub avg_price: Decimal,
    /// Client order ID
    pub client_order_id: String,
    /// Cumulative quote quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cum_quote: Decimal,
    /// Executed quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub executed_qty: Decimal,
    /// Order ID
    pub order_id: i64,
    /// Original quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_qty: Decimal,
    /// Original type
    pub orig_type: Option<FuturesOrderType>,
    /// Price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Reduce only
    pub reduce_only: bool,
    /// Side
    pub side: BinanceOrderSide,
    /// Position side
    pub position_side: BinancePositionSide,
    /// Status
    pub status: BinanceOrderStatus,
    /// Stop price
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub stop_price: Option<Decimal>,
    /// Close position
    pub close_position: bool,
    /// Symbol
    pub symbol: String,
    /// Time
    pub time: i64,
    /// Time in force
    pub time_in_force: BinanceTimeInForce,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: FuturesOrderType,
    /// Activation price
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub activate_price: Option<Decimal>,
    /// Price rate
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub price_rate: Option<Decimal>,
    /// Update time
    pub update_time: i64,
    /// Working type
    pub working_type: Option<String>,
    /// Price protect
    pub price_protect: Option<bool>,
}

/// Response from cancelling a futures order.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesCancelOrderResponse {
    /// Client order ID
    pub client_order_id: String,
    /// Cumulative quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cum_qty: Decimal,
    /// Cumulative quote
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cum_quote: Decimal,
    /// Executed quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub executed_qty: Decimal,
    /// Order ID
    pub order_id: i64,
    /// Original quantity
    #[serde(deserialize_with = "deserialize_decimal")]
    pub orig_qty: Decimal,
    /// Original type
    pub orig_type: Option<FuturesOrderType>,
    /// Price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Reduce only
    pub reduce_only: bool,
    /// Side
    pub side: BinanceOrderSide,
    /// Position side
    pub position_side: BinancePositionSide,
    /// Status
    pub status: BinanceOrderStatus,
    /// Stop price
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub stop_price: Option<Decimal>,
    /// Close position
    pub close_position: bool,
    /// Symbol
    pub symbol: String,
    /// Time in force
    pub time_in_force: BinanceTimeInForce,
    /// Order type
    #[serde(rename = "type")]
    pub order_type: FuturesOrderType,
    /// Activation price
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub activate_price: Option<Decimal>,
    /// Price rate
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub price_rate: Option<Decimal>,
    /// Update time
    pub update_time: i64,
    /// Working type
    pub working_type: Option<String>,
    /// Price protect
    pub price_protect: Option<bool>,
}

/// Futures account information (v2).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAccountInfo {
    /// Fee tier
    pub fee_tier: i32,
    /// Can trade
    pub can_trade: bool,
    /// Can deposit
    pub can_deposit: bool,
    /// Can withdraw
    pub can_withdraw: bool,
    /// Update time
    pub update_time: i64,
    /// Multi-assets margin
    pub multi_assets_margin: Option<bool>,
    /// Total initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_initial_margin: Decimal,
    /// Total maintenance margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_maint_margin: Decimal,
    /// Total wallet balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_wallet_balance: Decimal,
    /// Total unrealized profit
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_unrealized_profit: Decimal,
    /// Total margin balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_margin_balance: Decimal,
    /// Total position initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_position_initial_margin: Decimal,
    /// Total open order initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_open_order_initial_margin: Decimal,
    /// Total cross wallet balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_cross_wallet_balance: Decimal,
    /// Total cross unrealized PNL
    #[serde(deserialize_with = "deserialize_decimal")]
    pub total_cross_un_pnl: Decimal,
    /// Available balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub available_balance: Decimal,
    /// Max withdraw amount
    #[serde(deserialize_with = "deserialize_decimal")]
    pub max_withdraw_amount: Decimal,
    /// Assets
    pub assets: Vec<FuturesAsset>,
    /// Positions
    pub positions: Vec<FuturesPosition>,
}

/// Futures asset balance.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesAsset {
    /// Asset
    pub asset: String,
    /// Wallet balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub wallet_balance: Decimal,
    /// Unrealized profit
    #[serde(deserialize_with = "deserialize_decimal")]
    pub unrealized_profit: Decimal,
    /// Margin balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub margin_balance: Decimal,
    /// Maintenance margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub maint_margin: Decimal,
    /// Initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub initial_margin: Decimal,
    /// Position initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub position_initial_margin: Decimal,
    /// Open order initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub open_order_initial_margin: Decimal,
    /// Cross wallet balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cross_wallet_balance: Decimal,
    /// Cross unrealized PNL
    #[serde(deserialize_with = "deserialize_decimal")]
    pub cross_un_pnl: Decimal,
    /// Available balance
    #[serde(deserialize_with = "deserialize_decimal")]
    pub available_balance: Decimal,
    /// Max withdraw amount
    #[serde(deserialize_with = "deserialize_decimal")]
    pub max_withdraw_amount: Decimal,
    /// Margin available
    pub margin_available: bool,
    /// Update time
    pub update_time: i64,
}

/// Futures position from account info.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPosition {
    /// Symbol
    pub symbol: String,
    /// Initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub initial_margin: Decimal,
    /// Maintenance margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub maint_margin: Decimal,
    /// Unrealized profit
    #[serde(deserialize_with = "deserialize_decimal")]
    pub unrealized_profit: Decimal,
    /// Position initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub position_initial_margin: Decimal,
    /// Open order initial margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub open_order_initial_margin: Decimal,
    /// Leverage
    #[serde(deserialize_with = "deserialize_decimal")]
    pub leverage: Decimal,
    /// Isolated
    pub isolated: bool,
    /// Entry price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub entry_price: Decimal,
    /// Max notional
    #[serde(deserialize_with = "deserialize_decimal")]
    pub max_notional: Decimal,
    /// Bid notional (futures)
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub bid_notional: Option<Decimal>,
    /// Ask notional (futures)
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub ask_notional: Option<Decimal>,
    /// Position side
    pub position_side: BinancePositionSide,
    /// Position amount
    #[serde(deserialize_with = "deserialize_decimal")]
    pub position_amt: Decimal,
    /// Update time
    pub update_time: i64,
}

/// Position risk response (more detailed than account positions).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPositionRisk {
    /// Entry price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub entry_price: Decimal,
    /// Margin type
    pub margin_type: BinanceMarginType,
    /// Is auto add margin
    #[serde(rename = "isAutoAddMargin")]
    pub is_auto_add_margin: String,
    /// Isolated margin
    #[serde(deserialize_with = "deserialize_decimal")]
    pub isolated_margin: Decimal,
    /// Leverage
    #[serde(deserialize_with = "deserialize_decimal")]
    pub leverage: Decimal,
    /// Liquidation price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub liquidation_price: Decimal,
    /// Mark price
    #[serde(deserialize_with = "deserialize_decimal")]
    pub mark_price: Decimal,
    /// Max notional value
    #[serde(deserialize_with = "deserialize_decimal")]
    pub max_notional_value: Decimal,
    /// Position amount
    #[serde(deserialize_with = "deserialize_decimal")]
    pub position_amt: Decimal,
    /// Notional
    #[serde(deserialize_with = "deserialize_decimal")]
    pub notional: Decimal,
    /// Isolated wallet
    #[serde(default, deserialize_with = "deserialize_decimal_opt")]
    pub isolated_wallet: Option<Decimal>,
    /// Symbol
    pub symbol: String,
    /// Unrealized profit
    #[serde(rename = "unRealizedProfit", deserialize_with = "deserialize_decimal")]
    pub unrealized_profit: Decimal,
    /// Position side
    pub position_side: BinancePositionSide,
    /// Update time
    pub update_time: i64,
}

/// Leverage change response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesLeverageResponse {
    /// Leverage
    pub leverage: u32,
    /// Max notional value
    #[serde(deserialize_with = "deserialize_decimal")]
    pub max_notional_value: Decimal,
    /// Symbol
    pub symbol: String,
}

/// Margin type change response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesMarginTypeResponse {
    /// Status code
    pub code: i32,
    /// Message
    pub msg: String,
}

/// Position mode response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesPositionModeResponse {
    /// Dual side position (hedge mode)
    pub dual_side_position: bool,
}

/// Order trade update from User Data Stream.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderTradeUpdate {
    /// Event type
    #[serde(rename = "e")]
    pub event_type: String,
    /// Event time
    #[serde(rename = "E")]
    pub event_time: i64,
    /// Transaction time
    #[serde(rename = "T")]
    pub transaction_time: i64,
    /// Order info
    #[serde(rename = "o")]
    pub order: FuturesOrderInfo,
}

/// Order info in trade update.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FuturesOrderInfo {
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
    pub order_type: FuturesOrderType,
    /// Time in force
    #[serde(rename = "f")]
    pub time_in_force: BinanceTimeInForce,
    /// Original quantity
    #[serde(rename = "q", deserialize_with = "deserialize_decimal")]
    pub quantity: Decimal,
    /// Price
    #[serde(rename = "p", deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    /// Average price
    #[serde(rename = "ap", deserialize_with = "deserialize_decimal")]
    pub avg_price: Decimal,
    /// Stop price
    #[serde(rename = "sp", deserialize_with = "deserialize_decimal")]
    pub stop_price: Decimal,
    /// Execution type
    #[serde(rename = "x")]
    pub execution_type: BinanceExecutionType,
    /// Order status
    #[serde(rename = "X")]
    pub order_status: BinanceOrderStatus,
    /// Order ID
    #[serde(rename = "i")]
    pub order_id: i64,
    /// Last filled quantity
    #[serde(rename = "l", deserialize_with = "deserialize_decimal")]
    pub last_qty: Decimal,
    /// Cumulative filled quantity
    #[serde(rename = "z", deserialize_with = "deserialize_decimal")]
    pub cum_qty: Decimal,
    /// Last filled price
    #[serde(rename = "L", deserialize_with = "deserialize_decimal")]
    pub last_price: Decimal,
    /// Commission
    #[serde(rename = "n", default, deserialize_with = "deserialize_decimal_opt")]
    pub commission: Option<Decimal>,
    /// Commission asset
    #[serde(rename = "N")]
    pub commission_asset: Option<String>,
    /// Order trade time
    #[serde(rename = "T")]
    pub order_trade_time: i64,
    /// Trade ID
    #[serde(rename = "t")]
    pub trade_id: i64,
    /// Bids notional
    #[serde(rename = "b", deserialize_with = "deserialize_decimal")]
    pub bids_notional: Decimal,
    /// Asks notional
    #[serde(rename = "a", deserialize_with = "deserialize_decimal")]
    pub asks_notional: Decimal,
    /// Is maker
    #[serde(rename = "m")]
    pub is_maker: bool,
    /// Reduce only
    #[serde(rename = "R")]
    pub reduce_only: bool,
    /// Working type
    #[serde(rename = "wt")]
    pub working_type: String,
    /// Original order type
    #[serde(rename = "ot")]
    pub orig_type: FuturesOrderType,
    /// Position side
    #[serde(rename = "ps")]
    pub position_side: BinancePositionSide,
    /// Close all (if close position)
    #[serde(rename = "cp")]
    pub close_position: bool,
    /// Activation price
    #[serde(rename = "AP", default, deserialize_with = "deserialize_decimal_opt")]
    pub activation_price: Option<Decimal>,
    /// Callback rate
    #[serde(rename = "cr", default, deserialize_with = "deserialize_decimal_opt")]
    pub callback_rate: Option<Decimal>,
    /// Price protect
    #[serde(rename = "pP")]
    pub price_protect: bool,
    /// Realized profit
    #[serde(rename = "rp", deserialize_with = "deserialize_decimal")]
    pub realized_profit: Decimal,
    /// STP mode
    #[serde(rename = "V")]
    pub stp_mode: Option<String>,
    /// Price match
    #[serde(rename = "pm")]
    pub price_match: Option<String>,
    /// GTD time
    #[serde(rename = "gtd", default)]
    pub gtd_time: Option<i64>,
}

impl FuturesOrderInfo {
    /// Get liquidity side (maker/taker).
    pub fn liquidity_side(&self) -> BinanceLiquiditySide {
        BinanceLiquiditySide::from_is_maker(self.is_maker)
    }
}

// Custom deserializers
fn deserialize_decimal<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    Ok(parse_decimal(&s))
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
    fn test_futures_order_type_conversion() {
        assert_eq!(
            FuturesOrderType::from_order_type(crate::orders::OrderType::Market),
            Some(FuturesOrderType::Market)
        );
        assert_eq!(
            FuturesOrderType::from_order_type(crate::orders::OrderType::Limit),
            Some(FuturesOrderType::Limit)
        );
    }

    #[test]
    fn test_deserialize_position_risk() {
        let json = r#"{
            "entryPrice": "0.00000",
            "marginType": "isolated",
            "isAutoAddMargin": "false",
            "isolatedMargin": "0.00000000",
            "leverage": "10",
            "liquidationPrice": "0",
            "markPrice": "6679.50671178",
            "maxNotionalValue": "20000000",
            "positionAmt": "0.000",
            "notional": "0",
            "isolatedWallet": "0",
            "symbol": "BTCUSDT",
            "unRealizedProfit": "0.00000000",
            "positionSide": "BOTH",
            "updateTime": 0
        }"#;

        let position: FuturesPositionRisk = serde_json::from_str(json).unwrap();
        assert_eq!(position.symbol, "BTCUSDT");
        assert_eq!(position.leverage, Decimal::from(10));
        assert_eq!(position.margin_type, BinanceMarginType::Isolated);
    }

    #[test]
    fn test_deserialize_order_trade_update() {
        let json = r#"{
            "e": "ORDER_TRADE_UPDATE",
            "E": 1568879465651,
            "T": 1568879465650,
            "o": {
                "s": "BTCUSDT",
                "c": "TEST",
                "S": "BUY",
                "o": "LIMIT",
                "f": "GTC",
                "q": "0.001",
                "p": "9910",
                "ap": "0",
                "sp": "0",
                "x": "NEW",
                "X": "NEW",
                "i": 8886774,
                "l": "0",
                "z": "0",
                "L": "0",
                "T": 1568879465651,
                "t": 0,
                "b": "0",
                "a": "9.91",
                "m": false,
                "R": false,
                "wt": "CONTRACT_PRICE",
                "ot": "LIMIT",
                "ps": "BOTH",
                "cp": false,
                "pP": false,
                "rp": "0"
            }
        }"#;

        let update: FuturesOrderTradeUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.order.symbol, "BTCUSDT");
        assert_eq!(update.order.side, BinanceOrderSide::Buy);
        assert_eq!(update.order.order_type, FuturesOrderType::Limit);
    }
}
