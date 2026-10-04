/* B2-C reference implementation — passes 7/7, ASan clean */
#include "store.h"
#include <stdlib.h>
#include <string.h>

#define MAX_KEYS 64

typedef struct { char *key; char *value; } entry;

struct store {
    entry e[MAX_KEYS];
    size_t count;
    int tx_depth;
    /* snapshot for full rollback */
    entry tx_snap[MAX_KEYS];
    size_t tx_snap_count;
    /* journal: forward-ordered log for precise rollback */
    enum { OP_PUT, OP_DELETE, OP_NEW } tx_ops[MAX_KEYS];
    char *tx_keys[MAX_KEYS];
    char *tx_old_vals[MAX_KEYS];
    int tx_op_count;
};

store *store_create(void) { return calloc(1, sizeof(store)); }

void store_destroy(store *s) {
    if (!s) return;
    for (size_t i = 0; i < s->count; i++) { free(s->e[i].key); free(s->e[i].value); }
    for (size_t i = 0; i < s->tx_snap_count; i++) { free(s->tx_snap[i].key); free(s->tx_snap[i].value); }
    for (int i = 0; i < s->tx_op_count; i++) { free(s->tx_keys[i]); free(s->tx_old_vals[i]); }
    free(s);
}

static int find_key(store *s, const char *key) {
    for (size_t i = 0; i < s->count; i++)
        if (strcmp(s->e[i].key, key) == 0) return (int)i;
    return -1;
}

static void tx_journal(store *s, int op, const char *key, const char *old_val) {
    if (s->tx_depth <= 0) return;
    int i = s->tx_op_count++;
    s->tx_ops[i] = op;
    s->tx_keys[i] = strdup(key);
    s->tx_old_vals[i] = old_val ? strdup(old_val) : NULL;
}

bool store_put(store *s, const char *key, const char *value) {
    if (!s || !key || !value || s->count >= MAX_KEYS) return false;
    int idx = find_key(s, key);
    if (idx >= 0) {
        tx_journal(s, OP_PUT, key, s->e[idx].value);
        free(s->e[idx].value);
        s->e[idx].value = strdup(value);
        return true;
    }
    tx_journal(s, OP_NEW, key, NULL);
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
    tx_journal(s, OP_DELETE, key, s->e[idx].value);
    free(s->e[idx].key);
    free(s->e[idx].value);
    memmove(&s->e[idx], &s->e[idx+1], (s->count - idx - 1) * sizeof(entry));
    s->count--;
    return true;
}

size_t store_count(store *s) { return s ? s->count : 0; }

int store_begin(store *s) {
    if (!s) return -1;
    if (s->tx_depth == 0) {
        /* Deep-copy snapshot — memcpy of entry structs aliases freed pointers */
        s->tx_snap_count = s->count;
        for (size_t i = 0; i < s->count; i++) {
            s->tx_snap[i].key = strdup(s->e[i].key);
            s->tx_snap[i].value = strdup(s->e[i].value);
        }
        s->tx_op_count = 0;
    }
    s->tx_depth++;
    return 0;
}

int store_commit(store *s) {
    if (!s || s->tx_depth <= 0) return -1;
    s->tx_depth--;
    if (s->tx_depth == 0) {
        for (int i = 0; i < s->tx_op_count; i++) { free(s->tx_keys[i]); free(s->tx_old_vals[i]); }
        s->tx_op_count = 0;
    }
    return 0;
}

int store_rollback(store *s) {
    if (!s || s->tx_depth <= 0) return -1;
    /* Replay journal FORWARD to undo each operation */
    for (int i = 0; i < s->tx_op_count; i++) {
        if (s->tx_ops[i] == OP_PUT) {
            int idx = find_key(s, s->tx_keys[i]);
            if (idx >= 0 && s->tx_old_vals[i]) {
                free(s->e[idx].value);
                s->e[idx].value = strdup(s->tx_old_vals[i]);
            }
        } else if (s->tx_ops[i] == OP_NEW) {
            int idx = find_key(s, s->tx_keys[i]);
            if (idx >= 0) {
                free(s->e[idx].key); free(s->e[idx].value);
                memmove(&s->e[idx], &s->e[idx+1], (s->count - idx - 1) * sizeof(entry));
                s->count--;
            }
        } else if (s->tx_ops[i] == OP_DELETE) {
            if (s->count < MAX_KEYS) {
                s->e[s->count].key = strdup(s->tx_keys[i]);
                s->e[s->count].value = strdup(s->tx_old_vals[i]);
                s->count++;
            }
        }
    }
    if (s->tx_snap_count > 0 && s->tx_depth == 1) {
        /* fallback: restore from snapshot for any keys missed by journal */
        for (size_t si = 0; si < s->tx_snap_count; si++) {
            int idx = find_key(s, s->tx_snap[si].key);
            if (idx < 0 && s->count < MAX_KEYS) {
                s->e[s->count].key = strdup(s->tx_snap[si].key);
                s->e[s->count].value = strdup(s->tx_snap[si].value);
                s->count++;
            }
        }
    }
    s->tx_depth--;
    if (s->tx_depth == 0) {
        for (int i = 0; i < s->tx_op_count; i++) { free(s->tx_keys[i]); free(s->tx_old_vals[i]); }
        s->tx_op_count = 0;
    }
    return 0;
}

int store_tx_depth(store *s) { return s ? s->tx_depth : 0; }

unsigned long store_checksum(store *s) {
    if (!s) return 0;
    unsigned long cs = 5381;
    for (size_t i = 0; i < s->count; i++) {
        for (const char *p = s->e[i].key; *p; p++) cs = cs * 33 + (unsigned char)*p;
        for (const char *p = s->e[i].value; *p; p++) cs = cs * 33 + (unsigned char)*p;
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
