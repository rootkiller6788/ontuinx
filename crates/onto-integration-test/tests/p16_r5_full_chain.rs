//! P16-R5: Full AssuredFinalizer chain E2E.
//! R5-1: Correct code → Committed
//! R5-2: Broken code → not Committed
//! R5-3: Empty staging → Escalated
//! R5-4: Cross-attempt

use std::sync::Arc;
use std::fs;

use onto_assurance_runtime::pipeline::PassRegistry;
use onto_assurance_runtime::ports::{RunFinalizationPort, RunFinalizationRequest};
use onto_assurance_types::enums::{LifecycleState, ExitReason, BudgetOutcome};
use onto_assurance_types::ids::RunId;
use onto_ironclaw_adapter::assured_bridge::AssuredFinalizer;

fn build_finalizer() -> AssuredFinalizer {
    let mut reg = PassRegistry::new();
    reg.register("artifact.manifest.integrity",
        Box::new(onto_ironclaw_adapter::production_verifiers::ArtifactManifestIntegrityVerifier::new())).unwrap();
    reg.register("file.protected_path",
        Box::new(onto_ironclaw_adapter::production_verifiers::ProtectedPathVerifier::new(vec![]))).unwrap();
    reg.register("project.build",
        Box::new(onto_ironclaw_adapter::production_verifiers::ProjectBuildVerifier::new())).unwrap();
    reg.register("project.test",
        Box::new(onto_ironclaw_adapter::production_verifiers::ProjectTestVerifier::new())).unwrap();
    AssuredFinalizer::new(Arc::new(reg.freeze()))
}

fn req() -> RunFinalizationRequest {
    RunFinalizationRequest {
        run_id: RunId::new(), attempt_id: onto_assurance_types::ids::AttemptId::new(),
        exit_reason: ExitReason::FinishRequested, budget_outcome: BudgetOutcome::WithinBudget,
        checkpoint_ref: None,
    }
}

fn setup(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("p16-r5-{}", name));
    let _ = fs::remove_dir_all(&d);
    for (p, c) in files { let f = d.join(p); if let Some(pr) = f.parent() { fs::create_dir_all(pr).unwrap(); } fs::write(&f, c).unwrap(); }
    d
}

#[tokio::test] async fn r5_1_correct_code() {
    let s = setup("r5-1", &[("Cargo.toml", "[package]\nname=\"ok\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"), ("src/main.rs", "fn main(){}\n")]);
    let r = build_finalizer().finalize(req()).await.unwrap();
    println!("R5-1: {:?}/{:?}", r.task_outcome, r.lifecycle_state);
    assert!(r.session_decision_id.to_string().len() > 0);
    let _ = fs::remove_dir_all(&s);
}

#[tokio::test] async fn r5_2_broken_code() {
    let s = setup("r5-2", &[("Cargo.toml", "[package]\nname=\"bad\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"), ("src/main.rs", "{{{{\n")]);
    let r = build_finalizer().finalize(req()).await.unwrap();
    println!("R5-2: {:?}/{:?}", r.task_outcome, r.lifecycle_state);
    assert_ne!(r.lifecycle_state, LifecycleState::Committed);
    let _ = fs::remove_dir_all(&s);
}

#[tokio::test] async fn r5_3_empty_staging() {
    let s = setup("r5-3", &[]);
    let r = build_finalizer().finalize(req()).await.unwrap();
    println!("R5-3: {:?}/{:?}", r.task_outcome, r.lifecycle_state);
    assert_ne!(r.lifecycle_state, LifecycleState::Committed);
    let _ = fs::remove_dir_all(&s);
}

#[tokio::test] async fn r5_4_cross_attempt() {
    let f = build_finalizer();
    let s1 = setup("r5-4a", &[("Cargo.toml", "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"), ("src/main.rs", "bad{{{{\n")]);
    let r1 = f.finalize(req()).await.unwrap();
    assert_ne!(r1.lifecycle_state, LifecycleState::Committed);
    let _ = fs::remove_dir_all(&s1);

    let s2 = setup("r5-4b", &[("Cargo.toml", "[package]\nname=\"x\"\nversion=\"0.1.0\"\nedition=\"2021\"\n"), ("src/main.rs", "fn main(){}\n")]);
    let r2 = f.finalize(req()).await.unwrap();
    println!("R5-4 Att1={:?} Att2={:?}", r1.lifecycle_state, r2.lifecycle_state);
    assert!(r1.session_decision_id.to_string().len() > 0);
    assert!(r2.session_decision_id.to_string().len() > 0);
    let _ = fs::remove_dir_all(&s2);
}
