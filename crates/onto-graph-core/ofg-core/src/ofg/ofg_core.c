/*
 * ofg_core.c — OntoFirmwareGraph C compute kernel (P2).
 *
 * Thin wrapper around cbm_pipeline_run.  The pipeline writes to SQLite
 * (Golden-verified path).  The sink receives begin/commit/rollback
 * lifecycle events.  Full batch export through sink is deferred to P3
 * (SharedMemoryGraphSink writes directly during pipeline execution).
 */
#include "ofg_core.h"
#include "pipeline/pipeline.h"
#include "foundation/log.h"
#include "foundation/mem.h"
#include "foundation/platform.h"

#include <stdlib.h>
#include <string.h>

struct ofg_core {
    int index_mode;    /* CBM_MODE_FULL / MODERATE / FAST */
};

ofg_core_t *ofg_core_create(void) {
    ofg_core_t *c = calloc(1, sizeof(*c));
    if (c) c->index_mode = CBM_MODE_FULL;
    return c;
}

int ofg_core_load_profile(ofg_core_t *c, const char *profile_json) {
    (void)profile_json;
    if (!c) return -1;
    /* Profile parsing deferred: CBM index_mode controls pass depth. */
    return 0;
}

void ofg_core_destroy(ofg_core_t *c) {
    free(c);
}

int ofg_core_analyze_baseline(ofg_core_t *c,
                               const char *repo_path,
                               const char *project_name,
                               ofg_graph_sink_t *sink) {
    if (!c || !repo_path || !sink) return -1;

    cbm_mem_init_with_cap(cbm_mem_ram_fraction_for_total(cbm_system_info().total_ram), 0);

    /* Notify sink: snapshot beginning. */
    ofg_snapshot_meta_t meta = {
        .repository_id        = project_name ? project_name : repo_path,
        .base_commit_sha      = "",
        .extractor_version    = "ofg-dev",
        .graph_schema_version = 1,
    };
    if (sink->begin_snapshot && sink->begin_snapshot(sink->ctx, &meta) != 0) {
        return -1;
    }

    /* Run the pipeline. This is the SAME code path as CLI mode,
     * Golden-verified: 27/40 C, 32/64 Python, 27/70 TS, 54/114 Java. */
    cbm_pipeline_t *p = cbm_pipeline_new(repo_path, NULL, (cbm_index_mode_t)c->index_mode);
    if (!p) {
        if (sink->rollback_snapshot) sink->rollback_snapshot(sink->ctx);
        return -1;
    }

    int rc = cbm_pipeline_run(p);
    cbm_pipeline_free(p);

    if (rc != 0) {
        cbm_log_error("ofg_core", "phase", "pipeline_run", "project",
                      project_name ? project_name : repo_path, "rc", rc);
        if (sink->rollback_snapshot) sink->rollback_snapshot(sink->ctx);
    } else {
        if (sink->commit_snapshot) sink->commit_snapshot(sink->ctx);
    }

    return rc;
}

int ofg_core_analyze_delta(ofg_core_t *c,
                            const char *repo_path,
                            const char *project_name,
                            const char **changed_files,
                            int changed_count,
                            ofg_graph_sink_t *sink) {
    (void)changed_files;
    (void)changed_count;
    /* Delta = full reindex for MVP. Incremental via pipeline_incremental
     * requires a warm store; that path is exercised by `cli sync`. */
    return ofg_core_analyze_baseline(c, repo_path, project_name, sink);
}
