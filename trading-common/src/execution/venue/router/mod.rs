//! Venue Router for multi-venue order management.
//!
//! The `VenueRouter` provides a unified interface for routing orders to multiple
//! execution venues. This enables:
//!
//! - **Cross-exchange arbitrage**: Trade the same underlying across venues
//! - **Best execution**: Route to venue with best price/liquidity
//! - **Redundancy**: Failover to backup venue if primary unavailable
//! - **Unified reporting**: Aggregate execution reports from all venues
//!
//! # Databento Symbology Alignment
//!
//! The router uses [Databento's symbology conventions](https://databento.com/docs/standards-and-conventions/symbology):
//!
//! | DBT Type | Our Usage | Example |
//! |----------|-----------|---------|
//! | `parent` | Canonical ID | `BTC.SPOT`, `ES.FUT` |
//! | `raw_symbol` | Venue symbol | `BTCUSDT`, `XBT/USD`, `ESH6` |
//! | `continuous` | Roll contracts | `ES.c.0`, `CL.v.0` |
//!
//! # Symbol Conversion Flow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Strategy Layer                              │
//! │                                                                     │
//! │   Uses canonical symbols (DBT parent format):                       │
//! │   Order { instrument_id: "BTC.SPOT.BINANCE", ... }                 │
//! │   Order { instrument_id: "BTC.PERP.KRAKEN_FUTURES", ... }          │
//! └────────────────────────────┬────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        VenueRouter                                  │
//! │                                                                     │
//! │   ┌─────────────────────────────────────────────────────────────┐  │
//! │   │  SymbolRegistry                                              │  │
//! │   │  Canonical → raw_symbol conversion                          │  │
//! │   │  "BTC.SPOT" + "BINANCE" → "BTCUSDT"                         │  │
//! │   │  "BTC.PERP" + "KRAKEN_FUTURES" → "PI_XBTUSD"                │  │
//! │   └─────────────────────────────────────────────────────────────┘  │
//! │                                                                     │
//! │   ┌─────────────────────────────────────────────────────────────┐  │
//! │   │  Venue Registry                                              │  │
//! │   │  HashMap<VenueId, Box<dyn FullExecutionVenue>>              │  │
//! │   └─────────────────────────────────────────────────────────────┘  │
//! └────────────────────────────┬────────────────────────────────────────┘
//!                              │
//!          ┌───────────────────┼───────────────────┐
//!          ▼                   ▼                   ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │ Binance Spot    │ │ Kraken Futures  │ │ CME Globex      │
//! │ raw: BTCUSDT    │ │ raw: PI_XBTUSD  │ │ raw: ESH6       │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//! ```
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::venue::router::{
//!     VenueRouter, VenueRouterConfig, SymbolRegistry,
//!     CanonicalSymbol, InstrumentClass,
//! };
//! use trading_common::execution::venue::binance::create_binance_spot_us;
//! use trading_common::execution::venue::kraken::create_kraken_futures_demo;
//! use trading_common::orders::{Order, OrderSide};
//! use rust_decimal_macros::dec;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create router with symbol registry
//!     let mut router = VenueRouter::new(VenueRouterConfig::default());
//!
//!     // Symbol registry uses DBT parent format for canonical IDs
//!     let registry = SymbolRegistry::with_crypto_defaults();
//!     router.set_symbol_registry(registry);
//!
//!     // Register venues
//!     router.register_venue("BINANCE", create_binance_spot_us()?).await;
//!     router.register_venue("KRAKEN_FUTURES", create_kraken_futures_demo()?).await;
//!
//!     // Connect all venues
//!     router.connect().await?;
//!
//!     // Submit orders using canonical symbols
//!     // Router converts "BTC.SPOT" → "BTCUSDT" for Binance
//!     let spot_order = Order::limit("BTC.SPOT", OrderSide::Buy, dec!(0.01), dec!(50000))
//!         .with_venue("BINANCE")
//!         .build()?;
//!
//!     // Router converts "BTC.PERP" → "PI_XBTUSD" for Kraken Futures
//!     let perp_order = Order::limit("BTC.PERP", OrderSide::Sell, dec!(0.01), dec!(50100))
//!         .with_venue("KRAKEN_FUTURES")
//!         .build()?;
//!
//!     // Both orders go to their respective venues with correct raw_symbols
//!     let binance_id = router.submit_order(&spot_order).await?;
//!     let kraken_id = router.submit_order(&perp_order).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Cross-Exchange Arbitrage with Canonical Symbols
//!
//! ```ignore
//! impl ArbitrageStrategy {
//!     async fn execute_spread(&self, router: &VenueRouter, spread: &Spread) {
//!         // Both orders use same canonical symbol "BTC.SPOT"
//!         // Router converts to venue-specific raw_symbols
//!
//!         // "BTC.SPOT" + "BINANCE" → raw_symbol "BTCUSDT"
//!         let buy_order = Order::limit("BTC.SPOT", OrderSide::Buy, spread.qty, spread.bid)
//!             .with_venue("BINANCE")
//!             .build()?;
//!
//!         // "BTC.SPOT" + "KRAKEN" → raw_symbol "XBT/USD"
//!         let sell_order = Order::limit("BTC.SPOT", OrderSide::Sell, spread.qty, spread.ask)
//!             .with_venue("KRAKEN")
//!             .build()?;
//!
//!         // Submit both legs
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

pub use config::{RouteRule, RouteStrategy, VenueRouterConfig};
pub use router::VenueRouter;
pub use symbol_registry::{
    CanonicalSymbol, ContinuousRollMethod, ContinuousSymbol, InstrumentClass, SymbolRegistry,
    VenueSymbolMapping,
};
