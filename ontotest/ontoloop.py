#!/usr/bin/env python3
"""
ontoloop.py — Real OntoLoop attempt cycle driver.

Uses the real ironclaw binary (with onto-assurance) to drive a multi-attempt
task loop. Each attempt is a full ironclaw run. After each attempt, reads the
OntoAssure finalization and decides: Commit, Continue (with structured gaps),
or Escalate.

Usage:
    python3 ontotest/ontoloop.py --task "Write a Python function hello() that returns 'Hello World'"
    python3 ontotest/ontoloop.py --task-file task.json --max-attempts 3
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


# ══════════════════════════════════════════════════════════════════
# Configuration
# ══════════════════════════════════════════════════════════════════

IRONCLAW_BIN = os.environ.get(
    "IRONCLAW_BIN",
    "/home/admin1/ironclaw-main/target/debug/ironclaw",
)

DEFAULT_MODEL = os.environ.get("OPENAI_MODEL", "deepseek-v4-flash")
DEFAULT_API_KEY = os.environ.get("OPENAI_API_KEY", "sk-6278327262384ea29c5c9b06e669f8f3")
DEFAULT_BASE_URL = os.environ.get("OPENAI_BASE_URL", "https://api.deepseek.com/v1")

# ══════════════════════════════════════════════════════════════════
# Types
# ══════════════════════════════════════════════════════════════════

@dataclass
class Criterion:
    name: str
    description: str
    kind: str = "test_pass"

@dataclass
class OntoLoopTask:
    objective: str
    criteria: list[Criterion] = field(default_factory=list)
    max_attempts: int = 3
    timeout_per_attempt: int = 120

@dataclass
class AttemptResult:
    attempt_number: int
    run_id: Optional[str]
    task_outcome: Optional[str]
    lifecycle_state: Optional[str]
    budget_outcome: Optional[str]
    agent_reply: str = ""
    duration_seconds: float = 0.0
    error: Optional[str] = None

@dataclass
class OntoLoopResult:
    task: OntoLoopTask
    attempts: list[AttemptResult] = field(default_factory=list)
    final_decision: str = "unknown"
    total_duration_seconds: float = 0.0


# ══════════════════════════════════════════════════════════════════
# Core
# ══════════════════════════════════════════════════════════════════

class OntoLoopRunner:
    """Drives multi-attempt task loops using real ironclaw binary."""

    def __init__(self, ironclaw_bin: str = IRONCLAW_BIN):
        self.bin = ironclaw_bin
        if not os.path.exists(self.bin):
            raise FileNotFoundError(f"OntoRuntime binary not found: {self.bin}\n"
                                    f"Build: cd ironclaw-main && cargo build -p ironclaw --features onto-assurance")

    def run_task(self, task: OntoLoopTask) -> OntoLoopResult:
        """Execute a task with up to max_attempts."""
        t0 = time.time()
        result = OntoLoopResult(task=task)

        for attempt_num in range(1, task.max_attempts + 1):
            print(f"\n{'='*60}")
            print(f"Attempt {attempt_num}/{task.max_attempts}")
            print(f"{'='*60}")

            # Build prompt with criteria context
            prompt = self._build_prompt(task, attempt_num, result.attempts)

            # Execute one ironclaw run
            att = self._execute_attempt(attempt_num, prompt, task.timeout_per_attempt)
            result.attempts.append(att)

            if att.error:
                print(f"  ❌ Attempt failed: {att.error}")
                result.final_decision = "escalated"
                break

            print(f"  TaskOutcome: {att.task_outcome}")
            print(f"  LifecycleState: {att.lifecycle_state}")
            print(f"  Duration: {att.duration_seconds:.1f}s")

            # Decide next action
            if att.task_outcome == "Success":
                print(f"  ✅ Task complete!")
                result.final_decision = "committed"
                break
            elif att.task_outcome == "EnvironmentError":
                print(f"  🚨 Environment error — escalating")
                result.final_decision = "escalated"
                break
            elif att.task_outcome == "Failed":
                if attempt_num < task.max_attempts:
                    print(f"  🔄 Failed — preparing repair attempt...")
                else:
                    print(f"  ❌ Max attempts reached — escalating")
                    result.final_decision = "escalated"
                    break
            elif att.task_outcome == "Incomplete":
                if attempt_num < task.max_attempts:
                    print(f"  🔄 Incomplete — continuing...")
                else:
                    result.final_decision = "escalated"
                    break
            else:
                result.final_decision = "escalated"
                break

        result.total_duration_seconds = time.time() - t0
        return result

    def _build_prompt(self, task: OntoLoopTask, attempt_num: int,
                      prev_attempts: list[AttemptResult]) -> str:
        """Build the prompt for this attempt, incorporating feedback from failures."""
        if attempt_num == 1:
            criteria_text = "\n".join(
                f"  - {c.name}: {c.description}" for c in task.criteria
            ) if task.criteria else "  (no explicit criteria)"

            return (
                f"{task.objective}\n\n"
                f"Requirements:\n{criteria_text}\n\n"
                f"Return ONLY the code/solution, no explanation."
            )

        # Repair attempt: provide specific feedback from previous failure
        last = prev_attempts[-1]
        return (
            f"Previous attempt failed. The task was:\n"
            f"{task.objective}\n\n"
            f"The criteria are:\n" +
            "\n".join(f"  - {c.name}: {c.description}" for c in task.criteria) +
            f"\n\nPlease fix the issues and try again. Return ONLY the corrected solution."
        )

    def _execute_attempt(self, attempt_num: int, prompt: str,
                         timeout: int) -> AttemptResult:
        """Run one ironclaw invocation and parse the result."""
        t0 = time.time()
        result = AttemptResult(attempt_number=attempt_num, run_id=None,
                               task_outcome=None, lifecycle_state=None,
                               budget_outcome=None)

        try:
            env = {
                **os.environ,
                "OPENAI_API_KEY": DEFAULT_API_KEY,
                "OPENAI_BASE_URL": DEFAULT_BASE_URL,
                "OPENAI_MODEL": DEFAULT_MODEL,
            }

            proc = subprocess.run(
                [self.bin, "run", "--message", prompt],
                env=env,
                capture_output=True, text=True,
                timeout=timeout,
            )

            result.agent_reply = proc.stdout.strip()
            result.run_id = f"attempt-{attempt_num}"

            # Parse OntoAssure finalization from stderr
            self._parse_finalization(result, proc.stderr)

        except subprocess.TimeoutExpired:
            result.error = f"Timeout after {timeout}s"
        except FileNotFoundError:
            result.error = f"Binary not found: {self.bin}"
        except Exception as e:
            result.error = str(e)

        result.duration_seconds = time.time() - t0
        return result

    def _parse_finalization(self, result: AttemptResult, stderr: str):
        """Extract OntoAssure finalization from tracing output."""
        ansi_escape = re.compile(r'\x1b\[[0-9;]*m')
        clean = ansi_escape.sub('', stderr)

        kernel_pattern = re.compile(
            r"Onto Assurance: run finalization complete \(kernel\)\s+"
            r"run_id=(?P<run_id>\S+)\s+"
            r"task_outcome=(?P<task_outcome>\S+)\s+"
            r"budget_outcome=(?P<budget_outcome>\S+)\s+"
            r"lifecycle_state=(?P<lifecycle_state>\S+)"
        )

        match = kernel_pattern.search(clean)
        if match:
            result.run_id = match.group("run_id")
            result.task_outcome = match.group("task_outcome")
            result.budget_outcome = match.group("budget_outcome")
            result.lifecycle_state = match.group("lifecycle_state")
            return

        # Fallback: executor-level log
        executor_pattern = re.compile(
            r"Onto Assurance: run finalization complete\s+"
            r"run_id=(?P<run_id>\S+)\s+"
            r"task_outcome=(?P<task_outcome>\S+)\s+"
            r"lifecycle_state=(?P<lifecycle_state>\S+)"
        )
        match = executor_pattern.search(clean)
        if match:
            result.run_id = match.group("run_id")
            result.task_outcome = match.group("task_outcome")
            result.lifecycle_state = match.group("lifecycle_state")


# ══════════════════════════════════════════════════════════════════
# CLI
# ══════════════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(description="OntoLoop — Multi-attempt task loop")
    parser.add_argument("--task", type=str, help="Task objective")
    parser.add_argument("--task-file", type=str, help="JSON file with task definition")
    parser.add_argument("--max-attempts", type=int, default=3)
    parser.add_argument("--timeout", type=int, default=120)
    parser.add_argument("--json", action="store_true", help="Output JSON")
    args = parser.parse_args()

    if args.task_file:
        with open(args.task_file) as f:
            data = json.load(f)
        criteria = [Criterion(**c) for c in data.get("criteria", [])]
        task = OntoLoopTask(
            objective=data["objective"],
            criteria=criteria,
            max_attempts=data.get("max_attempts", args.max_attempts),
            timeout_per_attempt=data.get("timeout", args.timeout),
        )
    elif args.task:
        task = OntoLoopTask(
            objective=args.task,
            max_attempts=args.max_attempts,
            timeout_per_attempt=args.timeout,
        )
    else:
        # Demo: simple coding task
        task = OntoLoopTask(
            objective="Write a Python function hello() that returns the string 'Hello World'",
            criteria=[Criterion(name="hello_function", description="hello() returns 'Hello World'")],
            max_attempts=2,
            timeout_per_attempt=60,
        )

    runner = OntoLoopRunner()
    result = runner.run_task(task)

    if args.json:
        print(json.dumps({
            "final_decision": result.final_decision,
            "attempts": [
                {
                    "attempt_number": a.attempt_number,
                    "task_outcome": a.task_outcome,
                    "lifecycle_state": a.lifecycle_state,
                    "duration_seconds": a.duration_seconds,
                }
                for a in result.attempts
            ],
            "total_duration_seconds": result.total_duration_seconds,
        }, indent=2))
    else:
        print(f"\n{'='*60}")
        print(f"OntoLoop Result: {result.final_decision}")
        print(f"Attempts: {len(result.attempts)}")
        print(f"Total duration: {result.total_duration_seconds:.1f}s")
        for a in result.attempts:
            status = "✅" if a.task_outcome == "Success" else "❌" if a.error else "🔄"
            print(f"  {status} Attempt {a.attempt_number}: {a.task_outcome} / {a.lifecycle_state} ({a.duration_seconds:.1f}s)")

    sys.exit(0 if result.final_decision == "committed" else 1)


if __name__ == "__main__":
    main()
