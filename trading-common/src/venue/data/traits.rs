//! Data venue trait definitions.
//!
//! This module defines the trait hierarchy for data venues:
//!
//! - [`DataStreamVenue`]: Real-time data streaming
//! - [`HistoricalDataVenue`]: Historical data retrieval
//! - [`FullDataVenue`]: Combined trait for full-featured venues

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

use super::types::{
    DataAvailability, HistoricalRequest, LiveSubscription, StreamCallback, SubscriptionStatus,
};
use crate::data::types::TickData;
use crate::venue::connection::VenueConnection;
use crate::venue::error::VenueResult;
use crate::venue::symbology::SymbolNormalizer;

/// Normalized OHLC data for historical queries.
#[derive(Debug, Clone)]
pub struct NormalizedOHLC {
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub open: rust_decimal::Decimal,
    pub high: rust_decimal::Decimal,
    pub low: rust_decimal::Decimal,
    pub close: rust_decimal::Decimal,
    pub volume: rust_decimal::Decimal,
}

/// Trait for venues that provide real-time data streaming.
///
/// Venues implementing this trait can stream live market data (trades, quotes,
/// order book updates) via WebSocket or similar protocols.
///
/// # Example
///
/// ```ignore
/// use trading_common::venue::data::{DataStreamVenue, LiveSubscription, StreamEvent};
///
/// async fn stream_trades<V: DataStreamVenue>(venue: &mut V) -> VenueResult<()> {
///     let subscription = LiveSubscription::trades(vec!["BTCUSD".into()]);
///     let callback = Arc::new(|event: StreamEvent| {
///         if let StreamEvent::Tick(tick) = event {
///             println!("Trade: {} @ {}", tick.quantity, tick.price);
///         }
///     });
///
///     let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);
///     venue.subscribe(subscription, callback, shutdown_rx).await
/// }
/// ```
#[async_trait]
pub trait DataStreamVenue: VenueConnection + SymbolNormalizer {
    /// Subscribe to live data.
    ///
    /// The callback will be invoked for each stream event.
    /// The shutdown receiver signals when to stop.
    ///
    /// # Arguments
    ///
    /// * `subscription` - What data to subscribe to
    /// * `callback` - Called for each stream event
    /// * `shutdown_rx` - Receiver for shutdown signal
    async fn subscribe(
        &mut self,
        subscription: LiveSubscription,
        callback: StreamCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()>;

    /// Unsubscribe from symbols.
    ///
    /// # Arguments
    ///
    /// * `symbols` - Symbols to unsubscribe from (canonical format)
    async fn unsubscribe(&mut self, symbols: &[String]) -> VenueResult<()>;

    /// Get current subscription status.
    fn subscription_status(&self) -> SubscriptionStatus;
}

/// Trait for venues that provide historical data.
///
/// Venues implementing this trait can retrieve historical market data
/// for backtesting and analysis.
///
/// # Example
///
/// ```ignore
/// use trading_common::venue::data::{HistoricalDataVenue, HistoricalRequest, DataType};
///
/// async fn fetch_history<V: HistoricalDataVenue>(venue: &V) -> VenueResult<()> {
///     let request = HistoricalRequest::trades(
///         vec!["BTCUSD".into()],
///         Utc::now() - Duration::days(7),
///         Utc::now(),
///     );
///
///     let ticks = venue.fetch_ticks(&request).await?;
///     for tick_result in ticks {
///         let tick = tick_result?;
///         println!("{}: {} @ {}", tick.symbol, tick.quantity, tick.price);
///     }
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait HistoricalDataVenue: VenueConnection + SymbolNormalizer {
    /// Fetch tick/trade data.
    ///
    /// Returns a boxed iterator over results to support streaming large datasets.
    async fn fetch_ticks(
        &self,
        request: &HistoricalRequest,
    ) -> VenueResult<Box<dyn Iterator<Item = VenueResult<TickData>> + Send>>;

    /// Fetch OHLC bar data.
    async fn fetch_ohlc(&self, request: &HistoricalRequest) -> VenueResult<Vec<NormalizedOHLC>>;

    /// Check data availability for a symbol.
    ///
    /// # Arguments
    ///
    /// * `symbol` - Symbol to check (canonical format)
    /// * `start` - Start time
    /// * `end` - End time
    async fn check_availability(
        &self,
        symbol: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> VenueResult<DataAvailability>;
}

/// Combined trait for venues that support both streaming and historical data.
///
/// Venues that implement both [`DataStreamVenue`] and [`HistoricalDataVenue`]
/// automatically implement this trait.
pub trait FullDataVenue: DataStreamVenue + HistoricalDataVenue {}

// Auto-implement FullDataVenue for types implementing all required traits
impl<T> FullDataVenue for T where T: DataStreamVenue + HistoricalDataVenue {}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time check that traits have correct bounds
    fn _assert_send_sync<T: Send + Sync>() {}

    fn _check_trait_bounds() {
        _assert_send_sync::<Box<dyn DataStreamVenue>>();
        _assert_send_sync::<Box<dyn HistoricalDataVenue>>();
    }
}
