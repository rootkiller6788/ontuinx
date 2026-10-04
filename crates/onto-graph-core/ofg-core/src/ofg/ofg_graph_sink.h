/*
 * ofg_graph_sink.h — GraphSink abstraction for OntoFirmwareGraph P2.
 *
 * The GraphSink is the single output boundary of the C compute kernel.
 * Every node, edge, deletion, coverage row, and diagnostic passes through
 * a sink implementation.  Two adapters ship with P2:
 *
 *   SQLiteCompatibilitySink  — writes to existing CBM SQLite store (P2–P3)
 *   PostgreSQLBatchSink      — COPY-based PostgreSQL writer (P4)
 *
 * Switching storage backends means swapping the sink, NOT rewriting the
 * pipeline.
 */
#ifndef OFG_GRAPH_SINK_H
#define OFG_GRAPH_SINK_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Snapshot metadata ──────────────────────────────────────────── */

typedef struct ofg_snapshot_meta {
    const char *repository_id;          /* UUID */
    const char *base_commit_sha;        /* git SHA or empty */
    const char *candidate_checkpoint_hash;
    uint64_t    execution_generation;
    const char *analysis_profile_hash;
    const char *extractor_version;
    uint32_t    graph_schema_version;
} ofg_snapshot_meta_t;

/* ── Batch types ─────────────────────────────────────────────────── */

/* One node in a batch.  Mirrors cbm_gbuf_node_t but is standalone
 * (no heap ownership by the buffer — the sink copies what it needs). */
typedef struct ofg_node {
    const char *label;           /* "Function", "Class", "CallbackSlot", ... */
    const char *name;            /* short name */
    const char *qualified_name;  /* project-relative QN */
    const char *file_path;       /* relative path */
    int32_t     start_line;
    int32_t     end_line;
    const char *properties_json; /* "{}" default */
} ofg_node_t;

/* Serialised batch of nodes.  Caller owns the items array; sink copies. */
typedef struct ofg_node_batch {
    const ofg_node_t *items;
    int32_t           count;
} ofg_node_batch_t;

/* One edge. */
typedef struct ofg_edge {
    const char *source_qn;       /* qualified_name of source node */
    const char *target_qn;       /* qualified_name of target node */
    const char *type;            /* "CALLS", "POINTS_TO", ... */
    const char *properties_json; /* "{}" default */
    /* Optional: for dedup and stable-key support. */
    const char *callsite_key;    /* NULL if not applicable */
    const char *semantic_slot;   /* NULL if not applicable */
} ofg_edge_t;

typedef struct ofg_edge_batch {
    const ofg_edge_t *items;
    int32_t           count;
} ofg_edge_batch_t;

/* Deletion batch — nodes to purge before re-adding (incremental). */
typedef struct ofg_delete_batch {
    const char **qualified_names;  /* QNs to delete */
    int32_t      count;
} ofg_delete_batch_t;

/* Coverage row. */
typedef struct ofg_coverage_row {
    const char *rel_path;
    const char *kind;            /* "parse_partial", "read", "extract", ... */
    const char *detail;          /* line ranges or "" */
} ofg_coverage_row_t;

typedef struct ofg_coverage_batch {
    const ofg_coverage_row_t *items;
    int32_t                   count;
} ofg_coverage_batch_t;

/* Diagnostic. */
typedef struct ofg_diagnostic {
    const char *rel_path;        /* may be NULL for project-level */
    const char *pass_name;
    const char *level;           /* "ERROR", "WARN", "INFO" */
    const char *message;
    const char *detail_json;     /* "{}" default */
} ofg_diagnostic_t;

typedef struct ofg_diagnostic_batch {
    const ofg_diagnostic_t *items;
    int32_t                 count;
} ofg_diagnostic_batch_t;

/* ── GraphSink function table ───────────────────────────────────── */

typedef struct ofg_graph_sink ofg_graph_sink_t;

struct ofg_graph_sink {
    /* Opaque context — owned by the sink implementation. */
    void *ctx;

    /* Lifecycle.  Called once per snapshot. */
    int (*begin_snapshot)(void *ctx, const ofg_snapshot_meta_t *meta);

    /* Write batches.  May be called multiple times. */
    int (*write_nodes)(void *ctx, const ofg_node_batch_t *batch);
    int (*write_edges)(void *ctx, const ofg_edge_batch_t *batch);
    int (*write_deletions)(void *ctx, const ofg_delete_batch_t *batch);
    int (*write_coverage)(void *ctx, const ofg_coverage_batch_t *batch);
    int (*write_diagnostics)(void *ctx, const ofg_diagnostic_batch_t *batch);

    /* Finalisation.  After commit the sink is reusable for another snapshot. */
    int (*commit_snapshot)(void *ctx);
    int (*rollback_snapshot)(void *ctx);

    /* Destructor. */
    void (*destroy)(void *ctx);
};

#ifdef __cplusplus
}
#endif

#endif /* OFG_GRAPH_SINK_H */
