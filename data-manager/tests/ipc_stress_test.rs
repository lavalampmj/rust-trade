//! IPC Stress Tests
//!
//! These tests verify the shared memory IPC implementation under stress conditions.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use data_manager::schema::{CompactTick, NormalizedTick, TradeSide};
use data_manager::transport::ipc::{SharedMemoryChannel, SharedMemoryConfig};
use rust_decimal_macros::dec;

fn create_test_tick(symbol: &str, exchange: &str, seq: i64) -> NormalizedTick {
    NormalizedTick::new(
        Utc::now(),
        symbol.to_string(),
        exchange.to_string(),
        dec!(5025.50) + rust_decimal::Decimal::from(seq % 100),
        dec!(10),
        if seq % 2 == 0 {
            TradeSide::Buy
        } else {
            TradeSide::Sell
        },
        "test".to_string(),
        seq,
    )
}

/// Test basic producer/consumer communication
#[test]
fn test_basic_producer_consumer() {
    let config = SharedMemoryConfig {
        path_prefix: "/test_basic_".to_string(),
        buffer_entries: 1024,
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    let channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
    let mut producer = channel.producer();
    let mut consumer = channel.consumer();

    // Send 100 ticks
    for i in 0..100i64 {
        let tick = create_test_tick("ES", "CME", i);
        producer.send(&tick).unwrap();
    }

    // Receive all ticks
    let mut received = 0;
    while let Some(tick) = consumer.try_recv().unwrap() {
        assert_eq!(tick.sequence, received);
        received += 1;
    }

    assert_eq!(received, 100);
}

/// Test high-throughput single-threaded performance
#[test]
fn test_throughput_single_thread() {
    let buffer_entries = 65536usize;
    let config = SharedMemoryConfig {
        path_prefix: "/test_throughput_".to_string(),
        buffer_entries,
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    let channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
    let mut producer = channel.producer();
    let mut consumer = channel.consumer();

    let tick_count = 100_000u64;

    // Measure send throughput
    let start = Instant::now();
    for i in 0..tick_count as i64 {
        let tick = create_test_tick("ES", "CME", i);
        producer.send(&tick).unwrap();
    }
    let send_elapsed = start.elapsed();

    // Measure receive throughput - we may have lost some due to overflow
    let start = Instant::now();
    let mut received = 0u64;
    while let Some(_tick) = consumer.try_recv().unwrap() {
        received += 1;
    }
    let recv_elapsed = start.elapsed();

    // We expect to receive at most buffer_entries (oldest were overwritten)
    assert!(
        received <= buffer_entries as u64,
        "Received more than buffer size"
    );
    assert!(received > 0, "Should receive some ticks");

    let send_rate = tick_count as f64 / send_elapsed.as_secs_f64();
    let recv_rate = received as f64 / recv_elapsed.as_secs_f64();

    println!(
        "Single-thread throughput: send={:.0} ticks/sec ({} ticks in {:?})",
        send_rate, tick_count, send_elapsed
    );
    println!(
        "Single-thread throughput: recv={:.0} ticks/sec ({} ticks in {:?})",
        recv_rate, received, recv_elapsed
    );

    // Should achieve at least 500K ticks/second for sends
    assert!(send_rate > 500_000.0, "Send rate too low: {}", send_rate);
}

/// Test concurrent producer/consumer with separate threads
#[test]
fn test_concurrent_producer_consumer() {
    let config = SharedMemoryConfig {
        path_prefix: "/test_concurrent_".to_string(),
        buffer_entries: 65536,
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    let channel = Arc::new(SharedMemoryChannel::create("ES", "CME", config).unwrap());
    let tick_count = 50_000u64; // Reduced for concurrent test
    let stop = Arc::new(AtomicBool::new(false));
    let received_count = Arc::new(AtomicU64::new(0));
    let sent_count = Arc::new(AtomicU64::new(0));

    // Consumer thread
    let consumer_channel = Arc::clone(&channel);
    let consumer_stop = Arc::clone(&stop);
    let consumer_received = Arc::clone(&received_count);
    let consumer_handle = thread::spawn(move || {
        let mut consumer = consumer_channel.consumer();
        let mut last_seq = -1i64;
        let mut out_of_order = 0u64;

        while !consumer_stop.load(Ordering::Relaxed) {
            if let Some(tick) = consumer.try_recv().unwrap() {
                if tick.sequence <= last_seq && last_seq != -1 {
                    out_of_order += 1;
                }
                last_seq = tick.sequence;
                consumer_received.fetch_add(1, Ordering::Relaxed);
            } else {
                thread::yield_now();
            }
        }

        // Drain remaining
        while let Some(tick) = consumer.try_recv().unwrap() {
            if tick.sequence <= last_seq && last_seq != -1 {
                out_of_order += 1;
            }
            last_seq = tick.sequence;
            consumer_received.fetch_add(1, Ordering::Relaxed);
        }

        out_of_order
    });

    // Producer thread
    let producer_channel = Arc::clone(&channel);
    let producer_sent = Arc::clone(&sent_count);
    let producer_handle = thread::spawn(move || {
        let mut producer = producer_channel.producer();

        for i in 0..tick_count as i64 {
            let tick = create_test_tick("ES", "CME", i);
            producer.send(&tick).unwrap();
            producer_sent.fetch_add(1, Ordering::Relaxed);
        }
    });

    // Wait for producer to finish
    producer_handle.join().unwrap();

    // Give consumer time to catch up
    thread::sleep(Duration::from_millis(200));
    stop.store(true, Ordering::Relaxed);

    let out_of_order = consumer_handle.join().unwrap();

    let received = received_count.load(Ordering::Relaxed);
    let sent = sent_count.load(Ordering::Relaxed);

    println!(
        "Concurrent test: sent={}, received={}, out_of_order={}",
        sent, received, out_of_order
    );

    assert_eq!(sent, tick_count);
    // Should receive most ticks (allowing for some timing variance)
    assert!(
        received >= tick_count * 90 / 100,
        "Should receive at least 90% of ticks: got {} of {}",
        received,
        tick_count
    );
}

/// Test buffer overflow behavior (producer faster than consumer)
#[test]
fn test_overflow_behavior() {
    let config = SharedMemoryConfig {
        path_prefix: "/test_overflow_".to_string(),
        buffer_entries: 64, // Small buffer to force overflow
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    let channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
    let mut producer = channel.producer();
    let mut consumer = channel.consumer();

    // Send more ticks than buffer can hold
    let tick_count = 200i64;
    for i in 0..tick_count {
        let tick = create_test_tick("ES", "CME", i);
        producer.send(&tick).unwrap();
    }

    // Consumer should detect overflow
    assert!(consumer.had_overflow(), "Overflow should be detected");

    // Should be able to read remaining ticks (most recent ones)
    let mut received = Vec::new();
    while let Some(tick) = consumer.try_recv().unwrap() {
        received.push(tick.sequence);
    }

    // We should have received the most recent ticks
    assert!(!received.is_empty());
    assert_eq!(*received.last().unwrap(), tick_count - 1);

    println!(
        "Overflow test: sent={}, received={} (oldest lost as expected)",
        tick_count,
        received.len()
    );
}

/// Test multiple symbol channels
#[test]
fn test_multiple_symbols() {
    let symbols = vec![("ES", "CME"), ("NQ", "CME"), ("CL", "NYMEX"), ("GC", "COMEX")];
    let tick_count = 1_000i64; // Reduced for faster test

    let mut channels = Vec::new();
    for (symbol, exchange) in &symbols {
        let config = SharedMemoryConfig {
            path_prefix: format!("/test_multi_{}_", symbol),
            buffer_entries: 4096,
            entry_size: std::mem::size_of::<CompactTick>(),
        };
        channels.push(SharedMemoryChannel::create(symbol, exchange, config).unwrap());
    }

    // Send ticks to all channels
    for i in 0..tick_count {
        for (idx, channel) in channels.iter().enumerate() {
            let (symbol, exchange) = &symbols[idx];
            let tick = create_test_tick(symbol, exchange, i);
            channel.producer().send(&tick).unwrap();
        }
    }

    // Verify all channels received correct data
    for (idx, channel) in channels.iter().enumerate() {
        let mut consumer = channel.consumer();
        let mut count = 0i64;
        while let Some(tick) = consumer.try_recv().unwrap() {
            assert_eq!(tick.symbol, symbols[idx].0);
            assert_eq!(tick.exchange, symbols[idx].1);
            count += 1;
        }
        assert_eq!(count, tick_count);
    }
}

/// Stress test with high message rate
#[test]
fn test_stress_high_rate() {
    let buffer_entries = 131072usize; // 128K entries
    let config = SharedMemoryConfig {
        path_prefix: "/test_stress_".to_string(),
        buffer_entries,
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    let channel = Arc::new(SharedMemoryChannel::create("ES", "CME", config).unwrap());
    let tick_count = 200_000u64; // Reduced for more reliable test
    let stop = Arc::new(AtomicBool::new(false));
    let received_count = Arc::new(AtomicU64::new(0));

    // Start consumer first
    let consumer_channel = Arc::clone(&channel);
    let consumer_stop = Arc::clone(&stop);
    let consumer_received = Arc::clone(&received_count);
    let consumer_handle = thread::spawn(move || {
        let mut consumer = consumer_channel.consumer();
        let mut max_seq = -1i64;

        while !consumer_stop.load(Ordering::Relaxed) {
            if let Some(tick) = consumer.try_recv().unwrap() {
                if tick.sequence > max_seq {
                    max_seq = tick.sequence;
                }
                consumer_received.fetch_add(1, Ordering::Relaxed);
            } else {
                // Small sleep to reduce CPU usage
                thread::sleep(Duration::from_micros(10));
            }
        }

        // Drain remaining
        while let Some(tick) = consumer.try_recv().unwrap() {
            if tick.sequence > max_seq {
                max_seq = tick.sequence;
            }
            consumer_received.fetch_add(1, Ordering::Relaxed);
        }

        max_seq
    });

    // Small delay to let consumer start
    thread::sleep(Duration::from_millis(10));

    // Producer with timing
    let start = Instant::now();
    {
        let mut producer = channel.producer();
        for i in 0..tick_count as i64 {
            let tick = create_test_tick("ES", "CME", i);
            producer.send(&tick).unwrap();
        }
    }
    let elapsed = start.elapsed();

    // Signal stop and wait
    thread::sleep(Duration::from_millis(200));
    stop.store(true, Ordering::Relaxed);
    let max_seq = consumer_handle.join().unwrap();

    let received = received_count.load(Ordering::Relaxed);
    let rate = tick_count as f64 / elapsed.as_secs_f64();

    println!(
        "Stress test: {} ticks in {:?} ({:.0} ticks/sec), received={}, max_seq={}",
        tick_count, elapsed, rate, received, max_seq
    );

    // Verify we received the last tick (max_seq should be tick_count - 1)
    assert_eq!(max_seq, tick_count as i64 - 1, "Should receive last tick");

    // Should receive a good portion of ticks (at least 50% given the high rate)
    let min_expected = tick_count / 2;
    assert!(
        received >= min_expected,
        "Should receive at least {} ticks, got {}",
        min_expected,
        received
    );
}

/// Test latency characteristics
#[test]
fn test_latency() {
    let config = SharedMemoryConfig {
        path_prefix: "/test_latency_".to_string(),
        buffer_entries: 1024,
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    let channel = SharedMemoryChannel::create("ES", "CME", config).unwrap();
    let mut producer = channel.producer();
    let mut consumer = channel.consumer();

    let mut latencies = Vec::with_capacity(10000);

    for i in 0..10000i64 {
        let tick = create_test_tick("ES", "CME", i);
        let start = Instant::now();
        producer.send(&tick).unwrap();
        let _received = consumer.try_recv().unwrap().unwrap();
        let latency = start.elapsed();
        latencies.push(latency.as_nanos() as u64);
    }

    latencies.sort();

    let p50 = latencies[latencies.len() / 2];
    let p99 = latencies[latencies.len() * 99 / 100];
    let p999 = latencies[latencies.len() * 999 / 1000];
    let avg = latencies.iter().sum::<u64>() / latencies.len() as u64;

    println!(
        "Latency: avg={}ns, p50={}ns, p99={}ns, p99.9={}ns",
        avg, p50, p99, p999
    );

    // Realistic expectations for shared memory IPC
    // p50 should be under 10 microseconds (10,000 ns)
    assert!(p50 < 10_000, "p50 latency too high: {}ns", p50);
    // p99 should be under 50 microseconds
    assert!(p99 < 50_000, "p99 latency too high: {}ns", p99);
}

/// Test real POSIX shared memory (cross-thread, simulating cross-process)
#[test]
fn test_real_shared_memory() {
    let shm_name = "/test_real_shm_es_cme";
    let config = SharedMemoryConfig {
        path_prefix: "/test_real_".to_string(),
        buffer_entries: 4096,
        entry_size: std::mem::size_of::<CompactTick>(),
    };

    // Create channel (simulates data-manager)
    let channel = SharedMemoryChannel::create_named(shm_name, "ES", "CME", config.clone()).unwrap();

    let tick_count = 1000i64;

    // Producer thread (data-manager side)
    let producer_handle = thread::spawn(move || {
        let mut producer = channel.producer();
        for i in 0..tick_count {
            let tick = create_test_tick("ES", "CME", i);
            producer.send(&tick).unwrap();
        }
        // Keep channel alive until consumer is done
        thread::sleep(Duration::from_millis(500));
    });

    // Give producer time to create shared memory
    thread::sleep(Duration::from_millis(50));

    // Consumer thread opens existing shared memory (simulates trading-core)
    let consumer_handle = thread::spawn(move || {
        let consumer_channel = SharedMemoryChannel::open_named(shm_name, "ES", "CME").unwrap();
        let mut consumer = consumer_channel.consumer();

        let mut received = Vec::new();
        let start = Instant::now();
        while received.len() < tick_count as usize && start.elapsed() < Duration::from_secs(2) {
            if let Some(tick) = consumer.try_recv().unwrap() {
                received.push(tick.sequence);
            } else {
                thread::yield_now();
            }
        }

        received
    });

    producer_handle.join().unwrap();
    let received = consumer_handle.join().unwrap();

    assert_eq!(
        received.len(),
        tick_count as usize,
        "Did not receive all ticks"
    );

    // Verify order
    for (i, seq) in received.iter().enumerate() {
        assert_eq!(*seq, i as i64, "Tick {} out of order", i);
    }

    // Cleanup
    SharedMemoryChannel::unlink(shm_name).ok();
}
