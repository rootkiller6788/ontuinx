#ifndef DRIVER_H
#define DRIVER_H
#include <stddef.h>

struct file_operations {
    int (*open)(const char *path, int flags);
    int (*read)(int fd, void *buf, size_t count);
    int (*write)(int fd, const void *buf, size_t count);
    int (*close)(int fd);
};

struct device_driver {
    const char *name;
    struct file_operations ops;
    void *private_data;
};

int register_driver(struct device_driver *drv);
int unregister_driver(const char *name);
struct device_driver *find_driver(const char *name);

#endif
