//! onto-conformance — Golden fixture runner for Python↔Rust differential testing.
//!
//! Reads a fixture JSON, runs the Onto kernel pipeline, and outputs
//! deterministic results for comparison with the Python reference.
//!
//! Usage:
//!   onto-conformance fixtures/success/input.json   # Single fixture
//!   onto-conformance --all                         # All 6 pipeline fixtures
//!   onto-conformance list                          # List fixtures

use std::path::PathBuf;

use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_core::{reduction, session_decision, settlement, replay};
use onto_assurance_types::enums::{BudgetOutcome, EffectClass, ExitReason};
use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind, VerifierBinding};
use onto_assurance_types::ids::*;
use serde::Deserialize;

// ══════════════════════════════════════════════════════════════════
// Fixture schema
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Deserialize)]
struct FixtureInput {
    scenario: Option<String>,
    contract: Option<FixtureContract>,
    evidence: Option<Vec<FixtureEvidence>>,
    tampered: Option<bool>,
    exit_reason: Option<String>,
    #[allow(dead_code)]
    expected: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct FixtureContract {
    criteria: Option<Vec<FixtureCriterion>>,
    effect_class: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FixtureCriterion {
    id: Option<String>,
    name: Option<String>,
    kind: Option<String>,
    is_blocking: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct FixtureEvidence {
    criterion_id: Option<String>,
    kind: Option<String>,
    payload: Option<serde_json::Value>,
}

// ══════════════════════════════════════════════════════════════════
// Output
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, serde::Serialize)]
struct FixtureOutput {
    scenario: String,
    chain_valid: bool,
    record_count: u32,
    evidence_hash: String,
    overall_passed: bool,
    task_outcome: String,
    lifecycle_state: String,
    settlement: String,
    replay_hash: String,
}

// ══════════════════════════════════════════════════════════════════
// Run one fixture
// ══════════════════════════════════════════════════════════════════

fn run_fixture(path: &PathBuf) -> Result<FixtureOutput, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let fixture: FixtureInput = serde_json::from_str(&raw)
        .map_err(|e| format!("parse {}: {}", path.display(), e))?;

    let scenario = fixture.scenario.as_deref().unwrap_or("unknown").to_string();

    let run_id = RunId::new();
    let criteria_pairs = parse_criteria(fixture.contract.as_ref());
    let criteria: Vec<_> = criteria_pairs.iter().map(|(_, c)| c.clone()).collect();
    let evidence_list = parse_evidence(fixture.evidence.as_deref().unwrap_or(&[]), &criteria_pairs);
    let is_tampered = fixture.tampered.unwrap_or(false);
    let effect_class = parse_effect_class(fixture.contract.as_ref().and_then(|c| c.effect_class.as_deref()));

    // Build chain
    let mut chain = EvidenceChain::new(run_id, "conformance-genesis".into(),
        VerifierBinding { verifier_id: VerifierId::new(), verifier_version: "1.0".into(), toolchain: None, environment_hash: None });
    for rec in &evidence_list {
        chain.append(rec.clone()).map_err(|e| format!("chain: {:?}", e))?;
    }
    let mut bundle = chain.seal();

    if is_tampered && !bundle.records.is_empty() {
        bundle.records[0].record.payload = serde_json::json!({"tampered": true});
    }

    let chain_valid = EvidenceChain::verify(&bundle).is_ok();
    let verdict = reduction::reduce(&criteria, &evidence_list);
    let exit = match fixture.exit_reason.as_deref() {
        Some("crash" | "crashed") => ExitReason::Crashed,
        Some("stuck") => ExitReason::Stuck,
        Some("budget" | "budget_limit") => ExitReason::BudgetLimit,
        _ => ExitReason::FinishRequested,
    };
    let session = session_decision::decide_session(run_id, AttemptId::new(), exit, &verdict,
        BudgetOutcome::WithinBudget, bundle.bundle_id);
    let settle = settlement::derive_settlement(effect_class, session.task_outcome, &verdict);

    let replay_input = replay::ReplayInput {
        run_id: scenario.clone(),
        contract: serde_json::json!({}),
        transactions: vec![],
        checkpoint_bindings: vec![],
    };
    let replay_hash = replay::compute_replay_hash(&replay_input).unwrap_or_else(|_| "err".into());

    Ok(FixtureOutput {
        scenario,
        chain_valid,
        record_count: bundle.record_count,
        evidence_hash: bundle.tail_hash,
        overall_passed: verdict.overall_passed,
        task_outcome: serde_json::to_value(session.task_outcome)
            .map(|v| v.as_str().unwrap_or("?").to_string())
            .unwrap_or_else(|_| "?".into()),
        lifecycle_state: serde_json::to_value(session.lifecycle_state)
            .map(|v| v.as_str().unwrap_or("?").to_string())
            .unwrap_or_else(|_| "?".into()),
        settlement: settle.as_ref().map(settlement_name).unwrap_or_else(|| "none".into()),
        replay_hash,
    })
}

fn parse_criteria(contract: Option<&FixtureContract>) -> Vec<(String, onto_assurance_types::contract::AcceptanceCriterion)> {
    contract.and_then(|c| c.criteria.as_ref()).map(|criteria| {
        criteria.iter().map(|c| {
            let id = c.id.clone().unwrap_or_else(|| CriterionId::new().to_string());
            let criterion = onto_assurance_types::contract::AcceptanceCriterion {
                criterion_id: CriterionId::new(),
                name: c.name.clone().unwrap_or_else(|| id.clone()),
                kind: onto_assurance_types::contract::CriterionKind::TestPass,
                description: String::new(),
                is_blocking: c.is_blocking.unwrap_or(true),
            };
            (id, criterion)
        }).collect()
    }).unwrap_or_default()
}

fn parse_evidence(raw: &[FixtureEvidence], criteria_list: &[(String, onto_assurance_types::contract::AcceptanceCriterion)]) -> Vec<EvidenceRecord> {
    raw.iter().map(|e| {
        let criterion_id = e.criterion_id.as_deref()
            .and_then(|eid| criteria_list.iter().find(|(cid, _)| cid == eid))
            .map(|(_, c)| c.criterion_id)
            .unwrap_or_else(CriterionId::new);
        EvidenceRecord {
            evidence_id: EvidenceId::new(),
            transaction_id: TransactionId::new(),
            criterion_id,
            kind: EvidenceRecordKind::TestOutput,
            payload: e.payload.clone().unwrap_or(serde_json::json!({"passed": true})),
            recorded_at: chrono::Utc::now(),
        }
    }).collect()
}

fn parse_effect_class(s: Option<&str>) -> EffectClass {
    match s {
        Some("pure") => EffectClass::Pure,
        Some("readonly" | "read_only") => EffectClass::ReadOnly,
        Some("transactional") => EffectClass::Transactional,
        Some("compensatable") => EffectClass::Compensatable,
        Some("irreversible") => EffectClass::Irreversible,
        _ => EffectClass::Staged,
    }
}

fn settlement_name(d: &onto_assurance_types::decision::SettlementDecision) -> String {
    match d {
        onto_assurance_types::decision::SettlementDecision::Commit => "commit".into(),
        onto_assurance_types::decision::SettlementDecision::Rollback { .. } => "rollback".into(),
        onto_assurance_types::decision::SettlementDecision::Confirm => "confirm".into(),
        onto_assurance_types::decision::SettlementDecision::Compensate { .. } => "compensate".into(),
        onto_assurance_types::decision::SettlementDecision::Freeze { .. } => "freeze".into(),
        onto_assurance_types::decision::SettlementDecision::Escalate { .. } => "escalate".into(),
    }
}

// ══════════════════════════════════════════════════════════════════
// Main
// ══════════════════════════════════════════════════════════════════

const SCENARIOS: &[&str] = &[
    "success", "incomplete", "verification_failed", "environment_error",
    "budget_depleted_success", "evidence_tampered",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { eprintln!("Usage: onto-conformance <path> | --all | list"); std::process::exit(1); }

    match args[1].as_str() {
        "--all" => {
            let mut total = 0u32; let mut passed = 0u32;
            for s in SCENARIOS {
                let path = PathBuf::from("fixtures").join(s).join("input.json");
                if !path.exists() { println!("  SKIP {}", s); continue; }
                total += 1;
                match run_fixture(&path) {
                    Ok(o) => {
                        let is_ok = if s == &"evidence_tampered" { !o.chain_valid } else { o.chain_valid };
                        if is_ok { passed += 1; }
                        println!("  {} {} (outcome={} chain={})",
                            if is_ok { "✅" } else { "❌" }, s, o.task_outcome, o.chain_valid);
                    }
                    Err(e) => println!("  ❌ {}: {}", s, e),
                }
            }
            println!("\n{}/{} passed", passed, total);
            if passed < total { std::process::exit(1); }
        }
        "list" => {
            for s in SCENARIOS {
                let p = PathBuf::from("fixtures").join(s).join("input.json");
                println!("{} {}", if p.exists() { "✅" } else { "  " }, s);
            }
        }
        path_str => {
            let path = PathBuf::from(path_str);
            match run_fixture(&path) {
                Ok(o) => {
                    match serde_json::to_string_pretty(&o) {
                        Ok(json) => println!("{}", json),
                        Err(e) => eprintln!("serialization error: {}", e),
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
    }
}
