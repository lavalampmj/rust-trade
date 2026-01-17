# Multi-Tenancy Readiness: Architectural Review & Recommendations

## Executive Summary

**Current State**: Single-tenant architecture with zero user isolation
- No authentication or authorization layer
- Global shared state across all components (database, cache, portfolios)
- No user context in any data models or API calls
- Suitable for single-user desktop app, **not ready for multi-tenancy**

**Assessment**: The system requires foundational changes to support multi-user scenarios, but the current architecture is well-structured with good separation of concerns, making it feasible to add multi-tenancy support through incremental changes.

**Recommendation**: Implement supporting capabilities NOW using a phased approach that maintains backward compatibility while building the foundation for future multi-tenancy.

---

## Current Architecture Analysis

### Data Layer
**Schema** (`config/schema.sql`):
- `tick_data` table: No `user_id` column, globally accessible
- `live_strategy_log` table: No user tracking
- Missing tables: `users`, `sessions`, `audit_log`, `user_strategies`, `user_portfolios`, `backtest_results`
- No Row-Level Security (RLS) policies

**Repository** (`trading-common/src/data/repository.rs`):
- All methods query globally with symbol as only filter dimension
- No user context parameter in any method signature
- Cache-aware queries but no user namespacing

**Cache** (`trading-common/src/data/cache.rs`):
- Redis keys: `tick:BTCUSDT` (global, symbol-based only)
- In-memory: `HashMap<String, VecDeque<TickData>>` (symbol as key)
- Zero user segmentation

### Service Layer
**State Management** (`src-tauri/src/state.rs`):
- Single global `AppState` with `Arc<TickDataRepository>`
- No session manager or user context
- Shared across all Tauri command invocations

**Paper Trading** (`trading-core/src/live_trading/paper_trading.rs`):
- Single `PaperTradingProcessor` instance per application
- Stateful portfolio (cash, position, trades) shared globally
- No per-user portfolio isolation

### API Layer
**Tauri Commands** (`src-tauri/src/commands.rs`):
- 7 commands exposed, all publicly callable
- No authentication token parameter
- No authorization checks
- No audit logging

### Security Gaps
- ❌ No authentication (no users table, no login system)
- ❌ No session management
- ❌ No authorization/permissions
- ❌ No rate limiting
- ❌ No audit trail
- ❌ CSP disabled in Tauri config (`"csp": null`)
- ❌ No input validation beyond type checking
- ✅ SQL injection protection (parameterized queries with sqlx)

---

## Recommendations: Building Supporting Capabilities NOW

### Phase 1: Database Foundation (Week 1-2)
**Goal**: Add schema support for users without breaking existing functionality

#### 1.1 Create User Management Tables

```sql
-- New file: config/migrations/001_add_multi_user_support.sql

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    display_name VARCHAR(100),
    role VARCHAR(20) NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user', 'readonly')),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(255) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ip_address INET,
    user_agent TEXT
);

CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(50),
    resource_id UUID,
    metadata JSONB,
    ip_address INET,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX idx_users_email ON users(email);
CREATE INDEX idx_users_active ON users(is_active) WHERE is_active = TRUE;
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_sessions_token ON sessions(token_hash);
CREATE INDEX idx_sessions_expires ON sessions(expires_at);
CREATE INDEX idx_audit_user_time ON audit_log(user_id, created_at DESC);
CREATE INDEX idx_audit_action ON audit_log(action, created_at DESC);
```

#### 1.2 Add User Context to Existing Tables (Backward Compatible)

```sql
-- NULLABLE for backward compatibility
ALTER TABLE tick_data ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE live_strategy_log ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;

-- Conditional indexes (only index user-specific data)
CREATE INDEX idx_tick_user_symbol_time ON tick_data(user_id, symbol, timestamp DESC)
    WHERE user_id IS NOT NULL;
CREATE INDEX idx_live_log_user ON live_strategy_log(user_id, timestamp DESC)
    WHERE user_id IS NOT NULL;
```

#### 1.3 Create User-Scoped Resource Tables

```sql
CREATE TABLE user_strategies (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    strategy_id VARCHAR(50) NOT NULL,
    name VARCHAR(100) NOT NULL,
    parameters JSONB NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, name)
);

CREATE TABLE user_portfolios (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    initial_capital DECIMAL(20, 8) NOT NULL,
    current_cash DECIMAL(20, 8) NOT NULL,
    current_position DECIMAL(20, 8) NOT NULL DEFAULT 0,
    avg_cost DECIMAL(20, 8) NOT NULL DEFAULT 0,
    total_trades BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, name)
);

CREATE TABLE backtest_results (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    strategy_id UUID REFERENCES user_strategies(id) ON DELETE SET NULL,
    symbol VARCHAR(20) NOT NULL,
    timeframe VARCHAR(10),
    start_time TIMESTAMPTZ NOT NULL,
    end_time TIMESTAMPTZ NOT NULL,
    initial_capital DECIMAL(20, 8) NOT NULL,
    final_value DECIMAL(20, 8) NOT NULL,
    total_pnl DECIMAL(20, 8) NOT NULL,
    sharpe_ratio DECIMAL(10, 4),
    max_drawdown DECIMAL(10, 4),
    total_trades INTEGER NOT NULL,
    results_data JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_user_strategies_user ON user_strategies(user_id);
CREATE INDEX idx_user_portfolios_user ON user_portfolios(user_id);
CREATE INDEX idx_backtest_user_time ON backtest_results(user_id, created_at DESC);
```

**Critical Files**:
- `/home/mj/rust-trade/config/migrations/001_add_multi_user_support.sql` (NEW)
- `/home/mj/rust-trade/config/schema.sql` (UPDATE with migration reference)

---

### Phase 2: Domain Model Updates (Week 3-4)
**Goal**: Introduce user context types without breaking existing code

#### 2.1 Add User Context Types

```rust
// trading-common/src/data/types.rs

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct UserContext {
    pub user_id: Uuid,
    pub role: UserRole,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UserRole {
    Admin,
    User,
    ReadOnly,
}

impl UserRole {
    pub fn as_str(&self) -> &str {
        match self {
            UserRole::Admin => "admin",
            UserRole::User => "user",
            UserRole::ReadOnly => "readonly",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, DataError> {
        match s {
            "admin" => Ok(UserRole::Admin),
            "user" => Ok(UserRole::User),
            "readonly" => Ok(UserRole::ReadOnly),
            _ => Err(DataError::InvalidFormat(format!("Invalid role: {}", s))),
        }
    }
}

impl UserContext {
    pub fn can_write(&self) -> bool {
        matches!(self.role, UserRole::Admin | UserRole::User)
    }

    pub fn can_access_global_data(&self) -> bool {
        matches!(self.role, UserRole::Admin)
    }
}
```

#### 2.2 Extend Repository with User-Aware Methods (Backward Compatible)

```rust
// trading-common/src/data/repository.rs

impl TickDataRepository {
    // KEEP existing methods unchanged for backward compatibility
    pub async fn get_ticks(&self, query: &TickQuery) -> DataResult<Vec<TickData>> {
        self.get_ticks_with_context(query, None).await
    }

    // ADD new user-aware variants
    pub async fn get_ticks_with_context(
        &self,
        query: &TickQuery,
        user_ctx: Option<&UserContext>
    ) -> DataResult<Vec<TickData>> {
        // Build query with optional user filtering
        let mut sql_query = QueryBuilder::new(
            "SELECT timestamp, symbol, price, quantity, side, trade_id, is_buyer_maker
             FROM tick_data WHERE symbol = "
        );
        sql_query.push_bind(&query.symbol);

        // Add user scoping if context provided
        if let Some(ctx) = user_ctx {
            if !ctx.can_access_global_data() {
                sql_query.push(" AND (user_id = ")
                    .push_bind(ctx.user_id)
                    .push(" OR user_id IS NULL)"); // Allow shared data
            }
        }

        // ... rest of existing query logic
    }

    pub async fn batch_insert_with_context(
        &self,
        ticks: Vec<TickData>,
        user_ctx: Option<&UserContext>
    ) -> DataResult<usize> {
        if let Some(ctx) = user_ctx {
            if !ctx.can_write() {
                return Err(DataError::Permission("User lacks write permission".into()));
            }
        }
        // ... existing logic with user_id set on ticks
    }
}
```

**Critical Files**:
- `/home/mj/rust-trade/trading-common/src/data/types.rs` (UPDATE)
- `/home/mj/rust-trade/trading-common/src/data/repository.rs` (UPDATE)

---

### Phase 3: Authentication Infrastructure (Week 5-6)
**Goal**: Add authentication without forcing all commands to use it

#### 3.1 Create User Repository

```rust
// trading-common/src/data/user_repository.rs (NEW FILE)

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2
};
use uuid::Uuid;
use sqlx::PgPool;

pub struct UserRepository {
    pool: PgPool,
}

impl UserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_user(
        &self,
        email: &str,
        password: &str,
        role: UserRole
    ) -> DataResult<Uuid> {
        let password_hash = hash_password(password)?;
        let user_id = sqlx::query_scalar!(
            "INSERT INTO users (email, password_hash, role)
             VALUES ($1, $2, $3) RETURNING id",
            email, password_hash, role.as_str()
        ).fetch_one(&self.pool).await?;
        Ok(user_id)
    }

    pub async fn authenticate(
        &self,
        email: &str,
        password: &str
    ) -> DataResult<Option<UserContext>> {
        let user = sqlx::query!(
            "SELECT id, password_hash, role FROM users
             WHERE email = $1 AND is_active = TRUE",
            email
        ).fetch_optional(&self.pool).await?;

        match user {
            Some(u) if verify_password(password, &u.password_hash)? => {
                Ok(Some(UserContext {
                    user_id: u.id,
                    role: UserRole::from_str(&u.role)?,
                }))
            }
            _ => Ok(None)
        }
    }

    pub async fn create_session(&self, user_id: Uuid, ip: Option<IpAddr>) -> DataResult<String> {
        let token = generate_secure_token();
        let token_hash = hash_token(&token);
        let expires_at = Utc::now() + Duration::hours(24);

        sqlx::query!(
            "INSERT INTO sessions (user_id, token_hash, expires_at, ip_address)
             VALUES ($1, $2, $3, $4)",
            user_id, token_hash, expires_at, ip.map(|i| i.to_string())
        ).execute(&self.pool).await?;

        Ok(token)
    }

    pub async fn validate_session(&self, token: &str) -> DataResult<Option<UserContext>> {
        let token_hash = hash_token(token);

        let session = sqlx::query!(
            "SELECT s.user_id, u.role
             FROM sessions s
             JOIN users u ON s.user_id = u.id
             WHERE s.token_hash = $1
               AND s.expires_at > NOW()
               AND u.is_active = TRUE",
            token_hash
        ).fetch_optional(&self.pool).await?;

        // Update last_activity
        if session.is_some() {
            sqlx::query!(
                "UPDATE sessions SET last_activity_at = NOW()
                 WHERE token_hash = $1",
                token_hash
            ).execute(&self.pool).await?;
        }

        Ok(session.map(|s| UserContext {
            user_id: s.user_id,
            role: UserRole::from_str(&s.role).unwrap_or(UserRole::ReadOnly),
        }))
    }
}

fn hash_password(password: &str) -> Result<String, DataError> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    let argon2 = Argon2::default();

    argon2.hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| DataError::Validation(format!("Password hashing failed: {}", e)))
}

fn verify_password(password: &str, hash: &str) -> Result<bool, DataError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| DataError::Validation(format!("Invalid hash: {}", e)))?;

    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

fn generate_secure_token() -> String {
    use rand::Rng;
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

fn hash_token(token: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
```

#### 3.2 Update Cargo Dependencies

Add to `src-tauri/Cargo.toml`:
```toml
argon2 = { version = "0.5", features = ["std"] }
rand = "0.8"
sha2 = "0.10"
hex = "0.4"
uuid = { version = "1.0", features = ["v4", "serde"] }
```

**Critical Files**:
- `/home/mj/rust-trade/trading-common/src/data/user_repository.rs` (NEW)
- `/home/mj/rust-trade/src-tauri/Cargo.toml` (UPDATE)

---

### Phase 4: State Management Updates (Week 7-8)
**Goal**: Add session management to app state

#### 4.1 Extend AppState

```rust
// src-tauri/src/state.rs

use trading_common::data::user_repository::UserRepository;
use std::collections::HashMap;
use tokio::sync::RwLock;

pub struct AppState {
    pub repository: Arc<TickDataRepository>,
    pub user_repository: Arc<UserRepository>,
    pub session_manager: Arc<SessionManager>,
}

pub struct SessionManager {
    active_sessions: Arc<RwLock<HashMap<String, UserContext>>>,
    repository: Arc<UserRepository>,
}

impl SessionManager {
    pub fn new(repository: Arc<UserRepository>) -> Self {
        Self {
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            repository,
        }
    }

    pub async fn validate_token(&self, token: &str) -> Result<UserContext, String> {
        // Check in-memory cache first (fast path)
        if let Some(ctx) = self.active_sessions.read().await.get(token) {
            return Ok(ctx.clone());
        }

        // Validate against database
        let ctx = self.repository.validate_session(token).await
            .map_err(|e| format!("Session validation failed: {}", e))?
            .ok_or_else(|| "Invalid or expired session".to_string())?;

        // Cache for future requests
        self.active_sessions.write().await.insert(token.to_string(), ctx.clone());

        Ok(ctx)
    }

    pub async fn cleanup_expired_sessions(&self) {
        self.active_sessions.write().await.clear();
    }
}

impl AppState {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // ... existing initialization ...

        let pool_clone = pool.clone();
        let user_repository = Arc::new(UserRepository::new(pool_clone));
        let session_manager = Arc::new(SessionManager::new(Arc::clone(&user_repository)));

        Ok(Self {
            repository: Arc::new(repository),
            user_repository,
            session_manager,
        })
    }
}
```

**Critical Files**:
- `/home/mj/rust-trade/src-tauri/src/state.rs` (UPDATE)

---

### Phase 5: API Layer Updates (Week 9-10)
**Goal**: Add authenticated command variants

#### 5.1 Add Authentication Commands

```rust
// src-tauri/src/commands.rs

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: String,
    pub role: String,
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> Result<LoginResponse, String> {
    // Authenticate user
    let user_ctx = state.user_repository
        .authenticate(&email, &password)
        .await
        .map_err(|e| format!("Authentication failed: {}", e))?
        .ok_or_else(|| "Invalid credentials".to_string())?;

    // Create session
    let token = state.user_repository
        .create_session(user_ctx.user_id, None)
        .await
        .map_err(|e| format!("Session creation failed: {}", e))?;

    Ok(LoginResponse {
        token,
        user_id: user_ctx.user_id.to_string(),
        role: user_ctx.role.as_str().to_string(),
    })
}

#[tauri::command]
pub async fn logout(
    state: State<'_, AppState>,
    token: String,
) -> Result<(), String> {
    // Invalidate session in database
    // ... implementation
    Ok(())
}

// KEEP existing commands for backward compatibility
#[tauri::command]
pub async fn run_backtest(
    state: State<'_, AppState>,
    request: BacktestRequest,
) -> Result<BacktestResponse, String> {
    // Call new version with None user context
    run_backtest_with_context(state, None, request).await
}

// ADD new authenticated variant
#[tauri::command]
pub async fn run_backtest_authenticated(
    state: State<'_, AppState>,
    token: String,
    request: BacktestRequest,
) -> Result<BacktestResponse, String> {
    // Validate token
    let user_ctx = state.session_manager.validate_token(&token).await?;

    // Check permissions
    if !user_ctx.can_write() {
        return Err("Insufficient permissions".to_string());
    }

    run_backtest_with_context(state, Some(user_ctx), request).await
}

async fn run_backtest_with_context(
    state: State<'_, AppState>,
    user_ctx: Option<UserContext>,
    request: BacktestRequest,
) -> Result<BacktestResponse, String> {
    // Shared implementation with optional user context
    // ... existing backtest logic with user_ctx passed to repository
}
```

**Critical Files**:
- `/home/mj/rust-trade/src-tauri/src/commands.rs` (UPDATE)
- `/home/mj/rust-trade/src-tauri/src/types.rs` (UPDATE for new request/response types)

---

### Phase 6: Cache Multi-Tenancy (Week 11-12)
**Goal**: Support user-namespaced caching

#### 6.1 Update Cache Key Strategy

```rust
// trading-common/src/data/cache.rs

impl RedisTickCache {
    // KEEP existing method
    fn get_cache_key(&self, symbol: &str) -> String {
        self.get_cache_key_with_user(symbol, None)
    }

    // ADD user-aware variant
    fn get_cache_key_with_user(&self, symbol: &str, user_id: Option<Uuid>) -> String {
        match user_id {
            Some(uid) => format!("tick:user:{}:{}", uid, symbol),
            None => format!("tick:global:{}", symbol),
        }
    }
}

#[async_trait]
impl TickDataCache for TieredCache {
    // ADD user-aware cache methods
    async fn push_tick_for_user(
        &self,
        tick: &TickData,
        user_id: Option<Uuid>
    ) -> DataResult<()> {
        // Write to user-specific cache if user_id provided
        if let Some(uid) = user_id {
            self.redis_cache.push_tick_with_user(tick, Some(uid)).await?;
        }

        // Always update global cache for live data
        self.redis_cache.push_tick(tick).await?;
        self.memory_cache.push_tick(tick).await?;

        Ok(())
    }
}
```

**Critical Files**:
- `/home/mj/rust-trade/trading-common/src/data/cache.rs` (UPDATE)

---

### Phase 7: Security Hardening (Week 13-14)
**Goal**: Add critical security measures

#### 7.1 Rate Limiting

```rust
// src-tauri/src/middleware/rate_limit.rs (NEW FILE)

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct RateLimiter {
    requests: Arc<Mutex<HashMap<Uuid, VecDeque<Instant>>>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            requests: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window,
        }
    }

    pub async fn check_rate_limit(&self, user_id: Uuid) -> Result<(), String> {
        let now = Instant::now();
        let mut requests = self.requests.lock().await;

        let user_requests = requests.entry(user_id).or_insert_with(VecDeque::new);

        // Remove expired requests
        while let Some(req_time) = user_requests.front() {
            if now.duration_since(*req_time) > self.window {
                user_requests.pop_front();
            } else {
                break;
            }
        }

        // Check limit
        if user_requests.len() >= self.max_requests {
            return Err("Rate limit exceeded".to_string());
        }

        user_requests.push_back(now);
        Ok(())
    }
}
```

#### 7.2 Input Validation

```rust
// trading-common/src/validation/mod.rs (NEW FILE)

pub struct InputValidator;

impl InputValidator {
    pub fn validate_email(email: &str) -> Result<(), String> {
        if email.len() > 255 {
            return Err("Email too long".into());
        }

        let email_regex = regex::Regex::new(
            r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"
        ).unwrap();

        if !email_regex.is_match(email) {
            return Err("Invalid email format".into());
        }

        Ok(())
    }

    pub fn validate_password(password: &str) -> Result<(), String> {
        if password.len() < 8 || password.len() > 128 {
            return Err("Password must be 8-128 characters".into());
        }

        let has_lower = password.chars().any(|c| c.is_lowercase());
        let has_upper = password.chars().any(|c| c.is_uppercase());
        let has_digit = password.chars().any(|c| c.is_numeric());

        if !(has_lower && has_upper && has_digit) {
            return Err("Password must contain uppercase, lowercase, and digits".into());
        }

        Ok(())
    }
}
```

**Critical Files**:
- `/home/mj/rust-trade/src-tauri/src/middleware/rate_limit.rs` (NEW)
- `/home/mj/rust-trade/trading-common/src/validation/mod.rs` (NEW)

---

## Migration Strategy

### Backward Compatibility Approach

1. **Database**: Use NULLABLE `user_id` columns
   - NULL = global/shared data (existing behavior)
   - NOT NULL = user-specific data (new behavior)

2. **Repository**: Add `_with_context` method variants
   - Keep existing methods unchanged
   - New methods accept `Option<&UserContext>`
   - Gradual migration over time

3. **API**: Parallel command exposure
   - Keep deprecated commands for transition period
   - Add new `_authenticated` variants
   - Both work simultaneously

4. **Cache**: Dual namespace strategy
   - `tick:global:{symbol}` for shared data
   - `tick:user:{user_id}:{symbol}` for user data
   - No performance impact on existing flows

### Rollback Safety

Each phase is independently rollback-able:
- Schema changes use migrations with down scripts
- Code changes use feature flags and backward-compatible APIs
- No breaking changes to existing functionality

---

## Performance & Scalability Considerations

### Database
- **Connection pooling**: Already implemented (max 5 connections)
- **Indexing strategy**: Conditional indexes on user_id WHERE NOT NULL
- **Query optimization**: Use prepared statements (already done with sqlx)
- **Scaling**: Add read replicas when user count grows

### Caching
- **L1 (Memory)**: Keep global for live data (low latency)
- **L2 (Redis)**: User-namespaced for isolation
- **Eviction**: LRU per user with configurable limits
- **Scaling**: Redis Cluster for horizontal scaling

### Application
- **Async/await**: Already Tokio-based (good foundation)
- **Arc sharing**: Already using Arc for state (thread-safe)
- **Rate limiting**: Implement early to prevent abuse
- **Monitoring**: Add metrics collection (Prometheus/OpenTelemetry)

---

## Security Best Practices

### Authentication
- ✅ Argon2 password hashing (recommended by OWASP)
- ✅ Session tokens (32-byte cryptographically secure random)
- ✅ Token hashing before storage (SHA-256)
- ✅ Session expiration (24 hours with refresh)
- ⚠️ TODO: MFA support (future enhancement)

### Authorization
- ✅ Role-based access control (Admin/User/ReadOnly)
- ✅ Permission checks before operations
- ✅ Audit logging for accountability
- ⚠️ TODO: Resource-level permissions (future enhancement)

### Data Protection
- ✅ SQL injection prevention (parameterized queries)
- ✅ Input validation (email, password, symbols)
- ⚠️ TODO: Enable CSP in Tauri config
- ⚠️ TODO: Encryption at rest for sensitive data

---

## Verification & Testing

### Database Verification
```sql
-- Verify schema changes
\dt
\d+ users
\d+ sessions
\d+ tick_data

-- Test user creation
INSERT INTO users (email, password_hash, role)
VALUES ('test@example.com', '$argon2...', 'user');

-- Test backward compatibility
SELECT * FROM tick_data WHERE user_id IS NULL;
```

### Repository Testing
```rust
#[tokio::test]
async fn test_user_aware_query() {
    let repo = create_repository().await;
    let user_ctx = UserContext { user_id: Uuid::new_v4(), role: UserRole::User };

    let query = TickQuery { symbol: "BTCUSDT".into(), ..Default::default() };

    // Should only return user's data
    let ticks = repo.get_ticks_with_context(&query, Some(&user_ctx)).await?;
    assert!(ticks.iter().all(|t| t.user_id == Some(user_ctx.user_id)));
}
```

### API Testing
```bash
# Test authentication
curl -X POST http://localhost:1420/api/login \
  -d '{"email": "test@example.com", "password": "SecurePass123"}'

# Test authenticated backtest
curl -X POST http://localhost:1420/api/backtest \
  -H "Authorization: Bearer TOKEN" \
  -d '{"symbol": "BTCUSDT", ...}'
```

### Performance Testing
```bash
# Load test authentication
ab -n 1000 -c 10 -p login.json http://localhost:1420/api/login

# Measure query latency with user filtering
cargo bench -- benchmark_user_query
```

---

## Critical Files Summary

### Schema (2 files)
- `config/migrations/001_add_multi_user_support.sql` (NEW)
- `config/schema.sql` (UPDATE - add migration reference)

### Data Layer (4 files)
- `trading-common/src/data/types.rs` (UPDATE - add UserContext)
- `trading-common/src/data/repository.rs` (UPDATE - add _with_context methods)
- `trading-common/src/data/cache.rs` (UPDATE - user-namespaced keys)
- `trading-common/src/data/user_repository.rs` (NEW)

### Authentication (2 files)
- `trading-common/src/validation/mod.rs` (NEW)
- `src-tauri/src/middleware/rate_limit.rs` (NEW)

### Application Layer (3 files)
- `src-tauri/src/state.rs` (UPDATE - add SessionManager)
- `src-tauri/src/commands.rs` (UPDATE - add authenticated commands)
- `src-tauri/src/types.rs` (UPDATE - add auth types)

### Dependencies (1 file)
- `src-tauri/Cargo.toml` (UPDATE - add argon2, rand, sha2, hex)

**Total**: 12 files (5 new, 7 updated)

---

## Estimated Timeline

| Phase | Duration | Effort | Risk |
|-------|----------|--------|------|
| Database Foundation | 2 weeks | Medium | Low |
| Domain Model Updates | 2 weeks | Low | Low |
| Authentication Infrastructure | 2 weeks | Medium | Medium |
| State Management Updates | 2 weeks | Low | Low |
| API Layer Updates | 2 weeks | Medium | Low |
| Cache Multi-Tenancy | 2 weeks | Medium | Medium |
| Security Hardening | 2 weeks | High | Low |
| **Total** | **14 weeks** | **~350 hours** | **Low-Medium** |

---

## Conclusion

**Current State**: ❌ Not ready for multi-tenancy
**After Phase 1-7**: ✅ Production-ready multi-tenant architecture

**Key Strengths**:
- Well-structured codebase with clear separation of concerns
- Good use of Rust patterns (Arc, async/await, traits)
- Strong SQL injection protection via sqlx
- Existing caching layer ready for extension

**Key Gaps to Address**:
- Zero authentication (critical security gap)
- No user context anywhere in data flow
- Global shared state across all layers
- No audit trail or rate limiting

**Recommended Next Steps**:
1. Start with Phase 1 (Database Foundation) - low risk, enables everything else
2. Implement Phase 3 (Authentication) early - critical security requirement
3. Gradually add user context to repository methods - maintains backward compatibility
4. Test extensively with both authenticated and legacy flows
5. Migrate existing data to "system" user once authentication is stable

The phased approach ensures zero breaking changes while building a solid foundation for future multi-tenancy. Each phase adds value independently and can be deployed incrementally.
