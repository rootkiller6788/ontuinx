//! gRPC client abstraction for onto-graph-service.
//!
//! Follows the `CommandRunner` pattern from onto-code-pack/src/build_verifier.rs:
//! trait + tonic production impl + mock test impl.

use async_trait::async_trait;

// ── Error type ──

#[derive(Debug, thiserror::Error)]
pub enum GraphClientError {
    #[error("gRPC connection failed: {0}")]
    ConnectionFailed(String),
    #[error("gRPC call failed: {0}")]
    GrpcError(String),
    #[error("snapshot not found: {0}")]
    NotFound(String),
}

// ── Response types (opaque structs — no proto dependency) ──

/// Response from SnapshotQueryService.CheckIntegrity
#[derive(Debug, Clone, Default)]
pub struct IntegrityResult {
    pub snapshot_exists: bool,
    pub is_sealed: bool,
    pub checkpoint_matches: bool,
    pub generation_matches: bool,
    pub all_files_processed: bool,
    pub coverage_complete: bool,
    pub snapshot_id: String,
    pub graph_content_hash: String,
    pub node_count: i64,
    pub edge_count: i64,
    pub coverage_status: String,
    pub unprocessed_files: Vec<String>,
    pub missing_passes: Vec<String>,
    pub error_detail: String,
}

/// Response from GraphRiskService.AnalyzeCallChain
#[derive(Debug, Clone, Default)]
pub struct CallChainResult {
    pub entries: Vec<CallChainEntry>,
    pub total_affected: i32,
}

#[derive(Debug, Clone, Default)]
pub struct CallChainEntry {
    pub entity_key: String,
    pub qualified_name: String,
    pub entity_kind: String,
    pub file_path: String,
    pub start_line: i32,
    pub edge_kind: String,
    pub direction: String,
    pub depth: i32,
}

/// Response from GraphRiskService.GetLanguageRisks
#[derive(Debug, Clone, Default)]
pub struct LanguageRiskResult {
    pub risks: Vec<LanguageRiskItem>,
}

#[derive(Debug, Clone, Default)]
pub struct LanguageRiskItem {
    pub risk_type: String,
    pub detail: String,
    pub severity: String,
    pub entity_key: String,
    pub qualified_name: String,
    pub file_path: String,
}

/// Response from GraphRiskService.GetSharedStateAccess
#[derive(Debug, Clone, Default)]
pub struct SharedStateResult {
    pub accesses: Vec<SharedStateAccessItem>,
}

#[derive(Debug, Clone, Default)]
pub struct SharedStateAccessItem {
    pub variable_key: String,
    pub variable_name: String,
    pub access_kind: String,
    pub accessed_by_key: String,
}

/// Response from GraphRiskService.GetDependencyClosure
#[derive(Debug, Clone, Default)]
pub struct DependencyResult {
    pub closure_size: i32,
    pub entries: Vec<DependencyEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct DependencyEntry {
    pub entity_key: String,
    pub qualified_name: String,
    pub entity_kind: String,
    pub file_path: String,
}

/// Response from GraphRiskService.GetAffectedTests
#[derive(Debug, Clone, Default)]
pub struct AffectedTestsResult {
    pub tests: Vec<AffectedTestItem>,
}

#[derive(Debug, Clone, Default)]
pub struct AffectedTestItem {
    pub entity_key: String,
    pub qualified_name: String,
    pub entity_kind: String,
    pub file_path: String,
    pub relationship: String,
    pub distance: i32,
}

// ── Trait definitions ──

#[async_trait]
pub trait SnapshotQueryClient: Send + Sync {
    async fn check_integrity(
        &self, repository_name: &str, base_commit_sha: &str,
        candidate_checkpoint_hash: &str, execution_generation: u64,
        changed_files: &[String], required_passes: &[String],
    ) -> Result<IntegrityResult, GraphClientError>;
}

#[async_trait]
pub trait GraphRiskClient: Send + Sync {
    async fn analyze_call_chain(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
        max_depth: i32, max_results: i32,
    ) -> Result<CallChainResult, GraphClientError>;

    async fn get_dependency_closure(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
        transitive: bool, max_results: i32,
    ) -> Result<DependencyResult, GraphClientError>;

    async fn get_affected_tests(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
    ) -> Result<AffectedTestsResult, GraphClientError>;

    async fn get_shared_state_access(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
    ) -> Result<SharedStateResult, GraphClientError>;

    async fn get_language_risks(
        &self, snapshot_id: &str, changed_entity_keys: &[String],
        language: &str,
    ) -> Result<LanguageRiskResult, GraphClientError>;
}

// ── Mock implementations for testing ──

#[derive(Default)]
pub struct MockSnapshotClient {
    pub response: IntegrityResult,
    pub error: Option<GraphClientError>,
}

#[async_trait]
impl SnapshotQueryClient for MockSnapshotClient {
    async fn check_integrity(
        &self, _: &str, _: &str, _: &str, _: u64, _: &[String], _: &[String],
    ) -> Result<IntegrityResult, GraphClientError> {
        if let Some(ref e) = self.error {
            return Err(match e {
                GraphClientError::ConnectionFailed(s) => GraphClientError::ConnectionFailed(s.clone()),
                GraphClientError::GrpcError(s) => GraphClientError::GrpcError(s.clone()),
                GraphClientError::NotFound(s) => GraphClientError::NotFound(s.clone()),
            });
        }
        Ok(self.response.clone())
    }
}

#[derive(Default)]
pub struct MockRiskClient {
    pub call_chain: CallChainResult,
    pub dependency: DependencyResult,
    pub affected_tests: AffectedTestsResult,
    pub shared_state: SharedStateResult,
    pub language_risks: LanguageRiskResult,
    pub error: Option<GraphClientError>,
}

#[async_trait]
impl GraphRiskClient for MockRiskClient {
    async fn analyze_call_chain(&self, _: &str, _: &[String], _: i32, _: i32) -> Result<CallChainResult, GraphClientError> {
        if let Some(ref e) = self.error { return Err(e.clone_err()); }
        Ok(self.call_chain.clone())
    }
    async fn get_dependency_closure(&self, _: &str, _: &[String], _: bool, _: i32) -> Result<DependencyResult, GraphClientError> {
        if let Some(ref e) = self.error { return Err(e.clone_err()); }
        Ok(self.dependency.clone())
    }
    async fn get_affected_tests(&self, _: &str, _: &[String]) -> Result<AffectedTestsResult, GraphClientError> {
        if let Some(ref e) = self.error { return Err(e.clone_err()); }
        Ok(self.affected_tests.clone())
    }
    async fn get_shared_state_access(&self, _: &str, _: &[String]) -> Result<SharedStateResult, GraphClientError> {
        if let Some(ref e) = self.error { return Err(e.clone_err()); }
        Ok(self.shared_state.clone())
    }
    async fn get_language_risks(&self, _: &str, _: &[String], _: &str) -> Result<LanguageRiskResult, GraphClientError> {
        if let Some(ref e) = self.error { return Err(e.clone_err()); }
        Ok(self.language_risks.clone())
    }
}

impl GraphClientError {
    fn clone_err(&self) -> Self {
        match self {
            Self::ConnectionFailed(s) => Self::ConnectionFailed(s.clone()),
            Self::GrpcError(s) => Self::GrpcError(s.clone()),
            Self::NotFound(s) => Self::NotFound(s.clone()),
        }
    }
}
