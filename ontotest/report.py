"""
Test report generator.

Produces structured reports from test results in JSON and terminal formats.
"""

import json
import time
from dataclasses import dataclass, field
from datetime import datetime
from typing import Any


@dataclass
class TestCase:
    name: str
    status: str  # "PASS" | "FAIL" | "SKIP" | "ERROR"
    duration_seconds: float = 0.0
    details: dict[str, Any] = field(default_factory=dict)
    errors: list[str] = field(default_factory=list)


@dataclass
class TestSuite:
    name: str
    cases: list[TestCase] = field(default_factory=list)
    started_at: str = ""
    finished_at: str = ""

    @property
    def passed(self) -> int:
        return sum(1 for c in self.cases if c.status == "PASS")

    @property
    def failed(self) -> int:
        return sum(1 for c in self.cases if c.status == "FAIL")

    @property
    def total(self) -> int:
        return len(self.cases)

    @property
    def duration(self) -> float:
        return sum(c.duration_seconds for c in self.cases)


class ReportGenerator:
    """Generates terminal and JSON reports."""

    def __init__(self):
        self.suites: list[TestSuite] = []

    def add_suite(self, suite: TestSuite):
        self.suites.append(suite)

    def terminal_report(self) -> str:
        """Render a colored terminal report."""
        lines = []
        lines.append("")
        lines.append("═" * 60)
        lines.append("  OntoOS Assurance Test Report")
        lines.append(f"  {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
        lines.append("═" * 60)

        total_pass = 0
        total_fail = 0
        total_all = 0

        for suite in self.suites:
            lines.append(f"\n── {suite.name} ──")
            for case in suite.cases:
                icon = {"PASS": "✅", "FAIL": "❌", "SKIP": "⏭️", "ERROR": "💥"}[case.status]
                duration = f" ({case.duration_seconds:.1f}s)" if case.duration_seconds else ""
                lines.append(f"  {icon} {case.name}{duration}")
                for err in case.errors:
                    lines.append(f"      {err}")
            total_pass += suite.passed
            total_fail += suite.failed
            total_all += suite.total

        lines.append("")
        lines.append("─" * 60)
        lines.append(f"  Total: {total_all}  |  Passed: {total_pass}  |  Failed: {total_fail}")
        if total_all > 0:
            rate = total_pass / total_all * 100
            lines.append(f"  Pass Rate: {rate:.1f}%")
        lines.append("═" * 60)
        return "\n".join(lines)

    def json_report(self) -> dict:
        """Produce a machine-readable report."""
        return {
            "timestamp": datetime.now().isoformat(),
            "suites": [
                {
                    "name": s.name,
                    "started_at": s.started_at,
                    "finished_at": s.finished_at,
                    "passed": s.passed,
                    "failed": s.failed,
                    "total": s.total,
                    "duration_seconds": s.duration,
                    "cases": [
                        {
                            "name": c.name,
                            "status": c.status,
                            "duration_seconds": c.duration_seconds,
                            "errors": c.errors,
                            "details": c.details,
                        }
                        for c in s.cases
                    ],
                }
                for s in self.suites
            ],
        }

    def save_json(self, path: str):
        with open(path, "w") as f:
            json.dump(self.json_report(), f, indent=2)
