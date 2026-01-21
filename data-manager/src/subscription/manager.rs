//! Subscription manager
//!
//! Manages consumer subscriptions and data distribution.

use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info};

use crate::symbol::SymbolSpec;

/// Subscription request from a consumer
#[derive(Debug, Clone)]
pub struct SubscriptionRequest {
    /// Consumer identifier
    pub consumer_id: String,
    /// Symbols to subscribe to
    pub symbols: Vec<SymbolSpec>,
    /// Optional backfill request
    pub backfill: Option<BackfillRequest>,
}

impl SubscriptionRequest {
    pub fn new(consumer_id: String, symbols: Vec<SymbolSpec>) -> Self {
        Self {
            consumer_id,
            symbols,
            backfill: None,
        }
    }

    pub fn with_backfill(mut self, backfill: BackfillRequest) -> Self {
        self.backfill = Some(backfill);
        self
    }
}

/// Backfill request for historical context on subscribe
#[derive(Debug, Clone)]
pub struct BackfillRequest {
    /// Duration of historical data to backfill
    pub duration: Duration,
    /// Maximum number of records to backfill
    pub max_records: usize,
}

impl BackfillRequest {
    pub fn new(duration: Duration, max_records: usize) -> Self {
        Self {
            duration,
            max_records,
        }
    }

    /// Create a backfill request for the last N minutes
    pub fn last_minutes(minutes: i64, max_records: usize) -> Self {
        Self::new(Duration::minutes(minutes), max_records)
    }
}

impl Default for BackfillRequest {
    fn default() -> Self {
        Self::new(Duration::minutes(5), 10000)
    }
}

/// Consumer subscription state
#[derive(Debug, Clone)]
pub struct ConsumerSubscription {
    /// Consumer identifier
    pub consumer_id: String,
    /// Subscribed symbols
    pub symbols: HashSet<SymbolSpec>,
    /// When the subscription was created
    pub created_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Number of messages sent
    pub messages_sent: u64,
}

impl ConsumerSubscription {
    pub fn new(consumer_id: String, symbols: impl IntoIterator<Item = SymbolSpec>) -> Self {
        let now = Utc::now();
        Self {
            consumer_id,
            symbols: symbols.into_iter().collect(),
            created_at: now,
            last_activity: now,
            messages_sent: 0,
        }
    }

    pub fn add_symbols(&mut self, symbols: impl IntoIterator<Item = SymbolSpec>) {
        self.symbols.extend(symbols);
        self.last_activity = Utc::now();
    }

    pub fn remove_symbols(&mut self, symbols: &[SymbolSpec]) {
        for symbol in symbols {
            self.symbols.remove(symbol);
        }
        self.last_activity = Utc::now();
    }

    pub fn is_subscribed(&self, symbol: &SymbolSpec) -> bool {
        self.symbols.contains(symbol)
    }
}

/// Subscription manager
pub struct SubscriptionManager {
    /// Active subscriptions by consumer ID
    subscriptions: Arc<RwLock<HashMap<String, ConsumerSubscription>>>,
    /// Symbol to consumer mapping for fast routing
    symbol_consumers: Arc<RwLock<HashMap<SymbolSpec, HashSet<String>>>>,
}

impl SubscriptionManager {
    /// Create a new subscription manager
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            symbol_consumers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe a consumer to symbols
    pub fn subscribe(&self, request: SubscriptionRequest) -> SubscriptionResult {
        let mut subscriptions = self.subscriptions.write();
        let mut symbol_consumers = self.symbol_consumers.write();

        let consumer_id = request.consumer_id.clone();

        // Update or create subscription
        let subscription = subscriptions
            .entry(consumer_id.clone())
            .or_insert_with(|| ConsumerSubscription::new(consumer_id.clone(), vec![]));

        let mut added_symbols = Vec::new();
        for symbol in &request.symbols {
            if subscription.symbols.insert(symbol.clone()) {
                added_symbols.push(symbol.clone());
                symbol_consumers
                    .entry(symbol.clone())
                    .or_insert_with(HashSet::new)
                    .insert(consumer_id.clone());
            }
        }

        info!(
            "Consumer {} subscribed to {} symbols ({} new)",
            consumer_id,
            request.symbols.len(),
            added_symbols.len()
        );

        SubscriptionResult {
            consumer_id,
            symbols_added: added_symbols,
            total_subscribed: subscription.symbols.len(),
        }
    }

    /// Unsubscribe a consumer from symbols
    pub fn unsubscribe(&self, consumer_id: &str, symbols: &[SymbolSpec]) -> bool {
        let mut subscriptions = self.subscriptions.write();
        let mut symbol_consumers = self.symbol_consumers.write();

        if let Some(subscription) = subscriptions.get_mut(consumer_id) {
            for symbol in symbols {
                subscription.symbols.remove(symbol);
                if let Some(consumers) = symbol_consumers.get_mut(symbol) {
                    consumers.remove(consumer_id);
                    if consumers.is_empty() {
                        symbol_consumers.remove(symbol);
                    }
                }
            }
            debug!("Consumer {} unsubscribed from {} symbols", consumer_id, symbols.len());
            true
        } else {
            false
        }
    }

    /// Unsubscribe a consumer from all symbols
    pub fn unsubscribe_all(&self, consumer_id: &str) -> bool {
        let mut subscriptions = self.subscriptions.write();
        let mut symbol_consumers = self.symbol_consumers.write();

        if let Some(subscription) = subscriptions.remove(consumer_id) {
            for symbol in &subscription.symbols {
                if let Some(consumers) = symbol_consumers.get_mut(symbol) {
                    consumers.remove(consumer_id);
                    if consumers.is_empty() {
                        symbol_consumers.remove(symbol);
                    }
                }
            }
            info!("Consumer {} unsubscribed from all symbols", consumer_id);
            true
        } else {
            false
        }
    }

    /// Get consumers subscribed to a symbol
    pub fn get_consumers_for_symbol(&self, symbol: &SymbolSpec) -> Vec<String> {
        self.symbol_consumers
            .read()
            .get(symbol)
            .map(|c| c.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get subscription for a consumer
    pub fn get_subscription(&self, consumer_id: &str) -> Option<ConsumerSubscription> {
        self.subscriptions.read().get(consumer_id).cloned()
    }

    /// Get all active subscriptions
    pub fn all_subscriptions(&self) -> Vec<ConsumerSubscription> {
        self.subscriptions.read().values().cloned().collect()
    }

    /// Get total number of subscriptions
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.read().len()
    }

    /// Get all subscribed symbols (union of all consumers)
    pub fn all_subscribed_symbols(&self) -> Vec<SymbolSpec> {
        self.symbol_consumers.read().keys().cloned().collect()
    }

    /// Check if a symbol has any subscribers
    pub fn has_subscribers(&self, symbol: &SymbolSpec) -> bool {
        self.symbol_consumers
            .read()
            .get(symbol)
            .map(|c| !c.is_empty())
            .unwrap_or(false)
    }

    /// Record that a message was sent to a consumer
    pub fn record_message_sent(&self, consumer_id: &str) {
        if let Some(subscription) = self.subscriptions.write().get_mut(consumer_id) {
            subscription.messages_sent += 1;
            subscription.last_activity = Utc::now();
        }
    }

    /// Get statistics
    pub fn stats(&self) -> SubscriptionStats {
        let subscriptions = self.subscriptions.read();
        let symbol_consumers = self.symbol_consumers.read();

        SubscriptionStats {
            total_consumers: subscriptions.len(),
            total_unique_symbols: symbol_consumers.len(),
            total_subscriptions: subscriptions.values().map(|s| s.symbols.len()).sum(),
            messages_sent: subscriptions.values().map(|s| s.messages_sent).sum(),
        }
    }
}

impl Default for SubscriptionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a subscription request
#[derive(Debug, Clone)]
pub struct SubscriptionResult {
    pub consumer_id: String,
    pub symbols_added: Vec<SymbolSpec>,
    pub total_subscribed: usize,
}

/// Subscription statistics
#[derive(Debug, Clone)]
pub struct SubscriptionStats {
    pub total_consumers: usize,
    pub total_unique_symbols: usize,
    pub total_subscriptions: usize,
    pub messages_sent: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscribe() {
        let manager = SubscriptionManager::new();

        let request = SubscriptionRequest::new(
            "consumer1".to_string(),
            vec![
                SymbolSpec::new("ES", "CME"),
                SymbolSpec::new("NQ", "CME"),
            ],
        );

        let result = manager.subscribe(request);
        assert_eq!(result.symbols_added.len(), 2);
        assert_eq!(result.total_subscribed, 2);

        // Subscribe to same symbols again - should not add duplicates
        let request2 = SubscriptionRequest::new(
            "consumer1".to_string(),
            vec![SymbolSpec::new("ES", "CME")],
        );
        let result2 = manager.subscribe(request2);
        assert_eq!(result2.symbols_added.len(), 0);
        assert_eq!(result2.total_subscribed, 2);
    }

    #[test]
    fn test_unsubscribe() {
        let manager = SubscriptionManager::new();

        manager.subscribe(SubscriptionRequest::new(
            "consumer1".to_string(),
            vec![
                SymbolSpec::new("ES", "CME"),
                SymbolSpec::new("NQ", "CME"),
            ],
        ));

        assert!(manager.unsubscribe("consumer1", &[SymbolSpec::new("ES", "CME")]));

        let subscription = manager.get_subscription("consumer1").unwrap();
        assert_eq!(subscription.symbols.len(), 1);
        assert!(!subscription.is_subscribed(&SymbolSpec::new("ES", "CME")));
        assert!(subscription.is_subscribed(&SymbolSpec::new("NQ", "CME")));
    }

    #[test]
    fn test_multiple_consumers() {
        let manager = SubscriptionManager::new();

        manager.subscribe(SubscriptionRequest::new(
            "consumer1".to_string(),
            vec![SymbolSpec::new("ES", "CME")],
        ));

        manager.subscribe(SubscriptionRequest::new(
            "consumer2".to_string(),
            vec![SymbolSpec::new("ES", "CME")],
        ));

        let consumers = manager.get_consumers_for_symbol(&SymbolSpec::new("ES", "CME"));
        assert_eq!(consumers.len(), 2);

        let stats = manager.stats();
        assert_eq!(stats.total_consumers, 2);
        assert_eq!(stats.total_unique_symbols, 1);
        assert_eq!(stats.total_subscriptions, 2);
    }
}
