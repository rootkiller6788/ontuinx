/*
 * ofg_shm_sink.c — SharedMemoryGraphSink: ring-buffer ofg_graph_sink_t (P3).
 *
 * Writes wire-format frames to an SPSC ring buffer in shared memory.
 * eventfd used for notification (data_ready: C→Rust, space_ready: Rust→C).
 *
 * Does NOT touch SQLite, PostgreSQL, or any network.  Pure ring I/O.
 */
#include "ofg_graph_sink.h"
#include "ofg_wire.h"

#include <stdatomic.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/eventfd.h>
#include <sys/mman.h>

/* ── Internal state ─────────────────────────────────────────────── */

typedef struct {
    /* Mapped shared memory. */
    uint8_t         *shm_base;
    uint64_t         shm_size;
    ofg_ctrl_t      *ctrl;
    ofg_ring_meta_t *ring;

    /* eventfd handles. */
    int data_ready_fd;    /* C → Rust: new frame available */
    int space_ready_fd;   /* Rust → C: space freed */

    /* Sequence counter per snapshot. */
    uint64_t frame_seq;
    uint64_t snapshot_epoch;
} shm_sink_t;

/* ── Ring helpers ──────────────────────────────────────────────── */

/* Wait for space_ready event (Rust has freed room).  Polls with backoff. */
static int wait_for_space(shm_sink_t *s, uint64_t needed) {
    uint64_t avail;
    for (int retry = 0; retry < 1000; retry++) {
        uint64_t w = atomic_load_explicit(&s->ring->write_offset, memory_order_acquire);
        uint64_t r = atomic_load_explicit(&s->ring->read_offset, memory_order_acquire);
        uint64_t used = (w >= r) ? (w - r) : (s->ring->capacity - r + w);
        avail = s->ring->capacity - used - 128; /* reserve for header */
        if (avail >= needed) return 0;

        /* Read space_ready_fd to clear any pending notification. */
        uint64_t val = 0;
        ssize_t nr = read(s->space_ready_fd, &val, sizeof(val));
        (void)nr;
    }
    return -1; /* timeout */
}

/* Write a complete frame to the ring.  Blocks if no space. */
static int ring_write_frame(shm_sink_t *s, uint16_t kind, uint64_t seq,
                             const uint8_t *payload, uint32_t payload_len,
                             uint32_t record_count) {
    uint64_t total = sizeof(ofg_frame_header_t) + payload_len;
    if (wait_for_space(s, total) != 0) return -1;

    uint64_t w = atomic_load_explicit(&s->ring->write_offset, memory_order_relaxed);
    uint64_t cap = s->ring->capacity;

    ofg_frame_header_t hdr = {
        .magic              = OFG_WIRE_MAGIC,
        .abi_version        = OFG_WIRE_ABI_VERSION,
        .record_kind        = kind,
        .sequence           = seq,
        .snapshot_epoch     = s->snapshot_epoch,
        .payload_length     = payload_len,
        .total_frame_length = total,
        .record_count       = record_count,
        .checksum           = 0, /* CRC32 deferred */
    };

    /* Write header. */
    uint64_t hdr_end = w + sizeof(hdr);
    if (hdr_end <= cap) {
        memcpy(s->shm_base + OFG_FRAMES_OFFSET + w, &hdr, sizeof(hdr));
    } else {
        /* Wrap. */
        uint64_t first = cap - w;
        memcpy(s->shm_base + OFG_FRAMES_OFFSET + w, &hdr, first);
        memcpy(s->shm_base + OFG_FRAMES_OFFSET, ((uint8_t *)&hdr) + first, sizeof(hdr) - first);
    }

    /* Write payload. */
    uint64_t pay_start = (w + sizeof(hdr)) % cap;
    if (pay_start + payload_len <= cap) {
        memcpy(s->shm_base + OFG_FRAMES_OFFSET + pay_start, payload, payload_len);
    } else {
        uint64_t first = cap - pay_start;
        memcpy(s->shm_base + OFG_FRAMES_OFFSET + pay_start, payload, first);
        memcpy(s->shm_base + OFG_FRAMES_OFFSET, payload + first, payload_len - first);
    }

    /* Advance write_offset with release semantics. */
    uint64_t new_w = (w + total) % cap;
    atomic_store_explicit(&s->ring->write_offset, new_w, memory_order_release);

    /* Notify Rust. */
    uint64_t one = 1;
    ssize_t nw = write(s->data_ready_fd, &one, sizeof(one));
    (void)nw;

    return 0;
}

/* ── Sink callbacks ─────────────────────────────────────────────── */

static int shm_begin_snapshot(void *ctx, const ofg_snapshot_meta_t *meta) {
    shm_sink_t *s = (shm_sink_t *)ctx;
    s->snapshot_epoch++;
    s->frame_seq = 0;

    atomic_store_explicit(&s->ctrl->snapshot_epoch, s->snapshot_epoch, memory_order_release);
    atomic_store_explicit(&s->ctrl->state, 1, memory_order_release); /* running */

    /* Write SNAPSHOT_BEGIN frame with metadata as payload (inline JSON). */
    char meta_buf[512];
    int ml = snprintf(meta_buf, sizeof(meta_buf),
                      "{\"repository_id\":\"%s\",\"base_commit_sha\":\"%s\","
                      "\"candidate_checkpoint_hash\":\"%s\","
                      "\"execution_generation\":%llu,"
                      "\"extractor_version\":\"%s\","
                      "\"graph_schema_version\":%u}",
                      meta->repository_id ? meta->repository_id : "",
                      meta->base_commit_sha ? meta->base_commit_sha : "",
                      meta->candidate_checkpoint_hash ? meta->candidate_checkpoint_hash : "",
                      (unsigned long long)meta->execution_generation,
                      meta->extractor_version ? meta->extractor_version : "",
                      meta->graph_schema_version);
    return ring_write_frame(s, OFG_KIND_SNAPSHOT_BEGIN, s->frame_seq++,
                            (const uint8_t *)meta_buf, (uint32_t)ml, 0);
}

static int shm_write_nodes(void *ctx, const ofg_node_batch_t *batch) {
    shm_sink_t *s = (shm_sink_t *)ctx;
    if (!batch || batch->count == 0) return 0;

    uint32_t plen = ofg_wire_node_payload_bytes(batch->count);
    uint8_t *payload = calloc(1, plen);
    if (!payload) return -1;

    ofg_node_wire_t *wires = (ofg_node_wire_t *)payload;
    uint32_t str_cursor = (uint32_t)(batch->count * sizeof(ofg_node_wire_t));

    for (int32_t i = 0; i < batch->count; i++) {
        const ofg_node_t *n = &batch->items[i];
        ofg_node_wire_t *w = &wires[i];
        memset(w, 0, sizeof(*w));
        w->kind = 0; /* TODO: map label string → entity kind enum */
        w->start_line = (uint32_t)n->start_line;
        w->end_line = (uint32_t)n->end_line;

        if (n->qualified_name) {
            uint32_t len = (uint32_t)strlen(n->qualified_name);
            w->qualified_name_offset = str_cursor;
            w->qualified_name_length = len;
            memcpy(payload + str_cursor, n->qualified_name, len);
            str_cursor += len;
        }
        if (n->file_path) {
            uint32_t len = (uint32_t)strlen(n->file_path);
            w->file_path_offset = str_cursor;
            w->file_path_length = len;
            memcpy(payload + str_cursor, n->file_path, len);
            str_cursor += len;
        }
        if (n->properties_json) {
            uint32_t len = (uint32_t)strlen(n->properties_json);
            w->properties_offset = str_cursor;
            w->properties_length = len;
            memcpy(payload + str_cursor, n->properties_json, len);
            str_cursor += len;
        }
    }

    int rc = ring_write_frame(s, OFG_KIND_NODE_BATCH, s->frame_seq++,
                               payload, str_cursor, (uint32_t)batch->count);
    free(payload);
    return rc;
}

static int shm_write_edges(void *ctx, const ofg_edge_batch_t *batch) {
    shm_sink_t *s = (shm_sink_t *)ctx;
    if (!batch || batch->count == 0) return 0;

    uint32_t plen = (uint32_t)(batch->count * (int)sizeof(ofg_edge_wire_t));
    uint8_t *payload = calloc(1, plen + 4096);
    if (!payload) return -1;

    ofg_edge_wire_t *wires = (ofg_edge_wire_t *)payload;
    uint32_t str_cursor = (uint32_t)(batch->count * sizeof(ofg_edge_wire_t));

    for (int32_t i = 0; i < batch->count; i++) {
        const ofg_edge_t *e = &batch->items[i];
        ofg_edge_wire_t *w = &wires[i];
        memset(w, 0, sizeof(*w));
        if (e->properties_json) {
            uint32_t len = (uint32_t)strlen(e->properties_json);
            w->properties_offset = str_cursor;
            w->properties_length = len;
            memcpy(payload + str_cursor, e->properties_json, len);
            str_cursor += len;
        }
    }

    int rc = ring_write_frame(s, OFG_KIND_EDGE_BATCH, s->frame_seq++,
                               payload, str_cursor, (uint32_t)batch->count);
    free(payload);
    return rc;
}

static int shm_write_deletions(void *ctx, const ofg_delete_batch_t *batch) {
    /* For P3 MVP: deletions are rare; log count and skip for now. */
    (void)ctx;
    (void)batch;
    return 0;
}

static int shm_write_coverage(void *ctx, const ofg_coverage_batch_t *batch) {
    (void)ctx;
    (void)batch;
    return 0;
}

static int shm_write_diagnostics(void *ctx, const ofg_diagnostic_batch_t *batch) {
    (void)ctx;
    (void)batch;
    return 0;
}

static int shm_commit_snapshot(void *ctx) {
    shm_sink_t *s = (shm_sink_t *)ctx;
    ring_write_frame(s, OFG_KIND_SNAPSHOT_END, s->frame_seq++, NULL, 0, 0);
    atomic_store_explicit(&s->ctrl->state, 0, memory_order_release); /* idle */
    return 0;
}

static int shm_rollback_snapshot(void *ctx) {
    shm_sink_t *s = (shm_sink_t *)ctx;
    ring_write_frame(s, OFG_KIND_FATAL_ERROR, s->frame_seq++, NULL, 0, 0);
    atomic_store_explicit(&s->ctrl->state, 3, memory_order_release); /* error */
    return 0;
}

static void shm_destroy(void *ctx) {
    shm_sink_t *s = (shm_sink_t *)ctx;
    if (!s) return;
    if (s->shm_base)  munmap(s->shm_base, s->shm_size);
    if (s->data_ready_fd >= 0)  close(s->data_ready_fd);
    if (s->space_ready_fd >= 0) close(s->space_ready_fd);
    free(s);
}

/* ── Constructor ────────────────────────────────────────────────── */

ofg_graph_sink_t *ofg_shm_sink_create(uint8_t *shm_base, uint64_t shm_size,
                                        int data_ready_fd, int space_ready_fd) {
    if (!shm_base || shm_size < OFG_FRAMES_OFFSET + 4096) return NULL;

    shm_sink_t *s = calloc(1, sizeof(*s));
    if (!s) return NULL;

    s->shm_base       = shm_base;
    s->shm_size       = shm_size;
    s->ctrl           = (ofg_ctrl_t *)(shm_base + OFG_CTRL_OFFSET);
    s->ring           = (ofg_ring_meta_t *)(shm_base + OFG_RING_META_OFFSET);
    s->data_ready_fd  = data_ready_fd;
    s->space_ready_fd = space_ready_fd;

    /* Initialise shared control header (not zeroed by mmap). */
    if (s->ctrl->magic != OFG_WIRE_MAGIC) {
        memset(s->ctrl, 0, sizeof(*s->ctrl));
        s->ctrl->magic = OFG_WIRE_MAGIC;
        s->ctrl->abi_version = OFG_WIRE_ABI_VERSION;
    }

    ofg_graph_sink_t *sink = calloc(1, sizeof(*sink));
    if (!sink) { free(s); return NULL; }

    sink->ctx               = s;
    sink->begin_snapshot    = shm_begin_snapshot;
    sink->write_nodes       = shm_write_nodes;
    sink->write_edges       = shm_write_edges;
    sink->write_deletions   = shm_write_deletions;
    sink->write_coverage    = shm_write_coverage;
    sink->write_diagnostics = shm_write_diagnostics;
    sink->commit_snapshot   = shm_commit_snapshot;
    sink->rollback_snapshot = shm_rollback_snapshot;
    sink->destroy           = shm_destroy;

    return sink;
}
