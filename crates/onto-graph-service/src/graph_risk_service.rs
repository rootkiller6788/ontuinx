//! gRPC GraphRiskService — 图谱风险分析（P7 GraphRiskVerifier 后端）

use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::info;

pub mod ocg_risk {
    tonic::include_proto!("ocg.graph_risk.v1");
}
use ocg_risk::{
    graph_risk_service_server::GraphRiskService as GrsTrait,
    AnalyzeCallChainRequest, AnalyzeCallChainResponse, CallChainEntry, EntityRef,
    GetDependencyClosureRequest, GetDependencyClosureResponse,
    GetAffectedTestsRequest, GetAffectedTestsResponse, AffectedTest,
    GetSharedStateAccessRequest, GetSharedStateAccessResponse, SharedStateAccess,
    GetLanguageRisksRequest, GetLanguageRisksResponse, LanguageRisk,
};

pub struct GraphRiskServiceImpl {
    pool: PgPool,
}

impl GraphRiskServiceImpl {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[tonic::async_trait]
impl GrsTrait for GraphRiskServiceImpl {
    async fn analyze_call_chain(
        &self, req: Request<AnalyzeCallChainRequest>,
    ) -> Result<Response<AnalyzeCallChainResponse>, Status> {
        let r = req.into_inner();
        let depth = r.max_depth.min(5).max(1);
        info!(snapshot_id = %r.snapshot_id, ?r.changed_entity_keys, depth, "AnalyzeCallChain");

        let mut entries = Vec::new();

        // Callers (recursive CTE upward)
        let callers = sqlx::query_as::<_, CallChainRow>(
            "WITH RECURSIVE caller_chain AS (
                SELECT e.source_entity_id, e.edge_kind, 1 as depth
                FROM ofg_edge e JOIN ofg_entity tgt ON e.target_entity_id = tgt.entity_id
                WHERE e.snapshot_id = $1::uuid AND tgt.stable_key = ANY($2)
                UNION ALL
                SELECT e.source_entity_id, e.edge_kind, cc.depth + 1
                FROM ofg_edge e JOIN caller_chain cc ON e.target_entity_id = cc.source_entity_id
                WHERE e.snapshot_id = $1::uuid AND cc.depth < $3
            )
            SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                   f.rel_path, ev.start_line::int4, cc.edge_kind, cc.depth::int4, 'caller' as direction
            FROM caller_chain cc
            JOIN ofg_entity ent ON cc.source_entity_id = ent.entity_id
            JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
            LEFT JOIN ofg_file f ON ev.file_id = f.file_id
            LIMIT $4"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys)
        .bind(depth).bind(r.max_results)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("call chain: {}", e)))?;
        entries.extend(into_entries(callers));

        // Callees (recursive CTE downward)
        let callees = sqlx::query_as::<_, CallChainRow>(
            "WITH RECURSIVE callee_chain AS (
                SELECT e.target_entity_id as source_entity_id, e.edge_kind, 1 as depth
                FROM ofg_edge e JOIN ofg_entity src ON e.source_entity_id = src.entity_id
                WHERE e.snapshot_id = $1::uuid AND src.stable_key = ANY($2)
                UNION ALL
                SELECT e.target_entity_id, e.edge_kind, cc.depth + 1
                FROM ofg_edge e JOIN callee_chain cc ON e.source_entity_id = cc.source_entity_id
                WHERE e.snapshot_id = $1::uuid AND cc.depth < $3
            )
            SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                   f.rel_path, ev.start_line::int4, cc.edge_kind, cc.depth::int4, 'callee' as direction
            FROM callee_chain cc
            JOIN ofg_entity ent ON cc.source_entity_id = ent.entity_id
            JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
            LEFT JOIN ofg_file f ON ev.file_id = f.file_id
            LIMIT $4"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys)
        .bind(depth).bind(r.max_results)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("callee chain: {}", e)))?;
        entries.extend(into_entries(callees));

        Ok(Response::new(AnalyzeCallChainResponse {
            total_affected: entries.len() as i32, entries,
        }))
    }

    async fn get_dependency_closure(
        &self, req: Request<GetDependencyClosureRequest>,
    ) -> Result<Response<GetDependencyClosureResponse>, Status> {
        let r = req.into_inner();
        let deps = sqlx::query_as::<_, EntityRefRow>(
            "SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line::int4
             FROM ofg_edge e
             JOIN ofg_entity src ON e.source_entity_id = src.entity_id
             JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE e.snapshot_id = $1::uuid
               AND src.stable_key = ANY($2)
               AND e.edge_kind IN ('IMPORTS', 'DEPENDS_ON', 'INCLUDES', 'USES_TYPE')
             LIMIT $3"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys).bind(r.max_results)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("dep closure: {}", e)))?;

        let direct: Vec<EntityRef> = deps.into_iter().map(|r| EntityRef {
            entity_key: r.stable_key, qualified_name: r.qualified_name,
            entity_kind: r.entity_kind, file_path: r.rel_path.unwrap_or_default(),
            start_line: r.start_line,
        }).collect();

        Ok(Response::new(GetDependencyClosureResponse {
            closure_size: direct.len() as i32, direct_dependencies: direct,
            dependents: vec![],
        }))
    }

    async fn get_affected_tests(
        &self, req: Request<GetAffectedTestsRequest>,
    ) -> Result<Response<GetAffectedTestsResponse>, Status> {
        let r = req.into_inner();
        let tests = sqlx::query_as::<_, TestRow>(
            "SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line::int4,
                    CASE WHEN e.edge_kind = 'CALLS' THEN 'direct_caller'
                         ELSE 'indirect_dependency' END as relationship,
                    CASE WHEN e.edge_kind = 'CALLS' THEN 1 ELSE 2 END as distance
             FROM ofg_edge e
             JOIN ofg_entity src ON e.source_entity_id = src.entity_id
             JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE e.snapshot_id = $1::uuid
               AND src.stable_key = ANY($2)
               AND ent.entity_kind IN ('Test', 'TestCase', 'TestSuite', 'TestFunction')
             LIMIT 50"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("affected tests: {}", e)))?;

        Ok(Response::new(GetAffectedTestsResponse {
            tests: tests.into_iter().map(|t| AffectedTest {
                test_entity: Some(EntityRef {
                    entity_key: t.stable_key, qualified_name: t.qualified_name,
                    entity_kind: t.entity_kind, file_path: t.rel_path.unwrap_or_default(),
                    start_line: t.start_line,
                }),
                relationship: t.relationship, distance: t.distance,
            }).collect(),
        }))
    }

    async fn get_shared_state_access(
        &self, req: Request<GetSharedStateAccessRequest>,
    ) -> Result<Response<GetSharedStateAccessResponse>, Status> {
        let r = req.into_inner();
        let accesses = sqlx::query_as::<_, SharedStateRow>(
            "SELECT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line::int4,
                    e.edge_kind as access_kind,
                    src_ent.stable_key as accessed_by_key
             FROM ofg_edge e
             JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
             JOIN ofg_entity src_ent ON e.source_entity_id = src_ent.entity_id
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE e.snapshot_id = $1::uuid
               AND src_ent.stable_key = ANY($2)
               AND e.edge_kind IN ('READS', 'WRITES')
               AND ent.entity_kind IN ('Variable', 'GlobalVariable', 'StaticVariable', 'Singleton')
             LIMIT 50"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("shared state: {}", e)))?;

        Ok(Response::new(GetSharedStateAccessResponse {
            accesses: accesses.into_iter().map(|a| SharedStateAccess {
                variable: Some(EntityRef {
                    entity_key: a.stable_key, qualified_name: a.qualified_name,
                    entity_kind: a.entity_kind, file_path: a.rel_path.unwrap_or_default(),
                    start_line: a.start_line,
                }),
                access_kind: a.access_kind,
                accessed_by: Some(EntityRef {
                    entity_key: a.accessed_by_key, ..Default::default()
                }),
            }).collect(),
        }))
    }

    async fn get_language_risks(
        &self, req: Request<GetLanguageRisksRequest>,
    ) -> Result<Response<GetLanguageRisksResponse>, Status> {
        let r = req.into_inner();
        let risks = sqlx::query_as::<_, LanguageRiskRow>(
            "SELECT ent.stable_key, ev.qualified_name, ent.entity_kind,
                    f.rel_path, ev.start_line::int4,
                    ev.properties->>'unsafe' as unsafe_flag,
                    ev.properties->>'reflection' as reflection_flag,
                    ev.properties->>'dynamic_import' as dynamic_import_flag,
                    ev.properties->>'fn_ptr_table' as fn_ptr_table_flag
             FROM ofg_entity ent
             JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
                AND ev.snapshot_id = $1::uuid AND NOT ev.tombstone
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE ent.stable_key = ANY($2) AND ent.language = $3
               AND (ev.properties->>'unsafe' = 'true'
                 OR ev.properties->>'reflection' = 'true'
                 OR ev.properties->>'dynamic_import' = 'true'
                 OR ev.properties->>'fn_ptr_table' = 'true')
             LIMIT 50"
        )
        .bind(&r.snapshot_id).bind(&r.changed_entity_keys).bind(&r.language)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("language risks: {}", e)))?;

        let mut results = Vec::new();
        for row in risks {
            if row.unsafe_flag.as_deref() == Some("true") {
                results.push(make_risk(&row, "unsafe_block", "critical"));
            }
            if row.reflection_flag.as_deref() == Some("true") {
                results.push(make_risk(&row, "reflection", "high"));
            }
            if row.dynamic_import_flag.as_deref() == Some("true") {
                results.push(make_risk(&row, "dynamic_import", "medium"));
            }
            if row.fn_ptr_table_flag.as_deref() == Some("true") {
                results.push(make_risk(&row, "fn_ptr_table", "high"));
            }
        }
        Ok(Response::new(GetLanguageRisksResponse { risks: results }))
    }
}

// ── Internal query rows ──

#[derive(sqlx::FromRow)]
struct CallChainRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    edge_kind: String, depth: i32, direction: String,
}

fn into_entries(rows: Vec<CallChainRow>) -> Vec<CallChainEntry> {
    rows.into_iter().map(|r| CallChainEntry {
        entity: Some(EntityRef {
            entity_key: r.stable_key, qualified_name: r.qualified_name,
            entity_kind: r.entity_kind, file_path: r.rel_path.unwrap_or_default(),
            start_line: r.start_line,
        }),
        edge_kind: r.edge_kind, direction: r.direction, depth: r.depth,
    }).collect()
}

#[derive(sqlx::FromRow)]
struct EntityRefRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
}

#[derive(sqlx::FromRow)]
struct TestRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    relationship: String, distance: i32,
}

#[derive(sqlx::FromRow)]
struct SharedStateRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    access_kind: String, accessed_by_key: String,
}

#[derive(sqlx::FromRow)]
struct LanguageRiskRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    rel_path: Option<String>, start_line: i32,
    unsafe_flag: Option<String>, reflection_flag: Option<String>,
    dynamic_import_flag: Option<String>, fn_ptr_table_flag: Option<String>,
}

fn make_risk(row: &LanguageRiskRow, risk_type: &str, severity: &str) -> LanguageRisk {
    LanguageRisk {
        risk_type: risk_type.into(), severity: severity.into(),
        detail: format!("{} {} in {}", row.entity_kind, row.qualified_name,
                        row.rel_path.as_deref().unwrap_or("?")),
        entity: Some(EntityRef {
            entity_key: row.stable_key.clone(),
            qualified_name: row.qualified_name.clone(),
            entity_kind: row.entity_kind.clone(),
            file_path: row.rel_path.clone().unwrap_or_default(),
            start_line: row.start_line,
        }),
    }
}
