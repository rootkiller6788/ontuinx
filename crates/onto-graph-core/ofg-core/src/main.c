/*
 * main_headless.c — OntoFirmwareGraph headless entry point (P2.5 / P3).
 *
 * Modes:
 *   (default)             Print help
 *   cli index  <path>     Index a repository (SQLite, dev/debug)
 *   cli sync   <path>     Incremental sync (SQLite, dev/debug)
 *   --worker              P3 shared-memory worker mode
 *     --shm-fd=<fd>       memfd file descriptor
 *     --data-event-fd=<fd>  C→Rust notification
 *     --space-event-fd=<fd> Rust→C backpressure
 *   cli status <project>  Show index status
 *   --version             Print version
 *   --help                Print this help
 *
 * No daemon.  No MCP.  No UI.  Direct pipeline access only.
 */
#include "cbm.h"
#include "store/store.h"
#include "foundation/constants.h"
#include "foundation/log.h"
#include "foundation/mem.h"
#include "foundation/platform.h"
#include "foundation/compat.h"
#include "foundation/compat_fs.h"
#include "pipeline/pipeline.h"
#include "graph_buffer/graph_buffer.h"
#include "ofg/ofg_wire.h"
#include "ofg/ofg_shm_sink.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdatomic.h>
#include <sys/mman.h>
#include <unistd.h>

#ifndef OFG_VERSION
#define OFG_VERSION "ofg-dev"
#endif

/* ── Minimal CLI usage ──────────────────────────────────────────── */

static const char *USAGE =
    "ontofirmwaregraph " OFG_VERSION "\n"
    "\n"
    "Usage:\n"
    "  ontofirmwaregraph cli index <repo-path> [--name <project>]\n"
    "  ontofirmwaregraph --worker --shm-fd=<fd> ...  (P3 shared memory)\n"
    "  ontofirmwaregraph --version\n"
    "  ontofirmwaregraph --help\n";

/* ── Basename ───────────────────────────────────────────────────── */

static const char *basename_of(const char *path) {
    const char *s = strrchr(path, '/');
    return s ? s + 1 : path;
}

/* ── Index ──────────────────────────────────────────────────────── */

static int cmd_index(int argc, char **argv) {
    const char *repo_path = NULL;
    const char *project_name = NULL;

    for (int i = 0; i < argc; i++) {
        if (strcmp(argv[i], "--name") == 0 && i + 1 < argc) {
            project_name = argv[++i];
        } else if (argv[i][0] != '-') {
            repo_path = argv[i];
        }
    }

    if (!repo_path) {
        fprintf(stderr, "error: repo-path is required\n");
        return 1;
    }
    if (!project_name) project_name = basename_of(repo_path);

    cbm_mem_init_with_cap(cbm_mem_ram_fraction_for_total(cbm_system_info().total_ram), 0);

    cbm_pipeline_t *p = cbm_pipeline_new(repo_path, NULL, CBM_MODE_FULL);
    if (!p) {
        fprintf(stderr, "error: failed to create pipeline\n");
        return 1;
    }

    /* Override project name if provided. */
    /* (cbm_pipeline_new derives it from the path; we pass the configured name below) */

    int rc = cbm_pipeline_run(p);
    if (rc != 0) {
        fprintf(stderr, "error: pipeline failed (rc=%d)\n", rc);
        cbm_pipeline_free(p);
        return 1;
    }

    fprintf(stderr, "indexed: project=%s (rc=%d)\n", project_name, rc);
    cbm_pipeline_free(p);
    return 0;
}

/* ── CLI dispatch ───────────────────────────────────────────────── */

static int run_headless_cli(int argc, char **argv) {
    if (argc < 1) {
        fprintf(stderr, "%s", USAGE);
        return 1;
    }
    if (strcmp(argv[0], "index") == 0)
        return cmd_index(argc - 1, argv + 1);

    fprintf(stderr, "error: unknown subcommand '%s'\n%s", argv[0], USAGE);
    return 1;
}

/* ── Worker mode (P3 shared memory) ────────────────────────────── */

static int run_worker(int shm_fd, int data_fd, int space_fd,
                      const char *repo_path, const char *project_name) {
    /* Map shared memory. */
    uint64_t shm_size = OFG_RING_DEFAULT_CAP;
    uint8_t *shm_base = mmap(NULL, shm_size, PROT_READ | PROT_WRITE,
                              MAP_SHARED, shm_fd, 0);
    if (shm_base == MAP_FAILED) {
        fprintf(stderr, "worker: mmap failed\n");
        return 1;
    }

    /* Create sink. */
    ofg_graph_sink_t *sink = ofg_shm_sink_create(shm_base, shm_size, data_fd, space_fd);
    if (!sink) {
        fprintf(stderr, "worker: sink creation failed\n");
        munmap(shm_base, shm_size);
        return 1;
    }

    /* Init and go. */
    cbm_mem_init_with_cap(cbm_mem_ram_fraction_for_total(cbm_system_info().total_ram), 0);
    cbm_pipeline_t *p = cbm_pipeline_new(repo_path, NULL, CBM_MODE_FULL);
    if (!p) {
        fprintf(stderr, "worker: pipeline creation failed\n");
        sink->destroy(sink->ctx);
        munmap(shm_base, shm_size);
        return 1;
    }

    int rc = cbm_pipeline_run(p);
    cbm_pipeline_free(p);

    /* Signal end. */
    ofg_ctrl_t *ctrl = (ofg_ctrl_t *)(shm_base + OFG_CTRL_OFFSET);
    atomic_store_explicit(&ctrl->state, rc == 0 ? 2u : 3u, memory_order_release);

    sink->destroy(sink->ctx);
    munmap(shm_base, shm_size);
    return rc != 0;
}

/* ── main ───────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    cbm_alloc_init();

    /* P3 worker mode: short-circuit before normal parsing. */
    {
        int shm_fd = -1, data_fd = -1, space_fd = -1;
        const char *repo = NULL, *project = NULL;
        bool worker = false;

        for (int i = 1; i < argc; i++) {
            if (strcmp(argv[i], "--worker") == 0) { worker = true; }
            else if (strncmp(argv[i], "--shm-fd=", 9) == 0)  { shm_fd = atoi(argv[i] + 9); }
            else if (strncmp(argv[i], "--data-event-fd=", 16) == 0) { data_fd = atoi(argv[i] + 16); }
            else if (strncmp(argv[i], "--space-event-fd=", 17) == 0) { space_fd = atoi(argv[i] + 17); }
            else if (strncmp(argv[i], "--repo=", 7) == 0) { repo = argv[i] + 7; }
            else if (strncmp(argv[i], "--project=", 10) == 0) { project = argv[i] + 10; }
        }

        if (worker && shm_fd >= 0 && repo) {
            if (!project) { project = strrchr(repo, '/'); project = project ? project + 1 : repo; }
            return run_worker(shm_fd, data_fd >= 0 ? data_fd : -1,
                              space_fd >= 0 ? space_fd : -1, repo, project);
        }
    }

    if (argc < 2) {
        fprintf(stderr, "%s", USAGE);
        return 0;
    }

    /* Global flags */
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--version") == 0) {
            printf("ontofirmwaregraph %s\n", OFG_VERSION);
            return 0;
        }
        if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            fprintf(stderr, "%s", USAGE);
            return 0;
        }
    }

    /* Subcommand dispatch */
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "cli") == 0) {
            return run_headless_cli(argc - i - 1, argv + i + 1);
        }
    }

    fprintf(stderr, "%s", USAGE);
    return 0;
}
