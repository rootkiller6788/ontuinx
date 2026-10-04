#!/usr/bin/env python3
"""B2-A grader: compile + ASan test for kvstore library.
Always prints a stable JSON document on stdout (success, failure_stage,
build_success, asan_clean, tests_passed, tests_total, error)."""
import subprocess, sys, os, json
from pathlib import Path

def grade(project_dir):
    proj = Path(project_dir).resolve()
    build_dir = proj / "build"
    results = {
        "build_success": False, "asan_clean": False,
        "tests_passed": 0, "tests_total": 0,
        "files_missing": [], "success": False,
        "failure_stage": None, "error": None,
        "test_binary_exit": None,
    }
    for f in ["kvstore.h", "kvstore.c", "test_kvstore.c", "CMakeLists.txt"]:
        if not (proj / f).exists():
            results["files_missing"].append(f)
    if results["files_missing"]:
        results["failure_stage"] = "MISSING_FILES"
        results["success"] = False
        return results

    # Clean build dir each call
    import shutil
    shutil.rmtree(str(build_dir), ignore_errors=True)
    build_dir.mkdir(parents=True, exist_ok=True)

    # CMake configure + build
    r = subprocess.run(["cmake", str(proj), "-DCMAKE_BUILD_TYPE=Debug",
        "-DCMAKE_C_FLAGS=-fsanitize=address,undefined -g -O0"],
        cwd=str(build_dir), capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        results["failure_stage"] = "BUILD_FAILURE"
        results["error"] = f"cmake: {r.stderr[:300]}"
        return results
    r = subprocess.run(["make"], cwd=str(build_dir), capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        results["failure_stage"] = "BUILD_FAILURE"
        results["error"] = f"make: {r.stderr[:300]}"
        return results
    results["build_success"] = True

    # Run test. Run from proj root (test writes+reads test.conf in CWD), with
    # the built binary path explicit — avoids cwd ambiguity.
    r = subprocess.run([str(build_dir / "test_kvstore")],
        cwd=str(proj), capture_output=True, text=True, timeout=15)
    results["test_binary_exit"] = r.returncode
    combined = r.stdout + r.stderr
    for line in r.stdout.split('\n'):
        if "tests passed" in line.lower():
            parts = line.strip().split('/')
            if len(parts) >= 2:
                try:
                    results["tests_passed"] = int(parts[0].split()[-1])
                    results["tests_total"] = int(parts[1].split()[0])
                except ValueError:
                    pass
    if "ERROR: AddressSanitizer" in combined or "runtime error" in combined.lower():
        results["asan_clean"] = False
        results["failure_stage"] = "SANITIZER_FAILURE"
        results["success"] = False
        return results
    results["asan_clean"] = True

    if results["tests_total"] == 0:
        results["failure_stage"] = "TEST_FAILURE"
        results["error"] = f"no test output parsed; exit={r.returncode}"
        results["success"] = False
        return results

    results["success"] = results["tests_passed"] == results["tests_total"]
    if not results["success"]:
        results["failure_stage"] = "TEST_FAILURE"
    return results

if __name__ == "__main__":
    d = sys.argv[1] if len(sys.argv) > 1 else "."
    r = grade(d)
    # Single-line JSON so the wrapper's command substitution captures it whole.
    print(json.dumps(r, separators=(",", ":")))
    sys.exit(0 if r["success"] else 1)
