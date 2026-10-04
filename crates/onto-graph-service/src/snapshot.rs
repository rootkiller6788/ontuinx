//! Snapshot state machine (P5).
//!
//! Lifecycle:
//!   BUILDING → SEALED → PROMOTED (new baseline)
//!                    → DISCARDED (rejected)
//!   BUILDING → INVALID  (mid-build failure)
//!
//! Only SEALED snapshots can be used for Assurance.

use crate::merkle::{self, FileFact};
use crate::pg_store::PgStore;
use anyhow::Result;
use std::collections::BTreeMap;
use tracing::info;
use uuid::Uuid;

/// Snapshot lifecycle states.
pub enum SnapshotState {
    Building,
    Sealed,
    Invalid,
    Discarded,
    Promoted,
}

impl SnapshotState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Building => "BUILDING",
            Self::Sealed => "SEALED",
            Self::Invalid => "INVALID",
            Self::Discarded => "DISCARDED",
            Self::Promoted => "PROMOTED",
        }
    }
}

/// Reference to a sealed snapshot.
#[derive(Debug, Clone)]
pub struct GraphSnapshotRef {
    pub snapshot_id: Uuid,
    pub repository_id: Uuid,
    pub base_commit_sha: String,
    pub candidate_checkpoint_hash: Option<String>,
    pub execution_generation: u64,
    pub analysis_profile_hash: String,
    pub extractor_version: String,
    pub graph_schema_version: u32,
    pub graph_content_hash: String,
    pub node_count: i64,
    pub edge_count: i64,
    pub coverage_status: String,
}

/// Snapshot manager wrapping a PgStore.
pub struct SnapshotManager {
    store: PgStore,
    repository_id: Uuid,
}

impl SnapshotManager {
    pub fn new(store: PgStore, repository_id: Uuid) -> Self {
        Self { store, repository_id }
    }

    /// Begin a new baseline snapshot.
    pub async fn begin_baseline(
        &self, base_sha: &str, profile_hash: &str,
        extractor_ver: &str, schema_ver: i32,
    ) -> Result<Uuid> {
        let sid = self.store.create_snapshot(
            self.repository_id, base_sha, profile_hash, extractor_ver, schema_ver,
        ).await?;
        info!(snapshot_id = %sid, "baseline snapshot BUILDING");
        Ok(sid)
    }

    /// Begin a candidate snapshot from a baseline.
    pub async fn begin_candidate(
        &self, base_sha: &str, checkpoint_hash: &str,
        generation: u64, profile_hash: &str,
        extractor_ver: &str, schema_ver: i32,
    ) -> Result<Uuid> {
        let sid = self.store.create_snapshot(
            self.repository_id, base_sha, profile_hash, extractor_ver, schema_ver,
        ).await?;
        // Update with candidate-specific fields.
        sqlx::query(
            "UPDATE ofg_snapshot SET candidate_checkpoint_hash=$2, execution_generation=$3
             WHERE snapshot_id=$1"
        )
        .bind(sid).bind(checkpoint_hash).bind(generation as i64)
        .execute(self.store.pool()).await?;
        info!(snapshot_id = %sid, gen = generation, "candidate snapshot BUILDING");
        Ok(sid)
    }

    /// Seal a snapshot: compute Merkle root, set SEALED.
    pub async fn seal(
        &self, snapshot_id: Uuid, files: &[FileFact],
        previous_hashes: &BTreeMap<Uuid, String>,
    ) -> Result<GraphSnapshotRef> {
        let root = merkle::compute_snapshot_root(files, previous_hashes);
        let (nodes, edges) = self.store.count_snapshot(snapshot_id).await?;

        self.store.seal_snapshot(snapshot_id, nodes, edges).await?;
        // Update with the computed Merkle root.
        sqlx::query(
            "UPDATE ofg_snapshot SET graph_content_hash=$2 WHERE snapshot_id=$1"
        )
        .bind(snapshot_id).bind(&root)
        .execute(self.store.pool()).await?;

        let ref_ = GraphSnapshotRef {
            snapshot_id,
            repository_id: self.repository_id,
            base_commit_sha: String::new(),       // populated by caller
            candidate_checkpoint_hash: None,
            execution_generation: 0,
            analysis_profile_hash: String::new(),
            extractor_version: String::new(),
            graph_schema_version: 0,
            graph_content_hash: root,
            node_count: nodes,
            edge_count: edges,
            coverage_status: "COMPLETE".into(),
        };

        info!(snapshot_id = %snapshot_id, nodes = nodes, edges = edges, root = %ref_.graph_content_hash,
              "snapshot SEALED");
        Ok(ref_)
    }

    /// Promote a candidate: the new baseline.
    pub async fn promote(&self, _snapshot_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE ofg_snapshot SET state='PROMOTED' WHERE snapshot_id=$1")
            .bind(_snapshot_id)
            .execute(self.store.pool()).await?;
        info!(snapshot_id = %_snapshot_id, "snapshot PROMOTED");
        Ok(())
    }

    /// Discard a candidate: does NOT pollute baseline.
    pub async fn discard(&self, _snapshot_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE ofg_snapshot SET state='DISCARDED' WHERE snapshot_id=$1")
            .bind(_snapshot_id)
            .execute(self.store.pool()).await?;
        info!(snapshot_id = %_snapshot_id, "snapshot DISCARDED");
        Ok(())
    }

    /// Mark INVALID after a build failure.
    pub async fn invalidate(&self, _snapshot_id: Uuid) -> Result<()> {
        sqlx::query("UPDATE ofg_snapshot SET state='INVALID' WHERE snapshot_id=$1")
            .bind(_snapshot_id)
            .execute(self.store.pool()).await?;
        info!(snapshot_id = %_snapshot_id, "snapshot INVALID");
        Ok(())
    }
}
