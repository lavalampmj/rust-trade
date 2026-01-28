-- =================================================================
-- Core Data Table for Quantitative Trading System: Tick Data
-- Design Principles: Single table storage, high-performance queries, data integrity
-- TimescaleDB: Hypertable with automatic compression
-- =================================================================

-- Enable TimescaleDB extension (requires superuser on first install)
-- Run this manually if not already installed:
-- CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;

-- Check if TimescaleDB is available
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'timescaledb') THEN
        RAISE NOTICE 'TimescaleDB extension not installed. Installing...';
        CREATE EXTENSION IF NOT EXISTS timescaledb CASCADE;
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS tick_data (
    -- 【Timestamp】UTC time, supports millisecond precision
    -- Why use TIMESTAMP WITH TIME ZONE:
    -- 1. Global markets require a unified timezone (UTC)
    -- 2. Supports millisecond-level precision for high-frequency trading needs
    -- 3. Time zone info avoids issues like daylight saving time
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,

    -- 【Trading Pair】e.g., 'BTCUSDT', 'ETHUSDT'
    -- Why use VARCHAR(20):
    -- 1. Cryptocurrency trading pairs are typically 8-15 characters
    -- 2. Reserved space for future new trading pairs
    -- 3. Fixed length storage offers better performance
    symbol VARCHAR(20) NOT NULL,

    -- 【Trade Price】Use DECIMAL to ensure precision
    -- Why use DECIMAL(20, 8):
    -- 1. Total 20 digits: supports prices in the trillions
    -- 2. 8 decimal places: meets cryptocurrency precision requirements (Bitcoin has 8 decimals)
    -- 3. Avoids floating point precision loss
    price DECIMAL(20, 8) NOT NULL,

    -- 【Trade Quantity】Also uses DECIMAL to ensure precision
    -- Why use DECIMAL(20, 8):
    -- 1. Trade volume calculations require high precision
    -- 2. Consistent precision with price
    quantity DECIMAL(20, 8) NOT NULL,

    -- 【Trade Side】Buy or Sell
    -- Why use VARCHAR(4) + CHECK constraint:
    -- 1. 'BUY'/'SELL' is more intuitive than boolean
    -- 2. CHECK constraint enforces data validity
    -- 3. Facilitates SQL querying and reporting
    side VARCHAR(4) NOT NULL CHECK (side IN ('BUY', 'SELL')),

    -- 【Trade ID】Original trade identifier from exchange
    -- Why use VARCHAR(50):
    -- 1. Different exchanges have different ID formats (numeric, alphanumeric, UUID, etc.)
    -- 2. Used for deduplication and traceability
    -- 3. Supports various exchange ID lengths
    trade_id VARCHAR(50) NOT NULL,

    -- 【Maker Flag】Whether the buyer is the maker (order placer)
    -- Why this field is needed:
    -- 1. Distinguish between aggressive and passive trades
    -- 2. Calculate market liquidity metrics
    -- 3. Basis for fee calculation
    is_buyer_maker BOOLEAN NOT NULL
);

-- =================================================================
-- TimescaleDB Hypertable Conversion
-- Enables automatic partitioning by time for efficient queries
-- =================================================================

-- Convert to hypertable (only if not already a hypertable)
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_name = 'tick_data'
    ) THEN
        PERFORM create_hypertable(
            'tick_data',
            'timestamp',
            chunk_time_interval => INTERVAL '1 day',
            if_not_exists => TRUE,
            migrate_data => TRUE
        );
        RAISE NOTICE 'Created hypertable for tick_data with 1-day chunks';
    ELSE
        RAISE NOTICE 'tick_data is already a hypertable';
    END IF;
END $$;

-- =================================================================
-- Index Strategy: Optimized for different query scenarios
-- Principle: Balance query performance and write efficiency
-- =================================================================

-- 【Index 1】Real-time trading query index
-- Use cases:
-- - Fetch the latest price for a trading pair: WHERE symbol = 'BTCUSDT' ORDER BY timestamp DESC LIMIT 1
-- - Get recent N minutes data of a trading pair: WHERE symbol = 'BTCUSDT' AND timestamp >= NOW() - INTERVAL '5 minutes'
-- - Real-time price push, risk control checks, and other high-frequency operations
-- Design notes:
-- - Composite index (symbol, timestamp DESC): group by symbol first, then order by time descending
-- - DESC order: prioritizes newest data, aligns with real-time query needs
-- - Supports index-only scans to avoid heap fetches and improve performance
CREATE INDEX IF NOT EXISTS idx_tick_symbol_time ON tick_data(symbol, timestamp DESC);

-- 【Index 2】Data integrity unique index
-- Use cases:
-- - Prevent duplicate data insertion due to network retransmission or program restart (idempotency)
-- - Data consistency checks to ensure no duplicated trade records
-- Design notes:
-- - Unique constraint on three fields: same symbol + same trade_id + same timestamp = unique record
-- - Unique constraint implicitly creates corresponding unique index to support fast duplicate checks
-- - Business logic aligns with financial system requirement of no duplicate and no missing data
CREATE UNIQUE INDEX IF NOT EXISTS idx_tick_unique ON tick_data(symbol, trade_id, timestamp);

-- 【Index 3】Backtesting time index
-- Use cases:
-- - Multi-symbol backtesting: WHERE timestamp BETWEEN '2025-01-01' AND '2025-01-02' AND symbol IN (...)
-- - Market-wide statistics: WHERE timestamp >= '2025-01-01' GROUP BY symbol
-- - Time-range data export: batch processing historical data by time intervals
-- Design notes:
-- - Single-column time index: more efficient than composite index when queries do not filter by symbol
-- - Supports range queries: BETWEEN operation fully utilizes B-tree index
-- - Essential for backtesting: ensures performance of historical data analysis
CREATE INDEX IF NOT EXISTS idx_tick_timestamp ON tick_data(timestamp);

-- =================================================================
-- TimescaleDB Compression Configuration
-- Automatically compresses chunks older than 7 days
-- =================================================================

-- Enable compression on the hypertable
DO $$
BEGIN
    -- Set compression settings
    ALTER TABLE tick_data SET (
        timescaledb.compress,
        timescaledb.compress_segmentby = 'symbol',
        timescaledb.compress_orderby = 'timestamp DESC'
    );
    RAISE NOTICE 'Compression enabled for tick_data';
EXCEPTION
    WHEN others THEN
        RAISE NOTICE 'Compression settings may already be configured: %', SQLERRM;
END $$;

-- Add compression policy (compress chunks older than 7 days)
DO $$
BEGIN
    PERFORM add_compression_policy('tick_data', INTERVAL '7 days', if_not_exists => TRUE);
    RAISE NOTICE 'Compression policy added: compress chunks older than 7 days';
EXCEPTION
    WHEN others THEN
        RAISE NOTICE 'Compression policy may already exist: %', SQLERRM;
END $$;

-- =================================================================
-- Paper Trading Log Table
-- =================================================================

CREATE TABLE IF NOT EXISTS live_strategy_log (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    symbol VARCHAR(20) NOT NULL,
    strategy VARCHAR(50) NOT NULL,
    signal VARCHAR(10) NOT NULL,
    price DECIMAL(20, 8) NOT NULL,
    quantity DECIMAL(20, 8),
    portfolio_value DECIMAL(20, 8),
    cash_balance DECIMAL(20, 8),
    position_size DECIMAL(20, 8),
    pnl DECIMAL(20, 8),
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_strategy_log_time ON live_strategy_log(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_strategy_log_symbol ON live_strategy_log(symbol, timestamp DESC);

-- =================================================================
-- Verification Queries
-- =================================================================

-- Check hypertable information
SELECT
    hypertable_name,
    num_chunks,
    compression_enabled
FROM timescaledb_information.hypertables
WHERE hypertable_name = 'tick_data';

-- Check chunk information
SELECT
    chunk_name,
    range_start,
    range_end,
    is_compressed,
    pg_size_pretty(total_bytes) as size
FROM timescaledb_information.chunks
WHERE hypertable_name = 'tick_data'
ORDER BY range_start DESC
LIMIT 10;

-- Check compression policy
SELECT * FROM timescaledb_information.jobs
WHERE proc_name = 'policy_compression';

-- View index information and sizes
SELECT
    indexname,
    pg_size_pretty(pg_relation_size(indexname::regclass)) AS size
FROM pg_indexes
WHERE tablename = 'tick_data'
ORDER BY indexname;

-- =================================================================
-- Symbol Definitions Table
-- Stores comprehensive symbol metadata for all tradeable instruments
-- =================================================================

CREATE TABLE IF NOT EXISTS symbol_definitions (
    -- Primary identifier: "{symbol}.{venue}" format
    id VARCHAR(100) PRIMARY KEY,

    -- Core identifiers
    symbol VARCHAR(50) NOT NULL,
    venue VARCHAR(50) NOT NULL,

    -- Numeric instrument ID (from exchange/provider)
    instrument_id BIGINT NOT NULL DEFAULT 0,

    -- Raw symbol as used by the primary data provider
    raw_symbol VARCHAR(50) NOT NULL,

    -- SymbolInfo (JSONB for flexibility)
    -- Contains: asset, asset_class, instrument_class, security_type,
    -- cfi_code, base_currency, quote_currency, settlement_currency, etc.
    info JSONB NOT NULL,

    -- VenueConfig (JSONB)
    -- Contains: venue, mic_code, dataset, publisher_id, country,
    -- is_primary_listing, venue_symbol, channel_id, rate_limits
    venue_config JSONB NOT NULL,

    -- TradingSpecs (JSONB)
    -- Contains: min_price_increment, display_factor, min_lot_size,
    -- maker_fee, taker_fee, margin_requirement, supported_order_types, etc.
    trading_specs JSONB NOT NULL,

    -- SessionSchedule (JSONB, nullable for 24/7 markets)
    -- Contains: timezone, regular_sessions, extended_sessions, calendar, maintenance_windows
    session_schedule JSONB,

    -- ContractSpec (JSONB, nullable for non-derivatives)
    -- Contains: activation, expiration, contract_multiplier, underlying,
    -- strike_price, option_type, roll_rule, etc.
    contract_spec JSONB,

    -- Provider-specific symbol mappings
    -- Key: provider name (e.g., "databento", "binance")
    -- Value: ProviderSymbol with symbol, dataset, instrument_id
    provider_mappings JSONB NOT NULL DEFAULT '{}',

    -- Symbol status
    status VARCHAR(20) NOT NULL DEFAULT 'ACTIVE',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Composite unique constraint
    CONSTRAINT uq_symbol_venue UNIQUE (symbol, venue),

    -- Status check
    CONSTRAINT chk_status CHECK (status IN ('ACTIVE', 'HALTED', 'SUSPENDED', 'DELISTED', 'PENDING', 'EXPIRED'))
);

-- Indexes for symbol_definitions
CREATE INDEX IF NOT EXISTS idx_symbol_def_symbol ON symbol_definitions(symbol);
CREATE INDEX IF NOT EXISTS idx_symbol_def_venue ON symbol_definitions(venue);
CREATE INDEX IF NOT EXISTS idx_symbol_def_status ON symbol_definitions(status);
CREATE INDEX IF NOT EXISTS idx_symbol_def_raw_symbol ON symbol_definitions(raw_symbol);

-- GIN index for JSONB queries on info (e.g., filter by asset_class)
CREATE INDEX IF NOT EXISTS idx_symbol_def_info ON symbol_definitions USING GIN (info);

-- =================================================================
-- Market Calendar Table
-- Stores holidays, early closes, and late opens for each venue
-- =================================================================

CREATE TABLE IF NOT EXISTS market_calendars (
    -- Venue identifier
    venue VARCHAR(50) NOT NULL,

    -- Calendar date
    date DATE NOT NULL,

    -- Calendar entry type: 'holiday', 'early_close', 'late_open'
    calendar_type VARCHAR(20) NOT NULL,

    -- Time for early_close/late_open (null for holidays)
    time TIME,

    -- Description (e.g., "Christmas", "Independence Day")
    description VARCHAR(200),

    -- Composite primary key
    PRIMARY KEY (venue, date, calendar_type),

    -- Type check
    CONSTRAINT chk_calendar_type CHECK (calendar_type IN ('holiday', 'early_close', 'late_open'))
);

-- Indexes for market_calendars
CREATE INDEX IF NOT EXISTS idx_market_cal_date ON market_calendars(date);
CREATE INDEX IF NOT EXISTS idx_market_cal_venue_date ON market_calendars(venue, date);

-- =================================================================
-- Continuous Contracts Table (Optional - for tracking roll state)
-- Stores current mapping of continuous contracts to underlyings
-- =================================================================

CREATE TABLE IF NOT EXISTS continuous_contracts (
    -- Continuous symbol (e.g., "ES.c.0.GLBX")
    id VARCHAR(100) PRIMARY KEY,

    -- Base symbol (e.g., "ES")
    base_symbol VARCHAR(50) NOT NULL,

    -- Venue (e.g., "GLBX")
    venue VARCHAR(50) NOT NULL,

    -- Roll method: 'c' (calendar), 'v' (volume), 'n' (open interest)
    roll_method CHAR(1) NOT NULL,

    -- Rank (0 = front month, 1 = second, etc.)
    rank SMALLINT NOT NULL DEFAULT 0,

    -- Currently mapped underlying contract ID
    current_contract_id VARCHAR(100),

    -- Current contract's raw symbol (e.g., "ESH6")
    current_raw_symbol VARCHAR(50),

    -- Adjustment factors history (JSONB array)
    adjustment_factors JSONB NOT NULL DEFAULT '[]',

    -- Last update timestamp
    last_updated TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Roll method check
    CONSTRAINT chk_roll_method CHECK (roll_method IN ('c', 'v', 'n')),

    -- Unique constraint
    CONSTRAINT uq_continuous UNIQUE (base_symbol, venue, roll_method, rank)
);

-- Index for continuous_contracts
CREATE INDEX IF NOT EXISTS idx_continuous_base ON continuous_contracts(base_symbol, venue);

-- =================================================================
-- Verification Queries for New Tables
-- =================================================================

-- Check symbol_definitions table
SELECT
    COUNT(*) as total_symbols,
    COUNT(DISTINCT venue) as venues
FROM symbol_definitions;

-- Check market_calendars table
SELECT
    venue,
    calendar_type,
    COUNT(*) as entries
FROM market_calendars
GROUP BY venue, calendar_type
ORDER BY venue, calendar_type;

-- =================================================================
-- L1 Quote Data Table (Best Bid/Ask - BBO)
-- Design: Time-series storage for top-of-book quotes
-- =================================================================

CREATE TABLE IF NOT EXISTS quote_data (
    -- Event timestamp from exchange
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,

    -- Trading pair (e.g., 'BTCUSDT')
    symbol VARCHAR(20) NOT NULL,

    -- Exchange identifier (e.g., 'KRAKEN', 'BINANCE')
    exchange VARCHAR(20) NOT NULL,

    -- Best bid price
    bid_price DECIMAL(20, 8) NOT NULL,

    -- Best ask price
    ask_price DECIMAL(20, 8) NOT NULL,

    -- Best bid size/quantity
    bid_size DECIMAL(20, 8) NOT NULL,

    -- Best ask size/quantity
    ask_size DECIMAL(20, 8) NOT NULL,

    -- Sequence number for ordering
    sequence BIGINT NOT NULL DEFAULT 0
);

-- Convert quote_data to hypertable
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_name = 'quote_data'
    ) THEN
        PERFORM create_hypertable(
            'quote_data',
            'timestamp',
            chunk_time_interval => INTERVAL '1 day',
            if_not_exists => TRUE,
            migrate_data => TRUE
        );
        RAISE NOTICE 'Created hypertable for quote_data with 1-day chunks';
    ELSE
        RAISE NOTICE 'quote_data is already a hypertable';
    END IF;
END $$;

-- Indexes for quote_data
CREATE INDEX IF NOT EXISTS idx_quote_symbol_time ON quote_data(symbol, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_quote_exchange_symbol_time ON quote_data(exchange, symbol, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_quote_timestamp ON quote_data(timestamp);

-- Unique index to prevent duplicate quotes
CREATE UNIQUE INDEX IF NOT EXISTS idx_quote_unique ON quote_data(symbol, exchange, timestamp, sequence);

-- Enable compression for quote_data
DO $$
BEGIN
    ALTER TABLE quote_data SET (
        timescaledb.compress,
        timescaledb.compress_segmentby = 'symbol,exchange',
        timescaledb.compress_orderby = 'timestamp DESC'
    );
    RAISE NOTICE 'Compression enabled for quote_data';
EXCEPTION
    WHEN others THEN
        RAISE NOTICE 'Compression settings may already be configured: %', SQLERRM;
END $$;

-- Add compression policy for quote_data
DO $$
BEGIN
    PERFORM add_compression_policy('quote_data', INTERVAL '7 days', if_not_exists => TRUE);
    RAISE NOTICE 'Compression policy added for quote_data';
EXCEPTION
    WHEN others THEN
        RAISE NOTICE 'Compression policy may already exist: %', SQLERRM;
END $$;

-- =================================================================
-- L2 Order Book Levels Table
-- Design: Stores order book snapshots and updates
-- Each row represents a single price level at a point in time
-- =================================================================

CREATE TABLE IF NOT EXISTS orderbook_levels (
    -- Event timestamp from exchange
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL,

    -- Trading pair (e.g., 'BTCUSDT')
    symbol VARCHAR(20) NOT NULL,

    -- Exchange identifier
    exchange VARCHAR(20) NOT NULL,

    -- Side: 'BID' or 'ASK'
    side VARCHAR(4) NOT NULL CHECK (side IN ('BID', 'ASK')),

    -- Price level
    price DECIMAL(20, 8) NOT NULL,

    -- Size/quantity at this level
    size DECIMAL(20, 8) NOT NULL,

    -- Number of orders at this level (if available)
    order_count INTEGER NOT NULL DEFAULT 0,

    -- Depth position (0 = best bid/ask, 1 = second best, etc.)
    depth_position SMALLINT NOT NULL DEFAULT 0,

    -- Sequence number for ordering
    sequence BIGINT NOT NULL DEFAULT 0,

    -- Is this part of a snapshot (TRUE) or incremental update (FALSE)
    is_snapshot BOOLEAN NOT NULL DEFAULT FALSE
);

-- Convert orderbook_levels to hypertable
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM timescaledb_information.hypertables
        WHERE hypertable_name = 'orderbook_levels'
    ) THEN
        PERFORM create_hypertable(
            'orderbook_levels',
            'timestamp',
            chunk_time_interval => INTERVAL '1 hour',
            if_not_exists => TRUE,
            migrate_data => TRUE
        );
        RAISE NOTICE 'Created hypertable for orderbook_levels with 1-hour chunks';
    ELSE
        RAISE NOTICE 'orderbook_levels is already a hypertable';
    END IF;
END $$;

-- Indexes for orderbook_levels
CREATE INDEX IF NOT EXISTS idx_orderbook_symbol_time ON orderbook_levels(symbol, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_orderbook_symbol_side_time ON orderbook_levels(symbol, side, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_orderbook_timestamp ON orderbook_levels(timestamp);
CREATE INDEX IF NOT EXISTS idx_orderbook_snapshot ON orderbook_levels(symbol, timestamp, is_snapshot) WHERE is_snapshot = TRUE;

-- Enable compression for orderbook_levels
DO $$
BEGIN
    ALTER TABLE orderbook_levels SET (
        timescaledb.compress,
        timescaledb.compress_segmentby = 'symbol,exchange,side',
        timescaledb.compress_orderby = 'timestamp DESC,depth_position'
    );
    RAISE NOTICE 'Compression enabled for orderbook_levels';
EXCEPTION
    WHEN others THEN
        RAISE NOTICE 'Compression settings may already be configured: %', SQLERRM;
END $$;

-- Add compression policy for orderbook_levels (compress after 1 day due to higher volume)
DO $$
BEGIN
    PERFORM add_compression_policy('orderbook_levels', INTERVAL '1 day', if_not_exists => TRUE);
    RAISE NOTICE 'Compression policy added for orderbook_levels';
EXCEPTION
    WHEN others THEN
        RAISE NOTICE 'Compression policy may already exist: %', SQLERRM;
END $$;

-- =================================================================
-- Verification Queries for Quote and OrderBook Tables
-- =================================================================

-- Check quote_data hypertable
SELECT
    hypertable_name,
    num_chunks,
    compression_enabled
FROM timescaledb_information.hypertables
WHERE hypertable_name = 'quote_data';

-- Check orderbook_levels hypertable
SELECT
    hypertable_name,
    num_chunks,
    compression_enabled
FROM timescaledb_information.hypertables
WHERE hypertable_name = 'orderbook_levels';
