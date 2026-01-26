//! Kraken API endpoints.
//!
//! This module centralizes all endpoint URLs for:
//! - Production and Sandbox environments
//! - REST and WebSocket APIs

/// Endpoint configuration for Kraken APIs.
#[derive(Debug, Clone)]
pub struct KrakenEndpoints {
    /// REST API base URL
    pub rest_url: String,
    /// Public WebSocket URL (V2)
    pub ws_public_url: String,
    /// Authenticated WebSocket URL (V2)
    pub ws_auth_url: String,
}

impl KrakenEndpoints {
    /// Get endpoints for the specified sandbox setting.
    pub fn for_config(sandbox: bool) -> Self {
        if sandbox {
            Self::sandbox()
        } else {
            Self::production()
        }
    }

    /// Kraken production endpoints.
    pub fn production() -> Self {
        Self {
            rest_url: "https://api.kraken.com".to_string(),
            ws_public_url: "wss://ws.kraken.com/v2".to_string(),
            ws_auth_url: "wss://ws-auth.kraken.com/v2".to_string(),
        }
    }

    /// Kraken sandbox/demo endpoints.
    ///
    /// **IMPORTANT**: Kraken Spot does NOT have a public sandbox like Binance.
    ///
    /// This "sandbox" mode uses **production endpoints** but is intended for:
    /// - Testing with minimal order sizes
    /// - Using limit orders far from market price
    /// - Verifying API connectivity and authentication
    ///
    /// For true sandbox testing:
    /// - Contact Kraken support for Spot UAT access
    /// - Use Kraken Futures Demo for testing auth patterns (requires separate Futures implementation)
    ///
    /// **WARNING**: Orders placed in sandbox mode will execute on REAL production markets!
    /// Always use small quantities and prices far from market to avoid accidental fills.
    pub fn sandbox() -> Self {
        // Use production endpoints - Kraken has no public spot testnet
        // The "sandbox" flag is used to indicate testing mode with small orders
        Self::production()
    }

    /// Get the WebSocket URL with token for authenticated streams.
    pub fn user_data_stream_url(&self, _token: &str) -> String {
        // For V2 WebSocket, the token is sent in the subscription message,
        // not in the URL. So we just return the auth URL.
        self.ws_auth_url.clone()
    }
}

/// REST API endpoint paths for Kraken Spot.
pub mod spot {
    // ==================== PUBLIC ENDPOINTS ====================

    /// Server time
    pub const SERVER_TIME: &str = "/0/public/Time";

    /// System status
    pub const SYSTEM_STATUS: &str = "/0/public/SystemStatus";

    /// Asset info
    pub const ASSETS: &str = "/0/public/Assets";

    /// Asset pairs
    pub const ASSET_PAIRS: &str = "/0/public/AssetPairs";

    /// Ticker info
    pub const TICKER: &str = "/0/public/Ticker";

    // ==================== PRIVATE ENDPOINTS ====================
    // Note: All private endpoints use POST method

    /// Account balance
    pub const BALANCE: &str = "/0/private/Balance";

    /// Extended balance (includes hold amounts)
    pub const BALANCE_EX: &str = "/0/private/BalanceEx";

    /// Trade balance (margin info)
    pub const TRADE_BALANCE: &str = "/0/private/TradeBalance";

    /// Open orders
    pub const OPEN_ORDERS: &str = "/0/private/OpenOrders";

    /// Closed orders
    pub const CLOSED_ORDERS: &str = "/0/private/ClosedOrders";

    /// Query orders
    pub const QUERY_ORDERS: &str = "/0/private/QueryOrders";

    /// Trades history
    pub const TRADES_HISTORY: &str = "/0/private/TradesHistory";

    /// Query trades
    pub const QUERY_TRADES: &str = "/0/private/QueryTrades";

    /// Open positions
    pub const OPEN_POSITIONS: &str = "/0/private/OpenPositions";

    /// Ledgers
    pub const LEDGERS: &str = "/0/private/Ledgers";

    /// Query ledgers
    pub const QUERY_LEDGERS: &str = "/0/private/QueryLedgers";

    /// Trade volume
    pub const TRADE_VOLUME: &str = "/0/private/TradeVolume";

    // ==================== ORDER ENDPOINTS ====================

    /// Add order
    pub const ADD_ORDER: &str = "/0/private/AddOrder";

    /// Add order batch (2-15 orders)
    pub const ADD_ORDER_BATCH: &str = "/0/private/AddOrderBatch";

    /// Amend order (true order modification!)
    pub const AMEND_ORDER: &str = "/0/private/AmendOrder";

    /// Edit order (deprecated, use AmendOrder)
    pub const EDIT_ORDER: &str = "/0/private/EditOrder";

    /// Cancel order
    pub const CANCEL_ORDER: &str = "/0/private/CancelOrder";

    /// Cancel all orders
    pub const CANCEL_ALL: &str = "/0/private/CancelAll";

    /// Cancel all orders after timeout (deadman's switch)
    pub const CANCEL_ALL_AFTER: &str = "/0/private/CancelAllOrdersAfter";

    /// Cancel order batch (up to 50 orders)
    pub const CANCEL_ORDER_BATCH: &str = "/0/private/CancelOrderBatch";

    // ==================== WEBSOCKET TOKEN ====================

    /// Get WebSocket authentication token
    pub const WS_TOKEN: &str = "/0/private/GetWebSocketsToken";
}

/// Endpoint configuration for Kraken Futures APIs.
#[derive(Debug, Clone)]
pub struct KrakenFuturesEndpoints {
    /// REST API base URL
    pub rest_url: String,
    /// WebSocket URL
    pub ws_url: String,
}

impl KrakenFuturesEndpoints {
    /// Get endpoints for the specified demo setting.
    pub fn for_config(demo: bool) -> Self {
        if demo {
            Self::demo()
        } else {
            Self::production()
        }
    }

    /// Kraken Futures production endpoints.
    pub fn production() -> Self {
        Self {
            rest_url: "https://futures.kraken.com".to_string(),
            ws_url: "wss://futures.kraken.com/ws/v1".to_string(),
        }
    }

    /// Kraken Futures demo/testnet endpoints.
    ///
    /// Unlike Spot, Kraken Futures HAS a public demo environment!
    pub fn demo() -> Self {
        Self {
            rest_url: "https://demo-futures.kraken.com".to_string(),
            ws_url: "wss://demo-futures.kraken.com/ws/v1".to_string(),
        }
    }
}

/// REST API endpoint paths for Kraken Futures.
pub mod futures {
    // ==================== PUBLIC ENDPOINTS ====================

    /// Instruments (tradeable pairs)
    pub const INSTRUMENTS: &str = "/derivatives/api/v3/instruments";

    /// Tickers
    pub const TICKERS: &str = "/derivatives/api/v3/tickers";

    /// Order book
    pub const ORDERBOOK: &str = "/derivatives/api/v3/orderbook";

    /// Trade history
    pub const HISTORY: &str = "/derivatives/api/v3/history";

    // ==================== PRIVATE ENDPOINTS ====================

    /// Account information
    pub const ACCOUNTS: &str = "/derivatives/api/v3/accounts";

    /// Open orders
    pub const OPEN_ORDERS: &str = "/derivatives/api/v3/openorders";

    /// Open positions
    pub const OPEN_POSITIONS: &str = "/derivatives/api/v3/openpositions";

    /// Fills (trade history)
    pub const FILLS: &str = "/derivatives/api/v3/fills";

    /// Transfer between accounts
    pub const TRANSFER: &str = "/derivatives/api/v3/transfer";

    // ==================== ORDER ENDPOINTS ====================

    /// Send order
    pub const SEND_ORDER: &str = "/derivatives/api/v3/sendorder";

    /// Edit order
    pub const EDIT_ORDER: &str = "/derivatives/api/v3/editorder";

    /// Cancel order
    pub const CANCEL_ORDER: &str = "/derivatives/api/v3/cancelorder";

    /// Cancel all orders
    pub const CANCEL_ALL: &str = "/derivatives/api/v3/cancelallorders";

    /// Cancel all orders after (dead man's switch)
    pub const CANCEL_ALL_AFTER: &str = "/derivatives/api/v3/cancelallordersafter";

    /// Batch order (send multiple orders)
    pub const BATCH_ORDER: &str = "/derivatives/api/v3/batchorder";

    // ==================== LEVERAGE ====================

    /// Get leverage settings
    pub const LEVERAGE: &str = "/derivatives/api/v3/leveragepreferences";

    /// Set leverage (POST to same endpoint)
    pub const SET_LEVERAGE: &str = "/derivatives/api/v3/leveragepreferences";

    // ==================== WEBSOCKET ====================

    /// Get WebSocket challenge for authentication
    pub const WS_CHALLENGE: &str = "/derivatives/api/v3/ws/challenge";
}

/// API weights for rate limiting.
///
/// Kraken uses a counter-based system per pair rather than weights.
/// These are approximate costs for the rate limiter.
pub mod weights {
    /// Add order cost
    pub const ADD_ORDER: u32 = 1;

    /// Amend order cost (varies: +1 to +3 based on timing)
    pub const AMEND_ORDER: u32 = 2;

    /// Cancel order cost (varies: 0 to +8 based on timing)
    /// Rapid cancellation incurs higher costs
    pub const CANCEL_ORDER: u32 = 1;

    /// Cancel all orders cost
    pub const CANCEL_ALL: u32 = 1;

    /// Balance query
    pub const BALANCE: u32 = 1;

    /// Order query
    pub const QUERY_ORDER: u32 = 1;

    /// Open orders query
    pub const OPEN_ORDERS: u32 = 1;

    /// WebSocket token
    pub const WS_TOKEN: u32 = 1;

    /// Server time (public)
    pub const SERVER_TIME: u32 = 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_production_endpoints() {
        let endpoints = KrakenEndpoints::production();
        assert_eq!(endpoints.rest_url, "https://api.kraken.com");
        assert!(endpoints.ws_public_url.contains("ws.kraken.com"));
        assert!(endpoints.ws_auth_url.contains("ws-auth.kraken.com"));
    }

    #[test]
    fn test_sandbox_endpoints() {
        // Kraken sandbox uses production endpoints (no public spot testnet)
        let endpoints = KrakenEndpoints::sandbox();
        assert_eq!(endpoints.rest_url, "https://api.kraken.com");
    }

    #[test]
    fn test_for_config() {
        let prod = KrakenEndpoints::for_config(false);
        assert_eq!(prod.rest_url, "https://api.kraken.com");

        // Sandbox uses production endpoints (Kraken has no public spot testnet)
        let sandbox = KrakenEndpoints::for_config(true);
        assert_eq!(sandbox.rest_url, "https://api.kraken.com");
    }

    #[test]
    fn test_endpoint_paths() {
        assert!(spot::ADD_ORDER.starts_with("/0/private/"));
        assert!(spot::BALANCE.starts_with("/0/private/"));
        assert!(spot::SERVER_TIME.starts_with("/0/public/"));
    }
}
