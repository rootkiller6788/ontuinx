#!/usr/bin/env python3
"""
ontotest — OntoOS Assurance Test Runner.

Self-contained: starts OntoRuntime, runs tests, collects evidence, stops OntoRuntime.

Usage:
    python3 ontotest/runner.py --suite unit     # Unit + conformance only
    python3 ontotest/runner.py --suite e2e      # Full E2E (starts OntoRuntime)
    python3 ontotest/runner.py                  # Everything
"""

import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from process_manager import OntoRuntimeConfig, OntoRuntimeProcess
from agent_client import AgentClient, AgentTask
from evidence_checker import (
    check_server_logs, run_conformance_check, run_unit_tests,
    verify_evidence_integrity, ONTOOS_DIR,
)
from report import ReportGenerator, TestCase, TestSuite
import requests


class OntoTestRunner:
    def __init__(self):
        self.base_url = "http://127.0.0.1:3000"
        self.webui_token = self._read_token()

        self.config = OntoRuntimeConfig(
            binary="/home/admin1/ironclaw-main/target/debug/ironclaw",
            api_key=os.environ.get("OPENAI_API_KEY", "sk-6278327262384ea29c5c9b06e669f8f3"),
            model=os.environ.get("OPENAI_MODEL", "deepseek-v4-flash"),
            base_url=os.environ.get("OPENAI_BASE_URL", "https://api.deepseek.com/v1"),
            webui_token=self.webui_token,
        )
        # Add DATABASE_URL for full product stack
        db_url = os.environ.get("DATABASE_URL", "postgres://ironclaw:ironclaw@localhost:5432/ironclaw")
        self.server_env = {
            "DATABASE_URL": db_url,
            "OPENAI_API_KEY": self.config.api_key,
            "OPENAI_BASE_URL": self.config.base_url,
            "OPENAI_MODEL": self.config.model,
            "IRONCLAW_REBORN_WEBUI_TOKEN": self.webui_token,
        }
        self.report = ReportGenerator()

    def _read_token(self) -> str:
        path = Path.home() / ".ironclaw" / "reborn" / "webui-token"
        return path.read_text().strip() if path.exists() else "ontotest-token"

    # ══════════════════════════════════════════════════════════════
    # Suite: unit
    # ══════════════════════════════════════════════════════════════

    def run_unit_suite(self) -> TestSuite:
        suite = TestSuite(name="ontoos-unit", started_at=str(time.time()))
        t0 = time.time()
        result = run_unit_tests()
        suite.cases.append(TestCase(
            name="onto-assurance-core tests",
            status="PASS" if result.get("all_ok", False) else "FAIL",
            duration_seconds=time.time() - t0, details=result,
        ))
        t0 = time.time()
        conf_ok = run_conformance_check()
        suite.cases.append(TestCase(
            name="golden conformance (6 fixtures)",
            status="PASS" if conf_ok else "FAIL",
            duration_seconds=time.time() - t0,
        ))
        suite.finished_at = str(time.time())
        return suite

    # ══════════════════════════════════════════════════════════════
    # Suite: e2e
    # ══════════════════════════════════════════════════════════════

    def run_e2e_suite(self) -> TestSuite:
        suite = TestSuite(name="ironclaw-e2e", started_at=str(time.time()))
        server = OntoRuntimeProcess(self.config)

        # 1. START OntoRuntime
        t0 = time.time()
        # Inject DB URL into environment for the server process
        for k, v in self.server_env.items():
            os.environ.setdefault(k, v)

        if not server.start():
            suite.cases.append(TestCase(
                name="OntoRuntime startup", status="FAIL",
                duration_seconds=time.time() - t0,
                errors=["Server failed to start"],
            ))
            suite.finished_at = str(time.time())
            return suite
        suite.cases.append(TestCase(
            name="OntoRuntime startup", status="PASS",
            duration_seconds=time.time() - t0, details={"pid": server.pid},
        ))

        try:
            # 2. Agent task
            client = AgentClient(self.base_url, self.webui_token)
            task = AgentTask(
                prompt="Write a Python function hello() that returns 'Hello World'. Return ONLY code.",
                requirements=["hello() returns 'Hello World'"],
            )
            t0 = time.time()
            result = client.execute_task(task, timeout_seconds=90)
            suite.cases.append(TestCase(
                name=f"Agent: {task.prompt[:60]}...",
                status="PASS" if result.status == "completed" else "FAIL",
                duration_seconds=result.duration_seconds,
                details={"thread_id": result.thread_id, "status": result.status},
                errors=[result.error] if result.error else [],
            ))

            # 3. Onto hook verification
            t0 = time.time()
            log_text = server.collect_logs()
            evidence = check_server_logs(log_text)
            suite.cases.append(TestCase(
                name="AfterLoopExit hook fired",
                status="PASS" if evidence.hook_fired else "FAIL",
                duration_seconds=time.time() - t0,
                details={"onto_messages": len(evidence.onto_log_messages)},
            ))
            suite.cases.append(TestCase(
                name="RunFinalizationPort called",
                status="PASS" if evidence.finalizer_called else "FAIL",
            ))

            # 4. Evidence integrity
            t0 = time.time()
            verdict = verify_evidence_integrity("success")
            suite.cases.append(TestCase(
                name="Evidence integrity",
                status="PASS" if "error" not in verdict else "FAIL",
                duration_seconds=time.time() - t0, details=verdict,
            ))

        finally:
            # 5. STOP OntoRuntime (always, even on failure)
            t0 = time.time()
            server.stop(graceful=True)
            suite.cases.append(TestCase(
                name="OntoRuntime shutdown", status="PASS",
                duration_seconds=time.time() - t0,
            ))

        suite.finished_at = str(time.time())
        return suite

    # ══════════════════════════════════════════════════════════════

    def run(self, suite_filter: str = "all"):
        if suite_filter in ("all", "unit"):
            self.report.add_suite(self.run_unit_suite())
        if suite_filter in ("all", "e2e"):
            self.report.add_suite(self.run_e2e_suite())

        print(self.report.terminal_report())
        report_dir = Path("/tmp/ontotest")
        report_dir.mkdir(exist_ok=True)
        json_path = report_dir / f"report_{int(time.time())}.json"
        self.report.save_json(str(json_path))
        print(f"\nJSON: {json_path}")

        failed = sum(s.failed for s in self.report.suites)
        sys.exit(0 if failed == 0 else 1)


if __name__ == "__main__":
    import argparse
    p = argparse.ArgumentParser(description="OntoOS Assurance Test Runner")
    p.add_argument("--suite", default="all", choices=["all", "unit", "e2e"])
    args = p.parse_args()
    OntoTestRunner().run(args.suite)
