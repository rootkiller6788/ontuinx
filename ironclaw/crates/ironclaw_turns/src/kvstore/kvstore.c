#include "kvstore.h"
#include <stdlib.h>
#include <string.h>

#define MAX_KEYS 256

struct kvstore {
    char *keys[MAX_KEYS];
    char *values[MAX_KEYS];
    size_t count;
};

kvstore *kvstore_create(void) {
    kvstore *s = calloc(1, sizeof(*s));
    return s;
}

void kvstore_destroy(kvstore *s) {
    if (!s) return;
    for (size_t i = 0; i < s->count; i++) {
        free(s->keys[i]);
        free(s->values[i]);
    }
    free(s);
}

static int find_key(kvstore *s, const char *key) {
    for (size_t i = 0; i < s->count; i++)
        if (strcmp(s->keys[i], key) == 0) return (int)i;
    return -1;
}

bool kvstore_put(kvstore *s, const char *key, const char *value) {
    if (!s || !key || !value) return false;

    int idx = find_key(s, key);
    if (idx >= 0) {
        /* Overwrite existing key: replace the value string. */
        char *nv = strdup(value);
        if (!nv) return false;
        free(s->values[idx]);
        s->values[idx] = nv;
        return true;
    }

    if (s->count >= MAX_KEYS) return false;

    char *nk = strdup(key);
    char *nv = nk ? strdup(value) : NULL;

    if (!nk || !nv) {
        /* Transactional: free whatever we allocated and leave state intact. */
        free(nk);
        free(nv);
        return false;
    }

    s->keys[s->count] = nk;
    s->values[s->count] = nv;
    s->count++;
    return true;
}

const char *kvstore_get(kvstore *s, const char *key) {
    if (!s || !key) return NULL;
    int idx = find_key(s, key);
    return idx >= 0 ? s->values[idx] : NULL;
}

bool kvstore_delete(kvstore *s, const char *key) {
    if (!s || !key) return false;
    int idx = find_key(s, key);
    if (idx < 0) return false;

    free(s->keys[idx]);
    free(s->values[idx]);
    s->keys[idx] = NULL;
    s->values[idx] = NULL;

    /* Compact the array so no freed slot remains within [0, count).
     * Without compaction, destroy() would free the same pointer twice
     * once count shrinks, and find_key() would miss shifted entries. */
    for (size_t i = idx; i + 1 < s->count; i++) {
        s->keys[i] = s->keys[i + 1];
        s->values[i] = s->values[i + 1];
    }
    s->keys[s->count - 1] = NULL;
    s->values[s->count - 1] = NULL;
    s->count--;
    return true;
}

bool kvstore_put_batch(kvstore *s, const char **keys, const char **values, size_t count) {
    if (!s || !keys || !values || count == 0) return false;

    /* ---- Pass 1: validate the whole batch without mutating the store. ---- */
    size_t extra = 0; /* number of distinct new keys this batch would insert */
    for (size_t i = 0; i < count; i++) {
        if (!keys[i] || !values[i]) return false;
        if (find_key(s, keys[i]) >= 0) continue; /* already in the store */
        /* Only count a key once, even if it repeats several times in the batch. */
        bool seen = false;
        for (size_t j = 0; j < i; j++)
            if (strcmp(keys[j], keys[i]) == 0) { seen = true; break; }
        if (!seen) extra++;
    }
    if (s->count + extra > MAX_KEYS) return false;

    /* ---- Stage duplicated strings for every entry. ---- */
    /* Sized by `count`, not MAX_KEYS: a batch may hold up to `count` entries
     * (including duplicates), and `count` can exceed MAX_KEYS as long as the
     * number of distinct new keys fits within the store's capacity. */
    char **new_keys = malloc(sizeof(*new_keys) * count);
    char **new_vals = malloc(sizeof(*new_vals) * count);
    if (!new_keys || !new_vals) {
        free(new_keys);
        free(new_vals);
        return false;
    }
    for (size_t i = 0; i < count; i++) {
        new_keys[i] = NULL;
        new_vals[i] = NULL;
    }

    for (size_t i = 0; i < count; i++) {
        new_keys[i] = strdup(keys[i]);
        if (!new_keys[i]) goto fail;
        new_vals[i] = strdup(values[i]);
        if (!new_vals[i]) goto fail;
    }

    /* ---- Pass 2: commit. All allocations succeeded, so nothing can fail. ---- */
    for (size_t i = 0; i < count; i++) {
        int idx = find_key(s, keys[i]);
        if (idx >= 0) {
            /* Existing key (from the store or inserted earlier in this batch):
             * the last occurrence in the batch wins. */
            free(s->values[idx]);
            s->values[idx] = new_vals[i];
            new_vals[i] = NULL;
            free(new_keys[i]);
            new_keys[i] = NULL;
        } else {
            /* New key: capacity was validated in pass 1. */
            s->keys[s->count] = new_keys[i];
            s->values[s->count] = new_vals[i];
            new_keys[i] = NULL;
            new_vals[i] = NULL;
            s->count++;
        }
    }
    free(new_keys);
    free(new_vals);
    return true;

fail:
    for (size_t i = 0; i < count; i++) {
        free(new_keys[i]);
        free(new_vals[i]);
    }
    free(new_keys);
    free(new_vals);
    return false;
}

int kvstore_merge(kvstore *dst, kvstore *src) {
    if (!dst || !src) return -1;
    int merged = 0;
    for (size_t i = 0; i < src->count; i++) {
        /* src overrides dst on conflict; kvstore_put handles both cases. */
        if (kvstore_put(dst, src->keys[i], src->values[i]))
            merged++;
    }
    return merged;
}

size_t kvstore_count(kvstore *s) {
    return s ? s->count : 0;
}
