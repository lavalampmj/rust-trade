"""
Base strategy interface for rust-trade Python strategies.

All trading strategies must inherit from BaseStrategy and implement
the required methods.

The unified interface uses on_bar_data() with three operational modes:
- OnEachTick: Called for every tick with accumulated OHLC state
- OnPriceMove: Called only when price changes
- OnCloseBar: Called only when bar closes (most efficient for OHLC strategies)

This module also provides Series<T> abstraction for NinjaTrader-style
reverse-indexed access to time series data:
- series[0] = most recent value
- series[1] = previous value
- series.sma(period) = simple moving average

Component State Management:
- ComponentState class provides state constants for lifecycle management
- on_state_change() callback notifies strategies of state transitions
"""

from abc import ABC, abstractmethod
from collections import deque
from decimal import Decimal
from typing import Dict, Optional, Any, Generic, TypeVar, Iterator, List, Union, TYPE_CHECKING
from enum import IntEnum

if TYPE_CHECKING:
    from .bars_context import BarsContext

# Type variable for Series values
T = TypeVar('T')


class ComponentState(IntEnum):
    """
    Unified component lifecycle states (NinjaTrader pattern).

    Applies to ALL component types: strategies, indicators, data sources, caches, services.

    State Flow (Data-processing components):
        Undefined -> SetDefaults -> Configure -> DataLoaded -> Historical -> Transition -> Realtime -> Terminated -> Finalized
                                                                                                    |
                                                                                                 Faulted

    State Flow (Non-data components like services/caches):
        Undefined -> SetDefaults -> Configure -> Active -> Terminated -> Finalized

    Usage in on_state_change:
        >>> def on_state_change(self, event):
        ...     if event['new_state_int'] == ComponentState.SET_DEFAULTS:
        ...         self.period = 20  # Set default parameters
        ...     elif event['new_state_int'] == ComponentState.DATA_LOADED:
        ...         # Initialize child indicators
        ...         pass
        ...     elif event['new_state_int'] == ComponentState.REALTIME:
        ...         print("Going live!")
    """
    UNDEFINED = 0       # Component has not been initialized
    SET_DEFAULTS = 1    # Setting default property values (keep lean)
    CONFIGURE = 2       # Adding data series, configuring dependencies
    ACTIVE = 3          # For non-data components (services, adapters) - equivalent to "running"
    DATA_LOADED = 4     # All data series loaded - instantiate child indicators here
    HISTORICAL = 5      # Processing historical data (backtest warmup period)
    TRANSITION = 6      # Switching from historical to realtime processing
    REALTIME = 7        # Processing live/realtime data
    TERMINATED = 8      # Normal shutdown initiated - cleanup resources here
    FAULTED = 9         # Fatal error occurred
    FINALIZED = 10      # Internal cleanup complete (terminal state)

    def is_running(self) -> bool:
        """Check if component is in a running state (Historical, Realtime, or Active)."""
        return self in (ComponentState.HISTORICAL, ComponentState.REALTIME, ComponentState.ACTIVE)

    def is_terminal(self) -> bool:
        """Check if component is in a terminal state."""
        return self in (ComponentState.TERMINATED, ComponentState.FAULTED, ComponentState.FINALIZED)

    def can_process_data(self) -> bool:
        """Check if component can accept new data."""
        return self in (ComponentState.HISTORICAL, ComponentState.TRANSITION, ComponentState.REALTIME)

    def is_realtime(self) -> bool:
        """Check if component is processing live data."""
        return self == ComponentState.REALTIME

    def is_initializing(self) -> bool:
        """Check if component is still initializing."""
        return self in (ComponentState.UNDEFINED, ComponentState.SET_DEFAULTS,
                        ComponentState.CONFIGURE, ComponentState.DATA_LOADED)

    @classmethod
    def from_string(cls, state_str: str) -> 'ComponentState':
        """
        Convert state string to ComponentState enum.

        Args:
            state_str: State string like "REALTIME", "HISTORICAL", "SET_DEFAULTS"

        Returns:
            ComponentState enum value

        Raises:
            ValueError if unknown state
        """
        mapping = {
            'UNDEFINED': cls.UNDEFINED,
            'SET_DEFAULTS': cls.SET_DEFAULTS,
            'CONFIGURE': cls.CONFIGURE,
            'ACTIVE': cls.ACTIVE,
            'DATA_LOADED': cls.DATA_LOADED,
            'HISTORICAL': cls.HISTORICAL,
            'TRANSITION': cls.TRANSITION,
            'REALTIME': cls.REALTIME,
            'TERMINATED': cls.TERMINATED,
            'FAULTED': cls.FAULTED,
            'FINALIZED': cls.FINALIZED,
        }
        if state_str not in mapping:
            raise ValueError(f"Unknown state: {state_str}")
        return mapping[state_str]


class Series(Generic[T]):
    """
    Reverse-indexed time series data structure.

    Provides NinjaTrader-style access where:
    - series[0] returns the most recent value
    - series[1] returns the previous value
    - series[n] returns the value n bars ago

    Warmup System:
    - Each series tracks a `warmup_period` indicating samples needed before ready
    - Use `is_ready` property to check if enough samples have been collected
    - Allows indicators to propagate their ready state up the call chain

    Example:
        >>> series = Series("close", warmup_period=20)
        >>> series.push(Decimal("100"))
        >>> series.is_ready  # False - not enough samples
        >>> # ... push 19 more values ...
        >>> series.is_ready  # True - now ready
        >>> series[0]  # Most recent
        >>> series[1]  # Previous
    """

    def __init__(self, name: str, max_lookback: int = 256, warmup_period: int = 1):
        """
        Create a new series.

        Args:
            name: Series name for identification
            max_lookback: Maximum number of values to keep (FIFO eviction).
                         Use 0 for infinite lookback.
            warmup_period: Number of samples needed before is_ready returns True.
                          Default is 1 (ready after first sample).
        """
        self._name = name
        self._max_lookback = max_lookback
        self._data: deque = deque(maxlen=max_lookback if max_lookback > 0 else None)
        self._current_bar_index = 0
        self._warmup_period = warmup_period

    @property
    def name(self) -> str:
        """Get series name."""
        return self._name

    @property
    def is_ready(self) -> bool:
        """
        Check if series has enough samples to be ready.

        Returns True when count() >= warmup_period.
        """
        return self.count() >= self._warmup_period

    @property
    def warmup_period(self) -> int:
        """Get the warmup period (samples needed before is_ready returns True)."""
        return self._warmup_period

    def set_warmup_period(self, period: int) -> None:
        """
        Set the warmup period (for dynamic indicators).

        Args:
            period: New warmup period
        """
        self._warmup_period = period

    def push(self, value: T) -> None:
        """
        Push a new value to the series.

        This should be called once per bar to maintain alignment.
        Old values are automatically evicted based on max_lookback.

        Args:
            value: Value to add
        """
        self._data.append(value)
        self._current_bar_index += 1

    def get(self, bars_ago: int) -> Optional[T]:
        """
        Get value at bars_ago index (0 = most recent).

        Args:
            bars_ago: Number of bars back (0 = current, 1 = previous, etc.)

        Returns:
            Value or None if bars_ago exceeds available data
        """
        if not self._data or bars_ago < 0:
            return None

        # Convert reverse index to forward index
        forward_index = len(self._data) - 1 - bars_ago
        if forward_index < 0:
            return None

        return self._data[forward_index]

    def __getitem__(self, bars_ago: int) -> T:
        """
        Get value using series[n] syntax.

        Raises IndexError if bars_ago exceeds available data.

        Args:
            bars_ago: Number of bars back

        Returns:
            Value at bars_ago
        """
        value = self.get(bars_ago)
        if value is None:
            raise IndexError(
                f"Series '{self._name}' index out of bounds: bars_ago={bars_ago} "
                f"but only {len(self._data)} bars available"
            )
        return value

    def has_data(self, bars_ago: int) -> bool:
        """Check if data is available at bars_ago index."""
        return self.get(bars_ago) is not None

    def count(self) -> int:
        """Get the number of available data points."""
        return len(self._data)

    def is_empty(self) -> bool:
        """Check if series is empty."""
        return len(self._data) == 0

    def current_bar(self) -> int:
        """Get the current bar index."""
        return self._current_bar_index

    def reset(self) -> None:
        """Reset the series, clearing all data but preserving warmup_period."""
        self._data.clear()
        self._current_bar_index = 0
        # Note: warmup_period is preserved through reset

    def current(self) -> Optional[T]:
        """Get most recent value (bars_ago = 0)."""
        return self.get(0)

    def previous(self) -> Optional[T]:
        """Get previous value (bars_ago = 1)."""
        return self.get(1)

    def take(self, n: int) -> List[T]:
        """
        Take the N most recent values (newest first).

        Args:
            n: Number of values to take

        Returns:
            List of values from newest to oldest
        """
        result = []
        for i in range(min(n, len(self._data))):
            val = self.get(i)
            if val is not None:
                result.append(val)
        return result

    def __iter__(self) -> Iterator[T]:
        """Iterate over values from oldest to newest."""
        return iter(self._data)

    def iter_reverse(self) -> Iterator[T]:
        """Iterate over values from newest to oldest."""
        return reversed(self._data)

    def __len__(self) -> int:
        """Return number of values in series."""
        return len(self._data)


class DecimalSeries(Series[Decimal]):
    """
    Series specialized for Decimal values with indicator helpers.

    Provides common trading calculations like SMA, highest, lowest.
    """

    def sma(self, period: int) -> Optional[Decimal]:
        """
        Calculate Simple Moving Average over last N values.

        Args:
            period: Number of values to average

        Returns:
            SMA value or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        total = sum(self.get(i) for i in range(period))
        return total / Decimal(period)

    def highest(self, period: int) -> Optional[Decimal]:
        """
        Get highest value over last N bars.

        Args:
            period: Number of bars to check

        Returns:
            Highest value or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        values = [self.get(i) for i in range(period)]
        return max(values)

    def lowest(self, period: int) -> Optional[Decimal]:
        """
        Get lowest value over last N bars.

        Args:
            period: Number of bars to check

        Returns:
            Lowest value or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        values = [self.get(i) for i in range(period)]
        return min(values)

    def sum(self, period: int) -> Optional[Decimal]:
        """
        Calculate sum of last N values.

        Args:
            period: Number of values to sum

        Returns:
            Sum or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        return sum(self.get(i) for i in range(period))


class FloatSeries(Series[float]):
    """
    Series specialized for float values with indicator helpers.

    Provides common trading calculations like SMA, highest, lowest.
    """

    def sma(self, period: int) -> Optional[float]:
        """
        Calculate Simple Moving Average over last N values.

        Args:
            period: Number of values to average

        Returns:
            SMA value or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        total = sum(self.get(i) for i in range(period))
        return total / period

    def highest(self, period: int) -> Optional[float]:
        """
        Get highest value over last N bars.

        Args:
            period: Number of bars to check

        Returns:
            Highest value or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        values = [self.get(i) for i in range(period)]
        return max(values)

    def lowest(self, period: int) -> Optional[float]:
        """
        Get lowest value over last N bars.

        Args:
            period: Number of bars to check

        Returns:
            Lowest value or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        values = [self.get(i) for i in range(period)]
        return min(values)

    def sum(self, period: int) -> Optional[float]:
        """
        Calculate sum of last N values.

        Args:
            period: Number of values to sum

        Returns:
            Sum or None if insufficient data
        """
        if self.count() < period or period <= 0:
            return None

        return sum(self.get(i) for i in range(period))


class Signal:
    """Helper class for creating trading signals."""

    @staticmethod
    def buy(symbol: str, quantity: str) -> Dict[str, Any]:
        """
        Create a buy signal.

        Args:
            symbol: Trading pair (e.g., "BTCUSDT")
            quantity: Amount to buy as decimal string (e.g., "100")

        Returns:
            Dictionary representing a buy signal
        """
        return {
            "type": "Buy",
            "symbol": symbol,
            "quantity": quantity
        }

    @staticmethod
    def sell(symbol: str, quantity: str) -> Dict[str, Any]:
        """
        Create a sell signal.

        Args:
            symbol: Trading pair (e.g., "BTCUSDT")
            quantity: Amount to sell as decimal string (e.g., "100")

        Returns:
            Dictionary representing a sell signal
        """
        return {
            "type": "Sell",
            "symbol": symbol,
            "quantity": quantity
        }

    @staticmethod
    def hold() -> Dict[str, Any]:
        """
        Create a hold signal (no action).

        Returns:
            Dictionary representing a hold signal
        """
        return {"type": "Hold"}


class BaseStrategy(ABC):
    """
    Abstract base class for all trading strategies.

    All strategies must implement:
    - name(): Return strategy name
    - on_bar_data(bar_data, bars): Process bar data and return signal
    - initialize(params): Initialize with parameters
    - is_ready(bars): Check if strategy has enough data for valid signals (REQUIRED)
    - warmup_period(): Return minimum bars needed before is_ready can return true (REQUIRED)

    Optional methods:
    - bar_data_mode(): Return operational mode (OnEachTick/OnPriceMove/OnCloseBar)
    - preferred_bar_type(): Return preferred bar type (TimeBased/TickBased)
    - reset(): Reset strategy state
    - max_bars_lookback(): Return maximum bars to keep in BarsContext
    """

    @abstractmethod
    def name(self) -> str:
        """
        Return the strategy name.

        Returns:
            Strategy name as string
        """
        pass

    @abstractmethod
    def on_bar_data(self, bar_data: Dict[str, Any], bars: 'BarsContext') -> Dict[str, Any]:
        """
        Process bar data and generate trading signal.

        This is the unified entry point for all strategy processing.
        The bar_data structure contains OHLC data and optionally tick data
        depending on the bar_data_mode().

        Args:
            bar_data: Dictionary with keys:
                - timestamp (str): ISO 8601 timestamp of the bar
                - symbol (str): Trading pair (e.g., "BTCUSDT")
                - open (str): Decimal string of opening price
                - high (str): Decimal string of high price
                - low (str): Decimal string of low price
                - close (str): Decimal string of closing price
                - volume (str): Decimal string of volume
                - trade_count (int): Number of trades in period
                - is_first_tick_of_bar (bool): True if this is first tick of bar
                - is_bar_closed (bool): True if this bar is complete
                - is_synthetic (bool): True if bar was generated with no ticks
                - tick_count_in_bar (int): Number of ticks accumulated in bar
                - current_tick (Optional[Dict]): Current tick data if available
                    - timestamp (str): Tick timestamp
                    - price (str): Tick price
                    - quantity (str): Tick quantity
                    - side (str): "Buy" or "Sell"
                    - trade_id (str): Trade identifier
            bars: BarsContext with OHLCV series and helpers
                - bars.close[0]: Current close price
                - bars.close[1]: Previous close price
                - bars.sma(20): 20-period SMA of close
                - bars.highest_high(14): Highest high over 14 bars
                - See BarsContext for full API

        Returns:
            Signal dictionary (use Signal.buy/sell/hold methods)

        Example:
            >>> def on_bar_data(self, bar_data, bars):
            ...     # Use BarsContext for easy access to price series
            ...     sma = bars.sma(20)
            ...     if sma is not None and bars.close[0] > sma:
            ...         return Signal.buy(bar_data["symbol"], "100")
            ...     return Signal.hold()
        """
        pass

    @abstractmethod
    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """
        Initialize strategy with parameters.

        Called once before strategy execution begins. Use this to
        set up internal state based on configuration parameters.

        Args:
            params: Dictionary of string parameters from config

        Returns:
            None if successful, error message string if failed

        Example:
            >>> def initialize(self, params):
            ...     try:
            ...         self.period = int(params.get("period", "14"))
            ...         return None
            ...     except Exception as e:
            ...         return str(e)
        """
        pass

    # ========================================================================
    # Warmup / Ready State
    # ========================================================================

    @abstractmethod
    def is_ready(self, bars: 'BarsContext') -> bool:
        """
        Check if strategy has enough data to generate valid signals.

        **REQUIRED** - must implement to combine your indicators' ready states.

        Args:
            bars: BarsContext to check for data availability

        Returns:
            True if all indicators are warmed up and strategy can generate
            valid signals

        Example:
            >>> def is_ready(self, bars):
            ...     # Ready when we have enough data for our longest indicator
            ...     return bars.is_ready_for(self.long_period)
        """
        pass

    @abstractmethod
    def warmup_period(self) -> int:
        """
        Return minimum bars needed before is_ready() can return true.

        **REQUIRED** - must match the logic in is_ready().
        Used by the framework for progress indication and optimization.

        Returns:
            Number of warmup bars needed

        Example:
            >>> def warmup_period(self):
            ...     return self.long_period  # Longest indicator period
        """
        pass

    def bar_data_mode(self) -> str:
        """
        Return the operational mode for this strategy.

        Modes:
        - "OnEachTick": Strategy called for every tick with accumulated OHLC
        - "OnPriceMove": Strategy called only when price changes
        - "OnCloseBar": Strategy called only when bar closes (default)

        Returns:
            One of: "OnEachTick", "OnPriceMove", "OnCloseBar"
            Default is "OnCloseBar" (most efficient for OHLC strategies)
        """
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """
        Return preferred bar type for this strategy.

        Bar types:
        - TimeBased: Bars close after fixed time (1m, 5m, 1h, etc.)
        - TickBased: Bars close after N ticks (e.g., 100-tick bars)

        Returns:
            Dictionary with format:
            - For TimeBased: {"type": "TimeBased", "timeframe": "1m"}
            - For TickBased: {"type": "TickBased", "tick_count": 100}

        Default is 1-minute time-based bars.
        """
        return {"type": "TimeBased", "timeframe": "1m"}

    def reset(self):
        """
        Reset strategy state for new backtest.

        Called before each backtest run. Use this to clear any
        internal state (price history, indicators, etc.).

        Default implementation does nothing.
        """
        pass

    def max_bars_lookback(self) -> int:
        """
        Return maximum bars lookback for BarsContext.

        Override to customize how much history BarsContext maintains.
        Default is 256 bars.

        Returns:
            Maximum number of bars to keep
        """
        return 256

    # ========================================================================
    # Order Event Handlers (Optional - for advanced order management)
    # ========================================================================

    def on_order_filled(self, event: Dict[str, Any]) -> None:
        """
        Called when an order is filled (partial or complete).

        Override this method to react to order fills. This is useful for:
        - Updating internal state based on executed orders
        - Placing follow-up orders (e.g., stop-loss after entry)
        - Tracking realized P&L

        Args:
            event: Order filled event dictionary with keys:
                - client_order_id (str): Client-assigned order ID
                - venue_order_id (str): Exchange-assigned order ID
                - symbol (str): Trading pair
                - order_side (str): "Buy" or "Sell"
                - last_qty (str): Quantity filled in this event
                - last_px (str): Price of this fill
                - cum_qty (str): Cumulative quantity filled
                - leaves_qty (str): Remaining quantity
                - commission (str): Commission for this fill
                - timestamp (str): ISO 8601 timestamp

        Example:
            def on_order_filled(self, event):
                # Place a stop-loss after a buy is filled
                if event["order_side"] == "Buy":
                    stop_price = Decimal(event["last_px"]) * Decimal("0.95")
                    self.pending_stop = stop_price
        """
        pass  # Default: no-op

    def on_order_rejected(self, event: Dict[str, Any]) -> None:
        """
        Called when an order is rejected by the venue.

        Override this method to handle order rejections. This is useful for:
        - Logging rejection reasons
        - Adjusting order parameters and retrying
        - Updating strategy state

        Args:
            event: Order rejected event dictionary with keys:
                - client_order_id (str): Client-assigned order ID
                - reason (str): Rejection reason
                - timestamp (str): ISO 8601 timestamp
        """
        pass  # Default: no-op

    def on_order_canceled(self, event: Dict[str, Any]) -> None:
        """
        Called when an order is canceled.

        Override this method to handle order cancellations. This is useful for:
        - Tracking canceled orders
        - Placing replacement orders
        - Updating order tracking state

        Args:
            event: Order canceled event dictionary with keys:
                - client_order_id (str): Client-assigned order ID
                - venue_order_id (str, optional): Exchange-assigned order ID
                - timestamp (str): ISO 8601 timestamp
        """
        pass  # Default: no-op

    def on_order_submitted(self, order: Dict[str, Any]) -> None:
        """
        Called when a new order is submitted.

        Override this method to track submitted orders.

        Args:
            order: Order dictionary with keys:
                - client_order_id (str): Client-assigned order ID
                - symbol (str): Trading pair
                - order_side (str): "Buy" or "Sell"
                - order_type (str): "Market", "Limit", "StopMarket", etc.
                - quantity (str): Order quantity
                - price (str, optional): Limit price if applicable
                - stop_price (str, optional): Stop price if applicable
                - time_in_force (str): "GTC", "IOC", "FOK", "DAY"
        """
        pass  # Default: no-op

    def uses_order_management(self) -> bool:
        """
        Whether this strategy uses advanced order management.

        When True, the execution engine will:
        - Call order event handlers (on_order_filled, etc.)
        - Not auto-convert signals to market orders (strategy manages orders directly)

        Default is False for backward compatibility with signal-based strategies.

        Returns:
            True if strategy manages its own orders, False for signal-based execution
        """
        return False

    def get_orders(self, bar_data: Dict[str, Any], bars: 'BarsContext') -> List[Dict[str, Any]]:
        """
        Get orders to submit for this bar.

        Advanced strategies can override this to submit complex orders
        (limit orders, bracket orders, etc.) instead of using signals.

        Only called if `uses_order_management()` returns True.

        Args:
            bar_data: Current bar data dictionary
            bars: BarsContext with OHLCV series

        Returns:
            List of order dictionaries, each with keys:
                - client_order_id (str): Unique client-assigned ID
                - symbol (str): Trading pair
                - order_side (str): "Buy" or "Sell"
                - order_type (str): "Market", "Limit", "StopMarket", "StopLimit"
                - quantity (str): Order quantity as decimal string
                - price (str, optional): Limit price for Limit/StopLimit orders
                - stop_price (str, optional): Stop price for Stop orders
                - time_in_force (str): "GTC", "IOC", "FOK", "DAY" (default: "GTC")

        Example:
            def get_orders(self, bar_data, bars):
                orders = []
                if self.should_enter_long(bars):
                    entry_price = bars.close[0] * Decimal("0.99")
                    orders.append({
                        "client_order_id": f"entry_{bars.current_bar()}",
                        "symbol": bar_data["symbol"],
                        "order_side": "Buy",
                        "order_type": "Limit",
                        "quantity": "0.1",
                        "price": str(entry_price),
                        "time_in_force": "GTC"
                    })
                return orders
        """
        return []  # Default: no orders

    def get_cancellations(self, bar_data: Dict[str, Any], bars: 'BarsContext') -> List[str]:
        """
        Get orders to cancel for this bar.

        Advanced strategies can override this to cancel pending orders
        based on market conditions.

        Only called if `uses_order_management()` returns True.

        Args:
            bar_data: Current bar data dictionary
            bars: BarsContext with OHLCV series

        Returns:
            List of client_order_id strings to cancel

        Example:
            def get_cancellations(self, bar_data, bars):
                # Cancel stale orders if price moved significantly
                if abs(bars.percent_change() or 0) > 2:
                    return list(self.pending_order_ids)
                return []
        """
        return []  # Default: no cancellations

    # ========================================================================
    # State Change Handler (Optional - for lifecycle management)
    # ========================================================================

    def on_state_change(self, event: Dict[str, Any]) -> None:
        """
        Called when the strategy's lifecycle state changes.

        Override this method to perform state-specific initialization:
        - SET_DEFAULTS: Set default parameter values (keep lean)
        - CONFIGURE: Configure dependencies, validate parameters
        - DATA_LOADED: Initialize child indicators (series, custom indicators)
        - HISTORICAL: Entering backtest warmup phase
        - REALTIME: Going live - ready for real-time data
        - TERMINATED: Cleanup resources, close connections

        Args:
            event: State change event dictionary with keys:
                - strategy_id (str): Strategy identifier
                - old_state (str): Previous state name (e.g., "HISTORICAL")
                - new_state (str): New state name (e.g., "REALTIME")
                - old_state_int (int): Previous state as integer (use with ComponentState)
                - new_state_int (int): New state as integer (use with ComponentState)
                - reason (str, optional): Reason for state change
                - timestamp (str): ISO 8601 timestamp
                - is_going_live (bool): True if transitioning to REALTIME
                - is_terminal (bool): True if new state is terminal
                - is_fault (bool): True if new state is FAULTED
                - can_process_data (bool): True if can accept market data
                - is_running (bool): True if in a running state

        Example:
            >>> def on_state_change(self, event):
            ...     state = ComponentState(event['new_state_int'])
            ...
            ...     if state == ComponentState.SET_DEFAULTS:
            ...         # Set default parameters
            ...         self.period = 20
            ...
            ...     elif state == ComponentState.DATA_LOADED:
            ...         # Initialize indicators
            ...         self.sma_short = DecimalSeries("sma_short", warmup_period=self.short_period)
            ...         self.sma_long = DecimalSeries("sma_long", warmup_period=self.long_period)
            ...
            ...     elif state == ComponentState.REALTIME:
            ...         print(f"Strategy {event['strategy_id']} is now live!")
            ...
            ...     elif state == ComponentState.TERMINATED:
            ...         # Cleanup
            ...         self.cleanup_resources()
            ...
            ...     elif state == ComponentState.FAULTED:
            ...         reason = event.get('reason', 'Unknown')
            ...         print(f"Strategy faulted: {reason}")
        """
        pass  # Default: no-op
