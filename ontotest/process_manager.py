"""
Process lifecycle manager for OntoRuntime.

Owns the OntoRuntime process from start to stop.  Survives parent process
death by using setsid + nohup.  Provides health-check polling and
graceful shutdown with evidence preservation.
"""

import os
import signal
import subprocess
import time
import json
import requests
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass
class OntoRuntimeConfig:
    binary: str = "/home/admin1/ironclaw-main/target/debug/ironclaw"
    host: str = "127.0.0.1"
    port: int = 3000
    health_timeout: int = 30
    api_key: str = ""
    model: str = "deepseek-v4-flash"
    base_url: str = "https://api.deepseek.com/v1"
    webui_token: str = ""
    log_dir: str = "/tmp/ontotest"


class OntoRuntimeProcess:
    """Manages a single OntoRuntime server process."""

    def __init__(self, config: OntoRuntimeConfig):
        self.config = config
        self.process: Optional[subprocess.Popen] = None
        self.pid: Optional[int] = None
        self.log_path = Path(config.log_dir) / f"ironclaw_{int(time.time())}.log"
        self._ensure_log_dir()

    def _ensure_log_dir(self):
        Path(self.config.log_dir).mkdir(parents=True, exist_ok=True)

    @property
    def base_url(self) -> str:
        return f"http://{self.config.host}:{self.config.port}"

    def start(self) -> bool:
        """Start OntoRuntime. Returns True when healthy."""
        log_file = open(self.log_path, "w")
        env = os.environ.copy()
        env.update({
            "OPENAI_API_KEY": self.config.api_key,
            "OPENAI_BASE_URL": self.config.base_url,
            "OPENAI_MODEL": self.config.model,
            "IRONCLAW_REBORN_WEBUI_TOKEN": self.config.webui_token,
        })

        # Use setsid to detach from parent process group.
        # The process survives even if the test runner is killed.
        self.process = subprocess.Popen(
            [self.config.binary, "serve"],
            stdout=log_file,
            stderr=subprocess.STDOUT,
            env=env,
            preexec_fn=os.setsid,
        )
        self.pid = self.process.pid
        return self._wait_healthy()

    def _wait_healthy(self) -> bool:
        """Poll /api/health until ready or timeout."""
        deadline = time.time() + self.config.health_timeout
        while time.time() < deadline:
            if not self.is_alive():
                return False
            try:
                resp = requests.get(f"{self.base_url}/api/health", timeout=2)
                if resp.status_code == 200 and "healthy" in resp.text:
                    return True
            except requests.RequestException:
                pass
            time.sleep(1)
        return False

    def is_alive(self) -> bool:
        """Check if the process is still running."""
        if self.process is None:
            return False
        return self.process.poll() is None

    def stop(self, graceful: bool = True) -> None:
        """Stop OntoRuntime. Graceful by default (SIGTERM), SIGKILL if forced."""
        if self.process is None:
            return
        sig = signal.SIGTERM if graceful else signal.SIGKILL
        try:
            os.killpg(os.getpgid(self.process.pid), sig)
            self.process.wait(timeout=10)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            if graceful:
                os.killpg(os.getpgid(self.process.pid), signal.SIGKILL)
        self.process = None

    def collect_logs(self) -> str:
        """Read collected log output."""
        if self.log_path.exists():
            return self.log_path.read_text()
        return ""

    def grep_log(self, pattern: str) -> list[str]:
        """Search logs for pattern."""
        if not self.log_path.exists():
            return []
        matches = []
        for line in self.log_path.read_text().splitlines():
            if pattern.lower() in line.lower():
                matches.append(line.strip())
        return matches
