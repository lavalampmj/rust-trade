//! Test data generator for the integration test harness
//!
//! This module generates deterministic test data in DBN format (dbn::TradeMsg)
//! that can be replayed through the data pipeline for stress testing.

use chrono::{DateTime, Duration, Utc};
use dbn::record::TradeMsg;
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;
use rust_decimal::Decimal;
use std::collections::HashMap;

use trading_common::data::dbn_types::{create_trade_msg_from_decimals, TradeSideCompat};

use crate::config::{DataGenConfig, VolumeProfile};

/// Metadata about the generated test data
#[derive(Debug, Clone)]
pub struct TestDataMetadata {
    /// List of symbols in the test data
    pub symbols: Vec<String>,
    /// Exchange name
    pub exchange: String,
    /// Start time of the generated data window
    pub start_time: DateTime<Utc>,
    /// End time of the generated data window
    pub end_time: DateTime<Utc>,
    /// Total tick count
    pub total_ticks: u64,
    /// Random seed used
    pub seed: u64,
    /// Volume profile used
    pub profile: VolumeProfile,
}

/// Bundle of generated test data with metadata
#[derive(Debug, Clone)]
pub struct TestDataBundle {
    /// Generated ticks sorted by timestamp
    pub ticks: Vec<TradeMsg>,
    /// Metadata about the generation
    pub metadata: TestDataMetadata,
    /// Per-symbol tick counts
    pub symbol_counts: HashMap<String, u64>,
}

impl TestDataBundle {
    /// Get the total number of ticks
    pub fn total_ticks(&self) -> usize {
        self.ticks.len()
    }

    /// Get the time range of the data
    pub fn time_range(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        (self.metadata.start_time, self.metadata.end_time)
    }
}

/// State for generating ticks for a single symbol
struct SymbolState {
    /// Current price (random walk)
    current_price: Decimal,
    /// Current sequence number (monotonically increasing)
    sequence: u32,
    /// Current size counter (monotonically increasing per spec)
    size_counter: u32,
    /// Alternating buy/sell
    is_buy: bool,
}

impl SymbolState {
    fn new(base_price: Decimal) -> Self {
        Self {
            current_price: base_price,
            sequence: 0,
            size_counter: 1,
            is_buy: true,
        }
    }

    /// Update price with random walk
    fn update_price(&mut self, rng: &mut ChaCha8Rng) {
        // Random walk: +/- 0.01% to 0.1%
        let change_pct = rng.gen_range(-0.001..0.001);
        let change =
            self.current_price * Decimal::from_f64_retain(change_pct).unwrap_or(Decimal::ZERO);
        self.current_price += change;
        // Ensure price stays positive
        if self.current_price <= Decimal::ZERO {
            self.current_price = Decimal::from(1);
        }
    }

    /// Generate next tick
    fn next_tick(
        &mut self,
        ts_nanos: u64,
        symbol: &str,
        exchange: &str,
        rng: &mut ChaCha8Rng,
    ) -> TradeMsg {
        // Update price
        self.update_price(rng);

        // Increment counters
        self.sequence += 1;
        self.size_counter += 1;

        // Alternate side
        let side = if self.is_buy {
            TradeSideCompat::Buy
        } else {
            TradeSideCompat::Sell
        };
        self.is_buy = !self.is_buy;

        // Create the TradeMsg
        create_trade_msg_from_decimals(
            ts_nanos,
            ts_nanos,
            symbol,
            exchange,
            self.current_price,
            Decimal::from(self.size_counter),
            side,
            self.sequence,
        )
    }
}

/// Test data generator that produces deterministic DBN-format ticks
pub struct TestDataGenerator {
    config: DataGenConfig,
    rng: ChaCha8Rng,
}

impl TestDataGenerator {
    /// Create a new generator with the given configuration
    pub fn new(config: DataGenConfig) -> Self {
        let rng = ChaCha8Rng::seed_from_u64(config.seed);
        Self { config, rng }
    }

    /// Generate the test data bundle
    pub fn generate(&mut self) -> TestDataBundle {
        let symbols = self.config.generate_symbol_names();
        let start_time = Utc::now();
        let end_time = start_time + Duration::seconds(self.config.time_window_secs as i64);

        // Initialize per-symbol state
        let base_price =
            Decimal::from_f64_retain(self.config.base_price).unwrap_or(Decimal::from(50000));
        let mut symbol_states: HashMap<String, SymbolState> = symbols
            .iter()
            .map(|s| (s.clone(), SymbolState::new(base_price)))
            .collect();

        // Generate timestamps based on volume profile
        let timestamps = self.generate_timestamps(start_time, &symbols);

        // Generate ticks
        let mut ticks: Vec<TradeMsg> = Vec::with_capacity(timestamps.len());
        let mut symbol_counts: HashMap<String, u64> =
            symbols.iter().map(|s| (s.clone(), 0)).collect();

        for (ts_nanos, symbol) in timestamps {
            let state = symbol_states.get_mut(&symbol).unwrap();
            let tick = state.next_tick(ts_nanos, &symbol, &self.config.exchange, &mut self.rng);
            ticks.push(tick);
            *symbol_counts.get_mut(&symbol).unwrap() += 1;
        }

        // Sort by timestamp (should already be mostly sorted, but ensure it)
        ticks.sort_by_key(|t| t.hd.ts_event);

        let metadata = TestDataMetadata {
            symbols,
            exchange: self.config.exchange.clone(),
            start_time,
            end_time,
            total_ticks: ticks.len() as u64,
            seed: self.config.seed,
            profile: self.config.profile,
        };

        TestDataBundle {
            ticks,
            metadata,
            symbol_counts,
        }
    }

    /// Generate timestamps based on volume profile
    fn generate_timestamps(
        &mut self,
        start_time: DateTime<Utc>,
        symbols: &[String],
    ) -> Vec<(u64, String)> {
        let start_nanos = start_time.timestamp_nanos_opt().unwrap() as u64;
        let window_nanos = self.config.time_window_secs * 1_000_000_000;

        match self.config.profile {
            VolumeProfile::Heavy => {
                self.generate_heavy_timestamps(start_nanos, window_nanos, symbols)
            }
            VolumeProfile::Normal => {
                self.generate_normal_timestamps(start_nanos, window_nanos, symbols)
            }
            VolumeProfile::Lite => {
                self.generate_lite_timestamps(start_nanos, window_nanos, symbols)
            }
        }
    }

    /// Heavy profile: ~1000 ticks/sec/symbol with clustered bursts
    fn generate_heavy_timestamps(
        &mut self,
        start_nanos: u64,
        window_nanos: u64,
        symbols: &[String],
    ) -> Vec<(u64, String)> {
        let ticks_per_symbol = 1000 * (window_nanos / 1_000_000_000);
        let mut timestamps = Vec::new();

        for symbol in symbols {
            let mut current_time = start_nanos;
            let mut remaining_ticks = ticks_per_symbol;

            while remaining_ticks > 0 && current_time < start_nanos + window_nanos {
                // Generate a burst of 10-50 ticks
                let burst_size = self.rng.gen_range(10..=50).min(remaining_ticks as usize);
                let burst_duration_nanos = self.rng.gen_range(100_000..500_000); // 100-500μs

                for i in 0..burst_size {
                    let offset = (i as u64 * burst_duration_nanos) / burst_size as u64;
                    let ts = current_time + offset;
                    if ts < start_nanos + window_nanos {
                        timestamps.push((ts, symbol.clone()));
                    }
                }

                remaining_ticks = remaining_ticks.saturating_sub(burst_size as u64);

                // Gap between bursts: 500μs to 2ms
                current_time += burst_duration_nanos + self.rng.gen_range(500_000..2_000_000);
            }
        }

        timestamps.sort_by_key(|(ts, _)| *ts);
        timestamps
    }

    /// Normal profile: ~250 ticks/sec/symbol with Poisson-like distribution
    fn generate_normal_timestamps(
        &mut self,
        start_nanos: u64,
        window_nanos: u64,
        symbols: &[String],
    ) -> Vec<(u64, String)> {
        let ticks_per_symbol = 250 * (window_nanos / 1_000_000_000);
        let mut timestamps = Vec::new();

        for symbol in symbols {
            // Average inter-arrival time: 4ms (= 1s / 250 ticks)
            let avg_interval_nanos = 4_000_000u64;
            let mut current_time = start_nanos;

            for _ in 0..ticks_per_symbol {
                // Exponential distribution for inter-arrival times (Poisson process)
                let u: f64 = self.rng.gen();
                let interval = (-(avg_interval_nanos as f64) * u.ln()) as u64;
                current_time += interval.max(1000); // Min 1μs

                if current_time < start_nanos + window_nanos {
                    timestamps.push((current_time, symbol.clone()));
                } else {
                    break;
                }
            }
        }

        timestamps.sort_by_key(|(ts, _)| *ts);
        timestamps
    }

    /// Lite profile: ~25 ticks/sec/symbol with uniform distribution
    fn generate_lite_timestamps(
        &mut self,
        start_nanos: u64,
        window_nanos: u64,
        symbols: &[String],
    ) -> Vec<(u64, String)> {
        let ticks_per_symbol = 25 * (window_nanos / 1_000_000_000);
        let interval_nanos = window_nanos / ticks_per_symbol;
        let mut timestamps = Vec::new();

        for symbol in symbols {
            for i in 0..ticks_per_symbol {
                // Uniform distribution with small jitter (+/- 10%)
                let base_time = start_nanos + i * interval_nanos;
                let jitter = self
                    .rng
                    .gen_range(-(interval_nanos as i64 / 10)..(interval_nanos as i64 / 10));
                let ts = (base_time as i64 + jitter).max(start_nanos as i64) as u64;

                if ts < start_nanos + window_nanos {
                    timestamps.push((ts, symbol.clone()));
                }
            }
        }

        timestamps.sort_by_key(|(ts, _)| *ts);
        timestamps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trading_common::data::dbn_types::TradeMsgExt;

    #[test]
    fn test_generator_determinism() {
        let config = DataGenConfig {
            symbol_count: 3,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };

        // Generate twice with same config
        let mut gen1 = TestDataGenerator::new(config.clone());
        let bundle1 = gen1.generate();

        let mut gen2 = TestDataGenerator::new(config);
        let bundle2 = gen2.generate();

        // Should produce identical results
        assert_eq!(bundle1.ticks.len(), bundle2.ticks.len());

        for (t1, t2) in bundle1.ticks.iter().zip(bundle2.ticks.iter()) {
            assert_eq!(t1.price, t2.price);
            assert_eq!(t1.size, t2.size);
            assert_eq!(t1.sequence, t2.sequence);
            assert_eq!(t1.side, t2.side);
        }
    }

    #[test]
    fn test_generator_different_seeds() {
        let config1 = DataGenConfig {
            symbol_count: 2,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 11111,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };

        let config2 = DataGenConfig {
            seed: 22222,
            ..config1.clone()
        };

        let mut gen1 = TestDataGenerator::new(config1);
        let bundle1 = gen1.generate();

        let mut gen2 = TestDataGenerator::new(config2);
        let bundle2 = gen2.generate();

        // Should have same count but different values
        assert_eq!(bundle1.ticks.len(), bundle2.ticks.len());

        // Prices should differ (with very high probability)
        let different_prices = bundle1
            .ticks
            .iter()
            .zip(bundle2.ticks.iter())
            .filter(|(t1, t2)| t1.price != t2.price)
            .count();
        assert!(different_prices > 0);
    }

    #[test]
    fn test_symbol_generation() {
        let config = DataGenConfig {
            symbol_count: 5,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };

        let mut gen = TestDataGenerator::new(config);
        let bundle = gen.generate();

        assert_eq!(bundle.metadata.symbols.len(), 5);
        assert!(bundle.metadata.symbols.contains(&"TEST0000".to_string()));
        assert!(bundle.metadata.symbols.contains(&"TEST0004".to_string()));
    }

    #[test]
    fn test_monotonic_sequence() {
        let config = DataGenConfig {
            symbol_count: 2,
            time_window_secs: 10,
            profile: VolumeProfile::Normal,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };

        let mut gen = TestDataGenerator::new(config);
        let bundle = gen.generate();

        // Check that sizes are monotonically increasing per symbol
        // (using instrument_id as proxy)
        let mut sizes_by_instrument: HashMap<u32, Vec<u32>> = HashMap::new();
        for tick in &bundle.ticks {
            sizes_by_instrument
                .entry(tick.hd.instrument_id)
                .or_default()
                .push(tick.size);
        }

        for (_instrument_id, sizes) in sizes_by_instrument {
            for i in 1..sizes.len() {
                assert!(
                    sizes[i] > sizes[i - 1],
                    "Size should be monotonically increasing"
                );
            }
        }
    }

    #[test]
    fn test_volume_profiles() {
        // Test that each profile generates approximately the expected number of ticks
        for profile in [
            VolumeProfile::Lite,
            VolumeProfile::Normal,
            VolumeProfile::Heavy,
        ] {
            let config = DataGenConfig {
                symbol_count: 2,
                time_window_secs: 5,
                profile,
                seed: 12345,
                exchange: "TEST".to_string(),
                base_price: 50000.0,
            };

            let expected = config.expected_tick_count();
            let mut gen = TestDataGenerator::new(config);
            let bundle = gen.generate();

            // Allow +/- 20% variance due to timing boundaries
            let variance = (expected as f64 * 0.2) as u64;
            let actual = bundle.ticks.len() as u64;
            assert!(
                actual >= expected.saturating_sub(variance) && actual <= expected + variance,
                "Profile {:?}: expected ~{}, got {}",
                profile,
                expected,
                actual
            );
        }
    }

    #[test]
    fn test_ticks_sorted_by_timestamp() {
        let config = DataGenConfig {
            symbol_count: 5,
            time_window_secs: 10,
            profile: VolumeProfile::Normal,
            seed: 99999,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };

        let mut gen = TestDataGenerator::new(config);
        let bundle = gen.generate();

        // Verify ticks are sorted by timestamp
        for i in 1..bundle.ticks.len() {
            assert!(
                bundle.ticks[i].hd.ts_event >= bundle.ticks[i - 1].hd.ts_event,
                "Ticks should be sorted by timestamp"
            );
        }
    }

    #[test]
    fn test_trade_side_alternation() {
        let config = DataGenConfig {
            symbol_count: 1,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        };

        let mut gen = TestDataGenerator::new(config);
        let bundle = gen.generate();

        // Check that sides alternate for single symbol
        let mut buy_count = 0;
        let mut sell_count = 0;
        for tick in &bundle.ticks {
            match tick.trade_side() {
                TradeSideCompat::Buy => buy_count += 1,
                TradeSideCompat::Sell => sell_count += 1,
                _ => {}
            }
        }

        // Should be roughly 50/50
        let total = buy_count + sell_count;
        assert!(buy_count > 0 && sell_count > 0);
        let ratio = buy_count as f64 / total as f64;
        assert!(
            ratio > 0.4 && ratio < 0.6,
            "Buy/sell ratio should be ~50/50"
        );
    }
}
