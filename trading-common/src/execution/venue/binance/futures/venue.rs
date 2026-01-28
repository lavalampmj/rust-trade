//! Binance USDT-M Futures execution venue implementation.
//!
//! This module provides the `BinanceFuturesVenue` struct that implements
//! all execution venue traits plus futures-specific operations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::execution::venue::binance::common::{
    BinanceHmacSigner, BinanceMarginType, BinanceUserDataStream, RawMessageCallback,
};
use crate::execution::venue::binance::config::BinanceVenueConfig;
use crate::execution::venue::binance::endpoints::{futures, BinanceEndpoints};
use crate::execution::venue::error::{VenueError, VenueResult};
use crate::execution::venue::http::{HttpClient, RateLimiter};
use crate::execution::venue::traits::{
    AccountQueryVenue, ExecutionCallback, ExecutionStreamVenue, OrderSubmissionVenue,
};
use crate::execution::venue::types::{BalanceInfo, OrderQueryResponse, VenueInfo};
use crate::venue::{ConnectionStatus, VenueConnection};
use crate::orders::{ClientOrderId, Order, OrderType, PositionSide, TimeInForce, VenueOrderId};

use super::normalizer::FuturesExecutionNormalizer;
use super::rest_client::FuturesRestClient;
use super::types::FuturesPositionRisk;

/// Margin type for futures positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginType {
    /// Cross margin
    Cross,
    /// Isolated margin
    Isolated,
}

impl From<MarginType> for BinanceMarginType {
    fn from(mt: MarginType) -> Self {
        match mt {
            MarginType::Cross => BinanceMarginType::Cross,
            MarginType::Isolated => BinanceMarginType::Isolated,
        }
    }
}

/// Position information for futures.
#[derive(Debug, Clone)]
pub struct FuturesPositionInfo {
    /// Symbol
    pub symbol: String,
    /// Position side (Long, Short, Both)
    pub position_side: PositionSide,
    /// Position quantity (can be negative for short)
    pub quantity: Decimal,
    /// Entry price
    pub entry_price: Decimal,
    /// Mark price
    pub mark_price: Decimal,
    /// Unrealized PnL
    pub unrealized_pnl: Decimal,
    /// Current leverage
    pub leverage: u32,
    /// Margin type
    pub margin_type: MarginType,
    /// Liquidation price
    pub liquidation_price: Option<Decimal>,
}

impl From<&FuturesPositionRisk> for FuturesPositionInfo {
    fn from(risk: &FuturesPositionRisk) -> Self {
        let margin_type = match risk.margin_type {
            BinanceMarginType::Cross => MarginType::Cross,
            BinanceMarginType::Isolated => MarginType::Isolated,
        };

        let liquidation_price = if risk.liquidation_price > Decimal::ZERO {
            Some(risk.liquidation_price)
        } else {
            None
        };

        Self {
            symbol: risk.symbol.clone(),
            position_side: risk.position_side.to_position_side(),
            quantity: risk.position_amt,
            entry_price: risk.entry_price,
            mark_price: risk.mark_price,
            unrealized_pnl: risk.unrealized_profit,
            leverage: risk.leverage.to_string().parse().unwrap_or(1),
            margin_type,
            liquidation_price,
        }
    }
}

/// Binance USDT-M Futures execution venue.
///
/// Implements all venue traits plus futures-specific operations:
/// - `ExecutionVenue`: Connection management
/// - `OrderSubmissionVenue`: Order submission, cancellation, queries
/// - `ExecutionStreamVenue`: Real-time execution reports via WebSocket
/// - `AccountQueryVenue`: Balance queries
/// - Futures-specific: Leverage, margin type, position management
pub struct BinanceFuturesVenue {
    /// Venue configuration
    config: BinanceVenueConfig,
    /// Endpoints
    endpoints: BinanceEndpoints,
    /// Venue info
    info: VenueInfo,
    /// HTTP client
    http_client: Option<Arc<HttpClient>>,
    /// REST client
    rest_client: Option<FuturesRestClient>,
    /// Execution normalizer
    normalizer: FuturesExecutionNormalizer,
    /// Connection status
    connected: Arc<AtomicBool>,
    /// Stream status
    stream_active: Arc<AtomicBool>,
}

impl BinanceFuturesVenue {
    /// Create a new Binance Futures venue.
    pub fn new(config: BinanceVenueConfig) -> VenueResult<Self> {
        let endpoints = BinanceEndpoints::for_config(
            config.platform,
            config.market_type,
            config.testnet,
        );

        let venue_id = format!(
            "binance_futures{}",
            if config.testnet { "_testnet" } else { "" }
        );

        let display_name = format!(
            "Binance USDT-M Futures{}",
            if config.testnet { " (Testnet)" } else { "" }
        );

        let info = VenueInfo::new(&venue_id, &display_name)
            .with_name("BINANCE_FUTURES")
            .with_order_types(vec![
                OrderType::Market,
                OrderType::Limit,
                OrderType::Stop,
                OrderType::StopLimit,
                OrderType::TrailingStop,
                OrderType::LimitIfTouched, // Maps to TakeProfit
            ])
            .with_tif(vec![TimeInForce::GTC, TimeInForce::IOC, TimeInForce::FOK])
            .with_batch_support(5)
            .with_stop_orders()
            .with_trailing_stop();

        let normalizer = FuturesExecutionNormalizer::new(&info.name);

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

        let base_url = if self.config.base.rest.base_url.is_empty() {
            self.endpoints.rest_url.clone()
        } else {
            self.config.base.rest.base_url.clone()
        };

        HttpClient::new(base_url, Box::new(signer), rate_limiter, self.config.base.rest.clone())
    }

    // ==================== Futures-specific methods ====================

    /// Set leverage for a symbol.
    pub async fn set_leverage(&self, symbol: &str, leverage: u32) -> VenueResult<()> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let response = rest_client.set_leverage(symbol, leverage).await?;
        info!(
            "Set leverage for {}: {}x (max notional: {})",
            symbol, response.leverage, response.max_notional_value
        );
        Ok(())
    }

    /// Get current leverage for a symbol.
    pub async fn get_leverage(&self, symbol: &str) -> VenueResult<u32> {
        let position = self.query_position(symbol).await?;
        Ok(position.leverage)
    }

    /// Set margin type for a symbol.
    pub async fn set_margin_type(&self, symbol: &str, margin_type: MarginType) -> VenueResult<()> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        rest_client.set_margin_type(symbol, margin_type.into()).await?;
        info!("Set margin type for {}: {:?}", symbol, margin_type);
        Ok(())
    }

    /// Query all open positions.
    pub async fn query_positions(&self) -> VenueResult<Vec<FuturesPositionInfo>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let positions = rest_client.query_positions().await?;

        // Filter out positions with zero quantity
        Ok(positions
            .iter()
            .filter(|p| p.position_amt != Decimal::ZERO)
            .map(FuturesPositionInfo::from)
            .collect())
    }

    /// Query position for a specific symbol.
    pub async fn query_position(&self, symbol: &str) -> VenueResult<FuturesPositionInfo> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let position = rest_client.query_position(symbol).await?;
        Ok(FuturesPositionInfo::from(&position))
    }

    /// Set position mode (hedge or one-way).
    ///
    /// Note: This is account-wide and cannot be changed if there are open positions.
    pub async fn set_position_mode(&self, hedge_mode: bool) -> VenueResult<()> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        rest_client.set_position_mode(hedge_mode).await?;
        info!(
            "Set position mode: {}",
            if hedge_mode { "hedge" } else { "one-way" }
        );
        Ok(())
    }

    /// Get current position mode.
    pub async fn is_hedge_mode(&self) -> VenueResult<bool> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        rest_client.get_position_mode().await
    }
}

#[async_trait]
impl VenueConnection for BinanceFuturesVenue {
    fn info(&self) -> &VenueInfo {
        &self.info
    }

    async fn connect(&mut self) -> VenueResult<()> {
        info!("Connecting to {}", self.info.display_name);

        let http_client = Arc::new(self.create_http_client()?);
        let rest_client = FuturesRestClient::new(http_client.clone());

        // Test connection
        rest_client.ping().await.map_err(|e| {
            VenueError::Connection(format!("Failed to ping {}: {}", self.info.display_name, e))
        })?;

        // Verify credentials
        let account = rest_client.query_account().await.map_err(|e| {
            VenueError::Authentication(format!("Failed to authenticate: {}", e))
        })?;

        if !account.can_trade {
            return Err(VenueError::Authentication(
                "Account does not have trading permission".to_string(),
            ));
        }

        info!(
            "Connected to {} (wallet balance: {} USDT)",
            self.info.display_name, account.total_wallet_balance
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
impl OrderSubmissionVenue for BinanceFuturesVenue {
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
        // Binance Futures doesn't support order modification directly
        Err(VenueError::InvalidOrder(
            "Order modification not supported for Binance Futures".to_string(),
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

        let _ = rest_client.cancel_all_orders(symbol).await?;

        // Binance doesn't return individual order IDs for cancel all
        info!("Cancelled all orders for {}", symbol);
        Ok(vec![])
    }
}

#[async_trait]
impl ExecutionStreamVenue for BinanceFuturesVenue {
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

        let ws_url = if self.config.base.stream.ws_url.is_empty() {
            self.endpoints.user_data_ws_url.clone()
        } else {
            self.config.base.stream.ws_url.clone()
        };

        let stream = BinanceUserDataStream::new(
            http_client,
            self.config.base.stream.clone(),
            ws_url,
            futures::USER_DATA_STREAM,
        );

        let normalizer = FuturesExecutionNormalizer::new(&self.info.name);
        let stream_active = self.stream_active.clone();

        let raw_callback: RawMessageCallback = Arc::new(move |msg: String| {
            match normalizer.parse_ws_message(&msg) {
                Ok(Some(report)) => {
                    if let Some(event) = normalizer.to_order_event(&report) {
                        callback(event);
                    }
                }
                Ok(None) => {}
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
impl AccountQueryVenue for BinanceFuturesVenue {
    async fn query_balances(&self) -> VenueResult<Vec<BalanceInfo>> {
        let rest_client = self
            .rest_client
            .as_ref()
            .ok_or(VenueError::NotConnected)?;

        let account = rest_client.query_account().await?;

        Ok(account
            .assets
            .into_iter()
            .filter(|a| a.wallet_balance > Decimal::ZERO || a.available_balance > Decimal::ZERO)
            .map(|a| {
                let locked = a.wallet_balance - a.available_balance;
                BalanceInfo::new(a.asset, a.available_balance, locked.max(Decimal::ZERO))
            })
            .collect())
    }

    async fn query_balance(&self, asset: &str) -> VenueResult<BalanceInfo> {
        let balances = self.query_balances().await?;

        balances
            .into_iter()
            .find(|b| b.asset.eq_ignore_ascii_case(asset))
            .ok_or_else(|| VenueError::SymbolNotFound(format!("Asset not found: {}", asset)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::VenueConnection;

    #[test]
    fn test_venue_creation() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert!(venue.info.venue_id.contains("futures"));
        assert!(venue.info.supports_order_type(&OrderType::Market));
        assert!(venue.info.supports_order_type(&OrderType::TrailingStop));
    }

    #[test]
    fn test_venue_testnet() {
        let config = BinanceVenueConfig::usdt_futures().with_testnet(true);
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert!(venue.info.display_name.contains("Testnet"));
        assert!(venue.endpoints.rest_url.contains("testnet"));
    }

    #[test]
    fn test_initial_state() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert!(!venue.is_connected());
        assert!(!venue.is_stream_active());
    }

    #[test]
    fn test_venue_info_capabilities() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = BinanceFuturesVenue::new(config).unwrap();
        let info = venue.info();

        // Check supported order types
        assert!(info.supported_order_types.contains(&OrderType::Market));
        assert!(info.supported_order_types.contains(&OrderType::Limit));
        assert!(info.supported_order_types.contains(&OrderType::StopLimit));
        assert!(info.supported_order_types.contains(&OrderType::TrailingStop));

        // Check supported time-in-force
        assert!(info.supported_tif.contains(&TimeInForce::GTC));
        assert!(info.supported_tif.contains(&TimeInForce::IOC));
        assert!(info.supported_tif.contains(&TimeInForce::FOK));

        // Check futures-specific capabilities
        assert!(info.supports_stop_orders);
        assert!(info.supports_trailing_stop);
        assert!(info.max_orders_per_batch > 0);
    }

    #[test]
    fn test_venue_name() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert_eq!(venue.info().name, "BINANCE_FUTURES");
        assert!(venue.info().display_name.contains("Futures"));
    }

    #[test]
    fn test_futures_endpoints() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert!(venue.endpoints.rest_url.contains("fapi.binance.com"));
        assert!(venue.endpoints.ws_url.contains("fstream.binance.com"));
    }

    #[test]
    fn test_futures_testnet_endpoints() {
        let config = BinanceVenueConfig::usdt_futures().with_testnet(true);
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert!(venue.endpoints.rest_url.contains("testnet.binancefuture.com"));
    }

    #[test]
    fn test_connection_status() {
        let config = BinanceVenueConfig::usdt_futures();
        let venue = BinanceFuturesVenue::new(config).unwrap();

        assert_eq!(venue.connection_status(), ConnectionStatus::Disconnected);
        assert!(!venue.connected.load(std::sync::atomic::Ordering::SeqCst));
    }
}
