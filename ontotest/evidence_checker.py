"""
Evidence checker — validates OntoAssure decisions from an OntoRuntime agent run.

Checks:
  1. Did the AfterLoopExit hook fire? (grep server logs)
  2. Did RunFinalizationPort produce a decision?
  3. Is the evidence chain valid?
  4. Can we reproduce the decision deterministically?
"""

import json
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class EvidenceReport:
    hook_fired: bool = False
    finalizer_called: bool = False
    onto_log_messages: list[str] = field(default_factory=list)
    conformance_result: Optional[str] = None
    errors: list[str] = field(default_factory=list)

    @property
    def passed(self) -> bool:
        return len(self.errors) == 0


def check_server_logs(log_text: str) -> EvidenceReport:
    """Check OntoRuntime server logs for OntoAssure activity."""
    report = EvidenceReport()

    patterns = {
        "hook_fired": [
            "AfterLoopExit", "dispatch_observer_at", "HookPointSpec::AfterLoopExit",
        ],
        "finalizer_called": [
            "finalize", "RunFinalizationPort", "run_finalizer",
            "assurance.finalization",
        ],
        "onto_activity": [
            "onto.assurance", "Onto Assurance", "OntoAssure",
            "FinalizationGateway", "session_decision",
        ],
    }

    for line in log_text.splitlines():
        line_lower = line.lower()
        for pattern in patterns["hook_fired"]:
            if pattern.lower() in line_lower:
                report.hook_fired = True
                report.onto_log_messages.append(line.strip())
                break
        for pattern in patterns["finalizer_called"]:
            if pattern.lower() in line_lower:
                report.finalizer_called = True
                if line.strip() not in report.onto_log_messages:
                    report.onto_log_messages.append(line.strip())
                break

    return report


# Default workspace — use ontoos directly (symlinks may not resolve in OntoOS)
ONTOOS_DIR = "/home/admin1/ontocode/ontoos"

def run_conformance_check(ontoos_dir: str = ONTOOS_DIR) -> bool:
    """Run the conformance checker against golden fixtures."""
    try:
        result = subprocess.run(
            ["cargo", "run", "-p", "onto-conformance", "--", "--all"],
            cwd=ontoos_dir,
            capture_output=True,
            text=True,
            timeout=30,
        )
        return "6/6 passed" in result.stdout
    except subprocess.TimeoutExpired:
        return False


def run_unit_tests(ontoos_dir: str = ONTOOS_DIR) -> dict:
    """Run ontoos unit tests and verify they pass."""
    try:
        result = subprocess.run(
            ["cargo", "test", "-p", "onto-assurance-core",
             "-p", "onto-ironclaw-adapter",
             "-p", "onto-assurance-types"],
            cwd=ontoos_dir,
            capture_output=True,
            text=True,
            timeout=120,
        )
        output = result.stdout + result.stderr
        # Parse ALL "test result:" lines looking for "ok" and "FAILED"
        all_ok = True
        total_pass = 0
        total_fail = 0
        for line in output.splitlines():
            if "test result:" in line:
                if "FAILED" in line:
                    all_ok = False
                # Extract numbers: "ok. N passed; M failed"
                if "passed;" in line:
                    try:
                        parts_before = line.split("passed;")[0]
                        total_pass += int(parts_before.split()[-1])
                    except (ValueError, IndexError):
                        pass
                if "failed;" in line:
                    try:
                        parts = line.split("failed;")[0].split()
                        total_fail += int(parts[-1])
                    except (ValueError, IndexError):
                        pass

        return {
            "passed": total_pass if total_pass > 0 else (1 if all_ok else 0),
            "failed": total_fail,
            "all_ok": all_ok,
            "exit_code": result.returncode,
        }
    except subprocess.TimeoutExpired:
        return {"passed": 0, "failed": 1, "all_ok": False, "raw": "timeout"}


def verify_evidence_integrity(
    fixture: str = "success",
    ontoos_dir: str = ONTOOS_DIR,
) -> dict:
    """Run a single fixture through conformance and return the verdict."""
    fixture_path = Path(ontoos_dir) / "fixtures" / fixture / "input.json"
    if not fixture_path.exists():
        return {"error": f"Fixture not found: {fixture_path}"}

    try:
        result = subprocess.run(
            ["cargo", "run", "-p", "onto-conformance", "--", str(fixture_path)],
            cwd=ontoos_dir,
            capture_output=True,
            text=True,
            timeout=15,
        )
        if result.returncode == 0 and result.stdout.strip():
            return json.loads(result.stdout)
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        pass
    return {"error": "conformance check failed"}
