#include "kvstore.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

static int tests_run = 0, tests_passed = 0;
#define T(name) do { tests_run++; printf("  %s ... ", name); fflush(stdout); } while(0)
#define P() do { tests_passed++; printf("PASS\n"); } while(0)
#define FAIL(msg) do { printf("FAIL: %s\n", msg); } while(0)

int main(void) {
    printf("KVSTORE TESTS\n=============\n");

    T("put and get"); {
        kvstore *s = kvstore_create();
        assert(kvstore_put(s, "a", "1"));
        assert(strcmp(kvstore_get(s, "a"), "1") == 0);
        kvstore_destroy(s);
        P();
    }

    T("delete removes key"); {
        kvstore *s = kvstore_create();
        kvstore_put(s, "x", "v");
        assert(kvstore_delete(s, "x"));
        assert(kvstore_get(s, "x") == NULL);
        kvstore_destroy(s);
        P();
    }

    T("count after put/delete"); {
        kvstore *s = kvstore_create();
        kvstore_put(s, "k1", "v1");
        kvstore_put(s, "k2", "v2");
        assert(kvstore_count(s) == 2);
        kvstore_delete(s, "k1");
        assert(kvstore_count(s) == 1);
        kvstore_destroy(s);
        P();
    }

    T("batch atomic: all succeed"); {
        kvstore *s = kvstore_create();
        const char *ks[] = {"a", "b", "c"};
        const char *vs[] = {"1", "2", "3"};
        assert(kvstore_put_batch(s, ks, vs, 3));
        assert(kvstore_count(s) == 3);
        kvstore_destroy(s);
        P();
    }

    T("batch partial fails without side effects"); {
        /* Fill store to near capacity */
        kvstore *s = kvstore_create();
        char buf[16];
        for (int i = 0; i < 250; i++) {
            sprintf(buf, "k%d", i);
            kvstore_put(s, buf, "v");
        }
        size_t before = kvstore_count(s);
        /* Try to batch-add 10 — should fail due to MAX_KEYS=256 */
        const char *ks[] = {"a", "b", "c", "d", "e", "f", "g", "h", "i", "j"};
        const char *vs[] = {"1", "2", "3", "4", "5", "6", "7", "8", "9", "10"};
        bool ok = kvstore_put_batch(s, ks, vs, 10);
        (void)ok;
        /* After failed batch, count must be unchanged — no partial commit */
        assert(kvstore_count(s) == before);
        kvstore_destroy(s);
        P();
    }

    T("merge overrides existing"); {
        kvstore *a = kvstore_create();
        kvstore *b = kvstore_create();
        kvstore_put(a, "shared", "old");
        kvstore_put(a, "only_a", "a");
        kvstore_put(b, "shared", "new");
        kvstore_put(b, "only_b", "b");
        int n = kvstore_merge(a, b);
        assert(n == 2);
        assert(strcmp(kvstore_get(a, "shared"), "new") == 0);
        assert(strcmp(kvstore_get(a, "only_b"), "b") == 0);
        kvstore_destroy(a);
        kvstore_destroy(b);
        P();
    }

    printf("\n%d/%d tests passed\n", tests_passed, tests_run);
    return tests_passed == tests_run ? 0 : 1;
}
