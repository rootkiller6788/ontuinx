//! P16-X: Golden Vector verification (Rust equivalent of Go verifier).
//! Proves the protocol is self-consistent: digest computation, tamper detection,
//! Committed-without-receipt rejection, Escalated≠Committed enforcement.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Envelope {
    schema_version: u32,
    flow_id: String,
    work_item_id: String,
    loop_id: String,
    execution_generation: u64,
    request_binding_hash: String,
    reported_terminal_state: String,
    decision_id: Option<String>,
    decision_hash: Option<String>,
    evidence_bundle_ref: Option<String>,
    settlement_receipt_ref: Option<String>,
    output_checkpoint_hash: Option<String>,
    outcome_binding_hash: String,
    total_attempts: u32,
    terminal_reason: String,
}

#[derive(Debug, Deserialize)]
struct Expected {
    go_accepts: bool,
    #[allow(dead_code)]
    is_success: Option<bool>,
    #[allow(dead_code)]
    reject_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    #[allow(dead_code)]
    description: String,
    envelope: Envelope,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct VectorsFile {
    #[allow(dead_code)]
    protocol_version: u32,
    #[allow(dead_code)]
    description: String,
    vectors: Vec<Vector>,
}

fn compute_outcome_binding_hash(e: &Envelope) -> String {
    use sha2::{Digest, Sha256};
    let payload = format!(
        "{}:{}:{}:{}:{}:{}::{}:{}:{}",
        e.request_binding_hash,
        e.reported_terminal_state,
        e.decision_id.as_deref().unwrap_or(""),
        e.decision_hash.as_deref().unwrap_or(""),
        e.output_checkpoint_hash.as_deref().unwrap_or(""),
        e.settlement_receipt_ref.as_deref().unwrap_or(""),
        e.execution_generation,
        e.total_attempts,
        1, // schema_version
    );
    let hash = Sha256::digest(payload.as_bytes());
    hex::encode(&hash[..8]) // first 16 hex chars
}

fn validate(e: &Envelope) -> (bool, String) {
    if e.schema_version != 1 {
        return (false, format!("unsupported schema_version: {}", e.schema_version));
    }

    let computed = compute_outcome_binding_hash(e);
    if computed != e.outcome_binding_hash {
        return (false, format!("digest mismatch: computed={}", computed));
    }

    if e.reported_terminal_state == "Committed" && e.settlement_receipt_ref.is_none() {
        return (false, "Committed without receipt".into());
    }

    if e.reported_terminal_state == "Committed"
        && (e.terminal_reason.contains("escalated") || e.terminal_reason.contains("Escalated"))
    {
        return (false, "Committed with escalated reason".into());
    }

    (true, String::new())
}

#[test]
fn p16_x_golden_vectors_all_pass() {
    let json = include_str!("../../../tests/runtime/golden_vectors/p16_golden_vectors.json");
    let vf: VectorsFile = serde_json::from_str(json).expect("parse golden vectors");

    println!("P16-X: {} golden vectors", vf.vectors.len());
    let mut passed = 0;
    let mut failed = 0;

    for v in &vf.vectors {
        let (accepted, reason) = validate(&v.envelope);
        let expected = v.expected.go_accepts;

        if accepted == expected {
            passed += 1;
            println!("  ✅ {}", v.name);
        } else {
            failed += 1;
            if expected && !accepted {
                println!("  ❌ {} REJECTED_BUT_SHOULD_ACCEPT: {}", v.name, reason);
            } else {
                println!("  ❌ {} ACCEPTED_BUT_SHOULD_REJECT", v.name);
            }
        }
    }

    println!("\n{}/{} passed", passed, vf.vectors.len());
    assert_eq!(failed, 0, "{} golden vector(s) failed", failed);
}
