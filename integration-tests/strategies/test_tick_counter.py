"""
Test Tick Counter Strategy for Integration Tests

This is a minimal Python strategy that counts ticks and records latency.
It does not generate trading signals - it always returns HOLD.

Usage:
    This strategy is loaded by the PythonStrategyRunner during integration tests.
    It receives ticks through the on_bar_data() method and tracks:
    - Total tick count
    - Receive timestamps for latency calculation
    - Optional volume statistics
"""

import time
from decimal import Decimal
from typing import Optional, Dict, Any


class TestTickCounterStrategy:
    """
    Minimal test strategy for counting ticks and measuring latency.

    This strategy is designed for integration testing and does not
    generate actual trading signals.
    """

    def __init__(self):
        """Initialize the strategy with default state."""
        self._tick_count: int = 0
        self._total_volume: Decimal = Decimal('0')
        self._total_value: Decimal = Decimal('0')
        self._first_tick_time: Optional[float] = None
        self._last_tick_time: Optional[float] = None
        self._latency_sum_us: int = 0
        self._latency_count: int = 0

    @property
    def name(self) -> str:
        """Return the strategy name."""
        return "Test Tick Counter (Python)"

    @property
    def tick_count(self) -> int:
        """Return total tick count."""
        return self._tick_count

    @property
    def total_volume(self) -> Decimal:
        """Return total volume processed."""
        return self._total_volume

    @property
    def total_value(self) -> Decimal:
        """Return total value (sum of price * quantity)."""
        return self._total_value

    @property
    def average_latency_us(self) -> float:
        """Return average latency in microseconds."""
        if self._latency_count == 0:
            return 0.0
        return self._latency_sum_us / self._latency_count

    def initialize(self, params: Dict[str, str]) -> None:
        """
        Initialize strategy with parameters.

        Args:
            params: Dictionary of string parameters
        """
        # No parameters needed for test strategy
        pass

    def reset(self) -> None:
        """Reset strategy state for new test run."""
        self._tick_count = 0
        self._total_volume = Decimal('0')
        self._total_value = Decimal('0')
        self._first_tick_time = None
        self._last_tick_time = None
        self._latency_sum_us = 0
        self._latency_count = 0

    def is_ready(self) -> bool:
        """Check if strategy is ready to process data."""
        return True

    def warmup_period(self) -> int:
        """Return warmup period (number of bars needed)."""
        return 0

    def on_bar_data(self, bar_data: Dict[str, Any]) -> str:
        """
        Process a bar data event.

        This method receives tick/bar data and updates internal statistics.
        It always returns HOLD as this strategy does not trade.

        Args:
            bar_data: Dictionary containing:
                - current_tick: Optional tick data dict
                - ohlc_bar: OHLC bar dict
                - metadata: Bar metadata dict

        Returns:
            Signal string: "HOLD"
        """
        now = time.time()

        # Update timestamps
        if self._first_tick_time is None:
            self._first_tick_time = now
        self._last_tick_time = now

        # Increment tick count
        self._tick_count += 1

        # Process tick data if available
        current_tick = bar_data.get('current_tick')
        if current_tick:
            price = Decimal(str(current_tick.get('price', 0)))
            quantity = Decimal(str(current_tick.get('quantity', 0)))

            self._total_volume += quantity
            self._total_value += price * quantity

            # Calculate latency if timestamp available
            ts_recv = current_tick.get('ts_recv')
            if ts_recv:
                # ts_recv is typically a datetime or timestamp
                # Calculate latency from receive time to now
                try:
                    if isinstance(ts_recv, float):
                        latency_us = int((now - ts_recv) * 1_000_000)
                    else:
                        # Assume ts_recv is already in microseconds
                        latency_us = int(now * 1_000_000) - int(ts_recv)

                    if latency_us >= 0 and latency_us < 10_000_000:  # Sanity check: < 10s
                        self._latency_sum_us += latency_us
                        self._latency_count += 1
                except (TypeError, ValueError):
                    pass

        # Always return HOLD - this strategy doesn't trade
        return "HOLD"

    def on_tick(self, tick: Dict[str, Any]) -> str:
        """
        Convenience method for direct tick processing.

        Args:
            tick: Tick data dictionary

        Returns:
            Signal string: "HOLD"
        """
        return self.on_bar_data({
            'current_tick': tick,
            'ohlc_bar': {},
            'metadata': {}
        })

    def get_stats(self) -> Dict[str, Any]:
        """
        Get strategy statistics.

        Returns:
            Dictionary with statistics
        """
        duration = 0.0
        if self._first_tick_time and self._last_tick_time:
            duration = self._last_tick_time - self._first_tick_time

        throughput = 0.0
        if duration > 0:
            throughput = self._tick_count / duration

        return {
            'name': self.name,
            'tick_count': self._tick_count,
            'total_volume': float(self._total_volume),
            'total_value': float(self._total_value),
            'duration_seconds': duration,
            'throughput_ticks_per_sec': throughput,
            'average_latency_us': self.average_latency_us,
            'latency_samples': self._latency_count,
        }

    def __repr__(self) -> str:
        return f"TestTickCounterStrategy(ticks={self._tick_count})"


# For direct testing
if __name__ == "__main__":
    strategy = TestTickCounterStrategy()

    # Simulate some ticks
    for i in range(100):
        tick = {
            'symbol': f'TEST{i % 10:04d}',
            'price': 50000 + i,
            'quantity': 100,
            'ts_recv': time.time() - 0.001,  # 1ms ago
        }
        signal = strategy.on_tick(tick)
        assert signal == "HOLD"

    stats = strategy.get_stats()
    print(f"Strategy: {strategy.name}")
    print(f"Ticks processed: {stats['tick_count']}")
    print(f"Total value: {stats['total_value']}")
    print(f"Average latency: {stats['average_latency_us']:.1f}μs")
    print("Test passed!")
