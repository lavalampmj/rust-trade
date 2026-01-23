# QuantConnect Lean - Complete Object Model Documentation

This document provides a comprehensive reference of all major objects, classes, interfaces, and their relationships within the QuantConnect Lean algorithmic trading engine.

---

## Table of Contents

1. [System Architecture Overview](#1-system-architecture-overview)
2. [Core Domain Objects](#2-core-domain-objects)
3. [Securities System](#3-securities-system)
4. [Orders System](#4-orders-system)
5. [Data System](#5-data-system)
6. [Algorithm Framework](#6-algorithm-framework)
7. [Engine Components](#7-engine-components)
8. [Brokerage Models](#8-brokerage-models)
9. [Object Relationship Diagrams](#9-object-relationship-diagrams)

---

## 1. System Architecture Overview

### High-Level Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USER ALGORITHM                                  │
│                         (QCAlgorithm : IAlgorithm)                          │
└─────────────────────────────────┬───────────────────────────────────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          │                       │                       │
          ▼                       ▼                       ▼
┌─────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
│   Securities    │   │    Transactions     │   │     Portfolio       │
│   Manager       │   │    Manager          │   │     Manager         │
│                 │   │                     │   │                     │
│ Dict<Symbol,    │   │ Order processing    │   │ Holdings + Cash     │
│      Security>  │   │ P&L tracking        │   │ Margin management   │
└────────┬────────┘   └──────────┬──────────┘   └──────────┬──────────┘
         │                       │                         │
         └───────────────────────┼─────────────────────────┘
                                 │
                                 ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                               ENGINE                                         │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ ┌─────────────┐           │
│  │ DataFeed    │ │Transaction  │ │  Result     │ │  RealTime   │           │
│  │ Handler     │ │ Handler     │ │  Handler    │ │  Handler    │           │
│  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘ └──────┬──────┘           │
│         │               │               │               │                   │
│         └───────────────┼───────────────┼───────────────┘                   │
│                         ▼               ▼                                    │
│                  ┌─────────────────────────────┐                            │
│                  │       Brokerage             │                            │
│                  │  (Backtesting or Live)      │                            │
│                  └─────────────────────────────┘                            │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Core Namespaces

| Namespace | Purpose |
|-----------|---------|
| `QuantConnect` | Core types (Symbol, Securities, Orders) |
| `QuantConnect.Algorithm` | Algorithm base classes (QCAlgorithm) |
| `QuantConnect.Algorithm.Framework` | Framework models (Alpha, Portfolio, Execution, Risk, Selection) |
| `QuantConnect.Data` | Market data types (BaseData, Slice, TradeBar, Tick) |
| `QuantConnect.Orders` | Order types and management |
| `QuantConnect.Securities` | Security definitions and models |
| `QuantConnect.Brokerages` | Brokerage models and integrations |
| `QuantConnect.Indicators` | Technical indicators |
| `QuantConnect.Lean.Engine` | Engine implementation |

---

## 2. Core Domain Objects

### 2.1 Symbol

The fundamental identifier for all tradeable assets.

```
Symbol
├── ID (SecurityIdentifier)
│   ├── Market (string)
│   ├── SecurityType (enum)
│   ├── Date (for options/futures expiry)
│   ├── Strike (for options)
│   └── OptionRight (Put/Call)
├── Value (string ticker)
├── Underlying (Symbol - for derivatives)
└── SecurityType (enum)
```

**SecurityType Enum:**
- `Equity`, `Option`, `Future`, `Forex`, `Crypto`, `Cfd`, `Index`
- `FutureOption`, `IndexOption`, `CryptoFuture`

### 2.2 Resolution

Data granularity for subscriptions.

```csharp
enum Resolution
{
    Tick = 0,      // Individual ticks
    Second = 1,    // 1-second bars
    Minute = 2,    // 1-minute bars (default)
    Hour = 3,      // 1-hour bars
    Daily = 4      // 1-day bars
}
```

### 2.3 MarketDataType

Classification of data types.

```csharp
enum MarketDataType
{
    Base = 0,           // Custom data
    TradeBar = 1,       // OHLCV bars
    Tick = 2,           // Individual ticks
    Auxiliary = 3,      // Splits, Dividends, Delistings
    QuoteBar = 4,       // Bid/Ask OHLC bars
    OptionChain = 5,    // Option chain data
    FuturesChain = 6    // Futures chain data
}
```

---

## 3. Securities System

### 3.1 Security Class Hierarchy

```
Security (base class)
├── Equity
├── Option
├── Future
├── Forex
├── Crypto
├── Cfd
├── Index
├── FutureOption
├── IndexOption
└── CryptoFuture
```

### 3.2 Security Object Structure

Each `Security` contains:

```
Security
├── Symbol                    // Unique identifier
├── Type (SecurityType)       // Asset class
├── Price                     // Current price
├── Open, High, Low, Close    // OHLC prices
├── Volume                    // Current volume
├── BidPrice, AskPrice        // Quote prices
├── IsTradable                // Can be traded
├── IsDelisted                // Delisted flag
│
├── Holdings (SecurityHolding)
│   ├── Quantity              // Position size
│   ├── AveragePrice          // Cost basis
│   ├── HoldingsValue         // Market value
│   ├── UnrealizedProfit      // Open P&L
│   ├── Profit                // Realized P&L
│   └── TotalFees             // Accumulated fees
│
├── Cache (SecurityCache)
│   ├── OHLCV data storage
│   └── Custom properties
│
├── Exchange (SecurityExchange)
│   ├── Hours (market hours)
│   ├── TimeZone
│   └── ExchangeOpen (bool)
│
├── SymbolProperties
│   ├── ContractMultiplier
│   ├── MinimumPriceVariation (tick size)
│   ├── LotSize
│   └── QuoteCurrency
│
└── Models (Pluggable)
    ├── FeeModel (IFeeModel)
    ├── FillModel (IFillModel)
    ├── SlippageModel (ISlippageModel)
    ├── BuyingPowerModel (IBuyingPowerModel)
    ├── SettlementModel (ISettlementModel)
    ├── VolatilityModel (IVolatilityModel)
    └── MarginInterestRateModel
```

### 3.3 Security Managers

```
SecurityManager : IDictionary<Symbol, Security>
├── Add(Symbol, Security)
├── Remove(Symbol)
├── this[Symbol] indexer
└── CollectionChanged event

SecurityPortfolioManager : IDictionary<Symbol, SecurityHolding>
├── Securities reference
├── Transactions reference
├── CashBook (settled cash)
├── UnsettledCashBook
├── TotalPortfolioValue
├── MarginCallModel
└── Positions (position groups)

SecurityTransactionManager : IOrderProvider
├── Orders dictionary
├── OrderTickets dictionary
├── TransactionRecord (P&L by date)
├── WinCount, LossCount
└── WinningTransactions, LosingTransactions
```

### 3.4 Cash Management

```
CashBook : IDictionary<string, Cash>
├── AccountCurrency (e.g., "USD")
├── TotalValueInAccountCurrency
└── EnsureCurrencyDataFeeds()

Cash
├── Symbol (currency code)
├── Amount
├── ConversionRate
├── ValueInAccountCurrency
└── CurrencyConversion
```

### 3.5 Buying Power / Margin Models

```
IBuyingPowerModel
├── GetLeverage(Security)
├── SetLeverage(Security, leverage)
├── GetMaintenanceMargin(parameters)
├── GetInitialMarginRequirement(parameters)
├── HasSufficientBuyingPowerForOrder(parameters)
├── GetMaximumOrderQuantityForTargetBuyingPower(parameters)
├── GetReservedBuyingPowerForPosition(parameters)
└── GetBuyingPower(parameters)

Implementations:
├── BuyingPowerModel (base)
├── SecurityMarginModel
├── CashBuyingPowerModel
├── FutureMarginModel
├── CryptoFutureMarginModel
├── OptionMarginModel
├── FuturesOptionsMarginModel
├── PatternDayTradingMarginModel
├── ConstantBuyingPowerModel
└── NullBuyingPowerModel
```

### 3.6 Settlement Models

```
ISettlementModel
├── ApplyFunds(parameters)
├── Scan(parameters)
└── GetUnsettledCash()

Implementations:
├── ImmediateSettlementModel     // T+0
├── DelayedSettlementModel       // T+N
├── FutureSettlementModel        // Mark-to-market
└── AccountCurrencyImmediateSettlementModel
```

---

## 4. Orders System

### 4.1 Order Class Hierarchy

```
Order (abstract base)
├── MarketOrder
├── LimitOrder
├── StopMarketOrder
│   └── TrailingStopOrder
├── StopLimitOrder
├── LimitIfTouchedOrder
├── MarketOnOpenOrder
├── MarketOnCloseOrder
├── OptionExerciseOrder
└── ComboOrder (abstract)
    ├── ComboMarketOrder
    ├── ComboLimitOrder
    └── ComboLegLimitOrder
```

### 4.2 Order Object Structure

```
Order
├── Id (int)
├── Symbol
├── Quantity
├── Type (OrderType enum)
├── Status (OrderStatus enum)
├── Direction (Buy/Sell/Hold)
├── Price
├── Time, CreatedTime
├── LastFillTime, LastUpdateTime, CanceledTime
├── Tag (user metadata)
├── Properties (IOrderProperties)
│   └── TimeInForce
├── GroupOrderManager (for combos)
└── OrderSubmissionData
```

### 4.3 Order Types Enum

```csharp
enum OrderType
{
    Market = 0,
    Limit = 1,
    StopMarket = 2,
    StopLimit = 3,
    MarketOnOpen = 4,
    MarketOnClose = 5,
    OptionExercise = 6,
    LimitIfTouched = 7,
    ComboMarket = 8,
    ComboLimit = 9,
    ComboLegLimit = 10,
    TrailingStop = 11
}
```

### 4.4 Order Status Enum

```csharp
enum OrderStatus
{
    New = 0,
    Submitted = 1,
    PartiallyFilled = 2,
    Filled = 3,
    Canceled = 5,
    None = 6,
    Invalid = 7,
    CancelPending = 8,
    UpdateSubmitted = 9
}
```

### 4.5 Order Lifecycle Objects

```
OrderTicket
├── OrderId
├── Status
├── Symbol
├── Quantity
├── AverageFillPrice
├── QuantityFilled, QuantityRemaining
├── SubmitRequest (SubmitOrderRequest)
├── UpdateRequests (List<UpdateOrderRequest>)
├── CancelRequest
├── OrderEvents (List<OrderEvent>)
├── OrderClosed (WaitHandle)
├── Update(fields), Cancel()
└── Implicit cast to int (OrderId)

OrderEvent
├── OrderId, Id
├── Symbol
├── UtcTime
├── Status
├── OrderFee
├── FillPrice, FillQuantity
├── Direction
├── Message
├── IsAssignment
├── StopPrice, TriggerPrice, LimitPrice
└── Ticket reference

OrderRequest (abstract)
├── SubmitOrderRequest
├── UpdateOrderRequest
└── CancelOrderRequest

OrderResponse
├── OrderId
├── ErrorMessage
├── ErrorCode (OrderResponseErrorCode)
├── IsSuccess, IsError, IsProcessed
```

### 4.6 Time In Force

```
TimeInForce (abstract)
├── GoodTilCanceledTimeInForce  // Never expires
├── DayTimeInForce              // Expires at market close
└── GoodTilDateTimeInForce      // Expires at specific date
    └── Expiry (DateTime)
```

---

## 5. Data System

### 5.1 BaseData Hierarchy

```
BaseData (abstract)
├── Tick
│   ├── TickType (Trade/Quote/OpenInterest)
│   ├── BidPrice, BidSize
│   ├── AskPrice, AskSize
│   ├── Quantity (for trades)
│   └── Exchange
├── TradeBar : IBaseDataBar
│   ├── Open, High, Low, Close
│   ├── Volume
│   └── Period
├── QuoteBar : IBaseDataBar
│   ├── Bid (Bar)
│   ├── Ask (Bar)
│   └── Computed OHLC (midpoint)
├── Dividend
│   ├── Distribution
│   └── ReferencePrice
├── Split
│   ├── Type (Warning/SplitOccurred)
│   ├── SplitFactor
│   └── ReferencePrice
├── Delisting
│   ├── Type (Warning/Delisted)
│   └── Ticket (for liquidation)
└── [Custom Data Types]
```

### 5.2 BaseData Properties

```
BaseData
├── Time (DateTime)
├── Symbol
├── Value (decimal) - primary value
├── Price (alias for Value)
├── EndTime
├── DataType (MarketDataType)
└── IsFillForward (bool)
```

### 5.3 Slice - Primary Data Container

```
Slice : ExtendedDictionary<Symbol, dynamic>
├── Time, UtcTime
├── HasData
├── AllData (IEnumerable<BaseData>)
│
├── Bars (TradeBars)           // Dict<Symbol, TradeBar>
├── QuoteBars                   // Dict<Symbol, QuoteBar>
├── Ticks                       // Dict<Symbol, List<Tick>>
│
├── OptionChains               // Dict<Symbol, OptionChain>
├── FuturesChains              // Dict<Symbol, FuturesChain>
│
├── Splits                     // Dict<Symbol, Split>
├── Dividends                  // Dict<Symbol, Dividend>
├── Delistings                 // Dict<Symbol, Delisting>
├── SymbolChangedEvents        // Symbol mapping events
└── MarginInterestRates
```

### 5.4 Chain Data Types

```
OptionChain : BaseChain<OptionContract, OptionContracts>
├── Underlying (BaseData)
├── Ticks, TradeBars, QuoteBars
├── Contracts (OptionContracts dictionary)
├── FilteredContracts
└── DataFrame (for Python)

FuturesChain : BaseChain<FuturesContract, FuturesContracts>
├── Underlying
├── Ticks, TradeBars, QuoteBars
├── Contracts (FuturesContracts dictionary)
└── FilteredContracts
```

### 5.5 SubscriptionDataConfig

```
SubscriptionDataConfig
├── Type (data class type)
├── Symbol
├── Resolution
├── TickType
├── Increment (TimeSpan)
├── FillDataForward
├── ExtendedMarketHours
├── IsInternalFeed
├── IsCustomData
├── DataTimeZone
├── ExchangeTimeZone
├── DataNormalizationMode
├── DataMappingMode
├── ContractDepthOffset
├── Consolidators
├── PriceScaleFactor
└── SumOfDividends
```

### 5.6 Universe Selection Data

```
CoarseFundamental : BaseData
├── Market
├── DollarVolume
├── Volume
├── HasFundamentalData
├── PriceFactor, SplitFactor
├── AdjustedPrice
└── Price

FineFundamental : BaseData
├── MarketCap
├── [Financial statement data]
└── [via FundamentalInstanceProvider]
```

---

## 6. Algorithm Framework

### 6.1 Framework Pipeline

```
┌──────────────────────────────────────────────────────────────────┐
│                    FRAMEWORK PIPELINE                             │
├──────────────────────────────────────────────────────────────────┤
│                                                                   │
│  1. Universe Selection (IUniverseSelectionModel)                 │
│     └──→ Selects securities to trade                             │
│                         │                                         │
│                         ▼                                         │
│  2. Alpha Generation (IAlphaModel)                               │
│     └──→ Generates Insight objects                               │
│                         │                                         │
│                         ▼                                         │
│  3. Portfolio Construction (IPortfolioConstructionModel)         │
│     └──→ Converts Insights → PortfolioTargets                    │
│                         │                                         │
│                         ▼                                         │
│  4. Risk Management (IRiskManagementModel)                       │
│     └──→ Modifies targets based on risk rules                    │
│                         │                                         │
│                         ▼                                         │
│  5. Execution (IExecutionModel)                                  │
│     └──→ Converts PortfolioTargets → Orders                      │
│                                                                   │
└──────────────────────────────────────────────────────────────────┘
```

### 6.2 Framework Interfaces

```
IUniverseSelectionModel
├── GetNextRefreshTimeUtc()
└── CreateUniverses(algorithm)

IAlphaModel : INotifiedSecurityChanges
└── Update(algorithm, slice) → IEnumerable<Insight>

IPortfolioConstructionModel : INotifiedSecurityChanges
└── CreateTargets(algorithm, insights) → IEnumerable<IPortfolioTarget>

IRiskManagementModel : INotifiedSecurityChanges
└── ManageRisk(algorithm, targets) → IEnumerable<IPortfolioTarget>

IExecutionModel : INotifiedSecurityChanges
├── Execute(algorithm, targets)
└── OnOrderEvent(algorithm, orderEvent)

INotifiedSecurityChanges
└── OnSecuritiesChanged(algorithm, changes)
```

### 6.3 Insight Object

```
Insight
├── Id (Guid)
├── GroupId (Guid?)
├── Symbol
├── Type (InsightType: Price, Volatility)
├── Direction (Up, Down, Flat)
├── Period (TimeSpan)
├── Magnitude (decimal?)
├── Confidence (double?)
├── Weight (double?)
├── GeneratedTimeUtc
├── CloseTimeUtc
├── Score (InsightScore)
├── SourceModel
├── Tag
│
├── IsExpired(time)
├── IsActive(time)
├── Expire(time)
├── Cancel(time)
└── Clone()

Static Factory Methods:
├── Insight.Price(symbol, period, direction, ...)
├── Insight.Price(symbol, resolution, barCount, direction, ...)
└── Insight.Group(insights)
```

### 6.4 PortfolioTarget Object

```
IPortfolioTarget
├── Symbol
├── Quantity
└── Tag

PortfolioTarget : IPortfolioTarget
├── Constructor(symbol, quantity, tag)
├── Constructor(symbol, direction, tag)
└── Static: Percent(algorithm, symbol, percent, ...)
```

### 6.5 Alpha Model Implementations

| Model | Description |
|-------|-------------|
| `EmaCrossAlphaModel` | EMA crossover signals |
| `MacdAlphaModel` | MACD indicator signals |
| `RsiAlphaModel` | RSI overbought/oversold |
| `ConstantAlphaModel` | Fixed direction signals |
| `HistoricalReturnsAlphaModel` | Historical returns analysis |
| `BasePairsTradingAlphaModel` | Pairs trading base |
| `PearsonCorrelationPairsTradingAlphaModel` | Correlation-based pairs |

### 6.6 Portfolio Construction Implementations

| Model | Description |
|-------|-------------|
| `EqualWeightingPortfolioConstructionModel` | Equal 1/N weighting |
| `InsightWeightingPortfolioConstructionModel` | Insight weight-based |
| `ConfidenceWeightedPortfolioConstructionModel` | Confidence-based |
| `MeanVarianceOptimizationPortfolioConstructionModel` | MPT optimization |
| `BlackLittermanOptimizationPortfolioConstructionModel` | Bayesian optimization |
| `RiskParityPortfolioConstructionModel` | Equal risk contribution |
| `SectorWeightingPortfolioConstructionModel` | Sector balance |
| `AccumulativeInsightPortfolioConstructionModel` | Accumulated insights |

### 6.7 Execution Model Implementations

| Model | Description |
|-------|-------------|
| `ImmediateExecutionModel` | Immediate market orders |
| `VolumeWeightedAveragePriceExecutionModel` | VWAP-based execution |
| `StandardDeviationExecutionModel` | Std dev band execution |
| `SpreadExecutionModel` | Spread-based execution |

### 6.8 Risk Management Implementations

| Model | Description |
|-------|-------------|
| `MaximumDrawdownPercentPortfolio` | Portfolio drawdown limit |
| `MaximumDrawdownPercentPerSecurity` | Per-security drawdown |
| `MaximumSectorExposureRiskManagementModel` | Sector concentration limit |
| `MaximumUnrealizedProfitPercentPerSecurity` | Profit taking |
| `TrailingStopRiskManagementModel` | Trailing stop loss |

### 6.9 Universe Selection Implementations

| Model | Description |
|-------|-------------|
| `FundamentalUniverseSelectionModel` | Fundamental data filter |
| `CoarseFundamentalUniverseSelectionModel` | Coarse data filter |
| `FineFundamentalUniverseSelectionModel` | Fine data filter |
| `ETFConstituentsUniverseSelectionModel` | ETF holdings |
| `OptionUniverseSelectionModel` | Option chains |
| `FutureUniverseSelectionModel` | Futures contracts |
| `ScheduledUniverseSelectionModel` | Time-based refresh |
| `QC500UniverseSelectionModel` | QuantConnect 500 |

---

## 7. Engine Components

### 7.1 Engine Architecture

```
Engine (Main Orchestrator)
│
├── SystemHandlers (LeanEngineSystemHandlers)
│   ├── Messaging (IMessagingHandler)
│   └── Api (IApi)
│
└── AlgorithmHandlers (LeanEngineAlgorithmHandlers)
    ├── Results (IResultHandler)
    ├── Setup (ISetupHandler)
    ├── DataFeed (IDataFeed)
    ├── Transactions (ITransactionHandler)
    ├── RealTime (IRealTimeHandler)
    ├── MapFileProvider
    ├── FactorFileProvider
    ├── DataProvider
    ├── DataCacheProvider
    ├── ObjectStore
    ├── DataPermissionsManager
    └── DataMonitor
```

### 7.2 Handler Interfaces

```
IDataFeed
├── IsActive
├── Initialize(algorithm, job, ...)
├── CreateSubscription(request)
├── RemoveSubscription(subscription)
└── Exit()

Implementations:
├── FileSystemDataFeed (backtesting)
└── LiveTradingDataFeed (live)

ITransactionHandler : IOrderProcessor, IOrderEventProvider
├── IsActive
├── Orders, OrderTickets
├── OrderEvents
├── Initialize(algorithm, brokerage, resultHandler)
├── Exit()
├── ProcessSynchronousEvents()
└── AddOpenOrder(order, algorithm)

Implementations:
├── BrokerageTransactionHandler (base)
└── BacktestingTransactionHandler

IResultHandler
├── IsActive
├── Messages
├── Initialize(parameters)
├── SetAlgorithm(algorithm, startingValue)
├── DebugMessage, LogMessage, ErrorMessage
├── Sample(time)
├── OrderEvent(event)
├── SendStatusUpdate(status, message)
└── Exit()

Implementations:
├── BacktestingResultHandler
└── LiveTradingResultHandler

IRealTimeHandler : IEventSchedule
├── IsActive
├── Setup(algorithm, job, ...)
├── SetTime(time)
├── ScanPastEvents(time)
├── Exit()
└── OnSecuritiesChanged(changes)

Implementations:
├── BacktestingRealTimeHandler
└── LiveTradingRealTimeHandler

ISetupHandler
├── WorkerThread
├── Errors
├── MaximumRuntime
├── StartingPortfolioValue
├── StartingDate
├── MaxOrders
├── CreateAlgorithmInstance(packet, assemblyPath)
├── CreateBrokerage(packet, algorithm)
└── Setup(parameters)

Implementations:
├── BacktestingSetupHandler
├── BrokerageSetupHandler
└── ConsoleSetupHandler

IHistoryProvider : IDataProviderEvents
├── DataPointCount
├── Initialize(parameters)
└── GetHistory(requests, timeZone) → IEnumerable<Slice>

Implementations:
├── SubscriptionDataReaderHistoryProvider
├── BrokerageHistoryProvider
└── HistoryProviderManager (composite)
```

### 7.3 Synchronizer and TimeSlice

```
ISynchronizer
└── StreamData(cancellationToken) → IEnumerable<TimeSlice>

Implementations:
├── Synchronizer (backtesting)
└── LiveSynchronizer (live trading)

TimeSlice
├── Time (DateTime UTC)
├── DataPointCount
├── Data (List<DataFeedPacket>)
├── Slice (algorithm's Slice input)
├── SecuritiesUpdateData
├── ConsolidatorUpdateData
├── CustomData
├── SecurityChanges
├── UniverseData
└── IsTimePulse
```

### 7.4 AlgorithmManager

The `AlgorithmManager` executes the main event loop:

```
AlgorithmManager.Run() Loop:
│
├── For each TimeSlice from Synchronizer
│   ├── Time limit check
│   ├── Cancellation check
│   ├── Portfolio validation
│   │
│   ├── Process scheduled events
│   ├── Update securities with new data
│   ├── Apply margin interest (hourly)
│   ├── Settlement scan (hourly)
│   │
│   ├── Process corporate actions
│   │   ├── Symbol changed events
│   │   ├── Splits
│   │   ├── Dividends
│   │   └── Delistings
│   │
│   ├── Update consolidators
│   ├── Fire custom data handlers
│   │
│   ├── Call algorithm events
│   │   ├── OnSplits, OnDividends, OnDelistings
│   │   ├── OnData(Slice)
│   │   ├── OnFrameworkData
│   │   └── ProcessSynchronousEvents
│   │
│   └── OnEndOfTimeStep
│
└── Post-loop
    ├── OnEndOfAlgorithm
    └── Final statistics
```

---

## 8. Brokerage Models

### 8.1 IBrokerageModel Interface

```
IBrokerageModel
├── AccountType (Margin/Cash)
├── RequiredFreeBuyingPowerPercent
├── DefaultMarkets
│
├── CanSubmitOrder(security, order, message)
├── CanUpdateOrder(security, order, request, message)
├── CanExecuteOrder(security, order)
├── ApplySplit(tickets, split)
│
├── GetLeverage(security)
├── GetBenchmark(algorithm)
│
├── GetFillModel(security)
├── GetFeeModel(security)
├── GetSlippageModel(security)
├── GetSettlementModel(security)
├── GetBuyingPowerModel(security)
├── GetMarginInterestRateModel(security)
└── GetShortableProvider(security)
```

### 8.2 Supported Brokerages (BrokerageName Enum)

| Category | Brokerages |
|----------|------------|
| **Default** | Default, TerminalLink |
| **US Equities** | InteractiveBrokers, TDAmeritrade, Alpaca, TradeStation, CharlesSchwab, Tastytrade, Axos |
| **Forex/CFD** | Oanda, FXCM |
| **Crypto Spot** | Coinbase, Binance, BinanceUS, Bitfinex, Kraken |
| **Crypto Futures** | BinanceFutures, BinanceCoinFutures, Bybit, dYdX |
| **India** | Zerodha, Samco |
| **Other** | Exante, Wolverine, RBI, Eze, AlphaStreams |

### 8.3 Fee Models

```
IFeeModel
└── GetOrderFee(parameters) → OrderFee

Implementations:
├── ConstantFeeModel(fee)
├── InteractiveBrokersFeeModel (complex tiered)
├── AlpacaFeeModel
├── TDAmeritradeFeeModel
├── CharlesSchwabFeeModel
├── TradeStationFeeModel
├── TastytradeFeeModel
├── BinanceFeeModel, BinanceFuturesFeeModel
├── BybitFeeModel, BybitFuturesFeeModel
├── CoinbaseFeeModel
├── BitfinexFeeModel
├── KrakenFeeModel
├── dYdXFeeModel
├── ZerodhaFeeModel
├── SamcoFeeModel
└── [30+ total implementations]
```

### 8.4 Fill Models

```
IFillModel
└── Fill(parameters) → Fill (collection of OrderEvents)

Base FillModel routes to:
├── MarketFill()
├── LimitFill()
├── StopMarketFill()
├── StopLimitFill()
├── LimitIfTouchedFill()
├── TrailingStopFill()
├── MarketOnOpenFill()
├── MarketOnCloseFill()
└── OptionExerciseFill()

Implementations:
├── ImmediateFillModel
├── EquityFillModel
├── FutureFillModel
├── FutureOptionFillModel
└── LatestPriceFillModel
```

### 8.5 Slippage Models

```
ISlippageModel
└── GetSlippageApproximation(asset, order) → decimal

Implementations:
├── NullSlippageModel (no slippage)
├── ConstantSlippageModel(percent)
├── VolumeShareSlippageModel
├── MarketImpactSlippageModel
└── AlphaStreamsSlippageModel
```

### 8.6 DefaultBrokerageModel Defaults

| Security Type | Default Leverage | Default Fee Model | Default Fill Model |
|---------------|------------------|-------------------|-------------------|
| Equity | 2x | InteractiveBrokersFeeModel | EquityFillModel |
| Option | 1x | InteractiveBrokersFeeModel | ImmediateFillModel |
| Future | 1x | InteractiveBrokersFeeModel | FutureFillModel |
| Forex | 50x | ConstantFeeModel(0) | ImmediateFillModel |
| Crypto | 1x | ConstantFeeModel(0) | ImmediateFillModel |
| Crypto Futures | 25x | ConstantFeeModel(0) | ImmediateFillModel |
| CFD | 50x | ConstantFeeModel(0) | ImmediateFillModel |

---

## 9. Object Relationship Diagrams

### 9.1 Algorithm ↔ Core Managers

```
                    ┌─────────────────────────────────────┐
                    │           QCAlgorithm               │
                    │         (IAlgorithm)                │
                    └──────────────┬──────────────────────┘
                                   │
       ┌───────────────────────────┼───────────────────────────┐
       │                           │                           │
       ▼                           ▼                           ▼
┌──────────────────┐    ┌────────────────────┐    ┌──────────────────┐
│ SecurityManager  │    │ SecurityTransaction│    │SecurityPortfolio │
│                  │    │     Manager        │    │    Manager       │
│ Dict<Symbol,     │    │                    │    │                  │
│      Security>   │    │ Orders, Tickets    │    │ Holdings, Cash   │
└────────┬─────────┘    │ P&L tracking       │    │ Margin calls     │
         │              └─────────┬──────────┘    └────────┬─────────┘
         │                        │                        │
         └────────────────────────┼────────────────────────┘
                                  │
                                  ▼
                         ┌───────────────┐
                         │   Security    │
                         │               │
                         │ • Holdings    │
                         │ • Cache       │
                         │ • Exchange    │
                         │ • Models      │
                         └───────────────┘
```

### 9.2 Order Processing Flow

```
Algorithm Order Request
         │
         ▼
┌─────────────────────┐
│  SubmitOrderRequest │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐     ┌─────────────────────┐
│    OrderTicket      │────▶│       Order         │
│                     │     │                     │
│ • Status tracking   │     │ • Type, Quantity    │
│ • Fill info         │     │ • Prices            │
│ • Events list       │     │ • Status            │
└──────────┬──────────┘     └─────────────────────┘
           │
           ▼
┌─────────────────────┐
│  TransactionHandler │
│                     │
│ BuyingPower check   │
│       ↓             │
│ Brokerage submit    │
│       ↓             │
│ FillModel execution │
│       ↓             │
│ FeeModel applied    │
│       ↓             │
│ SettlementModel     │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│     OrderEvent      │
│                     │
│ • FillPrice         │
│ • FillQuantity      │
│ • OrderFee          │
│ • Status            │
└─────────────────────┘
```

### 9.3 Data Flow

```
Data Sources
     │
     ▼
┌─────────────────────┐
│SubscriptionData     │
│Config               │
│                     │
│ • Symbol, Type      │
│ • Resolution        │
│ • FillForward       │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│     IDataFeed       │
│                     │
│ CreateSubscription  │
│ RemoveSubscription  │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│   Synchronizer      │
│                     │
│ Merges streams      │
│ Time-ordered        │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│     TimeSlice       │
│                     │
│ • Slice (for algo)  │
│ • SecurityChanges   │
│ • UniverseData      │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│       Slice         │
│                     │
│ • Bars (TradeBars)  │
│ • QuoteBars         │
│ • Ticks             │
│ • OptionChains      │
│ • FuturesChains     │
│ • Splits, Dividends │
│ • Delistings        │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│Algorithm.OnData()   │
└─────────────────────┘
```

### 9.4 Framework Model Flow

```
┌───────────────────────────────────────────────────────────────────┐
│                         QCAlgorithm                               │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                  Framework Models                            │ │
│  │                                                              │ │
│  │  ┌──────────────────┐                                       │ │
│  │  │UniverseSelection │                                       │ │
│  │  │Model             │                                       │ │
│  │  └────────┬─────────┘                                       │ │
│  │           │ CreateUniverses()                               │ │
│  │           ▼                                                  │ │
│  │  ┌──────────────────┐                                       │ │
│  │  │   AlphaModel     │──── Update(slice) ────────────────┐   │ │
│  │  └──────────────────┘                                   │   │ │
│  │                                                         ▼   │ │
│  │                                              ┌──────────────┐│ │
│  │                                              │   Insight    ││ │
│  │                                              │              ││ │
│  │                                              │ • Symbol     ││ │
│  │                                              │ • Direction  ││ │
│  │                                              │ • Period     ││ │
│  │                                              │ • Confidence ││ │
│  │  ┌──────────────────┐                        └──────┬───────┘│ │
│  │  │PortfolioConstr.  │◀── CreateTargets(insights) ──┘        │ │
│  │  │Model             │                                       │ │
│  │  └────────┬─────────┘                                       │ │
│  │           │                                                  │ │
│  │           ▼                                                  │ │
│  │  ┌──────────────────┐                                       │ │
│  │  │PortfolioTarget   │                                       │ │
│  │  │                  │                                       │ │
│  │  │ • Symbol         │                                       │ │
│  │  │ • Quantity       │                                       │ │
│  │  └────────┬─────────┘                                       │ │
│  │           │                                                  │ │
│  │           ▼                                                  │ │
│  │  ┌──────────────────┐                                       │ │
│  │  │RiskManagement    │──── ManageRisk(targets) ──────────┐   │ │
│  │  │Model             │                                   │   │ │
│  │  └──────────────────┘                                   │   │ │
│  │                                                         ▼   │ │
│  │  ┌──────────────────┐        (modified targets)             │ │
│  │  │ExecutionModel    │◀──────────────────────────────────┘   │ │
│  │  │                  │                                       │ │
│  │  │ Execute(targets) │──────▶ Submit Orders                  │ │
│  │  └──────────────────┘                                       │ │
│  │                                                              │ │
│  └─────────────────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────────────────┘
```

### 9.5 Brokerage Model Integration

```
┌─────────────────────────────────────────────────────────────────┐
│                      IBrokerageModel                            │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                     Security                             │   │
│  │                                                          │   │
│  │  Configured with models from brokerage:                  │   │
│  │                                                          │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌──────────────┐ │   │
│  │  │   FeeModel    │  │   FillModel   │  │SlippageModel │ │   │
│  │  │               │  │               │  │              │ │   │
│  │  │ Calculates    │  │ Simulates     │  │ Price impact │ │   │
│  │  │ commissions   │  │ order fills   │  │              │ │   │
│  │  └───────────────┘  └───────────────┘  └──────────────┘ │   │
│  │                                                          │   │
│  │  ┌───────────────┐  ┌───────────────┐  ┌──────────────┐ │   │
│  │  │BuyingPower    │  │ Settlement    │  │MarginInterest│ │   │
│  │  │Model          │  │ Model         │  │Model         │ │   │
│  │  │               │  │               │  │              │ │   │
│  │  │ Leverage,     │  │ Cash timing   │  │ Interest on  │ │   │
│  │  │ Margin        │  │ (T+N)         │  │ borrowing    │ │   │
│  │  └───────────────┘  └───────────────┘  └──────────────┘ │   │
│  │                                                          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  Order Validation:                                              │
│  ├── CanSubmitOrder(security, order)                           │
│  ├── CanUpdateOrder(security, order, request)                  │
│  └── CanExecuteOrder(security, order)                          │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Appendix: Key File Locations

| Component | Primary Location |
|-----------|------------------|
| QCAlgorithm | `Algorithm/QCAlgorithm*.cs` (9 partial files) |
| Security classes | `Common/Securities/` |
| Order classes | `Common/Orders/` |
| Data classes | `Common/Data/` |
| Framework interfaces | `Algorithm/[Alphas\|Execution\|Portfolio\|Risk\|Selection]/` |
| Framework implementations | `Algorithm.Framework/` |
| Brokerage models | `Common/Brokerages/` |
| Engine | `Engine/` |
| Indicators | `Indicators/` |

---

*Document generated for QuantConnect Lean Engine - .NET 10.0*
