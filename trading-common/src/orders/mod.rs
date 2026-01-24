//! Order management system for rust-trade.
//!
//! This module provides a comprehensive order management infrastructure including:
//!
//! - **Order Types**: Market, Limit, Stop, StopLimit, TrailingStop, etc.
//! - **Order Lifecycle**: Full state machine with validated transitions
//! - **Order Events**: Complete event stream for audit and strategy feedback
//! - **Order Manager**: Thread-safe tracking and querying of all orders
//!
//! # Architecture
//!
//! The order system is designed with these principles:
//!
//! 1. **Type Safety**: Strong types for IDs, quantities, prices prevent mixing
//! 2. **Immutability**: Orders are created via builders, state changes via events
//! 3. **Event Sourcing**: All state changes emit events for audit trail
//! 4. **Thread Safety**: Async order manager supports concurrent access
//!
//! # Example: Creating Orders
//!
//! ```ignore
//! use trading_common::orders::{Order, OrderSide, OrderType, TimeInForce};
//! use rust_decimal_macros::dec;
//!
//! // Market order
//! let market_order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1))
//!     .with_strategy_id("momentum-strategy")
//!     .build()?;
//!
//! // Limit order with custom settings
//! let limit_order = Order::limit("ETHUSDT", OrderSide::Sell, dec!(2.0), dec!(3500))
//!     .with_venue("BINANCE")
//!     .with_time_in_force(TimeInForce::IOC)
//!     .post_only()
//!     .build()?;
//!
//! // Stop-limit order
//! let stop_limit = Order::stop_limit(
//!     "BTCUSDT",
//!     OrderSide::Sell,
//!     dec!(1.0),
//!     dec!(49000),  // limit price
//!     dec!(50000),  // trigger price
//! )
//! .with_time_in_force(TimeInForce::GTC)
//! .build()?;
//! ```
//!
//! # Example: Using OrderManager
//!
//! ```ignore
//! use trading_common::orders::{OrderManager, OrderManagerConfig, Order, OrderSide};
//! use rust_decimal_macros::dec;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create order manager
//!     let manager = OrderManager::new(OrderManagerConfig {
//!         max_open_orders: 100,
//!         max_orders_per_symbol: 10,
//!         ..Default::default()
//!     });
//!
//!     // Register event callback
//!     manager.on_event(Box::new(|event| {
//!         println!("Order event: {:?}", event);
//!     })).await;
//!
//!     // Submit an order
//!     let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.1)).build()?;
//!     let client_id = manager.submit_order(order).await?;
//!
//!     // Query orders
//!     let open_orders = manager.get_open_orders().await;
//!     let btc_orders = manager.get_orders_for_symbol("BTCUSDT").await;
//!
//!     // Order lifecycle
//!     manager.mark_submitted(&client_id, "account-1".into()).await?;
//!     manager.mark_accepted(&client_id, "venue-123".into(), "account-1".into()).await?;
//!
//!     // Apply fill
//!     manager.apply_fill(
//!         &client_id,
//!         "venue-123".into(),
//!         "account-1".into(),
//!         TradeId::generate(),
//!         dec!(0.1),
//!         dec!(50000),
//!         dec!(0.005),
//!         "USDT".to_string(),
//!         LiquiditySide::Taker,
//!     ).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! # Order State Machine
//!
//! Orders follow a strict state machine:
//!
//! ```text
//! ┌──────────────┐
//! │ Initialized  │
//! └──────┬───────┘
//!        │
//!        ▼
//! ┌──────────────┐     ┌─────────┐
//! │  Submitted   │────►│ Denied  │ (risk check failed)
//! └──────┬───────┘     └─────────┘
//!        │
//!        ▼
//! ┌──────────────┐     ┌──────────┐
//! │   Accepted   │────►│ Rejected │ (venue rejected)
//! └──────┬───────┘     └──────────┘
//!        │
//!        ├─────────────────┬────────────────┐
//!        ▼                 ▼                ▼
//! ┌──────────────┐  ┌───────────┐   ┌──────────┐
//! │PartialFilled │  │ Triggered │   │ Canceled │
//! └──────┬───────┘  └─────┬─────┘   └──────────┘
//!        │                │
//!        ▼                │
//! ┌──────────────┐◄───────┘
//! │    Filled    │
//! └──────────────┘
//! ```
//!
//! # Events
//!
//! Every state transition generates an event:
//!
//! - `OrderInitialized` - Order created by strategy
//! - `OrderDenied` - Pre-submission risk check failed
//! - `OrderSubmitted` - Sent to execution system
//! - `OrderAccepted` - Acknowledged by venue
//! - `OrderRejected` - Rejected by venue
//! - `OrderFilled` - Partial or complete execution
//! - `OrderCanceled` - Canceled by user/system
//! - `OrderExpired` - Time-in-force expired
//! - `OrderTriggered` - Stop/conditional order triggered

mod contingent_manager;
mod events;
mod manager;
mod order;
mod order_list;
mod types;

// Re-export all public types
pub use contingent_manager::{ContingentAction, ContingentOrderManager};
pub use order_list::OrderList;

pub use events::{
    EventId, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderEvent,
    OrderEventAny, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
    OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderSubmitted, OrderTriggered,
    OrderUpdated,
};

pub use manager::{
    OrderManager, OrderManagerConfig, OrderManagerError, OrderManagerResult, OrderManagerStats,
    SyncOrderManager,
};

pub use order::{Order, OrderBuilder, OrderError};

pub use types::{
    AccountId, ClientOrderId, ContingencyType, InstrumentId, LiquiditySide, Money, OrderListId,
    OrderSide, OrderStatus, OrderType, PositionId, PositionSide, Price, Quantity,
    SessionEnforcement, StrategyId, TimeInForce, TradeId, TrailingOffsetType, TriggerType,
    VenueOrderId,
};
