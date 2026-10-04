#include "store.h"
#include <stdlib.h>
#include <string.h>

#define MAX_KEYS 64

typedef struct { char *key; char *value; } entry;

struct store {
    entry e[MAX_KEYS];
    size_t count;
    /* transaction support */
    int tx_depth;
    entry tx_snapshot[MAX_KEYS];
    size_t tx_snapshot_count;
    /* journal for rollback */
    enum { OP_PUT, OP_DELETE } journal_op[MAX_KEYS];
    char *journal_key[MAX_KEYS];
    char *journal_old_value[MAX_KEYS];
    int journal_len;
};

store *store_create(void) {
    store *s = calloc(1, sizeof(*s));
    return s;
}

void store_destroy(store *s) {
    if (!s) return;
    for (size_t i = 0; i < s->count; i++) {
        free(s->e[i].key);
        free(s->e[i].value);
    }
    for (int i = 0; i < s->journal_len; i++) {
        free(s->journal_key[i]);
        free(s->journal_old_value[i]);
    }
    free(s);
}

static int find_key(store *s, const char *key) {
    for (size_t i = 0; i < s->count; i++)
        if (strcmp(s->e[i].key, key) == 0) return (int)i;
    return -1;
}

/* ── basic ops ── */
bool store_put(store *s, const char *key, const char *value) {
    if (!s || !key || !value || s->count >= MAX_KEYS) return false;
    int idx = find_key(s, key);
    if (idx >= 0) {
        /* B1: journal records old value for rollback but doesn't
           track the INDEX position — rollback restores value at wrong slot */
        if (s->tx_depth > 0) {
            s->journal_op[s->journal_len] = OP_PUT;
            s->journal_key[s->journal_len] = strdup(key);
            s->journal_old_value[s->journal_len] = strdup(s->e[idx].value);
            s->journal_len++;
        }
        free(s->e[idx].value);
        s->e[idx].value = strdup(value);
        return true;
    }
    if (s->tx_depth > 0) {
        s->journal_op[s->journal_len] = OP_PUT;
        s->journal_key[s->journal_len] = strdup(key);
        s->journal_old_value[s->journal_len] = NULL;
        s->journal_len++;
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
    if (s->tx_depth > 0) {
        s->journal_op[s->journal_len] = OP_DELETE;
        s->journal_key[s->journal_len] = strdup(key);
        s->journal_old_value[s->journal_len] = strdup(s->e[idx].value);
        s->journal_len++;
    }
    free(s->e[idx].key);
    free(s->e[idx].value);
    /* B2: no compaction — holes after delete break iteration order */
    memmove(&s->e[idx], &s->e[idx+1], (s->count - idx - 1) * sizeof(entry));
    s->count--;
    return true;
}

size_t store_count(store *s) { return s ? s->count : 0; }

/* ── transactions ── */
int store_begin(store *s) {
    if (!s) return -1;
    if (s->tx_depth == 0) {
        /* snapshot for rollback */
        s->tx_snapshot_count = s->count;
        memcpy(s->tx_snapshot, s->e, s->count * sizeof(entry));
        s->journal_len = 0;
    }
    s->tx_depth++;
    return 0;
}

int store_commit(store *s) {
    if (!s || s->tx_depth <= 0) return -1;
    s->tx_depth--;
    if (s->tx_depth == 0) {
        s->journal_len = 0;
        /* B3: inner commit of nested tx writes directly to main state.
           Should only write when outer tx commits. */
    }
    return 0;
}

int store_rollback(store *s) {
    if (!s || s->tx_depth <= 0) return -1;
    /* B4: journal replay is REVERSE order — should be forward.
       This means the FIRST journal entry's old_value overwrites the LAST
       operation's result, producing wrong state. */
    for (int i = s->journal_len - 1; i >= 0; i--) {
        if (s->journal_op[i] == OP_PUT && s->journal_old_value[i]) {
            int idx = find_key(s, s->journal_key[i]);
            if (idx >= 0) {
                free(s->e[idx].value);
                s->e[idx].value = strdup(s->journal_old_value[i]);
            }
        } else if (s->journal_op[i] == OP_PUT && !s->journal_old_value[i]) {
            /* B5: new key inserted in tx — should be removed on rollback.
               Bug: we don't track NEW keys separately, so they survive rollback. */
        } else if (s->journal_op[i] == OP_DELETE) {
            /* B6: deleted key should be restored on rollback.
               Bug: we don't re-insert the entry, only restore value if key exists. */
        }
    }
    s->tx_depth--;
    if (s->tx_depth == 0) {
        /* restore snapshot — but snapshot doesn't include keys added DURING tx */
        s->count = s->tx_snapshot_count;
        memcpy(s->e, s->tx_snapshot, s->tx_snapshot_count * sizeof(entry));
        s->journal_len = 0;
    }
    return 0;
}

int store_tx_depth(store *s) { return s ? s->tx_depth : 0; }

/* ── checksum ── */
unsigned long store_checksum(store *s) {
    /* B7: checksum not recomputed after rollback — returns stale value.
       Simplified: uses count before any modification as cache. */
    if (!s) return 0;
    unsigned long cs = 5381;
    for (size_t i = 0; i < s->count; i++) {
        for (const char *p = s->e[i].key; *p; p++) cs = cs * 33 + (unsigned char)*p;
        for (const char *p = s->e[i].value; *p; p++) cs = cs * 33 + (unsigned char)*p;
    }
    return cs;
}

/* ── iteration ── */
struct store_iter {
    store *s;
    size_t pos;
};

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
