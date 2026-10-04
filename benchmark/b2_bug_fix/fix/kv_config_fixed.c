#include "kv_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_LINE 256

struct kv_config {
    char **keys;
    char **values;
    size_t count;
    size_t cap;
};

static void kv_config_free(kv_config *cfg) {
    if (!cfg) return;
    for (size_t i = 0; i < cfg->count; i++) {
        free(cfg->keys[i]);
        free(cfg->values[i]);
    }
    free(cfg->keys);
    free(cfg->values);
    free(cfg);
}

/* Grow the backing arrays so they can hold at least `need` entries. */
static int kv_config_reserve(kv_config *cfg, size_t need) {
    if (need <= cfg->cap) return 1;

    size_t ncap = cfg->cap ? cfg->cap : 4;
    while (ncap < need) ncap *= 2;

    /* Grow one array at a time so a partial realloc failure never orphans the
     * unrelated pointer. Commit the new allocation only after both succeed. */
    char **nk = realloc(cfg->keys, ncap * sizeof(char*));
    if (!nk) return 0;
    char **nv = realloc(cfg->values, ncap * sizeof(char*));
    if (!nv) return 0;
    cfg->keys = nk;
    cfg->values = nv;
    cfg->cap = ncap;
    return 1;
}

kv_config *kv_config_parse(const char *path) {
    if (!path) return NULL;
    FILE *f = fopen(path, "r");
    if (!f) return NULL;

    kv_config *cfg = calloc(1, sizeof(*cfg));
    if (!cfg) { fclose(f); return NULL; }

    char line[MAX_LINE];
    while (fgets(line, sizeof(line), f)) {
        /* Trim trailing newline/CR. */
        size_t len = strlen(line);
        while (len > 0 && (line[len-1] == '\n' || line[len-1] == '\r'))
            line[--len] = '\0';

        /* Skip blank, comment, and lines with no '=' separator. */
        char *eq = strchr(line, '=');
        if (!eq) continue;
        *eq = '\0';

        char *key = line;
        char *value = eq + 1;
        if (*key == '\0') continue;

        if (!kv_config_reserve(cfg, cfg->count + 1)) goto fail;

        char *k = strdup(key);
        char *v = strdup(value);
        if (!k || !v) {
            free(k);
            free(v);
            goto fail;
        }
        cfg->keys[cfg->count] = k;
        cfg->values[cfg->count] = v;
        cfg->count++;
    }

    fclose(f);
    return cfg;

fail:
    fclose(f);
    kv_config_free(cfg);
    return NULL;
}

void kv_config_destroy(kv_config *cfg) {
    kv_config_free(cfg);
}

const char *kv_config_get(kv_config *cfg, const char *key) {
    if (!cfg || !key) return NULL;
    for (size_t i = 0; i < cfg->count; i++)
        if (strcmp(cfg->keys[i], key) == 0) return cfg->values[i];
    return NULL;
}

size_t kv_config_count(kv_config *cfg) {
    if (!cfg) return 0;
    return cfg->count;
}
