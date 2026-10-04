#include "kvstore.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails = 0;
#define CHECK(cond) do { if (!(cond)) { printf("FAIL: %s:%d  %s\n", __FILE__, __LINE__, #cond); fails++; } } while (0)

int main(void) {
    /* Case 1: batch with 300 distinct keys (count=300 > MAX_KEYS=256) must fail. */
    {
        kvstore *s = kvstore_create();
        const char **ks = malloc(sizeof(char*)*300);
        const char **vs = malloc(sizeof(char*)*300);
        for (int i = 0; i < 300; i++) { char *b = malloc(32); sprintf(b,"k%d",i); ks[i]=b; vs[i]="v"; }
        CHECK(kvstore_put_batch(s, ks, vs, 300) == false);
        CHECK(kvstore_count(s) == 0);
        for (int i = 0; i < 300; i++) free((void*)ks[i]);
        free(ks); free(vs);
        kvstore_destroy(s);
    }

    /* Case 2: batch with 1000 entries all the same existing key -> ok, count stays small. */
    {
        kvstore *s = kvstore_create();
        kvstore_put(s, "a", "0");
        const char **ks = malloc(sizeof(char*)*1000);
        const char **vs = malloc(sizeof(char*)*1000);
        for (int i = 0; i < 1000; i++) { ks[i]="a"; vs[i]="v"; }
        CHECK(kvstore_put_batch(s, ks, vs, 1000) == true);
        CHECK(kvstore_count(s) == 1);
        CHECK(strcmp(kvstore_get(s,"a"),"v")==0);
        free(ks); free(vs);
        kvstore_destroy(s);
    }

    /* Case 3: duplicate new keys within batch -> last wins, only inserted once. */
    {
        kvstore *s = kvstore_create();
        const char *ks[] = {"z","z","z"};
        const char *vs[] = {"1","2","3"};
        CHECK(kvstore_put_batch(s, ks, vs, 3) == true);
        CHECK(kvstore_count(s) == 1);
        CHECK(strcmp(kvstore_get(s,"z"),"3")==0);
        kvstore_destroy(s);
    }

    /* Case 4: mixed existing + duplicate new keys. */
    {
        kvstore *s = kvstore_create();
        kvstore_put(s, "old", "0");
        const char *ks[] = {"old","a","b","a","old"};
        const char *vs[] = {"o1","A","B","A2","o2"};
        CHECK(kvstore_put_batch(s, ks, vs, 5) == true);
        CHECK(kvstore_count(s) == 3);
        CHECK(strcmp(kvstore_get(s,"old"),"o2")==0);
        CHECK(strcmp(kvstore_get(s,"a"),"A2")==0);
        CHECK(strcmp(kvstore_get(s,"b"),"B")==0);
        kvstore_destroy(s);
    }

    printf(fails ? "%d FAILURES\n" : "ALL OK\n", fails);
    return fails ? 1 : 0;
}
