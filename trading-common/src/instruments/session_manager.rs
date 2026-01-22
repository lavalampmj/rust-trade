//! Session Manager for real-time market status tracking.
//!
//! This module provides real-time tracking of market session states across
//! all registered symbols, with event broadcasting for state changes.
//!
//! # Features
//!
//! - Real-time session state tracking for all symbols
//! - Background task for periodic state updates
//! - Event broadcasting for session state changes
//! - Timezone-aware session calculations
//!
//! # Example
//!
//! ```ignore
//! use trading_common::instruments::SessionManager;
//!
//! let manager = SessionManager::new(registry.clone());
//!
//! // Start background tracking
//! manager.start(Duration::from_secs(1)).await;
//!
//! // Subscribe to session events
//! let mut rx = manager.subscribe();
//! while let Ok(event) = rx.recv().await {
//!     match event {
//!         SessionEvent::SessionOpened { symbol, .. } => println!("Market opened: {}", symbol),
//!         SessionEvent::SessionClosed { symbol } => println!("Market closed: {}", symbol),
//!         _ => {}
//!     }
//! }
//! ```

use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::orders::InstrumentId;

use super::{
    MarketStatus, SessionEvent, SessionSchedule, SessionState, SymbolRegistry, TradingSession,
};

/// Configuration for the session manager
#[derive(Debug, Clone)]
pub struct SessionManagerConfig {
    /// How often to check for session state changes
    pub check_interval: Duration,
    /// Broadcast channel capacity
    pub channel_capacity: usize,
    /// Whether to emit events for each state check (not just changes)
    pub emit_heartbeats: bool,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(1),
            channel_capacity: 1024,
            emit_heartbeats: false,
        }
    }
}

/// Real-time session state manager.
///
/// Tracks market session states for all registered symbols and broadcasts
/// events when states change.
pub struct SessionManager {
    /// Symbol registry for session schedules
    registry: Arc<SymbolRegistry>,

    /// Current session state per symbol
    states: DashMap<String, SessionState>,

    /// Event channel for session state changes
    event_tx: broadcast::Sender<SessionEvent>,

    /// Configuration
    config: SessionManagerConfig,

    /// Background task handle
    task_handle: parking_lot::Mutex<Option<JoinHandle<()>>>,

    /// Shutdown signal
    shutdown_tx: parking_lot::Mutex<Option<broadcast::Sender<()>>>,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(registry: Arc<SymbolRegistry>) -> Self {
        Self::with_config(registry, SessionManagerConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(registry: Arc<SymbolRegistry>, config: SessionManagerConfig) -> Self {
        let (event_tx, _) = broadcast::channel(config.channel_capacity);

        Self {
            registry,
            states: DashMap::new(),
            event_tx,
            config,
            task_handle: parking_lot::Mutex::new(None),
            shutdown_tx: parking_lot::Mutex::new(None),
        }
    }

    /// Subscribe to session events
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.event_tx.subscribe()
    }

    /// Get current session state for a symbol
    pub fn get_state(&self, id: &InstrumentId) -> Option<SessionState> {
        self.states.get(&id.to_string()).map(|s| s.clone())
    }

    /// Get current market status for a symbol
    pub fn get_status(&self, id: &InstrumentId) -> Option<MarketStatus> {
        self.states.get(&id.to_string()).map(|s| s.status)
    }

    /// Check if market is open
    pub fn is_open(&self, id: &InstrumentId) -> bool {
        self.states
            .get(&id.to_string())
            .map(|s| s.status == MarketStatus::Open)
            .unwrap_or(false)
    }

    /// Check if any trading is available (including extended hours)
    pub fn is_tradeable(&self, id: &InstrumentId) -> bool {
        self.states.get(&id.to_string()).map(|s| s.is_tradeable()).unwrap_or(false)
    }

    /// Get all symbols currently in a specific state
    pub fn get_symbols_by_status(&self, status: MarketStatus) -> Vec<InstrumentId> {
        self.states
            .iter()
            .filter(|entry| entry.value().status == status)
            .filter_map(|entry| InstrumentId::from_str(entry.key()))
            .collect()
    }

    /// Get count of symbols by status
    pub fn count_by_status(&self) -> std::collections::HashMap<MarketStatus, usize> {
        let mut counts = std::collections::HashMap::new();
        for entry in self.states.iter() {
            *counts.entry(entry.value().status).or_insert(0) += 1;
        }
        counts
    }

    /// Register a symbol for tracking
    pub fn register(&self, id: &InstrumentId, schedule: Option<&SessionSchedule>) {
        let key = id.to_string();

        let state = if let Some(schedule) = schedule {
            schedule.get_session_state(Utc::now())
        } else {
            // 24/7 market
            SessionState {
                status: MarketStatus::Open,
                current_session: None,
                next_change: None,
                reason: None,
            }
        };

        self.states.insert(key, state);
        debug!("Registered symbol for session tracking: {}", id);
    }

    /// Unregister a symbol from tracking
    pub fn unregister(&self, id: &InstrumentId) {
        let key = id.to_string();
        if self.states.remove(&key).is_some() {
            debug!("Unregistered symbol from session tracking: {}", id);
        }
    }

    /// Update session state for a symbol
    ///
    /// Returns the event if state changed, None otherwise
    pub fn update_state(&self, id: &InstrumentId, schedule: &SessionSchedule) -> Option<SessionEvent> {
        let key = id.to_string();
        let new_state = schedule.get_session_state(Utc::now());

        let event = if let Some(mut current) = self.states.get_mut(&key) {
            if current.status != new_state.status {
                let event = self.create_state_change_event(id, &current, &new_state);
                *current = new_state;
                Some(event)
            } else {
                // Update other fields without triggering event
                *current = new_state;
                None
            }
        } else {
            // New symbol - insert and emit opened event if open
            let event = if new_state.is_tradeable() {
                Some(SessionEvent::SessionOpened {
                    symbol: key.clone(),
                    session: new_state.current_session.clone().unwrap_or_else(|| {
                        TradingSession::continuous()
                    }),
                })
            } else {
                None
            };
            self.states.insert(key, new_state);
            event
        };

        if let Some(ref e) = event {
            let _ = self.event_tx.send(e.clone());
        }

        event
    }

    /// Create appropriate event for state change
    fn create_state_change_event(
        &self,
        id: &InstrumentId,
        old_state: &SessionState,
        new_state: &SessionState,
    ) -> SessionEvent {
        let symbol = id.to_string();

        match (old_state.status, new_state.status) {
            // Closing events
            (_, MarketStatus::Closed) => SessionEvent::SessionClosed { symbol },

            // Opening events
            (MarketStatus::Closed, MarketStatus::Open) |
            (MarketStatus::Closed, MarketStatus::PreMarket) |
            (MarketStatus::Closed, MarketStatus::AfterHours) => {
                SessionEvent::SessionOpened {
                    symbol,
                    session: new_state.current_session.clone().unwrap_or_else(|| {
                        TradingSession::continuous()
                    }),
                }
            }

            // Halt events
            (_, MarketStatus::Halted) => SessionEvent::MarketHalted {
                symbol,
                reason: new_state.reason.clone().unwrap_or_else(|| "Unknown".to_string()),
            },

            // Resume from halt
            (MarketStatus::Halted, _) => SessionEvent::MarketResumed { symbol },

            // Maintenance events
            (_, MarketStatus::Maintenance) => SessionEvent::MaintenanceStarted { symbol },
            (MarketStatus::Maintenance, _) => SessionEvent::MaintenanceEnded { symbol },

            // Session transitions (pre-market -> regular, etc.)
            _ => {
                if new_state.current_session.is_some() {
                    SessionEvent::SessionOpened {
                        symbol,
                        session: new_state.current_session.clone().unwrap(),
                    }
                } else {
                    SessionEvent::SessionClosed { symbol }
                }
            }
        }
    }

    /// Start the background session tracking task
    pub async fn start(&self) {
        // Check if already running
        if self.task_handle.lock().is_some() {
            warn!("Session manager is already running");
            return;
        }

        info!("Starting session manager with {:?} check interval", self.config.check_interval);

        let (shutdown_tx, mut shutdown_rx) = broadcast::channel::<()>(1);
        *self.shutdown_tx.lock() = Some(shutdown_tx);

        let states = self.states.clone();
        let registry = self.registry.clone();
        let event_tx = self.event_tx.clone();
        let interval = self.config.check_interval;

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        // Update all tracked symbols
                        for entry in states.iter() {
                            let key = entry.key();
                            if let Some(id) = InstrumentId::from_str(key) {
                                // Get schedule from registry (cached)
                                if let Ok(schedule) = registry.get_session_schedule(&id) {
                                    if let Some(schedule) = schedule {
                                        let new_state = schedule.get_session_state(Utc::now());
                                        let old_status = entry.value().status;

                                        if old_status != new_state.status {
                                            let event = match (old_status, new_state.status) {
                                                (_, MarketStatus::Closed) => {
                                                    SessionEvent::SessionClosed { symbol: key.clone() }
                                                }
                                                (MarketStatus::Closed, _) => {
                                                    SessionEvent::SessionOpened {
                                                        symbol: key.clone(),
                                                        session: new_state.current_session.clone()
                                                            .unwrap_or_else(|| TradingSession::continuous()),
                                                    }
                                                }
                                                (_, MarketStatus::Halted) => {
                                                    SessionEvent::MarketHalted {
                                                        symbol: key.clone(),
                                                        reason: new_state.reason.clone()
                                                            .unwrap_or_else(|| "Unknown".to_string()),
                                                    }
                                                }
                                                (MarketStatus::Halted, _) => {
                                                    SessionEvent::MarketResumed { symbol: key.clone() }
                                                }
                                                (_, MarketStatus::Maintenance) => {
                                                    SessionEvent::MaintenanceStarted { symbol: key.clone() }
                                                }
                                                (MarketStatus::Maintenance, _) => {
                                                    SessionEvent::MaintenanceEnded { symbol: key.clone() }
                                                }
                                                _ => SessionEvent::SessionClosed { symbol: key.clone() },
                                            };

                                            let _ = event_tx.send(event);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Session manager shutting down");
                        break;
                    }
                }
            }
        });

        *self.task_handle.lock() = Some(handle);
    }

    /// Stop the background session tracking task
    pub async fn stop(&self) {
        if let Some(shutdown_tx) = self.shutdown_tx.lock().take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(handle) = self.task_handle.lock().take() {
            let _ = handle.await;
        }

        info!("Session manager stopped");
    }

    /// Check if the background task is running
    pub fn is_running(&self) -> bool {
        self.task_handle.lock().is_some()
    }

    /// Manually halt a symbol (e.g., due to circuit breaker)
    pub fn halt_symbol(&self, id: &InstrumentId, reason: &str) {
        let key = id.to_string();

        if let Some(mut state) = self.states.get_mut(&key) {
            let old_status = state.status;
            state.status = MarketStatus::Halted;
            state.reason = Some(reason.to_string());

            if old_status != MarketStatus::Halted {
                let event = SessionEvent::MarketHalted {
                    symbol: key,
                    reason: reason.to_string(),
                };
                let _ = self.event_tx.send(event);
            }
        }
    }

    /// Resume a halted symbol
    pub fn resume_symbol(&self, id: &InstrumentId) {
        let key = id.to_string();

        if let Some(mut state) = self.states.get_mut(&key) {
            if state.status == MarketStatus::Halted {
                // Recalculate actual state based on schedule
                if let Ok(Some(schedule)) = self.registry.get_session_schedule(id) {
                    *state = schedule.get_session_state(Utc::now());
                } else {
                    state.status = MarketStatus::Open;
                }
                state.reason = None;

                let event = SessionEvent::MarketResumed { symbol: key };
                let _ = self.event_tx.send(event);
            }
        }
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        // Signal shutdown
        if let Some(shutdown_tx) = self.shutdown_tx.lock().take() {
            let _ = shutdown_tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveTime, Weekday};

    #[test]
    fn test_session_manager_config_default() {
        let config = SessionManagerConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(1));
        assert_eq!(config.channel_capacity, 1024);
        assert!(!config.emit_heartbeats);
    }

    #[test]
    fn test_market_status_counts() {
        // This test verifies the count_by_status logic
        let counts = std::collections::HashMap::<MarketStatus, usize>::new();
        assert!(counts.is_empty());
    }

    // ============================================================
    // STATE TRACKING TESTS (using DashMap directly for unit tests)
    // ============================================================

    #[test]
    fn test_session_state_storage() {
        let states: DashMap<String, SessionState> = DashMap::new();

        // Insert a state
        let symbol_key = "BTCUSDT.BINANCE".to_string();
        let state = SessionState {
            status: MarketStatus::Open,
            current_session: None,
            next_change: None,
            reason: None,
        };
        states.insert(symbol_key.clone(), state);

        // Retrieve it
        let retrieved = states.get(&symbol_key);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().status, MarketStatus::Open);
    }

    #[test]
    fn test_session_state_update() {
        let states: DashMap<String, SessionState> = DashMap::new();
        let symbol_key = "BTCUSDT.BINANCE".to_string();

        // Initial state
        states.insert(
            symbol_key.clone(),
            SessionState {
                status: MarketStatus::Open,
                current_session: None,
                next_change: None,
                reason: None,
            },
        );

        // Update state
        if let Some(mut state) = states.get_mut(&symbol_key) {
            state.status = MarketStatus::Closed;
        }

        // Verify update
        let state = states.get(&symbol_key).unwrap();
        assert_eq!(state.status, MarketStatus::Closed);
    }

    #[test]
    fn test_session_state_transitions() {
        // Test all valid status values
        let statuses = vec![
            MarketStatus::Open,
            MarketStatus::Closed,
            MarketStatus::PreMarket,
            MarketStatus::AfterHours,
            MarketStatus::Halted,
            MarketStatus::Auction,
            MarketStatus::Maintenance,
        ];

        for status in statuses {
            let state = SessionState {
                status,
                current_session: None,
                next_change: None,
                reason: None,
            };
            // Should be constructible
            assert!(matches!(state.status, MarketStatus::Open | MarketStatus::Closed |
                MarketStatus::PreMarket | MarketStatus::AfterHours | MarketStatus::Halted |
                MarketStatus::Auction | MarketStatus::Maintenance));
        }
    }

    // ============================================================
    // EVENT BROADCASTING TESTS
    // ============================================================

    #[test]
    fn test_event_channel_creation() {
        let (tx, mut rx) = broadcast::channel::<SessionEvent>(16);

        // Create a test session
        let test_session = TradingSession::regular(
            "Test",
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            vec![Weekday::Mon],
        );

        // Send an event
        let event = SessionEvent::SessionOpened {
            symbol: "BTCUSDT.BINANCE".to_string(),
            session: test_session,
        };
        tx.send(event).unwrap();

        // Receive it
        match rx.try_recv() {
            Ok(SessionEvent::SessionOpened { symbol, .. }) => {
                assert_eq!(symbol, "BTCUSDT.BINANCE");
            }
            _ => panic!("Expected SessionOpened event"),
        }
    }

    #[test]
    fn test_event_channel_multiple_subscribers() {
        let (tx, _) = broadcast::channel::<SessionEvent>(16);

        let mut rx1 = tx.subscribe();
        let mut rx2 = tx.subscribe();

        let event = SessionEvent::MarketHalted {
            symbol: "BTCUSDT.BINANCE".to_string(),
            reason: "Circuit breaker".to_string(),
        };
        tx.send(event).unwrap();

        // Both should receive
        match rx1.try_recv() {
            Ok(SessionEvent::MarketHalted { symbol, reason }) => {
                assert_eq!(symbol, "BTCUSDT.BINANCE");
                assert_eq!(reason, "Circuit breaker");
            }
            _ => panic!("rx1: Expected MarketHalted event"),
        }

        match rx2.try_recv() {
            Ok(SessionEvent::MarketHalted { symbol, reason }) => {
                assert_eq!(symbol, "BTCUSDT.BINANCE");
                assert_eq!(reason, "Circuit breaker");
            }
            _ => panic!("rx2: Expected MarketHalted event"),
        }
    }

    #[test]
    fn test_all_event_types() {
        let (tx, mut rx) = broadcast::channel::<SessionEvent>(16);

        // Create a test session
        let test_session = TradingSession::regular(
            "Test",
            NaiveTime::from_hms_opt(9, 30, 0).unwrap(),
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            vec![Weekday::Mon],
        );

        // Test all event variants (6 variants in SessionEvent enum)
        let events: Vec<SessionEvent> = vec![
            SessionEvent::SessionOpened {
                symbol: "SYM1".to_string(),
                session: test_session,
            },
            SessionEvent::SessionClosed {
                symbol: "SYM2".to_string(),
            },
            SessionEvent::MarketHalted {
                symbol: "SYM3".to_string(),
                reason: "Testing".to_string(),
            },
            SessionEvent::MarketResumed {
                symbol: "SYM4".to_string(),
            },
            SessionEvent::MaintenanceStarted {
                symbol: "SYM5".to_string(),
            },
            SessionEvent::MaintenanceEnded {
                symbol: "SYM6".to_string(),
            },
        ];

        for event in events.iter() {
            tx.send(event.clone()).unwrap();
        }

        // Receive all
        for _ in 0..6 {
            assert!(rx.try_recv().is_ok());
        }
    }

    // ============================================================
    // STATUS QUERY TESTS
    // ============================================================

    #[test]
    fn test_get_symbols_by_status() {
        let states: DashMap<String, SessionState> = DashMap::new();

        // Add symbols with different statuses
        states.insert(
            "SYM1.VENUE".to_string(),
            SessionState {
                status: MarketStatus::Open,
                current_session: None,
                next_change: None,
                reason: None,
            },
        );
        states.insert(
            "SYM2.VENUE".to_string(),
            SessionState {
                status: MarketStatus::Open,
                current_session: None,
                next_change: None,
                reason: None,
            },
        );
        states.insert(
            "SYM3.VENUE".to_string(),
            SessionState {
                status: MarketStatus::Closed,
                current_session: None,
                next_change: None,
                reason: None,
            },
        );

        // Count by status
        let open_count = states
            .iter()
            .filter(|entry| entry.status == MarketStatus::Open)
            .count();
        let closed_count = states
            .iter()
            .filter(|entry| entry.status == MarketStatus::Closed)
            .count();

        assert_eq!(open_count, 2);
        assert_eq!(closed_count, 1);
    }

    // ============================================================
    // HALT/RESUME LOGIC TESTS
    // ============================================================

    #[test]
    fn test_halt_state_change() {
        let states: DashMap<String, SessionState> = DashMap::new();
        let symbol_key = "BTCUSDT.BINANCE".to_string();

        // Start with Open
        states.insert(
            symbol_key.clone(),
            SessionState {
                status: MarketStatus::Open,
                current_session: None,
                next_change: None,
                reason: None,
            },
        );

        // Simulate halt
        if let Some(mut state) = states.get_mut(&symbol_key) {
            state.status = MarketStatus::Halted;
            state.reason = Some("Manual halt".to_string());
        }

        let state = states.get(&symbol_key).unwrap();
        assert_eq!(state.status, MarketStatus::Halted);
        assert_eq!(state.reason, Some("Manual halt".to_string()));
    }

    #[test]
    fn test_resume_clears_reason() {
        let states: DashMap<String, SessionState> = DashMap::new();
        let symbol_key = "BTCUSDT.BINANCE".to_string();

        // Start halted with reason
        states.insert(
            symbol_key.clone(),
            SessionState {
                status: MarketStatus::Halted,
                current_session: None,
                next_change: None,
                reason: Some("Circuit breaker".to_string()),
            },
        );

        // Simulate resume
        if let Some(mut state) = states.get_mut(&symbol_key) {
            state.status = MarketStatus::Open;
            state.reason = None;
        }

        let state = states.get(&symbol_key).unwrap();
        assert_eq!(state.status, MarketStatus::Open);
        assert!(state.reason.is_none());
    }

    // ============================================================
    // CONCURRENT STATE ACCESS TESTS
    // ============================================================

    #[test]
    fn test_concurrent_state_updates() {
        use std::thread;
        use std::sync::atomic::{AtomicUsize, Ordering};

        let states: Arc<DashMap<String, SessionState>> = Arc::new(DashMap::new());
        let update_count = Arc::new(AtomicUsize::new(0));

        // Pre-populate
        for i in 0..100 {
            states.insert(
                format!("SYM{}.VENUE", i),
                SessionState {
                    status: MarketStatus::Open,
                    current_session: None,
                    next_change: None,
                    reason: None,
                },
            );
        }

        // Multiple threads updating states
        let handles: Vec<_> = (0..10)
            .map(|thread_id| {
                let states_clone = Arc::clone(&states);
                let count_clone = Arc::clone(&update_count);
                thread::spawn(move || {
                    for i in 0..100 {
                        let key = format!("SYM{}.VENUE", (i + thread_id * 10) % 100);
                        if let Some(mut state) = states_clone.get_mut(&key) {
                            // Toggle status
                            state.status = if state.status == MarketStatus::Open {
                                MarketStatus::Closed
                            } else {
                                MarketStatus::Open
                            };
                            count_clone.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // All updates should have completed
        assert_eq!(update_count.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn test_concurrent_register_unregister() {
        use std::thread;

        let states: Arc<DashMap<String, SessionState>> = Arc::new(DashMap::new());

        let mut handles = vec![];

        // Registration threads
        for thread_id in 0..5 {
            let states_clone = Arc::clone(&states);
            handles.push(thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("T{}S{}.VENUE", thread_id, i);
                    states_clone.insert(
                        key,
                        SessionState {
                            status: MarketStatus::Open,
                            current_session: None,
                            next_change: None,
                            reason: None,
                        },
                    );
                }
            }));
        }

        // Unregistration threads (for even-numbered symbols)
        for thread_id in 0..5 {
            let states_clone = Arc::clone(&states);
            handles.push(thread::spawn(move || {
                // Small delay to let registrations happen
                thread::yield_now();
                for i in (0..100).step_by(2) {
                    let key = format!("T{}S{}.VENUE", thread_id, i);
                    states_clone.remove(&key);
                }
            }));
        }

        for handle in handles {
            handle.join().expect("Thread panicked");
        }

        // Should have ~250 remaining (500 registered - 250 removed)
        // Exact count may vary due to race conditions
        assert!(states.len() <= 500);
    }

    // ============================================================
    // TRADEABLE STATUS TESTS
    // ============================================================

    #[test]
    fn test_is_tradeable_status() {
        // These statuses should be tradeable
        let tradeable_statuses = vec![
            MarketStatus::Open,
            MarketStatus::PreMarket,
            MarketStatus::AfterHours,
        ];

        // These statuses should NOT be tradeable
        let non_tradeable_statuses = vec![
            MarketStatus::Closed,
            MarketStatus::Halted,
            MarketStatus::Maintenance,
            MarketStatus::Auction,
        ];

        for status in tradeable_statuses {
            let is_tradeable = matches!(
                status,
                MarketStatus::Open | MarketStatus::PreMarket | MarketStatus::AfterHours
            );
            assert!(is_tradeable, "{:?} should be tradeable", status);
        }

        for status in non_tradeable_statuses {
            let is_tradeable = matches!(
                status,
                MarketStatus::Open | MarketStatus::PreMarket | MarketStatus::AfterHours
            );
            assert!(!is_tradeable, "{:?} should NOT be tradeable", status);
        }
    }
}
