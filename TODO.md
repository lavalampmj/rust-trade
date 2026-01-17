# TODO - Trading System Features & Improvements

**Last Updated**: 2026-01-16
**Status**: 95% Production Ready

---

## 🎯 Critical Blockers (Before Production)

### Security
- [ ] Add rate limiting to WebSocket reconnections (prevent ban from Binance)
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
- [ ] Strategy parameter optimization framework
-     Brute Force parameter
-     Parameter optima search, e.g. genetic, PSO, 
- [ ] Walk-forward analysis
- [ ] Strategy portfolio allocation
- [ ] Risk management rules (max drawdown, position sizing)

---

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

### Exchange Integration
- [ ] Multiple exchange support (Coinbase, Kraken, etc.)
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

### Python Strategy Enhancements
- [ ] Python strategy sandboxing (security)
- [ ] CPU/memory limits per strategy
- [ ] Strategy code signing/verification
- [ ] Audit logging for strategy execution
- [ ] Hot-reload improvements

---

## ✅ Recently Completed

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
