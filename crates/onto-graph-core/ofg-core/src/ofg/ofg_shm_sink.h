/*
 * ofg_shm_sink.h — SharedMemoryGraphSink constructor (P3).
 */
#ifndef OFG_SHM_SINK_H
#define OFG_SHM_SINK_H

#include "ofg_graph_sink.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Create a sink that writes wire frames to shared memory.
 * shm_base + shm_size: mmap'd shared memory region
 * data_ready_fd:       eventfd, C writes when frames are available
 * space_ready_fd:      eventfd, Rust writes when space is freed
 *
 * Returns NULL on failure.  Caller owns the memory and FDs. */
ofg_graph_sink_t *ofg_shm_sink_create(uint8_t *shm_base, uint64_t shm_size,
                                        int data_ready_fd, int space_ready_fd);

#ifdef __cplusplus
}
#endif

#endif /* OFG_SHM_SINK_H */
