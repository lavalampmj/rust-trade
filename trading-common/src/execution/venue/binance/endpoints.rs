//! Binance API endpoints for different platforms and market types.
//!
//! This module centralizes all endpoint URLs for:
//! - Binance.com and Binance.US
//! - Spot and Futures markets
//! - Production and Testnet environments

use super::config::{BinanceMarketType, BinancePlatform};

/// Endpoint configuration for Binance APIs.
#[derive(Debug, Clone)]
pub struct BinanceEndpoints {
    /// REST API base URL
    pub rest_url: String,
    /// WebSocket stream URL
    pub ws_url: String,
    /// User data stream WebSocket URL (may differ from public streams)
    pub user_data_ws_url: String,
}

impl BinanceEndpoints {
    /// Get endpoints for the specified platform, market type, and testnet setting.
    pub fn for_config(
        platform: BinancePlatform,
        market_type: BinanceMarketType,
        testnet: bool,
    ) -> Self {
        match (platform, market_type, testnet) {
            // Binance.com Spot
            (BinancePlatform::Com, BinanceMarketType::Spot, false) => Self::spot_com(),
            (BinancePlatform::Com, BinanceMarketType::Spot, true) => Self::spot_com_testnet(),

            // Binance.US Spot
            (BinancePlatform::Us, BinanceMarketType::Spot, false) => Self::spot_us(),
            (BinancePlatform::Us, BinanceMarketType::Spot, true) => Self::spot_us_testnet(),

            // Binance USDT-M Futures (only .com)
            (_, BinanceMarketType::UsdtFutures, false) => Self::usdt_futures(),
            (_, BinanceMarketType::UsdtFutures, true) => Self::usdt_futures_testnet(),

            // Binance COIN-M Futures (only .com)
            (_, BinanceMarketType::CoinFutures, false) => Self::coin_futures(),
            (_, BinanceMarketType::CoinFutures, true) => Self::coin_futures_testnet(),
        }
    }

    // ==================== SPOT ENDPOINTS ====================

    /// Binance.com Spot production endpoints.
    pub fn spot_com() -> Self {
        Self {
            rest_url: "https://api.binance.com".to_string(),
            ws_url: "wss://stream.binance.com:9443/ws".to_string(),
            user_data_ws_url: "wss://stream.binance.com:9443/ws".to_string(),
        }
    }

    /// Binance.com Spot testnet endpoints.
    pub fn spot_com_testnet() -> Self {
        Self {
            rest_url: "https://testnet.binance.vision".to_string(),
            ws_url: "wss://testnet.binance.vision/ws".to_string(),
            user_data_ws_url: "wss://testnet.binance.vision/ws".to_string(),
        }
    }

    /// Binance.US Spot production endpoints.
    pub fn spot_us() -> Self {
        Self {
            rest_url: "https://api.binance.us".to_string(),
            ws_url: "wss://stream.binance.us:9443/ws".to_string(),
            user_data_ws_url: "wss://stream.binance.us:9443/ws".to_string(),
        }
    }

    /// Binance.US Spot testnet endpoints (uses same as .com testnet).
    pub fn spot_us_testnet() -> Self {
        // Binance.US doesn't have its own testnet, use .com testnet
        Self::spot_com_testnet()
    }

    // ==================== USDT-M FUTURES ENDPOINTS ====================

    /// Binance USDT-M Futures production endpoints.
    pub fn usdt_futures() -> Self {
        Self {
            rest_url: "https://fapi.binance.com".to_string(),
            ws_url: "wss://fstream.binance.com/ws".to_string(),
            user_data_ws_url: "wss://fstream.binance.com/ws".to_string(),
        }
    }

    /// Binance USDT-M Futures testnet endpoints.
    pub fn usdt_futures_testnet() -> Self {
        Self {
            rest_url: "https://testnet.binancefuture.com".to_string(),
            ws_url: "wss://stream.binancefuture.com/ws".to_string(),
            user_data_ws_url: "wss://stream.binancefuture.com/ws".to_string(),
        }
    }

    // ==================== COIN-M FUTURES ENDPOINTS ====================

    /// Binance COIN-M Futures production endpoints.
    pub fn coin_futures() -> Self {
        Self {
            rest_url: "https://dapi.binance.com".to_string(),
            ws_url: "wss://dstream.binance.com/ws".to_string(),
            user_data_ws_url: "wss://dstream.binance.com/ws".to_string(),
        }
    }

    /// Binance COIN-M Futures testnet endpoints.
    pub fn coin_futures_testnet() -> Self {
        Self {
            rest_url: "https://testnet.binancefuture.com".to_string(),
            ws_url: "wss://dstream.binancefuture.com/ws".to_string(),
            user_data_ws_url: "wss://dstream.binancefuture.com/ws".to_string(),
        }
    }

    /// Get the user data stream URL with listen key.
    pub fn user_data_stream_url(&self, listen_key: &str) -> String {
        format!("{}/{}", self.user_data_ws_url, listen_key)
    }
}

/// REST API endpoint paths for Binance Spot.
pub mod spot {
    /// Create user data stream (listen key)
    pub const USER_DATA_STREAM: &str = "/api/v3/userDataStream";

    /// Account information
    pub const ACCOUNT: &str = "/api/v3/account";

    /// New order
    pub const ORDER: &str = "/api/v3/order";

    /// Query order
    pub const ORDER_QUERY: &str = "/api/v3/order";

    /// Cancel order
    pub const ORDER_CANCEL: &str = "/api/v3/order";

    /// Cancel all open orders
    pub const ORDER_CANCEL_ALL: &str = "/api/v3/openOrders";

    /// Open orders
    pub const OPEN_ORDERS: &str = "/api/v3/openOrders";

    /// All orders (history)
    pub const ALL_ORDERS: &str = "/api/v3/allOrders";

    /// Exchange information
    pub const EXCHANGE_INFO: &str = "/api/v3/exchangeInfo";

    /// Server time
    pub const SERVER_TIME: &str = "/api/v3/time";

    /// Test connectivity
    pub const PING: &str = "/api/v3/ping";
}

/// REST API endpoint paths for Binance USDT-M Futures.
pub mod futures {
    /// Create user data stream (listen key)
    pub const USER_DATA_STREAM: &str = "/fapi/v1/listenKey";

    /// Account information (v2)
    pub const ACCOUNT: &str = "/fapi/v2/account";

    /// Position risk (v2)
    pub const POSITION_RISK: &str = "/fapi/v2/positionRisk";

    /// New order
    pub const ORDER: &str = "/fapi/v1/order";

    /// Query order
    pub const ORDER_QUERY: &str = "/fapi/v1/order";

    /// Cancel order
    pub const ORDER_CANCEL: &str = "/fapi/v1/order";

    /// Cancel all open orders
    pub const ORDER_CANCEL_ALL: &str = "/fapi/v1/allOpenOrders";

    /// Open orders
    pub const OPEN_ORDERS: &str = "/fapi/v1/openOrders";

    /// All orders (history)
    pub const ALL_ORDERS: &str = "/fapi/v1/allOrders";

    /// Set leverage
    pub const LEVERAGE: &str = "/fapi/v1/leverage";

    /// Set margin type
    pub const MARGIN_TYPE: &str = "/fapi/v1/marginType";

    /// Set position mode (hedge/one-way)
    pub const POSITION_MODE: &str = "/fapi/v1/positionSide/dual";

    /// Get position mode
    pub const POSITION_MODE_GET: &str = "/fapi/v1/positionSide/dual";

    /// Exchange information
    pub const EXCHANGE_INFO: &str = "/fapi/v1/exchangeInfo";

    /// Server time
    pub const SERVER_TIME: &str = "/fapi/v1/time";

    /// Test connectivity
    pub const PING: &str = "/fapi/v1/ping";
}

/// API weights for rate limiting.
pub mod weights {
    /// Most query endpoints
    pub const QUERY_DEFAULT: u32 = 1;

    /// Account information
    pub const ACCOUNT_INFO: u32 = 5;

    /// Exchange info
    pub const EXCHANGE_INFO: u32 = 10;

    /// Order submission
    pub const ORDER_SUBMIT: u32 = 1;

    /// Order cancel
    pub const ORDER_CANCEL: u32 = 1;

    /// Cancel all orders
    pub const ORDER_CANCEL_ALL: u32 = 1;

    /// Open orders query
    pub const OPEN_ORDERS: u32 = 3;

    /// All orders query
    pub const ALL_ORDERS: u32 = 5;

    /// Position risk query
    pub const POSITION_RISK: u32 = 5;

    /// Listen key operations
    pub const LISTEN_KEY: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spot_com_endpoints() {
        let endpoints = BinanceEndpoints::spot_com();
        assert_eq!(endpoints.rest_url, "https://api.binance.com");
        assert!(endpoints.ws_url.contains("stream.binance.com"));
    }

    #[test]
    fn test_spot_us_endpoints() {
        let endpoints = BinanceEndpoints::spot_us();
        assert_eq!(endpoints.rest_url, "https://api.binance.us");
        assert!(endpoints.ws_url.contains("stream.binance.us"));
    }

    #[test]
    fn test_usdt_futures_endpoints() {
        let endpoints = BinanceEndpoints::usdt_futures();
        assert_eq!(endpoints.rest_url, "https://fapi.binance.com");
        assert!(endpoints.ws_url.contains("fstream.binance.com"));
    }

    #[test]
    fn test_for_config() {
        let spot = BinanceEndpoints::for_config(
            BinancePlatform::Com,
            BinanceMarketType::Spot,
            false,
        );
        assert_eq!(spot.rest_url, "https://api.binance.com");

        let futures = BinanceEndpoints::for_config(
            BinancePlatform::Com,
            BinanceMarketType::UsdtFutures,
            false,
        );
        assert_eq!(futures.rest_url, "https://fapi.binance.com");

        let testnet = BinanceEndpoints::for_config(
            BinancePlatform::Com,
            BinanceMarketType::Spot,
            true,
        );
        assert!(testnet.rest_url.contains("testnet"));
    }

    #[test]
    fn test_user_data_stream_url() {
        let endpoints = BinanceEndpoints::spot_com();
        let url = endpoints.user_data_stream_url("abc123");
        assert_eq!(url, "wss://stream.binance.com:9443/ws/abc123");
    }
}
