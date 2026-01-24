//! Contingent order management for automatic bracket order handling.
//!
//! This module handles automatic OTO/OCO/OUO order management:
//!
//! - **OTO (One-Triggers-Other)**: When entry order fills, submit child orders
//! - **OCO (One-Cancels-Other)**: When one order fills/cancels, cancel siblings
//! - **OUO (One-Updates-Other)**: When entry partially fills, update child quantities
//!
//! # Example
//! ```ignore
//! let mut manager = ContingentOrderManager::new();
//!
//! // Register a bracket order
//! let bracket = OrderList::bracket(entry, stop_loss, take_profit);
//! manager.register_list(bracket);
//!
//! // Process order events
//! let actions = manager.on_order_event(&OrderEventAny::Filled(fill_event));
//!
//! // Execute actions
//! for action in actions {
//!     match action {
//!         ContingentAction::SubmitOrders(orders) => {
//!             for order in orders {
//!                 exchange.submit_order(order);
//!             }
//!         }
//!         ContingentAction::CancelOrders(ids) => {
//!             for id in ids {
//!                 exchange.cancel_order(id);
//!             }
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use super::order::Order;
use super::order_list::OrderList;
use super::types::{ClientOrderId, ContingencyType, OrderListId};
use crate::orders::events::OrderEventAny;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Actions to take in response to contingent order events.
#[derive(Debug, Clone)]
pub enum ContingentAction {
    /// Submit child orders (OTO: entry filled)
    SubmitOrders(Vec<Order>),
    /// Cancel sibling orders (OCO: one filled or canceled)
    CancelOrders(Vec<ClientOrderId>),
    /// Update order quantities (OUO: partial fill)
    UpdateOrders(Vec<(ClientOrderId, Decimal)>),
}

/// Manager for contingent order relationships.
///
/// Tracks order lists and generates appropriate actions when order
/// events occur (fills, cancels, etc.).
#[derive(Debug, Default)]
pub struct ContingentOrderManager {
    /// Active order lists indexed by list ID
    order_lists: HashMap<OrderListId, OrderList>,
    /// Mapping from order ID to its list ID
    order_to_list: HashMap<ClientOrderId, OrderListId>,
    /// Track which OTO children have been submitted
    submitted_children: HashMap<OrderListId, bool>,
    /// Track which OCO orders have been processed
    processed_oco: HashMap<OrderListId, bool>,
}

impl ContingentOrderManager {
    /// Create a new contingent order manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an order list for contingent management.
    pub fn register_list(&mut self, list: OrderList) {
        let list_id = list.id.clone();

        // Map each order to this list
        for order in list.orders() {
            self.order_to_list
                .insert(order.client_order_id.clone(), list_id.clone());
        }

        self.order_lists.insert(list_id, list);
    }

    /// Unregister an order list.
    pub fn unregister_list(&mut self, list_id: &OrderListId) {
        if let Some(list) = self.order_lists.remove(list_id) {
            for order in list.orders() {
                self.order_to_list.remove(&order.client_order_id);
            }
            self.submitted_children.remove(list_id);
            self.processed_oco.remove(list_id);
        }
    }

    /// Get the order list for a specific order.
    pub fn get_list_for_order(&self, order_id: &ClientOrderId) -> Option<&OrderList> {
        self.order_to_list
            .get(order_id)
            .and_then(|list_id| self.order_lists.get(list_id))
    }

    /// Get an order list by ID.
    pub fn get_list(&self, list_id: &OrderListId) -> Option<&OrderList> {
        self.order_lists.get(list_id)
    }

    /// Process an order event and return actions to take.
    ///
    /// Call this method for every order event. It will check if the order
    /// is part of a contingent order list and return appropriate actions.
    pub fn on_order_event(&mut self, event: &OrderEventAny) -> Vec<ContingentAction> {
        match event {
            OrderEventAny::Filled(fill) => {
                // Check if this is a partial fill (leaves_qty > 0) or full fill
                if fill.leaves_qty > Decimal::ZERO {
                    // Partial fill
                    self.on_order_partially_filled(&fill.client_order_id, fill.last_qty)
                } else {
                    // Full fill
                    self.on_order_filled(&fill.client_order_id)
                }
            }
            OrderEventAny::Canceled(cancel) => self.on_order_canceled(&cancel.client_order_id),
            _ => Vec::new(),
        }
    }

    /// Handle order filled event.
    fn on_order_filled(&mut self, order_id: &ClientOrderId) -> Vec<ContingentAction> {
        let mut actions = Vec::new();

        // Get the list for this order
        let list_id = match self.order_to_list.get(order_id).cloned() {
            Some(id) => id,
            None => return actions,
        };

        let list = match self.order_lists.get(&list_id) {
            Some(l) if l.is_active => l,
            _ => return actions,
        };

        match list.contingency_type {
            ContingencyType::OTO => {
                // Entry order filled -> submit children
                if list.is_entry_order(order_id) {
                    // Check if we haven't already submitted children
                    if !self
                        .submitted_children
                        .get(&list_id)
                        .copied()
                        .unwrap_or(false)
                    {
                        let children = list.child_orders_owned();
                        if !children.is_empty() {
                            actions.push(ContingentAction::SubmitOrders(children));
                            self.submitted_children.insert(list_id.clone(), true);
                        }
                    }
                }
                // Child order filled -> cancel siblings (OCO behavior within bracket)
                else if list.is_child_order(order_id) {
                    if !self.processed_oco.get(&list_id).copied().unwrap_or(false) {
                        let siblings = list.sibling_orders(order_id);
                        // Only cancel other child orders, not the entry
                        let child_siblings: Vec<_> = siblings
                            .into_iter()
                            .filter(|id| list.is_child_order(id))
                            .collect();
                        if !child_siblings.is_empty() {
                            actions.push(ContingentAction::CancelOrders(child_siblings));
                            self.processed_oco.insert(list_id.clone(), true);
                        }
                    }
                }
            }
            ContingencyType::OCO => {
                // One filled -> cancel all others
                if !self.processed_oco.get(&list_id).copied().unwrap_or(false) {
                    let siblings = list.sibling_orders(order_id);
                    if !siblings.is_empty() {
                        actions.push(ContingentAction::CancelOrders(siblings));
                        self.processed_oco.insert(list_id, true);
                    }
                }
            }
            ContingencyType::OUO => {
                // One filled -> update others (but this is for full fills, handled differently)
                // For full fills on entry, we might want to adjust child quantities
                // This is more complex and depends on business rules
            }
            ContingencyType::None => {}
        }

        actions
    }

    /// Handle order partially filled event.
    fn on_order_partially_filled(
        &mut self,
        order_id: &ClientOrderId,
        filled_qty: Decimal,
    ) -> Vec<ContingentAction> {
        let mut actions = Vec::new();

        // Get the list for this order
        let list_id = match self.order_to_list.get(order_id).cloned() {
            Some(id) => id,
            None => return actions,
        };

        let list = match self.order_lists.get(&list_id) {
            Some(l) if l.is_active => l,
            _ => return actions,
        };

        // OUO: Partial fill on entry -> update child quantities
        if list.contingency_type == ContingencyType::OUO && list.is_entry_order(order_id) {
            // Get entry order's original quantity
            if let Some(entry) = list.entry_order() {
                let original_qty = entry.quantity;
                // Calculate remaining proportion
                let fill_ratio = filled_qty / original_qty;

                // Update child order quantities proportionally
                let updates: Vec<_> = list
                    .child_order_ids
                    .iter()
                    .filter_map(|child_id| {
                        list.orders()
                            .iter()
                            .find(|o| &o.client_order_id == child_id)
                            .map(|child| {
                                let new_qty = child.quantity * fill_ratio;
                                (child_id.clone(), new_qty)
                            })
                    })
                    .collect();

                if !updates.is_empty() {
                    actions.push(ContingentAction::UpdateOrders(updates));
                }
            }
        }

        actions
    }

    /// Handle order canceled event.
    fn on_order_canceled(&mut self, order_id: &ClientOrderId) -> Vec<ContingentAction> {
        let mut actions = Vec::new();

        // Get the list for this order
        let list_id = match self.order_to_list.get(order_id).cloned() {
            Some(id) => id,
            None => return actions,
        };

        let list = match self.order_lists.get(&list_id) {
            Some(l) if l.is_active => l,
            _ => return actions,
        };

        match list.contingency_type {
            ContingencyType::OCO => {
                // One canceled -> cancel all others
                if !self.processed_oco.get(&list_id).copied().unwrap_or(false) {
                    let siblings = list.sibling_orders(order_id);
                    if !siblings.is_empty() {
                        actions.push(ContingentAction::CancelOrders(siblings));
                        self.processed_oco.insert(list_id, true);
                    }
                }
            }
            ContingencyType::OTO => {
                // If entry is canceled before fill, cancel children too (if submitted)
                if list.is_entry_order(order_id) {
                    if self
                        .submitted_children
                        .get(&list_id)
                        .copied()
                        .unwrap_or(false)
                    {
                        let children: Vec<_> = list.child_order_ids.iter().cloned().collect();
                        if !children.is_empty() {
                            actions.push(ContingentAction::CancelOrders(children));
                        }
                    }
                }
            }
            _ => {}
        }

        actions
    }

    /// Get the number of active order lists.
    pub fn active_list_count(&self) -> usize {
        self.order_lists.values().filter(|l| l.is_active).count()
    }

    /// Clear all order lists.
    pub fn clear(&mut self) {
        self.order_lists.clear();
        self.order_to_list.clear();
        self.submitted_children.clear();
        self.processed_oco.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::events::{EventId, OrderCanceled, OrderFilled};
    use crate::orders::{AccountId, OrderSide, VenueOrderId};
    use chrono::Utc;
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

    fn create_fill_event(order: &Order) -> OrderFilled {
        OrderFilled {
            event_id: EventId(uuid::Uuid::new_v4()),
            client_order_id: order.client_order_id.clone(),
            venue_order_id: VenueOrderId("TEST-1".to_string()),
            account_id: AccountId::default(),
            instrument_id: order.instrument_id.clone(),
            trade_id: crate::orders::TradeId::generate(),
            position_id: None,
            strategy_id: order.strategy_id.clone(),
            order_side: order.side,
            order_type: order.order_type,
            last_qty: order.quantity,
            last_px: order.price.unwrap_or(dec!(50000)),
            cum_qty: order.quantity,
            leaves_qty: Decimal::ZERO,
            currency: "USD".to_string(),
            commission: Decimal::ZERO,
            commission_currency: "USD".to_string(),
            liquidity_side: crate::orders::LiquiditySide::Taker,
            ts_event: Utc::now(),
            ts_init: Utc::now(),
        }
    }

    fn create_cancel_event(order: &Order) -> OrderCanceled {
        OrderCanceled {
            event_id: EventId(uuid::Uuid::new_v4()),
            client_order_id: order.client_order_id.clone(),
            venue_order_id: Some(VenueOrderId("TEST-1".to_string())),
            account_id: AccountId::default(),
            ts_event: Utc::now(),
            ts_init: Utc::now(),
        }
    }

    #[test]
    fn test_register_bracket_order() {
        let mut manager = ContingentOrderManager::new();
        let (entry, stop, tp) = create_test_orders();
        let entry_id = entry.client_order_id.clone();

        let bracket = OrderList::bracket(entry, stop, tp);
        let list_id = bracket.id.clone();

        manager.register_list(bracket);

        assert_eq!(manager.active_list_count(), 1);
        assert!(manager.get_list_for_order(&entry_id).is_some());
        assert!(manager.get_list(&list_id).is_some());
    }

    #[test]
    fn test_oto_entry_fill_submits_children() {
        let mut manager = ContingentOrderManager::new();
        let (entry, stop, tp) = create_test_orders();
        let stop_id = stop.client_order_id.clone();
        let tp_id = tp.client_order_id.clone();

        let bracket = OrderList::bracket(entry.clone(), stop, tp);
        manager.register_list(bracket);

        // Simulate entry fill
        let fill = create_fill_event(&entry);
        let actions = manager.on_order_event(&OrderEventAny::Filled(fill));

        // Should submit child orders
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ContingentAction::SubmitOrders(orders) => {
                assert_eq!(orders.len(), 2);
                let ids: Vec<_> = orders.iter().map(|o| o.client_order_id.clone()).collect();
                assert!(ids.contains(&stop_id));
                assert!(ids.contains(&tp_id));
            }
            _ => panic!("Expected SubmitOrders action"),
        }
    }

    #[test]
    fn test_oto_child_fill_cancels_siblings() {
        let mut manager = ContingentOrderManager::new();
        let (entry, stop, tp) = create_test_orders();
        let stop_id = stop.client_order_id.clone();
        let tp_id = tp.client_order_id.clone();

        let bracket = OrderList::bracket(entry.clone(), stop.clone(), tp);
        manager.register_list(bracket);

        // First, entry fills
        let entry_fill = create_fill_event(&entry);
        manager.on_order_event(&OrderEventAny::Filled(entry_fill));

        // Then, stop loss fills
        let stop_fill = create_fill_event(&stop);
        let actions = manager.on_order_event(&OrderEventAny::Filled(stop_fill));

        // Should cancel take profit
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ContingentAction::CancelOrders(ids) => {
                assert_eq!(ids.len(), 1);
                assert_eq!(ids[0], tp_id);
                // Should not contain the entry order ID
                assert!(!ids.contains(&stop_id));
            }
            _ => panic!("Expected CancelOrders action"),
        }
    }

    #[test]
    fn test_oco_fill_cancels_siblings() {
        let mut manager = ContingentOrderManager::new();
        let (_, stop, tp) = create_test_orders();
        let _stop_id = stop.client_order_id.clone();
        let tp_id = tp.client_order_id.clone();

        let oco = OrderList::oco(vec![stop.clone(), tp]);
        manager.register_list(oco);

        // Stop fills
        let fill = create_fill_event(&stop);
        let actions = manager.on_order_event(&OrderEventAny::Filled(fill));

        // Should cancel take profit
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ContingentAction::CancelOrders(ids) => {
                assert_eq!(ids.len(), 1);
                assert_eq!(ids[0], tp_id);
            }
            _ => panic!("Expected CancelOrders action"),
        }
    }

    #[test]
    fn test_oco_cancel_cancels_siblings() {
        let mut manager = ContingentOrderManager::new();
        let (_, stop, tp) = create_test_orders();
        let tp_id = tp.client_order_id.clone();

        let oco = OrderList::oco(vec![stop.clone(), tp]);
        manager.register_list(oco);

        // Stop is canceled
        let cancel = create_cancel_event(&stop);
        let actions = manager.on_order_event(&OrderEventAny::Canceled(cancel));

        // Should cancel take profit
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ContingentAction::CancelOrders(ids) => {
                assert_eq!(ids.len(), 1);
                assert_eq!(ids[0], tp_id);
            }
            _ => panic!("Expected CancelOrders action"),
        }
    }

    #[test]
    fn test_no_duplicate_actions() {
        let mut manager = ContingentOrderManager::new();
        let (entry, stop, tp) = create_test_orders();

        let bracket = OrderList::bracket(entry.clone(), stop.clone(), tp);
        manager.register_list(bracket);

        // Entry fills - should submit children
        let entry_fill = create_fill_event(&entry);
        let actions1 = manager.on_order_event(&OrderEventAny::Filled(entry_fill.clone()));
        assert_eq!(actions1.len(), 1);

        // Entry fills again (duplicate event) - should not submit again
        let actions2 = manager.on_order_event(&OrderEventAny::Filled(entry_fill));
        assert!(actions2.is_empty());

        // Stop fills - should cancel TP
        let stop_fill = create_fill_event(&stop);
        let actions3 = manager.on_order_event(&OrderEventAny::Filled(stop_fill.clone()));
        assert_eq!(actions3.len(), 1);

        // Stop fills again - should not cancel again
        let actions4 = manager.on_order_event(&OrderEventAny::Filled(stop_fill));
        assert!(actions4.is_empty());
    }

    #[test]
    fn test_unregister_list() {
        let mut manager = ContingentOrderManager::new();
        let (entry, stop, tp) = create_test_orders();
        let entry_id = entry.client_order_id.clone();

        let bracket = OrderList::bracket(entry, stop, tp);
        let list_id = bracket.id.clone();

        manager.register_list(bracket);
        assert_eq!(manager.active_list_count(), 1);

        manager.unregister_list(&list_id);
        assert_eq!(manager.active_list_count(), 0);
        assert!(manager.get_list_for_order(&entry_id).is_none());
    }

    #[test]
    fn test_clear() {
        let mut manager = ContingentOrderManager::new();
        let (entry, stop, tp) = create_test_orders();

        manager.register_list(OrderList::bracket(entry, stop, tp));
        assert_eq!(manager.active_list_count(), 1);

        manager.clear();
        assert_eq!(manager.active_list_count(), 0);
    }

    #[test]
    fn test_non_contingent_order_no_action() {
        let mut manager = ContingentOrderManager::new();

        // Order not registered in any list
        let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(1.0))
            .build()
            .unwrap();
        let fill = create_fill_event(&order);

        let actions = manager.on_order_event(&OrderEventAny::Filled(fill));
        assert!(actions.is_empty());
    }
}
