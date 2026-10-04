//! R2: Authority Resolution — Decision 绑定验证

use onto_temporal_adapter::authority_projection::{InMemoryAuthorityProjection, AuthorityProjectionPort, ResolveLoopOutcomeRequest, VerifiedOutcome};

fn req(lid: &str) -> ResolveLoopOutcomeRequest {
    ResolveLoopOutcomeRequest { flow_id: "f".into(), work_item_id: "w".into(), loop_id: lid.into(), execution_generation: 1, terminal_envelope_hash: "h".into() }
}

#[test] fn r2_1_decision_exists_committed() {
    let a = InMemoryAuthorityProjection::new(); a.record_decision("L1","d1","h1","c1",Some("r1"));
    let r = a.resolve_loop_outcome(req("L1")).unwrap();
    assert!(r.outcome.is_committed());
    assert_eq!(r.decision_id, "d1");
}

#[test] fn r2_2_no_decision_not_found() {
    let a = InMemoryAuthorityProjection::new();
    assert!(a.resolve_loop_outcome(req("L2")).is_err());
}

#[test] fn r2_3_rejected_decision_not_committed() {
    let a = InMemoryAuthorityProjection::new(); a.record_rejected_decision("L3","d3","fail");
    let r = a.resolve_loop_outcome(req("L3")).unwrap();
    assert!(matches!(r.outcome, VerifiedOutcome::NotCommitted{..}));
}

#[test] fn r2_4_binding_hash_deterministic() {
    let a = InMemoryAuthorityProjection::new(); a.record_decision("L4","d4","h4","c4",Some("r4"));
    let r1 = a.resolve_loop_outcome(req("L4")).unwrap();
    let r2 = a.resolve_loop_outcome(req("L4")).unwrap();
    assert_eq!(r1.authority_binding_hash, r2.authority_binding_hash);
}

#[test] fn r2_5_different_loops_different_hashes() {
    let a = InMemoryAuthorityProjection::new();
    a.record_decision("LA","dA","hA","cA",Some("rA"));
    a.record_decision("LB","dB","hB","cB",Some("rB"));
    assert_ne!(a.resolve_loop_outcome(req("LA")).unwrap().authority_binding_hash, a.resolve_loop_outcome(req("LB")).unwrap().authority_binding_hash);
}
