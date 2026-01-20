"""
Example RSI (Relative Strength Index) Strategy in Python

This strategy demonstrates:
- RSI calculation using price changes
- Overbought/oversold threshold trading
- State management with collections
- Type hints for clarity

Strategy Logic:
- Buy when RSI < 30 (oversold)
- Sell when RSI > 70 (overbought)
- Hold otherwise

Uses the unified on_bar_data() interface with OnCloseBar mode.
"""

from typing import Optional, Dict, Any
from collections import deque
from decimal import Decimal
from base_strategy import BaseStrategy, Signal


class ExampleRsiStrategy(BaseStrategy):
    """
    RSI strategy that buys when oversold (RSI < 30) and sells when overbought (RSI > 70).

    The RSI is calculated over a configurable period (default 14) using the
    average gains and losses.
    """

    def __init__(self):
        """Initialize the RSI strategy with default parameters."""
        self.period = 14  # RSI period
        self.oversold = 30.0  # Buy threshold
        self.overbought = 70.0  # Sell threshold

        # Track price changes for RSI calculation
        self.price_changes = deque(maxlen=self.period)
        self.last_price = None

        # Track position state
        self.has_position = False
        self.last_signal_type = None

    def name(self) -> str:
        """Return the strategy name."""
        return "Relative Strength Index (Python)"

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """
        Initialize strategy with parameters.

        Supported parameters:
        - period: RSI calculation period (default: 14)
        - oversold: Buy threshold (default: 30)
        - overbought: Sell threshold (default: 70)

        Returns error message if validation fails, None otherwise.
        """
        if "period" in params:
            try:
                period = int(params["period"])
                if period < 2:
                    return "RSI period must be at least 2"
                self.period = period
                self.price_changes = deque(maxlen=self.period)
            except ValueError:
                return "Invalid period parameter: must be an integer"

        if "oversold" in params:
            try:
                oversold = float(params["oversold"])
                if not (0 <= oversold <= 100):
                    return "Oversold threshold must be between 0 and 100"
                self.oversold = oversold
            except ValueError:
                return "Invalid oversold parameter: must be a number"

        if "overbought" in params:
            try:
                overbought = float(params["overbought"])
                if not (0 <= overbought <= 100):
                    return "Overbought threshold must be between 0 and 100"
                self.overbought = overbought
            except ValueError:
                return "Invalid overbought parameter: must be a number"

        if self.oversold >= self.overbought:
            return "Oversold threshold must be less than overbought threshold"

        return None

    def reset(self) -> None:
        """Reset strategy state."""
        self.price_changes.clear()
        self.last_price = None
        self.has_position = False
        self.last_signal_type = None

    def calculate_rsi(self) -> Optional[float]:
        """
        Calculate RSI from price changes.

        Returns:
            RSI value (0-100) or None if not enough data
        """
        if len(self.price_changes) < self.period:
            return None

        gains = []
        losses = []

        for change in self.price_changes:
            if change > 0:
                gains.append(change)
                losses.append(0)
            else:
                gains.append(0)
                losses.append(abs(change))

        avg_gain = sum(gains) / self.period
        avg_loss = sum(losses) / self.period

        if avg_loss == 0:
            return 100.0  # No losses means maximum RSI

        rs = avg_gain / avg_loss
        rsi = 100 - (100 / (1 + rs))

        return rsi

    def on_bar_data(self, bar_data: Dict[str, Any]) -> Dict[str, Any]:
        """
        Process bar data and generate trading signal.

        Uses close price for RSI calculation. Operates in OnCloseBar mode
        so only processes completed bars.

        Args:
            bar_data: Bar data dictionary with OHLC and metadata

        Returns:
            Signal dictionary (from Signal.buy/sell/hold)
        """
        symbol = bar_data["symbol"]
        close_price = float(bar_data["close"])

        # Track price changes using close prices
        if self.last_price is not None:
            change = close_price - self.last_price
            self.price_changes.append(change)

        self.last_price = close_price

        # Calculate RSI
        rsi = self.calculate_rsi()

        if rsi is None:
            return Signal.hold()

        # Generate signals based on RSI
        # Buy when oversold and don't have position
        if rsi < self.oversold and not self.has_position:
            self.has_position = True
            self.last_signal_type = "Buy"
            # Use 10% of capital for each trade
            return Signal.buy(symbol, "0.1")

        # Sell when overbought and have position
        if rsi > self.overbought and self.has_position:
            self.has_position = False
            self.last_signal_type = "Sell"
            return Signal.sell(symbol, "0.1")

        return Signal.hold()

    def bar_data_mode(self) -> str:
        """Return OnCloseBar mode - process only completed bars."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """Prefer 1-minute time-based bars."""
        return {"type": "TimeBased", "timeframe": "1m"}
