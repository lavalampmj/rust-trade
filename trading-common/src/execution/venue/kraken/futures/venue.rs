//! Kraken Futures execution venue implementation.
//!
//! This module provides the `KrakenFuturesVenue` struct that implements
//! all execution venue traits for Kraken Futures (derivatives) trading.
//!
//! # Key Features
//!
//! - **True Demo Environment**: Kraken Futures has a fully functional demo!
//! - **Perpetual Contracts**: PI_XBTUSD, PI_ETHUSD, etc.
//! - **Leverage Support**: Configurable up to 50x
//! - **WebSocket V1**: Different protocol from Spot V2

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::kraken::common::KrakenFuturesSigner;
use crate::execution::venue::kraken::config::KrakenFuturesVenueConfig;
use crate::execution::venue::kraken::endpoints::KrakenFuturesEndpoints;
use crate::execution::venue::traits::{
    AccountQueryVenue, ExecutionCallback, ExecutionStreamVenue, OrderSubmissionVenue,
};
use crate::execution::venue::types::{BalanceInfo, OrderQueryResponse, VenueInfo};
use crate::venue::{ConnectionStatus, VenueConnection};
use crate::orders::{ClientOrderId, Order, OrderType, TimeInForce, VenueOrderId};

use super::normalizer::FuturesExecutionNormalizer;
use super::rest_client::FuturesRestClient;

/// Kraken Futures execution venue.
///
/// Implements all venue traits for Kraken Futures trading:
/// - `ExecutionVenue`: Connection management
/// - `OrderSubmissionVenue`: Order submission, cancellation, modification
/// - `ExecutionStreamVenue`: Real-time execution reports via WebSocket
/// - `AccountQueryVenue`: Balance queries
///
/// # Demo Environment
///
/// Unlike Spot, Kraken Futures has a fully functional demo environment!
/// Use `KrakenFuturesVenueConfig::demo()` for testing.
///
/// # Example
///
/// ```ignore
/// let config = KrakenFuturesVenueConfig::demo();
/// let mut venue = KrakenFuturesVenue::new(config)?;
///
/// venue.connect().await?;
/// let balances = venue.query_balances().await?;
/// ```
pub struct KrakenFuturesVenue {
    /// Venue configuration
    config: KrakenFuturesVenueConfig,
    /// Endpoints
    endpoints: KrakenFuturesEndpoints,
    /// Venue info
    info: VenueInfo,
    /// HTTP client
    http_client: Option<reqwest::Client>,
    /// Request signer (Futures-specific)
    signer: Option<Arc<KrakenFuturesSigner>>,
    /// REST client
    rest_client: Option<FuturesRestClient>,
    /// Execution normalizer
    normalizer: FuturesExecutionNormalizer,
    /// Connection status
    connected: Arc<AtomicBool>,
    /// Stream status
    stream_active: Arc<AtomicBool>,
}

impl KrakenFuturesVenue {
    /// Create a new Kraken Futures venue.
    pub fn new(config: KrakenFuturesVenueConfig) -> VenueResult<Self> {
        let endpoints = KrakenFuturesEndpoints::for_config(config.demo);

        let venue_id = format!(
            "kraken_futures{}",
            if config.demo { "_demo" } else { "" }
        );

        let display_name = format!(
            "Kraken Futures{}",
            if config.demo { " (Demo)" } else { "" }
        );

        let info = VenueInfo::new(&venue_id, &display_name)
            .with_name("KRAKEN_FUTURES")
            .with_order_types(vec![
                OrderType::Market,
                OrderType::Limit,
                OrderType::Stop,
                OrderType::StopLimit,
                OrderType::LimitIfTouched, // Take profit
                OrderType::TrailingStop,
            ])
            .with_tif(vec![TimeInForce::GTC, TimeInForce::IOC])
            .with_batch_support(10)
            .with_stop_orders();

        let normalizer = FuturesExecutionNormalizer::new(&info.name);

        Ok(Self {
            config,
            endpoints,
            info,
            http_client: None,
            signer: None,
            rest_client: None,
            normalizer,
            connected: Arc::new(AtomicBool::new(false)),
            stream_active: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Create the HTTP client and signer.
    fn create_clients(&self) -> VenueResult<(reqwest::Client, Arc<KrakenFuturesSigner>)> {
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

        let signer = Arc::new(KrakenFuturesSigner::new(api_key, &api_secret)?);

        let http_client = reqwest::Client::builder()
            .timeout(self.config.base.rest.timeout())
            .build()
            .map_err(|e| VenueError::Configuration(format!("Failed to create HTTP client: {}", e)))?;

        Ok((http_client, signer))
    }
}

#[async_trait]
impl VenueConnection for KrakenFuturesVenue {
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

        let rest_client = FuturesRestClient::new(http_client.clone(), signer.clone(), &base_url);

        // Test connection by fetching instruments
        let instruments = rest_client.instruments().await.map_err(|e| {
            VenueError::Connection(format!("Failed to connect to {}: {}", self.info.display_name, e))
        })?;

        info!(
            "Connected to {} ({} instruments available)",
            self.info.display_name,
            instruments.instruments.len()
        );

        // Verify credentials by querying account
        let _accounts = rest_client.accounts().await.map_err(|e| {
            VenueError::Authentication(format!("Failed to authenticate: {}", e))
        })?;

        self.http_client = Some(http_client);
        self.signer = Some(signer);
        self.rest_client = Some(rest_client);
        self.connected.store(true, Ordering::SeqCst);

        Ok(())
    }

    async fn disconnect(&mut self) -> VenueResult<()> {
        info!("Disconnecting from {}", self.info.display_name);

        self.connected.store(false, Ordering::SeqCst);
        self.rest_client = None;
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
impl OrderSubmissionVenue for KrakenFuturesVenue {
    async fn submit_order(&self, order: &Order) -> VenueResult<VenueOrderId> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client.send_order(order).await?;

        let venue_order_id = response
            .order_id
            .or_else(|| {
                response
                    .send_status
                    .as_ref()
                    .and_then(|s| s.order_id.clone())
            })
            .ok_or_else(|| VenueError::Parse("No order_id in response".to_string()))?;

        info!(
            "Order submitted: {} -> {}",
            order.client_order_id, venue_order_id
        );

        Ok(VenueOrderId::new(venue_order_id))
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
            .cancel_order(
                Some(client_order_id.as_str()),
                venue_order_id.map(|v| v.as_str()),
            )
            .await?;

        let status = response
            .cancel_status
            .and_then(|s| s.status)
            .unwrap_or_else(|| "unknown".to_string());

        if status.to_lowercase().contains("cancelled") || status.to_lowercase().contains("canceled")
        {
            info!("Order cancelled: {}", client_order_id);
            Ok(())
        } else {
            Err(VenueError::OrderNotFound(format!(
                "Order not found or already cancelled: {} (status: {})",
                client_order_id, status
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

        let response = rest_client
            .edit_order(
                Some(client_order_id.as_str()),
                venue_order_id.map(|v| v.as_str()),
                new_price,
                new_quantity,
            )
            .await?;

        let new_order_id = response
            .edit_status
            .and_then(|s| s.order_id)
            .ok_or_else(|| VenueError::Parse("No order_id in edit response".to_string()))?;

        info!("Order edited: {} -> {}", client_order_id, new_order_id);

        Ok(VenueOrderId::new(new_order_id))
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

        let open_orders = rest_client.open_orders().await?;

        // Find by client order ID first
        let order = open_orders
            .open_orders
            .iter()
            .find(|o| {
                o.cli_ord_id
                    .as_ref()
                    .map(|id| id == client_order_id.as_str())
                    .unwrap_or(false)
            })
            .or_else(|| {
                // Fall back to venue order ID
                venue_order_id.and_then(|vid| {
                    open_orders.open_orders.iter().find(|o| {
                        o.order_id
                            .as_ref()
                            .map(|id| id == vid.as_str())
                            .unwrap_or(false)
                    })
                })
            })
            .ok_or_else(|| {
                VenueError::OrderNotFound(format!("Order not found: {}", client_order_id))
            })?;

        Ok(self.normalizer.normalize_order_query(order))
    }

    async fn query_open_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<OrderQueryResponse>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client.open_orders().await?;

        let orders: Vec<OrderQueryResponse> = response
            .open_orders
            .iter()
            .filter(|o| {
                if let Some(sym) = symbol {
                    o.symbol
                        .as_ref()
                        .map(|s| super::types::from_futures_symbol(s) == sym)
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .map(|o| self.normalizer.normalize_order_query(o))
            .collect();

        Ok(orders)
    }

    async fn cancel_all_orders(&self, symbol: Option<&str>) -> VenueResult<Vec<ClientOrderId>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        // First get open orders to return their IDs
        let open_orders = rest_client.open_orders().await?;
        let client_order_ids: Vec<ClientOrderId> = open_orders
            .open_orders
            .iter()
            .filter(|o| {
                if let Some(sym) = symbol {
                    o.symbol
                        .as_ref()
                        .map(|s| super::types::from_futures_symbol(s) == sym)
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .filter_map(|o| o.cli_ord_id.as_ref().map(ClientOrderId::new))
            .collect();

        // Cancel all
        let response = rest_client.cancel_all(symbol).await?;

        let count = response
            .cancel_status
            .and_then(|s| s.cancel_count)
            .unwrap_or(0);

        info!("Cancelled {} orders", count);

        Ok(client_order_ids)
    }
}

#[async_trait]
impl ExecutionStreamVenue for KrakenFuturesVenue {
    async fn start_execution_stream(
        &mut self,
        _callback: ExecutionCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> VenueResult<()> {
        // Note: Kraken Futures uses a different WebSocket protocol than Spot
        // This is a simplified placeholder - full implementation would need
        // a dedicated WebSocket client for Futures V1 protocol

        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        // Get WebSocket challenge for authentication
        let challenge = rest_client.ws_challenge().await?;
        let _signed = rest_client.sign_challenge(&challenge.challenge);

        warn!(
            "Futures WebSocket stream not fully implemented. Challenge obtained: {}",
            &challenge.challenge[..8]
        );

        self.stream_active.store(true, Ordering::SeqCst);

        // Wait for shutdown
        let _ = shutdown_rx.recv().await;

        self.stream_active.store(false, Ordering::SeqCst);

        Ok(())
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
impl AccountQueryVenue for KrakenFuturesVenue {
    async fn query_balances(&self) -> VenueResult<Vec<BalanceInfo>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let accounts = rest_client.accounts().await?;

        let balances: Vec<BalanceInfo> = accounts
            .accounts
            .iter()
            .filter_map(|(account_type, info)| {
                let currency = info.currency.clone().unwrap_or_else(|| account_type.clone());
                let balance = info.balance.unwrap_or_default();
                let available = info.available.unwrap_or_default();

                if balance > Decimal::ZERO || available > Decimal::ZERO {
                    Some(BalanceInfo::new(currency, available, balance - available))
                } else {
                    None
                }
            })
            .collect();

        Ok(balances)
    }

    async fn query_balance(&self, asset: &str) -> VenueResult<BalanceInfo> {
        let balances = self.query_balances().await?;

        let search_asset = asset.to_uppercase();

        balances
            .into_iter()
            .find(|b| b.asset.to_uppercase() == search_asset)
            .ok_or_else(|| VenueError::SymbolNotFound(format!("Asset not found: {}", asset)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::VenueConnection;

    #[test]
    fn test_venue_creation() {
        let config = KrakenFuturesVenueConfig::production();
        let venue = KrakenFuturesVenue::new(config).unwrap();

        assert!(venue.info.venue_id.contains("kraken"));
        assert!(venue.info.venue_id.contains("futures"));
        assert!(venue.info.supports_order_type(&OrderType::Market));
        assert!(venue.info.supports_order_type(&OrderType::Limit));
    }

    #[test]
    fn test_venue_demo() {
        let config = KrakenFuturesVenueConfig::demo();
        let venue = KrakenFuturesVenue::new(config).unwrap();

        assert!(venue.info.venue_id.contains("demo"));
        assert!(venue.info.display_name.contains("Demo"));
        assert!(venue.endpoints.rest_url.contains("demo"));
    }

    #[test]
    fn test_initial_state() {
        let config = KrakenFuturesVenueConfig::production();
        let venue = KrakenFuturesVenue::new(config).unwrap();

        assert!(!venue.is_connected());
        assert!(!venue.is_stream_active());
        assert_eq!(venue.connection_status(), ConnectionStatus::Disconnected);
    }

    #[test]
    fn test_venue_info_capabilities() {
        let config = KrakenFuturesVenueConfig::production();
        let venue = KrakenFuturesVenue::new(config).unwrap();
        let info = venue.info();

        // Check supported order types
        assert!(info.supported_order_types.contains(&OrderType::Market));
        assert!(info.supported_order_types.contains(&OrderType::Limit));
        assert!(info.supported_order_types.contains(&OrderType::Stop));

        // Check supported time-in-force
        assert!(info.supported_tif.contains(&TimeInForce::GTC));
        assert!(info.supported_tif.contains(&TimeInForce::IOC));

        // Check capabilities
        assert!(info.supports_stop_orders);
        assert!(info.max_orders_per_batch > 0);
    }

    #[test]
    fn test_venue_info_name() {
        let config = KrakenFuturesVenueConfig::demo();
        let venue = KrakenFuturesVenue::new(config).unwrap();

        assert_eq!(venue.info().name, "KRAKEN_FUTURES");
        assert!(venue.info().venue_id.contains("kraken"));
    }
}
