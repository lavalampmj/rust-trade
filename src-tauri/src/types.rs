use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct DataInfoResponse {
    pub total_records: u64,
    pub symbols_count: u64,
    pub earliest_time: Option<String>,
    pub latest_time: Option<String>,
    pub symbol_info: Vec<SymbolInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SymbolInfo {
    pub symbol: String,
    pub records_count: u64,
    pub earliest_time: Option<String>,
    pub latest_time: Option<String>,
    pub min_price: Option<String>,
    pub max_price: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StrategyInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BacktestRequest {
    pub strategy_id: String,
    pub symbol: String,
    pub data_count: i64,
    pub initial_capital: String,
    pub commission_rate: String,
    pub strategy_params: HashMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BacktestResponse {
    pub strategy_name: String,
    pub initial_capital: String,
    pub final_value: String,
    pub total_pnl: String,
    pub return_percentage: String,
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub max_drawdown: String,
    pub sharpe_ratio: String,
    pub volatility: String,
    pub win_rate: String,
    pub profit_factor: String,
    pub total_commission: String,
    pub trades: Vec<TradeInfo>,
    pub equity_curve: Vec<String>,
    pub data_source: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TradeInfo {
    pub timestamp: String,
    pub symbol: String,
    pub side: String,
    pub quantity: String,
    pub price: String,
    pub realized_pnl: Option<String>,
    pub commission: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoricalDataRequest {
    pub symbol: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TickDataResponse {
    pub timestamp: String,
    pub symbol: String,
    pub price: String,
    pub quantity: String,
    pub side: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StrategyCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bar_data_mode: String,
    pub preferred_bar_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OHLCPreview {
    pub timestamp: String,
    pub symbol: String,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub trade_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OHLCRequest {
    pub symbol: String,
    pub timeframe: String,
    pub count: u32,
}

// ============================================================================
// Configuration Types
// ============================================================================

/// Response containing the full application configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigResponse {
    /// The merged configuration (defaults + user overrides)
    pub config: serde_json::Value,
    /// Whether there are user overrides
    pub has_overrides: bool,
}

/// Request to update a configuration section.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateConfigSectionRequest {
    /// Section path (e.g., "symbols", "accounts.default")
    pub section: String,
    /// New value for the section
    pub value: serde_json::Value,
}

/// Request to add an item to an array section.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct AddConfigItemRequest {
    /// Section path (e.g., "symbols", "accounts.simulation")
    pub section: String,
    /// Item to add
    pub item: serde_json::Value,
}

/// Request to update an item in an array section.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateConfigItemRequest {
    /// Section path
    pub section: String,
    /// Index of the item to update
    pub index: usize,
    /// New value for the item
    pub item: serde_json::Value,
}

/// Request to remove an item from an array section.
#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct RemoveConfigItemRequest {
    /// Section path
    pub section: String,
    /// Index of the item to remove
    pub index: usize,
}

/// Validation result response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationResultResponse {
    /// Whether the configuration is valid
    pub valid: bool,
    /// List of validation errors
    pub errors: Vec<ValidationErrorResponse>,
    /// List of validation warnings
    pub warnings: Vec<ValidationWarningResponse>,
}

/// Validation error.
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationErrorResponse {
    /// Field path
    pub field: String,
    /// Error message
    pub message: String,
    /// Error code
    pub code: String,
}

/// Validation warning.
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidationWarningResponse {
    /// Field path
    pub field: String,
    /// Warning message
    pub message: String,
}

/// Audit log entry.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigAuditEntryResponse {
    /// Timestamp
    pub timestamp: String,
    /// Section that was changed
    pub section: String,
    /// Action performed
    pub action: String,
    /// Previous value
    pub old_value: Option<serde_json::Value>,
    /// New value
    pub new_value: Option<serde_json::Value>,
}
