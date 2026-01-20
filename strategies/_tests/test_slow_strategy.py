"""
Test strategy - Slow Execution

This strategy intentionally takes a long time to execute to test
resource monitoring (Phase 3).

Expected Result:
- Strategy executes successfully (no security violation)
- WARNING log when execution exceeds 10ms threshold
- Metrics tracked: cpu_time_us, call_count, peak_execution_us

Uses the unified on_bar_data() interface.
"""

import time
from typing import Dict, Optional, Any
from _lib.base_strategy import BaseStrategy, Signal


class SlowStrategy(BaseStrategy):
    """Strategy with intentionally slow execution for testing resource monitoring"""

    def __init__(self):
        self.call_number = 0

    def name(self) -> str:
        return "Slow Test Strategy"

    def on_bar_data(self, bar_data: Dict[str, Any]) -> Dict[str, Any]:
        """Simulate slow computation"""
        self.call_number += 1

        # Sleep for 15ms to trigger warning (threshold is 10ms)
        # This simulates heavy computation
        time.sleep(0.015)  # 15 milliseconds

        # Do some computation
        result = 0
        for i in range(10000):
            result += i * i

        # Return hold signal
        return Signal.hold()

    def initialize(self, params: Dict[str, str]) -> Optional[str]:
        """Initialize with no errors"""
        return None

    def reset(self) -> None:
        """Reset state"""
        self.call_number = 0

    def bar_data_mode(self) -> str:
        """Return OnCloseBar mode by default."""
        return "OnCloseBar"

    def preferred_bar_type(self) -> Dict[str, Any]:
        """Default to 1-minute time-based bars."""
        return {"type": "TimeBased", "timeframe": "1m"}
