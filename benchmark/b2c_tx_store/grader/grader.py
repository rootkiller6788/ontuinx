#!/usr/bin/env python3
"""B2-C grader: build + test transactional store on a candidate directory.

Usage: grader.py <project_dir> [--expected-digest <sha256>]
"""
import subprocess, sys, json, shutil, hashlib, argparse, os
from pathlib import Path

def sha256_dir(d):
    h = hashlib.sha256()
    for root, dirs, files in sorted(os.walk(d)):
        dirs.sort()
        for f in sorted(files):
            p = os.path.join(root, f)
            h.update(os.path.relpath(p, d).encode())
            with open(p, 'rb') as fh: h.update(fh.read())
    return h.hexdigest()[:16]

def grade(project_dir, expected_digest=None):
    proj = Path(project_dir).resolve()
    results = {"build_success": False, "asan_clean": False, "tests_passed": 0, "tests_total": 0,
               "files_missing": [], "success": False, "failure_stage": None, "error": None,
               "test_binary_exit": None, "candidate_digest": sha256_dir(str(proj))}

    if expected_digest and results["candidate_digest"] != expected_digest:
        results["failure_stage"] = "DIGEST_MISMATCH"
        results["error"] = f"expected={expected_digest} actual={results['candidate_digest']}"
        return results

    for f in ["include/store.h", "src/store.c", "tests/test_store.c", "CMakeLists.txt"]:
        if not (proj / f).exists(): results["files_missing"].append(f)
    if results["files_missing"]:
        results["failure_stage"] = "MISSING_FILES"
        return results

    build_dir = proj / "build"
    shutil.rmtree(str(build_dir), ignore_errors=True)
    build_dir.mkdir(parents=True, exist_ok=True)
    r = subprocess.run(["cmake", str(proj), "-DCMAKE_BUILD_TYPE=Debug",
        "-DCMAKE_C_FLAGS=-fsanitize=address,undefined -g -O0"],
        cwd=str(build_dir), capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        results["failure_stage"] = "BUILD_FAILURE"; results["error"] = f"cmake: {r.stderr[:300]}"; return results
    r = subprocess.run(["make"], cwd=str(build_dir), capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        results["failure_stage"] = "BUILD_FAILURE"; results["error"] = f"make: {r.stderr[:300]}"; return results
    results["build_success"] = True
    r = subprocess.run([str(build_dir / "test_store")], cwd=str(proj), capture_output=True, text=True, timeout=15)
    results["test_binary_exit"] = r.returncode
    combined = r.stdout + r.stderr
    for line in r.stdout.split('\n'):
        if "tests passed" in line.lower():
            parts = line.strip().split('/')
            if len(parts) >= 2:
                try:
                    results["tests_passed"] = int(parts[0].split()[-1])
                    results["tests_total"] = int(parts[1].split()[0])
                except ValueError: pass
    if "ERROR: AddressSanitizer" in combined or "runtime error" in combined.lower():
        results["asan_clean"] = False; results["failure_stage"] = "SANITIZER_FAILURE"; return results
    results["asan_clean"] = True
    if results["tests_total"] == 0:
        results["failure_stage"] = "TEST_FAILURE"; results["error"] = f"no test output; exit={r.returncode}"; return results
    results["success"] = results["tests_passed"] == results["tests_total"]
    if not results["success"]: results["failure_stage"] = "TEST_FAILURE"
    return results

if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("project_dir")
    p.add_argument("--expected-digest", default=None)
    args = p.parse_args()
    r = grade(args.project_dir, args.expected_digest)
    print(json.dumps(r, separators=(",", ":")))
    sys.exit(0 if r["success"] else 1)
