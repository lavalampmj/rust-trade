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
"""

from abc import ABC, abstractmethod
from collections import deque
from decimal import Decimal
from typing import Dict, Optional, Any, Generic, TypeVar, Iterator, List, Union

# Type variable for Series values
T = TypeVar('T')


class Series(Generic[T]):
    """
    Reverse-indexed time series data structure.

    Provides NinjaTrader-style access where:
    - series[0] returns the most recent value
    - series[1] returns the previous value
    - series[n] returns the value n bars ago

    Example:
        >>> series = Series("close")
        >>> series.push(Decimal("100"))
        >>> series.push(Decimal("101"))
        >>> series.push(Decimal("102"))
        >>> series[0]  # Most recent: 102
        >>> series[1]  # Previous: 101
        >>> series[2]  # Two bars ago: 100
    """

    def __init__(self, name: str, max_lookback: int = 256):
        """
        Create a new series.

        Args:
            name: Series name for identification
            max_lookback: Maximum number of values to keep (FIFO eviction).
                         Use 0 for infinite lookback.
        """
        self._name = name
        self._max_lookback = max_lookback
        self._data: deque = deque(maxlen=max_lookback if max_lookback > 0 else None)
        self._current_bar_index = 0

    @property
    def name(self) -> str:
        """Get series name."""
        return self._name

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
        """Reset the series, clearing all data."""
        self._data.clear()
        self._current_bar_index = 0

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
    - on_bar_data(bar_data): Process bar data and return signal
    - initialize(params): Initialize with parameters

    Optional methods:
    - bar_data_mode(): Return operational mode (OnEachTick/OnPriceMove/OnCloseBar)
    - preferred_bar_type(): Return preferred bar type (TimeBased/TickBased)
    - reset(): Reset strategy state
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
    def on_bar_data(self, bar_data: Dict[str, Any]) -> Dict[str, Any]:
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

        Returns:
            Signal dictionary (use Signal.buy/sell/hold methods)

        Example:
            >>> def on_bar_data(self, bar_data):
            ...     close = Decimal(bar_data["close"])
            ...     symbol = bar_data["symbol"]
            ...     if self.should_buy(close):
            ...         return Signal.buy(symbol, "100")
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
