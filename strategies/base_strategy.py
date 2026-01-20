"""
Base strategy interface for rust-trade Python strategies.

All trading strategies must inherit from BaseStrategy and implement
the required methods.

The unified interface uses on_bar_data() with three operational modes:
- OnEachTick: Called for every tick with accumulated OHLC state
- OnPriceMove: Called only when price changes
- OnCloseBar: Called only when bar closes (most efficient for OHLC strategies)
"""

from abc import ABC, abstractmethod
from typing import Dict, Optional, Any


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
