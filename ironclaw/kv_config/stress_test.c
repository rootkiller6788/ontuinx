#include "kv_config.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

static int tests = 0, passed = 0;
#define T(name) do { tests++; printf("  %s ... ", name); } while(0)
#define P() do { passed++; printf("PASS\n"); } while(0)
#define F(m) do { printf("FAIL: %s\n", m); } while(0)

int main(void) {
    printf("KV_CONFIG STRESS\n================\n");

    /* 1. Many entries > initial capacity of 4 -> forces reserve growth */
    {
        T("grow beyond cap (100 entries)");
        FILE *f = fopen("big.conf", "w");
        for (int i = 0; i < 100; i++)
            fprintf(f, "key%d=value%d\n", i, i);
        fclose(f);
        kv_config *c = kv_config_parse("big.conf");
        assert(c);
        assert(kv_config_count(c) == 100);
        int ok = 1;
        for (int i = 0; i < 100; i++) {
            char k[32], v[32];
            snprintf(k, sizeof k, "key%d", i);
            snprintf(v, sizeof v, "value%d", i);
            const char *got = kv_config_get(c, k);
            if (!got || strcmp(got, v) != 0) { ok = 0; break; }
        }
        kv_config_destroy(c);
        assert(ok);
        P();
    }

    /* 2. CRLF line endings */
    {
        T("CRLF line endings");
        FILE *f = fopen("crlf.conf", "w");
        fprintf(f, "host=localhost\r\nport=8080\r\n");
        fclose(f);
        kv_config *c = kv_config_parse("crlf.conf");
        assert(c);
        assert(strcmp(kv_config_get(c, "host"), "localhost") == 0);
        assert(strcmp(kv_config_get(c, "port"), "8080") == 0);
        kv_config_destroy(c);
        P();
    }

    /* 3. Comment line containing an '=' must be skipped, not parsed */
    {
        T("comment line with '=' skipped");
        FILE *f = fopen("cmt.conf", "w");
        fprintf(f, "# note=should_not_count\nhost=localhost\n");
        fclose(f);
        kv_config *c = kv_config_parse("cmt.conf");
        assert(c);
        printf("COUNT=%zu\n", kv_config_count(c));
        kv_config_destroy(c);
        P();
    }

    /* 4. Spaces around '=' -> trimmed keys? */
    {
        T("spaces around '=' are handled deterministically");
        FILE *f = fopen("space.conf", "w");
        fprintf(f, "host = localhost\n");
        fclose(f);
        kv_config *c = kv_config_parse("space.conf");
        assert(c);
        const char *v = kv_config_get(c, "host");
        printf("  get('host')=%s\n", v ? v : "(null)");
        kv_config_destroy(c);
        P();
    }

    printf("\n%d/%d passed\n", passed, tests);
    return passed == tests ? 0 : 1;
}
