//! onto-graph-mcp — P10 MCP Server for OntoCodeGraph
//!
//! Registered in ironclaw/registry/mcp-servers/onto-code-graph.json.
//! Supports stdio (Claude Desktop) and HTTP (IronClaw MCP runtime lane).
//!
//! Usage:
//!   onto-graph-mcp --repo my-project [--addr localhost:50051] [--http-port 50052]

use onto_graph_service::ocg_query::query_service_client::QueryServiceClient;
use onto_graph_service::ocg_query::{
    SearchSymbolsRequest, GetSymbolContextRequest, GetCallersRequest, GetCalleesRequest,
    GetReferencesRequest, GetPreviousChangesRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tonic::transport::Channel;

// ═══════════════════════════════════════════════════
// JSON-RPC 2.0 types
// ═══════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct Response {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError { code: i32, message: String }

// ═══════════════════════════════════════════════════
// MCP Tool definitions
// ═══════════════════════════════════════════════════

const TOOLS_JSON: &str = r#"[
  {
    "name": "search_symbols",
    "description": "Search code symbols by name across the repository graph.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "query": {"type": "string", "description": "Partial symbol name to search for"},
        "kind": {"type": "string", "description": "Optional filter: Function, Class, Struct, Variable"},
        "language": {"type": "string", "description": "Optional filter: rust, python, go, c, cpp"}
      },
      "required": ["query"]
    }
  },
  {
    "name": "get_symbol_context",
    "description": "Get full context for a symbol: definition location, callers, callees, and references.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "entity_key": {"type": "string", "description": "Stable entity key (e.g. 'src/main.rs::main')"}
      },
      "required": ["entity_key"]
    }
  },
  {
    "name": "get_callers",
    "description": "Find all functions that call the given symbol.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "entity_key": {"type": "string"},
        "max_depth": {"type": "integer", "default": 1}
      },
      "required": ["entity_key"]
    }
  },
  {
    "name": "get_callees",
    "description": "Find all functions called by the given symbol.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "entity_key": {"type": "string"},
        "max_depth": {"type": "integer", "default": 1}
      },
      "required": ["entity_key"]
    }
  },
  {
    "name": "get_references",
    "description": "Find all references to a symbol (imports, type uses, reads, writes).",
    "inputSchema": {
      "type": "object",
      "properties": {
        "entity_key": {"type": "string"},
        "kind": {"type": "string", "description": "Optional: IMPORTS, USES_TYPE, READS, WRITES"}
      },
      "required": ["entity_key"]
    }
  },
  {
    "name": "get_change_history",
    "description": "Get the change history for a file or entity across past attempts.",
    "inputSchema": {
      "type": "object",
      "properties": {
        "entity_key": {"type": "string", "description": "Entity key or file path"}
      },
      "required": ["entity_key"]
    }
  }
]"#;

// ═══════════════════════════════════════════════════
// MCP Server
// ═══════════════════════════════════════════════════

struct McpServer { repo: String, addr: String }

impl McpServer {
    fn new(repo: String, addr: String) -> Self { Self { repo, addr } }

    async fn connect(&self) -> Result<QueryServiceClient<Channel>, String> {
        QueryServiceClient::connect(format!("http://{}", self.addr)).await
            .map_err(|e| format!("connect: {e}"))
    }

    async fn handle(&self, req: Request) -> Response {
        match req.method.as_str() {
            "initialize" => rpc_ok(req.id, json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "onto-graph-mcp", "version": "0.1.0"}
            })),
            "tools/list" => {
                let tools: Value = serde_json::from_str(TOOLS_JSON)
                    .expect("TOOLS_JSON must be valid JSON");
                rpc_ok(req.id, json!({"tools": tools}))
            }
            "tools/call" => self.handle_tool_call(req).await,
            _ => rpc_err(req.id, -32601, format!("unknown method: {}", req.method)),
        }
    }

    async fn handle_tool_call(&self, req: Request) -> Response {
        let name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
        match dispatch_tool(self, name, &args).await {
            Ok(text) => rpc_ok(req.id, json!({"content": [{"type": "text", "text": text}]})),
            Err(e) => rpc_ok(req.id, json!({"content": [{"type": "text", "text": format!("Error: {e}")}], "isError": true})),
        }
    }
}

/// Dispatch a tool by name. Extracted for clarity.
async fn dispatch_tool(srv: &McpServer, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "search_symbols"    => tool_search_symbols(srv, args).await,
        "get_symbol_context" => tool_get_context(srv, args).await,
        "get_callers"       => tool_get_callers(srv, args).await,
        "get_callees"       => tool_get_callees(srv, args).await,
        "get_references"    => tool_get_references(srv, args).await,
        "get_change_history" => tool_get_history(srv, args).await,
        _ => Err(format!("unknown tool: {name}")),
    }
}

// ── Tool implementations ──

async fn tool_search_symbols(srv: &McpServer, args: &Value) -> Result<String, String> {
    let query = arg_str(args, "query");
    let kind = arg_str(args, "kind");
    let lang = arg_str(args, "language");
    let mut c = srv.connect().await?;
    let resp = c.search_symbols(SearchSymbolsRequest {
        repository_name: srv.repo.clone(), query: query.into(),
        entity_kind: kind.into(), language: lang.into(), max_results: 20,
    }).await.map_err(|e| format!("{e}"))?;
    let r = resp.into_inner();
    if r.symbols.is_empty() { return Ok("No symbols found.".into()); }
    let lines: Vec<String> = r.symbols.iter().map(|s|
        format!("{} | {} | {} | {}:{}", s.qualified_name, s.entity_kind, s.language, s.file_path, s.start_line)
    ).collect();
    Ok(format!("Found {} symbol(s):\n{}", lines.len(), lines.join("\n")))
}

async fn tool_get_context(srv: &McpServer, args: &Value) -> Result<String, String> {
    let key = arg_str(args, "entity_key");
    let mut c = srv.connect().await?;
    let resp = c.get_symbol_context(GetSymbolContextRequest {
        repository_name: srv.repo.clone(), entity_key: key.into(),
    }).await.map_err(|e| format!("{e}"))?;
    let r = resp.into_inner();
    match r.symbol {
        Some(s) => Ok(format!("Symbol: {} ({} in {})\nCallers: {}\nCallees: {}\nReferences: {}",
            s.qualified_name, s.entity_kind, s.file_path,
            r.callers.len(), r.callees.len(), r.references.len())),
        None => Err("Symbol not found.".into()),
    }
}

async fn tool_get_callers(srv: &McpServer, args: &Value) -> Result<String, String> {
    let key = arg_str(args, "entity_key");
    let depth = args.get("max_depth").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
    let mut c = srv.connect().await?;
    let resp = c.get_callers(GetCallersRequest {
        repository_name: srv.repo.clone(), entity_key: key.into(), max_depth: depth, max_results: 20,
    }).await.map_err(|e| format!("{e}"))?;
    let symbols: Vec<String> = resp.into_inner().symbols.iter().map(|s| s.qualified_name.clone()).collect();
    if symbols.is_empty() { Ok("No callers found.".into()) }
    else { Ok(format!("{} caller(s):\n{}", symbols.len(), symbols.join("\n"))) }
}

async fn tool_get_callees(srv: &McpServer, args: &Value) -> Result<String, String> {
    let key = arg_str(args, "entity_key");
    let mut c = srv.connect().await?;
    let resp = c.get_callees(GetCalleesRequest {
        repository_name: srv.repo.clone(), entity_key: key.into(),
        max_depth: 1, max_results: 20,
    }).await.map_err(|e| format!("{e}"))?;
    let symbols: Vec<String> = resp.into_inner().symbols.iter().map(|s| s.qualified_name.clone()).collect();
    if symbols.is_empty() { Ok("No callees found.".into()) }
    else { Ok(format!("{} callee(s):\n{}", symbols.len(), symbols.join("\n"))) }
}

async fn tool_get_references(srv: &McpServer, args: &Value) -> Result<String, String> {
    let key = arg_str(args, "entity_key");
    let mut c = srv.connect().await?;
    let resp = c.get_references(GetReferencesRequest {
        repository_name: srv.repo.clone(), entity_key: key.into(),
        reference_kind: String::new(), max_results: 20,
    }).await.map_err(|e| format!("{e}"))?;
    let symbols: Vec<String> = resp.into_inner().symbols.iter().map(|s| s.qualified_name.clone()).collect();
    if symbols.is_empty() { Ok("No references found.".into()) }
    else { Ok(format!("{} reference(s):\n{}", symbols.len(), symbols.join("\n"))) }
}

async fn tool_get_history(srv: &McpServer, args: &Value) -> Result<String, String> {
    let key = arg_str(args, "entity_key");
    let mut c = srv.connect().await?;
    let resp = c.get_previous_changes(GetPreviousChangesRequest {
        repository_name: srv.repo.clone(), entity_key: key.into(), max_results: 10,
    }).await.map_err(|e| format!("{e}"))?;
    let r = resp.into_inner();
    if r.changes.is_empty() { return Ok("No change history found.".into()); }
    let lines: Vec<String> = r.changes.iter().map(|ch|
        format!("{} | {} | {}", ch.attempt_id, ch.change_type,
                ch.checkpoint_hash.chars().take(12).collect::<String>())
    ).collect();
    Ok(format!("{} change(s):\n{}", lines.len(), lines.join("\n")))
}

// ── Helpers ──

fn arg_str<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn rpc_ok(id: Option<Value>, result: Value) -> Response {
    Response { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
}

fn rpc_err(id: Option<Value>, code: i32, message: String) -> Response {
    Response { jsonrpc: "2.0".into(), id, result: None, error: Some(RpcError { code, message }) }
}

// ═══════════════════════════════════════════════════
// Main — stdio + HTTP entry points
// ═══════════════════════════════════════════════════

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let repo = std::env::args()
        .position(|a| a == "--repo").and_then(|i| std::env::args().nth(i+1))
        .unwrap_or_else(|| "default".into());
    let grpc_addr = std::env::args()
        .position(|a| a == "--addr").and_then(|i| std::env::args().nth(i+1))
        .unwrap_or_else(|| "localhost:50051".into());
    let http_port: u16 = std::env::args()
        .position(|a| a == "--http-port").and_then(|i| std::env::args().nth(i+1))
        .and_then(|p| p.parse().ok()).unwrap_or(0);

    eprintln!("[onto-graph-mcp] repo={repo} grpc={grpc_addr} http_port={http_port}");
    let state = Arc::new(McpServer::new(repo, grpc_addr));

    if http_port > 0 { run_http(state, http_port).await }
    else { run_stdio(state).await }
}

async fn run_stdio(state: Arc<McpServer>) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() { continue; }
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(req) => state.handle(req).await,
            Err(e) => rpc_err(None, -32700, format!("Parse error: {e}")),
        };
        writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
        stdout.flush()?;
    }
    Ok(())
}

async fn run_http(state: Arc<McpServer>, port: u16) -> anyhow::Result<()> {
    use axum::{Router, routing::post, Json, extract::State as AxumState};

    #[derive(Clone)]
    struct AppState { server: Arc<McpServer> }

    async fn mcp_handler(AxumState(state): AxumState<AppState>, Json(body): Json<Value>) -> Json<Value> {
        let resp = match serde_json::from_value::<Request>(body) {
            Ok(req) => state.server.handle(req).await,
            Err(e) => rpc_err(None, -32700, format!("Parse error: {e}")),
        };
        Json(serde_json::to_value(resp).unwrap_or(json!({"error": "serialize"})))
    }

    let app = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(AppState { server: state });
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    eprintln!("[onto-graph-mcp] HTTP MCP on :{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ═══════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_has_six_tools() {
        let tools: Value = serde_json::from_str(TOOLS_JSON).unwrap();
        assert_eq!(tools.as_array().unwrap().len(), 6);
    }

    #[test]
    fn initialize_returns_capabilities() {
        let s = McpServer::new("test".into(), "localhost:0".into());
        let req = Request { jsonrpc: "2.0".into(), id: Some(json!(1)),
            method: "initialize".into(), params: json!({}) };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(s.handle(req));
        assert!(resp.result.unwrap()["capabilities"]["tools"].is_object());
    }

    #[test]
    fn unknown_method_returns_error() {
        let s = McpServer::new("test".into(), "localhost:0".into());
        let req = Request { jsonrpc: "2.0".into(), id: Some(json!(1)),
            method: "nonexistent".into(), params: json!({}) };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(s.handle(req));
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn unknown_tool_returns_error_text() {
        let s = McpServer::new("test".into(), "localhost:0".into());
        let req = Request { jsonrpc: "2.0".into(), id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({"name": "nonexistent", "arguments": {}}) };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let resp = rt.block_on(s.handle(req));
        assert!(resp.result.unwrap()["content"][0]["text"].as_str().unwrap().contains("unknown tool"));
    }
}
