// metrics.rs - Prometheus metrics for trading system monitoring

use lazy_static::lazy_static;
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, IntCounter, IntGauge, Registry,
};

lazy_static! {
    /// Global Prometheus registry
    pub static ref REGISTRY: Registry = Registry::new();

    // ============================================================================
    // Tick Processing Metrics
    // ============================================================================

    /// Total number of ticks received from exchange
    pub static ref TICKS_RECEIVED_TOTAL: IntCounter = IntCounter::new(
        "trading_ticks_received_total",
        "Total number of ticks received from exchange"
    ).expect("Failed to create ticks_received_total metric");

    /// Total number of ticks processed successfully
    pub static ref TICKS_PROCESSED_TOTAL: IntCounter = IntCounter::new(
        "trading_ticks_processed_total",
        "Total number of ticks processed successfully"
    ).expect("Failed to create ticks_processed_total metric");

    /// Current channel buffer utilization (0-100%)
    pub static ref CHANNEL_UTILIZATION: Gauge = Gauge::new(
        "trading_channel_utilization_percent",
        "Current utilization of tick processing channel (0-100%)"
    ).expect("Failed to create channel_utilization metric");

    /// Current number of ticks in channel buffer
    pub static ref CHANNEL_BUFFER_SIZE: IntGauge = IntGauge::new(
        "trading_channel_buffer_size",
        "Current number of ticks in processing channel buffer"
    ).expect("Failed to create channel_buffer_size metric");

    // ============================================================================
    // Batch Processing Metrics
    // ============================================================================

    /// Total number of batches flushed to database
    pub static ref BATCHES_FLUSHED_TOTAL: IntCounter = IntCounter::new(
        "trading_batches_flushed_total",
        "Total number of batches successfully flushed to database"
    ).expect("Failed to create batches_flushed_total metric");

    /// Total number of failed batch inserts
    pub static ref BATCHES_FAILED_TOTAL: IntCounter = IntCounter::new(
        "trading_batches_failed_total",
        "Total number of batch inserts that failed after retries"
    ).expect("Failed to create batches_failed_total metric");

    /// Total number of batch retry attempts
    pub static ref BATCH_RETRIES_TOTAL: IntCounter = IntCounter::new(
        "trading_batch_retries_total",
        "Total number of batch insert retry attempts"
    ).expect("Failed to create batch_retries_total metric");

    /// Current batch buffer size (number of ticks)
    pub static ref BATCH_BUFFER_SIZE: IntGauge = IntGauge::new(
        "trading_batch_buffer_size",
        "Current number of ticks in batch buffer"
    ).expect("Failed to create batch_buffer_size metric");

    /// Histogram of batch insert durations
    pub static ref BATCH_INSERT_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "trading_batch_insert_duration_seconds",
            "Duration of batch insert operations in seconds"
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0])
    ).expect("Failed to create batch_insert_duration metric");

    // ============================================================================
    // Cache Metrics
    // ============================================================================

    /// Total cache hits
    pub static ref CACHE_HITS_TOTAL: IntCounter = IntCounter::new(
        "trading_cache_hits_total",
        "Total number of cache hits"
    ).expect("Failed to create cache_hits_total metric");

    /// Total cache misses
    pub static ref CACHE_MISSES_TOTAL: IntCounter = IntCounter::new(
        "trading_cache_misses_total",
        "Total number of cache misses"
    ).expect("Failed to create cache_misses_total metric");

    /// Total cache update failures
    pub static ref CACHE_UPDATE_FAILURES_TOTAL: IntCounter = IntCounter::new(
        "trading_cache_update_failures_total",
        "Total number of cache update failures"
    ).expect("Failed to create cache_update_failures_total metric");

    // ============================================================================
    // Database Connection Pool Metrics
    // ============================================================================

    /// Current number of active database connections
    pub static ref DB_CONNECTIONS_ACTIVE: IntGauge = IntGauge::new(
        "trading_db_connections_active",
        "Current number of active database connections"
    ).expect("Failed to create db_connections_active metric");

    /// Current number of idle database connections
    pub static ref DB_CONNECTIONS_IDLE: IntGauge = IntGauge::new(
        "trading_db_connections_idle",
        "Current number of idle database connections in pool"
    ).expect("Failed to create db_connections_idle metric");

    /// Total number of connection pool wait events
    pub static ref DB_POOL_WAITS_TOTAL: IntCounter = IntCounter::new(
        "trading_db_pool_waits_total",
        "Total number of times a connection had to wait for pool availability"
    ).expect("Failed to create db_pool_waits_total metric");

    // ============================================================================
    // WebSocket Connection Metrics
    // ============================================================================

    /// Total WebSocket connection attempts
    pub static ref WS_CONNECTIONS_TOTAL: IntCounter = IntCounter::new(
        "trading_ws_connections_total",
        "Total number of WebSocket connection attempts"
    ).expect("Failed to create ws_connections_total metric");

    /// Total WebSocket disconnections
    pub static ref WS_DISCONNECTIONS_TOTAL: IntCounter = IntCounter::new(
        "trading_ws_disconnections_total",
        "Total number of WebSocket disconnections"
    ).expect("Failed to create ws_disconnections_total metric");

    /// Total WebSocket reconnection attempts
    pub static ref WS_RECONNECTS_TOTAL: IntCounter = IntCounter::new(
        "trading_ws_reconnects_total",
        "Total number of WebSocket reconnection attempts"
    ).expect("Failed to create ws_reconnects_total metric");

    /// Current WebSocket connection status (1=connected, 0=disconnected)
    pub static ref WS_CONNECTION_STATUS: IntGauge = IntGauge::new(
        "trading_ws_connection_status",
        "Current WebSocket connection status (1=connected, 0=disconnected)"
    ).expect("Failed to create ws_connection_status metric");

    // ============================================================================
    // Paper Trading Metrics
    // ============================================================================

    /// Total number of paper trades executed
    pub static ref PAPER_TRADES_TOTAL: IntCounter = IntCounter::new(
        "trading_paper_trades_total",
        "Total number of paper trades executed"
    ).expect("Failed to create paper_trades_total metric");

    /// Current paper trading portfolio value
    pub static ref PAPER_PORTFOLIO_VALUE: Gauge = Gauge::new(
        "trading_paper_portfolio_value",
        "Current paper trading portfolio value in USD"
    ).expect("Failed to create paper_portfolio_value metric");

    /// Current paper trading profit/loss
    pub static ref PAPER_PNL: Gauge = Gauge::new(
        "trading_paper_pnl",
        "Current paper trading profit/loss in USD"
    ).expect("Failed to create paper_pnl metric");

    /// Histogram of paper trading tick processing duration
    pub static ref PAPER_TICK_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "trading_paper_tick_duration_seconds",
            "Duration of paper trading tick processing in seconds"
        )
        .buckets(vec![0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5])
    ).expect("Failed to create paper_tick_duration metric");

    // ============================================================================
    // System Health Metrics
    // ============================================================================

    /// Application uptime in seconds
    pub static ref UPTIME_SECONDS: IntGauge = IntGauge::new(
        "trading_uptime_seconds",
        "Application uptime in seconds"
    ).expect("Failed to create uptime_seconds metric");

    /// Total number of errors logged
    pub static ref ERRORS_TOTAL: IntCounter = IntCounter::new(
        "trading_errors_total",
        "Total number of errors logged by the application"
    ).expect("Failed to create errors_total metric");
}

/// Register all metrics with the global registry
pub fn register_metrics() -> Result<(), Box<dyn std::error::Error>> {
    // Tick processing metrics
    REGISTRY.register(Box::new(TICKS_RECEIVED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(TICKS_PROCESSED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(CHANNEL_UTILIZATION.clone()))?;
    REGISTRY.register(Box::new(CHANNEL_BUFFER_SIZE.clone()))?;

    // Batch processing metrics
    REGISTRY.register(Box::new(BATCHES_FLUSHED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(BATCHES_FAILED_TOTAL.clone()))?;
    REGISTRY.register(Box::new(BATCH_RETRIES_TOTAL.clone()))?;
    REGISTRY.register(Box::new(BATCH_BUFFER_SIZE.clone()))?;
    REGISTRY.register(Box::new(BATCH_INSERT_DURATION.clone()))?;

    // Cache metrics
    REGISTRY.register(Box::new(CACHE_HITS_TOTAL.clone()))?;
    REGISTRY.register(Box::new(CACHE_MISSES_TOTAL.clone()))?;
    REGISTRY.register(Box::new(CACHE_UPDATE_FAILURES_TOTAL.clone()))?;

    // Database connection pool metrics
    REGISTRY.register(Box::new(DB_CONNECTIONS_ACTIVE.clone()))?;
    REGISTRY.register(Box::new(DB_CONNECTIONS_IDLE.clone()))?;
    REGISTRY.register(Box::new(DB_POOL_WAITS_TOTAL.clone()))?;

    // WebSocket connection metrics
    REGISTRY.register(Box::new(WS_CONNECTIONS_TOTAL.clone()))?;
    REGISTRY.register(Box::new(WS_DISCONNECTIONS_TOTAL.clone()))?;
    REGISTRY.register(Box::new(WS_RECONNECTS_TOTAL.clone()))?;
    REGISTRY.register(Box::new(WS_CONNECTION_STATUS.clone()))?;

    // Paper trading metrics
    REGISTRY.register(Box::new(PAPER_TRADES_TOTAL.clone()))?;
    REGISTRY.register(Box::new(PAPER_PORTFOLIO_VALUE.clone()))?;
    REGISTRY.register(Box::new(PAPER_PNL.clone()))?;
    REGISTRY.register(Box::new(PAPER_TICK_DURATION.clone()))?;

    // System health metrics
    REGISTRY.register(Box::new(UPTIME_SECONDS.clone()))?;
    REGISTRY.register(Box::new(ERRORS_TOTAL.clone()))?;

    Ok(())
}

/// Start the Prometheus metrics HTTP server
pub async fn start_metrics_server(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    use hyper::{
        service::{make_service_fn, service_fn},
        Body, Method, Request, Response, Server, StatusCode,
    };
    use prometheus::TextEncoder;
    use std::net::SocketAddr;

    async fn serve_metrics(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        let encoder = TextEncoder::new();
        let metric_families = REGISTRY.gather();

        let mut buffer = Vec::new();
        if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
            eprintln!("Failed to encode metrics: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Failed to encode metrics"))
                .unwrap());
        }

        Ok(Response::new(Body::from(buffer)))
    }

    async fn handle_request(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        match (req.method(), req.uri().path()) {
            (&Method::GET, "/metrics") => serve_metrics(req).await,
            (&Method::GET, "/health") => Ok(Response::new(Body::from("OK"))),
            _ => Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))
                .unwrap()),
        }
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let make_svc = make_service_fn(|_conn| async { Ok::<_, hyper::Error>(service_fn(handle_request)) });

    let server = Server::bind(&addr).serve(make_svc);

    tracing::info!("Prometheus metrics server listening on http://{}/metrics", addr);
    tracing::info!("Health check endpoint available at http://{}/health", addr);

    server.await?;

    Ok(())
}

/// Start a background task to update uptime metrics
pub fn start_uptime_monitor() {
    use std::time::Instant;
    use tokio::time::{interval, Duration};

    tokio::spawn(async move {
        let start_time = Instant::now();
        let mut ticker = interval(Duration::from_secs(1));

        loop {
            ticker.tick().await;
            let uptime = start_time.elapsed().as_secs() as i64;
            UPTIME_SECONDS.set(uptime);
        }
    });
}
