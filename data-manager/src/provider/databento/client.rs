//! Databento client wrapper

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::config::DatabentoSettings;
use crate::provider::{
    ConnectionStatus, DataAvailability, DataProvider, DataType, HistoricalDataProvider,
    HistoricalRequest, LiveStreamProvider, LiveSubscription, ProviderError, ProviderInfo,
    ProviderResult, StreamCallback, StreamEvent, SubscriptionStatus,
};
use crate::schema::NormalizedOHLC;
use crate::symbol::SymbolSpec;
use trading_common::data::types::TickData;

use super::normalizer::DatabentoNormalizer;

/// Databento data provider
#[allow(dead_code)] // Fields used for future implementation
pub struct DatabentoClient {
    /// Provider information
    info: ProviderInfo,
    /// Databento API key
    api_key: String,
    /// Default dataset
    default_dataset: String,
    /// Connection status
    connected: bool,
    /// Subscription status
    subscription_status: SubscriptionStatus,
    /// Normalizer for converting DBN messages
    normalizer: DatabentoNormalizer,
}

impl DatabentoClient {
    /// Create a new Databento client
    pub fn new(settings: &DatabentoSettings) -> Self {
        let info = ProviderInfo {
            name: "databento".to_string(),
            display_name: "Databento".to_string(),
            version: "0.18".to_string(),
            supported_exchanges: vec![
                "CME".to_string(),
                "CBOT".to_string(),
                "COMEX".to_string(),
                "NYMEX".to_string(),
                "NYSE".to_string(),
                "NASDAQ".to_string(),
                "BATS".to_string(),
                "IEX".to_string(),
            ],
            supported_data_types: vec![
                DataType::Trades,
                DataType::Quotes,
                DataType::OrderBook,
                DataType::OHLC,
            ],
            supports_historical: true,
            supports_live: true,
        };

        Self {
            info,
            api_key: settings.api_key.clone(),
            default_dataset: settings.default_dataset.clone(),
            connected: false,
            subscription_status: SubscriptionStatus {
                symbols: vec![],
                connection: ConnectionStatus::Disconnected,
                messages_received: 0,
                last_message: None,
            },
            normalizer: DatabentoNormalizer::new(),
        }
    }

    /// Create from API key directly
    pub fn from_api_key(api_key: String) -> Self {
        let settings = DatabentoSettings {
            api_key,
            default_dataset: "GLBX.MDP3".to_string(),
        };
        Self::new(&settings)
    }

    /// Get the dataset for a request
    fn get_dataset<'a>(&'a self, request_dataset: Option<&'a str>) -> &'a str {
        request_dataset.unwrap_or(&self.default_dataset)
    }
}

#[async_trait]
impl DataProvider for DatabentoClient {
    fn info(&self) -> &ProviderInfo {
        &self.info
    }

    async fn connect(&mut self) -> ProviderResult<()> {
        if self.connected {
            return Ok(());
        }

        // Validate API key by making a simple request
        // In production, we'd make an actual validation call to Databento
        if self.api_key.is_empty() {
            return Err(ProviderError::Authentication(
                "Databento API key not configured".to_string(),
            ));
        }

        info!("Connected to Databento");
        self.connected = true;
        self.subscription_status.connection = ConnectionStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ProviderResult<()> {
        self.connected = false;
        self.subscription_status.connection = ConnectionStatus::Disconnected;
        info!("Disconnected from Databento");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn discover_symbols(&self, exchange: Option<&str>) -> ProviderResult<Vec<SymbolSpec>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        // In production, we'd query Databento's metadata API
        // For now, return a stub list of common symbols
        let symbols = match exchange {
            Some("CME") | Some("GLBX") => vec![
                SymbolSpec::new("ES", "CME"),
                SymbolSpec::new("NQ", "CME"),
                SymbolSpec::new("CL", "CME"),
                SymbolSpec::new("GC", "CME"),
                SymbolSpec::new("ZN", "CME"),
                SymbolSpec::new("ZB", "CME"),
            ],
            Some("NASDAQ") | Some("XNAS") => vec![
                SymbolSpec::new("AAPL", "NASDAQ"),
                SymbolSpec::new("MSFT", "NASDAQ"),
                SymbolSpec::new("GOOGL", "NASDAQ"),
                SymbolSpec::new("AMZN", "NASDAQ"),
            ],
            Some("NYSE") | Some("XNYS") => vec![
                SymbolSpec::new("SPY", "NYSE"),
                SymbolSpec::new("QQQ", "NYSE"),
                SymbolSpec::new("IWM", "NYSE"),
            ],
            None => {
                // Return symbols from all supported exchanges
                let mut all = vec![];
                all.extend(self.discover_symbols(Some("CME")).await?);
                all.extend(self.discover_symbols(Some("NASDAQ")).await?);
                all.extend(self.discover_symbols(Some("NYSE")).await?);
                all
            }
            Some(ex) => {
                warn!("Unknown exchange: {}", ex);
                vec![]
            }
        };

        Ok(symbols)
    }
}

#[async_trait]
impl HistoricalDataProvider for DatabentoClient {
    async fn fetch_ticks(
        &self,
        request: &HistoricalRequest,
    ) -> ProviderResult<Box<dyn Iterator<Item = ProviderResult<TickData>> + Send>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let dataset = self.get_dataset(request.dataset.as_deref());
        debug!(
            "Fetching ticks from Databento: dataset={}, symbols={:?}, start={}, end={}",
            dataset, request.symbols, request.start, request.end
        );

        // In production, this would use the databento crate to fetch data
        // For now, return an empty iterator as a placeholder
        //
        // Example of what the real implementation would look like:
        // ```
        // let client = databento::HistoricalClient::builder()
        //     .key(&self.api_key)?
        //     .build()?;
        //
        // let symbols: Vec<&str> = request.symbols.iter()
        //     .map(|s| s.symbol.as_str())
        //     .collect();
        //
        // let decoder = client.timeseries()
        //     .get_range()
        //     .dataset(dataset)
        //     .symbols(&symbols)
        //     .schema(dbn::Schema::Trades)
        //     .stype_in(dbn::SType::RawSymbol)
        //     .start(request.start.format("%Y-%m-%dT%H:%M:%S%.fZ").to_string())
        //     .end(request.end.format("%Y-%m-%dT%H:%M:%S%.fZ").to_string())
        //     .send()
        //     .await?;
        //
        // let normalizer = self.normalizer.clone();
        // let iter = decoder.decode().filter_map(move |msg| {
        //     if let Ok(trade) = msg.get::<dbn::TradeMsg>() {
        //         Some(normalizer.normalize_trade(trade, dataset))
        //     } else {
        //         None
        //     }
        // });
        // ```

        Ok(Box::new(std::iter::empty()))
    }

    async fn fetch_ohlc(&self, request: &HistoricalRequest) -> ProviderResult<Vec<NormalizedOHLC>> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let dataset = self.get_dataset(request.dataset.as_deref());
        debug!(
            "Fetching OHLC from Databento: dataset={}, symbols={:?}",
            dataset, request.symbols
        );

        // Placeholder - would use databento crate in production
        Ok(vec![])
    }

    async fn check_availability(
        &self,
        _symbol: &SymbolSpec,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> ProviderResult<DataAvailability> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        // In production, query Databento's metadata API
        // For now, return a placeholder
        Ok(DataAvailability {
            available: true,
            actual_start: Some(start),
            actual_end: Some(end),
            estimated_records: None,
        })
    }
}

#[async_trait]
impl LiveStreamProvider for DatabentoClient {
    async fn subscribe(
        &mut self,
        subscription: LiveSubscription,
        callback: StreamCallback,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> ProviderResult<()> {
        if !self.connected {
            return Err(ProviderError::NotConnected);
        }

        let dataset = self.get_dataset(subscription.dataset.as_deref());
        info!(
            "Subscribing to live data: dataset={}, symbols={:?}",
            dataset, subscription.symbols
        );

        self.subscription_status.symbols = subscription.symbols.clone();

        // In production, this would use databento::LiveClient
        // Example:
        // ```
        // let mut client = databento::LiveClient::builder()
        //     .key(&self.api_key)?
        //     .dataset(dataset)?
        //     .build()
        //     .await?;
        //
        // client.subscribe(
        //     &symbols,
        //     dbn::Schema::Trades,
        //     dbn::SType::RawSymbol,
        // ).await?;
        //
        // client.start().await?;
        //
        // loop {
        //     tokio::select! {
        //         _ = shutdown_rx.recv() => break,
        //         msg = client.next_record() => {
        //             if let Some(record) = msg {
        //                 let tick = self.normalizer.normalize_trade(&record, dataset)?;
        //                 callback(StreamEvent::Tick(tick));
        //                 self.subscription_status.messages_received += 1;
        //                 self.subscription_status.last_message = Some(Utc::now());
        //             }
        //         }
        //     }
        // }
        // ```

        // Notify that we're connected
        callback(StreamEvent::Status(ConnectionStatus::Connected));

        // Wait for shutdown signal (placeholder)
        let _ = shutdown_rx.recv().await;

        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: &[SymbolSpec]) -> ProviderResult<()> {
        self.subscription_status.symbols.retain(|s| !symbols.contains(s));
        info!("Unsubscribed from {:?}", symbols);
        Ok(())
    }

    fn subscription_status(&self) -> SubscriptionStatus {
        self.subscription_status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let settings = DatabentoSettings {
            api_key: "test_key".to_string(),
            default_dataset: "GLBX.MDP3".to_string(),
        };
        let client = DatabentoClient::new(&settings);
        assert_eq!(client.info().name, "databento");
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_connect_without_key() {
        let settings = DatabentoSettings {
            api_key: String::new(),
            default_dataset: "GLBX.MDP3".to_string(),
        };
        let mut client = DatabentoClient::new(&settings);
        let result = client.connect().await;
        assert!(result.is_err());
    }
}
