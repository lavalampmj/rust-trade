//! Inflight command queue for latency-delayed order processing.
//!
//! This module provides a priority queue for trading commands that simulates
//! network latency. Commands are queued with an arrival time and only processed
//! when the simulated exchange time advances past that arrival time.
//!
//! # Example
//!
//! ```ignore
//! use trading_common::execution::inflight_queue::*;
//! use trading_common::orders::Order;
//!
//! let mut queue = InflightQueue::new();
//!
//! // Submit order at time 0ns with 50ms latency
//! let order = Order::market(...);
//! queue.push(TradingCommand::SubmitOrder(order), 50_000_000);
//!
//! // At time 30ms, order hasn't arrived yet
//! let ready = queue.pop_ready(30_000_000);
//! assert!(ready.is_empty());
//!
//! // At time 50ms, order arrives
//! let ready = queue.pop_ready(50_000_000);
//! assert_eq!(ready.len(), 1);
//! ```

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::orders::{ClientOrderId, Order};
use rust_decimal::Decimal;

/// Trading commands that can be queued with latency.
#[derive(Debug, Clone)]
pub enum TradingCommand {
    /// Submit a new order
    SubmitOrder(Order),

    /// Cancel an existing order
    CancelOrder(ClientOrderId),

    /// Modify an existing order's price and/or quantity
    ModifyOrder {
        client_order_id: ClientOrderId,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    },
}

impl TradingCommand {
    /// Get the client order ID associated with this command.
    pub fn client_order_id(&self) -> &ClientOrderId {
        match self {
            TradingCommand::SubmitOrder(order) => &order.client_order_id,
            TradingCommand::CancelOrder(id) => id,
            TradingCommand::ModifyOrder {
                client_order_id, ..
            } => client_order_id,
        }
    }

    /// Check if this is a submit order command.
    pub fn is_submit(&self) -> bool {
        matches!(self, TradingCommand::SubmitOrder(_))
    }

    /// Check if this is a cancel order command.
    pub fn is_cancel(&self) -> bool {
        matches!(self, TradingCommand::CancelOrder(_))
    }

    /// Check if this is a modify order command.
    pub fn is_modify(&self) -> bool {
        matches!(self, TradingCommand::ModifyOrder { .. })
    }
}

/// A command in flight, waiting to arrive at the exchange.
#[derive(Debug, Clone)]
pub struct InflightCommand {
    /// The trading command
    pub command: TradingCommand,
    /// When the command will arrive at the exchange (nanoseconds since epoch)
    pub arrival_time_ns: u64,
    /// Sequence number for FIFO ordering of same-time arrivals
    sequence: u64,
}

impl InflightCommand {
    /// Create a new inflight command.
    pub fn new(command: TradingCommand, arrival_time_ns: u64, sequence: u64) -> Self {
        Self {
            command,
            arrival_time_ns,
            sequence,
        }
    }
}

// ============================================================================
// Priority Queue Ordering
// ============================================================================
//
// Rust's BinaryHeap is a MAX-heap, but we need a MIN-heap (earliest arrival first).
// We achieve this by REVERSING the comparison: `other.cmp(&self)` instead of
// `self.cmp(&other)`.
//
// Why reversed ordering works:
// - Normal max-heap: largest element at top → pop returns largest
// - Reversed comparison: "largest" becomes smallest → pop returns smallest (earliest)
//
// FIFO tie-breaking with sequence numbers:
// - When two commands arrive at the same nanosecond, we need deterministic ordering
// - Lower sequence number = submitted earlier = should process first
// - So we also reverse the sequence comparison
//
// Example timeline:
//   t=100ns: SubmitOrder(A) → sequence=0
//   t=100ns: SubmitOrder(B) → sequence=1
//   t=150ns: CancelOrder(A) → sequence=2
//
// Pop order at t=200ns: SubmitOrder(A), SubmitOrder(B), CancelOrder(A)
// ============================================================================

impl PartialEq for InflightCommand {
    fn eq(&self, other: &Self) -> bool {
        self.arrival_time_ns == other.arrival_time_ns && self.sequence == other.sequence
    }
}

impl Eq for InflightCommand {}

impl PartialOrd for InflightCommand {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InflightCommand {
    fn cmp(&self, other: &Self) -> Ordering {
        // REVERSED comparison for min-heap behavior:
        // - `other.arrival_time_ns.cmp(&self.arrival_time_ns)` means smaller times are "greater"
        // - BinaryHeap pops "greatest" first, so we get earliest times first
        //
        // If same arrival time, use sequence number for FIFO (also reversed)
        match other.arrival_time_ns.cmp(&self.arrival_time_ns) {
            Ordering::Equal => other.sequence.cmp(&self.sequence),
            ordering => ordering,
        }
    }
}

/// Priority queue for inflight trading commands.
///
/// Commands are ordered by arrival time, with earliest arrivals processed first.
/// This simulates network latency between strategy decision and exchange processing.
#[derive(Debug, Default)]
pub struct InflightQueue {
    /// Priority queue (min-heap by arrival time)
    queue: BinaryHeap<InflightCommand>,
    /// Sequence counter for FIFO ordering
    sequence: u64,
}

impl InflightQueue {
    /// Create a new empty inflight queue.
    pub fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
            sequence: 0,
        }
    }

    /// Push a command onto the queue with its arrival time.
    ///
    /// # Arguments
    /// * `command` - The trading command to queue
    /// * `arrival_time_ns` - When the command arrives at the exchange (nanoseconds)
    pub fn push(&mut self, command: TradingCommand, arrival_time_ns: u64) {
        let inflight = InflightCommand::new(command, arrival_time_ns, self.sequence);
        self.sequence += 1;
        self.queue.push(inflight);
    }

    /// Pop all commands that have arrived by the given time.
    ///
    /// Returns commands in order of arrival (earliest first).
    /// Commands with arrival_time_ns <= current_time_ns are considered ready.
    ///
    /// # Arguments
    /// * `current_time_ns` - Current simulated time in nanoseconds
    pub fn pop_ready(&mut self, current_time_ns: u64) -> Vec<TradingCommand> {
        let mut ready = Vec::new();

        while let Some(inflight) = self.queue.peek() {
            if inflight.arrival_time_ns <= current_time_ns {
                let cmd = self.queue.pop().unwrap();
                ready.push(cmd.command);
            } else {
                break;
            }
        }

        ready
    }

    /// Peek at the next command's arrival time without removing it.
    pub fn next_arrival_time(&self) -> Option<u64> {
        self.queue.peek().map(|cmd| cmd.arrival_time_ns)
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Get the number of commands in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Clear all commands from the queue.
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Cancel a specific order by client order ID.
    ///
    /// Removes any pending SubmitOrder command for the given ID.
    /// Returns true if a command was removed.
    pub fn cancel_pending(&mut self, client_order_id: &ClientOrderId) -> bool {
        let original_len = self.queue.len();

        // Rebuild the queue without the canceled order
        let remaining: Vec<_> = self.queue.drain().collect();
        self.queue = remaining
            .into_iter()
            .filter(|cmd| cmd.command.client_order_id() != client_order_id)
            .collect();

        self.queue.len() < original_len
    }

    /// Get all pending commands for a specific order ID.
    pub fn get_pending_for_order(&self, client_order_id: &ClientOrderId) -> Vec<&TradingCommand> {
        self.queue
            .iter()
            .filter(|cmd| cmd.command.client_order_id() == client_order_id)
            .map(|cmd| &cmd.command)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::{Order, OrderSide};
    use rust_decimal_macros::dec;

    fn create_test_order(symbol: &str) -> Order {
        Order::market(symbol, OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap()
    }

    #[test]
    fn test_empty_queue() {
        let mut queue = InflightQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert!(queue.pop_ready(1_000_000_000).is_empty());
        assert!(queue.next_arrival_time().is_none());
    }

    #[test]
    fn test_push_and_pop_single() {
        let mut queue = InflightQueue::new();
        let order = create_test_order("BTCUSDT");

        queue.push(TradingCommand::SubmitOrder(order), 50_000_000);

        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.next_arrival_time(), Some(50_000_000));

        // Before arrival time
        let ready = queue.pop_ready(30_000_000);
        assert!(ready.is_empty());
        assert_eq!(queue.len(), 1);

        // At arrival time
        let ready = queue.pop_ready(50_000_000);
        assert_eq!(ready.len(), 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_fifo_ordering() {
        let mut queue = InflightQueue::new();

        let order1 = create_test_order("BTCUSDT");
        let order2 = create_test_order("ETHUSDT");
        let order3 = create_test_order("SOLUSDT");

        // All arrive at the same time
        queue.push(TradingCommand::SubmitOrder(order1.clone()), 100);
        queue.push(TradingCommand::SubmitOrder(order2.clone()), 100);
        queue.push(TradingCommand::SubmitOrder(order3.clone()), 100);

        let ready = queue.pop_ready(100);
        assert_eq!(ready.len(), 3);

        // Should be in FIFO order (by sequence)
        if let TradingCommand::SubmitOrder(o) = &ready[0] {
            assert_eq!(o.instrument_id.symbol, "BTCUSDT");
        }
        if let TradingCommand::SubmitOrder(o) = &ready[1] {
            assert_eq!(o.instrument_id.symbol, "ETHUSDT");
        }
        if let TradingCommand::SubmitOrder(o) = &ready[2] {
            assert_eq!(o.instrument_id.symbol, "SOLUSDT");
        }
    }

    #[test]
    fn test_priority_ordering() {
        let mut queue = InflightQueue::new();

        let order1 = create_test_order("BTCUSDT");
        let order2 = create_test_order("ETHUSDT");
        let order3 = create_test_order("SOLUSDT");

        // Different arrival times
        queue.push(TradingCommand::SubmitOrder(order3.clone()), 300);
        queue.push(TradingCommand::SubmitOrder(order1.clone()), 100);
        queue.push(TradingCommand::SubmitOrder(order2.clone()), 200);

        // Pop at time 150 - only order1 should be ready
        let ready = queue.pop_ready(150);
        assert_eq!(ready.len(), 1);
        if let TradingCommand::SubmitOrder(o) = &ready[0] {
            assert_eq!(o.instrument_id.symbol, "BTCUSDT");
        }

        // Pop at time 250 - order2 should be ready
        let ready = queue.pop_ready(250);
        assert_eq!(ready.len(), 1);
        if let TradingCommand::SubmitOrder(o) = &ready[0] {
            assert_eq!(o.instrument_id.symbol, "ETHUSDT");
        }

        // Pop at time 300 - order3 should be ready
        let ready = queue.pop_ready(300);
        assert_eq!(ready.len(), 1);
        if let TradingCommand::SubmitOrder(o) = &ready[0] {
            assert_eq!(o.instrument_id.symbol, "SOLUSDT");
        }
    }

    #[test]
    fn test_cancel_command() {
        let mut queue = InflightQueue::new();

        let order_id = ClientOrderId::generate();
        queue.push(TradingCommand::CancelOrder(order_id.clone()), 100);

        let ready = queue.pop_ready(100);
        assert_eq!(ready.len(), 1);
        assert!(ready[0].is_cancel());

        if let TradingCommand::CancelOrder(id) = &ready[0] {
            assert_eq!(id, &order_id);
        }
    }

    #[test]
    fn test_modify_command() {
        let mut queue = InflightQueue::new();

        let order_id = ClientOrderId::generate();
        queue.push(
            TradingCommand::ModifyOrder {
                client_order_id: order_id.clone(),
                new_price: Some(dec!(50000)),
                new_quantity: None,
            },
            100,
        );

        let ready = queue.pop_ready(100);
        assert_eq!(ready.len(), 1);
        assert!(ready[0].is_modify());

        if let TradingCommand::ModifyOrder {
            client_order_id,
            new_price,
            new_quantity,
        } = &ready[0]
        {
            assert_eq!(client_order_id, &order_id);
            assert_eq!(*new_price, Some(dec!(50000)));
            assert!(new_quantity.is_none());
        }
    }

    #[test]
    fn test_cancel_pending() {
        let mut queue = InflightQueue::new();

        let order1 = create_test_order("BTCUSDT");
        let order2 = create_test_order("ETHUSDT");

        let id1 = order1.client_order_id.clone();

        queue.push(TradingCommand::SubmitOrder(order1), 100);
        queue.push(TradingCommand::SubmitOrder(order2), 200);

        assert_eq!(queue.len(), 2);

        // Cancel order1
        let removed = queue.cancel_pending(&id1);
        assert!(removed);
        assert_eq!(queue.len(), 1);

        // Pop remaining
        let ready = queue.pop_ready(200);
        assert_eq!(ready.len(), 1);
        if let TradingCommand::SubmitOrder(o) = &ready[0] {
            assert_eq!(o.instrument_id.symbol, "ETHUSDT");
        }
    }

    #[test]
    fn test_cancel_pending_not_found() {
        let mut queue = InflightQueue::new();

        let order = create_test_order("BTCUSDT");
        queue.push(TradingCommand::SubmitOrder(order), 100);

        let removed = queue.cancel_pending(&ClientOrderId::generate());
        assert!(!removed);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_clear() {
        let mut queue = InflightQueue::new();

        queue.push(
            TradingCommand::SubmitOrder(create_test_order("BTCUSDT")),
            100,
        );
        queue.push(
            TradingCommand::SubmitOrder(create_test_order("ETHUSDT")),
            200,
        );

        assert_eq!(queue.len(), 2);

        queue.clear();

        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_get_pending_for_order() {
        let mut queue = InflightQueue::new();

        let order = create_test_order("BTCUSDT");
        let id = order.client_order_id.clone();

        queue.push(TradingCommand::SubmitOrder(order), 100);
        queue.push(
            TradingCommand::ModifyOrder {
                client_order_id: id.clone(),
                new_price: Some(dec!(50000)),
                new_quantity: None,
            },
            150,
        );
        queue.push(TradingCommand::CancelOrder(id.clone()), 200);

        let pending = queue.get_pending_for_order(&id);
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn test_pop_ready_multiple_batches() {
        let mut queue = InflightQueue::new();

        // Add orders at various times
        for i in 0..10 {
            let order = create_test_order(&format!("SYM{}", i));
            queue.push(TradingCommand::SubmitOrder(order), i * 100);
        }

        // Pop in batches
        let batch1 = queue.pop_ready(250); // Should get 0, 100, 200
        assert_eq!(batch1.len(), 3);

        let batch2 = queue.pop_ready(500); // Should get 300, 400, 500
        assert_eq!(batch2.len(), 3);

        let batch3 = queue.pop_ready(1000); // Should get rest
        assert_eq!(batch3.len(), 4);

        assert!(queue.is_empty());
    }
}
