//! Shared types for Binance API responses.
//!
//! These types are shared between spot and futures markets.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Binance order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceOrderSide {
    Buy,
    Sell,
}

impl BinanceOrderSide {
    /// Convert to our internal OrderSide.
    pub fn to_order_side(self) -> crate::orders::OrderSide {
        match self {
            Self::Buy => crate::orders::OrderSide::Buy,
            Self::Sell => crate::orders::OrderSide::Sell,
        }
    }

    /// Convert from our internal OrderSide.
    pub fn from_order_side(side: crate::orders::OrderSide) -> Self {
        match side {
            crate::orders::OrderSide::Buy => Self::Buy,
            crate::orders::OrderSide::Sell => Self::Sell,
        }
    }
}

/// Binance order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceOrderStatus {
    New,
    PartiallyFilled,
    Filled,
    Canceled,
    PendingCancel,
    Rejected,
    Expired,
    ExpiredInMatch,
    // Futures-specific
    NewInsurance,
    NewAdl,
}

impl BinanceOrderStatus {
    /// Convert to our internal OrderStatus.
    pub fn to_order_status(self) -> crate::orders::OrderStatus {
        match self {
            Self::New => crate::orders::OrderStatus::Accepted,
            Self::PartiallyFilled => crate::orders::OrderStatus::PartiallyFilled,
            Self::Filled => crate::orders::OrderStatus::Filled,
            Self::Canceled | Self::PendingCancel => crate::orders::OrderStatus::Canceled,
            Self::Rejected => crate::orders::OrderStatus::Rejected,
            Self::Expired | Self::ExpiredInMatch => crate::orders::OrderStatus::Expired,
            Self::NewInsurance | Self::NewAdl => crate::orders::OrderStatus::Accepted,
        }
    }

    /// Check if the order is open.
    pub fn is_open(self) -> bool {
        matches!(self, Self::New | Self::PartiallyFilled | Self::PendingCancel)
    }
}

/// Binance time in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceTimeInForce {
    /// Good till canceled
    #[serde(rename = "GTC")]
    Gtc,
    /// Immediate or cancel
    #[serde(rename = "IOC")]
    Ioc,
    /// Fill or kill
    #[serde(rename = "FOK")]
    Fok,
    /// Good till crossing (post only for futures)
    #[serde(rename = "GTX")]
    Gtx,
    /// Good till date (futures)
    #[serde(rename = "GTD")]
    Gtd,
}

impl BinanceTimeInForce {
    /// Convert to our internal TimeInForce.
    pub fn to_time_in_force(self) -> crate::orders::TimeInForce {
        match self {
            Self::Gtc | Self::Gtd => crate::orders::TimeInForce::GTC,
            Self::Ioc => crate::orders::TimeInForce::IOC,
            Self::Fok => crate::orders::TimeInForce::FOK,
            Self::Gtx => crate::orders::TimeInForce::GTC, // Post-only mapped to GTC
        }
    }

    /// Convert from our internal TimeInForce.
    pub fn from_time_in_force(tif: crate::orders::TimeInForce) -> Self {
        match tif {
            crate::orders::TimeInForce::GTC => Self::Gtc,
            crate::orders::TimeInForce::IOC => Self::Ioc,
            crate::orders::TimeInForce::FOK => Self::Fok,
            crate::orders::TimeInForce::Day | crate::orders::TimeInForce::GTD => Self::Gtc,
            // Other TIFs not supported by Binance, default to GTC
            _ => Self::Gtc,
        }
    }
}

/// Binance execution type (for WebSocket events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinanceExecutionType {
    New,
    Canceled,
    Replaced,
    Rejected,
    Trade,
    Expired,
    Amendment,
    // Futures-specific
    TradePrevention,
}

/// Response when creating/deleting a listen key.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    /// The listen key for user data stream
    pub listen_key: String,
}

/// Empty response for keepalive/delete operations.
#[derive(Debug, Clone, Deserialize)]
pub struct EmptyResponse {}

/// Binance error response structure.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceApiError {
    /// Error code
    pub code: i32,
    /// Error message
    pub msg: String,
}

/// Position side for futures (hedge mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BinancePositionSide {
    /// Both (one-way mode)
    #[default]
    Both,
    /// Long position (hedge mode)
    Long,
    /// Short position (hedge mode)
    Short,
}

impl BinancePositionSide {
    /// Convert to our internal PositionSide.
    pub fn to_position_side(self) -> crate::orders::PositionSide {
        match self {
            Self::Both => crate::orders::PositionSide::Flat,
            Self::Long => crate::orders::PositionSide::Long,
            Self::Short => crate::orders::PositionSide::Short,
        }
    }
}

impl std::fmt::Display for BinanceOrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::New => write!(f, "NEW"),
            Self::PartiallyFilled => write!(f, "PARTIALLY_FILLED"),
            Self::Filled => write!(f, "FILLED"),
            Self::Canceled => write!(f, "CANCELED"),
            Self::PendingCancel => write!(f, "PENDING_CANCEL"),
            Self::Rejected => write!(f, "REJECTED"),
            Self::Expired => write!(f, "EXPIRED"),
            Self::ExpiredInMatch => write!(f, "EXPIRED_IN_MATCH"),
            Self::NewInsurance => write!(f, "NEW_INSURANCE"),
            Self::NewAdl => write!(f, "NEW_ADL"),
        }
    }
}

/// Margin type for futures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BinanceMarginType {
    /// Cross margin
    #[default]
    #[serde(alias = "cross", alias = "CROSS")]
    Cross,
    /// Isolated margin
    #[serde(alias = "isolated", alias = "ISOLATED")]
    Isolated,
}

/// Convert milliseconds timestamp to DateTime.
pub fn timestamp_to_datetime(ts_ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ts_ms).unwrap_or_else(|| Utc::now())
}

/// Parse decimal from string, with fallback to zero.
pub fn parse_decimal(s: &str) -> Decimal {
    s.parse().unwrap_or(Decimal::ZERO)
}

/// User data stream event types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UserDataEventType {
    /// Account update (balance change)
    #[serde(rename = "outboundAccountPosition")]
    AccountUpdate,
    /// Balance update (deposit/withdrawal)
    #[serde(rename = "balanceUpdate")]
    BalanceUpdate,
    /// Execution report (order/trade)
    #[serde(rename = "executionReport")]
    ExecutionReport,
    /// Listen key expired
    #[serde(rename = "listenKeyExpired")]
    ListenKeyExpired,
    // Futures-specific
    /// Account update (futures)
    #[serde(rename = "ACCOUNT_UPDATE")]
    FuturesAccountUpdate,
    /// Order trade update (futures)
    #[serde(rename = "ORDER_TRADE_UPDATE")]
    OrderTradeUpdate,
    /// Margin call (futures)
    #[serde(rename = "MARGIN_CALL")]
    MarginCall,
    /// Account config update (futures)
    #[serde(rename = "ACCOUNT_CONFIG_UPDATE")]
    AccountConfigUpdate,
}

/// Liquidity side (maker/taker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinanceLiquiditySide {
    Maker,
    Taker,
}

impl BinanceLiquiditySide {
    /// Parse from Binance's "m" field (true = maker).
    pub fn from_is_maker(is_maker: bool) -> Self {
        if is_maker {
            Self::Maker
        } else {
            Self::Taker
        }
    }

    /// Convert to our internal LiquiditySide.
    pub fn to_liquidity_side(self) -> crate::orders::LiquiditySide {
        match self {
            Self::Maker => crate::orders::LiquiditySide::Maker,
            Self::Taker => crate::orders::LiquiditySide::Taker,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_side_conversion() {
        assert_eq!(
            BinanceOrderSide::Buy.to_order_side(),
            crate::orders::OrderSide::Buy
        );
        assert_eq!(
            BinanceOrderSide::from_order_side(crate::orders::OrderSide::Sell),
            BinanceOrderSide::Sell
        );
    }

    #[test]
    fn test_order_status_is_open() {
        assert!(BinanceOrderStatus::New.is_open());
        assert!(BinanceOrderStatus::PartiallyFilled.is_open());
        assert!(!BinanceOrderStatus::Filled.is_open());
        assert!(!BinanceOrderStatus::Canceled.is_open());
    }

    #[test]
    fn test_timestamp_conversion() {
        let ts = 1234567890123i64;
        let dt = timestamp_to_datetime(ts);
        assert_eq!(dt.timestamp_millis(), ts);
    }

    #[test]
    fn test_parse_decimal() {
        assert_eq!(parse_decimal("123.45"), Decimal::new(12345, 2));
        assert_eq!(parse_decimal("invalid"), Decimal::ZERO);
    }

    #[test]
    fn test_liquidity_side() {
        assert_eq!(
            BinanceLiquiditySide::from_is_maker(true),
            BinanceLiquiditySide::Maker
        );
        assert_eq!(
            BinanceLiquiditySide::from_is_maker(false),
            BinanceLiquiditySide::Taker
        );
    }

    #[test]
    fn test_deserialize_order_status() {
        let json = r#""PARTIALLY_FILLED""#;
        let status: BinanceOrderStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, BinanceOrderStatus::PartiallyFilled);
    }

    #[test]
    fn test_deserialize_position_side() {
        let json = r#""LONG""#;
        let side: BinancePositionSide = serde_json::from_str(json).unwrap();
        assert_eq!(side, BinancePositionSide::Long);
    }
}
