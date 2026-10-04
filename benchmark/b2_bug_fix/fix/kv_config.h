#ifndef KV_CONFIG_H
#define KV_CONFIG_H
#include <stddef.h>

typedef struct kv_config kv_config;

kv_config *kv_config_parse(const char *path);
void kv_config_destroy(kv_config *cfg);
const char *kv_config_get(kv_config *cfg, const char *key);
size_t kv_config_count(kv_config *cfg);

#endif
