-- P16-P2: PostgreSQL schema for Assurance persistence

CREATE TABLE IF NOT EXISTS p16_assurance_runs (
    run_id          TEXT PRIMARY KEY,
    state           TEXT NOT NULL DEFAULT 'created',
    plan_json       TEXT,
    evidence_json   TEXT,
    evidence_referenced BOOLEAN DEFAULT FALSE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS p16_verdicts (
    verdict_id      TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL UNIQUE REFERENCES p16_assurance_runs(run_id),
    verdict_json    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS p16_assurance_failures (
    failure_id      TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL UNIQUE REFERENCES p16_assurance_runs(run_id),
    failure_json    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A run can have EITHER a verdict OR a failure, not both.
-- Enforced by the UNIQUE constraint on run_id in both p16_verdicts and p16_assurance_failures,
-- plus CHECK that a run_id doesn't appear in both tables (application-level + query).

CREATE TABLE IF NOT EXISTS p16_directives (
    decision_id     TEXT PRIMARY KEY,
    directive_json  TEXT NOT NULL,
    state           TEXT NOT NULL DEFAULT 'pending',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS p16_transition_receipts (
    receipt_id      TEXT PRIMARY KEY,
    decision_id     TEXT NOT NULL UNIQUE REFERENCES p16_directives(decision_id),
    receipt_json    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- P16-P2E: Unified outcome table enforces Verdict XOR Failure
CREATE TABLE IF NOT EXISTS p16_assurance_outcomes (
    assurance_run_id TEXT PRIMARY KEY REFERENCES p16_assurance_runs(run_id),
    outcome_kind     TEXT NOT NULL CHECK (outcome_kind IN ('verdict', 'failure')),
    verdict_id       TEXT REFERENCES p16_verdicts(verdict_id),
    failure_id       TEXT REFERENCES p16_assurance_failures(failure_id),
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (outcome_kind = 'verdict' AND verdict_id IS NOT NULL AND failure_id IS NULL)
        OR
        (outcome_kind = 'failure' AND verdict_id IS NULL AND failure_id IS NOT NULL)
    )
);
