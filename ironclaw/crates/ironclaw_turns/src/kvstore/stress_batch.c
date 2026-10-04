#include "kvstore.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Stress: batch of many entries updating a small number of existing keys.
 * store stays tiny but count (entries) far exceeds MAX_KEYS staging arrays. */
int main(void) {
    kvstore *s = kvstore_create();
    /* 3 distinct keys */
    kvstore_put(s, "a", "0");
    kvstore_put(s, "b", "0");
    kvstore_put(s, "c", "0");

    const size_t N = 100000;
    const char **ks = malloc(sizeof(char *) * N);
    const char **vs = malloc(sizeof(char *) * N);
    if (!ks || !vs) { printf("malloc fail\n"); return 2; }
    for (size_t i = 0; i < N; i++) {
        ks[i] = i % 3 == 0 ? "a" : (i % 3 == 1 ? "b" : "c");
        vs[i] = "v";
    }
    bool ok = kvstore_put_batch(s, ks, vs, N);
    printf("put_batch ok=%d count=%zu\n", ok, kvstore_count(s));

    free(ks);
    free(vs);
    kvstore_destroy(s);
    printf("done\n");
    return 0;
}
