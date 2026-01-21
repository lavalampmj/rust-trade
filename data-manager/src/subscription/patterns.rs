//! Subscription patterns
//!
//! Defines different patterns for data subscription and delivery.

use serde::{Deserialize, Serialize};

use crate::schema::NormalizedTick;

/// Subscription pattern determining how data is delivered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubscriptionPattern {
    /// Topic-based: receive all data for subscribed symbols
    Topic,
    /// Filtered: receive only data matching filter criteria
    Filtered(super::SubscriptionFilter),
    /// Snapshot + Delta: receive initial snapshot then only changes
    SnapshotDelta(SnapshotDeltaConfig),
    /// Throttled: receive data at most every N milliseconds
    Throttled(ThrottleConfig),
    /// Sampled: receive every Nth tick
    Sampled(SampleConfig),
}

impl Default for SubscriptionPattern {
    fn default() -> Self {
        Self::Topic
    }
}

/// Configuration for snapshot + delta delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDeltaConfig {
    /// Include initial snapshot on subscribe
    pub include_snapshot: bool,
    /// Snapshot depth (number of historical ticks)
    pub snapshot_depth: usize,
    /// Only send deltas when price changes
    pub price_change_only: bool,
}

impl Default for SnapshotDeltaConfig {
    fn default() -> Self {
        Self {
            include_snapshot: true,
            snapshot_depth: 100,
            price_change_only: false,
        }
    }
}

/// Configuration for throttled delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleConfig {
    /// Minimum interval between deliveries in milliseconds
    pub interval_ms: u64,
    /// How to aggregate multiple ticks
    pub aggregation: ThrottleAggregation,
}

impl ThrottleConfig {
    pub fn new(interval_ms: u64) -> Self {
        Self {
            interval_ms,
            aggregation: ThrottleAggregation::Latest,
        }
    }
}

/// Aggregation strategy for throttled delivery
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ThrottleAggregation {
    /// Send only the latest tick
    Latest,
    /// Send the first tick
    First,
    /// Send all accumulated ticks as a batch
    Batch,
}

/// Configuration for sampled delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleConfig {
    /// Sample every Nth tick
    pub sample_rate: u32,
    /// Sample strategy
    pub strategy: SampleStrategy,
}

impl SampleConfig {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            strategy: SampleStrategy::Every,
        }
    }
}

/// Sampling strategy
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SampleStrategy {
    /// Sample every Nth tick
    Every,
    /// Random sampling with probability 1/N
    Random,
    /// Sample based on price change threshold
    PriceChange,
}

/// Delivery quality of service
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeliveryQoS {
    /// At most once (fire and forget)
    AtMostOnce,
    /// At least once (with acknowledgment)
    AtLeastOnce,
    /// Exactly once (with deduplication)
    ExactlyOnce,
}

impl Default for DeliveryQoS {
    fn default() -> Self {
        Self::AtMostOnce
    }
}

/// Message delivery wrapper
#[derive(Debug, Clone)]
pub struct DeliveryMessage {
    /// Sequence number for ordering/deduplication
    pub sequence: u64,
    /// The tick data
    pub tick: NormalizedTick,
    /// Whether this is part of a snapshot
    pub is_snapshot: bool,
    /// Timestamp when queued for delivery
    pub queued_at: chrono::DateTime<chrono::Utc>,
}

impl DeliveryMessage {
    pub fn new(sequence: u64, tick: NormalizedTick) -> Self {
        Self {
            sequence,
            tick,
            is_snapshot: false,
            queued_at: chrono::Utc::now(),
        }
    }

    pub fn snapshot(sequence: u64, tick: NormalizedTick) -> Self {
        Self {
            sequence,
            tick,
            is_snapshot: true,
            queued_at: chrono::Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_throttle_config() {
        let config = ThrottleConfig::new(100);
        assert_eq!(config.interval_ms, 100);
        assert!(matches!(config.aggregation, ThrottleAggregation::Latest));
    }

    #[test]
    fn test_sample_config() {
        let config = SampleConfig::new(10);
        assert_eq!(config.sample_rate, 10);
        assert!(matches!(config.strategy, SampleStrategy::Every));
    }

    #[test]
    fn test_delivery_qos_default() {
        let qos = DeliveryQoS::default();
        assert_eq!(qos, DeliveryQoS::AtMostOnce);
    }
}
