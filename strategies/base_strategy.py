"""
Base strategy interface for rust-trade Python strategies.

All trading strategies must inherit from BaseStrategy and implement
the required methods.
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
    - on_tick(tick_data): Process tick data and return signal
    - initialize(params): Initialize with parameters

    Optional methods:
    - on_ohlc(ohlc_data): Process OHLC data (if supported)
    - supports_ohlc(): Return True if strategy supports OHLC
    - preferred_timeframe(): Return preferred timeframe string
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
    def on_tick(self, tick: Dict[str, Any]) -> Dict[str, Any]:
        """
        Process tick data and generate trading signal.

        This method is called for each new tick in the market data.

        Args:
            tick: Dictionary with keys:
                - timestamp (str): ISO 8601 timestamp
                - symbol (str): Trading pair (e.g., "BTCUSDT")
                - price (str): Decimal string of trade price
                - quantity (str): Decimal string of trade quantity
                - side (str): "Buy" or "Sell"
                - trade_id (str): Unique trade identifier
                - is_buyer_maker (bool): Whether buyer is maker

        Returns:
            Signal dictionary (use Signal.buy/sell/hold methods)

        Example:
            >>> def on_tick(self, tick):
            ...     if self.should_buy(tick):
            ...         return Signal.buy(tick["symbol"], "100")
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

    def on_ohlc(self, ohlc: Dict[str, Any]) -> Dict[str, Any]:
        """
        Process OHLC (candlestick) data and generate trading signal.

        Optional method for strategies that work with OHLC data instead
        of tick-by-tick data. Override this if your strategy is
        designed for candlestick analysis.

        Args:
            ohlc: Dictionary with keys:
                - timestamp (str): ISO 8601 timestamp
                - symbol (str): Trading pair
                - timeframe (str): Timeframe ("1m", "5m", "1h", etc.)
                - open (str): Decimal string of opening price
                - high (str): Decimal string of high price
                - low (str): Decimal string of low price
                - close (str): Decimal string of closing price
                - volume (str): Decimal string of volume
                - trade_count (int): Number of trades in period

        Returns:
            Signal dictionary

        Default implementation returns hold signal.
        """
        return Signal.hold()

    def supports_ohlc(self) -> bool:
        """
        Return True if this strategy supports OHLC data.

        Override this method to return True if your strategy implements
        on_ohlc() and can work with candlestick data.

        Returns:
            False by default
        """
        return False

    def preferred_timeframe(self) -> Optional[str]:
        """
        Return preferred timeframe for OHLC data.

        Only relevant if supports_ohlc() returns True.

        Returns:
            One of: "1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"
            or None if no preference

        Default implementation returns None.
        """
        return None

    def reset(self):
        """
        Reset strategy state for new backtest.

        Called before each backtest run. Use this to clear any
        internal state (price history, indicators, etc.).

        Default implementation does nothing.
        """
        pass
