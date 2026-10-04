#!/usr/bin/env python3
"""B1-v2: Formal OntoOS vs OpenCode comparison with explicit Planned profile.

OntoOS uses the dedicated DeepSeek backend (rig-core native client) which
round-trips reasoning_content for thinking-mode tool calling.
OpenCode uses openai_compatible (no reasoning round-trip needed).

API keys are read from environment, never hardcoded.
"""

import subprocess, sys, os, json, hashlib, shutil, time
from pathlib import Path
from datetime import datetime, timezone

B1_DIR = Path(__file__).resolve().parent
TEMPLATE = B1_DIR / "template"
PROMPT_FILE = B1_DIR / "prompt.txt"
GRADER = B1_DIR / "grader" / "grader.py"

# ── Binaries ──
IRONCLAW = os.environ.get("IRONCLAW_BIN", str(Path.home() / "OntoOS" / "ironclaw" / "target" / "debug" / "ironclaw"))
CLAUDE_CLI = os.environ.get("CLAUDE_BIN", str(Path.home() / ".nvm/versions/node/v22.23.1/bin/claude"))

# ── OntoOS B1-P3 config ──
ONTOS_RUN_PROFILE = "reborn-planned-default"
ONTOS_REQUIRED_CONFIG = {
    "run_profile_resolved": ONTOS_RUN_PROFILE,
    "driver_id": "reborn:planned-default",
    "capability_surface": "interactive_tools",
    "assurance_mode": "required",
}

# ── Provider config from environment ──
DEEPSEEK_API_KEY = os.environ.get("DEEPSEEK_API_KEY", "")
DEEPSEEK_MODEL = os.environ.get("DEEPSEEK_MODEL", "deepseek-v4-pro")
OPENCODE_API_KEY = os.environ.get("OPENCODE_API_KEY", DEEPSEEK_API_KEY)
OPENCODE_MODEL = os.environ.get("OPENCODE_MODEL", DEEPSEEK_MODEL)

ONTOS_PROVIDER_ENV = {
    "LLM_BACKEND": "deepseek",
    "DEEPSEEK_API_KEY": DEEPSEEK_API_KEY,
    "DEEPSEEK_MODEL": DEEPSEEK_MODEL,
}
OPENCODE_PROVIDER_ENV = {
    "LLM_BACKEND": "openai_compatible",
    "LLM_BASE_URL": "https://api.deepseek.com/v1",
    "LLM_API_KEY": OPENCODE_API_KEY,
    "LLM_MODEL": OPENCODE_MODEL,
}

# ── Timeout tiers (seconds) ──
LLM_REQUEST_TIMEOUT = 600
TOOL_TIMEOUT = 900
TASK_HARD_DEADLINE = 3600
STALL_TIMEOUT = 600

def sha256_dir(d):
    h = hashlib.sha256()
    for root, dirs, files in sorted(os.walk(d)):
        dirs.sort()
        for f in sorted(files):
            p = os.path.join(root, f)
            h.update(os.path.relpath(p, d).encode())
            with open(p, 'rb') as fh:
                h.update(fh.read())
    return h.hexdigest()[:16]

def sha256_str(s):
    return hashlib.sha256(s.encode()).hexdigest()[:16]

def setup_repo(name):
    ts = datetime.now().strftime('%H%M%S')
    repo = B1_DIR / "results" / f"b1_{name}_{ts}"
    if repo.exists(): shutil.rmtree(repo)
    shutil.copytree(TEMPLATE, repo)
    (repo / "TASK.md").write_text(PROMPT_FILE.read_text())
    return repo

def parse_startup_config(stderr: str):
    """Parse the B1-P3 machine-parseable startup log from stderr."""
    for line in stderr.split('\n'):
        if line.startswith("run_profile_requested="):
            config = {}
            for part in line.strip().split():
                if '=' in part:
                    k, v = part.split('=', 1)
                    config[k] = v
            return config, line.strip()
    return None, ""

def validate_startup_config(config: dict):
    """Validate resolved config matches required values."""
    failures = []
    if config is None:
        return False, ["startup_config_line not found in stderr"], {}

    for field, expected in ONTOS_REQUIRED_CONFIG.items():
        actual = config.get(field)
        if actual is None:
            failures.append(f"missing field: {field}")
        elif actual != expected:
            failures.append(f"{field}: expected={expected} actual={actual}")

    return len(failures) == 0, failures, config

def count_tool_calls(stderr: str):
    """Count tool invocations from stderr."""
    count = 0
    for line in stderr.split('\n'):
        if 'capability' in line.lower() and ('invoke' in line.lower() or 'result' in line.lower()):
            count += 1
    return count

def extract_p16_metrics(stderr: str):
    """Extract P16 lifecycle metrics from stderr."""
    metrics = {
        "task_outcome_lines": [],
        "attempts": 0,
        "committed_count": 0,
        "continuing_count": 0,
        "failed_count": 0,
        "escalated_count": 0,
    }
    for line in stderr.split('\n'):
        if "task_outcome=" in line and "lifecycle_state=" in line:
            metrics["task_outcome_lines"].append(line.strip()[-300:])
            metrics["attempts"] += 1
            if "lifecycle_state=Committed" in line:
                metrics["committed_count"] += 1
            if "lifecycle_state=Continuing" in line:
                metrics["continuing_count"] += 1
            if "task_outcome=Failed" in line:
                metrics["failed_count"] += 1
            if "lifecycle_state=Escalated" in line:
                metrics["escalated_count"] += 1
    return metrics

def run_ontoos(repo):
    print(f"  OntoOS: starting (profile={ONTOS_RUN_PROFILE}, backend=deepseek, model={DEEPSEEK_MODEL})")
    t0 = time.time()
    env = {
        **os.environ,
        **ONTOS_PROVIDER_ENV,
        "IRONCLAW_REBORN_HOME": str(Path.home() / ".ironclaw" / "reborn"),
    }
    try:
        proc = subprocess.run(
            [IRONCLAW, "run", "--profile", ONTOS_RUN_PROFILE, "--message", PROMPT_FILE.read_text()],
            cwd=str(repo), capture_output=True, text=True,
            timeout=TASK_HARD_DEADLINE, env=env,
        )
        elapsed = time.time() - t0

        startup_config, config_line = parse_startup_config(proc.stderr)
        config_valid, config_failures, config_values = validate_startup_config(startup_config)

        p16 = extract_p16_metrics(proc.stderr)

        result = {
            "system": "OntoOS",
            "exit_code": proc.returncode,
            "elapsed_s": round(elapsed, 1),
            "stdout_tail": proc.stdout[-500:] if len(proc.stdout) > 500 else proc.stdout,
            "stderr_tail": proc.stderr[-500:] if len(proc.stderr) > 500 else proc.stderr,
            "config_valid": config_valid,
            "config_failures": config_failures,
            "config_values": config_values,
            "onto_assurance": "Onto Assurance" in proc.stderr,
            "p16_transition": "P16-T" in proc.stderr,
            "committed": "lifecycle_state=Committed" in proc.stderr,
            "p16_metrics": p16,
        }
        if not config_valid:
            result["run_status"] = "INVALID_CONFIGURATION"
            return result
        return result
    except subprocess.TimeoutExpired:
        return {"system": "OntoOS", "error": f"timeout after {TASK_HARD_DEADLINE}s", "elapsed_s": round(time.time()-t0, 1)}

def run_opencode(repo):
    print(f"  OpenCode: starting (backend=openai_compatible, model={OPENCODE_MODEL})")
    t0 = time.time()
    env = {**os.environ, **OPENCODE_PROVIDER_ENV}
    try:
        proc = subprocess.run(
            [CLAUDE_CLI, "--print", PROMPT_FILE.read_text()],
            cwd=str(repo), capture_output=True, text=True,
            timeout=TASK_HARD_DEADLINE, env=env,
        )
        elapsed = time.time() - t0
        return {
            "system": "OpenCode",
            "exit_code": proc.returncode,
            "elapsed_s": round(elapsed, 1),
            "stdout_tail": proc.stdout[-500:] if len(proc.stdout) > 500 else proc.stdout,
        }
    except subprocess.TimeoutExpired:
        return {"system": "OpenCode", "error": f"timeout after {TASK_HARD_DEADLINE}s", "elapsed_s": round(time.time()-t0, 1)}
    except FileNotFoundError:
        return {"system": "OpenCode", "error": "claude CLI not found", "elapsed_s": 0}

def grade(repo):
    try:
        r = subprocess.run(["python3", str(GRADER), str(repo)],
                          capture_output=True, text=True, timeout=120)
        return json.loads(r.stdout) if r.stdout.strip().startswith("{") else {"error": "not json"}
    except:
        return {"error": "grader failed"}

def main():
    if not DEEPSEEK_API_KEY:
        print("Error: DEEPSEEK_API_KEY environment variable is required.")
        sys.exit(1)

    prompt = PROMPT_FILE.read_text()
    prompt_hash = sha256_str(prompt)
    template_hash = sha256_dir(TEMPLATE)

    print(f"=== B1 Formal Benchmark ===")
    print(f"Prompt hash:  {prompt_hash}")
    print(f"Template hash: {template_hash}")
    print(f"OntoOS:  backend=deepseek model={DEEPSEEK_MODEL} profile={ONTOS_RUN_PROFILE}")
    print(f"OpenCode: backend=openai_compatible model={OPENCODE_MODEL}")
    print(f"Task deadline: {TASK_HARD_DEADLINE}s")
    print(f"Required startup config: {ONTOS_REQUIRED_CONFIG}\n")

    results = []
    for runner in [run_opencode, run_ontoos]:
        repo = setup_repo(runner.__name__.split('_')[1])
        initial_hash = sha256_dir(repo)
        print(f"  Initial hash: {initial_hash}")

        result = runner(repo)

        # Config validation gate
        if not result.get("config_valid", True):
            result["run_status"] = "INVALID_CONFIGURATION"
            result["grade"] = {"error": "INVALID_CONFIGURATION"}
            result["hidden_passed"] = 0
            result["build_ok"] = False
            results.append(result)
            print(f"    *** INVALID_CONFIGURATION: {result.get('config_failures', [])} ***\n")
            continue

        result["initial_hash"] = initial_hash
        result["final_hash"] = sha256_dir(repo)

        grade_result = grade(repo)
        result["grade"] = grade_result
        result["hidden_passed"] = grade_result.get("hidden_tests_passed", 0) if isinstance(grade_result, dict) else 0
        result["build_ok"] = grade_result.get("build_success", False) if isinstance(grade_result, dict) else False

        results.append(result)
        print(f"    elapsed: {result.get('elapsed_s', 0):.0f}s")
        if result["system"] == "OntoOS":
            p = result.get("p16_metrics", {})
            print(f"    P16: attempts={p.get('attempts',0)} committed={p.get('committed_count',0)} "
                  f"continuing={p.get('continuing_count',0)} failed={p.get('failed_count',0)}")
        print(f"    grade: hidden={result['hidden_passed']}/10 build={result['build_ok']}")
        print()

    # Summary
    ts = datetime.now().strftime('%Y%m%d_%H%M%S')
    results_file = B1_DIR / "results" / f"b1_{ts}.json"
    results_file.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "meta": {
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "prompt_hash": prompt_hash,
            "template_hash": template_hash,
            "ontoos_profile": ONTOS_RUN_PROFILE,
            "ontoos_model": DEEPSEEK_MODEL,
            "opencode_model": OPENCODE_MODEL,
            "deadline_s": TASK_HARD_DEADLINE,
        },
        "results": results,
    }
    results_file.write_text(json.dumps(payload, indent=2, default=str))
    print(f"Results: {results_file}")

    # Quick summary
    for r in results:
        status = r.get("run_status", "OK")
        print(f"  {r['system']:12s} status={status} hidden={r.get('hidden_passed','?')}/10 "
              f"build={r.get('build_ok','?')} elapsed={r.get('elapsed_s','?')}s")

if __name__ == "__main__":
    main()
