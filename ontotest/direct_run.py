#!/usr/bin/env python3
"""
direct_run.py — DirectRunAdapter for M5 Onto Assurance E2E testing.

Bypasses HTTP/WebUI, drives `ironclaw run --message` directly,
and extracts Onto Kernel finalization results from the output.

Usage:
    python3 ontotest/direct_run.py --prompt "Write Python hello()" --scenario success
    python3 ontotest/direct_run.py --scenario all
"""

import argparse
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class OntoRuntimeConfig:
    binary: str = "/home/admin1/ironclaw-main/target/debug/ironclaw"
    api_key: str = os.environ.get("OPENAI_API_KEY", "sk-6278327262384ea29c5c9b06e669f8f3")
    model: str = os.environ.get("OPENAI_MODEL", "deepseek-v4-flash")
    base_url: str = os.environ.get("OPENAI_BASE_URL", "https://api.deepseek.com/v1")
    reborn_home: str = os.environ.get(
        "IRONCLAW_REBORN_HOME",
        str(Path.home() / ".ironclaw" / "reborn"),
    )
    timeout_seconds: int = 120


@dataclass
class DirectRunResult:
    """Structured result from a single `ironclaw run` invocation."""
    scenario: str
    prompt: str
    exit_code: int
    task_outcome: Optional[str] = None
    budget_outcome: Optional[str] = None
    lifecycle_state: Optional[str] = None
    session_decision_id: Optional[str] = None
    kernel_called: bool = False
    agent_reply: Optional[str] = None
    raw_stdout: str = ""
    raw_stderr: str = ""
    duration_seconds: float = 0.0
    errors: list[str] = field(default_factory=list)


class DirectRunAdapter:
    """Drives `ironclaw run --message` directly, bypassing HTTP."""

    def __init__(self, config: OntoRuntimeConfig):
        self.bin = config.binary
        self.env = {
            "OPENAI_API_KEY": config.api_key,
            "OPENAI_BASE_URL": config.base_url,
            "OPENAI_MODEL": config.model,
            "IRONCLAW_REBORN_HOME": config.reborn_home,
        }
        self.timeout = config.timeout_seconds

    def execute(self, prompt: str, scenario: str = "unnamed") -> DirectRunResult:
        """Execute a single agent task and extract finalization."""
        t0 = time.time()
        result = DirectRunResult(scenario=scenario, prompt=prompt, exit_code=-1)

        try:
            proc = subprocess.run(
                [self.bin, "run", "--message", prompt],
                env={**os.environ, **self.env},
                capture_output=True,
                text=True,
                timeout=self.timeout,
            )
            result.exit_code = proc.returncode
            result.raw_stdout = proc.stdout
            result.raw_stderr = proc.stderr
            result.agent_reply = proc.stdout.strip()

            # Parse Onto Kernel finalization from stderr
            self._parse_finalization(result, proc.stderr)

        except subprocess.TimeoutExpired:
            result.errors.append(f"Timeout after {self.timeout}s")
        except FileNotFoundError:
            result.errors.append(f"OntoRuntime binary not found: {self.bin}")
        except Exception as e:
            result.errors.append(str(e))

        result.duration_seconds = time.time() - t0
        return result

    def _parse_finalization(self, result: DirectRunResult, stderr: str):
        """Extract Onto Kernel finalization from tracing log output."""
        # Strip ANSI escape codes for reliable parsing
        ansi_escape = re.compile(r'\x1b\[[0-9;]*m')
        clean = ansi_escape.sub('', stderr)

        # Pattern: "Onto Assurance: run finalization complete (kernel) run_id=... task_outcome=Success budget_outcome=WithinBudget lifecycle_state=Committed"
        kernel_pattern = re.compile(
            r"Onto Assurance: run finalization complete \(kernel\)\s+"
            r"run_id=(?P<run_id>\S+)\s+"
            r"task_outcome=(?P<task_outcome>\S+)\s+"
            r"budget_outcome=(?P<budget_outcome>\S+)\s+"
            r"lifecycle_state=(?P<lifecycle_state>\S+)"
        )

        match = kernel_pattern.search(clean)
        if match:
            result.kernel_called = True
            result.task_outcome = match.group("task_outcome")
            result.budget_outcome = match.group("budget_outcome")
            result.lifecycle_state = match.group("lifecycle_state")
            return

        # Fallback: executor-level log (NoopFinalizer or older format)
        executor_pattern = re.compile(
            r"Onto Assurance: run finalization complete\s+"
            r"run_id=(?P<run_id>\S+)\s+"
            r"task_outcome=(?P<task_outcome>\S+)\s+"
            r"lifecycle_state=(?P<lifecycle_state>\S+)"
        )
        match = executor_pattern.search(clean)
        if match:
            result.kernel_called = False  # old NoopFinalizer
            result.task_outcome = match.group("task_outcome")
            result.lifecycle_state = match.group("lifecycle_state")
            return

        result.errors.append("No Onto finalization found in output")


# ══════════════════════════════════════════════════════════════════
# M5 Acceptance Test Scenarios
# ══════════════════════════════════════════════════════════════════

SCENARIOS = {
    "success": {
        "prompt": "Write a Python function hello() that returns the string 'Hello World'. Return ONLY the code, no explanation.",
        "expected": {"task_outcome": "Success", "lifecycle_state": "Committed"},
    },
    "natural_stop": {
        "prompt": "What is 2+2? Answer with just the number.",
        "expected": {"task_outcome": "Success", "lifecycle_state": "Committed"},
    },
    "budget_success": {
        "prompt": "Write a Python function add(a, b) that returns a + b. Return ONLY the code.",
        "expected": {"task_outcome": "Success"},
        # Note: budget simulation depends on --max-turns flag support
    },
    "incomplete": {
        "prompt": "Write a Python function that can perfectly predict stock prices for the next 30 days using only historical data. Include a formal proof of correctness.",
        "expected": {"task_outcome": "Failed", "lifecycle_state": "Continuing"},
        "timeout_override": 90,
    },
    "persist_failure": {
        "prompt": "Write a Python function hello() that returns 'Hello World'. Return ONLY the code.",
        "expected": {"task_outcome": "Success"},
        # Note: persist failure requires injecting a failing DecisionStore
    },
}


def run_scenario(adapter: DirectRunAdapter, name: str, config: dict) -> DirectRunResult:
    """Run a single scenario and return the result."""
    prompt = config["prompt"]
    expected = config.get("expected", {})
    timeout = config.get("timeout_override")
    if timeout:
        adapter.timeout = timeout

    print(f"\n{'='*60}")
    print(f"Scenario: {name}")
    print(f"Prompt: {prompt[:80]}...")
    print(f"Expected: {expected}")
    print(f"{'='*60}")

    result = adapter.execute(prompt, scenario=name)

    # Reset timeout
    adapter.timeout = OntoRuntimeConfig().timeout_seconds

    return result


def check_result(result: DirectRunResult, expected: dict) -> list[str]:
    """Check a result against expectations. Returns list of failure messages."""
    failures = []

    if not result.kernel_called:
        failures.append("Onto Kernel was NOT called")

    for field, expected_value in expected.items():
        actual = getattr(result, field, None)
        if actual != expected_value:
            failures.append(
                f"{field}: expected={expected_value}, got={actual}"
            )

    if result.errors:
        for err in result.errors:
            failures.append(f"Error: {err}")

    return failures


def run_all_scenarios(adapter: DirectRunAdapter) -> tuple[int, int]:
    """Run all M5 scenarios. Returns (passed, failed) counts."""
    passed = 0
    failed = 0

    for name, config in SCENARIOS.items():
        result = run_scenario(adapter, name, config)
        failures = check_result(result, config.get("expected", {}))

        print(f"\n  Exit code: {result.exit_code}")
        print(f"  Duration: {result.duration_seconds:.1f}s")
        print(f"  Kernel called: {result.kernel_called}")
        print(f"  TaskOutcome: {result.task_outcome}")
        print(f"  BudgetOutcome: {result.budget_outcome}")
        print(f"  LifecycleState: {result.lifecycle_state}")
        if result.agent_reply:
            print(f"  Agent reply: {result.agent_reply[:120]}...")

        if failures:
            print(f"  ❌ FAILED:")
            for f_msg in failures:
                print(f"     - {f_msg}")
            failed += 1
        else:
            print(f"  ✅ PASSED")
            passed += 1

    return passed, failed


def main():
    parser = argparse.ArgumentParser(
        description="OntoOS M5 DirectRunAdapter — drive ironclaw run directly"
    )
    parser.add_argument(
        "--prompt", type=str, help="Single prompt to run"
    )
    parser.add_argument(
        "--scenario", type=str, default="success",
        choices=list(SCENARIOS.keys()) + ["all"],
        help="Scenario to run (default: success)",
    )
    parser.add_argument(
        "--ironclaw-bin", type=str,
        default="/home/admin1/ironclaw-main/target/debug/ironclaw",
        help="Path to OntoRuntime binary",
    )
    parser.add_argument(
        "--timeout", type=int, default=120,
        help="Timeout per scenario in seconds",
    )
    parser.add_argument(
        "--json", action="store_true", help="Output results as JSON"
    )
    args = parser.parse_args()

    config = OntoRuntimeConfig(
        binary=args.ironclaw_bin,
        timeout_seconds=args.timeout,
    )
    adapter = DirectRunAdapter(config)

    if not os.path.exists(config.binary):
        print(f"ERROR: OntoRuntime binary not found at {config.binary}")
        print("Build with: cd ironclaw-main && cargo build -p ironclaw --features onto-assurance")
        sys.exit(1)

    if args.scenario == "all":
        passed, failed = run_all_scenarios(adapter)
        print(f"\n{'='*60}")
        print(f"M5 Acceptance: {passed} passed, {failed} failed")
        print(f"{'='*60}")
        sys.exit(0 if failed == 0 else 1)
    elif args.prompt:
        # Custom prompt
        result = adapter.execute(args.prompt)
        if args.json:
            print(json.dumps({
                "task_outcome": result.task_outcome,
                "lifecycle_state": result.lifecycle_state,
                "budget_outcome": result.budget_outcome,
                "kernel_called": result.kernel_called,
                "errors": result.errors,
            }, indent=2))
        else:
            print(f"TaskOutcome: {result.task_outcome}")
            print(f"LifecycleState: {result.lifecycle_state}")
            print(f"Kernel called: {result.kernel_called}")
            if result.errors:
                for e in result.errors:
                    print(f"Error: {e}")
        sys.exit(0 if result.kernel_called and result.errors == [] else 1)
    else:
        # Named scenario
        config_data = SCENARIOS[args.scenario]
        result = run_scenario(adapter, args.scenario, config_data)
        failures = check_result(result, config_data.get("expected", {}))

        if args.json:
            print(json.dumps({
                "scenario": args.scenario,
                "task_outcome": result.task_outcome,
                "lifecycle_state": result.lifecycle_state,
                "budget_outcome": result.budget_outcome,
                "kernel_called": result.kernel_called,
                "duration_seconds": result.duration_seconds,
                "failures": failures,
                "errors": result.errors,
            }, indent=2))
        else:
            print(f"TaskOutcome: {result.task_outcome}")
            print(f"LifecycleState: {result.lifecycle_state}")
            if failures:
                for f in failures:
                    print(f"FAIL: {f}")
            else:
                print("PASS")

        sys.exit(0 if failures == [] else 1)


if __name__ == "__main__":
    main()
