#include "store.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

static int tests_run = 0, tests_passed = 0;
#define T(name) do { tests_run++; printf("  %s ... ", name); fflush(stdout); } while(0)
#define P() do { tests_passed++; printf("PASS\n"); } while(0)
#define FAIL(msg) do { printf("FAIL: %s\n", msg); } while(0)

int main(void) {
    printf("B2-C TRANSACTIONAL STORE TESTS\n==============================\n");

    T("basic put/get"); {
        store *s = store_create();
        assert(store_put(s, "a", "1"));
        assert(strcmp(store_get(s, "a"), "1") == 0);
        store_destroy(s);
        P();
    }

    T("transaction commit"); {
        store *s = store_create();
        store_begin(s);
        store_put(s, "x", "100");
        store_commit(s);
        assert(strcmp(store_get(s, "x"), "100") == 0);
        store_destroy(s);
        P();
    }

    T("transaction rollback restores state"); {
        store *s = store_create();
        store_put(s, "k", "original");
        store_begin(s);
        store_put(s, "k", "modified");
        store_rollback(s);
        /* After rollback, value must be original */
        const char *v = store_get(s, "k");
        assert(v && strcmp(v, "original") == 0);
        store_destroy(s);
        P();
    }

    T("rollback removes new key"); {
        store *s = store_create();
        store_begin(s);
        store_put(s, "newkey", "v");
        store_rollback(s);
        assert(store_get(s, "newkey") == NULL);
        store_destroy(s);
        P();
    }

    T("rollback restores deleted key"); {
        store *s = store_create();
        store_put(s, "delme", "value");
        store_begin(s);
        store_delete(s, "delme");
        store_rollback(s);
        assert(store_get(s, "delme") != NULL);
        store_destroy(s);
        P();
    }

    T("iteration stable after delete"); {
        store *s = store_create();
        store_put(s, "a", "1");
        store_put(s, "b", "2");
        store_put(s, "c", "3");
        store_delete(s, "b");
        /* iteration must still produce a,c in order */
        store_iter *it = store_iter_create(s);
        const char *k;
        assert(store_iter_next(it, &k) && strcmp(k,"a")==0);
        assert(store_iter_next(it, &k) && strcmp(k,"c")==0);
        assert(store_iter_next(it, &k) == NULL);
        store_iter_destroy(it);
        store_destroy(s);
        P();
    }

    T("checksum changes after modification"); {
        store *s = store_create();
        store_put(s, "a", "1");
        unsigned long cs1 = store_checksum(s);
        store_put(s, "a", "2");
        unsigned long cs2 = store_checksum(s);
        assert(cs1 != cs2);
        store_destroy(s);
        P();
    }

    printf("\n%d/%d tests passed\n", tests_passed, tests_run);
    return tests_passed == tests_run ? 0 : 1;
}
