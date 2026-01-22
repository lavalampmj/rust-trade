//! Execution module for order processing and fill simulation.
//!
//! This module provides the infrastructure for executing orders during
//! backtesting and live trading:
//!
//! - **StrategyContext**: Provides order management capabilities to strategies
//! - **ExecutionEngine**: Processes orders and simulates fills
//! - **FillModel**: Trait for customizable fill simulation
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                        Strategy                                  │
//! │                                                                  │
//! │  on_bar_data() ─────────────────────────────────────────┐       │
//! │         │                                                │       │
//! │         ▼                                                ▼       │
//! │  ┌──────────────┐    Order API    ┌──────────────────────────┐  │
//! │  │   Signal     │ ◄────────────── │   StrategyContext        │  │
//! │  │ (Buy/Sell)   │ (backward compat)│                          │  │
//! │  └──────┬───────┘                 │ • market_order()         │  │
//! │         │                         │ • limit_order()          │  │
//! │         │                         │ • stop_order()           │  │
//! │         │                         │ • cancel_order()         │  │
//! │         │                         │ • get_position()         │  │
//! └─────────┼─────────────────────────┴──────────────────────────┴──┘
//!           │                                    │
//!           ▼                                    ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                     ExecutionEngine                              │
//! │                                                                  │
//! │  ┌────────────────┐  ┌────────────────┐  ┌───────────────────┐  │
//! │  │ Signal         │  │ Order          │  │ Fill              │  │
//! │  │ Conversion     │──│ Processing     │──│ Simulation        │  │
//! │  │                │  │                │  │                   │  │
//! │  │ Buy → Market   │  │ Check limits   │  │ FillModel.fill()  │  │
//! │  │ Sell → Market  │  │ Check stops    │  │                   │  │
//! │  └────────────────┘  └────────────────┘  └─────────┬─────────┘  │
//! │                                                    │            │
//! │  ┌────────────────────────────────────────────────┴──────────┐  │
//! │  │                   Position & Cash Updates                  │  │
//! │  │                                                            │  │
//! │  │  Buy fill  → +position, -cash                             │  │
//! │  │  Sell fill → -position, +cash                             │  │
//! │  │  Commission → -cash                                       │  │
//! │  └────────────────────────────────────────────────────────────┘  │
//! └──────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ## Basic Signal-Based (Backward Compatible)
//!
//! ```ignore
//! let config = ExecutionEngineConfig::default();
//! let mut engine = ExecutionEngine::new(config);
//!
//! // Process market data
//! engine.on_tick(&tick);
//!
//! // Execute strategy signal
//! let signal = strategy.on_bar_data(&bar_data, &mut bars);
//! engine.execute_signal(&signal, current_price)?;
//!
//! // Process fills
//! let fills = engine.process_all_orders();
//! ```
//!
//! ## Advanced Order Management
//!
//! ```ignore
//! // Use context directly for advanced orders
//! let ctx = engine.context_mut();
//!
//! // Submit limit order
//! let order_id = ctx.limit_order("BTCUSDT", OrderSide::Buy, dec!(0.1), dec!(49000))?;
//!
//! // Check if filled
//! if let Some(order) = ctx.get_order(&order_id) {
//!     if order.is_filled() {
//!         println!("Order filled at {}", order.avg_px.unwrap());
//!     }
//! }
//!
//! // Cancel unfilled orders
//! ctx.cancel_orders_for_symbol("BTCUSDT")?;
//! ```
//!
//! # Fill Models
//!
//! The execution engine uses pluggable fill models:
//!
//! - `ImmediateFillModel`: Optimistic fills at current price
//! - `LimitAwareFillModel`: Respects limit prices and OHLC ranges
//! - `SlippageAwareFillModel`: Adds price impact based on order size
//!
//! ```ignore
//! let fill_model = SlippageAwareFillModel::new(
//!     dec!(0.001),  // 0.1% base slippage
//!     dec!(0.0001), // Additional slippage per unit
//! );
//!
//! let engine = ExecutionEngine::new(config)
//!     .with_fill_model(Box::new(fill_model));
//! ```

mod context;
mod engine;
mod fill_model;

pub use context::{ContextError, StrategyContext, StrategyContextConfig};
pub use engine::{ExecutionEngine, ExecutionEngineConfig, ExecutionMetrics};
pub use fill_model::{
    default_fill_model, FillModel, FillResult, ImmediateFillModel, LimitAwareFillModel,
    MarketSnapshot, SlippageAwareFillModel,
};
