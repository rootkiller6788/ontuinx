/* B2-C transactional store — corrected implementation.
 *
 * Transaction rollback uses a per-level deep snapshot stack. Each begin()
 * records a deep copy of the current state; each rollback() restores the
 * state faithfully from that level's snapshot, and each commit() discards the
 * snapshot. This is correct for arbitrary sequences of operations within a
 * level (multiple puts of the same key, put/delete sequences, etc.) and for
 * arbitrarily nested transactions, and it never leaves dangling/aliased
 * pointers (deep copies throughout).
 */
#include "store.h"
#include <stdlib.h>
#include <string.h>

#define MAX_KEYS 64
#define MAX_LEVELS (MAX_KEYS + 1)
#define SNAP_CAP (MAX_LEVELS * MAX_KEYS)

typedef struct { char *key; char *value; } entry;

struct store {
    entry e[MAX_KEYS];
    size_t count;
    int tx_depth;
    /* snapshot pool: each level's begin() appends a contiguous deep copy.
     * snap_mark[d] is the pool index where level d's snapshot starts and
     * snap_n[d] is how many entries it holds. */
    entry snap_pool[SNAP_CAP];
    size_t snap_pool_count;
    int snap_mark[MAX_LEVELS + 1];
    size_t snap_n[MAX_LEVELS + 1];
};

store *store_create(void) { return calloc(1, sizeof(store)); }

void store_destroy(store *s) {
    if (!s) return;
    for (size_t i = 0; i < s->count; i++) {
        free(s->e[i].key);
        free(s->e[i].value);
    }
    for (size_t i = 0; i < s->snap_pool_count; i++) {
        free(s->snap_pool[i].key);
        free(s->snap_pool[i].value);
    }
    free(s);
}

static int find_key(store *s, const char *key) {
    for (size_t i = 0; i < s->count; i++)
        if (strcmp(s->e[i].key, key) == 0) return (int)i;
    return -1;
}

bool store_put(store *s, const char *key, const char *value) {
    if (!s || !key || !value || s->count >= MAX_KEYS) return false;
    int idx = find_key(s, key);
    if (idx >= 0) {
        free(s->e[idx].value);
        s->e[idx].value = strdup(value);
        return true;
    }
    s->e[s->count].key = strdup(key);
    s->e[s->count].value = strdup(value);
    s->count++;
    return true;
}

const char *store_get(store *s, const char *key) {
    if (!s || !key) return NULL;
    int idx = find_key(s, key);
    return idx >= 0 ? s->e[idx].value : NULL;
}

bool store_delete(store *s, const char *key) {
    if (!s || !key) return false;
    int idx = find_key(s, key);
    if (idx < 0) return false;
    free(s->e[idx].key);
    free(s->e[idx].value);
    memmove(&s->e[idx], &s->e[idx + 1], (s->count - idx - 1) * sizeof(entry));
    s->count--;
    return true;
}

size_t store_count(store *s) { return s ? s->count : 0; }

int store_begin(store *s) {
    if (!s) return -1;
    if (s->tx_depth >= MAX_LEVELS) return -1;
    s->tx_depth++;
    int d = s->tx_depth;
    s->snap_mark[d] = (int)s->snap_pool_count;
    s->snap_n[d] = s->count;
    /* deep copy current state so no pointers are shared/aliased */
    for (size_t i = 0; i < s->count; i++) {
        s->snap_pool[s->snap_pool_count].key = strdup(s->e[i].key);
        s->snap_pool[s->snap_pool_count].value = strdup(s->e[i].value);
        s->snap_pool_count++;
    }
    return 0;
}

int store_commit(store *s) {
    if (!s || s->tx_depth <= 0) return -1;
    int d = s->tx_depth;
    /* release every snapshot at this level and below (the deepest level
     * is committed, so nothing beyond it must remain). */
    for (size_t i = s->snap_mark[d]; i < s->snap_pool_count; i++) {
        free(s->snap_pool[i].key);
        free(s->snap_pool[i].value);
    }
    s->snap_pool_count = (size_t)s->snap_mark[d];
    s->snap_n[d] = 0;
    s->tx_depth = d - 1;
    return 0;
}

int store_rollback(store *s) {
    if (!s || s->tx_depth <= 0) return -1;
    int d = s->tx_depth;
    /* remove all live entries first */
    for (size_t i = 0; i < s->count; i++) {
        free(s->e[i].key);
        free(s->e[i].value);
    }
    /* restore a faithful copy of this level's begin-state */
    s->count = s->snap_n[d];
    for (size_t i = 0; i < s->snap_n[d]; i++) {
        s->e[i].key = strdup(s->snap_pool[s->snap_mark[d] + i].key);
        s->e[i].value = s->snap_pool[s->snap_mark[d] + i].value
                            ? strdup(s->snap_pool[s->snap_mark[d] + i].value)
                            : NULL;
    }
    /* release this level's snapshot (and any deeper), then truncate the pool */
    for (size_t i = s->snap_mark[d]; i < s->snap_pool_count; i++) {
        free(s->snap_pool[i].key);
        free(s->snap_pool[i].value);
    }
    s->snap_pool_count = (size_t)s->snap_mark[d];
    s->snap_n[d] = 0;
    s->tx_depth = d - 1;
    return 0;
}

int store_tx_depth(store *s) { return s ? s->tx_depth : 0; }

unsigned long store_checksum(store *s) {
    if (!s) return 0;
    unsigned long cs = 5381;
    for (size_t i = 0; i < s->count; i++) {
        for (const char *p = s->e[i].key; p && *p; p++) cs = cs * 33 + (unsigned char)*p;
        for (const char *p = s->e[i].value; p && *p; p++) cs = cs * 33 + (unsigned char)*p;
    }
    return cs;
}

struct store_iter { store *s; size_t pos; };
store_iter *store_iter_create(store *s) {
    if (!s) return NULL;
    store_iter *it = calloc(1, sizeof(*it));
    it->s = s;
    return it;
}
const char *store_iter_next(store_iter *it, const char **key_out) {
    if (!it || it->pos >= it->s->count) return NULL;
    *key_out = it->s->e[it->pos].key;
    return it->s->e[it->pos++].value;
}
void store_iter_destroy(store_iter *it) { free(it); }
