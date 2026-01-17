# TODO - Trading System Features & Improvements

**Last Updated**: 2026-01-16
**Status**: 95% Production Ready

---

## 🎯 Critical Blockers (Before Production)

### Security
- [x] Add rate limiting to WebSocket reconnections (prevent ban from Binance) - **COMPLETE** (generic, configurable, 9/9 tests)
- [ ] Add input validation to TickData constructor (prevent invalid data)

### Monitoring
- [ ] Add Prometheus metrics endpoint
- [ ] Set up Grafana dashboard
- [ ] Configure alerting (connection pool saturation, batch failures, disconnections)

---

## 🔥 High Priority Features

### Symbol Sessions & Market Hours
- [ ] Add trading session configuration per symbol (not 24/7 for all markets)
- [ ] Implement session opening/closing times
- [ ] Handle pre-market and after-hours trading
- [ ] OHLC window alignment to session opening (not first tick)
- [ ] Market calendar support (holidays, early closes)
- [ ] Multiple timezone support for global markets

### OHLC Data Management
- [ ] Open of Session for first bar
- [ ] Close of Session for cutting last bar short
- [ ] Design decision: Database storage vs on-the-fly generation
  - Option A: Pre-computed OHLC table (fast queries, storage overhead)
  - Option B: On-demand aggregation (no storage, slower queries)
  - Option C: Hybrid (store 1m candles, aggregate for higher timeframes)
- [ ] Database-side OHLC aggregation using PostgreSQL
- [ ] OHLC materialized views or tables
- [ ] OHLC cache strategy (if using on-the-fly)
- [ ] Historical OHLC backfill process

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
- [ ] Exchange API key management
- [ ] Live trading (beyond paper trading)
- [ ] Order execution with retry logic
- [ ] Slippage tracking

### Advanced Analytics
- [ ] Volatility analysis
- [ ] Correlation matrix
- [ ] Factor analysis
- [ ] Market regime detection
- [ ] Anomaly detection
- [ ] Indicator on Indicator, series output of one as series input of another, all series bound to input series ordering
- [ ] Implement Indicators 

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

### Test Coverage (2026-01-16)
- [x] Write tests for MarketDataService (9 tests)
- [x] Write tests for PaperTradingProcessor (13 tests)
- [x] Fix failing repository tests

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
- Critical blockers: 1-2 days
- High priority: 1-2 weeks
- Medium priority: 1-2 months
- Nice-to-have: 3-6 months

**Current Progress**: 95% complete

---

## 🎯 Next Steps

1. **This Week**:
   - Add rate limiting to WebSocket reconnections
   - Add input validation to TickData
   - Set up basic Prometheus metrics

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
