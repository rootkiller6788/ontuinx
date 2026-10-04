#!/usr/bin/env python3
"""
Onto Assurance Kernel — Python ↔ Rust Differential Test Runner.

Compares Rust `onto-conformance` output against fixture `expected` values.

Usage:
    python conformance/diff_runner.py                     # Run all
    python conformance/diff_runner.py --scenario success   # Single
    python conformance/diff_runner.py --rust-bin path      # Pre-built binary
"""

import json
import subprocess
import sys
from pathlib import Path
from typing import Optional

FIXTURE_DIR = Path(__file__).resolve().parent.parent / "fixtures"
SCENARIOS = [
    "success", "incomplete", "verification_failed", "environment_error",
    "budget_depleted_success", "evidence_tampered",
]


def load_fixture(scenario: str) -> Optional[dict]:
    """Load the input.json fixture."""
    path = FIXTURE_DIR / scenario / "input.json"
    if not path.exists():
        return None
    with open(path) as f:
        return json.load(f)


def run_rust_binary(scenario: str, binary_path: Optional[str]) -> Optional[dict]:
    """Run onto-conformance and return parsed JSON output."""
    fixture_path = FIXTURE_DIR / scenario / "input.json"
    if not fixture_path.exists():
        return None

    if not binary_path:
        # Auto-detect
        for t in [
            Path(__file__).parent.parent / "target" / "debug" / "onto-conformance",
            Path(__file__).parent.parent / "target" / "release" / "onto-conformance",
        ]:
            if t.exists():
                binary_path = str(t)
                break

    if not binary_path:
        print(f"  [{scenario}] no binary found, build with: cargo build -p onto-conformance")
        return None

    result = subprocess.run(
        [binary_path, str(fixture_path)],
        capture_output=True, text=True, timeout=30,
    )
    if result.returncode != 0 or not result.stdout:
        return None

    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return None


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Onto Diff Runner")
    parser.add_argument("--scenario", default=None, help="Single scenario")
    parser.add_argument("--rust-bin", default=None, help="Path to onto-conformance binary")
    args = parser.parse_args()

    scenarios = [args.scenario] if args.scenario else SCENARIOS
    errors = 0

    for scenario in scenarios:
        fixture = load_fixture(scenario)
        if not fixture:
            print(f"  ❌ {scenario}: input.json not found")
            errors += 1
            continue

        expected = fixture.get("expected", {})
        if not expected:
            print(f"  ❌ {scenario}: no 'expected' field in fixture")
            errors += 1
            continue

        actual = run_rust_binary(scenario, args.rust_bin)
        if actual is None:
            print(f"  ❌ {scenario}: no Rust output")
            errors += 1
            continue

        # Compare deterministic fields
        scenario_errors: list[str] = []

        expected_outcome = expected.get("task_outcome", "")
        if expected_outcome:
            actual_outcome = actual.get("task_outcome", "")
            if actual_outcome != expected_outcome:
                scenario_errors.append(
                    f"outcome: got '{actual_outcome}', expected '{expected_outcome}'"
                )

        expected_settlement = expected.get("settlement", "")
        if expected_settlement:
            actual_settlement = actual.get("settlement", "")
            if actual_settlement != expected_settlement:
                scenario_errors.append(
                    f"settlement: got '{actual_settlement}', expected '{expected_settlement}'"
                )

        expected_lifecycle = expected.get("lifecycle_state", "")
        if expected_lifecycle:
            actual_lifecycle = actual.get("lifecycle_state", "")
            if actual_lifecycle != expected_lifecycle:
                scenario_errors.append(
                    f"lifecycle: got '{actual_lifecycle}', expected '{expected_lifecycle}'"
                )

        expected_chain = expected.get("evidence_chain_valid")
        if expected_chain is not None:
            actual_chain = actual.get("chain_valid", False)
            if actual_chain != expected_chain:
                scenario_errors.append(
                    f"chain_valid: got {actual_chain}, expected {expected_chain}"
                )

        if scenario_errors:
            errors += 1
            for err in scenario_errors:
                print(f"  ❌ {scenario}: {err}")
        else:
            print(f"  ✅ {scenario} ({actual.get('task_outcome')}, {actual.get('settlement')})")

    if errors:
        print(f"\n{'='*50}")
        print(f"FAIL: {errors} scenario(s) failed")
        sys.exit(1)


if __name__ == "__main__":
    main()
