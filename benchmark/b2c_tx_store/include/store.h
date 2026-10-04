#ifndef STORE_H
#define STORE_H
#include <stddef.h>
#include <stdbool.h>

typedef struct store store;

store *store_create(void);
void store_destroy(store *s);
bool store_put(store *s, const char *key, const char *value);
const char *store_get(store *s, const char *key);
bool store_delete(store *s, const char *key);
size_t store_count(store *s);

/* Transaction support */
int  store_begin(store *s);
int  store_commit(store *s);
int  store_rollback(store *s);
int  store_tx_depth(store *s);

/* Iteration — stable across modifications within a transaction */
typedef struct store_iter store_iter;
store_iter *store_iter_create(store *s);
const char *store_iter_next(store_iter *it, const char **key_out);
void store_iter_destroy(store_iter *it);

/* Checksum: CRC32 over all keys+values in sorted order */
unsigned long store_checksum(store *s);
#endif
