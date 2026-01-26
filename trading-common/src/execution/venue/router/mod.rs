//! Venue Router for multi-venue order management.
//!
//! The `VenueRouter` provides a unified interface for routing orders to multiple
//! execution venues based on the `instrument_id.venue` field. This enables:
//!
//! - **Cross-exchange arbitrage**: Trade the same underlying across venues
//! - **Best execution**: Route to venue with best price/liquidity
//! - **Redundancy**: Failover to backup venue if primary unavailable
//! - **Unified reporting**: Aggregate execution reports from all venues
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Strategy Layer                              │
//! │                                                                     │
//! │   Order { instrument_id: "BTCUSDT.BINANCE", ... }                  │
//! │   Order { instrument_id: "BTCUSDT.KRAKEN_FUTURES", ... }           │
//! └────────────────────────────┬────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        VenueRouter                                  │
//! │                                                                     │
//! │   ┌─────────────────────────────────────────────────────────────┐  │
//! │   │  Order ID Mapping                                            │  │
//! │   │  ClientOrderId → (VenueId, VenueOrderId)                    │  │
//! │   └─────────────────────────────────────────────────────────────┘  │
//! │                                                                     │
//! │   ┌─────────────────────────────────────────────────────────────┐  │
//! │   │  Venue Registry                                              │  │
//! │   │  HashMap<VenueId, Box<dyn FullExecutionVenue>>              │  │
//! │   └─────────────────────────────────────────────────────────────┘  │
//! │                                                                     │
//! │   ┌─────────────────────────────────────────────────────────────┐  │
//! │   │  Execution Stream Aggregator                                 │  │
//! │   │  Combines WebSocket feeds from all venues                   │  │
//! │   └─────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────┬────────────────────────────────────────┘
//!                              │
//!          ┌───────────────────┼───────────────────┐
//!          ▼                   ▼                   ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │ Binance Spot    │ │ Kraken Futures  │ │ Other Venues    │
//! │ (BINANCE)       │ │ (KRAKEN_FUTURES)│ │ (...)           │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::router::{VenueRouter, VenueRouterConfig};
//! use trading_common::execution::venue::binance::create_binance_spot_us;
//! use trading_common::execution::venue::kraken::create_kraken_futures_demo;
//! use trading_common::orders::{Order, OrderSide};
//! use rust_decimal_macros::dec;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create router
//!     let mut router = VenueRouter::new(VenueRouterConfig::default());
//!
//!     // Register venues
//!     router.register_venue("BINANCE", create_binance_spot_us()?);
//!     router.register_venue("KRAKEN_FUTURES", create_kraken_futures_demo()?);
//!
//!     // Connect all venues
//!     router.connect().await?;
//!
//!     // Submit orders - router routes based on venue in instrument_id
//!     let binance_order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.01), dec!(50000))
//!         .with_venue("BINANCE")
//!         .build()?;
//!
//!     let kraken_order = Order::limit("BTCUSDT", OrderSide::Sell, dec!(0.01), dec!(50100))
//!         .with_venue("KRAKEN_FUTURES")
//!         .build()?;
//!
//!     // Both orders go to their respective venues
//!     let binance_id = router.submit_order(&binance_order).await?;
//!     let kraken_id = router.submit_order(&kraken_order).await?;
//!
//!     // Query across all venues
//!     let all_open = router.query_open_orders(None).await?;
//!     println!("Total open orders: {}", all_open.len());
//!
//!     Ok(())
//! }
//! ```
//!
//! # Cross-Exchange Arbitrage
//!
//! ```ignore
//! // Arbitrage strategy using router
//! impl ArbitrageStrategy {
//!     async fn execute_spread(&self, router: &VenueRouter, spread: &Spread) {
//!         // Buy on cheaper exchange
//!         let buy_order = Order::limit("BTCUSDT", OrderSide::Buy, spread.qty, spread.bid)
//!             .with_venue(&spread.bid_venue)  // e.g., "BINANCE"
//!             .build()?;
//!
//!         // Sell on more expensive exchange
//!         let sell_order = Order::limit("BTCUSDT", OrderSide::Sell, spread.qty, spread.ask)
//!             .with_venue(&spread.ask_venue)  // e.g., "KRAKEN_FUTURES"
//!             .build()?;
//!
//!         // Submit both legs through unified router
//!         let (buy_result, sell_result) = tokio::join!(
//!             router.submit_order(&buy_order),
//!             router.submit_order(&sell_order),
//!         );
//!     }
//! }
//! ```

mod config;
mod router;
mod symbol_registry;

pub use config::{RouteRule, VenueRouterConfig};
pub use router::VenueRouter;
pub use symbol_registry::{CanonicalSymbol, SymbolRegistry, VenueSymbolMapping};
