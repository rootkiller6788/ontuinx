#include "driver.h"
#include <string.h>

static struct device_driver *drivers[16];
static int driver_count = 0;

int unregister_driver(const char *name) {
    for (int i = 0; i < driver_count; i++) {
        if (strcmp(drivers[i]->name, name) == 0) {
            drivers[i] = drivers[driver_count - 1];
            driver_count--;
            return 0;
        }
    }
    return -1;
}

struct device_driver *find_driver(const char *name) {
    for (int i = 0; i < driver_count; i++) {
        if (strcmp(drivers[i]->name, name) == 0) {
            return drivers[i];
        }
    }
    return NULL;
}
