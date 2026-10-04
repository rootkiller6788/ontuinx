#include "kv_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

#define MAX_LINE 256

/* strdup is a POSIX extension, not part of C11. Provide a portable fallback
 * so the library compiles cleanly under -std=c11 -pedantic. */
#if !defined(_POSIX_C_SOURCE) && !defined(__APPLE__) && !defined(_MSC_VER)
static char *kv_strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *d = malloc(n);
    if (d) memcpy(d, s, n);
    return d;
}
#define strdup kv_strdup
#endif

struct kv_config {
    char **keys;
    char **values;
    size_t count;
    size_t cap;
};

/* Reserve capacity for at least `need` entries in cfg. On failure the cfg
 * state is left unchanged and nothing is leaked; returns 0 on success, -1 on
 * allocation failure.
 *
 * This is deliberately transactional with respect to allocation failure: both
 * new buffers are malloc'd and copied before either old buffer is freed. No
 * half-committed state and no double-free are possible. */
static int kv_config_reserve(kv_config *cfg, size_t need) {
    if (need <= cfg->cap) return 0;

    size_t newcap = cfg->cap ? cfg->cap : 4;
    while (newcap < need) {
        if (newcap > (SIZE_MAX / 2)) return -1; /* overflow guard */
        newcap *= 2;
    }

    /* The byte count newcap * sizeof(char *) must not overflow size_t.
     * Without this check an enormous `need` yields a truncated allocation
     * and a later out-of-bounds write past the undersized buffer. */
    if (newcap > (SIZE_MAX / sizeof(char *))) return -1;

    char **nk = malloc(newcap * sizeof(char *));
    if (!nk) return -1;
    if (cfg->keys) memcpy(nk, cfg->keys, cfg->cap * sizeof(char *));

    char **nv = malloc(newcap * sizeof(char *));
    if (!nv) {
        free(nk);
        return -1;
    }
    if (cfg->values) memcpy(nv, cfg->values, cfg->cap * sizeof(char *));

    free(cfg->keys);
    free(cfg->values);
    cfg->keys = nk;
    cfg->values = nv;
    cfg->cap = newcap;
    return 0;
}

/* Read one logical line from f and return it NUL-terminated (newline/CR
 * trimmed, trailing whitespace kept for the caller to trim). The buffer is
 * grown as needed so arbitrarily long configuration lines are handled
 * correctly rather than silently truncated at MAX_LINE. Returns NULL on
 * EOF/error or allocation failure. */
static char *read_line(FILE *f) {
    size_t cap = MAX_LINE;
    size_t len = 0;
    char *buf = malloc(cap);
    if (!buf) return NULL;

    for (;;) {
        if (len + 1 >= cap) { /* need room for at least one more char + NUL */
            if (cap > (SIZE_MAX / 2)) { free(buf); return NULL; }
            size_t ncap = cap * 2;
            char *nb = realloc(buf, ncap);
            if (!nb) { free(buf); return NULL; }
            buf = nb;
            cap = ncap;
        }
        int c = fgetc(f);
        if (c == EOF) {
            if (len == 0) { free(buf); return NULL; } /* clean EOF */
            buf[len] = '\0';
            return buf;
        }
        if (c == '\n') {
            buf[len] = '\0';
            return buf;
        }
        buf[len++] = (char)c;
    }
}

/* Minimal in-place copy excluding a trailing CR (for CRLF line endings). */
static void strip_trailing_cr(char *s) {
    size_t n = strlen(s);
    while (n > 0 && s[n-1] == '\r') s[--n] = '\0';
}

kv_config *kv_config_parse(const char *path) {
    if (!path) return NULL;

    FILE *f = fopen(path, "r");
    if (!f) return NULL;

    kv_config *cfg = calloc(1, sizeof(*cfg));
    if (!cfg) { fclose(f); return NULL; }

    if (kv_config_reserve(cfg, 4) != 0) {
        fclose(f);
        free(cfg);
        return NULL;
    }

    char *line;
    while ((line = read_line(f)) != NULL) {
        strip_trailing_cr(line);

        /* Skip blank lines and comment lines (first non-space char is '#'). */
        char *scan = line;
        while (*scan == ' ' || *scan == '\t') scan++;
        if (*scan == '\0' || *scan == '#') {
            free(line);
            continue;
        }

        char *eq = strchr(line, '=');
        if (!eq) { free(line); continue; } /* no separator: skip line safely */
        *eq = '\0';

        /* Trim leading whitespace from the key. */
        char *key = line;
        while (*key == ' ' || *key == '\t') key++;
        /* Trim trailing whitespace from the key. */
        size_t klen = strlen(key);
        while (klen > 0 && (key[klen-1] == ' ' || key[klen-1] == '\t'))
            key[--klen] = '\0';
        if (klen == 0) { free(line); continue; } /* no actual key */

        /* Value: trim leading and trailing whitespace. */
        char *value = eq + 1;
        while (*value == ' ' || *value == '\t') value++;
        size_t vlen = strlen(value);
        while (vlen > 0 && (value[vlen-1] == ' ' || value[vlen-1] == '\t'))
            value[--vlen] = '\0';

        if (kv_config_reserve(cfg, cfg->count + 1) != 0) {
            free(line);
            kv_config_destroy(cfg);
            fclose(f);
            return NULL;
        }

        char *dup_key = strdup(key);
        char *dup_value = strdup(value);
        if (!dup_key || !dup_value) {
            free(dup_key);
            free(dup_value);
            free(line);
            kv_config_destroy(cfg);
            fclose(f);
            return NULL;
        }

        cfg->keys[cfg->count] = dup_key;
        cfg->values[cfg->count] = dup_value;
        cfg->count++;
        free(line);
    }

    fclose(f);
    return cfg;
}

void kv_config_destroy(kv_config *cfg) {
    if (!cfg) return;
    for (size_t i = 0; i < cfg->count; i++) {
        free(cfg->keys[i]);
        free(cfg->values[i]);
    }
    free(cfg->keys);
    free(cfg->values);
    free(cfg);
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
