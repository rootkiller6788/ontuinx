/*
 * ofg_core.h — OntoFirmwareGraph C compute kernel API (P2).
 *
 * The C Core is the pure-compute kernel extracted from CBM.  It owns:
 *   discover → parse → extract → resolve → graph buffer → batch export
 *
 * It does NOT own:
 *   - SQLite / PostgreSQL (the caller provides a ofg_graph_sink_t)
 *   - MCP / HTTP / CLI / daemon lifecycle
 *   - Installer / telemetry / agent config
 *
 * Thread safety:  ofg_core_t is NOT thread-safe.  One core per snapshot.
 *                 The caller may run multiple cores concurrently in separate
 *                 processes or threads with NO shared state.
 */
#ifndef OFG_CORE_H
#define OFG_CORE_H

#include "ofg_graph_sink.h"
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Opaque handle ──────────────────────────────────────────────── */

typedef struct ofg_core ofg_core_t;

/* ── Lifecycle ──────────────────────────────────────────────────── */

/* Create a new core.  Returns NULL on allocation failure. */
ofg_core_t *ofg_core_create(void);

/* Load an analysis profile.  profile_json is a JSON string with:
 *   {
 *     "language_overrides": { ".ext": "language" },
 *     "exclude_patterns": ["node_modules", ".git"],
 *     "mode": "full" | "moderate" | "fast"
 *   }
 * Returns 0 on success, -1 on parse failure. */
int ofg_core_load_profile(ofg_core_t *c, const char *profile_json);

/* ── Analysis entry points ──────────────────────────────────────── */

/* Full baseline index: discover all files, parse, resolve, and write
 * every node/edge/coverage row to `sink`.
 *
 * repo_path:    absolute path to repository root
 * project_name: stable project identifier (used in qualified names)
 * sink:         output target (SQLiteCompatibilitySink, etc.)
 *
 * Returns 0 on success, non-zero on error. */
int ofg_core_analyze_baseline(ofg_core_t *c,
                              const char *repo_path,
                              const char *project_name,
                              ofg_graph_sink_t *sink);

/* Candidate delta index: only re-index changed_files against the
 * existing graph state represented by `sink`.  The sink MUST have
 * been previously populated by ofg_core_analyze_baseline() for the
 * same project.
 *
 * changed_files: NULL-terminated array of relative paths, or NULL
 *                to re-discover all changes automatically.
 *
 * Returns 0 on success, non-zero on error. */
int ofg_core_analyze_delta(ofg_core_t *c,
                           const char *repo_path,
                           const char *project_name,
                           const char **changed_files,
                           int changed_count,
                           ofg_graph_sink_t *sink);

/* ── Teardown ───────────────────────────────────────────────────── */

/* Free all resources owned by the core.  NULL-safe. */
void ofg_core_destroy(ofg_core_t *c);

#ifdef __cplusplus
}
#endif

#endif /* OFG_CORE_H */
