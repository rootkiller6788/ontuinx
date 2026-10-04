//! gRPC QueryService — P9 Agent 只读图谱查询

use sqlx::PgPool;
use tonic::{Request, Response, Status};
use tracing::info;

pub mod ocg_query {
    tonic::include_proto!("ocg.query.v1");
}
use ocg_query::{
    query_service_server::QueryService as QsTrait,
    SearchSymbolsRequest, SearchSymbolsResponse, SymbolRef,
    GetSymbolContextRequest, GetSymbolContextResponse,
    GetCallersRequest, GetCallersResponse,
    GetCalleesRequest, GetCalleesResponse,
    GetReferencesRequest, GetReferencesResponse,
    GetPreviousChangesRequest, GetPreviousChangesResponse, ChangeRecord,
};

pub struct QueryServiceImpl {
    pool: PgPool,
}

impl QueryServiceImpl {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[tonic::async_trait]
impl QsTrait for QueryServiceImpl {
    async fn search_symbols(
        &self, req: Request<SearchSymbolsRequest>,
    ) -> Result<Response<SearchSymbolsResponse>, Status> {
        let r = req.into_inner();
        let limit = r.max_results.min(50).max(1);
        info!(query = %r.query, kind = ?r.entity_kind, lang = ?r.language, "SearchSymbols");

        let rows = sqlx::query_as::<_, SymbolRow>(
            "SELECT e.stable_key, ev.qualified_name, e.entity_kind, e.language,
                    COALESCE(f.rel_path, ''), COALESCE(ev.start_line, 0)
             FROM ofg_entity e
             JOIN ofg_entity_version ev ON e.entity_id = ev.entity_id
             JOIN ofg_snapshot s ON ev.snapshot_id = s.snapshot_id
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE s.state = 'PROMOTED'
               AND ($1::text IS NULL OR e.entity_kind = $1)
               AND ($2::text IS NULL OR e.language = $2)
               AND ev.qualified_name ILIKE '%' || $3 || '%'
               AND NOT ev.tombstone
             ORDER BY ev.qualified_name LIMIT $4"
        )
        .bind(&r.entity_kind).bind(&r.language)
        .bind(&r.query).bind(limit)
        .fetch_all(&self.pool).await
        .map_err(|e| Status::internal(format!("search: {e}")))?;

        Ok(Response::new(SearchSymbolsResponse {
            symbols: rows.into_iter().map(|r| SymbolRef {
                entity_key: r.stable_key, qualified_name: r.qualified_name,
                entity_kind: r.entity_kind, language: r.language.unwrap_or_default(),
                file_path: r.file_path, start_line: r.start_line,
            }).collect(),
        }))
    }

    async fn get_symbol_context(
        &self, req: Request<GetSymbolContextRequest>,
    ) -> Result<Response<GetSymbolContextResponse>, Status> {
        let r = req.into_inner();
        info!(entity_key = %r.entity_key, "GetSymbolContext");

        let sym = sqlx::query_as::<_, SymbolRow>(
            "SELECT e.stable_key, ev.qualified_name, e.entity_kind, e.language,
                    COALESCE(f.rel_path, ''), COALESCE(ev.start_line, 0)
             FROM ofg_entity e
             JOIN ofg_entity_version ev ON e.entity_id = ev.entity_id
             JOIN ofg_snapshot s ON ev.snapshot_id = s.snapshot_id
             LEFT JOIN ofg_file f ON ev.file_id = f.file_id
             WHERE s.state = 'PROMOTED' AND e.stable_key = $1 AND NOT ev.tombstone
             LIMIT 1"
        ).bind(&r.entity_key).fetch_optional(&self.pool).await
            .map_err(|e| Status::internal(format!("context: {e}")))?;

        let sym = match sym {
            Some(s) => s,
            None => return Ok(Response::new(GetSymbolContextResponse::default())),
        };

        let symbol = Some(SymbolRef {
            entity_key: sym.stable_key, qualified_name: sym.qualified_name,
            entity_kind: sym.entity_kind, language: sym.language.unwrap_or_default(),
            file_path: sym.file_path, start_line: sym.start_line,
        });

        // Callers (depth 1)
        let callers = get_related(&self.pool, &r.entity_key, "caller", 1, 10).await?;
        let callees = get_related(&self.pool, &r.entity_key, "callee", 1, 10).await?;
        let refs = get_references_raw(&self.pool, &r.entity_key, None, 10).await?;

        Ok(Response::new(GetSymbolContextResponse {
            symbol, callers, callees, references: refs, resolution_level: 2,
        }))
    }

    async fn get_callers(
        &self, req: Request<GetCallersRequest>,
    ) -> Result<Response<GetCallersResponse>, Status> {
        let r = req.into_inner();
        let depth = r.max_depth.min(3).max(1);
        info!(entity_key = %r.entity_key, depth, "GetCallers");
        let symbols = get_related(&self.pool, &r.entity_key, "caller", depth, r.max_results).await?;
        Ok(Response::new(GetCallersResponse { symbols }))
    }

    async fn get_callees(
        &self, req: Request<GetCalleesRequest>,
    ) -> Result<Response<GetCalleesResponse>, Status> {
        let r = req.into_inner();
        let depth = r.max_depth.min(3).max(1);
        info!(entity_key = %r.entity_key, depth, "GetCallees");
        let symbols = get_related(&self.pool, &r.entity_key, "callee", depth, r.max_results).await?;
        Ok(Response::new(GetCalleesResponse { symbols }))
    }

    async fn get_references(
        &self, req: Request<GetReferencesRequest>,
    ) -> Result<Response<GetReferencesResponse>, Status> {
        let r = req.into_inner();
        info!(entity_key = %r.entity_key, kind = ?r.reference_kind, "GetReferences");
        let symbols = get_references_raw(&self.pool, &r.entity_key, Some(&r.reference_kind), r.max_results).await?;
        Ok(Response::new(GetReferencesResponse { symbols }))
    }

    async fn get_previous_changes(
        &self, req: Request<GetPreviousChangesRequest>,
    ) -> Result<Response<GetPreviousChangesResponse>, Status> {
        let r = req.into_inner();
        let limit = r.max_results.min(10).max(1);
        info!(entity_key = %r.entity_key, "GetPreviousChanges");

        let rows = sqlx::query_as::<_, ChangeRow>(
            "SELECT cd.attempt_id, cd.candidate_checkpoint_hash, s.created_at::text
             FROM ofg_candidate_delta cd
             JOIN ofg_snapshot s ON cd.snapshot_id = s.snapshot_id
             WHERE s.state IN ('PROMOTED', 'SEALED')
               AND ($1 = ANY(cd.added_files) OR $1 = ANY(cd.changed_files) OR $1 = ANY(cd.deleted_files))
             ORDER BY s.created_at DESC LIMIT $2"
        ).bind(&r.entity_key).bind(limit)
            .fetch_all(&self.pool).await
            .map_err(|e| Status::internal(format!("changes: {e}")))?;

        Ok(Response::new(GetPreviousChangesResponse {
            changes: rows.into_iter().map(|r| ChangeRecord {
                attempt_id: r.attempt_id,
                change_type: "MODIFIED".into(),
                checkpoint_hash: r.checkpoint_hash.unwrap_or_default(),
                timestamp: r.ts.unwrap_or_default(),
            }).collect(),
        }))
    }
}

// ── Helpers ──

#[derive(sqlx::FromRow)]
struct SymbolRow {
    stable_key: String, qualified_name: String, entity_kind: String,
    language: Option<String>, file_path: String, start_line: i32,
}

#[derive(sqlx::FromRow)]
struct ChangeRow {
    attempt_id: String,
    checkpoint_hash: Option<String>,
    ts: Option<String>,
}

/// Get callers (direction="caller") or callees (direction="callee") via recursive CTE.
/// Uses fully static SQL — no dynamic column names.
async fn get_related(pool: &PgPool, entity_key: &str, direction: &str, depth: i32, max: i32) -> Result<Vec<SymbolRef>, Status> {
    // Two separate static queries — avoids format!() SQL injection risk.
    if direction == "caller" {
        get_callers_sql(pool, entity_key, depth, max).await
    } else {
        get_callees_sql(pool, entity_key, depth, max).await
    }
}

async fn get_callers_sql(pool: &PgPool, entity_key: &str, depth: i32, max: i32) -> Result<Vec<SymbolRef>, Status> {
    let rows = sqlx::query_as::<_, SymbolRow>(
        "WITH RECURSIVE chain AS (
            SELECT e.source_entity_id as entity_id, e.edge_kind, 1 as depth
            FROM ofg_edge e
            JOIN ofg_snapshot s ON e.snapshot_id = s.snapshot_id
            JOIN ofg_entity ent ON e.target_entity_id = ent.entity_id
            WHERE s.state = 'PROMOTED' AND ent.stable_key = $1
            UNION ALL
            SELECT e.source_entity_id, e.edge_kind, c.depth + 1
            FROM ofg_edge e JOIN chain c ON e.target_entity_id = c.entity_id
            JOIN ofg_snapshot s ON e.snapshot_id = s.snapshot_id
            WHERE s.state = 'PROMOTED' AND c.depth < $2
        )
        SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
               COALESCE(ent.language, ''), COALESCE(f.rel_path, ''), COALESCE(ev.start_line, 0)
        FROM chain c
        JOIN ofg_entity ent ON c.entity_id = ent.entity_id
        JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
        JOIN ofg_snapshot s ON ev.snapshot_id = s.snapshot_id AND s.state = 'PROMOTED'
        LEFT JOIN ofg_file f ON ev.file_id = f.file_id
        WHERE NOT ev.tombstone LIMIT $3"
    )
    .bind(entity_key).bind(depth).bind(max)
    .fetch_all(pool).await
    .map_err(|e| Status::internal(format!("callers: {e}")))?;
    Ok(rows_to_refs(rows))
}

async fn get_callees_sql(pool: &PgPool, entity_key: &str, depth: i32, max: i32) -> Result<Vec<SymbolRef>, Status> {
    let rows = sqlx::query_as::<_, SymbolRow>(
        "WITH RECURSIVE chain AS (
            SELECT e.target_entity_id as entity_id, e.edge_kind, 1 as depth
            FROM ofg_edge e
            JOIN ofg_snapshot s ON e.snapshot_id = s.snapshot_id
            JOIN ofg_entity ent ON e.source_entity_id = ent.entity_id
            WHERE s.state = 'PROMOTED' AND ent.stable_key = $1
            UNION ALL
            SELECT e.target_entity_id, e.edge_kind, c.depth + 1
            FROM ofg_edge e JOIN chain c ON e.source_entity_id = c.entity_id
            JOIN ofg_snapshot s ON e.snapshot_id = s.snapshot_id
            WHERE s.state = 'PROMOTED' AND c.depth < $2
        )
        SELECT DISTINCT ent.stable_key, ev.qualified_name, ent.entity_kind,
               COALESCE(ent.language, ''), COALESCE(f.rel_path, ''), COALESCE(ev.start_line, 0)
        FROM chain c
        JOIN ofg_entity ent ON c.entity_id = ent.entity_id
        JOIN ofg_entity_version ev ON ent.entity_id = ev.entity_id
        JOIN ofg_snapshot s ON ev.snapshot_id = s.snapshot_id AND s.state = 'PROMOTED'
        LEFT JOIN ofg_file f ON ev.file_id = f.file_id
        WHERE NOT ev.tombstone LIMIT $3"
    )
    .bind(entity_key).bind(depth).bind(max)
    .fetch_all(pool).await
    .map_err(|e| Status::internal(format!("callees: {e}")))?;
    Ok(rows_to_refs(rows))
}

fn rows_to_refs(rows: Vec<SymbolRow>) -> Vec<SymbolRef> {
    rows.into_iter().map(|r| SymbolRef {
        entity_key: r.stable_key, qualified_name: r.qualified_name,
        entity_kind: r.entity_kind, language: r.language.unwrap_or_default(),
        file_path: r.file_path, start_line: r.start_line,
    }).collect()
}

async fn get_references_raw(pool: &PgPool, entity_key: &str, kind: Option<&str>, max: i32) -> Result<Vec<SymbolRef>, Status> {
    let rows = sqlx::query_as::<_, SymbolRow>(
        "SELECT DISTINCT e.stable_key, ev.qualified_name, e.entity_kind,
                COALESCE(e.language, ''), COALESCE(f.rel_path, ''), COALESCE(ev.start_line, 0)
         FROM ofg_edge ed
         JOIN ofg_snapshot s ON ed.snapshot_id = s.snapshot_id
         JOIN ofg_entity src ON ed.source_entity_id = src.entity_id
         JOIN ofg_entity e ON ed.target_entity_id = e.entity_id
         JOIN ofg_entity_version ev ON e.entity_id = ev.entity_id AND ev.snapshot_id = s.snapshot_id
         LEFT JOIN ofg_file f ON ev.file_id = f.file_id
         WHERE s.state = 'PROMOTED' AND src.stable_key = $1
           AND ($2::text IS NULL OR ed.edge_kind = $2)
           AND NOT ev.tombstone
         LIMIT $3"
    ).bind(entity_key).bind(kind).bind(max)
        .fetch_all(pool).await
        .map_err(|e| Status::internal(format!("refs: {e}")))?;
    Ok(rows.into_iter().map(|r| SymbolRef {
        entity_key: r.stable_key, qualified_name: r.qualified_name,
        entity_kind: r.entity_kind, language: r.language.unwrap_or_default(),
        file_path: r.file_path, start_line: r.start_line,
    }).collect())
}
