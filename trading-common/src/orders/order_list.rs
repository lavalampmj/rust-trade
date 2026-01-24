//! Order list for managing bracket orders and contingent order groups.
//!
//! An `OrderList` groups related orders together (e.g., bracket orders with
//! entry, stop loss, and take profit) and defines how they interact through
//! contingency relationships.
//!
//! # Example
//! ```ignore
//! // Create a bracket order: entry triggers stop loss and take profit
//! let entry = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000));
//! let stop_loss = Order::stop("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(49000));
//! let take_profit = Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(52000));
//!
//! let bracket = OrderList::bracket(entry, stop_loss, take_profit);
//! ```

use super::order::Order;
use super::types::{ClientOrderId, ContingencyType, OrderListId};
use std::collections::HashSet;

/// A group of related orders with contingency relationships.
#[derive(Debug, Clone)]
pub struct OrderList {
    /// Unique identifier for this order list
    pub id: OrderListId,
    /// Orders in this list
    orders: Vec<Order>,
    /// Type of contingency relationship
    pub contingency_type: ContingencyType,
    /// ID of the primary/entry order (for OTO relationships)
    pub entry_order_id: Option<ClientOrderId>,
    /// IDs of orders that are triggered/submitted when entry fills (OTO children)
    pub child_order_ids: HashSet<ClientOrderId>,
    /// Whether the list is active
    pub is_active: bool,
}

impl OrderList {
    /// Create a new order list with the given contingency type.
    pub fn new(orders: Vec<Order>, contingency_type: ContingencyType) -> Self {
        let id = OrderListId::generate();
        Self {
            id,
            orders,
            contingency_type,
            entry_order_id: None,
            child_order_ids: HashSet::new(),
            is_active: true,
        }
    }

    /// Create a bracket order list (entry + stop loss + take profit).
    ///
    /// The entry order uses OTO contingency - when it fills, it triggers
    /// submission of the stop loss and take profit orders. The stop and
    /// take profit use OCO contingency - when one fills, the other is canceled.
    pub fn bracket(entry: Order, stop_loss: Order, take_profit: Order) -> Self {
        let id = OrderListId::generate();
        let entry_id = entry.client_order_id.clone();
        let stop_id = stop_loss.client_order_id.clone();
        let take_profit_id = take_profit.client_order_id.clone();

        let mut child_ids = HashSet::new();
        child_ids.insert(stop_id);
        child_ids.insert(take_profit_id);

        Self {
            id,
            orders: vec![entry, stop_loss, take_profit],
            contingency_type: ContingencyType::OTO, // Entry triggers others
            entry_order_id: Some(entry_id),
            child_order_ids: child_ids,
            is_active: true,
        }
    }

    /// Create an OCO (One-Cancels-Other) order list.
    ///
    /// When any order in the list is filled or canceled, all other
    /// orders in the list are automatically canceled.
    pub fn oco(orders: Vec<Order>) -> Self {
        let id = OrderListId::generate();
        Self {
            id,
            orders,
            contingency_type: ContingencyType::OCO,
            entry_order_id: None,
            child_order_ids: HashSet::new(),
            is_active: true,
        }
    }

    /// Create an OTO (One-Triggers-Other) order list.
    ///
    /// When the entry order fills, the child orders are submitted.
    pub fn oto(entry: Order, children: Vec<Order>) -> Self {
        let id = OrderListId::generate();
        let entry_id = entry.client_order_id.clone();
        let child_ids: HashSet<ClientOrderId> =
            children.iter().map(|o| o.client_order_id.clone()).collect();

        let mut orders = vec![entry];
        orders.extend(children);

        Self {
            id,
            orders,
            contingency_type: ContingencyType::OTO,
            entry_order_id: Some(entry_id),
            child_order_ids: child_ids,
            is_active: true,
        }
    }

    /// Get all orders in the list.
    pub fn orders(&self) -> &[Order] {
        &self.orders
    }

    /// Get the entry/primary order if this is an OTO list.
    pub fn entry_order(&self) -> Option<&Order> {
        self.entry_order_id
            .as_ref()
            .and_then(|id| self.orders.iter().find(|o| &o.client_order_id == id))
    }

    /// Get child orders (orders triggered by entry fill).
    pub fn child_orders(&self) -> Vec<&Order> {
        self.orders
            .iter()
            .filter(|o| self.child_order_ids.contains(&o.client_order_id))
            .collect()
    }

    /// Get owned copies of child orders for submission.
    pub fn child_orders_owned(&self) -> Vec<Order> {
        self.orders
            .iter()
            .filter(|o| self.child_order_ids.contains(&o.client_order_id))
            .cloned()
            .collect()
    }

    /// Get sibling orders (other orders in an OCO relationship).
    pub fn sibling_orders(&self, order_id: &ClientOrderId) -> Vec<ClientOrderId> {
        self.orders
            .iter()
            .filter(|o| &o.client_order_id != order_id)
            .map(|o| o.client_order_id.clone())
            .collect()
    }

    /// Check if an order is in this list.
    pub fn contains_order(&self, order_id: &ClientOrderId) -> bool {
        self.orders.iter().any(|o| &o.client_order_id == order_id)
    }

    /// Check if an order is the entry order.
    pub fn is_entry_order(&self, order_id: &ClientOrderId) -> bool {
        self.entry_order_id.as_ref() == Some(order_id)
    }

    /// Check if an order is a child order.
    pub fn is_child_order(&self, order_id: &ClientOrderId) -> bool {
        self.child_order_ids.contains(order_id)
    }

    /// Get the number of orders in the list.
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Check if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Deactivate the list (after all contingencies are resolved).
    pub fn deactivate(&mut self) {
        self.is_active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::OrderSide;
    use rust_decimal_macros::dec;

    fn create_test_orders() -> (Order, Order, Order) {
        let entry = Order::limit("BTCUSDT", OrderSide::Buy, dec!(1.0), dec!(50000))
            .build()
            .unwrap();
        let stop_loss = Order::stop("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(49000))
            .build()
            .unwrap();
        let take_profit = Order::limit("BTCUSDT", OrderSide::Sell, dec!(1.0), dec!(52000))
            .build()
            .unwrap();
        (entry, stop_loss, take_profit)
    }

    #[test]
    fn test_bracket_order_list() {
        let (entry, stop, tp) = create_test_orders();
        let entry_id = entry.client_order_id.clone();
        let stop_id = stop.client_order_id.clone();
        let tp_id = tp.client_order_id.clone();

        let list = OrderList::bracket(entry, stop, tp);

        assert_eq!(list.len(), 3);
        assert_eq!(list.contingency_type, ContingencyType::OTO);
        assert_eq!(list.entry_order_id, Some(entry_id.clone()));
        assert!(list.is_active);

        // Entry should be the entry order
        assert!(list.is_entry_order(&entry_id));
        assert!(!list.is_child_order(&entry_id));

        // Stop and TP should be child orders
        assert!(list.is_child_order(&stop_id));
        assert!(list.is_child_order(&tp_id));
        assert!(!list.is_entry_order(&stop_id));

        // Child orders
        let children = list.child_orders();
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn test_oco_order_list() {
        let (_, stop, tp) = create_test_orders();
        let stop_id = stop.client_order_id.clone();
        let tp_id = tp.client_order_id.clone();

        let list = OrderList::oco(vec![stop, tp]);

        assert_eq!(list.len(), 2);
        assert_eq!(list.contingency_type, ContingencyType::OCO);
        assert_eq!(list.entry_order_id, None);

        // Get siblings
        let siblings = list.sibling_orders(&stop_id);
        assert_eq!(siblings.len(), 1);
        assert_eq!(siblings[0], tp_id);
    }

    #[test]
    fn test_oto_order_list() {
        let (entry, stop, tp) = create_test_orders();
        let entry_id = entry.client_order_id.clone();

        let list = OrderList::oto(entry, vec![stop, tp]);

        assert_eq!(list.len(), 3);
        assert_eq!(list.contingency_type, ContingencyType::OTO);
        assert!(list.is_entry_order(&entry_id));
        assert_eq!(list.child_orders().len(), 2);
    }

    #[test]
    fn test_contains_and_lookup() {
        let (entry, stop, tp) = create_test_orders();
        let entry_id = entry.client_order_id.clone();
        let fake_id = ClientOrderId::generate();

        let list = OrderList::bracket(entry, stop, tp);

        assert!(list.contains_order(&entry_id));
        assert!(!list.contains_order(&fake_id));

        let found_entry = list.entry_order();
        assert!(found_entry.is_some());
        assert_eq!(found_entry.unwrap().client_order_id, entry_id);
    }

    #[test]
    fn test_deactivate() {
        let (entry, stop, tp) = create_test_orders();
        let mut list = OrderList::bracket(entry, stop, tp);

        assert!(list.is_active);
        list.deactivate();
        assert!(!list.is_active);
    }
}
