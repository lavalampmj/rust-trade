//! Kraken Spot execution venue implementation.
//!
//! This module provides the `KrakenSpotVenue` struct that implements
//! all execution venue traits for Kraken Spot trading.
//!
//! Key features:
//! - True order modification via AmendOrder
//! - WebSocket V2 execution stream
//! - No listen key keepalive needed (unlike Binance)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::kraken::common::{
    KrakenHmacSigner, KrakenUserStream, KrakenWsTokenManager, RawMessageCallback,
};
use crate::execution::venue::kraken::config::KrakenVenueConfig;
use crate::execution::venue::kraken::endpoints::KrakenEndpoints;
use crate::execution::venue::traits::{
    AccountQueryVenue, ExecutionCallback, ExecutionStreamVenue, OrderSubmissionVenue,
};
use crate::execution::venue::types::{BalanceInfo, OrderQueryResponse};
use crate::venue::{VenueCapabilities, VenueInfo};
use crate::venue::{ConnectionStatus, VenueConnection};
use crate::orders::{ClientOrderId, Order, OrderType, TimeInForce, VenueOrderId};

use super::normalizer::SpotExecutionNormalizer;
use super::rest_client::SpotRestClient;

/// Kraken Spot execution venue.
///
/// Implements all venue traits for Kraken Spot trading:
/// - `ExecutionVenue`: Connection management
/// - `OrderSubmissionVenue`: Order submission, cancellation, modification
/// - `ExecutionStreamVenue`: Real-time execution reports via WebSocket
/// - `AccountQueryVenue`: Balance queries
///
/// # Key Features
///
/// - **True Order Modification**: Unlike Binance, Kraken supports in-place order
///   amendment that preserves queue priority.
/// - **No Keepalive**: WebSocket token doesn't expire once connected.
/// - **WebSocket V2**: Uses modern JSON-RPC style messages.
///
/// # Example
///
/// ```ignore
/// let config = KrakenVenueConfig::production();
/// let mut venue = KrakenSpotVenue::new(config)?;
///
/// // Connect to venue
/// venue.connect().await?;
///
/// // Query account balance
/// let balances = venue.query_balances().await?;
///
/// // Submit an order
/// let order = Order::limit("BTCUSDT", OrderSide::Buy, dec!(0.001), dec!(50000)).build()?;
/// let venue_order_id = venue.submit_order(&order).await?;
///
/// // Modify order price (preserves queue priority!)
/// let new_id = venue.modify_order(&order.client_order_id, Some(&venue_order_id), "BTCUSDT", Some(dec!(49000)), None).await?;
/// ```
pub struct KrakenSpotVenue {
    /// Venue configuration
    config: KrakenVenueConfig,
    /// Endpoints
    endpoints: KrakenEndpoints,
    /// Venue info
    info: VenueInfo,
    /// HTTP client
    http_client: Option<reqwest::Client>,
    /// Request signer
    signer: Option<Arc<KrakenHmacSigner>>,
    /// REST client
    rest_client: Option<SpotRestClient>,
    /// Token manager for WebSocket
    token_manager: Option<Arc<KrakenWsTokenManager>>,
    /// Execution normalizer
    normalizer: SpotExecutionNormalizer,
    /// Connection status
    connected: Arc<AtomicBool>,
    /// Stream status
    stream_active: Arc<AtomicBool>,
}

impl KrakenSpotVenue {
    /// Create a new Kraken Spot venue.
    pub fn new(config: KrakenVenueConfig) -> VenueResult<Self> {
        let endpoints = KrakenEndpoints::for_config(config.sandbox);

        let venue_id = format!(
            "kraken_spot{}",
            if config.sandbox { "_sandbox" } else { "" }
        );

        // Note: Kraken has no public spot sandbox, so "sandbox" mode still uses production
        // The display name reflects this to avoid confusion
        let display_name = format!(
            "Kraken Spot{}",
            if config.sandbox { " (Test Mode - Production API)" } else { "" }
        );

        let info = VenueInfo::new(&venue_id, &display_name)
            .with_name("KRAKEN")
            .with_exchanges(vec!["KRAKEN".to_string()])
            .with_order_types(vec![
                OrderType::Market,
                OrderType::Limit,
                OrderType::Stop,
                OrderType::StopLimit,
                OrderType::LimitIfTouched, // TakeProfit
                OrderType::TrailingStop,
            ])
            .with_tif(vec![
                TimeInForce::GTC,
                TimeInForce::IOC,
                TimeInForce::GTD,
            ])
            .with_capabilities(VenueCapabilities {
                supports_orders: true,
                supports_batch: true,
                max_orders_per_batch: 15, // Kraken supports 2-15 orders in batch
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
            signer: None,
            rest_client: None,
            token_manager: None,
            normalizer,
            connected: Arc::new(AtomicBool::new(false)),
            stream_active: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create the HTTP client and signer.
    fn create_clients(&self) -> VenueResult<(reqwest::Client, Arc<KrakenHmacSigner>)> {
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

        let signer = Arc::new(KrakenHmacSigner::new(api_key, &api_secret)?);

        let http_client = reqwest::Client::builder()
            .timeout(self.config.base.rest.timeout())
            .build()
            .map_err(|e| VenueError::Configuration(format!("Failed to create HTTP client: {}", e)))?;

        Ok((http_client, signer))
    }
}

#[async_trait]
impl VenueConnection for KrakenSpotVenue {
    fn info(&self) -> &VenueInfo {
        &self.info
    }

    async fn connect(&mut self) -> VenueResult<()> {
        info!("Connecting to {}", self.info.display_name);

        // Create HTTP client and signer
        let (http_client, signer) = self.create_clients()?;

        // Use configured URL or default from endpoints
        let base_url = if self.config.base.rest.base_url.is_empty() {
            self.endpoints.rest_url.clone()
        } else {
            self.config.base.rest.base_url.clone()
        };

        let rest_client = SpotRestClient::new(http_client.clone(), signer.clone(), &base_url);

        // Test connection by checking system status
        let status = rest_client.system_status().await.map_err(|e| {
            VenueError::Connection(format!("Failed to connect to {}: {}", self.info.display_name, e))
        })?;

        if status.status.to_lowercase() != "online" {
            return Err(VenueError::Connection(format!(
                "Kraken system status is not online: {}",
                status.status
            )));
        }

        // Verify credentials by querying balance
        let _balance = rest_client.query_balance().await.map_err(|e| {
            VenueError::Authentication(format!("Failed to authenticate: {}", e))
        })?;

        info!(
            "Connected to {} (system: {})",
            self.info.display_name, status.status
        );

        // Create token manager for WebSocket
        let token_manager = Arc::new(KrakenWsTokenManager::new(
            http_client.clone(),
            signer.clone(),
            &base_url,
        ));

        self.http_client = Some(http_client);
        self.signer = Some(signer);
        self.rest_client = Some(rest_client);
        self.token_manager = Some(token_manager);
        self.connected.store(true, Ordering::SeqCst);

        Ok(())
    }

    async fn disconnect(&mut self) -> VenueResult<()> {
        info!("Disconnecting from {}", self.info.display_name);

        self.connected.store(false, Ordering::SeqCst);
        self.rest_client = None;
        self.token_manager = None;
        self.signer = None;
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
impl OrderSubmissionVenue for KrakenSpotVenue {
    async fn submit_order(&self, order: &Order) -> VenueResult<VenueOrderId> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client.submit_order(order).await?;

        let venue_order_id = response
            .txid
            .first()
            .ok_or_else(|| VenueError::Parse("No txid in response".to_string()))?;

        info!(
            "Order submitted: {} -> {} ({})",
            order.client_order_id, venue_order_id, response.descr.order
        );

        Ok(VenueOrderId::new(venue_order_id.clone()))
    }

    async fn cancel_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        _symbol: &str,
    ) -> VenueResult<()> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client
            .cancel_order(Some(client_order_id.as_str()), venue_order_id.map(|v| v.as_str()))
            .await?;

        if response.count > 0 {
            info!("Order cancelled: {} (count: {})", client_order_id, response.count);
            Ok(())
        } else {
            Err(VenueError::OrderNotFound(format!(
                "Order not found: {}",
                client_order_id
            )))
        }
    }

    async fn modify_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        _symbol: &str,
        new_price: Option<Decimal>,
        new_quantity: Option<Decimal>,
    ) -> VenueResult<VenueOrderId> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        // Kraken supports true order modification!
        let response = rest_client
            .amend_order(
                Some(client_order_id.as_str()),
                venue_order_id.map(|v| v.as_str()),
                new_price,
                new_quantity,
            )
            .await?;

        let new_txid = response
            .amend_id
            .or(response.txid)
            .ok_or_else(|| VenueError::Parse("No txid in amend response".to_string()))?;

        info!("Order amended: {} -> {}", client_order_id, new_txid);

        Ok(VenueOrderId::new(new_txid))
    }

    async fn query_order(
        &self,
        client_order_id: &ClientOrderId,
        venue_order_id: Option<&VenueOrderId>,
        _symbol: &str,
    ) -> VenueResult<OrderQueryResponse> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let order_info = rest_client
            .query_order(Some(client_order_id.as_str()), venue_order_id.map(|v| v.as_str()))
            .await?;

        let txid = venue_order_id
            .map(|v| v.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        Ok(self.normalizer.normalize_order_query(&txid, &order_info))
    }

    async fn query_open_orders(&self, _symbol: Option<&str>) -> VenueResult<Vec<OrderQueryResponse>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client.query_open_orders().await?;

        Ok(response
            .open
            .iter()
            .map(|(txid, info)| self.normalizer.normalize_order_query(txid, info))
            .collect())
    }

    async fn cancel_all_orders(&self, _symbol: Option<&str>) -> VenueResult<Vec<ClientOrderId>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        // First get open orders to return their IDs
        let open_orders = rest_client.query_open_orders().await?;
        let client_order_ids: Vec<ClientOrderId> = open_orders
            .open
            .values()
            .filter_map(|info| info.cl_ord_id.as_ref().map(ClientOrderId::new))
            .collect();

        // Cancel all
        let response = rest_client.cancel_all().await?;

        info!("Cancelled {} orders", response.count);

        Ok(client_order_ids)
    }
}

#[async_trait]
impl ExecutionStreamVenue for KrakenSpotVenue {
    async fn start_execution_stream(
        &mut self,
        callback: ExecutionCallback,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        let token_manager = self
            .token_manager
            .as_ref()
            .ok_or(VenueError::NotConnected)?
            .clone();

        // Get WebSocket URL from config or use default
        let ws_url = if self.config.base.stream.ws_url.is_empty() {
            self.endpoints.ws_auth_url.clone()
        } else {
            self.config.base.stream.ws_url.clone()
        };

        let stream = KrakenUserStream::new(
            token_manager,
            self.config.base.stream.clone(),
            ws_url,
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
                    // Not an execution report (e.g., heartbeat)
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
        self.stream_active.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_stream_active(&self) -> bool {
        self.stream_active.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl AccountQueryVenue for KrakenSpotVenue {
    async fn query_balances(&self) -> VenueResult<Vec<BalanceInfo>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let balances = rest_client.query_balance().await?;

        Ok(balances
            .non_zero()
            .into_iter()
            .map(|(asset, balance)| {
                // Kraken doesn't separate free/locked in simple balance query
                // Would need BalanceEx for that
                BalanceInfo::new(asset, balance, Decimal::ZERO)
            })
            .collect())
    }

    async fn query_balance(&self, asset: &str) -> VenueResult<BalanceInfo> {
        let balances = self.query_balances().await?;

        // Kraken uses X prefix for crypto (XXBT) and Z for fiat (ZUSD)
        let search_asset = asset.to_uppercase();
        let search_variants = vec![
            search_asset.clone(),
            format!("X{}", search_asset),
            format!("Z{}", search_asset),
        ];

        balances
            .into_iter()
            .find(|b| search_variants.contains(&b.asset.to_uppercase()))
            .ok_or_else(|| VenueError::SymbolNotFound(format!("Asset not found: {}", asset)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::VenueConnection;

    #[test]
    fn test_venue_creation() {
        let config = KrakenVenueConfig::production();
        let venue = KrakenSpotVenue::new(config).unwrap();

        assert!(venue.info.venue_id.contains("kraken"));
        assert!(venue.info.venue_id.contains("spot"));
        assert!(venue.info.supports_order_type(&OrderType::Market));
        assert!(venue.info.supports_order_type(&OrderType::Limit));
    }

    #[test]
    fn test_venue_sandbox() {
        let config = KrakenVenueConfig::sandbox();
        let venue = KrakenSpotVenue::new(config).unwrap();

        // Kraken has no public spot sandbox - "sandbox" mode uses production API
        assert!(venue.info.display_name.contains("Test Mode"));
        // Uses production endpoints since Kraken has no spot testnet
        assert!(venue.endpoints.rest_url.contains("api.kraken.com"));
    }

    #[test]
    fn test_initial_state() {
        let config = KrakenVenueConfig::production();
        let venue = KrakenSpotVenue::new(config).unwrap();

        assert!(!venue.is_connected());
        assert!(!venue.is_stream_active());
        assert_eq!(venue.connection_status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_venue_info_capabilities() {
        let config = KrakenVenueConfig::production();
        let venue = KrakenSpotVenue::new(config).unwrap();
        let info = venue.info();

        // Check supported order types
        assert!(info.supported_order_types.contains(&OrderType::Market));
        assert!(info.supported_order_types.contains(&OrderType::Limit));
        assert!(info.supported_order_types.contains(&OrderType::StopLimit));
        assert!(info.supported_order_types.contains(&OrderType::TrailingStop));

        // Check supported time-in-force
        assert!(info.supported_tif.contains(&TimeInForce::GTC));
        assert!(info.supported_tif.contains(&TimeInForce::IOC));

        // Check capabilities
        assert!(info.supports_stop_orders());
        assert!(info.max_orders_per_batch() > 0);
    }

    #[test]
    fn test_venue_info_name() {
        let config = KrakenVenueConfig::production();
        let venue = KrakenSpotVenue::new(config).unwrap();

        assert_eq!(venue.info().name, "KRAKEN");
        assert!(venue.info().venue_id.contains("kraken"));
    }
}
