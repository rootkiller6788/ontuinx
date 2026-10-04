//! RepositoryGraphReadCapability — P9
//!
//! Lane B (read-only) capability exposed to Agents via CapabilityGateway.
//! Agent 可查询图谱，但图谱不可用不阻断 Agent 执行。
//! P9 修复：真实 tonic client 连接 ofg-service QueryService。

use onto_graph_service::ocg_query::query_service_client::QueryServiceClient;
use onto_graph_service::ocg_query::{
    SearchSymbolsRequest, GetSymbolContextRequest, GetCallersRequest, GetCalleesRequest,
    GetReferencesRequest, GetPreviousChangesRequest,
};
use std::time::Duration;
use tonic::transport::Channel;

const DEFAULT_TIMEOUT_MS: u64 = 3000;
const MAX_RESULTS_PER_QUERY: u32 = 20;

/// Capability descriptor.
#[derive(Debug, Clone)]
pub struct GraphReadDescriptor {
    pub capability_id: String,
    pub capability_name: String,
    pub description: String,
    pub lane: String,
    pub is_readonly: bool,
}

/// Query result returned to Agent.
#[derive(Debug, Clone)]
pub struct GraphQueryResult {
    pub success: bool,
    pub data: serde_json::Value,
    pub error: Option<String>,
}

/// The RepositoryGraphReadCapability — wraps a tonic gRPC client.
pub struct RepositoryGraphReadCapability {
    service_addr: String,
    timeout: Duration,
    max_results: u32,
}

impl RepositoryGraphReadCapability {
    pub fn new(service_addr: String) -> Self {
        Self {
            service_addr,
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
            max_results: MAX_RESULTS_PER_QUERY,
        }
    }

    pub fn descriptor() -> GraphReadDescriptor {
        GraphReadDescriptor {
            capability_id: "repository_graph_read".into(),
            capability_name: "RepositoryGraphRead".into(),
            description: "Read-only queries against OntoCodeGraph.".into(),
            lane: "B".into(),
            is_readonly: true,
        }
    }

    /// Connect lazily to the ofg-service gRPC endpoint.
    async fn connect(&self) -> Result<QueryServiceClient<Channel>, String> {
        let endpoint = format!("http://{}", self.service_addr);
        QueryServiceClient::connect(endpoint).await
            .map_err(|e| format!("gRPC connect to {}: {}", self.service_addr, e))
    }

    pub async fn search_symbols(&self, repo: &str, query: &str) -> GraphQueryResult {
        let max_r = self.max_results as i32;
        self.with_client(|mut c| async move {
            let resp = c.search_symbols(SearchSymbolsRequest {
                repository_name: repo.into(), query: query.into(),
                entity_kind: String::new(), language: String::new(),
                max_results: max_r,
            }).await.map_err(|e| format!("{e}"))?;
            let r = resp.into_inner();
            let symbols: Vec<serde_json::Value> = r.symbols.iter().map(|s| serde_json::json!({
                "entity_key": s.entity_key, "qualified_name": s.qualified_name,
                "entity_kind": s.entity_kind, "language": s.language,
                "file_path": s.file_path, "start_line": s.start_line,
            })).collect();
            Ok(serde_json::json!({"symbols": symbols}))
        }).await
    }

    pub async fn get_symbol_context(&self, repo: &str, entity_key: &str) -> GraphQueryResult {
        self.with_client(|mut c| async move {
            let resp = c.get_symbol_context(GetSymbolContextRequest {
                repository_name: repo.into(), entity_key: entity_key.into(),
            }).await.map_err(|e| format!("{e}"))?;
            let r = resp.into_inner();
            Ok(serde_json::json!({
                "callers": r.callers.len(), "callees": r.callees.len(),
                "references": r.references.len(),
            }))
        }).await
    }

    pub async fn get_callers(&self, repo: &str, entity_key: &str) -> GraphQueryResult {
        self.with_client(|mut c| async move {
            let resp = c.get_callers(GetCallersRequest {
                repository_name: repo.into(), entity_key: entity_key.into(),
                max_depth: 1, max_results: 10,
            }).await.map_err(|e| format!("{e}"))?;
            let r = resp.into_inner();
            let symbols: Vec<serde_json::Value> = r.symbols.iter().map(|s| serde_json::json!({
                "entity_key": s.entity_key, "qualified_name": s.qualified_name,
            })).collect();
            Ok(serde_json::json!({"symbols": symbols}))
        }).await
    }

    pub async fn get_callees(&self, repo: &str, entity_key: &str) -> GraphQueryResult {
        self.with_client(|mut c| async move {
            let resp = c.get_callees(GetCalleesRequest {
                repository_name: repo.into(), entity_key: entity_key.into(),
                max_depth: 1, max_results: 10,
            }).await.map_err(|e| format!("{e}"))?;
            let r = resp.into_inner();
            let symbols: Vec<serde_json::Value> = r.symbols.iter().map(|s| serde_json::json!({
                "entity_key": s.entity_key, "qualified_name": s.qualified_name,
            })).collect();
            Ok(serde_json::json!({"symbols": symbols}))
        }).await
    }

    pub async fn get_previous_changes(&self, repo: &str, entity_key: &str) -> GraphQueryResult {
        self.with_client(|mut c| async move {
            let resp = c.get_previous_changes(GetPreviousChangesRequest {
                repository_name: repo.into(), entity_key: entity_key.into(),
                max_results: 5,
            }).await.map_err(|e| format!("{e}"))?;
            let r = resp.into_inner();
            let changes: Vec<serde_json::Value> = r.changes.iter().map(|ch| serde_json::json!({
                "attempt_id": ch.attempt_id, "change_type": ch.change_type,
                "checkpoint_hash": ch.checkpoint_hash,
            })).collect();
            Ok(serde_json::json!({"changes": changes}))
        }).await
    }

    /// Execute a gRPC call with timeout and graceful degradation.
    async fn with_client<F, Fut>(&self, f: F) -> GraphQueryResult
    where
        F: FnOnce(QueryServiceClient<Channel>) -> Fut,
        Fut: std::future::Future<Output = Result<serde_json::Value, String>>,
    {
        let client = match self.connect().await {
            Ok(c) => c,
            Err(e) => return GraphQueryResult {
                success: false, data: serde_json::json!({}),
                error: Some(format!("connect: {e}")),
            },
        };

        match tokio::time::timeout(self.timeout, f(client)).await {
            Ok(Ok(data)) => GraphQueryResult { success: true, data, error: None },
            Ok(Err(e)) => GraphQueryResult {
                success: false, data: serde_json::json!({}),
                error: Some(format!("query: {e}")),
            },
            Err(_elapsed) => GraphQueryResult {
                success: false, data: serde_json::json!({}),
                error: Some(format!("timeout after {:?}", self.timeout)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_descriptor_is_readonly() {
        let d = RepositoryGraphReadCapability::descriptor();
        assert!(d.is_readonly);
        assert_eq!(d.lane, "B");
    }

    #[tokio::test]
    async fn test_graph_unavailable_graceful() {
        // Use a random port — service won't be there, but it should fail gracefully
        let cap = RepositoryGraphReadCapability::new("127.0.0.1:19999".into());
        let result = cap.search_symbols("test-repo", "main").await;
        assert!(!result.success);
        assert!(result.error.is_some());
        // Critical P9 invariant: failure is graceful, never panics
    }
}
