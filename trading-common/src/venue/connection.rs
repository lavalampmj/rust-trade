//! Base venue connection trait.
//!
//! This module defines the [`VenueConnection`] trait that provides the common
//! interface for connecting to and managing the lifecycle of any venue connection,
//! whether for data streaming or order execution.

use async_trait::async_trait;

use super::error::VenueResult;
use super::types::VenueInfo;

/// Connection status for a venue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionStatus {
    /// Not connected
    Disconnected,
    /// Connection in progress
    Connecting,
    /// Connected and ready
    Connected,
    /// Reconnecting after a disconnect
    Reconnecting,
    /// Connection error
    Error,
}

impl ConnectionStatus {
    /// Returns true if the venue is ready for operations.
    pub fn is_ready(&self) -> bool {
        matches!(self, ConnectionStatus::Connected)
    }

    /// Returns true if the venue is in an error state.
    pub fn is_error(&self) -> bool {
        matches!(self, ConnectionStatus::Error)
    }

    /// Returns true if the venue is attempting to connect.
    pub fn is_connecting(&self) -> bool {
        matches!(
            self,
            ConnectionStatus::Connecting | ConnectionStatus::Reconnecting
        )
    }
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        ConnectionStatus::Disconnected
    }
}

/// Base trait for all venue connections.
///
/// This trait provides the common interface for connecting to and managing
/// the lifecycle of any venue connection. Both data venues and execution
/// venues extend this trait.
///
/// # Example
///
/// ```ignore
/// use trading_common::venue::{VenueConnection, VenueResult};
///
/// async fn connect_and_use<V: VenueConnection>(venue: &mut V) -> VenueResult<()> {
///     // Connect to the venue
///     venue.connect().await?;
///
///     // Check connection status
///     if venue.is_connected() {
///         println!("Connected to {}", venue.info().display_name);
///     }
///
///     // Disconnect when done
///     venue.disconnect().await?;
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait VenueConnection: Send + Sync {
    /// Returns information about this venue's capabilities.
    fn info(&self) -> &VenueInfo;

    /// Connect to the venue.
    ///
    /// This establishes the initial connection (e.g., validates API credentials,
    /// creates HTTP client, etc.). WebSocket streams are typically started
    /// separately via venue-specific stream methods.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails.
    async fn connect(&mut self) -> VenueResult<()>;

    /// Disconnect from the venue.
    ///
    /// This closes all connections and releases resources.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails.
    async fn disconnect(&mut self) -> VenueResult<()>;

    /// Returns true if the venue is connected and ready for operations.
    fn is_connected(&self) -> bool;

    /// Returns the current connection status.
    fn connection_status(&self) -> ConnectionStatus;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_status() {
        assert!(ConnectionStatus::Connected.is_ready());
        assert!(!ConnectionStatus::Connecting.is_ready());
        assert!(ConnectionStatus::Error.is_error());
        assert!(ConnectionStatus::Reconnecting.is_connecting());
        assert_eq!(ConnectionStatus::default(), ConnectionStatus::Disconnected);
    }
}
