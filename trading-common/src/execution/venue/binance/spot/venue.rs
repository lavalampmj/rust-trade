//! Binance Spot execution venue implementation.
//!
//! This module provides the `BinanceSpotVenue` struct that implements
//! all execution venue traits for Binance Spot trading.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::execution::venue::binance::common::{
    BinanceHmacSigner, BinanceUserDataStream, RawMessageCallback,
};
use crate::execution::venue::binance::config::BinanceVenueConfig;
use crate::execution::venue::binance::endpoints::{spot, BinanceEndpoints};
use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::http::{HttpClient, RateLimiter};
use crate::execution::venue::traits::{
    AccountQueryVenue, ExecutionCallback, ExecutionStreamVenue, OrderSubmissionVenue,
};
use crate::execution::venue::types::{BalanceInfo, OrderQueryResponse};
use crate::venue::{ConnectionStatus, VenueCapabilities, VenueConnection, VenueInfo};
use crate::orders::{ClientOrderId, Order, OrderType, TimeInForce, VenueOrderId};

use super::normalizer::SpotExecutionNormalizer;
use super::rest_client::SpotRestClient;

/// Binance Spot execution venue.
///
/// Implements all venue traits for Binance Spot trading:
/// - `ExecutionVenue`: Connection management
/// - `OrderSubmissionVenue`: Order submission, cancellation, queries
/// - `ExecutionStreamVenue`: Real-time execution reports via WebSocket
/// - `AccountQueryVenue`: Balance queries
///
/// # Example
///
/// ```ignore
/// let config = BinanceVenueConfig::spot_us();
/// let mut venue = BinanceSpotVenue::new(config)?;
///
/// // Connect to venue
/// venue.connect().await?;
///
/// // Query account balance
/// let balances = venue.query_balances().await?;
///
/// // Submit an order
/// let order = Order::market("BTCUSDT", OrderSide::Buy, dec!(0.001)).build()?;
/// let venue_order_id = venue.submit_order(&order).await?;
///
/// // Start execution stream
/// let callback = Arc::new(|event| { println!("{:?}", event); });
/// venue.start_execution_stream(callback, shutdown_rx).await?;
/// ```
pub struct BinanceSpotVenue {
    /// Venue configuration
    config: BinanceVenueConfig,
    /// Endpoints
    endpoints: BinanceEndpoints,
    /// Venue info
    info: VenueInfo,
    /// HTTP client
    http_client: Option<Arc<HttpClient>>,
    /// REST client
    rest_client: Option<SpotRestClient>,
    /// Execution normalizer
    normalizer: SpotExecutionNormalizer,
    /// Connection status
    connected: Arc<AtomicBool>,
    /// Stream status
    stream_active: Arc<AtomicBool>,
}

impl BinanceSpotVenue {
    /// Create a new Binance Spot venue.
    pub fn new(config: BinanceVenueConfig) -> VenueResult<Self> {
        let endpoints = BinanceEndpoints::for_config(
            config.platform,
            config.market_type,
            config.testnet,
        );

        let venue_id = format!(
            "binance_{}_spot{}",
            match config.platform {
                crate::execution::venue::binance::config::BinancePlatform::Com => "com",
                crate::execution::venue::binance::config::BinancePlatform::Us => "us",
            },
            if config.testnet { "_testnet" } else { "" }
        );

        let display_name = format!(
            "{} Spot{}",
            config.platform.display_name(),
            if config.testnet { " (Testnet)" } else { "" }
        );

        let info = VenueInfo::new(&venue_id, &display_name)
            .with_name("BINANCE")
            .with_exchanges(vec!["BINANCE".to_string()])
            .with_order_types(vec![
                OrderType::Market,
                OrderType::Limit,
                OrderType::Stop,
                OrderType::StopLimit,
                OrderType::LimitIfTouched, // Maps to TakeProfitLimit
            ])
            .with_tif(vec![TimeInForce::GTC, TimeInForce::IOC, TimeInForce::FOK])
            .with_capabilities(VenueCapabilities {
                supports_orders: true,
                supports_batch: true,
                max_orders_per_batch: 5,
                supports_stop_orders: true,
                supports_execution_stream: true,
                ..VenueCapabilities::default()
            });

        let normalizer = SpotExecutionNormalizer::new(&info.name);

        Ok(Self {
            config,
            endpoints,
            info,
            http_client: None,
            rest_client: None,
            normalizer,
            connected: Arc::new(AtomicBool::new(false)),
            stream_active: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create HTTP client with authentication.
    fn create_http_client(&self) -> VenueResult<HttpClient> {
        let api_key = self
            .config
            .base
            .auth
            .load_api_key()
            .ok_or_else(|| VenueError::Configuration("API key not found in environment".to_string()))?;

        let api_secret = self
            .config
            .base
            .auth
            .load_api_secret()
            .ok_or_else(|| VenueError::Configuration("API secret not found in environment".to_string()))?;

        let signer = BinanceHmacSigner::new(api_key, api_secret);
        let rate_limiter = RateLimiter::from_config(&self.config.base.rate_limits);

        // Use configured URL or default from endpoints
        let base_url = if self.config.base.rest.base_url.is_empty() {
            self.endpoints.rest_url.clone()
        } else {
            self.config.base.rest.base_url.clone()
        };

        HttpClient::new(base_url, Box::new(signer), rate_limiter, self.config.base.rest.clone())
    }
}

#[async_trait]
impl VenueConnection for BinanceSpotVenue {
    fn info(&self) -> &VenueInfo {
        &self.info
    }

    async fn connect(&mut self) -> VenueResult<()> {
        info!("Connecting to {}", self.info.display_name);

        // Create HTTP client
        let http_client = Arc::new(self.create_http_client()?);
        let rest_client = SpotRestClient::new(http_client.clone());

        // Test connection by pinging
        rest_client.ping().await.map_err(|e| {
            VenueError::Connection(format!("Failed to ping {}: {}", self.info.display_name, e))
        })?;

        // Verify credentials by querying account
        let account = rest_client.query_account().await.map_err(|e| {
            VenueError::Authentication(format!("Failed to authenticate: {}", e))
        })?;

        if !account.can_trade {
            return Err(VenueError::Authentication(
                "Account does not have trading permission".to_string(),
            ));
        }

        info!(
            "Connected to {} (account type: {})",
            self.info.display_name, account.account_type
        );

        self.http_client = Some(http_client);
        self.rest_client = Some(rest_client);
        self.connected.store(true, Ordering::SeqCst);

        Ok(())
    }

    async fn disconnect(&mut self) -> VenueResult<()> {
        info!("Disconnecting from {}", self.info.display_name);

        self.connected.store(false, Ordering::SeqCst);
        self.rest_client = None;
        self.http_client = None;

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn connection_status(&self) -> ConnectionStatus {
        if self.connected.load(Ordering::SeqCst) {
            ConnectionStatus::Connected
        } else {
            ConnectionStatus::Disconnected
        }
    }
}

#[async_trait]
impl OrderSubmissionVenue for BinanceSpotVenue {
    async fn submit_order(&self, order: &Order) -> VenueResult<VenueOrderId> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client.submit_order(order).await?;

        info!(
            "Order submitted: {} -> {} ({})",
            order.client_order_id, response.order_id, response.status
        );

        Ok(VenueOrderId::new(response.order_id.to_string()))
    }

    async fn cancel_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
    ) -> VenueResult<()> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let order_id = venue_order_id.and_then(|v| v.as_str().parse::<i64>().ok());

        rest_client
            .cancel_order(symbol, Some(client_order_id.as_str()), order_id)
            .await?;

        info!("Order cancelled: {}", client_order_id);
        Ok(())
    }

    async fn modify_order(
        &self,
        _client_order_id: &ClientOrderId,
        _venue_order_id: Option<&VenueOrderId>,
        _symbol: &str,
        _new_price: Option<Decimal>,
        _new_quantity: Option<Decimal>,
    ) -> VenueResult<VenueOrderId> {
        // Binance Spot doesn't support order modification
        // Would need to cancel and replace
        Err(VenueError::InvalidOrder(
            "Order modification not supported for Binance Spot".to_string(),
        ))
    }

    async fn query_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        symbol: &str,
    ) -> VenueResult<OrderQueryResponse> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let order_id = venue_order_id.and_then(|v| v.as_str().parse::<i64>().ok());

        let response = rest_client
            .query_order(symbol, Some(client_order_id.as_str()), order_id)
            .await?;

        Ok(self.normalizer.normalize_order_query(&response))
    }

    async fn query_open_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<OrderQueryResponse>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let responses = rest_client.query_open_orders(symbol).await?;

        Ok(responses
            .iter()
            .map(|r| self.normalizer.normalize_order_query(r))
            .collect())
    }

    async fn cancel_all_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<ClientOrderId>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let symbol = symbol.ok_or_else(|| {
            VenueError::InvalidOrder("Symbol is required for cancel_all_orders".to_string())
        })?;

        let responses = rest_client.cancel_all_orders(symbol).await?;

        let cancelled: Vec<ClientOrderId> = responses
            .iter()
            .map(|r| ClientOrderId::new(&r.orig_client_order_id))
            .collect();

        info!("Cancelled {} orders for {}", cancelled.len(), symbol);
        Ok(cancelled)
    }
}

#[async_trait]
impl ExecutionStreamVenue for BinanceSpotVenue {
    async fn start_execution_stream(
        &mut self,
        callback: ExecutionCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        let http_client = self
            .http_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?
            .clone();

        // Get WebSocket URL from config or use default
        let ws_url = if self.config.base.stream.ws_url.is_empty() {
            self.endpoints.user_data_ws_url.clone()
        } else {
            self.config.base.stream.ws_url.clone()
        };

        let stream = BinanceUserDataStream::new(
            http_client,
            self.config.base.stream.clone(),
            ws_url,
            spot::USER_DATA_STREAM,
        );

        let normalizer = SpotExecutionNormalizer::new(&self.info.name);
        let stream_active = self.stream_active.clone();

        // Create message handler
        let raw_callback: RawMessageCallback = Arc::new(move |msg: String| {
            match normalizer.parse_ws_message(&msg) {
                Ok(Some(report)) => {
                    if let Some(event) = normalizer.to_order_event(&report) {
                        callback(event);
                    }
                }
                Ok(None) => {
                    // Not an execution report (e.g., account update)
                }
                Err(e) => {
                    warn!("Failed to parse message: {}", e);
                }
            }
        });

        stream_active.store(true, Ordering::SeqCst);

        let result = stream.start(raw_callback, shutdown_rx).await;

        stream_active.store(false, Ordering::SeqCst);

        result
    }

    async fn stop_execution_stream(&mut self) -> VenueResult<()> {
        // Stream stops when shutdown signal is sent
        self.stream_active.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_stream_active(&self) -> bool {
        self.stream_active.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AccountQueryVenue for BinanceSpotVenue {
    async fn query_balances(&self) -> VenueResult<Vec<BalanceInfo>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let account = rest_client.query_account().await?;

        Ok(account
            .balances
            .into_iter()
            .filter(|b| b.free > Decimal::ZERO || b.locked > Decimal::ZERO)
            .map(|b| BalanceInfo::new(b.asset, b.free, b.locked))
            .collect())
    }

    async fn query_balance(&self, asset: &str) -> VenueResult<BalanceInfo> {
        let balances = self.query_balances().await?;

        balances
            .into_iter()
            .find(|b| b.asset.eq_ignore_ascii_case(asset))
            .ok_or_else(|| {
                VenueError::SymbolNotFound(format!("Asset not found: {}", asset))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::VenueConnection;

    #[test]
    fn test_venue_creation() {
        let config = BinanceVenueConfig::spot_com();
        let venue = BinanceSpotVenue::new(config).unwrap();

        assert!(venue.info.venue_id.contains("binance"));
        assert!(venue.info.venue_id.contains("spot"));
        assert!(venue.info.supports_order_type(&OrderType::Market));
        assert!(venue.info.supports_order_type(&OrderType::Limit));
    }

    #[test]
    fn test_venue_testnet() {
        let config = BinanceVenueConfig::spot_com().with_testnet(true);
        let venue = BinanceSpotVenue::new(config).unwrap();

        assert!(venue.info.display_name.contains("Testnet"));
        assert!(venue.endpoints.rest_url.contains("testnet"));
    }

    #[test]
    fn test_initial_state() {
        let config = BinanceVenueConfig::spot_us();
        let venue = BinanceSpotVenue::new(config).unwrap();

        assert!(!venue.is_connected());
        assert!(!venue.is_stream_active());
        assert_eq!(venue.connection_status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_venue_info_capabilities() {
        let config = BinanceVenueConfig::spot_us();
        let venue = BinanceSpotVenue::new(config).unwrap();
        let info = venue.info();

        // Check supported order types
        assert!(info.supported_order_types.contains(&OrderType::Market));
        assert!(info.supported_order_types.contains(&OrderType::Limit));
        assert!(info.supported_order_types.contains(&OrderType::StopLimit));

        // Check supported time-in-force
        assert!(info.supported_tif.contains(&TimeInForce::GTC));
        assert!(info.supported_tif.contains(&TimeInForce::IOC));
        assert!(info.supported_tif.contains(&TimeInForce::FOK));

        // Check capabilities
        assert!(info.supports_stop_orders());
        assert!(info.max_orders_per_batch() > 0);
    }

    #[test]
    fn test_venue_info_name() {
        let config = BinanceVenueConfig::spot_com();
        let venue = BinanceSpotVenue::new(config).unwrap();

        assert_eq!(venue.info().name, "BINANCE");
        assert!(venue.info().venue_id.contains("binance"));
    }

    #[test]
    fn test_different_platforms() {
        // Test Binance.com
        let com_config = BinanceVenueConfig::spot_com();
        let com_venue = BinanceSpotVenue::new(com_config).unwrap();
        assert!(com_venue.endpoints.rest_url.contains("api.binance.com"));

        // Test Binance.US
        let us_config = BinanceVenueConfig::spot_us();
        let us_venue = BinanceSpotVenue::new(us_config).unwrap();
        assert!(us_venue.endpoints.rest_url.contains("api.binance.us"));
    }

    #[test]
    fn test_config_with_custom_env_vars() {
        let mut config = BinanceVenueConfig::spot_us();
        config.base.auth.api_key_env = "CUSTOM_API_KEY".to_string();
        config.base.auth.api_secret_env = "CUSTOM_API_SECRET".to_string();

        let venue = BinanceSpotVenue::new(config).unwrap();
        // Venue should be created even if env vars don't exist
        // (credentials are read at connect time)
        assert!(!venue.is_connected());
    }

    #[test]
    fn test_connection_status_transitions() {
        let config = BinanceVenueConfig::spot_us();
        let venue = BinanceSpotVenue::new(config).unwrap();

        // Initial state
        assert_eq!(venue.connection_status(), ConnectionStatus::Disconnected);

        // Note: Can't test actual connection without mocking HTTP
        // But we can verify the state machine is properly initialized
        assert!(!venue.connected.load(std::sync::atomic::Ordering::SeqCst));
        assert!(!venue.stream_active.load(std::sync::atomic::Ordering::SeqCst));
    }
}
