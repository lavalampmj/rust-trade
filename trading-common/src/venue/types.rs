//! Venue-agnostic types for venues.
//!
//! These types provide a common interface for all venues,
//! abstracting away venue-specific details.

use crate::orders::{OrderType, TimeInForce};

/// Capabilities that a venue may support.
#[derive(Debug, Clone, Default)]
pub struct VenueCapabilities {
    // Data capabilities
    /// Supports live streaming
    pub supports_live_streaming: bool,
    /// Supports historical data
    pub supports_historical: bool,
    /// Supports L1 quotes (BBO)
    pub supports_quotes: bool,
    /// Supports L2 order book
    pub supports_orderbook: bool,
    /// Supports L3 market-by-order
    pub supports_mbo: bool,

    // Execution capabilities
    /// Supports order submission
    pub supports_orders: bool,
    /// Supports order modification
    pub supports_modify: bool,
    /// Supports batch operations
    pub supports_batch: bool,
    /// Maximum orders per batch (if batch is supported)
    pub max_orders_per_batch: usize,
    /// Supports stop orders natively
    pub supports_stop_orders: bool,
    /// Supports trailing stop orders
    pub supports_trailing_stop: bool,
    /// Supports execution stream (WebSocket fills)
    pub supports_execution_stream: bool,
}

impl VenueCapabilities {
    /// Create capabilities for a data-only venue.
    pub fn data_only() -> Self {
        Self {
            supports_live_streaming: true,
            supports_historical: false,
            supports_quotes: true,
            supports_orderbook: false,
            supports_mbo: false,
            supports_orders: false,
            supports_modify: false,
            supports_batch: false,
            max_orders_per_batch: 0,
            supports_stop_orders: false,
            supports_trailing_stop: false,
            supports_execution_stream: false,
        }
    }

    /// Create capabilities for an execution-only venue.
    pub fn execution_only() -> Self {
        Self {
            supports_live_streaming: false,
            supports_historical: false,
            supports_quotes: false,
            supports_orderbook: false,
            supports_mbo: false,
            supports_orders: true,
            supports_modify: false,
            supports_batch: false,
            max_orders_per_batch: 0,
            supports_stop_orders: false,
            supports_trailing_stop: false,
            supports_execution_stream: true,
        }
    }

    /// Create full capabilities (data + execution).
    pub fn full() -> Self {
        Self {
            supports_live_streaming: true,
            supports_historical: true,
            supports_quotes: true,
            supports_orderbook: true,
            supports_mbo: false,
            supports_orders: true,
            supports_modify: true,
            supports_batch: true,
            max_orders_per_batch: 10,
            supports_stop_orders: true,
            supports_trailing_stop: false,
            supports_execution_stream: true,
        }
    }
}

/// Information about a venue's identity and capabilities.
#[derive(Debug, Clone)]
pub struct VenueInfo {
    /// Unique identifier for this venue (e.g., "binance_us_spot")
    pub venue_id: String,
    /// Human-readable name (e.g., "Binance.US Spot")
    pub display_name: String,
    /// Short name for logging/display (e.g., "BINANCE_US")
    pub name: String,
    /// Venue capabilities
    pub capabilities: VenueCapabilities,
    /// Supported order types (for execution venues)
    pub supported_order_types: Vec<OrderType>,
    /// Supported time-in-force options (for execution venues)
    pub supported_tif: Vec<TimeInForce>,
}

impl VenueInfo {
    /// Create a new VenueInfo with default values.
    pub fn new(venue_id: impl Into<String>, display_name: impl Into<String>) -> Self {
        Self {
            venue_id: venue_id.into(),
            display_name: display_name.into(),
            name: String::new(),
            capabilities: VenueCapabilities::default(),
            supported_order_types: vec![OrderType::Market, OrderType::Limit],
            supported_tif: vec![TimeInForce::GTC, TimeInForce::IOC, TimeInForce::FOK],
        }
    }

    /// Set the venue name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the capabilities.
    pub fn with_capabilities(mut self, capabilities: VenueCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set supported order types.
    pub fn with_order_types(mut self, types: Vec<OrderType>) -> Self {
        self.supported_order_types = types;
        self
    }

    /// Set supported time-in-force options.
    pub fn with_tif(mut self, tif: Vec<TimeInForce>) -> Self {
        self.supported_tif = tif;
        self
    }

    /// Check if an order type is supported.
    pub fn supports_order_type(&self, order_type: &OrderType) -> bool {
        self.supported_order_types.contains(order_type)
    }

    /// Check if a time-in-force option is supported.
    pub fn supports_tif(&self, tif: &TimeInForce) -> bool {
        self.supported_tif.contains(tif)
    }

    // Convenience accessors for common capability checks

    /// Whether the venue supports order modification.
    pub fn supports_modify(&self) -> bool {
        self.capabilities.supports_modify
    }

    /// Whether the venue supports batch operations.
    pub fn supports_batch(&self) -> bool {
        self.capabilities.supports_batch
    }

    /// Maximum orders per batch.
    pub fn max_orders_per_batch(&self) -> usize {
        self.capabilities.max_orders_per_batch
    }

    /// Whether the venue supports stop orders.
    pub fn supports_stop_orders(&self) -> bool {
        self.capabilities.supports_stop_orders
    }

    /// Whether the venue supports trailing stop orders.
    pub fn supports_trailing_stop(&self) -> bool {
        self.capabilities.supports_trailing_stop
    }

    /// Whether the venue supports live streaming.
    pub fn supports_live_streaming(&self) -> bool {
        self.capabilities.supports_live_streaming
    }

    /// Whether the venue supports historical data.
    pub fn supports_historical(&self) -> bool {
        self.capabilities.supports_historical
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_venue_info_builder() {
        let info = VenueInfo::new("binance_us_spot", "Binance.US Spot")
            .with_name("BINANCE_US")
            .with_capabilities(VenueCapabilities {
                supports_modify: true,
                supports_batch: true,
                max_orders_per_batch: 10,
                supports_stop_orders: true,
                ..VenueCapabilities::default()
            })
            .with_order_types(vec![OrderType::Market, OrderType::Limit, OrderType::Stop]);

        assert_eq!(info.venue_id, "binance_us_spot");
        assert!(info.supports_modify());
        assert!(info.supports_batch());
        assert_eq!(info.max_orders_per_batch(), 10);
        assert!(info.supports_order_type(&OrderType::Stop));
        assert!(!info.supports_order_type(&OrderType::TrailingStop));
    }

    #[test]
    fn test_capabilities_presets() {
        let data = VenueCapabilities::data_only();
        assert!(data.supports_live_streaming);
        assert!(!data.supports_orders);

        let exec = VenueCapabilities::execution_only();
        assert!(!exec.supports_live_streaming);
        assert!(exec.supports_orders);

        let full = VenueCapabilities::full();
        assert!(full.supports_live_streaming);
        assert!(full.supports_orders);
    }
}
