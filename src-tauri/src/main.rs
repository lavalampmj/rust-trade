#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod state;
mod types;

use commands::*;
use state::AppState;
use trading_common::logging::{init_logging, LogConfig};

fn main() {
    if let Err(_) = dotenvy::dotenv() {
        println!("Warning: .env file not found, using environment variables");
    }

    // Initialize logging with standardized configuration
    // For desktop app, use pretty format by default
    let log_config = LogConfig::from_env()
        .with_app_name("trading-desktop")
        .with_default_level("info");

    if let Err(e) = init_logging(log_config) {
        eprintln!("Warning: Failed to initialize logging: {}", e);
    }

    tracing::info!("Trading Core Tauri Application starting...");

    // Initialize Python strategy system (optional - graceful degradation)
    let config_path = "config/development.toml";
    match trading_common::backtest::strategy::initialize_python_strategies(config_path) {
        Ok(_) => tracing::info!("✓ Python strategy system initialized"),
        Err(e) => {
            tracing::warn!("⚠ Python strategies unavailable: {}", e);
            tracing::warn!("  Rust strategies will still work normally");
        }
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => {
            tracing::info!("Tokio runtime created successfully");
            rt
        }
        Err(e) => {
            tracing::error!("Failed to create Tokio runtime: {}", e);
            std::process::exit(1);
        }
    };

    let app_state = runtime.block_on(async {
        match AppState::new().await {
            Ok(state) => {
                tracing::info!("App state initialized successfully");
                state
            }
            Err(e) => {
                tracing::error!("Failed to initialize app state: {}", e);
                tracing::error!("Please check your configuration:");
                tracing::error!("1. Ensure .env file exists with DATABASE_URL and REDIS_URL");
                tracing::error!("2. Ensure PostgreSQL is running and accessible");
                tracing::error!("3. Ensure Redis is running (optional but recommended)");
                tracing::error!("4. Ensure trading_core database and tick_data table exist");
                std::process::exit(1);
            }
        }
    });

    let result = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_data_info,
            get_available_strategies,
            run_backtest,
            get_historical_data,
            validate_backtest_config,
            get_strategy_capabilities,
            get_ohlc_preview,
            // Configuration commands
            get_config,
            get_config_section,
            update_config_section,
            add_config_item,
            update_config_item,
            remove_config_item,
            reset_config_section,
            validate_config,
            validate_config_section,
            get_config_audit_log,
            save_config,
            reload_config,
            get_config_overrides
        ])
        .setup(|app| {
            tracing::info!("Tauri setup started");
            #[cfg(debug_assertions)]
            {
                let app_handle = app.handle();
                if let Err(e) = app_handle.plugin(tauri_plugin_shell::init()) {
                    tracing::warn!("Failed to initialize shell plugin: {}", e);
                }
                tracing::info!("Debug plugins initialized");
            }
            tracing::info!("Tauri setup completed");
            Ok(())
        })
        .run(tauri::generate_context!());

    match result {
        Ok(_) => tracing::info!("Application exited normally"),
        Err(e) => {
            tracing::error!("Application error: {}", e);
            std::process::exit(1);
        }
    }
}
