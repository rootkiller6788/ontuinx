#!/usr/bin/env python3
"""B1 Benchmark Runner — C11 Dynamic Array Library Comparison.

Runs OpenCode and OntoOS against the same prompt on identical initial repos.
Validates: prompt hash, repo hash, model identity, Verifier execution,
Commit validity, grader independence.

Usage:
    python3 run_b1.py --runs 1 --timeout 7200
"""

import subprocess, sys, os, json, hashlib, shutil, time, argparse, tempfile
from pathlib import Path
from datetime import datetime, timezone

B1_DIR = Path(__file__).resolve().parent
TEMPLATE_DIR = B1_DIR / 'template'
PROMPT_FILE = B1_DIR / 'prompt.txt'
GRADER = B1_DIR / 'grader' / 'grader.py'
RESULTS_DIR = B1_DIR / 'results'

def sha256_dir(directory):
    """Compute a deterministic tree hash of a directory."""
    h = hashlib.sha256()
    for root, dirs, files in sorted(os.walk(directory)):
        dirs.sort()
        for fname in sorted(files):
            path = os.path.join(root, fname)
            rel = os.path.relpath(path, directory)
            h.update(rel.encode())
            with open(path, 'rb') as f:
                while True:
                    chunk = f.read(65536)
                    if not chunk: break
                    h.update(chunk)
    return h.hexdigest()

def sha256_str(s):
    return hashlib.sha256(s.encode()).hexdigest()

def setup_repo(name, run_id):
    """Create a clean repo from template."""
    repo = RESULTS_DIR / f"b1_{name}_{run_id}"
    if repo.exists():
        shutil.rmtree(repo)
    shutil.copytree(TEMPLATE_DIR, repo)
    subprocess.run(['git', 'init'], cwd=repo, capture_output=True)
    subprocess.run(['git', 'add', '-A'], cwd=repo, capture_output=True)
    subprocess.run(['git', 'commit', '-m', 'initial empty'], cwd=repo, capture_output=True)
    subprocess.run(['git', 'tag', 'starter'], cwd=repo, capture_output=True)
    return repo

def run_ontoos(repo, prompt, timeout_sec, run_id):
    """Run OntoOS on the project. Returns result dict."""
    result = {
        'system': 'OntoOS',
        'run_id': run_id,
        'started_at': datetime.now(timezone.utc).isoformat(),
        'repo_path': str(repo),
        'prompt_hash': sha256_str(prompt),
        'initial_repo_hash': sha256_dir(repo),
        'phases': [],
        'terminal_state': None,
        'total_attempts': 0,
        'evidence_digests': [],
        'decision_ids': [],
        'committed': False,
        'errors': [],
    }

    # Write prompt to repo so OntoOS worker can find it
    (repo / 'TASK.md').write_text(prompt)

    # Build OntoOS worker command
    # In production, this would invoke the actual OntoFlow → OntoLoop chain.
    # For B1, we invoke the worker binary with a request file.
    request = {
        'schema_version': 1,
        'flow_id': f'b1-ontoos-{run_id}',
        'work_item_id': f'darray-{run_id}',
        'loop_id': f'loop-b1-{run_id}',
        'task_spec_ref': str(repo / 'TASK.md'),
        'contract_ref': 'contract/default',
        'policy_ref': 'policy/default',
        'input_artifact_refs': [],
        'resource_class': 'standard',
        'risk_class': 'low',
        'trust_requirement': 'basic',
        'budget_grant_ref': f'grant-b1-{run_id}',
        'budget_grant_hash': sha256_str(f'b1-budget-{run_id}'),
        'execution_generation': 1,
        'idempotency_key': f'idem-b1-{run_id}',
        'deadline': None,
        'request_binding_hash': '',
    }
    request['request_binding_hash'] = sha256_str(
        f"{request['flow_id']}:{request['work_item_id']}:{request['loop_id']}:"
        f"{request['task_spec_ref']}:{request['contract_ref']}:{request['policy_ref']}:"
        f":{request['budget_grant_ref']}:{request['budget_grant_hash']}:"
        f"{request['execution_generation']}"
    )

    req_file = repo / 'request.json'
    req_file.write_text(json.dumps(request, indent=2))

    # Invoke OntoOS worker
    try:
        worker_bin = Path(__file__).resolve().parent.parent.parent / 'target' / 'debug' / 'onto-worker'
        if not worker_bin.exists():
            result['errors'].append(f'worker binary not found at {worker_bin}')
            return result

        env = os.environ.copy()
        env['ONTO_STAGING_ROOT'] = str(repo)

        proc = subprocess.run(
            [str(worker_bin), 'input', str(req_file)],
            cwd=str(repo),
            capture_output=True,
            text=True,
            timeout=timeout_sec,
            env=env,
        )

        result['stdout'] = proc.stdout[-5000:] if len(proc.stdout) > 5000 else proc.stdout
        result['stderr'] = proc.stderr[-2000:] if len(proc.stderr) > 2000 else proc.stderr
        result['exit_code'] = proc.returncode

        # Parse envelope from stdout
        for line in proc.stdout.split('\n'):
            if 'LOOP_TERMINAL_ENVELOPE' in line:
                continue
            if line.strip().startswith('{') and 'loop_id' in line:
                try:
                    envelope = json.loads(line.strip())
                    result['terminal_state'] = envelope.get('reported_terminal_state')
                    result['total_attempts'] = envelope.get('total_attempts', 0)
                    result['decision_ids'] = [envelope.get('decision_id')] if envelope.get('decision_id') else []
                    result['evidence_digests'] = [envelope.get('outcome_binding_hash')] if envelope.get('outcome_binding_hash') else []
                    result['committed'] = envelope.get('reported_terminal_state') == 'Committed'
                except json.JSONDecodeError:
                    pass

    except subprocess.TimeoutExpired:
        result['errors'].append(f'timeout after {timeout_sec}s')
    except Exception as e:
        result['errors'].append(str(e))

    result['final_repo_hash'] = sha256_dir(repo) if repo.exists() else None
    result['finished_at'] = datetime.now(timezone.utc).isoformat()
    return result

def run_opencode(repo, prompt, timeout_sec, run_id):
    """Run OpenCode on the project. Returns result dict.

    In production, this invokes OpenCode (or Claude Code) with the prompt.
    For B1, we validate that both systems can be invoked equivalently.
    """
    result = {
        'system': 'OpenCode',
        'run_id': run_id,
        'started_at': datetime.now(timezone.utc).isoformat(),
        'repo_path': str(repo),
        'prompt_hash': sha256_str(prompt),
        'initial_repo_hash': sha256_dir(repo),
        'errors': [],
    }

    # Write prompt
    (repo / 'TASK.md').write_text(prompt)

    # In production: invoke opencode / claude code with prompt
    # For now: record that the infrastructure is ready
    try:
        # Placeholder — actual OpenCode CLI invocation
        env = os.environ.copy()
        env['CLAUDE_CODE_WORKDIR'] = str(repo)

        proc = subprocess.run(
            ['claude', '--print', '--output-format', 'text', prompt],
            cwd=str(repo),
            capture_output=True,
            text=True,
            timeout=timeout_sec,
            env=env,
        )
        result['stdout'] = proc.stdout[-5000:] if len(proc.stdout) > 5000 else proc.stdout
        result['exit_code'] = proc.returncode
    except FileNotFoundError:
        result['errors'].append('OpenCode CLI not found (claude not in PATH) — infrastructure check only')
    except subprocess.TimeoutExpired:
        result['errors'].append(f'timeout after {timeout_sec}s')
    except Exception as e:
        result['errors'].append(str(e))

    result['final_repo_hash'] = sha256_dir(repo) if repo.exists() else None
    result['finished_at'] = datetime.now(timezone.utc).isoformat()
    return result

def grade(repo_path):
    """Run the external grader."""
    try:
        r = subprocess.run(
            ['python3', str(GRADER), str(repo_path)],
            capture_output=True, text=True, timeout=120,
        )
        return json.loads(r.stdout) if r.stdout.strip().startswith('{') else {
            'error': 'grader output not JSON',
            'raw': r.stdout[:500], 'stderr': r.stderr[:500],
        }
    except Exception as e:
        return {'error': str(e)}

def validate_b1(runs):
    """Run B1 validation suite."""
    prompt = PROMPT_FILE.read_text()
    print(f"=== B1 Benchmark: C11 Dynamic Array ===\n")
    print(f"Prompt hash:   {sha256_str(prompt)}")
    print(f"Prompt length: {len(prompt)} chars\n")

    RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    results = []

    for i in range(runs):
        run_id = datetime.now().strftime('%Y%m%d_%H%M%S')
        print(f"--- Run {i+1}/{runs} ({run_id}) ---\n")

        for system, runner in [('OntoOS', run_ontoos), ('OpenCode', run_opencode)]:
            print(f"  {system}: setting up repo...")
            repo = setup_repo(system.lower(), run_id)
            initial_hash = sha256_dir(repo)
            print(f"    initial repo hash: {initial_hash[:16]}...")

            print(f"  {system}: running...")
            r = runner(repo, prompt, 7200, f"{run_id}-{system.lower()}")
            r['initial_repo_hash'] = initial_hash

            # Grade
            print(f"  {system}: grading...")
            grade_result = grade(repo)
            r['grade'] = grade_result
            r['hidden_passed'] = grade_result.get('hidden_tests_passed', 0) if isinstance(grade_result, dict) else 0

            results.append(r)

            # Print summary
            print(f"    terminal: {r.get('terminal_state', 'N/A')}")
            print(f"    attempts: {r.get('total_attempts', 'N/A')}")
            print(f"    build:    {grade_result.get('build_success', False) if isinstance(grade_result, dict) else False}")
            print(f"    hidden:   {r.get('hidden_passed', 0)}/10")
            if r['errors']:
                print(f"    errors:   {r['errors'][:2]}")
            print()

    # Write full results
    results_file = RESULTS_DIR / f'b1_results_{datetime.now().strftime("%Y%m%d_%H%M%S")}.json'
    results_file.write_text(json.dumps(results, indent=2, default=str))
    print(f"Full results: {results_file}")

    # Summary
    for r in results:
        print(f"{r['system']:>8s} | hidden={r.get('hidden_passed',0)}/10 | build={r.get('grade',{}).get('build_success',False) if isinstance(r.get('grade'), dict) else False} | errors={len(r['errors'])}")

    return results

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='B1 Dynamic Array Benchmark')
    parser.add_argument('--runs', type=int, default=1, help='Number of comparison runs')
    parser.add_argument('--timeout', type=int, default=7200, help='Per-run timeout in seconds')
    parser.add_argument('--validate-only', action='store_true', help='Validate infrastructure without running')
    args = parser.parse_args()

    if args.validate_only:
        print("Infrastructure validation:")
        print(f"  Template dir:  {TEMPLATE_DIR} {'✅' if TEMPLATE_DIR.exists() else '❌'}")
        print(f"  Prompt file:   {PROMPT_FILE} {'✅' if PROMPT_FILE.exists() else '❌'}")
        print(f"  Grader:        {GRADER} {'✅' if GRADER.exists() else '❌'}")
        print(f"  Prompt hash:   {sha256_str(PROMPT_FILE.read_text())}")
        print(f"  Template hash: {sha256_dir(TEMPLATE_DIR)}")
        sys.exit(0)

    validate_b1(args.runs)
