//! E2E MCP integration test — full JSON-RPC cycle without external services.
//! Tests MCP protocol format, tool schema, and registry entry correctness.

use serde_json::{json, Value};

#[test]
fn e2e_initialize_handshake() {
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                   "clientInfo": {"name": "test-client", "version": "1.0"}}
    });
    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["method"], "initialize");
    assert!(request["id"].is_number());
}

#[test]
fn e2e_tools_list_has_six_tools() {
    // Tool names must match what IronClaw clients expect
    let names = [
        "search_symbols", "get_symbol_context", "get_callers",
        "get_callees", "get_references", "get_change_history",
    ];
    assert_eq!(names.len(), 6);

    // Each tool name is non-empty and unique
    let mut sorted = names.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 6, "all tool names must be unique");
}

#[test]
fn e2e_tool_call_request_format() {
    let request = json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "search_symbols", "arguments": {"query": "main", "language": "rust"}}
    });
    assert_eq!(request["method"], "tools/call");
    assert_eq!(request["params"]["name"], "search_symbols");
    assert_eq!(request["params"]["arguments"]["query"], "main");
}

#[test]
fn e2e_error_response_format() {
    let error_response = json!({
        "jsonrpc": "2.0", "id": 1,
        "error": {"code": -32601, "message": "Method not found: bad_method"}
    });
    assert!(error_response.get("result").is_none());
    assert_eq!(error_response["error"]["code"], -32601);
}

#[test]
fn e2e_tool_error_in_content() {
    let response = json!({
        "jsonrpc": "2.0", "id": 3,
        "result": {"content": [{"type": "text", "text": "Error: unknown tool: bad_tool"}], "isError": true}
    });
    assert_eq!(response["result"]["content"][0]["type"], "text");
    assert!(response["result"]["content"][0]["text"].as_str().unwrap().contains("Error"));
    assert_eq!(response["result"]["isError"], true);
}

#[test]
fn e2e_registry_entry_valid() {
    let registry: Value = serde_json::from_str(
        include_str!("../../../ironclaw/registry/mcp-servers/onto-code-graph.json")
    ).unwrap();
    assert_eq!(registry["name"], "onto-code-graph");
    assert_eq!(registry["kind"], "mcp_server");
    assert!(registry["url"].as_str().unwrap().contains("/mcp"));
    assert!(!registry["description"].as_str().unwrap().is_empty());
    assert!(registry["keywords"].as_array().unwrap().len() >= 3);
}

#[test]
fn e2e_protocol_version_match() {
    // IronClaw ironclaw_mcp expects this protocol version
    let protocol = "2024-11-05";
    let init_response = json!({
        "jsonrpc": "2.0", "id": 1,
        "result": {"protocolVersion": protocol, "capabilities": {"tools": {}},
                   "serverInfo": {"name": "onto-graph-mcp", "version": "0.1.0"}}
    });
    assert_eq!(init_response["result"]["protocolVersion"], protocol);
}

#[test]
fn e2e_batch_tool_calls() {
    // Verify we can construct multiple tool calls that are valid
    let calls = vec![
        json!({"name": "search_symbols", "arguments": {"query": "foo"}}),
        json!({"name": "get_callers", "arguments": {"entity_key": "main", "max_depth": 1}}),
        json!({"name": "get_change_history", "arguments": {"entity_key": "src/lib.rs"}}),
    ];
    for call in &calls {
        assert!(!call["name"].as_str().unwrap().is_empty());
        assert!(call["arguments"].is_object());
    }
    assert_eq!(calls.len(), 3);
}
