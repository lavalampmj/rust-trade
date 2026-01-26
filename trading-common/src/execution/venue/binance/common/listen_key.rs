//! Listen key management for Binance User Data Stream.
//!
//! Listen keys are required to connect to the User Data Stream WebSocket.
//! They must be refreshed every 30 minutes to keep the connection alive.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::execution::venue::error::VenueResult;
use crate::execution::venue::http::HttpClient;

use super::types::{EmptyResponse, ListenKeyResponse};

/// Default keepalive interval (30 minutes as recommended by Binance).
const DEFAULT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Keepalive margin - refresh a bit earlier than expiry.
const KEEPALIVE_MARGIN: Duration = Duration::from_secs(60);

/// Listen key manager for Binance User Data Stream.
///
/// This manager handles:
/// - Creating listen keys
/// - Automatic keepalive (refresh every 30 minutes)
/// - Closing listen keys
///
/// # Example
///
/// ```ignore
/// let manager = ListenKeyManager::new(
///     http_client.clone(),
///     "/api/v3/userDataStream",
/// );
///
/// // Start the manager (creates listen key and starts keepalive)
/// let listen_key = manager.start(shutdown_rx.resubscribe()).await?;
///
/// // Connect to WebSocket with listen key
/// let ws_url = format!("{}/{}", base_ws_url, listen_key);
///
/// // When done, stop the manager
/// manager.stop().await?;
/// ```
pub struct ListenKeyManager {
    /// HTTP client for API calls
    http_client: Arc<HttpClient>,
    /// Endpoint for listen key operations
    endpoint: String,
    /// Current listen key
    listen_key: RwLock<Option<String>>,
    /// Keepalive task handle
    keepalive_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Keepalive interval
    keepalive_interval: Duration,
}

impl ListenKeyManager {
    /// Create a new listen key manager.
    ///
    /// # Arguments
    ///
    /// * `http_client` - HTTP client for API calls
    /// * `endpoint` - The listen key endpoint (e.g., "/api/v3/userDataStream")
    pub fn new(http_client: Arc<HttpClient>, endpoint: impl Into<String>) -> Self {
        Self {
            http_client,
            endpoint: endpoint.into(),
            listen_key: RwLock::new(None),
            keepalive_handle: Mutex::new(None),
            keepalive_interval: DEFAULT_KEEPALIVE_INTERVAL - KEEPALIVE_MARGIN,
        }
    }

    /// Set a custom keepalive interval.
    pub fn with_keepalive_interval(mut self, interval: Duration) -> Self {
        self.keepalive_interval = interval;
        self
    }

    /// Create a new listen key.
    async fn create_listen_key(&self) -> VenueResult<String> {
        debug!("Creating new listen key");

        let response: ListenKeyResponse = self
            .http_client
            .post_signed(&self.endpoint, &[], 1)
            .await?;

        info!("Created listen key: {}...", &response.listen_key[..8]);
        Ok(response.listen_key)
    }

    /// Refresh the listen key (keepalive).
    async fn keepalive_listen_key(&self, listen_key: &str) -> VenueResult<()> {
        debug!("Refreshing listen key");

        let _: EmptyResponse = self
            .http_client
            .put_signed(&self.endpoint, &[("listenKey", listen_key)], 1)
            .await?;

        debug!("Listen key refreshed");
        Ok(())
    }

    /// Close the listen key.
    async fn close_listen_key(&self, listen_key: &str) -> VenueResult<()> {
        debug!("Closing listen key");

        let _: EmptyResponse = self
            .http_client
            .delete_signed(&self.endpoint, &[("listenKey", listen_key)], 1)
            .await?;

        info!("Listen key closed");
        Ok(())
    }

    /// Start the listen key manager.
    ///
    /// Creates a new listen key and starts the keepalive background task.
    ///
    /// # Arguments
    ///
    /// * `shutdown_rx` - Receiver for shutdown signal
    ///
    /// # Returns
    ///
    /// The created listen key.
    pub async fn start(&self, shutdown_rx: broadcast::Receiver<()>) -> VenueResult<String> {
        // Create listen key
        let listen_key = self.create_listen_key().await?;

        // Store listen key
        {
            let mut key = self.listen_key.write().await;
            *key = Some(listen_key.clone());
        }

        // Start keepalive task
        let manager_listen_key = listen_key.clone();
        let http_client = self.http_client.clone();
        let endpoint = self.endpoint.clone();
        let keepalive_interval = self.keepalive_interval;

        let handle = tokio::spawn(async move {
            Self::keepalive_task(
                http_client,
                endpoint,
                manager_listen_key,
                keepalive_interval,
                shutdown_rx,
            )
            .await;
        });

        {
            let mut handle_lock = self.keepalive_handle.lock().await;
            *handle_lock = Some(handle);
        }

        Ok(listen_key)
    }

    /// Keepalive background task.
    async fn keepalive_task(
        http_client: Arc<HttpClient>,
        endpoint: String,
        listen_key: String,
        keepalive_interval: Duration,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) {
        let mut interval = interval(keepalive_interval);

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    debug!("Listen key keepalive task shutting down");
                    break;
                }
                _ = interval.tick() => {
                    // Perform keepalive
                    let result: Result<EmptyResponse, _> = http_client
                        .put_signed(&endpoint, &[("listenKey", listen_key.as_str())], 1)
                        .await;

                    match result {
                        Ok(_) => {
                            debug!("Listen key keepalive successful");
                        }
                        Err(e) => {
                            warn!("Listen key keepalive failed: {}. Will retry on next interval.", e);
                        }
                    }
                }
            }
        }
    }

    /// Get the current listen key.
    pub async fn get_listen_key(&self) -> Option<String> {
        self.listen_key.read().await.clone()
    }

    /// Stop the listen key manager.
    ///
    /// Stops the keepalive task and closes the listen key.
    pub async fn stop(&self) -> VenueResult<()> {
        // Stop keepalive task
        {
            let mut handle = self.keepalive_handle.lock().await;
            if let Some(h) = handle.take() {
                h.abort();
            }
        }

        // Close listen key
        let listen_key = {
            let mut key = self.listen_key.write().await;
            key.take()
        };

        if let Some(key) = listen_key {
            if let Err(e) = self.close_listen_key(&key).await {
                // Don't fail if close fails, just warn
                warn!("Failed to close listen key: {}", e);
            }
        }

        Ok(())
    }

    /// Check if the manager is running.
    pub async fn is_running(&self) -> bool {
        let handle = self.keepalive_handle.lock().await;
        handle.is_some() && !handle.as_ref().unwrap().is_finished()
    }
}

impl Drop for ListenKeyManager {
    fn drop(&mut self) {
        // Best effort cleanup - can't do async in Drop
        // The listen key will eventually expire on its own
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_keepalive_interval() {
        assert_eq!(
            DEFAULT_KEEPALIVE_INTERVAL,
            Duration::from_secs(30 * 60)
        );
    }

    #[test]
    fn test_keepalive_margin() {
        let effective_interval = DEFAULT_KEEPALIVE_INTERVAL - KEEPALIVE_MARGIN;
        assert_eq!(effective_interval, Duration::from_secs(29 * 60));
    }
}
