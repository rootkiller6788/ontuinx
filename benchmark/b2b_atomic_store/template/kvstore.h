#ifndef KVSTORE_H
#define KVSTORE_H
#include <stddef.h>
#include <stdbool.h>

typedef struct kvstore kvstore;

kvstore *kvstore_create(void);
void kvstore_destroy(kvstore *s);

/* Single-key operations */
bool kvstore_put(kvstore *s, const char *key, const char *value);
const char *kvstore_get(kvstore *s, const char *key);
bool kvstore_delete(kvstore *s, const char *key);

/* Atomic batch: all-or-nothing. Returns true iff ALL puts succeed. */
bool kvstore_put_batch(kvstore *s, const char **keys, const char **values, size_t count);

/* Merge src into dst: src values override dst on key conflict.
   Returns count of merged keys, or -1 on error. */
int kvstore_merge(kvstore *dst, kvstore *src);

size_t kvstore_count(kvstore *s);
#endif
