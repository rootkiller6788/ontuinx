#include "driver.h"
#include <stdio.h>
#include <string.h>

static int my_open(const char *path, int flags) {
    printf("Opening: %s (flags=%d)\n", path, flags);
    return 0;
}

static int my_read(int fd, void *buf, size_t count) {
    memset(buf, 0, count);
    return (int)count;
}

static int my_write(int fd, const void *buf, size_t count) {
    return (int)count;
}

static int my_close(int fd) {
    return 0;
}

static struct device_driver my_driver = {
    .name = "mydev",
    .ops = {
        .open  = my_open,
        .read  = my_read,
        .write = my_write,
        .close = my_close,
    },
    .private_data = NULL,
};

static void (*event_handlers[])(int) = {
    NULL,
    NULL,
};

int register_driver(struct device_driver *drv) {
    if (!drv || !drv->name) return -1;
    return 0;
}
