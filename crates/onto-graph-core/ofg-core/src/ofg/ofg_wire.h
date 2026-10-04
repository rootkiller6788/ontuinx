/*
 * ofg_wire.h — Shared-memory Wire ABI for ofg-core ↔ ofg-service (P3).
 *
 * All structures use fixed-width integers and offset+length for strings.
 * NO native pointers cross the process boundary.  NO size_t, NO long,
 * NO compiler-dependent enums.  Every frame carries magic, ABI version,
 * length, and checksum.
 *
 * Ring layout (all offsets relative to mmap base):
 *
 *   [0]                    Control Header  (ofg_ctrl_t)
 *   [64]                   Ring Metadata   (ofg_ring_meta_t)
 *   [128]                  Frame 0
 *   [128 + frame_size]     Frame 1
 *   ...
 *   [ring_capacity - 1]    (end)
 */
#ifndef OFG_WIRE_H
#define OFG_WIRE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Constants ─────────────────────────────────────────────────── */

#define OFG_WIRE_MAGIC          0x4F464731u  /* "OFG1" */
#define OFG_WIRE_ABI_VERSION    1u
#define OFG_RING_DEFAULT_CAP    (256u * 1024u * 1024u)  /* 256 MiB */
#define OFG_RING_MIN_CAP        (64u  * 1024u * 1024u)  /*  64 MiB */
#define OFG_CTRL_OFFSET         0u
#define OFG_RING_META_OFFSET    64u
#define OFG_FRAMES_OFFSET       128u

/* ── Frame record kinds ────────────────────────────────────────── */

enum {
    OFG_KIND_SNAPSHOT_BEGIN     = 1,
    OFG_KIND_NODE_BATCH         = 2,
    OFG_KIND_EDGE_BATCH         = 3,
    OFG_KIND_DELETE_BATCH       = 4,
    OFG_KIND_COVERAGE_BATCH     = 5,
    OFG_KIND_DIAGNOSTIC_BATCH   = 6,
    OFG_KIND_SNAPSHOT_END       = 7,
    OFG_KIND_FATAL_ERROR        = 8,
    OFG_KIND_HEARTBEAT          = 9,
};

/* ── Control header (offset 0, 64 bytes) ───────────────────────── */

typedef struct {
    uint32_t magic;              /* OFG_WIRE_MAGIC */
    uint16_t abi_version;        /* OFG_WIRE_ABI_VERSION */
    uint16_t state;              /* 0=init, 1=running, 2=shutdown, 3=error */
    uint32_t error_code;         /* 0 = no error */
    uint64_t snapshot_epoch;     /* incremented per snapshot */
    uint64_t producer_seq;       /* C: last committed frame seq */
    uint64_t consumer_seq;       /* Rust: last consumed frame seq */
    uint8_t  cancel;             /* Rust → C: set to 1 to request stop */
    uint8_t  _pad[39];           /* reserved for future use */
} ofg_ctrl_t;

/* ── Ring metadata (offset 64, 64 bytes) ───────────────────────── */

typedef struct {
    uint64_t capacity;           /* total ring size in bytes */
    uint64_t write_offset;       /* C: next write position */
    uint64_t read_offset;        /* Rust: next read position */
    uint32_t wrap_generation;    /* incremented on wrap */
    uint32_t _pad;
    uint8_t  _reserved[36];      /* reserved for future use */
} ofg_ring_meta_t;

/* ── Frame header ──────────────────────────────────────────────── */

typedef struct {
    uint32_t magic;              /* OFG_WIRE_MAGIC */
    uint16_t abi_version;        /* OFG_WIRE_ABI_VERSION */
    uint16_t record_kind;        /* OFG_KIND_* */

    uint64_t sequence;           /* monotonic per-snapshot */
    uint64_t snapshot_epoch;     /* matches ctrl.snapshot_epoch */
    uint64_t payload_length;     /* bytes after this header */
    uint64_t total_frame_length; /* header (40) + payload */

    uint32_t record_count;       /* nodes/edges/etc in this batch */
    uint32_t checksum;           /* CRC32 of payload, 0 = no check */
} ofg_frame_header_t;

_Static_assert(sizeof(ofg_frame_header_t) == 48, "frame header size");

/* ── Node wire record (payload element) ────────────────────────── */

typedef struct {
    uint8_t  entity_id[16];      /* stable UUID for dedup across snapshots */
    uint32_t kind;               /* entity kind enum */
    uint32_t language;           /* CBMLanguage enum value */
    uint32_t stable_key_offset;  /* from frame payload start */
    uint32_t stable_key_length;
    uint32_t qualified_name_offset;
    uint32_t qualified_name_length;
    uint32_t file_path_offset;
    uint32_t file_path_length;
    uint32_t properties_offset;
    uint32_t properties_length;
    uint32_t start_line;
    uint32_t end_line;
    uint32_t flags;              /* is_exported, is_test, etc. */
} ofg_node_wire_t;

_Static_assert(sizeof(ofg_node_wire_t) == 68, "node wire size");

/* ── Edge wire record (payload element) ────────────────────────── */

typedef struct {
    uint8_t  source_entity_id[16];
    uint8_t  target_entity_id[16];
    uint32_t edge_kind;           /* CALLS, POINTS_TO, DEFINES, ... */
    uint32_t callsite_key_offset;
    uint32_t callsite_key_length;
    uint32_t semantic_slot_offset;
    uint32_t semantic_slot_length;
    uint32_t properties_offset;
    uint32_t properties_length;
} ofg_edge_wire_t;

_Static_assert(sizeof(ofg_edge_wire_t) == 60, "edge wire size");

/* ── Coverage wire record ──────────────────────────────────────── */

typedef struct {
    uint32_t file_path_offset;
    uint32_t file_path_length;
    uint32_t kind_offset;
    uint32_t kind_length;
    uint32_t detail_offset;
    uint32_t detail_length;
} ofg_coverage_wire_t;

/* ── Helpers for computing offsets ─────────────────────────────── */

/* Write a string into the payload byte region, return its offset from
 * the payload start.  Caller ensures payload has enough room. */
static inline uint32_t ofg_wire_write_string(uint8_t *payload, uint32_t *cursor,
                                              const char *s, uint32_t len) {
    uint32_t off = *cursor;
    if (s && len > 0) {
        __builtin_memcpy(payload + off, s, len);
        *cursor = off + len;
    }
    return off;
}

/* Number of bytes needed for a node batch payload. */
static inline uint32_t ofg_wire_node_payload_bytes(int count) {
    return (uint32_t)(count * (int)sizeof(ofg_node_wire_t));
}

/* Number of bytes needed for an edge batch payload. */
static inline uint32_t ofg_wire_edge_payload_bytes(int count) {
    return (uint32_t)(count * (int)sizeof(ofg_edge_wire_t));
}

#ifdef __cplusplus
}
#endif

#endif /* OFG_WIRE_H */
