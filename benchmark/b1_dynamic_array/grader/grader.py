#!/usr/bin/env python3
"""B1 external grader for C11 dynamic array library.

Independent of both OpenCode and OntoOS.
Evaluates the project directory for correctness, completeness, and safety.

Usage: python3 grader.py <project_dir>
Exit: 0 = all hidden tests pass
"""

import subprocess, sys, os, json, re, hashlib
from pathlib import Path

HIDDEN_TESTS_C = r"""
#include "darray.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

static int malloc_count = 0;
static int free_count = 0;
static int realloc_count = 0;
static size_t total_allocated = 0;

static void* counting_malloc(size_t s) { malloc_count++; total_allocated += s; return malloc(s); }
static void counting_free(void* p) { free_count++; free(p); }
static void* counting_realloc(void* p, size_t s) { realloc_count++; total_allocated += s; return realloc(p, s); }

static int tests_run = 0;
static int tests_passed = 0;
#define TEST(name) do { tests_run++; printf("  %s ... ", name); } while(0)
#define PASS() do { tests_passed++; printf("PASS\n"); } while(0)
#define FAIL(msg) do { printf("FAIL: %s\n", msg); } while(0)

static int alloc_counts_reset() { int r = malloc_count + free_count + realloc_count; malloc_count=free_count=realloc_count=0; return r; }

int main(void) {
    darray* da;
    int val, out;
    double dval, dout;
    typedef struct { int x; double y; } Point;
    Point p, pout;

    printf("HIDDEN TESTS\n============\n");

    // H1: create/destroy basics
    TEST("create/destroy int");
    da = darray_create(sizeof(int), 4);
    darray_destroy(da);
    PASS();

    // H2: push/pop int
    TEST("push/pop int");
    da = darray_create(sizeof(int), 4);
    val = 42; assert(!darray_push(da, &val));
    val = 99; assert(!darray_push(da, &val));
    out = 0;  assert(!darray_pop(da, &out)); assert(out == 99);
    out = 0;  assert(!darray_pop(da, &out)); assert(out == 42);
    darray_destroy(da);
    PASS();

    // H3: push/pop double
    TEST("push/pop double");
    da = darray_create(sizeof(double), 4);
    dval = 3.14; assert(!darray_push(da, &dval));
    dval = 2.718; assert(!darray_push(da, &dval));
    dout = 0; assert(!darray_pop(da, &dout)); assert(dout == 2.718);
    darray_destroy(da);
    PASS();

    // H4: insert/remove
    TEST("insert/remove at positions");
    da = darray_create(sizeof(int), 4);
    val=10; darray_push(da,&val); val=20; darray_push(da,&val); val=30; darray_push(da,&val);
    val=15; assert(!darray_insert(da, 1, &val)); // [10,15,20,30]
    darray_get(da, 1, &out); assert(out == 15);
    darray_get(da, 2, &out); assert(out == 20);
    assert(!darray_remove(da, 2, &out)); assert(out == 20); // [10,15,30]
    assert(!darray_remove(da, 0, &out)); assert(out == 10); // [15,30]
    assert(darray_size(da) == 2);
    darray_destroy(da);
    PASS();

    // H5: reserve/shrink
    TEST("reserve and shrink");
    da = darray_create(sizeof(int), 2);
    assert(darray_capacity(da) >= 2);
    assert(!darray_reserve(da, 100));
    assert(darray_capacity(da) >= 100);
    for (int i=0; i<100; i++) { int v=i; darray_push(da, &v); }
    assert(!darray_shrink(da));
    assert(darray_capacity(da) == 100);
    darray_destroy(da);
    PASS();

    // H6: custom allocator
    TEST("custom allocator tracking");
    da = darray_create(sizeof(int), 4);
    darray_set_allocator(da, counting_malloc, counting_free, counting_realloc);
    for (int i=0; i<50; i++) { int v=i; darray_push(da, &v); }
    assert(malloc_count > 0 || realloc_count > 0); // must have allocated
    assert(darray_size(da) == 50);
    alloc_counts_reset();
    darray_destroy(da);
    assert(free_count > 0); // must have freed
    PASS();

    // H7: overflow protection
    TEST("overflow protection");
    da = darray_create(sizeof(int), 4);
    int rc = darray_reserve(da, (size_t)-1);
    assert(rc != 0); // must reject overflow
    assert(darray_capacity(da) >= 4); // unchanged
    darray_destroy(da);
    PASS();

    // H8: error codes
    TEST("error codes");
    da = darray_create(sizeof(int), 4);
    assert(darray_get(da, 999, &out) != 0); // out of bounds
    assert(darray_set(da, 999, &val) != 0);
    assert(darray_insert(da, 999, &val) != 0);
    assert(darray_remove(da, 999, &out) != 0);
    assert(darray_pop(da, &out) != 0); // empty
    assert(darray_get(NULL, 0, &out) != 0); // null
    darray_destroy(da);
    PASS();

    // H9: struct elements
    TEST("struct element type");
    da = darray_create(sizeof(Point), 4);
    p.x=1; p.y=2.0; darray_push(da, &p);
    p.x=3; p.y=4.0; darray_push(da, &p);
    darray_get(da, 1, &pout);
    assert(pout.x == 3 && pout.y == 4.0);
    darray_destroy(da);
    PASS();

    // H10: empty array edge cases (capacity=0 is valid; implementation may use a minimum)
    TEST("empty array edge cases");
    da = darray_create(sizeof(int), 0);
    assert(da != NULL);
    assert(darray_size(da) == 0);
    assert(darray_pop(da, &out) != 0); // pop from empty
    assert(darray_shrink(da) == 0); // shrink empty
    darray_destroy(da);
    PASS();

    printf("\n%d/%d hidden tests passed\n", tests_passed, tests_run);
    return tests_passed == tests_run ? 0 : 1;
}
"""

def sha256_file(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        while True:
            chunk = f.read(8192)
            if not chunk: break
            h.update(chunk)
    return h.hexdigest()

def grade(project_dir):
    proj = Path(project_dir).resolve()
    results = {
        'project_dir': str(proj),
        'hidden_tests_passed': 0,
        'hidden_tests_total': 10,
        'build_success': False,
        'asan_clean': False,
        'files_present': [],
        'files_missing': [],
        'warnings': [],
    }

    # Check required files exist
    required = ['darray.h', 'darray.c', 'test_darray.c', 'CMakeLists.txt', 'README.md']
    for f in required:
        if (proj / f).exists():
            results['files_present'].append(f)
        else:
            results['files_missing'].append(f)

    # Check darray.h has error code defines
    header = (proj / 'darray.h')
    if header.exists():
        content = header.read_text()
        for code in ['ENOMEM', 'EOVERFLOW', 'EINDEX', 'ENULL']:
            if f'DARRAY_{code}' not in content:
                results['warnings'].append(f'Missing DARRAY_{code} in header')

    # Check darray.c does NOT include illegal patterns
    source = (proj / 'darray.c')
    if source.exists():
        content = source.read_text()
        # No VLAs
        if re.search(r'\b(static|auto|const)?\s*\w+\s+\w+\[[a-zA-Z_]\w*\]', content):
            results['warnings'].append('Possible VLA usage in darray.c')

    # Write hidden tests into project root (not build/) so it can #include "darray.h"
    hidden_test_file = proj / 'hidden_test.c'
    hidden_test_file.write_text(HIDDEN_TESTS_C)

    # Build darray library + hidden test as a standalone binary.
    # We compile directly (bypassing CMake for the hidden test) to avoid
    # coupling to the agent's CMakeLists.txt structure.
    build_dir = proj / 'build'
    build_dir.mkdir(exist_ok=True)
    hidden_binary = build_dir / 'hidden_test'

    cc = os.environ.get('CC', 'cc')
    cflags = ['-std=c11', '-Wall', '-Wextra', '-pedantic', '-g', '-O0',
              '-fsanitize=address,undefined', '-I', str(proj)]
    srcs = [str(proj / 'darray.c'), str(hidden_test_file)]
    compile_cmd = [cc] + cflags + ['-o', str(hidden_binary)] + srcs + ['-lm']

    try:
        r = subprocess.run(compile_cmd, capture_output=True, text=True, timeout=30)
        if r.returncode != 0:
            results['warnings'].append(f'hidden_test compile failed: {r.stderr[:300]}')
            # Still try CMake build for the agent's own tests
        else:
            results['build_success'] = True  # hidden test compiles = darray.c is valid C11
    except Exception as e:
        results['warnings'].append(f'hidden_test compile error: {e}')

    # Also run CMake + make for the agent's own test suite (separate from hidden tests)
    try:
        r = subprocess.run(['cmake', str(proj), '-DCMAKE_BUILD_TYPE=Debug',
                           '-DCMAKE_C_FLAGS=-fsanitize=address,undefined -g -O0'],
                          cwd=str(build_dir), capture_output=True, text=True, timeout=30)
        if r.returncode != 0:
            results['warnings'].append(f'cmake failed: {r.stderr[:200]}')
    except Exception as e:
        results['warnings'].append(f'cmake error: {e}')

    try:
        r = subprocess.run(['make'], cwd=str(build_dir), capture_output=True, text=True, timeout=30)
        if r.returncode == 0:
            results['build_success'] = True
    except Exception as e:
        results['warnings'].append(f'make error: {e}')

    # Run hidden tests (standalone binary)
    if hidden_binary.exists():
        try:
            r = subprocess.run([str(hidden_binary)], capture_output=True, text=True, timeout=10)
            for line in r.stdout.split('\n'):
                if 'hidden tests passed' in line.lower():
                    m = re.search(r'(\d+)/(\d+)', line)
                    if m:
                        results['hidden_tests_passed'] = int(m.group(1))
                        results['hidden_tests_total'] = int(m.group(2))
            # Per-test failure detail
            for line in r.stdout.split('\n'):
                if 'FAIL:' in line:
                    results.setdefault('failure_details', []).append(line.strip())
            # ASan check on hidden test output
            if 'ERROR: AddressSanitizer' in (r.stdout + r.stderr) or 'runtime error' in (r.stdout + r.stderr).lower():
                results['asan_clean'] = False
            else:
                results['asan_clean'] = True
        except Exception as e:
            results['warnings'].append(f'hidden_test run error: {e}')
    else:
        results['warnings'].append('hidden_test binary not built')

    return results

if __name__ == '__main__':
    project_dir = sys.argv[1] if len(sys.argv) > 1 else '.'
    results = grade(project_dir)
    print(json.dumps(results, indent=2))
    sys.exit(0 if results['hidden_tests_passed'] == results['hidden_tests_total'] and results['build_success'] else 1)
