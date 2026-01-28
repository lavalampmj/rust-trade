//! Conversion utilities between data-manager types and trading-common types

use crate::schema::{NormalizedOHLC, NormalizedTick, TradeSide};
use trading_common::data::types::{OHLCData, TickData, Timeframe, TradeSide as CommonTradeSide};

/// Parse timeframe string to Timeframe enum
pub fn parse_timeframe(s: &str) -> Option<Timeframe> {
    match s.to_lowercase().as_str() {
        "1m" | "1min" => Some(Timeframe::OneMinute),
        "5m" | "5min" => Some(Timeframe::FiveMinutes),
        "15m" | "15min" => Some(Timeframe::FifteenMinutes),
        "30m" | "30min" => Some(Timeframe::ThirtyMinutes),
        "1h" | "1hour" | "60m" => Some(Timeframe::OneHour),
        "4h" | "4hour" | "240m" => Some(Timeframe::FourHours),
        "1d" | "1day" | "d" => Some(Timeframe::OneDay),
        "1w" | "1week" | "w" => Some(Timeframe::OneWeek),
        _ => None,
    }
}

impl From<TradeSide> for CommonTradeSide {
    fn from(side: TradeSide) -> Self {
        match side {
            TradeSide::Buy => CommonTradeSide::Buy,
            TradeSide::Sell => CommonTradeSide::Sell,
            TradeSide::Unknown => CommonTradeSide::Unknown,
        }
    }
}

impl From<CommonTradeSide> for TradeSide {
    fn from(side: CommonTradeSide) -> Self {
        match side {
            CommonTradeSide::Buy => TradeSide::Buy,
            CommonTradeSide::Sell => TradeSide::Sell,
            CommonTradeSide::Unknown => TradeSide::Unknown,
        }
    }
}

impl From<NormalizedTick> for TickData {
    fn from(tick: NormalizedTick) -> Self {
        TickData::new(
            tick.ts_event,
            tick.symbol,
            tick.price,
            tick.size,
            tick.side.into(),
            tick.provider_trade_id.unwrap_or_default(),
            tick.is_buyer_maker.unwrap_or(false),
        )
    }
}

impl From<TickData> for NormalizedTick {
    fn from(tick: TickData) -> Self {
        NormalizedTick::with_details(
            tick.timestamp,
            tick.timestamp, // Use same timestamp for ts_recv when converting from TickData
            tick.symbol,
            String::new(), // No exchange info in TickData
            tick.price,
            tick.quantity,
            tick.side.into(),
            "legacy".to_string(),
            Some(tick.trade_id),
            Some(tick.is_buyer_maker),
            0, // Sequence will need to be assigned
        )
    }
}

impl From<NormalizedOHLC> for OHLCData {
    fn from(ohlc: NormalizedOHLC) -> Self {
        let timeframe = parse_timeframe(&ohlc.timeframe).unwrap_or(Timeframe::OneMinute);
        OHLCData::new(
            ohlc.timestamp,
            ohlc.symbol,
            timeframe,
            ohlc.open,
            ohlc.high,
            ohlc.low,
            ohlc.close,
            ohlc.volume,
            ohlc.trade_count,
        )
    }
}

impl From<OHLCData> for NormalizedOHLC {
    fn from(ohlc: OHLCData) -> Self {
        NormalizedOHLC::new(
            ohlc.timestamp,
            ohlc.symbol,
            String::new(), // No exchange info in OHLCData
            ohlc.timeframe.as_str().to_string(),
            ohlc.open,
            ohlc.high,
            ohlc.low,
            ohlc.close,
            ohlc.volume,
            ohlc.trade_count,
            "legacy".to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;

    #[test]
    fn test_trade_side_conversion() {
        let buy = TradeSide::Buy;
        let common: CommonTradeSide = buy.into();
        assert_eq!(common, CommonTradeSide::Buy);

        let sell = CommonTradeSide::Sell;
        let normalized: TradeSide = sell.into();
        assert_eq!(normalized, TradeSide::Sell);
    }

    #[test]
    fn test_tick_conversion() {
        let tick = NormalizedTick::new(
            Utc::now(),
            "BTCUSDT".to_string(),
            "BINANCE".to_string(),
            dec!(50000),
            dec!(1.5),
            TradeSide::Buy,
            "databento".to_string(),
            1,
        );

        let tick_data: TickData = tick.clone().into();
        assert_eq!(tick_data.symbol, "BTCUSDT");
        assert_eq!(tick_data.price, dec!(50000));
        assert_eq!(tick_data.side, CommonTradeSide::Buy);
    }

    #[test]
    fn test_timeframe_parsing() {
        assert_eq!(parse_timeframe("1m"), Some(Timeframe::OneMinute));
        assert_eq!(parse_timeframe("5min"), Some(Timeframe::FiveMinutes));
        assert_eq!(parse_timeframe("1h"), Some(Timeframe::OneHour));
        assert_eq!(parse_timeframe("1d"), Some(Timeframe::OneDay));
        assert_eq!(parse_timeframe("invalid"), None);
    }
}
