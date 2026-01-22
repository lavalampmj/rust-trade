# Symbol and Session Management Architecture

**Status**: Design Document
**Last Updated**: 2026-01-22
**Related TODO Items**: Symbol Sessions & Market Hours, Symbol Metadata, Futures Symbology

---

## Overview

This document describes the architecture for comprehensive symbol metadata and session management in the trading system. The design enables:

1. **Rich symbol metadata** - All attributes needed to trade a symbol
2. **Trading session management** - Market hours, holidays, pre/post market
3. **Venue-symbol association** - Easy mapping between venues and their symbols
4. **Continuous contracts** - Futures roll logic and back-adjustment

**Design Alignment**: This architecture aligns with [Databento's symbology conventions](https://databento.com/docs/standards-and-conventions/symbology) to ensure compatibility with their market data feeds.

---

## Databento Compatibility

### Symbology Types (stype)

Databento supports four symbology types that we adopt:

| Type | Field | Description | Example |
|------|-------|-------------|---------|
| `raw_symbol` | Publisher's original symbol | The ticker as used by the exchange | `ESH6`, `AAPL`, `BTCUSDT` |
| `instrument_id` | Numeric publisher ID | Unique numeric identifier | `12345` |
| `parent` | Root symbol grouping | Groups instruments by asset class | `ES.FUT`, `ES.OPT` |
| `continuous` | Smart symbology | Continuous contracts with roll rules | `ES.c.0`, `CL.v.0`, `GC.n.1` |

### Continuous Contract Notation

Databento's continuous contract format: `{base}.{rule}.{rank}`

| Rule | Meaning | Description |
|------|---------|-------------|
| `c` | Calendar | Next expiring contract (by expiration date) |
| `v` | Volume | Highest trading volume contract |
| `n` | Open Interest | Highest open interest contract |

| Rank | Meaning |
|------|---------|
| `0` | Front month (lead contract) |
| `1` | Second month |
| `2` | Third month |
| etc. | ... |

**Examples**:
- `ES.c.0` - E-mini S&P 500, front month by calendar
- `CL.v.0` - Crude Oil, highest volume contract
- `GC.n.1` - Gold, second-highest open interest contract

### Dataset/Publisher Format

Databento uses `{VENUE}.{FEED}` format for datasets:
- `GLBX.MDP3` - CME Globex MDP 3.0
- `XNAS.ITCH` - Nasdaq TotalView-ITCH
- `IFEU.IMPACT` - ICE Futures Europe iMpact

### Key InstrumentDefMsg Fields (Databento Schema)

Our `SymbolDefinition` maps to Databento's `InstrumentDefMsg` (70 fields):

| Databento Field | Our Field | Description |
|-----------------|-----------|-------------|
| `raw_symbol[71]` | `raw_symbol` | Publisher's ticker symbol |
| `exchange[5]` | `venue.mic_code` | MIC code (XCME, XNAS) |
| `asset[11]` | `info.asset` | Product root symbol (ES, CL) |
| `instrument_class` | `info.instrument_class` | F=Future, O=Option, S=Spot |
| `currency[4]` | `info.quote_currency` | ISO 4217 currency code |
| `min_price_increment` | `trading_specs.tick_size` | Tick size |
| `unit_of_measure_qty` | `contract_spec.multiplier` | Contract multiplier |
| `expiration` | `contract_spec.expiry` | Contract expiration |
| `activation` | `contract_spec.activation` | Contract activation date |
| `strike_price` | `contract_spec.strike_price` | Option strike price |
| `underlying` | `contract_spec.underlying` | Underlying symbol |

---

## Current State Analysis

### Existing Structures

| Structure | Location | Purpose |
|-----------|----------|---------|
| `InstrumentId` | `orders/types.rs` | `{symbol}.{venue}` identifier |
| `Instrument` trait | `instruments/instrument.rs` | Core instrument interface |
| `InstrumentSpecs` | `instruments/instrument.rs` | Tick size, lot size, fees |
| `Venue` | `instruments/types.rs` | Exchange with venue type |
| `SymbolSpec` | `data-manager/src/symbol/` | Provider mappings |
| `TradingHours` | `data/gap_detection.rs` | Basic session hours |

### Gaps Identified

1. **No timezone support** - All timestamps are UTC with no session-local context
2. **No trading hours constraints** - Orders can be submitted anytime
3. **No market calendar** - No holidays or early closes
4. **No session state tracking** - Open/closed/halted status
5. **No pre-market/after-hours** - No extended hours support
6. **No roll logic** - Continuous contracts lack automatic rollover

---

## Proposed Architecture

### Core Design Principles

1. **Single Source of Truth** - One `SymbolDefinition` struct contains all symbol data
2. **Venue-Centric** - Symbols are always scoped to a venue
3. **Timezone-Aware** - Sessions defined in exchange-local time
4. **Hierarchical Defaults** - Symbol → Asset Class → Venue → Global defaults
5. **Event-Driven Sessions** - Session state changes emit events

### Data Model

```
┌─────────────────────────────────────────────────────────────────┐
│                      SymbolDefinition                           │
├─────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ SymbolInfo   │  │ VenueConfig  │  │ SessionSchedule      │  │
│  │ - symbol     │  │ - venue      │  │ - timezone           │  │
│  │ - asset_class│  │ - venue_type │  │ - regular_sessions   │  │
│  │ - base_ccy   │  │ - mic_code   │  │ - extended_sessions  │  │
│  │ - quote_ccy  │  │ - country    │  │ - calendar           │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ TradingSpecs │  │ ContractSpec │  │ ProviderMappings     │  │
│  │ - tick_size  │  │ - expiry     │  │ - binance: "BTCUSDT" │  │
│  │ - lot_size   │  │ - multiplier │  │ - databento: "BTC"   │  │
│  │ - min_qty    │  │ - settlement │  │ - cme: "BTC"         │  │
│  │ - max_qty    │  │ - roll_rule  │  │                      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Detailed Structures

### 1. SymbolDefinition (Root Structure)

```rust
/// Complete definition of a tradeable symbol
/// Aligned with Databento's InstrumentDefMsg schema
pub struct SymbolDefinition {
    /// Unique identifier: "{raw_symbol}.{venue}"
    /// Format matches Databento: symbol + MIC code
    pub id: InstrumentId,

    /// Numeric instrument ID (Databento: instrument_id)
    /// Unique numeric identifier assigned by publisher
    pub instrument_id: u64,

    /// Raw symbol as used by the publisher (Databento: raw_symbol)
    /// Examples: "ESH6", "AAPL", "BTCUSDT"
    pub raw_symbol: String,

    /// Basic symbol information
    pub info: SymbolInfo,

    /// Venue-specific configuration
    pub venue_config: VenueConfig,

    /// Trading specifications
    pub trading_specs: TradingSpecs,

    /// Session schedule (None for 24/7 markets)
    pub session_schedule: Option<SessionSchedule>,

    /// Contract specifications (futures/options only)
    pub contract_spec: Option<ContractSpec>,

    /// Provider-specific symbol mappings
    /// Key: provider name (e.g., "databento", "binance")
    /// Value: provider-specific symbol
    pub provider_mappings: HashMap<String, ProviderSymbol>,

    /// Symbol status
    pub status: SymbolStatus,

    /// Timestamps (Databento: ts_recv for capture time)
    pub ts_recv: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Provider-specific symbol mapping
pub struct ProviderSymbol {
    /// Symbol as used by provider
    pub symbol: String,
    /// Provider's dataset identifier (e.g., "GLBX.MDP3")
    pub dataset: Option<String>,
    /// Provider's instrument ID if numeric
    pub instrument_id: Option<u64>,
}
```

### 2. SymbolInfo

```rust
/// Basic symbol information
/// Aligns with Databento InstrumentDefMsg fields
pub struct SymbolInfo {
    /// Product root symbol (Databento: asset[11])
    /// Examples: "ES" for E-mini S&P, "CL" for Crude Oil, "BTC" for Bitcoin
    pub asset: String,

    /// Asset class
    pub asset_class: AssetClass,

    /// Instrument class (Databento: instrument_class char)
    /// 'F' = Future, 'O' = Option, 'S' = Spot, 'P' = Perpetual
    pub instrument_class: InstrumentClass,

    /// Security type (Databento: security_type[7])
    /// Examples: "FUT", "OPT", "SPOT", "PERP"
    pub security_type: String,

    /// CFI code (Databento: cfi[7]) - ISO 10962 classification
    /// Examples: "FXXXXX" for futures, "OCASPS" for options
    pub cfi_code: Option<String>,

    /// Base currency/asset
    pub base_currency: Currency,

    /// Quote/settlement currency (Databento: currency[4])
    /// ISO 4217 code: "USD", "EUR", "BTC"
    pub quote_currency: Currency,

    /// Settlement currency if different (Databento: settl_currency[4])
    pub settlement_currency: Option<Currency>,

    /// Human-readable description
    pub description: Option<String>,

    /// ISIN if available (ISO 6166)
    pub isin: Option<String>,

    /// Security sub-type (Databento: secsubtype[6])
    pub security_subtype: Option<String>,

    /// Market segment ID (Databento: market_segment_id)
    pub market_segment_id: Option<u32>,

    /// Product group (Databento: group[21])
    /// Grouping for related products
    pub group: Option<String>,
}

/// Instrument class enumeration
/// Single character codes align with Databento's instrument_class field
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentClass {
    /// 'S' - Spot/Cash
    Spot,
    /// 'F' - Future
    Future,
    /// 'O' - Option
    Option,
    /// 'P' - Perpetual swap
    Perpetual,
    /// 'B' - Bond
    Bond,
    /// 'X' - Spread/Strategy
    Spread,
    /// 'M' - Multi-leg
    MultiLeg,
}

impl InstrumentClass {
    pub fn as_char(&self) -> char {
        match self {
            Self::Spot => 'S',
            Self::Future => 'F',
            Self::Option => 'O',
            Self::Perpetual => 'P',
            Self::Bond => 'B',
            Self::Spread => 'X',
            Self::MultiLeg => 'M',
        }
    }
}
```

### 3. VenueConfig

```rust
/// Venue-specific configuration
/// Aligns with Databento's publisher/dataset model
pub struct VenueConfig {
    /// Venue identifier
    pub venue: Venue,

    /// Market Identifier Code (ISO 10383) (Databento: exchange[5])
    /// Examples: "XCME", "XNAS", "XNYS", "GLBX"
    pub mic_code: String,

    /// Dataset identifier (Databento format: "{VENUE}.{FEED}")
    /// Examples: "GLBX.MDP3", "XNAS.ITCH", "IFEU.IMPACT"
    pub dataset: Option<String>,

    /// Publisher ID (Databento: publisher_id)
    /// Unique ID for the data publisher
    pub publisher_id: Option<u16>,

    /// Country code (ISO 3166-1 alpha-2)
    pub country: Option<String>,

    /// Primary listing flag
    pub is_primary_listing: bool,

    /// Venue-specific symbol (may differ from canonical)
    pub venue_symbol: String,

    /// Channel ID for market data (Databento: channel_id)
    pub channel_id: Option<u16>,

    /// API rate limits for this venue
    pub rate_limits: Option<RateLimits>,
}

/// Standard MIC codes for common venues
pub mod mic_codes {
    pub const CME_GLOBEX: &str = "GLBX";
    pub const CME: &str = "XCME";
    pub const CBOT: &str = "XCBT";
    pub const NYMEX: &str = "XNYM";
    pub const COMEX: &str = "XCEC";
    pub const NASDAQ: &str = "XNAS";
    pub const NYSE: &str = "XNYS";
    pub const NYSE_ARCA: &str = "ARCX";
    pub const ICE_EU: &str = "IFEU";
    pub const ICE_US: &str = "IFUS";
    pub const BINANCE: &str = "BINC";  // Crypto venues don't have official MICs
    pub const COINBASE: &str = "COIN";
}

pub struct RateLimits {
    pub requests_per_second: u32,
    pub requests_per_minute: u32,
    pub orders_per_second: u32,
}
```

### 4. TradingSpecs

```rust
/// Trading specifications and constraints
/// Aligns with Databento InstrumentDefMsg pricing fields
pub struct TradingSpecs {
    /// Minimum price increment (Databento: min_price_increment)
    /// The tick size in price units
    pub min_price_increment: Decimal,

    /// Display factor for price conversion (Databento: display_factor)
    /// Price = raw_price * display_factor
    pub display_factor: Decimal,

    /// Price display format (Databento: price_display_format)
    /// 0 = decimal, 1 = fractional 1/2, 2 = fractional 1/4, etc.
    pub price_display_format: u8,

    /// Variable tick size rule (Databento: tick_rule)
    /// References VTT (Variable Tick Table) code
    pub tick_rule: Option<u8>,

    /// Minimum lot size for regular orders (Databento: min_lot_size)
    pub min_lot_size: Decimal,

    /// Minimum lot size for block trades (Databento: min_lot_size_block)
    pub min_lot_size_block: Option<Decimal>,

    /// Minimum lot size for round lot (Databento: min_lot_size_round_lot)
    pub min_lot_size_round_lot: Option<Decimal>,

    /// Maximum trade volume per order (Databento: max_trade_vol)
    pub max_trade_vol: Option<u32>,

    /// Minimum trade volume per order (Databento: min_trade_vol)
    pub min_trade_vol: Option<u32>,

    /// High price limit (Databento: high_limit_price)
    pub high_limit_price: Option<Decimal>,

    /// Low price limit (Databento: low_limit_price)
    pub low_limit_price: Option<Decimal>,

    /// Maximum price variation (Databento: max_price_variation)
    pub max_price_variation: Option<Decimal>,

    /// Market depth implied (Databento: market_depth_implied)
    pub market_depth_implied: Option<i32>,

    /// Market depth (Databento: market_depth)
    pub market_depth: Option<i32>,

    /// Match algorithm (Databento: match_algorithm)
    /// 'F' = FIFO, 'P' = Pro-rata, 'A' = Allocation
    pub match_algorithm: MatchAlgorithm,

    /// Maker fee (negative = rebate)
    pub maker_fee: Decimal,

    /// Taker fee
    pub taker_fee: Decimal,

    /// Margin requirement (if applicable)
    pub margin_requirement: Option<MarginRequirement>,

    /// Supported order types
    pub supported_order_types: Vec<OrderType>,

    /// Instrument attributes (Databento: inst_attrib_value)
    /// Bitmask of instrument attributes
    pub inst_attrib_value: Option<i32>,

    /// Supports short selling
    pub shortable: bool,

    /// User-defined instrument flag (Databento: user_defined_instrument)
    pub user_defined_instrument: bool,
}

/// Match algorithm enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    /// 'F' - FIFO (First In, First Out)
    Fifo,
    /// 'P' - Pro-rata
    ProRata,
    /// 'A' - Allocation
    Allocation,
    /// 'C' - FIFO with LMM (Lead Market Maker)
    FifoLmm,
    /// 'T' - Threshold Pro-rata
    ThresholdProRata,
    /// 'U' - Undefined
    Undefined,
}

pub struct MarginRequirement {
    pub initial: Decimal,
    pub maintenance: Decimal,
}
```

### 5. SessionSchedule (New - Core Session Management)

```rust
/// Complete session schedule for a symbol
pub struct SessionSchedule {
    /// Exchange timezone (e.g., "America/New_York", "Asia/Tokyo")
    pub timezone: chrono_tz::Tz,

    /// Regular trading sessions
    pub regular_sessions: Vec<TradingSession>,

    /// Extended hours sessions (pre-market, after-hours)
    pub extended_sessions: Vec<TradingSession>,

    /// Market calendar for holidays
    pub calendar: MarketCalendar,

    /// Maintenance windows (scheduled downtime)
    pub maintenance_windows: Vec<MaintenanceWindow>,
}

/// A single trading session within a day
pub struct TradingSession {
    /// Session name (e.g., "Regular", "Pre-Market", "After-Hours")
    pub name: String,

    /// Session type
    pub session_type: SessionType,

    /// Days this session is active
    pub active_days: Vec<Weekday>,

    /// Session start time (local timezone)
    pub start_time: NaiveTime,

    /// Session end time (local timezone)
    pub end_time: NaiveTime,

    /// Order types allowed during this session
    pub allowed_order_types: Vec<OrderType>,

    /// Whether auction occurs at open
    pub has_opening_auction: bool,

    /// Whether auction occurs at close
    pub has_closing_auction: bool,
}

pub enum SessionType {
    Regular,
    PreMarket,
    AfterHours,
    Auction,
    Holiday,
}

/// Market calendar with holidays and special days
pub struct MarketCalendar {
    /// Holiday dates (market closed)
    pub holidays: Vec<NaiveDate>,

    /// Early close dates with close time
    pub early_closes: HashMap<NaiveDate, NaiveTime>,

    /// Late open dates with open time
    pub late_opens: HashMap<NaiveDate, NaiveTime>,
}

pub struct MaintenanceWindow {
    /// Day of week
    pub day: Weekday,
    /// Start time (local timezone)
    pub start_time: NaiveTime,
    /// End time (local timezone)
    pub end_time: NaiveTime,
}
```

### 6. ContractSpec (Futures/Options)

```rust
/// Contract specifications for derivatives
/// Aligns with Databento InstrumentDefMsg contract fields
pub struct ContractSpec {
    /// Contract activation date (Databento: activation)
    /// When the contract becomes tradeable
    pub activation: Option<DateTime<Utc>>,

    /// Contract expiration date (Databento: expiration)
    pub expiration: Option<DateTime<Utc>>,

    /// Maturity date components (Databento: maturity_year/month/day/week)
    pub maturity_year: Option<u16>,
    pub maturity_month: Option<u8>,
    pub maturity_day: Option<u8>,
    pub maturity_week: Option<u8>,

    /// Contract multiplier (Databento: unit_of_measure_qty)
    /// Notional value per point
    pub contract_multiplier: Decimal,

    /// Original contract size (Databento: original_contract_size)
    pub original_contract_size: Option<i32>,

    /// Unit of measure (Databento: unit_of_measure[31])
    /// Examples: "IPNT" (index points), "BBL" (barrels), "OZ" (ounces)
    pub unit_of_measure: Option<String>,

    /// Contract multiplier unit (Databento: contract_multiplier_unit)
    pub contract_multiplier_unit: Option<i8>,

    /// Flow schedule type (Databento: flow_schedule_type)
    pub flow_schedule_type: Option<i8>,

    /// Underlying symbol (Databento: underlying[21])
    pub underlying: Option<String>,

    /// Underlying product code (Databento: underlying_product)
    pub underlying_product: Option<u8>,

    /// Underlying instrument ID (Databento: underlying_id)
    pub underlying_id: Option<u32>,

    /// Decay quantity (Databento: decay_quantity)
    pub decay_quantity: Option<i32>,

    /// Decay start date (Databento: decay_start_date)
    pub decay_start_date: Option<u16>,

    /// Settlement type
    pub settlement: SettlementType,

    /// First notice date (futures physical delivery)
    pub first_notice_date: Option<NaiveDate>,

    /// Last trading date
    pub last_trading_date: Option<NaiveDate>,

    // --- Options-specific fields ---

    /// Strike price (Databento: strike_price)
    pub strike_price: Option<Decimal>,

    /// Strike price currency (Databento: strike_price_currency[4])
    pub strike_price_currency: Option<String>,

    // --- Continuous contract fields ---

    /// Roll rule for continuous contracts
    pub roll_rule: Option<RollRule>,
}

pub enum SettlementType {
    Cash,
    Physical,
}

/// Rules for rolling continuous contracts
/// Aligns with Databento's continuous contract symbology (c, v, n)
pub struct RollRule {
    /// Roll method matching Databento's continuous notation
    pub roll_method: ContinuousRollMethod,

    /// Rank in the roll chain (0 = front month, 1 = second month, etc.)
    pub rank: u8,

    /// Adjustment method for historical prices
    pub adjustment_method: AdjustmentMethod,
}

/// Continuous contract roll methods
/// Matches Databento notation: ES.c.0, ES.v.0, ES.n.0
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContinuousRollMethod {
    /// 'c' - Calendar: Roll by expiration date order
    /// Selects contract with nearest expiration
    Calendar,

    /// 'v' - Volume: Roll by highest trading volume
    /// Selects contract with most volume
    Volume,

    /// 'n' - Open Interest: Roll by highest open interest
    /// Selects contract with most open interest
    OpenInterest,
}

impl ContinuousRollMethod {
    pub fn as_char(&self) -> char {
        match self {
            Self::Calendar => 'c',
            Self::Volume => 'v',
            Self::OpenInterest => 'n',
        }
    }

    /// Generate Databento-compatible continuous symbol
    /// e.g., "ES", Calendar, 0 -> "ES.c.0"
    pub fn continuous_symbol(&self, base_symbol: &str, rank: u8) -> String {
        format!("{}.{}.{}", base_symbol, self.as_char(), rank)
    }
}

pub enum AdjustmentMethod {
    /// No adjustment (raw prices)
    None,
    /// Ratio adjustment (multiply by factor)
    Ratio,
    /// Difference adjustment (add/subtract)
    Difference,
    /// Back-adjust historical prices
    BackAdjust,
}
```

---

## Session State Management

### SessionManager Service

```rust
/// Tracks real-time session state for all symbols
pub struct SessionManager {
    /// Current session state per symbol
    states: DashMap<InstrumentId, SessionState>,

    /// Event channel for session state changes
    event_tx: broadcast::Sender<SessionEvent>,

    /// Symbol definitions
    definitions: Arc<SymbolRegistry>,
}

pub struct SessionState {
    /// Current session status
    pub status: MarketStatus,

    /// Current session (if any)
    pub current_session: Option<TradingSession>,

    /// Time until next session change
    pub next_change: Option<DateTime<Utc>>,

    /// Reason for current status
    pub reason: Option<String>,
}

pub enum MarketStatus {
    Open,
    Closed,
    PreMarket,
    AfterHours,
    Auction,
    Halted,
    Maintenance,
}

pub enum SessionEvent {
    SessionOpened { symbol: InstrumentId, session: TradingSession },
    SessionClosed { symbol: InstrumentId },
    MarketHalted { symbol: InstrumentId, reason: String },
    MarketResumed { symbol: InstrumentId },
    MaintenanceStarted { symbol: InstrumentId },
    MaintenanceEnded { symbol: InstrumentId },
}
```

### Session-Aware Order Validation

```rust
impl OrderValidator {
    /// Validate order against session constraints
    pub fn validate_session(&self, order: &Order) -> Result<(), OrderError> {
        let session_state = self.session_manager.get_state(&order.instrument_id)?;

        match session_state.status {
            MarketStatus::Closed => {
                Err(OrderError::MarketClosed)
            }
            MarketStatus::PreMarket | MarketStatus::AfterHours => {
                // Check if order type allowed in extended hours
                if let Some(session) = &session_state.current_session {
                    if !session.allowed_order_types.contains(&order.order_type) {
                        return Err(OrderError::OrderTypeNotAllowed {
                            order_type: order.order_type,
                            session: session.name.clone(),
                        });
                    }
                }
                Ok(())
            }
            MarketStatus::Halted => {
                Err(OrderError::TradingHalted {
                    reason: session_state.reason.clone(),
                })
            }
            MarketStatus::Open | MarketStatus::Auction => Ok(()),
            MarketStatus::Maintenance => {
                Err(OrderError::MaintenanceWindow)
            }
        }
    }
}
```

---

## Symbol Registry

### Database Schema

```sql
-- Symbol definitions table
CREATE TABLE symbol_definitions (
    id VARCHAR(100) PRIMARY KEY,  -- "{symbol}.{venue}"
    symbol VARCHAR(50) NOT NULL,
    venue VARCHAR(50) NOT NULL,

    -- SymbolInfo (JSONB for flexibility)
    info JSONB NOT NULL,

    -- VenueConfig
    venue_config JSONB NOT NULL,

    -- TradingSpecs
    trading_specs JSONB NOT NULL,

    -- SessionSchedule (nullable for 24/7)
    session_schedule JSONB,

    -- ContractSpec (nullable for non-derivatives)
    contract_spec JSONB,

    -- Provider mappings
    provider_mappings JSONB NOT NULL DEFAULT '{}',

    -- Status
    status VARCHAR(20) NOT NULL DEFAULT 'active',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Indexes
    UNIQUE(symbol, venue)
);

CREATE INDEX idx_symbol_definitions_symbol ON symbol_definitions(symbol);
CREATE INDEX idx_symbol_definitions_venue ON symbol_definitions(venue);
CREATE INDEX idx_symbol_definitions_status ON symbol_definitions(status);

-- Market calendar table (for efficient holiday queries)
CREATE TABLE market_calendars (
    venue VARCHAR(50) NOT NULL,
    date DATE NOT NULL,
    calendar_type VARCHAR(20) NOT NULL,  -- 'holiday', 'early_close', 'late_open'
    time TIME,  -- For early_close/late_open
    description VARCHAR(200),
    PRIMARY KEY (venue, date, calendar_type)
);

CREATE INDEX idx_market_calendars_date ON market_calendars(date);
```

### SymbolRegistry Service

```rust
/// Central registry for all symbol definitions
pub struct SymbolRegistry {
    /// In-memory cache of definitions
    cache: DashMap<InstrumentId, Arc<SymbolDefinition>>,

    /// Database repository
    repository: SymbolRepository,

    /// Default configurations per venue
    venue_defaults: HashMap<Venue, VenueDefaults>,

    /// Default configurations per asset class
    asset_defaults: HashMap<AssetClass, AssetDefaults>,
}

impl SymbolRegistry {
    /// Get symbol definition with fallback to defaults
    pub async fn get(&self, id: &InstrumentId) -> Result<Arc<SymbolDefinition>> {
        // Check cache first
        if let Some(def) = self.cache.get(id) {
            return Ok(def.clone());
        }

        // Load from database
        let def = self.repository.get(id).await?;
        let def = Arc::new(def);
        self.cache.insert(id.clone(), def.clone());
        Ok(def)
    }

    /// Get all symbols for a venue
    pub async fn get_by_venue(&self, venue: &Venue) -> Result<Vec<Arc<SymbolDefinition>>> {
        self.repository.get_by_venue(venue).await
    }

    /// Check if market is open for symbol
    pub fn is_market_open(&self, id: &InstrumentId) -> Result<bool> {
        let def = self.cache.get(id).ok_or(RegistryError::NotFound)?;

        if let Some(schedule) = &def.session_schedule {
            let now_local = Utc::now().with_timezone(&schedule.timezone);
            // Check against sessions...
            Ok(self.is_within_session(now_local, schedule))
        } else {
            // 24/7 market
            Ok(true)
        }
    }
}
```

---

## Continuous Contract Support

### Databento Continuous Symbol Format

Databento uses the format `{base}.{rule}.{rank}` for continuous contracts:

```
ES.c.0   -> E-mini S&P 500, calendar-based, front month
ES.c.1   -> E-mini S&P 500, calendar-based, second month
CL.v.0   -> Crude Oil, volume-based, highest volume
GC.n.0   -> Gold, open-interest-based, highest OI
```

Our system adopts this notation for:
1. Symbol resolution when requesting data
2. Internal continuous contract representation
3. API compatibility with Databento feeds

### ContinuousContract

```rust
/// Continuous contract that maps to underlying futures
/// Uses Databento notation: {base}.{rule}.{rank}
pub struct ContinuousContract {
    /// Continuous contract symbol (e.g., "ES.c.0")
    /// Format: "{base_symbol}.{roll_method}.{rank}"
    pub continuous_symbol: String,

    /// Venue (e.g., "GLBX" for CME Globex)
    pub venue: Venue,

    /// Full identifier including venue
    pub id: InstrumentId,

    /// Base/asset symbol (e.g., "ES", "CL", "GC")
    pub base_symbol: String,

    /// Roll method ('c' = calendar, 'v' = volume, 'n' = open interest)
    pub roll_method: ContinuousRollMethod,

    /// Rank in the roll chain (0 = front, 1 = second, etc.)
    pub rank: u8,

    /// Currently mapped underlying contract
    pub current_contract: InstrumentId,

    /// Current contract's raw symbol (e.g., "ESH6")
    pub current_raw_symbol: String,

    /// Contract chain (ordered by rank)
    pub contract_chain: Vec<ContractInfo>,

    /// Adjustment factors for back-adjusted prices
    pub adjustment_factors: Vec<AdjustmentFactor>,

    /// Last time the mapping was updated
    pub last_updated: DateTime<Utc>,
}

impl ContinuousContract {
    /// Parse a Databento continuous symbol
    /// "ES.c.0" -> (base="ES", method=Calendar, rank=0)
    pub fn parse(symbol: &str) -> Result<(String, ContinuousRollMethod, u8), ParseError> {
        let parts: Vec<&str> = symbol.split('.').collect();
        if parts.len() != 3 {
            return Err(ParseError::InvalidFormat);
        }

        let base = parts[0].to_string();
        let method = match parts[1] {
            "c" => ContinuousRollMethod::Calendar,
            "v" => ContinuousRollMethod::Volume,
            "n" => ContinuousRollMethod::OpenInterest,
            _ => return Err(ParseError::InvalidRollMethod),
        };
        let rank = parts[2].parse::<u8>()
            .map_err(|_| ParseError::InvalidRank)?;

        Ok((base, method, rank))
    }
}

pub struct ContractInfo {
    /// Raw symbol (e.g., "ESH6", "ESM6")
    pub raw_symbol: String,
    /// Instrument ID
    pub instrument_id: u64,
    /// Full identifier
    pub id: InstrumentId,
    /// Expiration date
    pub expiration: DateTime<Utc>,
    /// First notice date (if applicable)
    pub first_notice: Option<NaiveDate>,
    /// Last trading date
    pub last_trading: Option<NaiveDate>,
    /// Current open interest
    pub open_interest: Option<u64>,
    /// Current daily volume
    pub volume: Option<u64>,
}

pub struct AdjustmentFactor {
    pub roll_date: DateTime<Utc>,
    pub from_symbol: String,
    pub to_symbol: String,
    /// Ratio adjustment: new_price = old_price * ratio
    pub ratio: Decimal,
    /// Difference adjustment: new_price = old_price + difference
    pub difference: Decimal,
}
```

### Roll Manager

```rust
/// Manages automatic contract rolling for continuous contracts
pub struct RollManager {
    /// Active continuous contracts by symbol
    contracts: DashMap<String, ContinuousContract>,

    /// Symbol registry for underlying contract lookups
    registry: Arc<SymbolRegistry>,

    /// Event channel for roll notifications
    event_tx: broadcast::Sender<RollEvent>,

    /// Background task for periodic roll evaluation
    roll_check_interval: Duration,
}

impl RollManager {
    /// Resolve a continuous symbol to its current underlying
    /// "ES.c.0" -> "ESH6.GLBX"
    pub async fn resolve(&self, continuous_symbol: &str) -> Result<InstrumentId> {
        if let Some(contract) = self.contracts.get(continuous_symbol) {
            Ok(contract.current_contract.clone())
        } else {
            // Load and resolve from registry
            self.load_continuous_contract(continuous_symbol).await
        }
    }

    /// Check if any contracts need to roll based on their rule
    pub async fn evaluate_rolls(&self) -> Vec<RollEvent> {
        let mut events = Vec::new();

        for entry in self.contracts.iter() {
            let contract = entry.value();
            if let Some(roll) = self.check_roll_needed(contract).await {
                events.push(roll);
            }
        }

        events
    }
}

pub enum RollEvent {
    /// Roll is scheduled to occur
    RollScheduled {
        continuous_symbol: String,
        from_symbol: String,
        to_symbol: String,
        roll_date: DateTime<Utc>,
    },
    /// Roll has been executed
    RollExecuted {
        continuous_symbol: String,
        from_symbol: String,
        to_symbol: String,
        adjustment_factor: AdjustmentFactor,
    },
    /// Roll was cancelled
    RollCancelled {
        continuous_symbol: String,
        reason: String,
    },
    /// Mapping updated (e.g., due to volume/OI shift)
    MappingChanged {
        continuous_symbol: String,
        old_symbol: String,
        new_symbol: String,
    },
}
```

---

## Integration Points

### OHLC Generator Integration

```rust
impl RealtimeOHLCGenerator {
    pub fn new(symbol_def: Arc<SymbolDefinition>) -> Self {
        let session_schedule = symbol_def.session_schedule.clone();

        Self {
            symbol_def,
            // Use session open time for bar alignment
            bar_alignment: session_schedule
                .as_ref()
                .and_then(|s| s.regular_sessions.first())
                .map(|sess| sess.start_time),
            // ...
        }
    }

    /// Align bar open time to session start
    fn align_bar_to_session(&self, timestamp: DateTime<Utc>) -> DateTime<Utc> {
        if let Some(alignment) = &self.bar_alignment {
            // Align to session open time
            // ...
        }
        timestamp
    }
}
```

### Order Management Integration

```rust
impl OrderManager {
    pub async fn submit_order(&self, order: Order) -> Result<OrderId> {
        // Get symbol definition
        let symbol_def = self.registry.get(&order.instrument_id).await?;

        // Validate against trading specs
        self.validate_trading_specs(&order, &symbol_def.trading_specs)?;

        // Validate against session (if not 24/7)
        if symbol_def.session_schedule.is_some() {
            self.validate_session(&order)?;
        }

        // Submit order...
    }
}
```

---

## Configuration Examples

### Crypto (24/7)

```toml
[[symbols]]
id = "BTCUSDT.BINANCE"
symbol = "BTCUSDT"
venue = "BINANCE"

[symbols.info]
asset_class = "Crypto"
instrument_class = "Spot"
base_currency = "BTC"
quote_currency = "USDT"

[symbols.trading_specs]
tick_size = "0.01"
lot_size = "0.00001"
min_quantity = "0.00001"
maker_fee = "0.001"
taker_fee = "0.001"

# No session_schedule = 24/7 trading
```

### Equities (NYSE)

```toml
[[symbols]]
id = "AAPL.NYSE"
symbol = "AAPL"
venue = "NYSE"

[symbols.info]
asset_class = "Equity"
instrument_class = "Stock"
base_currency = "AAPL"
quote_currency = "USD"

[symbols.trading_specs]
tick_size = "0.01"
lot_size = "1"
min_quantity = "1"

[symbols.session_schedule]
timezone = "America/New_York"

[[symbols.session_schedule.regular_sessions]]
name = "Regular"
session_type = "Regular"
active_days = ["Mon", "Tue", "Wed", "Thu", "Fri"]
start_time = "09:30:00"
end_time = "16:00:00"
has_opening_auction = true
has_closing_auction = true

[[symbols.session_schedule.extended_sessions]]
name = "Pre-Market"
session_type = "PreMarket"
active_days = ["Mon", "Tue", "Wed", "Thu", "Fri"]
start_time = "04:00:00"
end_time = "09:30:00"
allowed_order_types = ["Limit"]

[[symbols.session_schedule.extended_sessions]]
name = "After-Hours"
session_type = "AfterHours"
active_days = ["Mon", "Tue", "Wed", "Thu", "Fri"]
start_time = "16:00:00"
end_time = "20:00:00"
allowed_order_types = ["Limit"]
```

### Futures (CME E-mini S&P 500)

```toml
[[symbols]]
# Databento-compatible identifier
id = "ESH6.GLBX"
raw_symbol = "ESH6"
instrument_id = 12345  # Numeric ID from exchange

[symbols.info]
asset = "ES"  # Databento asset field
asset_class = "Futures"
instrument_class = "F"  # 'F' = Future (Databento convention)
security_type = "FUT"
quote_currency = "USD"
description = "E-mini S&P 500 March 2026"
group = "ES"

[symbols.venue_config]
mic_code = "GLBX"  # CME Globex MIC
dataset = "GLBX.MDP3"  # Databento dataset ID
publisher_id = 1

[symbols.trading_specs]
min_price_increment = "0.25"
display_factor = "1"
min_lot_size = "1"
match_algorithm = "F"  # FIFO

[symbols.contract_spec]
expiration = "2026-03-20T08:30:00-06:00"
contract_multiplier = "50"
unit_of_measure = "IPNT"  # Index points
settlement = "Cash"
first_notice_date = "2026-03-19"
last_trading_date = "2026-03-20"
underlying = "ES"

[symbols.session_schedule]
timezone = "America/Chicago"

[[symbols.session_schedule.regular_sessions]]
name = "Globex"
session_type = "Regular"
active_days = ["Sun", "Mon", "Tue", "Wed", "Thu"]
start_time = "17:00:00"
end_time = "16:00:00"  # Next day

[[symbols.session_schedule.maintenance_windows]]
day = "Fri"
start_time = "16:00:00"
end_time = "17:00:00"

[symbols.provider_mappings]
databento = { symbol = "ESH6", dataset = "GLBX.MDP3", instrument_id = 12345 }
```

### Continuous Contracts (Databento Notation)

```toml
# Front month by calendar (ES.c.0)
[[continuous_contracts]]
continuous_symbol = "ES.c.0"
base_symbol = "ES"
venue = "GLBX"
roll_method = "c"  # calendar
rank = 0

[continuous_contracts.adjustment]
method = "BackAdjust"

# Front month by volume (ES.v.0)
[[continuous_contracts]]
continuous_symbol = "ES.v.0"
base_symbol = "ES"
venue = "GLBX"
roll_method = "v"  # volume
rank = 0

# Second month by open interest (ES.n.1)
[[continuous_contracts]]
continuous_symbol = "ES.n.1"
base_symbol = "ES"
venue = "GLBX"
roll_method = "n"  # open interest
rank = 1
```

---

## Implementation Phases

### Phase 1: Core Data Structures
- Implement `SymbolDefinition` and all nested structs
- Create `SymbolRegistry` with in-memory cache
- Database schema and repository

### Phase 2: Session Management
- Implement `SessionSchedule` and `TradingSession`
- Create `SessionManager` service
- Session state tracking and events

### Phase 3: Integration
- Integrate with `OrderManager` for session validation
- Integrate with `RealtimeOHLCGenerator` for bar alignment
- Update gap detection to use session schedule

### Phase 4: Continuous Contracts
- Implement `ContinuousContract` and `RollRule`
- Create `RollManager` service
- Back-adjustment calculation

### Phase 5: Market Calendar
- Holiday calendar data loading
- Early close / late open handling
- Calendar API integration (optional)

---

## Migration Strategy

1. **Backward Compatible**: New fields are optional initially
2. **Gradual Adoption**: Existing code continues to work
3. **Feature Flags**: Enable session validation per symbol
4. **Data Migration**: Populate from existing `SymbolSpec` + hardcoded defaults

---

## References

### Databento Documentation
- [Databento Symbology Standards](https://databento.com/docs/standards-and-conventions/symbology) - Symbology types and conventions
- [Databento Instrument Definitions](https://databento.com/docs/schemas-and-data-formats/instrument-definitions) - InstrumentDefMsg schema
- [Databento Continuous Contract Symbology](https://databento.com/blog/live-continuous-contract-symbology) - c/v/n roll rules
- [DBN Rust Crate](https://docs.rs/dbn) - Databento Binary Encoding library

### Standards
- [ISO 10383 - MIC Codes](https://www.iso20022.org/market-identifier-codes) - Market Identifier Codes
- [ISO 4217 - Currency Codes](https://www.iso.org/iso-4217-currency-codes.html) - Currency codes
- [ISO 10962 - CFI Codes](https://www.iso.org/standard/81140.html) - Classification of Financial Instruments

### Exchange Documentation
- [CME Globex Trading Hours](https://www.cmegroup.com/trading-hours.html)
- [NYSE Trading Hours](https://www.nyse.com/markets/hours-calendars)
- [ICE Futures Hours](https://www.theice.com/market-hours)
