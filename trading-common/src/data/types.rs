use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::dbn_types::{
    create_trade_msg_from_decimals, symbol_to_instrument_id, TradeMsg, TradeMsgExt, TradeSideCompat,
};

// =================================================================
// Core data type: completely corresponds to the tick_data table structure
// =================================================================

/// Trading direction enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TradeSide {
    Buy,
    Sell,
}

/// Standard trading data structure - corresponds one-to-one with the market_ticks table fields
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TickData {
    /// Event timestamp (ts_event in database)
    pub timestamp: DateTime<Utc>,

    /// Receive timestamp (ts_recv in database)
    #[serde(default = "Utc::now")]
    pub ts_recv: DateTime<Utc>,

    /// Trading pair, such as "BTCUSDT"
    pub symbol: String,

    /// Exchange name (e.g., "BINANCE", "DATABENTO")
    #[serde(default = "default_exchange")]
    pub exchange: String,

    /// Trading price
    pub price: Decimal,

    /// Trading quantity (size in database)
    pub quantity: Decimal,

    /// Trading direction
    pub side: TradeSide,

    /// Data provider (e.g., "BINANCE", "DATABENTO")
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Provider's original transaction ID (provider_trade_id in database)
    pub trade_id: String,

    /// Whether the buyer is the maker
    pub is_buyer_maker: bool,

    /// Sequence number for ordering
    #[serde(default)]
    pub sequence: i64,

    /// Raw DBN message bytes for archival (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_dbn: Option<Vec<u8>>,
}

fn default_exchange() -> String {
    "UNKNOWN".to_string()
}

fn default_provider() -> String {
    "UNKNOWN".to_string()
}

impl TickData {
    /// New TickData with minimal required fields (uses defaults for exchange/provider/sequence)
    pub fn new(
        timestamp: DateTime<Utc>,
        symbol: String,
        price: Decimal,
        quantity: Decimal,
        side: TradeSide,
        trade_id: String,
        is_buyer_maker: bool,
    ) -> Self {
        Self {
            timestamp,
            ts_recv: timestamp, // Default to same as event time
            symbol,
            exchange: "UNKNOWN".to_string(),
            price,
            quantity,
            side,
            provider: "UNKNOWN".to_string(),
            trade_id,
            is_buyer_maker,
            sequence: 0,
            raw_dbn: None,
        }
    }

    /// Create TickData with all fields specified
    pub fn with_details(
        timestamp: DateTime<Utc>,
        ts_recv: DateTime<Utc>,
        symbol: String,
        exchange: String,
        price: Decimal,
        quantity: Decimal,
        side: TradeSide,
        provider: String,
        trade_id: String,
        is_buyer_maker: bool,
        sequence: i64,
    ) -> Self {
        Self {
            timestamp,
            ts_recv,
            symbol,
            exchange,
            price,
            quantity,
            side,
            provider,
            trade_id,
            is_buyer_maker,
            sequence,
            raw_dbn: None,
        }
    }

    /// Create TickData with raw DBN bytes for archival
    pub fn with_raw_dbn(
        timestamp: DateTime<Utc>,
        ts_recv: DateTime<Utc>,
        symbol: String,
        exchange: String,
        price: Decimal,
        quantity: Decimal,
        side: TradeSide,
        provider: String,
        trade_id: String,
        is_buyer_maker: bool,
        sequence: i64,
        raw_dbn: Vec<u8>,
    ) -> Self {
        Self {
            timestamp,
            ts_recv,
            symbol,
            exchange,
            price,
            quantity,
            side,
            provider,
            trade_id,
            is_buyer_maker,
            sequence,
            raw_dbn: Some(raw_dbn),
        }
    }

    /// Create TickData without validation (for trusted sources like database reads)
    pub fn new_unchecked(
        timestamp: DateTime<Utc>,
        symbol: String,
        price: Decimal,
        quantity: Decimal,
        side: TradeSide,
        trade_id: String,
        is_buyer_maker: bool,
    ) -> Self {
        Self::new(
            timestamp,
            symbol,
            price,
            quantity,
            side,
            trade_id,
            is_buyer_maker,
        )
    }
}

// =================================================================
// Helper Types
// =================================================================

/// Database statistics
#[derive(Debug, Clone)]
pub struct DbStats {
    pub symbol: Option<String>,
    pub total_records: u64,
    pub earliest_timestamp: Option<DateTime<Utc>>,
    pub latest_timestamp: Option<DateTime<Utc>>,
}

// =================================================================
// TradeSide Implementation for Database Integration
// =================================================================

impl TradeSide {
    /// Convert to database string representation (single char: 'B' or 'S')
    pub fn as_db_str(&self) -> &'static str {
        match self {
            TradeSide::Buy => "B",
            TradeSide::Sell => "S",
        }
    }

    /// Convert to database char representation
    pub fn as_db_char(&self) -> char {
        match self {
            TradeSide::Buy => 'B',
            TradeSide::Sell => 'S',
        }
    }

    /// Parse from database char ('B' or 'S')
    pub fn from_db_char(c: char) -> Option<Self> {
        match c {
            'B' => Some(TradeSide::Buy),
            'S' => Some(TradeSide::Sell),
            _ => None,
        }
    }

    /// Parse from database string ('B', 'S', 'BUY', 'SELL')
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "B" | "BUY" => Some(TradeSide::Buy),
            "S" | "SELL" => Some(TradeSide::Sell),
            _ => None,
        }
    }

    /// Convert to DBN-compatible TradeSideCompat
    pub fn to_dbn_side(&self) -> TradeSideCompat {
        match self {
            TradeSide::Buy => TradeSideCompat::Buy,
            TradeSide::Sell => TradeSideCompat::Sell,
        }
    }

    /// Convert from DBN-compatible TradeSideCompat
    pub fn from_dbn_side(side: TradeSideCompat) -> Self {
        match side {
            TradeSideCompat::Buy => TradeSide::Buy,
            TradeSideCompat::Sell | TradeSideCompat::None => TradeSide::Sell,
        }
    }
}

// =================================================================
// Conversion between TickData and TradeMsg (DBN format)
// =================================================================

impl TickData {
    /// Convert TickData to TradeMsg (DBN format)
    ///
    /// Note: Some information is lost in this conversion:
    /// - trade_id is not stored in TradeMsg
    /// - is_buyer_maker is encoded in flags
    /// - exchange defaults to "UNKNOWN" (can be overridden with to_trade_msg_with_exchange)
    pub fn to_trade_msg(&self) -> TradeMsg {
        self.to_trade_msg_with_exchange("UNKNOWN")
    }

    /// Convert TickData to TradeMsg with explicit exchange
    pub fn to_trade_msg_with_exchange(&self, exchange: &str) -> TradeMsg {
        let ts_nanos = self.timestamp.timestamp_nanos_opt().unwrap_or(0) as u64;
        create_trade_msg_from_decimals(
            ts_nanos,
            ts_nanos, // Use same timestamp for recv
            &self.symbol,
            exchange,
            self.price,
            self.quantity,
            self.side.to_dbn_side(),
            // Use trade_id hash as sequence if it can be parsed, otherwise 0
            self.trade_id.parse::<u32>().unwrap_or(0),
        )
    }

    /// Create TickData from TradeMsg
    ///
    /// Requires symbol lookup since TradeMsg only contains instrument_id
    pub fn from_trade_msg(msg: &TradeMsg, symbol: String) -> Self {
        Self::from_trade_msg_with_exchange(msg, symbol, "UNKNOWN".to_string())
    }

    /// Create TickData from TradeMsg with explicit exchange
    pub fn from_trade_msg_with_exchange(msg: &TradeMsg, symbol: String, exchange: String) -> Self {
        let ts = msg.timestamp();
        TickData {
            timestamp: ts,
            ts_recv: ts, // Use same timestamp for recv
            symbol,
            exchange: exchange.clone(),
            price: msg.price_decimal(),
            quantity: msg.size_decimal(),
            side: TradeSide::from_dbn_side(msg.trade_side()),
            provider: exchange, // Use exchange as provider
            trade_id: msg.sequence.to_string(),
            is_buyer_maker: msg.side as u8 as char == 'A', // Ask side = buyer is maker
            sequence: msg.sequence as i64,
            raw_dbn: None,
        }
    }

    /// Get the full symbol identifier (symbol@exchange)
    pub fn full_symbol(&self) -> String {
        format!("{}@{}", self.symbol, self.exchange)
    }
}

/// Symbol registry for TradeMsg <-> TickData conversion
///
/// Since TradeMsg uses instrument_id instead of symbol string,
/// we need a way to look up symbols. This is a simple in-memory
/// registry that can be populated during system initialization.
#[derive(Debug, Clone, Default)]
pub struct SymbolRegistry {
    /// Maps instrument_id to (symbol, exchange) pairs
    id_to_symbol: std::collections::HashMap<u32, (String, String)>,
}

impl SymbolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a symbol/exchange pair and return its instrument_id
    pub fn register(&mut self, symbol: &str, exchange: &str) -> u32 {
        let id = symbol_to_instrument_id(symbol, exchange);
        self.id_to_symbol
            .insert(id, (symbol.to_string(), exchange.to_string()));
        id
    }

    /// Look up symbol by instrument_id
    pub fn get_symbol(&self, instrument_id: u32) -> Option<&(String, String)> {
        self.id_to_symbol.get(&instrument_id)
    }

    /// Convert TradeMsg to TickData using this registry
    pub fn trade_msg_to_tick_data(&self, msg: &TradeMsg) -> Option<TickData> {
        self.id_to_symbol
            .get(&msg.hd.instrument_id)
            .map(|(symbol, _)| TickData::from_trade_msg(msg, symbol.clone()))
    }
}

// =================================================================
// Query parameter type
// =================================================================

/// TickData Query parameters
#[derive(Debug, Clone)]
pub struct TickQuery {
    pub symbol: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub trade_side: Option<TradeSide>,
}

impl TickQuery {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
            start_time: None,
            end_time: None,
            limit: None,
            trade_side: None,
        }
    }
}

// =================================================================
// Error type definition
// =================================================================

#[derive(Error, Debug)]
pub enum DataError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Invalid data format: {0}")]
    InvalidFormat(String),

    #[error("Data not found: {0}")]
    NotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Decimal conversion error: {0}")]
    DecimalConversion(#[from] rust_decimal::Error),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Configuration error: {0}")]
    Config(String),
}

pub type DataResult<T> = Result<T, DataError>;

/// Backtest data information for user selection
#[derive(Debug, Clone)]
pub struct BacktestDataInfo {
    pub total_records: u64,
    pub symbols_count: u64,
    pub earliest_time: Option<DateTime<Utc>>,
    pub latest_time: Option<DateTime<Utc>>,
    pub symbol_info: Vec<SymbolDataInfo>,
}

/// Per-symbol data information
#[derive(Debug, Clone)]
pub struct SymbolDataInfo {
    pub symbol: String,
    pub records_count: u64,
    pub earliest_time: Option<DateTime<Utc>>,
    pub latest_time: Option<DateTime<Utc>>,
    pub min_price: Option<Decimal>,
    pub max_price: Option<Decimal>,
}

impl BacktestDataInfo {
    /// Get information for a specific symbol
    pub fn get_symbol_info(&self, symbol: &str) -> Option<&SymbolDataInfo> {
        self.symbol_info.iter().find(|info| info.symbol == symbol)
    }

    /// Get available symbols
    pub fn get_available_symbols(&self) -> Vec<String> {
        self.symbol_info
            .iter()
            .map(|info| info.symbol.clone())
            .collect()
    }

    /// Check if has sufficient data for backtesting
    pub fn has_sufficient_data(&self, symbol: &str, min_records: u64) -> bool {
        self.get_symbol_info(symbol)
            .map(|info| info.records_count >= min_records)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveStrategyLog {
    pub timestamp: DateTime<Utc>,
    pub strategy_id: String,
    pub symbol: String,
    pub current_price: Decimal,
    pub signal_type: String, // BUY/SELL/HOLD
    pub portfolio_value: Decimal,
    pub total_pnl: Decimal,
    pub cache_hit: bool,
    pub processing_time_us: u64,
}

/// Time frame for OHLC data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Timeframe {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    ThirtyMinutes,
    OneHour,
    FourHours,
    OneDay,
    OneWeek,
}

/// Bar type for OHLC data generation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarType {
    /// Time-based bars using timeframe windows (e.g., 1m, 5m, 1h)
    TimeBased(Timeframe),
    /// Tick-based bars - creates one bar every N ticks (e.g., 100-tick bars)
    TickBased(u32),
    // Future enhancement: Volume-based bars
    // VolumeBased(Decimal),
}

impl BarType {
    /// Get a string representation of the bar type
    pub fn as_str(&self) -> String {
        match self {
            BarType::TimeBased(timeframe) => timeframe.as_str().to_string(),
            BarType::TickBased(n) => format!("{}T", n),
        }
    }

    /// Check if this is a time-based bar
    pub fn is_time_based(&self) -> bool {
        matches!(self, BarType::TimeBased(_))
    }

    /// Check if this is a tick-based bar
    pub fn is_tick_based(&self) -> bool {
        matches!(self, BarType::TickBased(_))
    }
}

impl Timeframe {
    pub fn as_duration(&self) -> Duration {
        match self {
            Timeframe::OneMinute => Duration::minutes(1),
            Timeframe::FiveMinutes => Duration::minutes(5),
            Timeframe::FifteenMinutes => Duration::minutes(15),
            Timeframe::ThirtyMinutes => Duration::minutes(30),
            Timeframe::OneHour => Duration::hours(1),
            Timeframe::FourHours => Duration::hours(4),
            Timeframe::OneDay => Duration::days(1),
            Timeframe::OneWeek => Duration::weeks(1),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Timeframe::OneMinute => "1m",
            Timeframe::FiveMinutes => "5m",
            Timeframe::FifteenMinutes => "15m",
            Timeframe::ThirtyMinutes => "30m",
            Timeframe::OneHour => "1h",
            Timeframe::FourHours => "4h",
            Timeframe::OneDay => "1d",
            Timeframe::OneWeek => "1w",
        }
    }

    /// Get the start of the time window for a given timestamp
    pub fn align_timestamp(&self, timestamp: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Timeframe::OneMinute => timestamp
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
            Timeframe::FiveMinutes => {
                let aligned_minute = (timestamp.minute() / 5) * 5;
                timestamp
                    .with_minute(aligned_minute)
                    .unwrap()
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap()
            }
            Timeframe::FifteenMinutes => {
                let aligned_minute = (timestamp.minute() / 15) * 15;
                timestamp
                    .with_minute(aligned_minute)
                    .unwrap()
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap()
            }
            Timeframe::ThirtyMinutes => {
                let aligned_minute = (timestamp.minute() / 30) * 30;
                timestamp
                    .with_minute(aligned_minute)
                    .unwrap()
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap()
            }
            Timeframe::OneHour => timestamp
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
            Timeframe::FourHours => {
                let aligned_hour = (timestamp.hour() / 4) * 4;
                timestamp
                    .with_hour(aligned_hour)
                    .unwrap()
                    .with_minute(0)
                    .unwrap()
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap()
            }
            Timeframe::OneDay => timestamp
                .with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap(),
            Timeframe::OneWeek => {
                let days_from_monday = timestamp.weekday().num_days_from_monday();
                let week_start = timestamp - Duration::days(days_from_monday as i64);
                week_start
                    .with_hour(0)
                    .unwrap()
                    .with_minute(0)
                    .unwrap()
                    .with_second(0)
                    .unwrap()
                    .with_nanosecond(0)
                    .unwrap()
            }
        }
    }
}

/// OHLC data structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OHLCData {
    pub timestamp: DateTime<Utc>,
    pub symbol: String,
    pub timeframe: Timeframe,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: Decimal,
    pub trade_count: u64,
}

impl OHLCData {
    pub fn new(
        timestamp: DateTime<Utc>,
        symbol: String,
        timeframe: Timeframe,
        open: Decimal,
        high: Decimal,
        low: Decimal,
        close: Decimal,
        volume: Decimal,
        trade_count: u64,
    ) -> Self {
        Self {
            timestamp,
            symbol,
            timeframe,
            open,
            high,
            low,
            close,
            volume,
            trade_count,
        }
    }

    /// Create OHLC from a collection of tick data (time-based bars)
    pub fn from_ticks(
        ticks: &[TickData],
        timeframe: Timeframe,
        window_start: DateTime<Utc>,
    ) -> Option<Self> {
        if ticks.is_empty() {
            return None;
        }

        let symbol = ticks[0].symbol.clone();
        let open = ticks[0].price;
        let mut high = ticks[0].price;
        let mut low = ticks[0].price;
        let close = ticks[ticks.len() - 1].price;
        let mut volume = Decimal::ZERO;

        for tick in ticks {
            if tick.price > high {
                high = tick.price;
            }
            if tick.price < low {
                low = tick.price;
            }
            volume += tick.quantity;
        }

        Some(OHLCData::new(
            window_start,
            symbol,
            timeframe,
            open,
            high,
            low,
            close,
            volume,
            ticks.len() as u64,
        ))
    }

    /// Create N-tick OHLC bars from a collection of tick data
    ///
    /// Splits ticks into chunks of size `tick_count` and creates one OHLC bar per chunk.
    /// The timestamp of each bar is the timestamp of the first tick in that chunk.
    ///
    /// # Arguments
    /// * `ticks` - Slice of tick data (should be sorted by timestamp)
    /// * `tick_count` - Number of ticks per bar (e.g., 100 for 100-tick bars)
    ///
    /// # Returns
    /// Vector of OHLC bars, one per N-tick chunk
    pub fn from_ticks_n_tick(ticks: &[TickData], tick_count: u32) -> Vec<Self> {
        if ticks.is_empty() || tick_count == 0 {
            return Vec::new();
        }

        let tick_count_usize = tick_count as usize;
        let mut result = Vec::new();

        // Process ticks in chunks of tick_count
        for chunk in ticks.chunks(tick_count_usize) {
            if chunk.is_empty() {
                continue;
            }

            let symbol = chunk[0].symbol.clone();
            let timestamp = chunk[0].timestamp; // Use first tick's timestamp as bar timestamp
            let open = chunk[0].price;
            let close = chunk[chunk.len() - 1].price;
            let mut high = chunk[0].price;
            let mut low = chunk[0].price;
            let mut volume = Decimal::ZERO;

            // Calculate high, low, and volume
            for tick in chunk {
                if tick.price > high {
                    high = tick.price;
                }
                if tick.price < low {
                    low = tick.price;
                }
                volume += tick.quantity;
            }

            // For N-tick bars, timeframe is not meaningful, so we use OneMinute as placeholder
            // In the future, this could be refactored to use BarType instead
            let ohlc = OHLCData::new(
                timestamp,
                symbol,
                Timeframe::OneMinute, // Placeholder for tick-based bars
                open,
                high,
                low,
                close,
                volume,
                chunk.len() as u64,
            );

            result.push(ohlc);
        }

        result
    }
}

// =================================================================
// Unified Bar Data Types (for on_bar_data strategy interface)
// =================================================================

/// Operational mode for strategy bar data processing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BarDataMode {
    /// Fire on every tick (both current_tick and ohlc_bar available)
    OnEachTick,
    /// Fire only when price changes from previous tick
    OnPriceMove,
    /// Fire only when bar closes (current_tick = None, only ohlc_bar)
    OnCloseBar,
}

/// Metadata about bar generation and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarMetadata {
    /// Type of bar (time-based or tick-based)
    pub bar_type: BarType,
    /// True if this is the first tick of a new bar
    pub is_first_tick_of_bar: bool,
    /// True if the bar has closed (complete)
    pub is_bar_closed: bool,
    /// Number of ticks accumulated in this bar
    pub tick_count_in_bar: u64,
    /// True if bar was synthetically generated (no ticks during interval)
    pub is_synthetic: bool,
    /// When this bar data was generated
    pub generation_timestamp: DateTime<Utc>,
    /// True if bar was truncated early due to session close
    /// (e.g., 500-tick bar closed at tick 300 because session ended)
    #[serde(default)]
    pub is_session_truncated: bool,
    /// True if bar start time was aligned to session open
    /// (rather than first tick arrival time)
    #[serde(default)]
    pub is_session_aligned: bool,
}

/// Unified bar data structure for strategy processing
///
/// Combines current tick information with OHLC bar state.
/// Supports three operational modes:
/// - OnEachTick: current_tick is Some, fires on every tick
/// - OnPriceMove: current_tick is Some, fires only when price changes
/// - OnCloseBar: current_tick is None, fires only when bar closes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarData {
    /// Current tick that triggered this event (None for OnCloseBar mode)
    pub current_tick: Option<TickData>,
    /// Current state of the OHLC bar (always present)
    pub ohlc_bar: OHLCData,
    /// Metadata about bar generation
    pub metadata: BarMetadata,
}

impl BarData {
    /// Create BarData from a single tick (for OnEachTick/OnPriceMove modes)
    ///
    /// Creates a minimal OHLC bar where O=H=L=C=tick.price
    pub fn from_single_tick(tick: &TickData) -> Self {
        let ohlc = OHLCData::new(
            tick.timestamp,
            tick.symbol.clone(),
            Timeframe::OneMinute, // Placeholder
            tick.price,
            tick.price,
            tick.price,
            tick.price,
            tick.quantity,
            1,
        );

        BarData {
            current_tick: Some(tick.clone()),
            ohlc_bar: ohlc,
            metadata: BarMetadata {
                bar_type: BarType::TimeBased(Timeframe::OneMinute),
                is_first_tick_of_bar: true,
                is_bar_closed: false,
                tick_count_in_bar: 1,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        }
    }

    /// Create BarData from an OHLC bar (for OnCloseBar mode)
    ///
    /// Used when bar is complete and no current tick reference needed
    pub fn from_ohlc(ohlc: &OHLCData) -> Self {
        BarData {
            current_tick: None,
            ohlc_bar: ohlc.clone(),
            metadata: BarMetadata {
                bar_type: BarType::TimeBased(ohlc.timeframe),
                is_first_tick_of_bar: false,
                is_bar_closed: true,
                tick_count_in_bar: ohlc.trade_count,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        }
    }

    /// Create a synthetic bar (generated when no ticks during time interval)
    ///
    /// All OHLC values = last_known_price
    pub fn synthetic_bar(
        symbol: String,
        bar_type: BarType,
        timestamp: DateTime<Utc>,
        last_known_price: Decimal,
    ) -> Self {
        let timeframe = match bar_type {
            BarType::TimeBased(tf) => tf,
            BarType::TickBased(_) => Timeframe::OneMinute, // Placeholder
        };

        let ohlc = OHLCData::new(
            timestamp,
            symbol,
            timeframe,
            last_known_price,
            last_known_price,
            last_known_price,
            last_known_price,
            Decimal::ZERO,
            0,
        );

        BarData {
            current_tick: None,
            ohlc_bar: ohlc,
            metadata: BarMetadata {
                bar_type,
                is_first_tick_of_bar: false,
                is_bar_closed: true,
                tick_count_in_bar: 0,
                is_synthetic: true,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        }
    }

    /// Create BarData with tick and OHLC state (primary constructor for realtime)
    pub fn new(
        current_tick: Option<TickData>,
        ohlc_bar: OHLCData,
        bar_type: BarType,
        is_first_tick: bool,
        is_closed: bool,
        tick_count: u64,
    ) -> Self {
        BarData {
            current_tick,
            ohlc_bar,
            metadata: BarMetadata {
                bar_type,
                is_first_tick_of_bar: is_first_tick,
                is_bar_closed: is_closed,
                tick_count_in_bar: tick_count,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated: false,
                is_session_aligned: false,
            },
        }
    }

    /// Create BarData with session-aware metadata
    ///
    /// Used when bar is truncated due to session close or aligned to session open
    pub fn new_session_aware(
        current_tick: Option<TickData>,
        ohlc_bar: OHLCData,
        bar_type: BarType,
        is_first_tick: bool,
        is_closed: bool,
        tick_count: u64,
        is_session_truncated: bool,
        is_session_aligned: bool,
    ) -> Self {
        BarData {
            current_tick,
            ohlc_bar,
            metadata: BarMetadata {
                bar_type,
                is_first_tick_of_bar: is_first_tick,
                is_bar_closed: is_closed,
                tick_count_in_bar: tick_count,
                is_synthetic: false,
                generation_timestamp: Utc::now(),
                is_session_truncated,
                is_session_aligned,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn create_test_tick(
        symbol: &str,
        price: &str,
        quantity: &str,
        timestamp_offset_secs: i64,
    ) -> TickData {
        let ts = Utc::now() + Duration::seconds(timestamp_offset_secs);
        TickData {
            timestamp: ts,
            ts_recv: ts,
            symbol: symbol.to_string(),
            exchange: "TEST".to_string(),
            price: Decimal::from_str(price).unwrap(),
            quantity: Decimal::from_str(quantity).unwrap(),
            side: TradeSide::Buy,
            provider: "TEST".to_string(),
            trade_id: format!("trade_{}", timestamp_offset_secs),
            is_buyer_maker: false,
            sequence: timestamp_offset_secs,
            raw_dbn: None,
        }
    }

    #[test]
    fn test_bar_type_as_str() {
        let time_based = BarType::TimeBased(Timeframe::OneMinute);
        assert_eq!(time_based.as_str(), "1m");

        let time_based_hour = BarType::TimeBased(Timeframe::OneHour);
        assert_eq!(time_based_hour.as_str(), "1h");

        let tick_based = BarType::TickBased(100);
        assert_eq!(tick_based.as_str(), "100T");

        let tick_based_50 = BarType::TickBased(50);
        assert_eq!(tick_based_50.as_str(), "50T");
    }

    #[test]
    fn test_bar_type_checks() {
        let time_based = BarType::TimeBased(Timeframe::OneMinute);
        assert!(time_based.is_time_based());
        assert!(!time_based.is_tick_based());

        let tick_based = BarType::TickBased(100);
        assert!(tick_based.is_tick_based());
        assert!(!tick_based.is_time_based());
    }

    #[test]
    fn test_from_ticks_n_tick_basic() {
        // Create 5 ticks
        let ticks = vec![
            create_test_tick("BTCUSDT", "50000", "1.0", 0),
            create_test_tick("BTCUSDT", "50100", "1.5", 1),
            create_test_tick("BTCUSDT", "49900", "2.0", 2),
            create_test_tick("BTCUSDT", "50200", "1.2", 3),
            create_test_tick("BTCUSDT", "50300", "0.8", 4),
        ];

        // Create 2-tick bars
        let bars = OHLCData::from_ticks_n_tick(&ticks, 2);

        // Should create 3 bars: [0,1], [2,3], [4]
        assert_eq!(bars.len(), 3);

        // First bar: ticks 0-1
        assert_eq!(bars[0].symbol, "BTCUSDT");
        assert_eq!(bars[0].open, Decimal::from_str("50000").unwrap());
        assert_eq!(bars[0].high, Decimal::from_str("50100").unwrap());
        assert_eq!(bars[0].low, Decimal::from_str("50000").unwrap());
        assert_eq!(bars[0].close, Decimal::from_str("50100").unwrap());
        assert_eq!(bars[0].volume, Decimal::from_str("2.5").unwrap());
        assert_eq!(bars[0].trade_count, 2);

        // Second bar: ticks 2-3
        assert_eq!(bars[1].open, Decimal::from_str("49900").unwrap());
        assert_eq!(bars[1].high, Decimal::from_str("50200").unwrap());
        assert_eq!(bars[1].low, Decimal::from_str("49900").unwrap());
        assert_eq!(bars[1].close, Decimal::from_str("50200").unwrap());
        assert_eq!(bars[1].volume, Decimal::from_str("3.2").unwrap());
        assert_eq!(bars[1].trade_count, 2);

        // Third bar: tick 4 (incomplete chunk)
        assert_eq!(bars[2].open, Decimal::from_str("50300").unwrap());
        assert_eq!(bars[2].high, Decimal::from_str("50300").unwrap());
        assert_eq!(bars[2].low, Decimal::from_str("50300").unwrap());
        assert_eq!(bars[2].close, Decimal::from_str("50300").unwrap());
        assert_eq!(bars[2].volume, Decimal::from_str("0.8").unwrap());
        assert_eq!(bars[2].trade_count, 1);
    }

    #[test]
    fn test_from_ticks_n_tick_exact_multiple() {
        // Create exactly 6 ticks for 3-tick bars
        let ticks = vec![
            create_test_tick("ETHUSDT", "3000", "1.0", 0),
            create_test_tick("ETHUSDT", "3100", "1.0", 1),
            create_test_tick("ETHUSDT", "3050", "1.0", 2),
            create_test_tick("ETHUSDT", "3200", "1.0", 3),
            create_test_tick("ETHUSDT", "3150", "1.0", 4),
            create_test_tick("ETHUSDT", "3250", "1.0", 5),
        ];

        let bars = OHLCData::from_ticks_n_tick(&ticks, 3);

        // Should create exactly 2 bars
        assert_eq!(bars.len(), 2);

        // First bar: ticks 0-2
        assert_eq!(bars[0].open, Decimal::from_str("3000").unwrap());
        assert_eq!(bars[0].high, Decimal::from_str("3100").unwrap());
        assert_eq!(bars[0].low, Decimal::from_str("3000").unwrap());
        assert_eq!(bars[0].close, Decimal::from_str("3050").unwrap());
        assert_eq!(bars[0].trade_count, 3);

        // Second bar: ticks 3-5
        assert_eq!(bars[1].open, Decimal::from_str("3200").unwrap());
        assert_eq!(bars[1].high, Decimal::from_str("3250").unwrap());
        assert_eq!(bars[1].low, Decimal::from_str("3150").unwrap());
        assert_eq!(bars[1].close, Decimal::from_str("3250").unwrap());
        assert_eq!(bars[1].trade_count, 3);
    }

    #[test]
    fn test_from_ticks_n_tick_single_tick() {
        let ticks = vec![create_test_tick("BTCUSDT", "50000", "1.0", 0)];

        let bars = OHLCData::from_ticks_n_tick(&ticks, 1);

        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].open, bars[0].close);
        assert_eq!(bars[0].high, bars[0].low);
        assert_eq!(bars[0].trade_count, 1);
    }

    #[test]
    fn test_from_ticks_n_tick_empty() {
        let ticks: Vec<TickData> = vec![];
        let bars = OHLCData::from_ticks_n_tick(&ticks, 10);
        assert_eq!(bars.len(), 0);
    }

    #[test]
    fn test_from_ticks_n_tick_zero_count() {
        let ticks = vec![create_test_tick("BTCUSDT", "50000", "1.0", 0)];
        let bars = OHLCData::from_ticks_n_tick(&ticks, 0);
        assert_eq!(bars.len(), 0);
    }

    #[test]
    fn test_from_ticks_n_tick_large_count() {
        // Create 5 ticks but request 100-tick bars
        let ticks = vec![
            create_test_tick("BTCUSDT", "50000", "1.0", 0),
            create_test_tick("BTCUSDT", "50100", "1.0", 1),
            create_test_tick("BTCUSDT", "49900", "1.0", 2),
            create_test_tick("BTCUSDT", "50200", "1.0", 3),
            create_test_tick("BTCUSDT", "50300", "1.0", 4),
        ];

        let bars = OHLCData::from_ticks_n_tick(&ticks, 100);

        // Should create 1 incomplete bar with all 5 ticks
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].trade_count, 5);
        assert_eq!(bars[0].open, Decimal::from_str("50000").unwrap());
        assert_eq!(bars[0].close, Decimal::from_str("50300").unwrap());
        assert_eq!(bars[0].high, Decimal::from_str("50300").unwrap());
        assert_eq!(bars[0].low, Decimal::from_str("49900").unwrap());
    }

    #[test]
    fn test_from_ticks_n_tick_timestamp_ordering() {
        // Create ticks with specific timestamps
        let base_time = Utc::now();
        let ticks = vec![
            TickData {
                timestamp: base_time,
                ts_recv: base_time,
                symbol: "BTCUSDT".to_string(),
                exchange: "TEST".to_string(),
                price: Decimal::from_str("50000").unwrap(),
                quantity: Decimal::from_str("1.0").unwrap(),
                side: TradeSide::Buy,
                provider: "TEST".to_string(),
                trade_id: "1".to_string(),
                is_buyer_maker: false,
                sequence: 1,
                raw_dbn: None,
            },
            TickData {
                timestamp: base_time + Duration::seconds(10),
                ts_recv: base_time + Duration::seconds(10),
                symbol: "BTCUSDT".to_string(),
                exchange: "TEST".to_string(),
                price: Decimal::from_str("50100").unwrap(),
                quantity: Decimal::from_str("1.0").unwrap(),
                side: TradeSide::Buy,
                provider: "TEST".to_string(),
                trade_id: "2".to_string(),
                is_buyer_maker: false,
                sequence: 2,
                raw_dbn: None,
            },
            TickData {
                timestamp: base_time + Duration::seconds(20),
                ts_recv: base_time + Duration::seconds(20),
                symbol: "BTCUSDT".to_string(),
                exchange: "TEST".to_string(),
                price: Decimal::from_str("50200").unwrap(),
                quantity: Decimal::from_str("1.0").unwrap(),
                side: TradeSide::Buy,
                provider: "TEST".to_string(),
                trade_id: "3".to_string(),
                is_buyer_maker: false,
                sequence: 3,
                raw_dbn: None,
            },
        ];

        let bars = OHLCData::from_ticks_n_tick(&ticks, 2);

        // First bar should use timestamp of first tick in chunk
        assert_eq!(bars[0].timestamp, base_time);
        assert_eq!(bars[1].timestamp, base_time + Duration::seconds(20));
    }

    #[test]
    fn test_from_ticks_n_tick_price_extremes() {
        // Test with price movement including high and low extremes
        let ticks = vec![
            create_test_tick("BTCUSDT", "50000", "1.0", 0),
            create_test_tick("BTCUSDT", "55000", "1.0", 1), // Highest
            create_test_tick("BTCUSDT", "45000", "1.0", 2), // Lowest
            create_test_tick("BTCUSDT", "50000", "1.0", 3),
        ];

        let bars = OHLCData::from_ticks_n_tick(&ticks, 4);

        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].high, Decimal::from_str("55000").unwrap());
        assert_eq!(bars[0].low, Decimal::from_str("45000").unwrap());
        assert_eq!(bars[0].open, Decimal::from_str("50000").unwrap());
        assert_eq!(bars[0].close, Decimal::from_str("50000").unwrap());
    }

    #[test]
    fn test_from_ticks_n_tick_volume_accumulation() {
        let ticks = vec![
            create_test_tick("BTCUSDT", "50000", "1.5", 0),
            create_test_tick("BTCUSDT", "50100", "2.3", 1),
            create_test_tick("BTCUSDT", "50200", "0.7", 2),
        ];

        let bars = OHLCData::from_ticks_n_tick(&ticks, 3);

        assert_eq!(bars.len(), 1);
        assert_eq!(
            bars[0].volume,
            Decimal::from_str("1.5").unwrap()
                + Decimal::from_str("2.3").unwrap()
                + Decimal::from_str("0.7").unwrap()
        );
    }

    #[test]
    fn test_from_ticks_n_tick_100_tick_bars() {
        // Realistic scenario: 250 ticks creating 100-tick bars
        let mut ticks = Vec::new();
        for i in 0..250 {
            let price = format!("{}", 50000 + (i % 100));
            ticks.push(create_test_tick("BTCUSDT", &price, "1.0", i));
        }

        let bars = OHLCData::from_ticks_n_tick(&ticks, 100);

        // Should create 3 bars: 100 + 100 + 50
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0].trade_count, 100);
        assert_eq!(bars[1].trade_count, 100);
        assert_eq!(bars[2].trade_count, 50);
    }
}
