#include "kv_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

static int tests = 0, passed = 0;
#define T(name) do { tests++; printf("  %s ... ", name); } while(0)
#define P() do { passed++; printf("PASS\n"); } while(0)
#define F(m) do { printf("FAIL: %s\n", m); } while(0)

int main(void) {
    printf("KV_CONFIG TESTS\n===============\n");

    /* write test config */
    FILE *f = fopen("test.conf", "w");
    fprintf(f, "host=localhost\nport=8080\ndebug=true\n\n# comment\npath=/tmp\n");
    fclose(f);

    T("parse valid config");
    kv_config *c = kv_config_parse("test.conf");
    assert(c); P();

    T("get existing key");
    assert(strcmp(kv_config_get(c, "host"), "localhost") == 0); P();

    T("get numeric key");
    assert(strcmp(kv_config_get(c, "port"), "8080") == 0); P();

    T("get missing key");
    assert(kv_config_get(c, "nonexistent") == NULL); P();

    T("count entries");
    assert(kv_config_count(c) == 4); P();

    T("parse nonexistent file");
    assert(kv_config_parse("/nonexistent") == NULL); P();

    T("destroy does not crash");
    kv_config_destroy(c); P();

    printf("\n%d/%d tests passed\n", passed, tests);
    return tests == passed ? 0 : 1;
}
