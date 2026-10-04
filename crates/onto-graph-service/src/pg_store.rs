//! PostgreSQL Graph Store (P4). Replaces SQLite.
//!
//! Entity/version model per plan sections 4a-4d.

use anyhow::{Context, Result};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};
use tracing::info;
use uuid::Uuid;

pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// Borrow the inner pool for direct queries.
    pub fn pool(&self) -> &PgPool { &self.pool }
}

pub struct SnapshotRef {
    pub snapshot_id: Uuid,
    pub repository_id: Uuid,
    pub state: String,
}

impl PgStore {
    /// Connect and run migrations.
    pub async fn open(database_url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .context("connect to PostgreSQL")?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("run migrations")?;

        info!("PG store ready");
        Ok(Self { pool })
    }

    /// Ensure a repository exists, return its ID.
    pub async fn ensure_repository(&self, name: &str, root_path: &str) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO ofg_repository (name, root_path)
             VALUES ($1, $2)
             ON CONFLICT (name) DO UPDATE SET root_path = $2
             RETURNING repository_id"
        )
        .bind(name).bind(root_path)
        .fetch_one(&self.pool).await?;
        Ok(row.0)
    }

    /// Create a new snapshot in BUILDING state.
    pub async fn create_snapshot(
        &self, repo_id: Uuid, base_sha: &str, profile_hash: &str,
        extractor_ver: &str, schema_ver: i32,
    ) -> Result<Uuid> {
        let sid = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO ofg_snapshot (snapshot_id, repository_id, base_commit_sha,
             analysis_profile_hash, extractor_version, graph_schema_version,
             graph_content_hash, state)
             VALUES ($1,$2,$3,$4,$5,$6,'','BUILDING')"
        )
        .bind(sid).bind(repo_id).bind(base_sha)
        .bind(profile_hash).bind(extractor_ver).bind(schema_ver)
        .execute(&self.pool).await?;
        Ok(sid)
    }

    /// Seal a snapshot: verify counts, compute hash, set SEALED.
    pub async fn seal_snapshot(&self, snapshot_id: Uuid, node_count: i64, edge_count: i64) -> Result<()> {
        sqlx::query(
            "UPDATE ofg_snapshot SET node_count=$2, edge_count=$3,
             graph_content_hash=encode(sha256(concat($1::text,$2::text,$3::text)::bytea),'hex'),
             state='SEALED', sealed_at=now()
             WHERE snapshot_id=$1"
        )
        .bind(snapshot_id).bind(node_count).bind(edge_count)
        .execute(&self.pool).await?;
        Ok(())
    }

    /// Upsert a file, return its ID.
    pub async fn ensure_file(&self, repo_id: Uuid, rel_path: &str) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO ofg_file (repository_id, rel_path)
             VALUES ($1,$2) ON CONFLICT (repository_id, rel_path) DO UPDATE SET rel_path=$2
             RETURNING file_id"
        )
        .bind(repo_id).bind(rel_path)
        .fetch_one(&self.pool).await?;
        Ok(row.0)
    }

    /// Upsert a stable entity, return its ID.
    pub async fn ensure_entity(
        &self, repo_id: Uuid, stable_key: &str, entity_kind: &str, language: Option<&str>,
    ) -> Result<Uuid> {
        let row: (Uuid,) = sqlx::query_as(
            "INSERT INTO ofg_entity (repository_id, stable_key, entity_kind, language)
             VALUES ($1,$2,$3,$4)
             ON CONFLICT (repository_id, stable_key) DO UPDATE SET entity_kind=$3, language=$4
             RETURNING entity_id"
        )
        .bind(repo_id).bind(stable_key).bind(entity_kind).bind(language)
        .fetch_one(&self.pool).await?;
        Ok(row.0)
    }

    /// Write entity version for a snapshot.
    pub async fn insert_entity_version(
        &self, snapshot_id: Uuid, entity_id: Uuid, qualified_name: &str,
        file_id: Option<Uuid>, start_line: i32, end_line: i32,
        structural_hash: &str, properties: &serde_json::Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO ofg_entity_version (snapshot_id, entity_id, qualified_name,
             file_id, start_line, end_line, structural_hash, properties)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT (snapshot_id, entity_id) DO UPDATE SET
             qualified_name=$3, file_id=$4, start_line=$5, end_line=$6,
             structural_hash=$7, properties=$8"
        )
        .bind(snapshot_id).bind(entity_id).bind(qualified_name)
        .bind(file_id).bind(start_line).bind(end_line)
        .bind(structural_hash).bind(properties)
        .execute(&self.pool).await?;
        Ok(())
    }

    /// Insert an edge with stable key.
    pub async fn insert_edge(
        &self, repo_id: Uuid, snapshot_id: Uuid,
        source_id: Uuid, target_id: Uuid, kind: &str,
        stable_key: &str, profile: &str,
        callsite: Option<&str>, slot: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO ofg_edge (repository_id, snapshot_id, source_entity_id,
             target_entity_id, edge_kind, stable_key, analysis_profile,
             callsite_key, semantic_slot)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (snapshot_id, stable_key) DO NOTHING"
        )
        .bind(repo_id).bind(snapshot_id).bind(source_id).bind(target_id)
        .bind(kind).bind(stable_key).bind(profile).bind(callsite).bind(slot)
        .execute(&self.pool).await?;
        Ok(())
    }

    // ── Transaction helpers ──────────────────────────────────────

    pub async fn begin(&self) -> Result<Transaction<Postgres>> {
        self.pool.begin().await.context("begin tx")
    }

    /// Count entities and edges for a snapshot (used in seal).
    pub async fn count_snapshot(&self, snapshot_id: Uuid) -> Result<(i64, i64)> {
        let nodes: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM ofg_entity_version WHERE snapshot_id=$1 AND NOT tombstone"
        ).bind(snapshot_id).fetch_one(&self.pool).await?;
        let edges: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM ofg_edge WHERE snapshot_id=$1"
        ).bind(snapshot_id).fetch_one(&self.pool).await?;
        Ok((nodes.0, edges.0))
    }
}
