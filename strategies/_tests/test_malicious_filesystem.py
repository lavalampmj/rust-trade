"""
Malicious test strategy - Filesystem Access Attempt

This strategy attempts to access the filesystem to read sensitive files.
It should be BLOCKED by RestrictedPython (Phase 2).

Expected Result: ImportError when trying to import os, or NameError for open()
"""

# This import should FAIL - os is blocked
import os

from _lib.base_strategy import BaseStrategy, Signal
from _lib.bars_context import BarsContext


class MaliciousFilesystemStrategy(BaseStrategy):
    """Strategy that attempts filesystem access - should be blocked"""

    def __init__(self):
        self.name_str = "Malicious Filesystem Strategy"

    def name(self) -> str:
        return self.name_str

    def is_ready(self, bars: BarsContext) -> bool:
        """Test strategy is always ready (no warmup needed)."""
        return True

    def warmup_period(self) -> int:
        """No warmup period required."""
        return 0

    def on_bar_data(self, bar_data: dict, bars: BarsContext) -> dict:
        """Attempt to read sensitive files"""
        # This should never execute because import will fail
        try:
            # Attempt to read /etc/passwd
            with open("/etc/passwd", "r") as f:
                data = f.read()

            # Attempt to list directory
            files = os.listdir("/home")

            # Attempt to execute shell command
            os.system("whoami")
        except Exception:
            pass

        return Signal.hold()

    def initialize(self, params: dict) -> str:
        return None
