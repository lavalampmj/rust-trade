//! Generator Determinism Tests
//!
//! These tests validate that the test data generator produces deterministic
//! output when given the same seed, and different output with different seeds.

use integration_tests::{DataGenConfig, TestDataGenerator, VolumeProfile};
use trading_common::data::dbn_types::TradeMsgExt;

/// Test that the generator produces identical output with the same seed
#[test]
fn test_generator_determinism_same_seed() {
    let config = DataGenConfig {
        symbol_count: 5,
        time_window_secs: 10,
        profile: VolumeProfile::Lite,
        seed: 42,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let mut gen1 = TestDataGenerator::new(config.clone());
    let bundle1 = gen1.generate();

    let mut gen2 = TestDataGenerator::new(config);
    let bundle2 = gen2.generate();

    // Should have same number of ticks
    assert_eq!(
        bundle1.ticks.len(),
        bundle2.ticks.len(),
        "Tick counts should match"
    );

    // Every tick should be identical
    for (i, (t1, t2)) in bundle1.ticks.iter().zip(bundle2.ticks.iter()).enumerate() {
        assert_eq!(
            t1.price, t2.price,
            "Price mismatch at tick {}: {} vs {}",
            i, t1.price, t2.price
        );
        assert_eq!(
            t1.size, t2.size,
            "Size mismatch at tick {}: {} vs {}",
            i, t1.size, t2.size
        );
        assert_eq!(
            t1.side, t2.side,
            "Side mismatch at tick {}",
            i
        );
        assert_eq!(
            t1.sequence, t2.sequence,
            "Sequence mismatch at tick {}: {} vs {}",
            i, t1.sequence, t2.sequence
        );
        assert_eq!(
            t1.hd.instrument_id, t2.hd.instrument_id,
            "Instrument ID mismatch at tick {}",
            i
        );
    }

    // Metadata should also match
    assert_eq!(bundle1.metadata.symbols, bundle2.metadata.symbols);
    assert_eq!(bundle1.metadata.total_ticks, bundle2.metadata.total_ticks);
    assert_eq!(bundle1.metadata.seed, bundle2.metadata.seed);
}

/// Test that different seeds produce different output
#[test]
fn test_generator_different_seeds() {
    let config1 = DataGenConfig {
        symbol_count: 3,
        time_window_secs: 5,
        profile: VolumeProfile::Lite,
        seed: 111,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let config2 = DataGenConfig {
        seed: 222,
        ..config1.clone()
    };

    let mut gen1 = TestDataGenerator::new(config1);
    let bundle1 = gen1.generate();

    let mut gen2 = TestDataGenerator::new(config2);
    let bundle2 = gen2.generate();

    // Should have same number of ticks (deterministic count)
    assert_eq!(bundle1.ticks.len(), bundle2.ticks.len());

    // But prices should differ
    let different_prices = bundle1
        .ticks
        .iter()
        .zip(bundle2.ticks.iter())
        .filter(|(t1, t2)| t1.price != t2.price)
        .count();

    assert!(
        different_prices > bundle1.ticks.len() / 2,
        "At least half the prices should differ: {} of {} were different",
        different_prices,
        bundle1.ticks.len()
    );
}

/// Test symbol name generation
#[test]
fn test_symbol_name_generation() {
    let config = DataGenConfig {
        symbol_count: 10,
        time_window_secs: 5,
        profile: VolumeProfile::Lite,
        seed: 12345,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let symbols = config.generate_symbol_names();

    assert_eq!(symbols.len(), 10);
    assert_eq!(symbols[0], "TEST0000");
    assert_eq!(symbols[1], "TEST0001");
    assert_eq!(symbols[9], "TEST0009");

    // All should be unique
    let unique: std::collections::HashSet<_> = symbols.iter().collect();
    assert_eq!(unique.len(), symbols.len());
}

/// Test volume profile tick counts
#[test]
fn test_volume_profile_counts() {
    for profile in [VolumeProfile::Lite, VolumeProfile::Normal, VolumeProfile::Heavy] {
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

        // Allow +/- 30% variance due to timing boundaries
        let variance = (expected as f64 * 0.3) as u64;
        let actual = bundle.ticks.len() as u64;

        assert!(
            actual >= expected.saturating_sub(variance) && actual <= expected + variance,
            "Profile {:?}: expected ~{} (+/-{}), got {}",
            profile,
            expected,
            variance,
            actual
        );
    }
}

/// Test that ticks are sorted by timestamp
#[test]
fn test_ticks_sorted() {
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

    // Verify sorting
    for i in 1..bundle.ticks.len() {
        assert!(
            bundle.ticks[i].hd.ts_event >= bundle.ticks[i - 1].hd.ts_event,
            "Ticks should be sorted: tick {} has ts {} < tick {} ts {}",
            i,
            bundle.ticks[i].hd.ts_event,
            i - 1,
            bundle.ticks[i - 1].hd.ts_event
        );
    }
}

/// Test that size is monotonically increasing per instrument
#[test]
fn test_size_monotonic_per_instrument() {
    let config = DataGenConfig {
        symbol_count: 3,
        time_window_secs: 10,
        profile: VolumeProfile::Normal,
        seed: 12345,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let mut gen = TestDataGenerator::new(config);
    let bundle = gen.generate();

    // Group by instrument and check size monotonicity
    let mut last_size_by_instrument: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();

    for tick in &bundle.ticks {
        let instrument_id = tick.hd.instrument_id;
        let size = tick.size;

        if let Some(last_size) = last_size_by_instrument.get(&instrument_id) {
            assert!(
                size > *last_size,
                "Size should be monotonically increasing for instrument {}: {} <= {}",
                instrument_id,
                size,
                last_size
            );
        }

        last_size_by_instrument.insert(instrument_id, size);
    }
}

/// Test that sequence is monotonically increasing per instrument
#[test]
fn test_sequence_monotonic_per_instrument() {
    let config = DataGenConfig {
        symbol_count: 3,
        time_window_secs: 10,
        profile: VolumeProfile::Normal,
        seed: 12345,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let mut gen = TestDataGenerator::new(config);
    let bundle = gen.generate();

    let mut last_seq_by_instrument: std::collections::HashMap<u32, u32> =
        std::collections::HashMap::new();

    for tick in &bundle.ticks {
        let instrument_id = tick.hd.instrument_id;
        let seq = tick.sequence;

        if let Some(last_seq) = last_seq_by_instrument.get(&instrument_id) {
            assert!(
                seq > *last_seq,
                "Sequence should be monotonically increasing for instrument {}: {} <= {}",
                instrument_id,
                seq,
                last_seq
            );
        }

        last_seq_by_instrument.insert(instrument_id, seq);
    }
}

/// Test trade side alternation
#[test]
fn test_side_alternation() {
    let config = DataGenConfig {
        symbol_count: 1,
        time_window_secs: 10,
        profile: VolumeProfile::Lite,
        seed: 12345,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let mut gen = TestDataGenerator::new(config);
    let bundle = gen.generate();

    let mut buy_count = 0;
    let mut sell_count = 0;

    for tick in &bundle.ticks {
        match tick.trade_side() {
            trading_common::data::dbn_types::TradeSideCompat::Buy => buy_count += 1,
            trading_common::data::dbn_types::TradeSideCompat::Sell => sell_count += 1,
            _ => {}
        }
    }

    let total = buy_count + sell_count;
    assert!(total > 0, "Should have some trades");

    let ratio = buy_count as f64 / total as f64;
    assert!(
        ratio > 0.4 && ratio < 0.6,
        "Buy/sell ratio should be ~50/50: {:.1}% buys",
        ratio * 100.0
    );
}

/// Test price random walk stays positive
#[test]
fn test_price_positive() {
    let config = DataGenConfig {
        symbol_count: 10,
        time_window_secs: 60,
        profile: VolumeProfile::Heavy,
        seed: 54321,
        exchange: "TEST".to_string(),
        base_price: 100.0, // Start low to test edge case
    };

    let mut gen = TestDataGenerator::new(config);
    let bundle = gen.generate();

    for (i, tick) in bundle.ticks.iter().enumerate() {
        let price = tick.price_decimal();
        assert!(
            price > rust_decimal::Decimal::ZERO,
            "Price should always be positive: tick {} has price {}",
            i,
            price
        );
    }
}

/// Test metadata correctness
#[test]
fn test_metadata() {
    let config = DataGenConfig {
        symbol_count: 7,
        time_window_secs: 15,
        profile: VolumeProfile::Normal,
        seed: 77777,
        exchange: "TESTEX".to_string(),
        base_price: 12345.67,
    };

    let mut gen = TestDataGenerator::new(config.clone());
    let bundle = gen.generate();

    assert_eq!(bundle.metadata.symbols.len(), 7);
    assert_eq!(bundle.metadata.exchange, "TESTEX");
    assert_eq!(bundle.metadata.seed, 77777);
    assert_eq!(bundle.metadata.profile, VolumeProfile::Normal);
    assert_eq!(bundle.metadata.total_ticks, bundle.ticks.len() as u64);

    // Time range should be approximately correct
    let duration = bundle.metadata.end_time - bundle.metadata.start_time;
    assert!(
        duration.num_seconds() >= 14 && duration.num_seconds() <= 16,
        "Duration should be ~15 seconds: {} seconds",
        duration.num_seconds()
    );
}

/// Test per-symbol tick counts
#[test]
fn test_per_symbol_counts() {
    let config = DataGenConfig {
        symbol_count: 5,
        time_window_secs: 10,
        profile: VolumeProfile::Normal,
        seed: 12345,
        exchange: "TEST".to_string(),
        base_price: 50000.0,
    };

    let mut gen = TestDataGenerator::new(config);
    let bundle = gen.generate();

    // All symbols should have ticks
    for symbol in &bundle.metadata.symbols {
        let count = bundle.symbol_counts.get(symbol).unwrap_or(&0);
        assert!(
            *count > 0,
            "Symbol {} should have some ticks",
            symbol
        );
    }

    // Total should match
    let total: u64 = bundle.symbol_counts.values().sum();
    assert_eq!(total, bundle.ticks.len() as u64);
}

/// Test multiple generations with different configs don't interfere
#[test]
fn test_independent_generations() {
    let config1 = DataGenConfig {
        symbol_count: 2,
        time_window_secs: 5,
        profile: VolumeProfile::Lite,
        seed: 111,
        exchange: "EX1".to_string(),
        base_price: 100.0,
    };

    let config2 = DataGenConfig {
        symbol_count: 3,
        time_window_secs: 5,
        profile: VolumeProfile::Normal,
        seed: 222,
        exchange: "EX2".to_string(),
        base_price: 200.0,
    };

    let mut gen1 = TestDataGenerator::new(config1);
    let mut gen2 = TestDataGenerator::new(config2);

    let bundle1 = gen1.generate();
    let bundle2 = gen2.generate();

    // Should have different characteristics
    assert_ne!(bundle1.metadata.symbols, bundle2.metadata.symbols);
    assert_ne!(bundle1.metadata.exchange, bundle2.metadata.exchange);
    assert_ne!(bundle1.ticks.len(), bundle2.ticks.len());
}
