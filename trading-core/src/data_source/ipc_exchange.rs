//! IPC Exchange Adapter
//!
//! Provides an Exchange trait implementation that reads from IPC shared memory
//! instead of connecting to an external WebSocket.

use async_trait::async_trait;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use crate::exchange::{Exchange, ExchangeError};
use crate::metrics::IPC_CONNECTION_STATUS;
use trading_common::data::types::TickData;

use super::{IpcDataSource, IpcDataSourceConfig};

/// Exchange implementation that reads from IPC shared memory
///
/// This allows trading-core to receive data from data-manager
/// instead of connecting directly to Binance.
pub struct IpcExchange {
    config: IpcDataSourceConfig,
    /// Exchange identifier (e.g., "BINANCE")
    exchange: String,
}

impl IpcExchange {
    /// Create a new IPC exchange adapter
    pub fn new(config: IpcDataSourceConfig, exchange: &str) -> Self {
        Self {
            config,
            exchange: exchange.to_string(),
        }
    }

    /// Create with default config for Binance
    pub fn binance() -> Self {
        Self::new(IpcDataSourceConfig::default(), "BINANCE")
    }
}

#[async_trait]
impl Exchange for IpcExchange {
    async fn subscribe_trades(
        &self,
        symbols: &[String],
        callback: Box<dyn Fn(TickData) + Send + Sync>,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<(), ExchangeError> {
        if symbols.is_empty() {
            return Err(ExchangeError::InvalidSymbol(
                "No symbols provided".to_string(),
            ));
        }

        info!(
            "Starting IPC trade subscription for symbols: {:?} from {}",
            symbols, self.exchange
        );

        // Create IPC data source
        let mut data_source = IpcDataSource::new(self.config.clone());

        // Subscribe to all symbols
        for symbol in symbols {
            if let Err(e) = data_source.subscribe(symbol, &self.exchange) {
                // If symbol not available yet, log warning but continue
                warn!(
                    "Failed to subscribe to {}@{}: {} (data-manager may not be running)",
                    symbol, self.exchange, e
                );
            }
        }

        let subscribed = data_source.subscribed_symbols();
        if subscribed.is_empty() {
            error!("No symbols could be subscribed. Is data-manager running?");
            IPC_CONNECTION_STATUS.set(0);
            return Err(ExchangeError::ConnectionError(
                "No IPC channels available. Ensure data-manager is running.".to_string(),
            ));
        }

        // Mark IPC as connected for alerting system
        IPC_CONNECTION_STATUS.set(1);

        info!(
            "IPC subscription active for {} symbols: {:?}",
            subscribed.len(),
            subscribed
        );

        // Polling loop
        let poll_interval = self.config.poll_interval;
        let mut consecutive_empty = 0u32;
        let mut ticks_received = 0u64;
        let mut last_stats_time = std::time::Instant::now();

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    info!("Shutdown signal received, stopping IPC exchange");
                    break;
                }
                _ = async {
                    // Poll for ticks
                    let ticks = data_source.poll();

                    if ticks.is_empty() {
                        consecutive_empty += 1;
                        // Adaptive backoff
                        let sleep_time = if consecutive_empty > 100 {
                            std::time::Duration::from_millis(1)
                        } else {
                            poll_interval
                        };
                        tokio::time::sleep(sleep_time).await;
                    } else {
                        consecutive_empty = 0;
                        ticks_received += ticks.len() as u64;

                        for tick in ticks {
                            callback(tick);
                        }
                    }

                    // Log stats periodically (every 30 seconds)
                    if last_stats_time.elapsed() > std::time::Duration::from_secs(30) {
                        let stats = data_source.stats();
                        debug!(
                            "IPC stats: ticks_received={}, overflows={}, errors={}",
                            stats.ticks_received.load(std::sync::atomic::Ordering::Relaxed),
                            stats.overflows_detected.load(std::sync::atomic::Ordering::Relaxed),
                            stats.errors.load(std::sync::atomic::Ordering::Relaxed)
                        );
                        last_stats_time = std::time::Instant::now();
                    }
                } => {}
            }
        }

        info!(
            "IPC exchange stopped. Total ticks received: {}",
            ticks_received
        );

        // Mark IPC as disconnected for alerting system
        IPC_CONNECTION_STATUS.set(0);
        data_source.shutdown();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_exchange_creation() {
        let config = IpcDataSourceConfig::default();
        let exchange = IpcExchange::new(config, "BINANCE");
        assert_eq!(exchange.exchange, "BINANCE");
    }

    #[test]
    fn test_ipc_exchange_binance_default() {
        let exchange = IpcExchange::binance();
        assert_eq!(exchange.exchange, "BINANCE");
    }
}
