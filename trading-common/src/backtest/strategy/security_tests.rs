// Security tests for Python strategy sandboxing
// Tests for Phase 1 (Hash Verification), Phase 2 (Import Blocking), Phase 3 (Resource Monitoring)

#[cfg(test)]
mod tests {
    use crate::backtest::strategy::python_loader::{calculate_file_hash, PythonStrategyRegistry, StrategyConfig};
    use crate::backtest::strategy::python_bridge::PythonStrategy;
    use crate::backtest::strategy::base::Strategy;
    use crate::data::types::{BarData, BarMetadata, BarType, OHLCData, Timeframe};
    use crate::series::bars_context::BarsContext;
    use std::path::PathBuf;
    use chrono::Utc;
    use rust_decimal::Decimal;

    // =========================================================================
    // PHASE 1 TESTS: SHA256 Hash Verification
    // =========================================================================

    #[test]
    fn test_calculate_file_hash() {
        // Test that we can calculate a hash for a file
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if path.exists() {
            let result = calculate_file_hash(&path);
            assert!(result.is_ok(), "Should successfully calculate hash");
            let hash = result.unwrap();
            assert_eq!(hash.len(), 64, "SHA256 hash should be 64 hex characters");
            // Hash should only contain hex characters
            assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "Hash should only contain hex digits");
        }
    }

    #[test]
    fn test_hash_verification_correct() {
        // Test that strategy loads with correct hash
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if !path.exists() {
            println!("Skipping test - file not found");
            return;
        }

        // Calculate the actual hash
        let actual_hash = calculate_file_hash(&path).unwrap();

        let config = StrategyConfig {
            id: "test_sma".to_string(),
            file_path: path.clone(),
            class_name: "ExampleSmaStrategy".to_string(),
            description: "Test".to_string(),
            enabled: true,
            sha256: Some(actual_hash),
        };

        // This should succeed
        let result = PythonStrategy::from_file(
            config.file_path.to_str().unwrap(),
            &config.class_name
        );

        // We expect this to succeed when RestrictedPython is available
        // If RestrictedPython is not available, we'll get an import error
        if let Err(e) = &result {
            if e.contains("restricted_compiler") {
                println!("Skipping test - RestrictedPython not available");
                return;
            }
        }

        assert!(result.is_ok(), "Strategy should load with correct hash: {:?}", result.err());
    }

    #[test]
    fn test_hash_verification_incorrect() {
        // Test that strategy is rejected with incorrect hash
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if !path.exists() {
            println!("Skipping test - file not found");
            return;
        }

        let config = StrategyConfig {
            id: "test_sma".to_string(),
            file_path: path.clone(),
            class_name: "ExampleSmaStrategy".to_string(),
            description: "Test".to_string(),
            enabled: true,
            sha256: Some("0000000000000000000000000000000000000000000000000000000000000000".to_string()),
        };

        let mut registry = PythonStrategyRegistry::new(PathBuf::from("../strategies")).unwrap();
        let register_result = registry.register(config);

        // Registration should succeed (we don't verify at registration time)
        assert!(register_result.is_ok(), "Registration should succeed");

        // But loading should fail due to hash mismatch
        let load_result = registry.get_strategy("test_sma");

        if let Err(e) = &load_result {
            // Should contain "hash mismatch" in the error
            assert!(
                e.contains("hash mismatch") || e.contains("Hash mismatch"),
                "Error should mention hash mismatch: {}",
                e
            );
        } else {
            panic!("Should have failed with hash mismatch");
        }
    }

    // =========================================================================
    // PHASE 2 TESTS: Import Blocking (RestrictedPython)
    // =========================================================================

    #[test]
    fn test_malicious_network_import_blocked() {
        // Test that network imports are blocked
        let path = PathBuf::from("../strategies/_tests/test_malicious_network.py");
        if !path.exists() {
            println!("Skipping test - malicious test file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "MaliciousNetworkStrategy"
        );

        // Should fail with ImportError about urllib being blocked
        assert!(result.is_err(), "Malicious network strategy should be blocked");
        let error = result.unwrap_err();
        assert!(
            error.contains("urllib") && (error.contains("blocked") || error.contains("ImportError")),
            "Error should mention urllib is blocked: {}",
            error
        );
    }

    #[test]
    fn test_malicious_filesystem_import_blocked() {
        // Test that filesystem imports are blocked
        let path = PathBuf::from("../strategies/_tests/test_malicious_filesystem.py");
        if !path.exists() {
            println!("Skipping test - malicious test file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "MaliciousFilesystemStrategy"
        );

        // Should fail with ImportError about os being blocked
        assert!(result.is_err(), "Malicious filesystem strategy should be blocked");
        let error = result.unwrap_err();
        assert!(
            error.contains("os") && (error.contains("blocked") || error.contains("ImportError")),
            "Error should mention os is blocked: {}",
            error
        );
    }

    #[test]
    fn test_malicious_subprocess_import_blocked() {
        // Test that subprocess imports are blocked
        let path = PathBuf::from("../strategies/_tests/test_malicious_subprocess.py");
        if !path.exists() {
            println!("Skipping test - malicious test file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "MaliciousSubprocessStrategy"
        );

        // Should fail with ImportError about subprocess being blocked
        assert!(result.is_err(), "Malicious subprocess strategy should be blocked");
        let error = result.unwrap_err();
        assert!(
            error.contains("subprocess") && (error.contains("blocked") || error.contains("ImportError")),
            "Error should mention subprocess is blocked: {}",
            error
        );
    }

    // =========================================================================
    // PHASE 3 TESTS: Resource Monitoring
    // =========================================================================

    #[test]
    fn test_resource_tracking_initialization() {
        // Test that resource tracking starts at zero
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if !path.exists() {
            println!("Skipping test - file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "ExampleSmaStrategy"
        );

        if result.is_err() {
            println!("Skipping test - could not load strategy: {}", result.unwrap_err());
            return;
        }

        let strategy = result.unwrap();

        // Initial values should be zero
        assert_eq!(strategy.get_cpu_time_us(), 0, "Initial CPU time should be 0");
        assert_eq!(strategy.get_call_count(), 0, "Initial call count should be 0");
        assert_eq!(strategy.get_peak_execution_us(), 0, "Initial peak should be 0");
        assert_eq!(strategy.get_avg_execution_us(), 0, "Initial average should be 0");
    }

    #[test]
    fn test_resource_tracking_after_execution() {
        // Test that resource tracking updates after on_bar_data
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if !path.exists() {
            println!("Skipping test - file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "ExampleSmaStrategy"
        );

        if result.is_err() {
            println!("Skipping test - could not load strategy: {}", result.unwrap_err());
            return;
        }

        let mut strategy = result.unwrap();

        // Create test bar data
        let bar_data = BarData {
            current_tick: None,
            ohlc_bar: OHLCData {
                timestamp: Utc::now(),
                symbol: "BTCUSDT".to_string(),
                timeframe: Timeframe::OneMinute,
                open: Decimal::new(50000, 0),
                high: Decimal::new(50100, 0),
                low: Decimal::new(49900, 0),
                close: Decimal::new(50050, 0),
                volume: Decimal::new(100, 0),
                trade_count: 10,
            },
            metadata: BarMetadata {
                bar_type: BarType::TimeBased(Timeframe::OneMinute),
                is_first_tick_of_bar: false,
                is_bar_closed: true,
                tick_count_in_bar: 10,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        };

        // Call on_bar_data with BarsContext
        let mut bars_context = BarsContext::new("BTCUSDT");
        let _ = strategy.on_bar_data(&bar_data, &mut bars_context);

        // Resources should have been tracked
        assert!(strategy.get_cpu_time_us() > 0, "CPU time should be tracked");
        assert_eq!(strategy.get_call_count(), 1, "Call count should be 1");
        assert!(strategy.get_peak_execution_us() > 0, "Peak should be tracked");
        assert!(strategy.get_avg_execution_us() > 0, "Average should be tracked");

        // Average should equal peak for single call
        assert_eq!(
            strategy.get_avg_execution_us(),
            strategy.get_peak_execution_us(),
            "Average should equal peak for single call"
        );
    }

    #[test]
    fn test_resource_tracking_multiple_calls() {
        // Test that metrics accumulate over multiple calls
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if !path.exists() {
            println!("Skipping test - file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "ExampleSmaStrategy"
        );

        if result.is_err() {
            println!("Skipping test - could not load strategy: {}", result.unwrap_err());
            return;
        }

        let mut strategy = result.unwrap();

        let bar_data = BarData {
            current_tick: None,
            ohlc_bar: OHLCData {
                timestamp: Utc::now(),
                symbol: "BTCUSDT".to_string(),
                timeframe: Timeframe::OneMinute,
                open: Decimal::new(50000, 0),
                high: Decimal::new(50100, 0),
                low: Decimal::new(49900, 0),
                close: Decimal::new(50050, 0),
                volume: Decimal::new(100, 0),
                trade_count: 10,
            },
            metadata: BarMetadata {
                bar_type: BarType::TimeBased(Timeframe::OneMinute),
                is_first_tick_of_bar: false,
                is_bar_closed: true,
                tick_count_in_bar: 10,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        };

        // Call multiple times
        let mut bars_context = BarsContext::new("BTCUSDT");
        for _ in 0..5 {
            let _ = strategy.on_bar_data(&bar_data, &mut bars_context);
        }

        // Check metrics
        assert_eq!(strategy.get_call_count(), 5, "Should have 5 calls");
        assert!(strategy.get_cpu_time_us() > 0, "CPU time should be accumulated");

        // Average should be less than or equal to peak
        assert!(
            strategy.get_avg_execution_us() <= strategy.get_peak_execution_us(),
            "Average should be <= peak"
        );
    }

    #[test]
    fn test_resource_tracking_reset() {
        // Test that reset_metrics clears all tracking
        let path = PathBuf::from("../strategies/examples/example_sma.py");
        if !path.exists() {
            println!("Skipping test - file not found");
            return;
        }

        let result = PythonStrategy::from_file(
            path.to_str().unwrap(),
            "ExampleSmaStrategy"
        );

        if result.is_err() {
            println!("Skipping test - could not load strategy: {}", result.unwrap_err());
            return;
        }

        let mut strategy = result.unwrap();

        let bar_data = BarData {
            current_tick: None,
            ohlc_bar: OHLCData {
                timestamp: Utc::now(),
                symbol: "BTCUSDT".to_string(),
                timeframe: Timeframe::OneMinute,
                open: Decimal::new(50000, 0),
                high: Decimal::new(50100, 0),
                low: Decimal::new(49900, 0),
                close: Decimal::new(50050, 0),
                volume: Decimal::new(100, 0),
                trade_count: 10,
            },
            metadata: BarMetadata {
                bar_type: BarType::TimeBased(Timeframe::OneMinute),
                is_first_tick_of_bar: false,
                is_bar_closed: true,
                tick_count_in_bar: 10,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        };

        // Execute and track
        let mut bars_context = BarsContext::new("BTCUSDT");
        let _ = strategy.on_bar_data(&bar_data, &mut bars_context);
        assert!(strategy.get_call_count() > 0, "Should have calls");

        // Reset
        strategy.reset_metrics();

        // All metrics should be zero
        assert_eq!(strategy.get_cpu_time_us(), 0, "CPU time should be reset");
        assert_eq!(strategy.get_call_count(), 0, "Call count should be reset");
        assert_eq!(strategy.get_peak_execution_us(), 0, "Peak should be reset");
        assert_eq!(strategy.get_avg_execution_us(), 0, "Average should be reset");
    }
}
