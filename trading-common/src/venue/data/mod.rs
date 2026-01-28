//! Data venue traits for market data streaming and historical data retrieval.
//!
//! This module defines the traits for venues that provide market data:
//!
//! - [`DataStreamVenue`]: Real-time data streaming (trades, quotes, orderbook)
//! - [`HistoricalDataVenue`]: Historical data retrieval for backtesting
//! - [`FullDataVenue`]: Combined trait for full-featured data venues
//!
//! # Example
//!
//! ```ignore
//! use trading_common::venue::data::{DataStreamVenue, StreamEvent};
//!
//! async fn stream_data<V: DataStreamVenue>(venue: &mut V) -> VenueResult<()> {
//!     let subscription = LiveSubscription::trades(vec!["BTCUSD".into()]);
//!     let callback = Arc::new(|event: StreamEvent| {
//!         match event {
//!             StreamEvent::Tick(tick) => println!("Trade: {}", tick.price),
//!             _ => {}
//!         }
//!     });
//!     venue.subscribe(subscription, callback, shutdown_rx).await
//! }
//! ```

mod traits;
mod types;

pub use traits::{DataStreamVenue, FullDataVenue, HistoricalDataVenue};
pub use types::{
    DataAvailability, DataType, HistoricalRequest, LiveSubscription, StreamCallback,
    StreamCallbackDbn, StreamEvent, StreamEventDbn, SubscriptionStatus,
};
