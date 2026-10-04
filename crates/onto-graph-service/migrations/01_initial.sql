-- OntoFirmwareGraph P4: PostgreSQL Graph Store
-- Database: onto_firmware_graph
-- Replaces SQLite with versioned entity/edge model

CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ── Repository ──────────────────────────────────────────────────

CREATE TABLE ofg_repository (
    repository_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name          TEXT NOT NULL UNIQUE,
    root_path     TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Snapshot ─────────────────────────────────────────────────────

CREATE TABLE ofg_snapshot (
    snapshot_id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id            UUID NOT NULL REFERENCES ofg_repository,
    base_commit_sha          TEXT NOT NULL,
    candidate_checkpoint_hash TEXT,
    execution_generation     BIGINT DEFAULT 0,
    analysis_profile_hash    TEXT NOT NULL,
    extractor_version        TEXT NOT NULL,
    graph_schema_version     INT NOT NULL,
    graph_content_hash       TEXT NOT NULL,
    node_count               BIGINT NOT NULL DEFAULT 0,
    edge_count               BIGINT NOT NULL DEFAULT 0,
    coverage_status          TEXT NOT NULL DEFAULT 'BUILDING',
    state                    TEXT NOT NULL DEFAULT 'BUILDING',
    created_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    sealed_at                TIMESTAMPTZ,
    CONSTRAINT chk_snapshot_state CHECK (state IN ('BUILDING','SEALED','INVALID','DISCARDED','PROMOTED'))
);

-- ── File ─────────────────────────────────────────────────────────

CREATE TABLE ofg_file (
    file_id       UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id UUID NOT NULL REFERENCES ofg_repository,
    rel_path      TEXT NOT NULL,
    UNIQUE(repository_id, rel_path)
);

CREATE TABLE ofg_file_version (
    snapshot_id    UUID NOT NULL REFERENCES ofg_snapshot,
    file_id        UUID NOT NULL REFERENCES ofg_file,
    content_sha256 TEXT NOT NULL,
    size_bytes     BIGINT,
    language       TEXT,
    PRIMARY KEY(snapshot_id, file_id)
);

-- ── Entity (stable, cross-snapshot) ──────────────────────────────

CREATE TABLE ofg_entity (
    entity_id     UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id UUID NOT NULL REFERENCES ofg_repository,
    stable_key    TEXT NOT NULL,
    entity_kind   TEXT NOT NULL,
    language      TEXT,
    UNIQUE(repository_id, stable_key)
);

-- ── Entity Version (per-snapshot facts) ──────────────────────────

CREATE TABLE ofg_entity_version (
    snapshot_id      UUID NOT NULL REFERENCES ofg_snapshot,
    entity_id        UUID NOT NULL REFERENCES ofg_entity,
    qualified_name   TEXT,
    file_id          UUID REFERENCES ofg_file,
    start_line       INT,
    end_line         INT,
    structural_hash  TEXT NOT NULL,
    properties       JSONB NOT NULL DEFAULT '{}',
    tombstone        BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(snapshot_id, entity_id)
);

-- ── Edge ─────────────────────────────────────────────────────────

CREATE TABLE ofg_edge (
    edge_id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    repository_id     UUID NOT NULL REFERENCES ofg_repository,
    snapshot_id       UUID NOT NULL REFERENCES ofg_snapshot,
    source_entity_id  UUID NOT NULL REFERENCES ofg_entity,
    target_entity_id  UUID NOT NULL REFERENCES ofg_entity,
    edge_kind         TEXT NOT NULL,
    callsite_key      TEXT,
    semantic_slot     TEXT,
    analysis_profile  TEXT NOT NULL,
    properties        JSONB NOT NULL DEFAULT '{}',
    stable_key        TEXT NOT NULL,
    UNIQUE(snapshot_id, stable_key)
);

-- ── Candidate Delta ──────────────────────────────────────────────

CREATE TABLE ofg_candidate_delta (
    delta_id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    snapshot_id               UUID NOT NULL REFERENCES ofg_snapshot,
    attempt_id                TEXT NOT NULL,
    execution_generation      BIGINT NOT NULL,
    candidate_checkpoint_hash TEXT NOT NULL,
    added_files               UUID[],
    changed_files             UUID[],
    deleted_files             UUID[],
    renamed_files             JSONB
);

-- ── Pass Coverage ────────────────────────────────────────────────

CREATE TABLE ofg_pass_coverage (
    snapshot_id UUID NOT NULL REFERENCES ofg_snapshot,
    entity_id   UUID NOT NULL REFERENCES ofg_entity,
    pass_name   TEXT NOT NULL,
    status      TEXT NOT NULL,
    detail      JSONB,
    PRIMARY KEY(snapshot_id, entity_id, pass_name)
);

-- ── Diagnostic ───────────────────────────────────────────────────

CREATE TABLE ofg_diagnostic (
    diagnostic_id BIGSERIAL PRIMARY KEY,
    snapshot_id   UUID NOT NULL REFERENCES ofg_snapshot,
    entity_id     UUID REFERENCES ofg_entity,
    pass_name     TEXT NOT NULL,
    level         TEXT NOT NULL,
    message       TEXT NOT NULL,
    detail        JSONB
);

-- ── Execution Projection ─────────────────────────────────────────

CREATE TABLE ofg_execution_projection (
    projection_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    event_id      TEXT NOT NULL UNIQUE,
    event_type    TEXT NOT NULL,
    aggregate_id  TEXT NOT NULL,
    payload       JSONB NOT NULL DEFAULT '{}',
    projected_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── P8: Dead Letter Queue ─────────────────────────────────────────

CREATE TABLE IF NOT EXISTS ofg_dead_letter (
    event_id     TEXT PRIMARY KEY,
    error        TEXT NOT NULL,
    recorded_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── P8: Projection Offset (for Exactly-Once consumption tracking) ──

CREATE TABLE IF NOT EXISTS ofg_projection_offset (
    event_id    TEXT PRIMARY KEY,
    consumed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── B-tree indexes ───────────────────────────────────────────────

CREATE INDEX idx_ofg_entity_repo       ON ofg_entity(repository_id);
CREATE INDEX idx_ofg_entity_kind       ON ofg_entity(repository_id, entity_kind);
CREATE INDEX idx_ofg_entity_version_qn ON ofg_entity_version(qualified_name);
CREATE INDEX idx_ofg_entity_version_f  ON ofg_entity_version(file_id);
CREATE INDEX idx_ofg_edge_source       ON ofg_edge(source_entity_id, edge_kind);
CREATE INDEX idx_ofg_edge_target       ON ofg_edge(target_entity_id, edge_kind);
CREATE INDEX idx_ofg_edge_kind         ON ofg_edge(repository_id, edge_kind);
CREATE INDEX idx_ofg_edge_stable       ON ofg_edge(stable_key);
CREATE INDEX idx_ofg_snapshot_repo     ON ofg_snapshot(repository_id, state);
CREATE INDEX idx_ofg_file_repo         ON ofg_file(repository_id);

-- ── Full-text search (simple stemmer for code identifiers) ──────

CREATE INDEX idx_ofg_entity_version_fts
    ON ofg_entity_version
    USING GIN (to_tsvector('simple',
        coalesce(qualified_name, '') || ' ' || coalesce(structural_hash, '')));

-- ── Trigram indexes for fuzzy symbol matching ────────────────────

CREATE INDEX idx_ofg_entity_version_name_trgm
    ON ofg_entity_version USING GIN (qualified_name gin_trgm_ops);
CREATE INDEX idx_ofg_entity_stable_trgm
    ON ofg_entity USING GIN (stable_key gin_trgm_ops);
