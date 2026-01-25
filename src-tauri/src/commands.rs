use crate::state::AppState;
use crate::types::*;
use rust_decimal::Decimal;
use tauri::State;
use trading_common::{
    backtest::{
        engine::{BacktestConfig, BacktestData, BacktestEngine, BacktestResult},
        strategy::create_strategy,
    },
    config::{ConfigAction, ValidationResult},
    data::types::TradeSide,
};

use std::str::FromStr;
use tracing::{error, info, warn};

#[tauri::command]
pub async fn get_data_info(state: State<'_, AppState>) -> Result<DataInfoResponse, String> {
    info!("Getting backtest data info");

    let data_info = state
        .repository
        .get_backtest_data_info()
        .await
        .map_err(|e| {
            error!("Failed to get data info: {}", e);
            e.to_string()
        })?;

    let response = DataInfoResponse {
        total_records: data_info.total_records,
        symbols_count: data_info.symbols_count,
        earliest_time: data_info.earliest_time.map(|t| t.to_rfc3339()),
        latest_time: data_info.latest_time.map(|t| t.to_rfc3339()),
        symbol_info: data_info
            .symbol_info
            .into_iter()
            .map(|info| SymbolInfo {
                symbol: info.symbol,
                records_count: info.records_count,
                earliest_time: info.earliest_time.map(|t| t.to_rfc3339()),
                latest_time: info.latest_time.map(|t| t.to_rfc3339()),
                min_price: info.min_price.map(|p| p.to_string()),
                max_price: info.max_price.map(|p| p.to_string()),
            })
            .collect(),
    };

    info!(
        "Data info retrieved successfully: {} symbols, {} total records",
        response.symbols_count, response.total_records
    );
    Ok(response)
}

#[tauri::command]
pub async fn get_available_strategies() -> Result<Vec<StrategyInfo>, String> {
    info!("Getting available strategies");

    let strategies = trading_common::backtest::strategy::list_strategies();
    let response: Vec<StrategyInfo> = strategies
        .into_iter()
        .map(|s| StrategyInfo {
            id: s.id,
            name: s.name,
            description: s.description,
        })
        .collect();

    info!("Retrieved {} strategies", response.len());
    Ok(response)
}

#[tauri::command]
pub async fn validate_backtest_config(
    state: State<'_, AppState>,
    symbol: String,
    data_count: i64,
) -> Result<bool, String> {
    info!(
        "Validating backtest config for symbol: {}, data_count: {}",
        symbol, data_count
    );

    let data_info = state
        .repository
        .get_backtest_data_info()
        .await
        .map_err(|e| e.to_string())?;

    let is_valid = data_info.has_sufficient_data(&symbol, data_count as u64);
    info!("Validation result: {}", is_valid);

    Ok(is_valid)
}

#[tauri::command]
pub async fn get_historical_data(
    state: State<'_, AppState>,
    request: HistoricalDataRequest,
) -> Result<Vec<TickDataResponse>, String> {
    info!(
        "Getting historical data for symbol: {}, limit: {:?}",
        request.symbol, request.limit
    );

    let limit = request.limit.unwrap_or(1000).min(10000);
    let data = state
        .repository
        .get_recent_ticks_for_backtest(&request.symbol, limit)
        .await
        .map_err(|e| {
            error!("Failed to get historical data: {}", e);
            e.to_string()
        })?;

    let response: Vec<TickDataResponse> = data
        .into_iter()
        .map(|tick| TickDataResponse {
            timestamp: tick.timestamp.to_rfc3339(),
            symbol: tick.symbol,
            price: tick.price.to_string(),
            quantity: tick.quantity.to_string(),
            side: match tick.side {
                TradeSide::Buy => "Buy".to_string(),
                TradeSide::Sell => "Sell".to_string(),
            },
        })
        .collect();

    info!("Retrieved {} historical data points", response.len());
    Ok(response)
}

#[tauri::command]
pub async fn run_backtest(
    state: State<'_, AppState>,
    request: BacktestRequest,
) -> Result<BacktestResponse, String> {
    info!(
        "Starting backtest: strategy={}, symbol={}, data_count={}",
        request.strategy_id, request.symbol, request.data_count
    );

    let initial_capital =
        Decimal::from_str(&request.initial_capital).map_err(|_| "Invalid initial capital")?;
    let commission_rate =
        Decimal::from_str(&request.commission_rate).map_err(|_| "Invalid commission rate")?;

    let mut config = BacktestConfig::new(initial_capital).with_commission_rate(commission_rate);

    for (key, value) in request.strategy_params {
        config = config.with_param(&key, &value);
    }

    // Load tick data for backtest
    info!("Loading tick data for backtest");
    let data = state
        .repository
        .get_recent_ticks_for_backtest(&request.symbol, request.data_count)
        .await
        .map_err(|e| {
            error!("Failed to load historical data: {}", e);
            e.to_string()
        })?;

    if data.is_empty() {
        return Err("No historical data available for the specified symbol".to_string());
    }

    info!(
        "Loaded {} tick data points, running unified backtest",
        data.len()
    );

    let strategy = create_strategy(&request.strategy_id)?;
    let bar_type = strategy.preferred_bar_type();
    let data_source = format!("tick-{}", bar_type.as_str());

    let mut engine = BacktestEngine::new(strategy, config).map_err(|e| {
        error!("Failed to create backtest engine: {}", e);
        e
    })?;

    let result = engine.run_unified(BacktestData::Ticks(data));
    Ok(create_backtest_response(result, data_source))
}

// 3. Add helper function to commands.rs
fn create_backtest_response(result: BacktestResult, data_source: String) -> BacktestResponse {
    info!("Backtest completed successfully");

    BacktestResponse {
        strategy_name: result.strategy_name.clone(),
        initial_capital: result.initial_capital.to_string(),
        final_value: result.final_value.to_string(),
        total_pnl: result.total_pnl.to_string(),
        return_percentage: result.return_percentage.to_string(),
        total_trades: result.total_trades,
        winning_trades: result.winning_trades,
        losing_trades: result.losing_trades,
        max_drawdown: result.max_drawdown.to_string(),
        sharpe_ratio: result.sharpe_ratio.to_string(),
        volatility: result.volatility.to_string(),
        win_rate: result.win_rate.to_string(),
        profit_factor: result.profit_factor.to_string(),
        total_commission: result.total_commission.to_string(),
        data_source, // NEW FIELD
        trades: result
            .trades
            .into_iter()
            .map(|trade| TradeInfo {
                timestamp: trade.timestamp.to_rfc3339(),
                symbol: trade.symbol,
                side: match trade.side {
                    trading_common::data::types::TradeSide::Buy => "Buy".to_string(),
                    trading_common::data::types::TradeSide::Sell => "Sell".to_string(),
                },
                quantity: trade.quantity.to_string(),
                price: trade.price.to_string(),
                realized_pnl: trade.realized_pnl.map(|pnl| pnl.to_string()),
                commission: trade.commission.to_string(),
            })
            .collect(),
        equity_curve: result
            .equity_curve
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
    }
}

#[tauri::command]
pub async fn get_strategy_capabilities() -> Result<Vec<StrategyCapability>, String> {
    info!("Getting strategy capabilities");

    let strategies = trading_common::backtest::strategy::list_strategies();
    let mut capabilities = Vec::new();

    for strategy_info in strategies {
        // Create temporary strategy instance to check capabilities
        match trading_common::backtest::strategy::create_strategy(&strategy_info.id) {
            Ok(strategy) => {
                let mode = match strategy.bar_data_mode() {
                    trading_common::data::types::BarDataMode::OnEachTick => "OnEachTick",
                    trading_common::data::types::BarDataMode::OnPriceMove => "OnPriceMove",
                    trading_common::data::types::BarDataMode::OnCloseBar => "OnCloseBar",
                };
                capabilities.push(StrategyCapability {
                    id: strategy_info.id,
                    name: strategy_info.name,
                    description: strategy_info.description,
                    bar_data_mode: mode.to_string(),
                    preferred_bar_type: strategy.preferred_bar_type().as_str().to_string(),
                });
            }
            Err(e) => {
                info!("Failed to create strategy {}: {}", strategy_info.id, e);
                capabilities.push(StrategyCapability {
                    id: strategy_info.id,
                    name: strategy_info.name,
                    description: strategy_info.description,
                    bar_data_mode: "OnEachTick".to_string(),
                    preferred_bar_type: "TimeBased(1m)".to_string(),
                });
            }
        }
    }

    info!(
        "Retrieved capabilities for {} strategies",
        capabilities.len()
    );
    Ok(capabilities)
}

#[tauri::command]
pub async fn get_ohlc_preview(
    state: State<'_, AppState>,
    request: OHLCRequest,
) -> Result<Vec<OHLCPreview>, String> {
    info!(
        "Getting OHLC preview: {} {} count={}",
        request.symbol, request.timeframe, request.count
    );

    let timeframe = match request.timeframe.as_str() {
        "1m" => trading_common::data::types::Timeframe::OneMinute,
        "5m" => trading_common::data::types::Timeframe::FiveMinutes,
        "15m" => trading_common::data::types::Timeframe::FifteenMinutes,
        "30m" => trading_common::data::types::Timeframe::ThirtyMinutes,
        "1h" => trading_common::data::types::Timeframe::OneHour,
        "4h" => trading_common::data::types::Timeframe::FourHours,
        "1d" => trading_common::data::types::Timeframe::OneDay,
        "1w" => trading_common::data::types::Timeframe::OneWeek,
        _ => return Err(format!("Invalid timeframe: {}", request.timeframe)),
    };

    let ohlc_data = state
        .repository
        .generate_recent_ohlc_for_backtest(&request.symbol, timeframe, request.count)
        .await
        .map_err(|e| {
            error!("Failed to generate OHLC preview: {}", e);
            e.to_string()
        })?;

    if ohlc_data.is_empty() {
        return Err("No OHLC data available for the specified parameters".to_string());
    }

    let response: Vec<OHLCPreview> = ohlc_data
        .into_iter()
        .map(|ohlc| OHLCPreview {
            timestamp: ohlc.timestamp.to_rfc3339(),
            symbol: ohlc.symbol,
            open: ohlc.open.to_string(),
            high: ohlc.high.to_string(),
            low: ohlc.low.to_string(),
            close: ohlc.close.to_string(),
            volume: ohlc.volume.to_string(),
            trade_count: ohlc.trade_count,
        })
        .collect();

    info!("Generated {} OHLC preview records", response.len());
    Ok(response)
}

// ============================================================================
// Configuration Commands
// ============================================================================

/// Get the full merged configuration.
#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<ConfigResponse, String> {
    info!("Getting full configuration");

    let config = state.config_service.get_config();
    let config_json = serde_json::to_value(&config).map_err(|e| e.to_string())?;

    Ok(ConfigResponse {
        config: config_json,
        has_overrides: state.config_service.has_overrides(),
    })
}

/// Get a specific configuration section.
#[tauri::command]
pub async fn get_config_section(
    section: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    info!("Getting config section: {}", section);

    state
        .config_service
        .get_section(&section)
        .map_err(|e| e.to_string())
}

/// Update a configuration section.
#[tauri::command]
pub async fn update_config_section(
    section: String,
    value: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Updating config section: {}", section);

    state
        .config_service
        .update_section(&section, value)
        .map_err(|e| e.to_string())?;

    // Auto-save after update
    if let Err(e) = state.config_service.save() {
        warn!("Failed to auto-save config: {}", e);
        // Don't fail the update, just log warning
    }

    Ok(())
}

/// Add an item to an array section.
#[tauri::command]
pub async fn add_config_item(
    section: String,
    item: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Adding item to config section: {}", section);

    state
        .config_service
        .add_item(&section, item)
        .map_err(|e| e.to_string())?;

    // Auto-save after update
    if let Err(e) = state.config_service.save() {
        warn!("Failed to auto-save config: {}", e);
    }

    Ok(())
}

/// Update an item in an array section.
#[tauri::command]
pub async fn update_config_item(
    section: String,
    index: usize,
    item: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Updating item {} in config section: {}", index, section);

    state
        .config_service
        .update_item(&section, index, item)
        .map_err(|e| e.to_string())?;

    // Auto-save after update
    if let Err(e) = state.config_service.save() {
        warn!("Failed to auto-save config: {}", e);
    }

    Ok(())
}

/// Remove an item from an array section.
#[tauri::command]
pub async fn remove_config_item(
    section: String,
    index: usize,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Removing item {} from config section: {}", index, section);

    state
        .config_service
        .remove_item(&section, index)
        .map_err(|e| e.to_string())?;

    // Auto-save after update
    if let Err(e) = state.config_service.save() {
        warn!("Failed to auto-save config: {}", e);
    }

    Ok(())
}

/// Reset a configuration section to its default value.
#[tauri::command]
pub async fn reset_config_section(
    section: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    info!("Resetting config section: {}", section);

    state
        .config_service
        .reset_section(&section)
        .map_err(|e| e.to_string())?;

    // Auto-save after reset
    if let Err(e) = state.config_service.save() {
        warn!("Failed to auto-save config: {}", e);
    }

    Ok(())
}

/// Validate the current configuration.
#[tauri::command]
pub async fn validate_config(state: State<'_, AppState>) -> Result<ValidationResultResponse, String> {
    info!("Validating configuration");

    let result = state.config_service.validate();
    Ok(convert_validation_result(result))
}

/// Validate a specific section value.
#[tauri::command]
pub async fn validate_config_section(
    section: String,
    value: serde_json::Value,
    state: State<'_, AppState>,
) -> Result<ValidationResultResponse, String> {
    info!("Validating config section: {}", section);

    let result = state.config_service.validate_section(&section, &value);
    Ok(convert_validation_result(result))
}

/// Get the configuration audit log.
#[tauri::command]
pub async fn get_config_audit_log(
    state: State<'_, AppState>,
) -> Result<Vec<ConfigAuditEntryResponse>, String> {
    info!("Getting config audit log");

    let log = state.config_service.get_audit_log();
    let response: Vec<ConfigAuditEntryResponse> = log
        .into_iter()
        .map(|entry| ConfigAuditEntryResponse {
            timestamp: entry.timestamp.to_rfc3339(),
            section: entry.section,
            action: match entry.action {
                ConfigAction::Update => "update".to_string(),
                ConfigAction::Add => "add".to_string(),
                ConfigAction::Remove => "remove".to_string(),
                ConfigAction::Reset => "reset".to_string(),
            },
            old_value: entry.old_value,
            new_value: entry.new_value,
        })
        .collect();

    Ok(response)
}

/// Save configuration to disk (usually auto-saved, but can force save).
#[tauri::command]
pub async fn save_config(state: State<'_, AppState>) -> Result<(), String> {
    info!("Saving configuration to disk");

    state.config_service.save().map_err(|e| e.to_string())
}

/// Reload configuration from disk.
#[tauri::command]
pub async fn reload_config(state: State<'_, AppState>) -> Result<(), String> {
    info!("Reloading configuration from disk");

    state.config_service.reload().map_err(|e| e.to_string())
}

/// Get user overrides (for debugging).
#[tauri::command]
pub async fn get_config_overrides(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    info!("Getting config overrides");

    Ok(state.config_service.get_overrides())
}

// Helper function to convert ValidationResult to response type
fn convert_validation_result(result: ValidationResult) -> ValidationResultResponse {
    ValidationResultResponse {
        valid: result.valid,
        errors: result
            .errors
            .into_iter()
            .map(|e| ValidationErrorResponse {
                field: e.field,
                message: e.message,
                code: format!("{:?}", e.code),
            })
            .collect(),
        warnings: result
            .warnings
            .into_iter()
            .map(|w| ValidationWarningResponse {
                field: w.field,
                message: w.message,
            })
            .collect(),
    }
}
