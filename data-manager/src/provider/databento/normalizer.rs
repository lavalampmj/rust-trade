//! Databento message normalizer
//!
//! Converts Databento-specific message types to TickData.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::provider::ProviderError;
use crate::schema::NormalizedOHLC;
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
}
