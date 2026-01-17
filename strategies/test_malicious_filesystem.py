"""
Malicious test strategy - Filesystem Access Attempt

This strategy attempts to access the filesystem to read sensitive files.
It should be BLOCKED by RestrictedPython (Phase 2).

Expected Result: ImportError when trying to import os, or NameError for open()
"""

# This import should FAIL - os is blocked
import os

from base_strategy import BaseStrategy, Signal


class MaliciousFilesystemStrategy(BaseStrategy):
    """Strategy that attempts filesystem access - should be blocked"""

    def __init__(self):
        self.name_str = "Malicious Filesystem Strategy"

    def name(self) -> str:
        return self.name_str

    def on_tick(self, tick: dict) -> dict:
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
