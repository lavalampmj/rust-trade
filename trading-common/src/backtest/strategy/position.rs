//! Position events for strategy notifications.
//!
//! This module provides the PositionEvent type that strategies receive
//! when their positions change due to fills or adjustments.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::orders::{AccountId, EventId, InstrumentId, PositionId, PositionSide, StrategyId};

/// Event generated when a position changes.
///
/// Position events notify strategies of changes to their positions, including:
/// - Position opened (new position from flat)
/// - Position increased (added to existing position)
/// - Position decreased (reduced existing position)
/// - Position closed (returned to flat)
/// - Mark-to-market updates (unrealized P&L changes)
///
/// # Example
///
/// ```ignore
/// fn on_position_update(&mut self, event: &PositionEvent) {
///     match event.side {
///         PositionSide::Long => {
///             println!("Long {} @ {}", event.quantity, event.avg_entry_price);
///         }
///         PositionSide::Short => {
///             println!("Short {} @ {}", event.quantity.abs(), event.avg_entry_price);
///         }
///         PositionSide::Flat => {
///             println!("Position closed. Realized P&L: {}", event.realized_pnl);
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionEvent {
    /// Unique event identifier
    pub event_id: EventId,

    /// Position identifier
    pub position_id: PositionId,

    /// Account holding the position
    pub account_id: AccountId,

    /// Instrument for this position
    pub instrument_id: InstrumentId,

    /// Strategy that owns this position
    pub strategy_id: StrategyId,

    /// Current position side (Long, Short, or Flat)
    pub side: PositionSide,

    /// Position quantity (positive for long, negative for short, zero for flat)
    pub quantity: Decimal,

    /// Average entry price for the position
    pub avg_entry_price: Decimal,

    /// Unrealized profit/loss at current mark price
    pub unrealized_pnl: Decimal,

    /// Realized profit/loss from closed portion
    pub realized_pnl: Decimal,

    /// Total commission paid
    pub commission: Decimal,

    /// Current mark price used for P&L calculation
    pub mark_price: Decimal,

    /// Timestamp when position was first opened
    pub ts_opened: DateTime<Utc>,

    /// Timestamp of last fill that affected this position
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts_last_fill: Option<DateTime<Utc>>,

    /// Event timestamp
    pub ts_event: DateTime<Utc>,
}

impl PositionEvent {
    /// Create a new position event
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        position_id: PositionId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        side: PositionSide,
        quantity: Decimal,
        avg_entry_price: Decimal,
        unrealized_pnl: Decimal,
        realized_pnl: Decimal,
        commission: Decimal,
        mark_price: Decimal,
        ts_opened: DateTime<Utc>,
    ) -> Self {
        Self {
            event_id: EventId::new(),
            position_id,
            account_id,
            instrument_id,
            strategy_id,
            side,
            quantity,
            avg_entry_price,
            unrealized_pnl,
            realized_pnl,
            commission,
            mark_price,
            ts_opened,
            ts_last_fill: None,
            ts_event: Utc::now(),
        }
    }

    /// Create a position opened event
    pub fn opened(
        position_id: PositionId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        side: PositionSide,
        quantity: Decimal,
        entry_price: Decimal,
        commission: Decimal,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            position_id,
            account_id,
            instrument_id,
            strategy_id,
            side,
            quantity,
            avg_entry_price: entry_price,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
            commission,
            mark_price: entry_price,
            ts_opened: now,
            ts_last_fill: Some(now),
            ts_event: now,
        }
    }

    /// Create a position closed event
    pub fn closed(
        position_id: PositionId,
        account_id: AccountId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        realized_pnl: Decimal,
        commission: Decimal,
        ts_opened: DateTime<Utc>,
    ) -> Self {
        Self {
            event_id: EventId::new(),
            position_id,
            account_id,
            instrument_id,
            strategy_id,
            side: PositionSide::Flat,
            quantity: Decimal::ZERO,
            avg_entry_price: Decimal::ZERO,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl,
            commission,
            mark_price: Decimal::ZERO,
            ts_opened,
            ts_last_fill: Some(Utc::now()),
            ts_event: Utc::now(),
        }
    }

    /// Set the last fill timestamp
    pub fn with_last_fill(mut self, ts_last_fill: DateTime<Utc>) -> Self {
        self.ts_last_fill = Some(ts_last_fill);
        self
    }

    /// Check if this represents an open position
    pub fn is_open(&self) -> bool {
        !self.quantity.is_zero()
    }

    /// Check if this represents a closed position (flat)
    pub fn is_flat(&self) -> bool {
        self.side == PositionSide::Flat || self.quantity.is_zero()
    }

    /// Check if this is a long position
    pub fn is_long(&self) -> bool {
        self.side == PositionSide::Long && !self.quantity.is_zero()
    }

    /// Check if this is a short position
    pub fn is_short(&self) -> bool {
        self.side == PositionSide::Short && !self.quantity.is_zero()
    }

    /// Get the notional value of the position
    pub fn notional(&self) -> Decimal {
        self.quantity.abs() * self.avg_entry_price
    }

    /// Get total P&L (realized + unrealized)
    pub fn total_pnl(&self) -> Decimal {
        self.realized_pnl + self.unrealized_pnl
    }

    /// Get net P&L (total P&L minus commission)
    pub fn net_pnl(&self) -> Decimal {
        self.total_pnl() - self.commission
    }

    /// Get the symbol from the instrument ID
    pub fn symbol(&self) -> &str {
        &self.instrument_id.symbol
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn create_test_ids() -> (PositionId, AccountId, InstrumentId, StrategyId) {
        (
            PositionId::new("pos-1"),
            AccountId::new("account-1"),
            InstrumentId::new("BTCUSDT", "BINANCE"),
            StrategyId::new("test-strategy"),
        )
    }

    #[test]
    fn test_position_event_new() {
        let (pos_id, acc_id, inst_id, strat_id) = create_test_ids();
        let ts_opened = Utc::now();

        let event = PositionEvent::new(
            pos_id.clone(),
            acc_id.clone(),
            inst_id.clone(),
            strat_id.clone(),
            PositionSide::Long,
            dec!(1.5),
            dec!(50000.0),
            dec!(100.0),
            Decimal::ZERO,
            dec!(0.5),
            dec!(50100.0),
            ts_opened,
        );

        assert_eq!(event.position_id.as_str(), "pos-1");
        assert_eq!(event.side, PositionSide::Long);
        assert_eq!(event.quantity, dec!(1.5));
        assert_eq!(event.avg_entry_price, dec!(50000.0));
        assert_eq!(event.unrealized_pnl, dec!(100.0));
        assert!(event.is_open());
        assert!(event.is_long());
        assert!(!event.is_short());
        assert!(!event.is_flat());
    }

    #[test]
    fn test_position_event_opened() {
        let (pos_id, acc_id, inst_id, strat_id) = create_test_ids();

        let event = PositionEvent::opened(
            pos_id,
            acc_id,
            inst_id,
            strat_id,
            PositionSide::Short,
            dec!(-2.0),
            dec!(45000.0),
            dec!(0.2),
        );

        assert_eq!(event.side, PositionSide::Short);
        assert_eq!(event.quantity, dec!(-2.0));
        assert_eq!(event.avg_entry_price, dec!(45000.0));
        assert_eq!(event.mark_price, dec!(45000.0));
        assert_eq!(event.commission, dec!(0.2));
        assert!(event.unrealized_pnl.is_zero());
        assert!(event.realized_pnl.is_zero());
        assert!(event.ts_last_fill.is_some());
    }

    #[test]
    fn test_position_event_closed() {
        let (pos_id, acc_id, inst_id, strat_id) = create_test_ids();
        let ts_opened = Utc::now();

        let event = PositionEvent::closed(
            pos_id,
            acc_id,
            inst_id,
            strat_id,
            dec!(500.0),
            dec!(1.0),
            ts_opened,
        );

        assert_eq!(event.side, PositionSide::Flat);
        assert!(event.quantity.is_zero());
        assert!(event.is_flat());
        assert!(!event.is_open());
        assert_eq!(event.realized_pnl, dec!(500.0));
    }

    #[test]
    fn test_position_event_calculations() {
        let (pos_id, acc_id, inst_id, strat_id) = create_test_ids();

        let event = PositionEvent::new(
            pos_id,
            acc_id,
            inst_id,
            strat_id,
            PositionSide::Long,
            dec!(2.0),
            dec!(50000.0),
            dec!(200.0), // unrealized
            dec!(100.0), // realized
            dec!(10.0),  // commission
            dec!(50100.0),
            Utc::now(),
        );

        assert_eq!(event.notional(), dec!(100000.0)); // 2 * 50000
        assert_eq!(event.total_pnl(), dec!(300.0)); // 200 + 100
        assert_eq!(event.net_pnl(), dec!(290.0)); // 300 - 10
    }

    #[test]
    fn test_position_event_symbol() {
        let (pos_id, acc_id, inst_id, strat_id) = create_test_ids();

        let event = PositionEvent::opened(
            pos_id,
            acc_id,
            inst_id,
            strat_id,
            PositionSide::Long,
            dec!(1.0),
            dec!(50000.0),
            Decimal::ZERO,
        );

        assert_eq!(event.symbol(), "BTCUSDT");
    }

    #[test]
    fn test_position_event_with_last_fill() {
        let (pos_id, acc_id, inst_id, strat_id) = create_test_ids();
        let last_fill_time = Utc::now();

        let event = PositionEvent::new(
            pos_id,
            acc_id,
            inst_id,
            strat_id,
            PositionSide::Long,
            dec!(1.0),
            dec!(50000.0),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(50000.0),
            Utc::now(),
        )
        .with_last_fill(last_fill_time);

        assert_eq!(event.ts_last_fill, Some(last_fill_time));
    }
}
