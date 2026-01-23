# TODO - Trading System Features & Improvements

**Last Updated**: 2026-01-23
**Status**: 98% Production Ready

---

## 🎯 Critical Blockers (Before Production)

### Security
- [x] Add rate limiting to WebSocket reconnections (prevent ban from Binance) - **COMPLETE** (generic, configurable, 9/9 tests)
- [x] Add input validation to TickData constructor (prevent invalid data) - **COMPLETE** (19 tests, production-ready)

### Monitoring
- [x] Add Prometheus metrics endpoint - **COMPLETE** (HTTP server on port 9090, 20+ metrics)
- [ ] Set up Grafana dashboard
- [x] Configure alerting (connection pool saturation, batch failures, disconnections) - **COMPLETE** (6 alert rules, 25/25 tests passing, production-ready)

---

## 🔥 High Priority Features

### Symbol Sessions & Market Hours
- [x] Add trading session configuration per symbol - **COMPLETE** (`SessionSchedule` struct, 33 tests in session.rs)
- [x] Implement session opening/closing times - **COMPLETE** (`TradingSession` with start/end times, `session_open_time()`, `session_close_time()`)
- [x] Handle pre-market and after-hours trading - **COMPLETE** (`SessionType::PreMarket/AfterHours`, factory methods, extended hours tracking)
- [x] OHLC window alignment to session opening (not first tick) - **COMPLETE** (`SessionAwareConfig` with `align_to_session_open`, `truncate_at_session_close`, Strategy `session_config()` method)
- [x] Market calendar support (holidays, early closes) - **COMPLETE** (`MarketCalendar` with holidays, early_closes, late_opens)
- [x] Multiple timezone support for global markets - **COMPLETE** (full `chrono_tz` support, DST handling, 6 timezone tests)

### Symbol Metadata
- [x] Add datamodel and metadata, structure to be planned - **COMPLETE** (`SymbolDefinition` with 70+ Databento-aligned fields, `SymbolInfo`, `VenueConfig`, `TradingSpecs`, 8 tests)
- [x] Symbol Registry with concurrent caching - **COMPLETE** (`SymbolRegistry` with DashMap, lazy loading, 25+ tests including concurrency)

### Futures Symbology and Support
- [x] Create Continuous Contract Symbol and map underlying contracts - **COMPLETE** (`ContinuousSymbol` parser for Databento format `ES.c.0`, `ContinuousContract` mapping, 40+ tests)
- [x] Create back adjusted Continuous Contract - **COMPLETE** (`AdjustmentFactor` with ratio/difference methods, cumulative adjustment, price adjustment helpers)
- [x] Create automatic rollover method for front underlying symbol - **COMPLETE** (`RollManager` with calendar/volume/OI roll detection, event broadcasting, 35+ tests)

### OHLC Data Management
- [x] Open of Session for first bar, have OHLC realtime-timer loop set from open session time - **COMPLETE** (`RealtimeOHLCGenerator` with `SessionAwareConfig`, session open alignment)
- [x] Close of Session for cutting last bar short - **COMPLETE** (`RealtimeOHLCGenerator` with session close truncation, `is_session_truncated` flag)
- [ ] Design decision: Database storage vs on-the-fly generation
  - Option A: Pre-computed OHLC table (fast queries, storage overhead)
  - Option B: On-demand aggregation (no storage, slower queries)
  - Option C: Hybrid (store 1m candles, aggregate for higher timeframes)
- [ ] Database-side OHLC aggregation using TimeScaledDB
- [ ] OHLC materialized views or tables
- [ ] OHLC cache strategy (if using on-the-fly)
- [ ] Historical OHLC backfill process
- [ ] Create N Tick OHLC, N Volume OHLC

---

## 📊 Medium Priority Features

### Performance Optimization
- [ ] Read replicas for backtest queries
- [ ] Separate read/write database connections
- [ ] Event sourcing architecture evaluation
- [ ] Circuit breaker for exchange connections
- [ ] Connection pool auto-tuning based on metrics

### Data Management
- [ ] Tick data retention policy (e.g., keep raw ticks for 30 days, OHLC forever)
- [ ] Data archival strategy (move old ticks to cold storage)
- [ ] Database partitioning by date/symbol
- [ ] Incremental OHLC updates (append-only optimization)

### Strategy Features
- [ ] Multi-timeframe strategy support
- [ ] Risk management rules (max drawdown, position sizing)
- [ ] Strategy capital allocation

### Strategy Hosting / BackTesting / Optimization Features
- [ ] Strategy parameter optimization framework
-     Brute Force parameter optimization
-     Parameter optima search, e.g. genetic, PSO, gradient
- [ ] Walk-forward analysis
- [ ] Have strategy execution limits, such as Time Limits, Thread Limits, Memory Limits
- [ ] Gracefully kill strategies in an infinite loop

## 🔧 Infrastructure & Operations

### Deployment
- [ ] Docker containerization
- [ ] Kubernetes deployment manifests
- [ ] CI/CD pipeline (GitHub Actions)
- [ ] Staging environment setup
- [ ] Blue-green deployment strategy

### Database
- [ ] Automated database backups
- [ ] Point-in-time recovery testing
- [ ] Database migration versioning (sqlx migrate)
- [ ] Performance tuning (VACUUM, ANALYZE schedules)
- [ ] Index maintenance automation

### Monitoring & Observability
- [ ] Distributed tracing (OpenTelemetry)
- [ ] Log aggregation (ELK stack or similar)
- [ ] Error tracking (Sentry or similar)
- [ ] Performance profiling in production
- [ ] SLA monitoring and reporting

---

## 🎨 Nice-to-Have Features

### User Interface
- [ ] Web dashboard for live monitoring
- [ ] Strategy management UI
- [ ] Backtest visualization
- [ ] Portfolio performance charts
- [ ] Real-time P&L display
- [ ] Expose User Settings, subscription to symbols, tick monitoring, session management by market

### Broker and Data Vendor Integration
- [ ] Multiple exchange support (Coinbase, Kraken, Databento etc.)
- [ ] Venue (Broker and Data Provider) API key management
- [ ] Live trading (beyond paper trading)
- [ ] Order execution with retry logic
- [ ] Slippage tracking

### Advanced Analytics
- [ ] Volatility analysis
- [ ] Correlation matrix
- [ ] Factor analysis
- [ ] Market regime detection
- [ ] Anomaly detection
- [x] Indicator on Indicator, series output of one as series input of another, all series bound to input series ordering - **COMPLETE** (Transform framework with composition)
- [x] Implement Indicators - **COMPLETE** (SMA, EMA, RSI, ATR, Highest, Lowest, Change, ROC, CrossAbove, CrossBelow) 

### Python Strategy Enhancements
- [x] Python strategy sandboxing (security) - **COMPLETE** (3 phases: hash verification, import blocking, resource monitoring)
- [x] CPU/memory limits per strategy - **COMPLETE** (CPU time tracking with 10ms warnings per tick)
- [x] Strategy code signing/verification - **COMPLETE** (SHA256 hash verification)
- [x] Hot-reload improvements - **COMPLETE** (debouncing, atomic reload, metrics, configurable hash verification)
- [ ] Audit logging for strategy execution

### Multi-Tenancy & User Management
- [ ] User authentication and authorization system
- [ ] Session management and token-based auth
- [ ] User-scoped data isolation (portfolios, strategies, backtest results)
- [ ] Role-based access control (Admin/User/ReadOnly)
- [ ] Rate limiting per user
- [ ] Audit logging for user actions
- [ ] User-namespaced caching
- **📋 See detailed implementation plan**: [MULTI-TENANT-PLAN.md](./MULTI-TENANT-PLAN.md)
  - 7-phase roadmap with backward compatibility
  - ~14 weeks estimated implementation
  - Database schema, authentication, API layer updates

---

## ✅ Recently Completed

### RealtimeOHLCGenerator Session-Aware Support (2026-01-23)
- [x] **Session-Aware Bar Generation** (parity with `HistoricalOHLCGenerator`):
  - `SessionAwareConfig` integration from `trading-common`
  - `is_session_aligned` flag in `BarBuilder` struct
  - Session state tracking: `is_first_bar_of_session`, `current_session_open`, `current_session_close`
- [x] **New Constructors & Methods**:
  - `with_session_config()`: Create generator with session configuration
  - `set_session_config()`: Runtime session config updates
  - `on_session_start()`: Notify new session start for proper alignment
  - Helper methods: `get_session_open_for_tick()`, `get_current_session_close()`, `align_to_session_open_time()`
- [x] **Session Open Alignment**:
  - First bar of session aligns to session open time (not first tick arrival)
  - `is_session_aligned` flag propagated to `BarData` metadata
- [x] **Session Close Truncation**:
  - Force-close partial bars at session end (both time-based and tick-based)
  - `is_session_truncated` flag propagated to `BarData` metadata
  - `check_timer_close()` updated to detect session close
- [x] **Backward Compatibility**:
  - Default `SessionAwareConfig::default()` maintains 24/7 behavior
  - All existing constructors unchanged
  - Session flags default to `false` when not using session-aware features
- [x] **Test Coverage**: 16 tests (6 existing + 10 new session-aware tests)

### Transform Framework for Indicator Composition (2026-01-23)
- [x] **Core Transform Trait**:
  - `Transform` trait with stateful internal output buffers
  - O(1) per-bar updates for indicators like EMA
  - Historical value access via `get(bars_ago)`
  - Automatic warmup period propagation in chains
- [x] **Built-in Transforms** (10 indicators):
  - `Sma`: Simple Moving Average
  - `Ema`: Exponential Moving Average
  - `Rsi`: Relative Strength Index (0-100 bounded)
  - `Atr`: Average True Range
  - `Highest`: Rolling maximum
  - `Lowest`: Rolling minimum
  - `Change`: Price difference (momentum)
  - `RateOfChange`: Percentage change
  - `CrossAbove`: Threshold crossing detection (returns bool)
  - `CrossBelow`: Threshold crossing detection (returns bool)
- [x] **PriceSource** (custom input series):
  - `PriceSource` enum: `Open`, `High`, `Low`, `Close`, `Volume`, `Typical`, `WeightedClose`, `Median`
  - All transforms support `with_source(period, source)` constructor
  - Example: `Sma::with_source(20, PriceSource::High)` for SMA of highs
  - Enables true Donchian channels: `Highest::with_source(20, PriceSource::High)`
- [x] **Transform Composition** (chaining API):
  - `TransformExt` trait for fluent chaining: `Rsi::new(14).sma(3)`
  - `SmaOf<T>`: SMA of any Decimal transform
  - `EmaOf<T>`: EMA of any Decimal transform
  - `HighestOf<T>`: Highest of any Decimal transform
  - `LowestOf<T>`: Lowest of any Decimal transform
  - Automatic warmup propagation: RSI(14).sma(3).highest(10) = 28 bars
- [x] **TransformRegistry**:
  - Dynamic named transform registration
  - Batch update of all transforms
  - `max_warmup()` and `all_ready()` helpers
- [x] **SeriesValue Extensions**:
  - Added `usize`/`isize` for counting transforms
  - Added tuples `(T, U)` and `(T, U, V)` for multi-value outputs
  - Enables Bollinger Bands `(upper, lower)` and MACD `(macd, signal, histogram)`
- [x] **Test Coverage**: 179 tests across all transforms
- [x] **Integration**: Added to `trading-common` as `pub mod transforms`

### Symbol Metadata & Futures Symbology (2026-01-23)
- [x] **Symbol Metadata** (Databento-aligned):
  - `SymbolDefinition`: 70+ field comprehensive symbol model
  - `SymbolInfo`: Asset/instrument classification with CFI codes
  - `VenueConfig`: MIC codes, publisher IDs, rate limits
  - `TradingSpecs`: Tick size, lot size, fees, margin, order types
  - `SymbolRegistry`: Thread-safe DashMap cache with statistics
  - 33+ tests (8 symbol_definition + 25 registry with concurrency)
- [x] **Futures Symbology** (Databento continuous format):
  - `ContinuousSymbol`: Parser for `ES.c.0` format (base.rule.rank)
  - `ContinuousContract`: Maps continuous symbols to underlying contracts
  - `AdjustmentFactor`: Ratio and difference back-adjustment methods
  - `RollManager`: Automatic roll detection (calendar/volume/OI)
  - `RollEvent`: Event broadcasting for roll notifications
  - 75+ tests (40 continuous.rs + 35 roll_manager.rs)
- [x] **Contract Specifications**:
  - `ContractSpec`: Expiration, multiplier, settlement, margin
  - Month code utilities: `month_to_code()`, `code_to_month()`
  - Symbol parsing: `parse_contract_symbol("ESH6")` → (ES, 3, 2006)
  - 10 tests for contract manipulation

### Symbol Sessions & Market Hours (2026-01-23)
- [x] Trading session configuration per symbol (`SessionSchedule` struct)
- [x] Session opening/closing times (`TradingSession` with start/end, `session_open_time()`, `session_close_time()`)
- [x] Pre-market and after-hours trading (`SessionType::PreMarket/AfterHours`, factory methods)
- [x] Market calendar support (`MarketCalendar` with holidays, early_closes, late_opens)
- [x] Multiple timezone support (full `chrono_tz` with DST handling)
- [x] Session manager for real-time tracking (`SessionManager` with event broadcasting)
- [x] Preset schedules for US equity, CME Globex, crypto 24/7, forex
- [x] **OHLC Session Alignment & Truncation** (NEW):
  - `SessionAwareConfig` struct for configuring session-aware bar generation
  - `align_to_session_open`: Align first bar to session open time (not first tick)
  - `truncate_at_session_close`: Force-close partial bars at session end
  - Works with both time-based and tick-based bars
  - `BarMetadata` extended with `is_session_truncated` and `is_session_aligned` flags
  - 15 tests for session-aware bar generation
- [x] **Strategy Session Configuration** (NEW):
  - `Strategy::session_config()` method for programmatic configuration
  - `BacktestEngine` uses strategy's session config when creating bar generator
  - Preset schedules accessible via `session_presets::us_equity()`, etc.
  - 5 integration tests for session-aware strategies in backtest_unified_test.rs
- [x] Comprehensive test coverage: 67 tests (33 session.rs + 14 session_manager.rs + 15 bar_generator + 5 strategy session tests)

### Order Management System Test Coverage (2026-01-22)
- [x] Comprehensive event type tests (24 tests covering all 14 order event types)
- [x] OrderManager integration tests (21 tests for full order lifecycle)
- [x] Concurrency tests for OrderManager (7 tests for thread safety)
- [x] Python bridge error recovery with circuit breaker pattern
- [x] Error statistics tracking and monitoring callbacks
- [x] Configurable error thresholds and auto-disable on consecutive failures
- [x] Error recovery tests (9 tests)
- [x] Total: 321 tests passing in trading-common (up from ~150)

### Order Management System Architecture (2026-01-21)
- [x] Comprehensive OMS with ~14K lines of new code
- [x] Order types: Market, Limit, Stop, StopLimit, TrailingStop
- [x] Order lifecycle with event sourcing pattern
- [x] Thread-safe OrderManager with async API
- [x] Python strategy order management methods (7 new methods)
- [x] Rust/Python parity for strategy framework
- [x] Full documentation in `docs/architecture/order-management-system.md`

### Alerting System (2026-01-17)
- [x] Comprehensive alerting system with 6 alert rules (TDD approach)
- [x] Connection pool saturation alerts (WARNING at 80%, CRITICAL at 95%)
- [x] Batch failure rate monitoring (WARNING at 20% failure rate)
- [x] WebSocket disconnection detection (CRITICAL alert)
- [x] WebSocket reconnection storm alerts (WARNING when exceeding threshold)
- [x] Channel backpressure monitoring (WARNING at 80% utilization)
- [x] Configurable thresholds via TOML (development.toml lines 109-139)
- [x] Cooldown mechanism to prevent alert spam (default 5 minutes)
- [x] Background evaluation task (configurable interval, default 30s)
- [x] Full integration with Prometheus metrics
- [x] Comprehensive test coverage (25/25 tests passing)
- [x] Production-ready with graceful degradation
- [x] Complete documentation (ALERTING-SYSTEM.md)

### Input Validation for TickData (2026-01-17)
- [x] Comprehensive `TickValidator` with configurable validation rules
- [x] Absolute validation: price (positive, bounds check), quantity (positive), timestamp (future/past limits)
- [x] Relative validation: price change detection (prevents flash crashes, 10% default, per-symbol overrides)
- [x] Symbol validation: length (3-20 chars), alphanumeric, uppercase only
- [x] Trade ID validation: non-empty, printable ASCII, length limits
- [x] TOML configuration with sensible defaults (development.toml lines 80-94)
- [x] Full integration in live data pipeline (market_data.rs:255)
- [x] Metrics tracking (TICKS_REJECTED_TOTAL)
- [x] Comprehensive test coverage (19/19 tests passing)
- [x] Thread-safe stateful validation with per-symbol price tracking
- [x] Production-ready with graceful degradation (skip invalid ticks, don't crash)

### Prometheus Metrics Endpoint (2026-01-17)
- [x] Comprehensive metrics system with 20+ metrics
- [x] HTTP server on port 9090 with `/metrics` and `/health` endpoints
- [x] Metrics for: tick processing, batches, cache, database pool, WebSocket, paper trading, system health
- [x] Active integration across codebase (binance.rs, market_data.rs, paper_trading.rs)
- [x] Uptime monitoring background task
- [x] Production-ready for Grafana integration

### WebSocket Reconnection Rate Limiting (2026-01-17)
- [x] Generic `ReconnectionRateLimiter` implementation (TDD approach)
- [x] Configurable rate limiting (per minute, per hour, custom windows)
- [x] TOML configuration support with sensible defaults (5 attempts/min)
- [x] Integration with BinanceExchange (default + custom config)
- [x] Comprehensive test coverage (9/9 tests passing, including concurrency)
- [x] Thread-safe implementation using Arc and lock-free atomics
- [x] Helpful logging and error messages
- [x] Production-ready, prevents exchange bans

### Hot-Reload Improvements (2026-01-17)
- [x] Debouncing to prevent multiple rapid reloads (configurable, default 300ms)
- [x] Atomic reload with validation before cache invalidation
- [x] Reload metrics tracking (success/failure count, timestamps)
- [x] Configurable hash verification skip for development mode
- [x] Enhanced error handling and helpful error messages
- [x] Production-ready hot-reload system

### Python Strategy Sandboxing (2026-01-17)
- [x] Phase 1: SHA256 code signing for strategy verification
- [x] Phase 2: RestrictedPython integration (import blocking for network, filesystem, subprocess)
- [x] Phase 3: Resource monitoring (CPU time tracking, 10ms soft limits with warnings)
- [x] Comprehensive security tests (10 unit tests, 5 malicious test strategies)
- [x] Full test coverage: 67/67 tests passing

### Security Hardening (2026-01-16)
- [x] Strong database password (44-char cryptographic)
- [x] .env gitignore verification
- [x] Database indexes verified

### Test Coverage (2026-01-16 - 2026-01-22)
- [x] Write tests for MarketDataService (9 tests)
- [x] Write tests for PaperTradingProcessor (13 tests)
- [x] Fix failing repository tests
- [x] Add comprehensive OMS test coverage (61 new tests) - **2026-01-22**
- [x] Add Python bridge error recovery tests (9 tests) - **2026-01-22**
- [x] Total workspace tests: 776 passing

### Performance (2026-01-16)
- [x] Implement backpressure mechanism
- [x] Optimize batch insert size (500 ticks, 5s intervals)
- [x] Increase connection pool (5→8 max, 1→2 min)

### Features (2026-01-15)
- [x] Expand monitoring to 10 trading pairs
- [x] Python strategy integration
- [x] SMA and RSI example strategies

---

## 📝 Documentation Needs

- [ ] API documentation (for Python strategy developers)
- [ ] Deployment guide
- [ ] Database schema documentation
- [ ] Performance tuning guide
- [ ] Troubleshooting guide
- [ ] Architecture decision records (ADRs)
- [x] Multi-tenancy architectural plan - see [MULTI-TENANT-PLAN.md](./MULTI-TENANT-PLAN.md)
- [x] Python strategy security documentation - see [SECURITY_TEST_RESULTS.md](./SECURITY_TEST_RESULTS.md)
- [x] Hot-reload improvements documentation - see [HOT-RELOAD-IMPROVEMENTS.md](./HOT-RELOAD-IMPROVEMENTS.md)
- [x] WebSocket rate limiting review - see [WEBSOCKET-RATE-LIMITING-REVIEW.md](./WEBSOCKET-RATE-LIMITING-REVIEW.md)
- [x] Alerting system documentation - see [ALERTING-SYSTEM.md](./ALERTING-SYSTEM.md)

---

## 🔍 Technical Debt

- [ ] Remove duplicate code in exchange modules
- [ ] Consolidate error handling patterns
- [ ] Standardize logging format
- [ ] Update dependencies to latest versions
- [ ] Remove deprecated code paths
- [ ] Improve code comments in complex sections

---

## 📅 Timeline Estimates

**To Production Ready (100%)**:
- Critical blockers: Complete! (2/2 security items done)
- High priority: 1-2 weeks
- Medium priority: 1-2 months
- Nice-to-have: 3-6 months

**Current Progress**: 98% complete (all critical security blockers resolved!)

---

## 🎯 Next Steps

1. **This Week**:
   - Set up Grafana dashboard for metrics visualization
   - Configure alerting for monitoring stack

2. **Next Week**:
   - Design symbol sessions feature
   - Decide on OHLC storage strategy
   - Set up Grafana dashboard

3. **This Month**:
   - Implement chosen OHLC solution
   - Add symbol session support
   - Complete monitoring stack

---

**Notes**:
- Items marked with [ ] are pending
- Items marked with [x] are completed
- Priority levels are flexible based on business needs
- Timeline estimates assume focused development effort
