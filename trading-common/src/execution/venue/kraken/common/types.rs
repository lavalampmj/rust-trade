//! Shared types for Kraken API responses.
//!
//! These types are shared between spot and futures markets.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Kraken order side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KrakenOrderSide {
    Buy,
    Sell,
}

impl KrakenOrderSide {
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

    /// Get the Kraken API string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        }
    }
}

/// Kraken order status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KrakenOrderStatus {
    /// Order is pending
    Pending,
    /// Order is open
    Open,
    /// Order is closed (filled)
    Closed,
    /// Order is canceled
    Canceled,
    /// Order is expired
    Expired,
}

impl KrakenOrderStatus {
    /// Convert to our internal OrderStatus.
    pub fn to_order_status(self) -> crate::orders::OrderStatus {
        match self {
            Self::Pending => crate::orders::OrderStatus::Submitted,
            Self::Open => crate::orders::OrderStatus::Accepted,
            Self::Closed => crate::orders::OrderStatus::Filled,
            Self::Canceled => crate::orders::OrderStatus::Canceled,
            Self::Expired => crate::orders::OrderStatus::Expired,
        }
    }

    /// Check if the order is open.
    pub fn is_open(self) -> bool {
        matches!(self, Self::Pending | Self::Open)
    }
}

impl std::fmt::Display for KrakenOrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Open => write!(f, "open"),
            Self::Closed => write!(f, "closed"),
            Self::Canceled => write!(f, "canceled"),
            Self::Expired => write!(f, "expired"),
        }
    }
}

/// Kraken order type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KrakenOrderType {
    /// Market order
    Market,
    /// Limit order
    Limit,
    /// Stop loss order (market)
    StopLoss,
    /// Stop loss limit order
    StopLossLimit,
    /// Take profit order (market)
    TakeProfit,
    /// Take profit limit order
    TakeProfitLimit,
    /// Trailing stop order
    TrailingStop,
    /// Trailing stop limit order
    TrailingStopLimit,
    /// Settle position
    SettlePosition,
}

impl KrakenOrderType {
    /// Convert from our internal OrderType.
    pub fn from_order_type(
        order_type: crate::orders::OrderType,
        _is_post_only: bool,
    ) -> Option<Self> {
        match order_type {
            crate::orders::OrderType::Market => Some(Self::Market),
            crate::orders::OrderType::Limit => Some(Self::Limit),
            crate::orders::OrderType::Stop => Some(Self::StopLoss),
            crate::orders::OrderType::StopLimit => Some(Self::StopLossLimit),
            crate::orders::OrderType::LimitIfTouched => Some(Self::TakeProfit),
            crate::orders::OrderType::TrailingStop => Some(Self::TrailingStop),
            // MarketToLimit not supported by Kraken
            crate::orders::OrderType::MarketToLimit => None,
        }
    }

    /// Convert to our internal OrderType.
    pub fn to_order_type(self) -> crate::orders::OrderType {
        match self {
            Self::Market => crate::orders::OrderType::Market,
            Self::Limit => crate::orders::OrderType::Limit,
            Self::StopLoss => crate::orders::OrderType::Stop,
            Self::StopLossLimit => crate::orders::OrderType::StopLimit,
            Self::TakeProfit => crate::orders::OrderType::LimitIfTouched,
            Self::TakeProfitLimit => crate::orders::OrderType::LimitIfTouched,
            Self::TrailingStop | Self::TrailingStopLimit => crate::orders::OrderType::TrailingStop,
            Self::SettlePosition => crate::orders::OrderType::Market,
        }
    }

    /// Get the Kraken API string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Market => "market",
            Self::Limit => "limit",
            Self::StopLoss => "stop-loss",
            Self::StopLossLimit => "stop-loss-limit",
            Self::TakeProfit => "take-profit",
            Self::TakeProfitLimit => "take-profit-limit",
            Self::TrailingStop => "trailing-stop",
            Self::TrailingStopLimit => "trailing-stop-limit",
            Self::SettlePosition => "settle-position",
        }
    }

    /// Check if this order type requires a price.
    pub fn requires_price(&self) -> bool {
        matches!(
            self,
            Self::Limit
                | Self::StopLossLimit
                | Self::TakeProfitLimit
                | Self::TrailingStopLimit
        )
    }

    /// Check if this order type requires a trigger/stop price.
    pub fn requires_trigger_price(&self) -> bool {
        matches!(
            self,
            Self::StopLoss
                | Self::StopLossLimit
                | Self::TakeProfit
                | Self::TakeProfitLimit
                | Self::TrailingStop
                | Self::TrailingStopLimit
        )
    }
}

/// Kraken time in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KrakenTimeInForce {
    /// Good till canceled
    #[serde(rename = "GTC")]
    Gtc,
    /// Immediate or cancel
    #[serde(rename = "IOC")]
    Ioc,
    /// Good till date
    #[serde(rename = "GTD")]
    Gtd,
}

impl KrakenTimeInForce {
    /// Convert to our internal TimeInForce.
    pub fn to_time_in_force(self) -> crate::orders::TimeInForce {
        match self {
            Self::Gtc => crate::orders::TimeInForce::GTC,
            Self::Ioc => crate::orders::TimeInForce::IOC,
            Self::Gtd => crate::orders::TimeInForce::GTD,
        }
    }

    /// Convert from our internal TimeInForce.
    pub fn from_time_in_force(tif: crate::orders::TimeInForce) -> Self {
        match tif {
            crate::orders::TimeInForce::GTC => Self::Gtc,
            crate::orders::TimeInForce::IOC => Self::Ioc,
            crate::orders::TimeInForce::GTD => Self::Gtd,
            // FOK not supported by Kraken, use IOC
            crate::orders::TimeInForce::FOK => Self::Ioc,
            // Day orders map to GTC
            crate::orders::TimeInForce::Day => Self::Gtc,
            // Others default to GTC
            _ => Self::Gtc,
        }
    }

    /// Get the Kraken API string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gtc => "GTC",
            Self::Ioc => "IOC",
            Self::Gtd => "GTD",
        }
    }
}

/// Kraken execution type (for WebSocket V2 events).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KrakenExecutionType {
    /// Order pending
    PendingNew,
    /// Order accepted
    New,
    /// Fill (partial or complete)
    Trade,
    /// Fully filled
    Filled,
    /// Order canceled
    Canceled,
    /// Order expired
    Expired,
    /// Order restated (e.g., after amendment)
    Restated,
}

impl KrakenExecutionType {
    /// Check if this is a fill event.
    pub fn is_fill(self) -> bool {
        matches!(self, Self::Trade | Self::Filled)
    }

    /// Check if this is a terminal event.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Filled | Self::Canceled | Self::Expired)
    }
}

/// Liquidity side (maker/taker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KrakenLiquiditySide {
    Maker,
    Taker,
}

impl KrakenLiquiditySide {
    /// Parse from Kraken's liquidity indicator.
    pub fn from_liquidity_ind(ind: &str) -> Self {
        match ind.to_lowercase().as_str() {
            "m" | "maker" => Self::Maker,
            _ => Self::Taker,
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

/// Kraken API error response.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenApiError {
    /// Error messages
    pub error: Vec<String>,
}

/// Kraken API response wrapper.
#[derive(Debug, Clone, Deserialize)]
pub struct KrakenResponse<T> {
    /// Error messages (empty if successful)
    pub error: Vec<String>,
    /// Result data (present if successful)
    pub result: Option<T>,
}

impl<T> KrakenResponse<T> {
    /// Check if the response contains errors.
    pub fn has_error(&self) -> bool {
        !self.error.is_empty()
    }

    /// Get the first error message if any.
    pub fn first_error(&self) -> Option<&str> {
        self.error.first().map(|s| s.as_str())
    }
}

/// Convert Kraken timestamp (ISO 8601 or Unix seconds) to DateTime.
pub fn timestamp_to_datetime(ts: &str) -> DateTime<Utc> {
    // Try parsing as ISO 8601 first
    if let Ok(dt) = ts.parse::<DateTime<Utc>>() {
        return dt;
    }

    // Try parsing as Unix timestamp (seconds with decimals)
    if let Ok(secs) = ts.parse::<f64>() {
        let secs_i64 = secs as i64;
        let nanos = ((secs - secs_i64 as f64) * 1_000_000_000.0) as u32;
        return DateTime::from_timestamp(secs_i64, nanos).unwrap_or_else(Utc::now);
    }

    // Fallback to current time
    Utc::now()
}

/// Parse decimal from Kraken's string format.
pub fn parse_kraken_decimal(s: &str) -> Decimal {
    s.parse().unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_side_conversion() {
        assert_eq!(
            KrakenOrderSide::Buy.to_order_side(),
            crate::orders::OrderSide::Buy
        );
        assert_eq!(
            KrakenOrderSide::from_order_side(crate::orders::OrderSide::Sell),
            KrakenOrderSide::Sell
        );
    }

    #[test]
    fn test_order_status_is_open() {
        assert!(KrakenOrderStatus::Open.is_open());
        assert!(KrakenOrderStatus::Pending.is_open());
        assert!(!KrakenOrderStatus::Closed.is_open());
        assert!(!KrakenOrderStatus::Canceled.is_open());
    }

    #[test]
    fn test_order_type_requires_price() {
        assert!(KrakenOrderType::Limit.requires_price());
        assert!(KrakenOrderType::StopLossLimit.requires_price());
        assert!(!KrakenOrderType::Market.requires_price());
        assert!(!KrakenOrderType::StopLoss.requires_price());
    }

    #[test]
    fn test_order_type_requires_trigger() {
        assert!(KrakenOrderType::StopLoss.requires_trigger_price());
        assert!(KrakenOrderType::TakeProfit.requires_trigger_price());
        assert!(!KrakenOrderType::Market.requires_trigger_price());
        assert!(!KrakenOrderType::Limit.requires_trigger_price());
    }

    #[test]
    fn test_timestamp_parsing() {
        // Unix timestamp with decimals
        let dt = timestamp_to_datetime("1699564800.123456");
        assert!(dt.timestamp() > 0);

        // ISO 8601
        let dt = timestamp_to_datetime("2024-01-15T10:30:00.000000Z");
        assert!(dt.timestamp() > 0);
    }

    #[test]
    fn test_parse_decimal() {
        assert_eq!(parse_kraken_decimal("123.45"), Decimal::new(12345, 2));
        assert_eq!(parse_kraken_decimal("invalid"), Decimal::ZERO);
    }

    #[test]
    fn test_liquidity_side() {
        assert_eq!(
            KrakenLiquiditySide::from_liquidity_ind("maker"),
            KrakenLiquiditySide::Maker
        );
        assert_eq!(
            KrakenLiquiditySide::from_liquidity_ind("taker"),
            KrakenLiquiditySide::Taker
        );
        assert_eq!(
            KrakenLiquiditySide::from_liquidity_ind("m"),
            KrakenLiquiditySide::Maker
        );
    }

    #[test]
    fn test_execution_type() {
        assert!(KrakenExecutionType::Trade.is_fill());
        assert!(KrakenExecutionType::Filled.is_fill());
        assert!(!KrakenExecutionType::New.is_fill());

        assert!(KrakenExecutionType::Filled.is_terminal());
        assert!(KrakenExecutionType::Canceled.is_terminal());
        assert!(!KrakenExecutionType::New.is_terminal());
    }

    #[test]
    fn test_kraken_response() {
        let response: KrakenResponse<String> = KrakenResponse {
            error: vec![],
            result: Some("data".to_string()),
        };
        assert!(!response.has_error());

        let error_response: KrakenResponse<String> = KrakenResponse {
            error: vec!["EOrder:Rate limit exceeded".to_string()],
            result: None,
        };
        assert!(error_response.has_error());
        assert_eq!(
            error_response.first_error(),
            Some("EOrder:Rate limit exceeded")
        );
    }
}
