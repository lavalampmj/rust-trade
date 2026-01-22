//! End-to-End Pipeline Integration Tests
//!
//! These tests validate the full data pipeline from data generation
//! through strategy processing, measuring tick throughput and latency.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::time::timeout;

use data_manager::provider::{DataProvider, LiveStreamProvider, LiveSubscription, StreamEvent};
use integration_tests::{
    generate_report,
    DataGenConfig, EmulatorConfig, IntegrationTestConfig, MetricsConfig,
    MetricsCollector, ReportFormat, StrategyConfig, StrategyRunnerManager,
    TestDataEmulator, TestDataGenerator, TransportConfig, TransportMode, VolumeProfile,
    WebSocketConfig, DbWriter, DbWriterConfig, DbVerifier,
};

/// Helper function to run a test with the given configuration
async fn run_pipeline_test(config: IntegrationTestConfig) -> integration_tests::TestResults {
    // 1. Generate test data
    let mut generator = TestDataGenerator::new(config.data_gen.clone());
    let bundle = generator.generate();

    let ticks_generated = bundle.ticks.len() as u64;
    println!(
        "Generated {} ticks for {} symbols",
        ticks_generated,
        bundle.metadata.symbols.len()
    );

    // 2. Create metrics collector
    let metrics_collector = MetricsCollector::new(config.metrics.clone());

    // 3. Create strategy runners with symbol subscriptions
    // Each runner subscribes to a specific symbol (rust_0 -> TEST0000, etc.)
    let symbols = bundle.metadata.symbols.clone();
    let manager = Arc::new(StrategyRunnerManager::from_config(
        &config.strategies,
        &symbols,
        config.metrics.latency_sample_limit,
    ));

    // Log subscription info
    for (id, strategy_type, symbol) in manager.subscription_info() {
        println!("  {} ({}) -> {}", id, strategy_type, symbol);
    }

    // Note: Don't register strategies with metrics_collector since runners
    // maintain their own metrics. We'll pass them directly to build_results.

    // 4. Create emulator
    let mut emulator = TestDataEmulator::new(bundle.clone(), config.emulator.clone());
    let emulator_metrics = emulator.metrics_clone();

    // 5. Connect emulator
    emulator.connect().await.expect("Failed to connect emulator");

    // 6. Set up callback to route ticks to subscribed strategy runners
    let manager_clone = manager.clone();
    let callback = Arc::new(move |event: StreamEvent| {
        if let StreamEvent::Tick(tick) = event {
            // Use blocking task to call async method from sync context
            let manager = manager_clone.clone();
            // Convert TickData to NormalizedTick for strategy runners
            let tick: data_manager::schema::NormalizedTick = tick.into();
            tokio::spawn(async move {
                // Route tick only to runners subscribed to this symbol
                manager.route_tick(&tick).await;
            });
        }
    });

    // 7. Create shutdown channel
    let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

    // 8. Start metrics timing
    metrics_collector.start();

    // 9. Run the pipeline with timeout
    let subscription = LiveSubscription::trades(vec![]);

    let test_result = timeout(
        config.timeout(),
        emulator.subscribe(subscription, callback, shutdown_rx),
    )
    .await;

    // 10. Handle timeout
    if test_result.is_err() {
        println!("Test timed out after {:?}", config.timeout());
    }

    // 11. Wait for settling time to allow spawned tasks to complete
    tokio::time::sleep(config.settling_time()).await;

    // 12. Stop metrics timing
    metrics_collector.stop();

    // 13. Shutdown runners
    manager.shutdown_all().await;

    // 14. Collect results using runner's actual metrics
    let ticks_sent = emulator_metrics.sent_count();
    let strategy_metrics = manager.all_metrics();
    let results = metrics_collector.build_results_with_metrics(
        ticks_generated,
        ticks_sent,
        None,
        strategy_metrics,
    );

    results
}

/// Quick validation test using Lite profile
#[tokio::test]
async fn test_lite_pipeline() {
    let config = IntegrationTestConfig::lite();

    println!("\n=== Running LITE Pipeline Test ===");
    println!("Expected ticks: ~{}", config.data_gen.expected_tick_count());

    let results = run_pipeline_test(config.clone()).await;

    // Generate and print report
    let report = generate_report(&results, ReportFormat::Simple);
    println!("{}", report);

    // Basic assertions
    assert!(
        results.ticks_sent > 0,
        "Should have sent some ticks"
    );

    // Lite test should complete quickly
    assert!(
        results.test_duration < Duration::from_secs(60),
        "Lite test took too long: {:?}",
        results.test_duration
    );
}

/// Standard stress test using Normal profile
#[tokio::test]
async fn test_normal_pipeline() {
    let config = IntegrationTestConfig::normal();

    println!("\n=== Running NORMAL Pipeline Test ===");
    println!("Expected ticks: ~{}", config.data_gen.expected_tick_count());

    let results = run_pipeline_test(config.clone()).await;

    // Generate and print report
    let report = generate_report(&results, ReportFormat::Pretty);
    println!("{}", report);

    // Assertions
    assert!(
        results.ticks_sent > 0,
        "Should have sent some ticks"
    );

    // Check tick throughput
    let expected = config.data_gen.expected_tick_count();
    let actual = results.ticks_sent;
    let ratio = actual as f64 / expected as f64;
    assert!(
        ratio > 0.5,
        "Should have sent at least 50% of expected ticks: {} / {}",
        actual,
        expected
    );
}

/// Heavy stress test (marked as ignored by default due to runtime)
#[tokio::test]
#[ignore = "Heavy test takes ~5 minutes, run with --ignored"]
async fn test_heavy_pipeline() {
    let config = IntegrationTestConfig::heavy();

    println!("\n=== Running HEAVY Pipeline Test ===");
    println!("Expected ticks: ~{}", config.data_gen.expected_tick_count());
    println!("This test may take several minutes...");

    let results = run_pipeline_test(config.clone()).await;

    // Generate and print report
    let report = generate_report(&results, ReportFormat::Pretty);
    println!("{}", report);

    // Under heavy load, we accept some tick loss
    let loss_rate = results.tick_loss_rate();
    assert!(
        loss_rate < 0.1,
        "Tick loss rate should be under 10%: {:.2}%",
        loss_rate * 100.0
    );
}

/// Test with custom configuration
#[tokio::test]
async fn test_custom_config() {
    let config = IntegrationTestConfig {
        data_gen: DataGenConfig {
            symbol_count: 3,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 99999, // Different seed
            exchange: "CUSTOM".to_string(),
            base_price: 100.0,
        },
        emulator: EmulatorConfig {
            replay_speed: 5.0, // 5x speed
            embed_send_time: true,
            min_delay_us: 1,
            transport: TransportConfig {
                websocket: WebSocketConfig {
                    port: 19500, // Unique port for custom config test
                    ..Default::default()
                },
                ..Default::default()
            },
        },
        strategies: StrategyConfig {
            rust_count: 3,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        },
        metrics: MetricsConfig {
            latency_sample_limit: 10000,
            tick_loss_tolerance: 0.1, // Very lenient for fast replay
            max_avg_latency_us: 10000,
            max_p99_latency_us: 100000,
        },
        test: integration_tests::TestConfig {
            timeout_secs: 30,
            settling_time_secs: 1,
            database_url: None,
            verify_db_persistence: false,
        },
    };

    println!("\n=== Running CUSTOM Pipeline Test ===");

    let results = run_pipeline_test(config).await;

    let report = generate_report(&results, ReportFormat::Compact);
    println!("{}", report);

    assert!(results.ticks_sent > 0);
}

/// Test multiple concurrent runs
#[tokio::test]
async fn test_concurrent_runs() {
    let configs: Vec<_> = (0..3)
        .map(|i| {
            let mut config = IntegrationTestConfig::lite();
            config.data_gen.seed = 10000 + i as u64;
            // Ensure runner count matches symbol count so all ticks have subscribers
            config.strategies.rust_count = config.data_gen.symbol_count;
            config.strategies.python_count = 0;
            // Use unique ports for concurrent runs (19110, 19111, 19112)
            config.emulator.transport.websocket.port = 19110 + i as u16;
            config
        })
        .collect();

    println!("\n=== Running CONCURRENT Pipeline Tests ===");

    let handles: Vec<_> = configs
        .into_iter()
        .enumerate()
        .map(|(i, config)| {
            tokio::spawn(async move {
                println!("Starting run {}", i);
                let results = run_pipeline_test(config).await;
                println!(
                    "Run {} complete: {} ticks in {:?}",
                    i, results.ticks_sent, results.test_duration
                );
                results
            })
        })
        .collect();

    let mut all_passed = true;
    for (i, handle) in handles.into_iter().enumerate() {
        let results = handle.await.expect("Task panicked");
        if !results.passed {
            println!("Run {} failed: {:?}", i, results.failures);
            all_passed = false;
        }
    }

    assert!(all_passed, "Some concurrent runs failed");
}

/// Test that emulator shutdown works correctly
#[tokio::test]
async fn test_early_shutdown() {
    let config = IntegrationTestConfig {
        data_gen: DataGenConfig {
            symbol_count: 5,
            time_window_secs: 30, // Long enough to interrupt
            profile: VolumeProfile::Normal,
            seed: 12345,
            exchange: "TEST".to_string(),
            base_price: 50000.0,
        },
        emulator: EmulatorConfig::default()
            .with_port(19400), // Unique port for early shutdown test
        strategies: StrategyConfig {
            rust_count: 1,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: false,
        },
        metrics: MetricsConfig::default(),
        test: integration_tests::TestConfig {
            timeout_secs: 5, // Short timeout to force early completion
            settling_time_secs: 1,
            database_url: None,
            verify_db_persistence: false,
        },
    };

    println!("\n=== Running EARLY SHUTDOWN Test ===");

    let mut generator = TestDataGenerator::new(config.data_gen.clone());
    let bundle = generator.generate();
    let total_ticks = bundle.ticks.len();

    let mut emulator = TestDataEmulator::new(bundle, config.emulator.clone());
    emulator.connect().await.expect("Failed to connect");

    let received_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let received_clone = received_count.clone();

    let callback = Arc::new(move |event: StreamEvent| {
        if matches!(event, StreamEvent::Tick(_)) {
            received_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    });

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

    // Send shutdown after 1 second
    let shutdown_handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        shutdown_tx.send(()).ok();
    });

    let _ = emulator
        .subscribe(LiveSubscription::trades(vec![]), callback, shutdown_rx)
        .await;

    shutdown_handle.await.unwrap();

    let received = received_count.load(std::sync::atomic::Ordering::SeqCst);
    println!(
        "Received {} / {} ticks before shutdown",
        received, total_ticks
    );

    // Should have received some but not all ticks
    assert!(received > 0, "Should have received some ticks");
    assert!(
        received < total_ticks as u64,
        "Should not have received all ticks"
    );
}

/// Test report generation with various results
#[tokio::test]
async fn test_report_formats() {
    let mut config = IntegrationTestConfig::lite();
    // Use unique port to avoid conflict with test_lite_pipeline
    config.emulator.transport.websocket.port = 19150;
    let results = run_pipeline_test(config).await;

    // Test all report formats
    let pretty = generate_report(&results, ReportFormat::Pretty);
    let simple = generate_report(&results, ReportFormat::Simple);
    let compact = generate_report(&results, ReportFormat::Compact);

    // Verify each format has expected content
    assert!(pretty.contains("TICK COUNTS"));
    assert!(pretty.contains("LATENCY"));

    assert!(simple.contains("Generated:"));
    assert!(simple.contains("Sent:"));

    assert!(compact.starts_with('['));
    assert!(compact.contains("ticks_sent="));

    println!("\n=== Report Format Comparison ===");
    println!("\n--- Pretty Format ---");
    println!("{}", pretty);
    println!("\n--- Simple Format ---");
    println!("{}", simple);
    println!("\n--- Compact Format ---");
    println!("{}", compact);
}

/// Test with WebSocket transport for realistic network simulation
#[tokio::test]
async fn test_websocket_transport() {
    // Use a unique port to avoid conflicts with other tests
    let port = 19800;

    let config = IntegrationTestConfig {
        data_gen: DataGenConfig {
            symbol_count: 3,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 77777,
            exchange: "TEST".to_string(),
            base_price: 100.0,
        },
        emulator: EmulatorConfig {
            replay_speed: 10.0, // Speed up for testing
            embed_send_time: true,
            min_delay_us: 1,
            transport: TransportConfig {
                mode: TransportMode::WebSocket,
                websocket: WebSocketConfig {
                    port,
                    ..Default::default()
                },
            },
        },
        strategies: StrategyConfig {
            rust_count: 3, // Must match symbol_count so all symbols have subscribers
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        },
        metrics: MetricsConfig {
            latency_sample_limit: 10000,
            tick_loss_tolerance: 0.05, // Allow slightly higher loss for network
            max_avg_latency_us: 50000, // 50ms - higher for WebSocket
            max_p99_latency_us: 100000, // 100ms
        },
        test: integration_tests::TestConfig {
            timeout_secs: 30,
            settling_time_secs: 2,
            database_url: None,
            verify_db_persistence: false,
        },
    };

    println!("\n=== Running WEBSOCKET Transport Test ===");
    println!("Port: {}", port);

    let results = run_pipeline_test(config).await;

    let report = generate_report(&results, ReportFormat::Compact);
    println!("{}", report);

    // Assertions
    assert!(
        results.ticks_sent > 0,
        "Should have sent some ticks through WebSocket"
    );

    // WebSocket should have higher latency than direct
    let avg_latency = results.latency_aggregate.average_us();
    println!("WebSocket average latency: {:.1}μs", avg_latency);

    // Just verify it completed - latency will be higher than direct
    assert!(results.passed || results.failures.is_empty() ||
        results.failures.iter().all(|f| f.contains("latency")),
        "Test should pass or only have latency-related issues");
}

/// Compare Direct vs WebSocket transport latencies
#[tokio::test]
async fn test_transport_comparison() {
    println!("\n=== Transport Comparison Test ===\n");

    // Common configuration
    let base_data_gen = DataGenConfig {
        symbol_count: 2,
        time_window_secs: 3,
        profile: VolumeProfile::Lite,
        seed: 88888,
        exchange: "TEST".to_string(),
        base_price: 100.0,
    };

    let base_strategies = StrategyConfig {
        rust_count: 1,
        python_count: 0,
        strategy_type: "tick_counter".to_string(),
        track_latency: true,
    };

    // Test 1: Direct transport
    let direct_config = IntegrationTestConfig {
        data_gen: base_data_gen.clone(),
        emulator: EmulatorConfig {
            replay_speed: 20.0,
            embed_send_time: true,
            min_delay_us: 1,
            transport: TransportConfig {
                mode: TransportMode::Direct,
                ..Default::default()
            },
        },
        strategies: base_strategies.clone(),
        metrics: MetricsConfig::default(),
        test: integration_tests::TestConfig {
            timeout_secs: 15,
            settling_time_secs: 1,
            database_url: None,
            verify_db_persistence: false,
        },
    };

    println!("--- Direct Transport ---");
    let direct_results = run_pipeline_test(direct_config).await;
    let direct_latency = direct_results.latency_aggregate.average_us();
    let direct_p99 = direct_results.latency_aggregate.percentile_us(99.0);
    println!(
        "Direct: avg={:.1}μs, p99={:.0}μs, ticks={}",
        direct_latency, direct_p99, direct_results.ticks_sent
    );

    // Test 2: WebSocket transport (use different port)
    let ws_config = IntegrationTestConfig {
        data_gen: base_data_gen,
        emulator: EmulatorConfig {
            replay_speed: 20.0,
            embed_send_time: true,
            min_delay_us: 1,
            transport: TransportConfig {
                mode: TransportMode::WebSocket,
                websocket: WebSocketConfig {
                    port: 19801,
                    ..Default::default()
                },
            },
        },
        strategies: base_strategies,
        metrics: MetricsConfig {
            max_avg_latency_us: 100000, // 100ms - very lenient for WS
            max_p99_latency_us: 500000, // 500ms
            ..Default::default()
        },
        test: integration_tests::TestConfig {
            timeout_secs: 15,
            settling_time_secs: 2, // Longer settling for network
            database_url: None,
            verify_db_persistence: false,
        },
    };

    println!("\n--- WebSocket Transport ---");
    let ws_results = run_pipeline_test(ws_config).await;
    let ws_latency = ws_results.latency_aggregate.average_us();
    let ws_p99 = ws_results.latency_aggregate.percentile_us(99.0);
    println!(
        "WebSocket: avg={:.1}μs, p99={:.0}μs, ticks={}",
        ws_latency, ws_p99, ws_results.ticks_sent
    );

    // Comparison summary
    println!("\n--- Comparison ---");
    println!(
        "Latency overhead: {:.1}μs ({:.1}x)",
        ws_latency - direct_latency,
        if direct_latency > 0.0 { ws_latency / direct_latency } else { 0.0 }
    );

    // Both should have processed ticks
    assert!(direct_results.ticks_sent > 0, "Direct should process ticks");
    assert!(ws_results.ticks_sent > 0, "WebSocket should process ticks");

    // WebSocket latency should typically be higher (network + serialization)
    // But we don't strictly assert this as localhost can be very fast
}

/// Test database persistence verification
///
/// This test verifies that ticks are correctly persisted to TimescaleDB.
/// Requires DATABASE_URL environment variable to be set.
#[tokio::test]
#[ignore = "Requires database connection, run with --ignored"]
async fn test_database_persistence() {
    use chrono::Utc;
    use std::env;

    println!("\n=== Running DATABASE PERSISTENCE Test ===");

    // Get database URL from environment
    let database_url = match env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!("DATABASE_URL not set, skipping test");
            return;
        }
    };

    // Configuration for the test
    let config = IntegrationTestConfig {
        data_gen: DataGenConfig {
            symbol_count: 3,
            time_window_secs: 5,
            profile: VolumeProfile::Lite,
            seed: 55555,
            exchange: "TEST".to_string(),
            base_price: 100.0,
        },
        emulator: EmulatorConfig {
            replay_speed: 10.0, // Speed up for testing
            embed_send_time: true,
            min_delay_us: 1,
            transport: TransportConfig {
                mode: TransportMode::WebSocket,
                websocket: WebSocketConfig {
                    port: 19900, // Unique port for db persistence test
                    ..Default::default()
                },
            },
        },
        strategies: StrategyConfig {
            rust_count: 1,
            python_count: 0,
            strategy_type: "tick_counter".to_string(),
            track_latency: true,
        },
        metrics: MetricsConfig::default(),
        test: integration_tests::TestConfig {
            timeout_secs: 60,
            settling_time_secs: 5,
            database_url: Some(database_url.clone()),
            verify_db_persistence: true,
        },
    };

    // Record start time for querying
    let test_start = Utc::now();

    // 1. Generate test data
    let mut generator = TestDataGenerator::new(config.data_gen.clone());
    let bundle = generator.generate();
    let ticks_generated = bundle.ticks.len() as u64;
    println!("Generated {} ticks", ticks_generated);

    // 2. Set up database writer
    let db_config = DbWriterConfig::new(database_url.clone())
        .with_batch_size(500)
        .with_flush_interval_ms(50);
    let mut db_writer = DbWriter::new(db_config);

    db_writer.connect().await.expect("Failed to connect to database");
    let db_sender = db_writer.start().expect("Failed to start db writer");

    // 3. Create metrics collector and strategy runners with symbol subscriptions
    let metrics_collector = MetricsCollector::new(config.metrics.clone());
    let symbols = bundle.metadata.symbols.clone();
    let manager = Arc::new(StrategyRunnerManager::from_config(
        &config.strategies,
        &symbols,
        config.metrics.latency_sample_limit,
    ));

    // 4. Create emulator
    let mut emulator = TestDataEmulator::new(bundle.clone(), config.emulator.clone());
    let emulator_metrics = emulator.metrics_clone();
    emulator.connect().await.expect("Failed to connect emulator");

    // 5. Set up callback that routes to subscribed strategy runners AND database writer
    let manager_clone = manager.clone();
    let db_sender_clone = db_sender.clone();
    let callback = Arc::new(move |event: StreamEvent| {
        if let StreamEvent::Tick(tick) = event {
            // Convert TickData to NormalizedTick for internal use
            let normalized_tick: data_manager::schema::NormalizedTick = tick.into();

            // Route to strategy runners subscribed to this symbol
            let manager = manager_clone.clone();
            let tick_clone = normalized_tick.clone();
            tokio::spawn(async move {
                manager.route_tick(&tick_clone).await;
            });

            // Send to database writer
            let sender = db_sender_clone.clone();
            tokio::spawn(async move {
                if let Err(e) = sender.send(normalized_tick).await {
                    eprintln!("Failed to send tick to db writer: {}", e);
                }
            });
        }
    });

    // 6. Create shutdown channel
    let (_shutdown_tx, shutdown_rx) = broadcast::channel(1);

    // 7. Start metrics timing
    metrics_collector.start();

    // 8. Run the pipeline
    let subscription = data_manager::provider::LiveSubscription::trades(vec![]);
    let test_result = timeout(
        config.timeout(),
        emulator.subscribe(subscription, callback, shutdown_rx),
    )
    .await;

    if test_result.is_err() {
        println!("Test timed out");
    }

    // 9. Wait for settling time
    tokio::time::sleep(config.settling_time()).await;

    // 10. Stop components
    metrics_collector.stop();
    manager.shutdown_all().await;

    // Drop the sender to signal the writer to finish
    drop(db_sender);
    db_writer.stop().await.expect("Failed to stop db writer");

    let db_metrics = db_writer.metrics().clone();

    // Record end time
    let test_end = Utc::now();

    // 11. Verify database persistence
    println!("\n--- Verifying Database Persistence ---");

    let verifier = DbVerifier::new(&database_url)
        .await
        .expect("Failed to connect to database for verification");

    let stats = verifier
        .get_test_tick_stats("TEST", Some(test_start), Some(test_end))
        .await
        .expect("Failed to get tick stats");

    println!("Database Stats:");
    println!("  Total ticks:     {}", stats.total_count);
    println!("  Distinct symbols: {}", stats.symbol_count);
    println!("  Time range:       {:?} - {:?}", stats.earliest_time, stats.latest_time);

    // Per-symbol breakdown
    println!("\nPer-symbol counts:");
    for (symbol, count) in &stats.symbol_counts {
        println!("  {}: {}", symbol, count);
    }

    // 12. Build results
    let ticks_sent = emulator_metrics.sent_count();
    let strategy_metrics = manager.all_metrics();
    let results = metrics_collector.build_results_with_metrics(
        ticks_generated,
        ticks_sent,
        Some(stats.total_count), // Use verified DB count
        strategy_metrics,
    );

    // Generate and print report
    let report = generate_report(&results, ReportFormat::Pretty);
    println!("{}", report);

    // 13. Assertions
    println!("\n--- Assertions ---");

    // Ticks should have been sent
    assert!(
        ticks_sent > 0,
        "Should have sent some ticks: sent={}",
        ticks_sent
    );
    println!("✓ Ticks sent: {}", ticks_sent);

    // Database writer should have written ticks
    assert!(
        db_metrics.written_count() > 0,
        "Database writer should have written ticks"
    );
    println!("✓ DB writer written: {}", db_metrics.written_count());

    // Database should have persisted ticks
    assert!(
        stats.total_count > 0,
        "Database should have persisted ticks: count={}",
        stats.total_count
    );
    println!("✓ DB persisted: {}", stats.total_count);

    // Verify persistence ratio (allow some loss due to timing)
    let persistence_ratio = stats.total_count as f64 / ticks_sent as f64;
    println!(
        "✓ Persistence ratio: {:.2}% ({} / {})",
        persistence_ratio * 100.0,
        stats.total_count,
        ticks_sent
    );

    // Should persist at least 90% of sent ticks
    assert!(
        persistence_ratio > 0.90,
        "Should persist at least 90% of ticks: {:.2}%",
        persistence_ratio * 100.0
    );

    // 14. Cleanup test data from database
    println!("\n--- Cleanup ---");
    let deleted = verifier
        .cleanup_test_ticks_in_range(test_start, test_end)
        .await
        .expect("Failed to cleanup test ticks");
    println!("Cleaned up {} test ticks from database", deleted);

    verifier.close().await;

    println!("\n=== Database Persistence Test PASSED ===");
}

/// Test database verification only (without writing)
///
/// This test just checks the database verification functionality.
#[tokio::test]
#[ignore = "Requires database connection, run with --ignored"]
async fn test_db_verifier_functionality() {
    use std::env;

    println!("\n=== Testing Database Verifier Functionality ===");

    let database_url = match env::var("DATABASE_URL") {
        Ok(url) => url,
        Err(_) => {
            println!("DATABASE_URL not set, skipping test");
            return;
        }
    };

    // Connect to database
    let verifier = DbVerifier::new(&database_url)
        .await
        .expect("Failed to connect to database");

    // Count all test ticks (should work even if empty)
    let total_count = verifier.count_test_ticks().await.expect("Failed to count ticks");
    println!("Total test ticks in database: {}", total_count);

    // Get stats
    let stats = verifier
        .get_test_tick_stats("TEST", None, None)
        .await
        .expect("Failed to get stats");

    println!("Stats:");
    println!("  Total: {}", stats.total_count);
    println!("  Symbols: {}", stats.symbol_count);

    verifier.close().await;

    println!("✓ Database verifier functionality works");
}
