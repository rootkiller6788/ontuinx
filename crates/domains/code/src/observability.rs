//! Observability — 统一 trace 传播 + 关键指标 (S3.4)。
//!
//! 所有服务统一传播 12 个 trace 字段。9 个关键指标衡量系统健康。

/// Trace 上下文 — 全链路传播。
#[derive(Debug, Clone, Default)]
pub struct TraceContext {
    pub trace_id: String,
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub attempt_id: String,
    pub run_id: String,
    pub verification_session_id: String,
    pub verification_unit_id: String,
    pub verifier_run_id: String,
    pub decision_id: String,
    pub settlement_id: String,
    pub generation: u64,
}

impl TraceContext {
    pub fn new(trace_id: &str) -> Self {
        Self { trace_id: trace_id.into(), ..Default::default() }
    }

    pub fn with_flow(mut self, id: &str) -> Self { self.flow_id = id.into(); self }
    pub fn with_work_item(mut self, id: &str) -> Self { self.work_item_id = id.into(); self }
    pub fn with_loop(mut self, id: &str) -> Self { self.loop_id = id.into(); self }
    pub fn with_attempt(mut self, id: &str) -> Self { self.attempt_id = id.into(); self }
    pub fn with_session(mut self, id: &str) -> Self { self.verification_session_id = id.into(); self }
    pub fn with_decision(mut self, id: &str) -> Self { self.decision_id = id.into(); self }
    pub fn with_generation(mut self, g: u64) -> Self { self.generation = g; self }

    /// 是否完整（所有关键字段都填充）。
    pub fn is_complete(&self) -> bool {
        !self.trace_id.is_empty() && !self.flow_id.is_empty() && !self.loop_id.is_empty()
    }
}

/// 系统健康指标（Prometheus 兼容）。
#[derive(Debug, Clone, Default)]
pub struct VerificationMetrics {
    pub scope_incomplete_total: u64,
    pub required_verifier_failed_total: u64,
    pub ambiguous_location_total: u64,
    pub stale_finding_rejected_total: u64,
    pub lane_violation_total: u64,
    pub duplicate_effect_prevented_total: u64,
    pub session_resume_total: u64,
    pub stale_generation_rejected_total: u64,
    pub authority_resolution_latency_ms: Vec<u64>,
}

impl VerificationMetrics {
    pub fn record_scope_incomplete(&mut self) { self.scope_incomplete_total += 1; }
    pub fn record_verifier_failed(&mut self) { self.required_verifier_failed_total += 1; }
    pub fn record_ambiguous(&mut self) { self.ambiguous_location_total += 1; }
    pub fn record_stale_rejected(&mut self) { self.stale_finding_rejected_total += 1; }
    pub fn record_lane_violation(&mut self) { self.lane_violation_total += 1; }
    pub fn record_duplicate_prevented(&mut self) { self.duplicate_effect_prevented_total += 1; }
    pub fn record_session_resume(&mut self) { self.session_resume_total += 1; }
    pub fn record_stale_gen_rejected(&mut self) { self.stale_generation_rejected_total += 1; }
    pub fn record_authority_latency(&mut self, ms: u64) { self.authority_resolution_latency_ms.push(ms); }

    /// 健康摘要 — 是否有任何异常指标。
    pub fn is_healthy(&self) -> bool {
        self.lane_violation_total == 0 && self.duplicate_effect_prevented_total == 0
    }

    /// 平均 Authority Resolution 延迟。
    pub fn avg_authority_latency_ms(&self) -> f64 {
        if self.authority_resolution_latency_ms.is_empty() { 0.0 }
        else { self.authority_resolution_latency_ms.iter().sum::<u64>() as f64 / self.authority_resolution_latency_ms.len() as f64 }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_context_builder() {
        let ctx = TraceContext::new("tr-1")
            .with_flow("f-1").with_work_item("w-1").with_loop("l-1")
            .with_attempt("a-1").with_session("s-1").with_decision("d-1")
            .with_generation(1);
        assert!(ctx.is_complete());
        assert_eq!(ctx.flow_id, "f-1");
        assert_eq!(ctx.generation, 1);
    }

    #[test]
    fn incomplete_trace() {
        let ctx = TraceContext::new("tr-2");
        assert!(!ctx.is_complete());
    }

    #[test]
    fn metrics_recording() {
        let mut m = VerificationMetrics::default();
        m.record_lane_violation();
        m.record_duplicate_prevented();
        m.record_duplicate_prevented();
        assert_eq!(m.lane_violation_total, 1);
        assert_eq!(m.duplicate_effect_prevented_total, 2);
        assert!(!m.is_healthy()); // violations exist
    }

    #[test]
    fn healthy_when_clean() {
        let m = VerificationMetrics::default();
        assert!(m.is_healthy());
    }

    #[test]
    fn authority_latency_tracking() {
        let mut m = VerificationMetrics::default();
        m.record_authority_latency(100);
        m.record_authority_latency(200);
        assert_eq!(m.avg_authority_latency_ms(), 150.0);
    }

    #[test]
    fn twelve_trace_fields() {
        let ctx = TraceContext::new("t")
            .with_flow("f").with_work_item("w").with_loop("l")
            .with_attempt("a").with_session("s").with_decision("d");
        // Verify all 12 fields are present on the struct
        let _ = ctx.trace_id;
        let _ = ctx.flow_id;
        let _ = ctx.work_item_id;
        let _ = ctx.loop_id;
        let _ = ctx.attempt_id;
        let _ = ctx.run_id;
        let _ = ctx.verification_session_id;
        let _ = ctx.verification_unit_id;
        let _ = ctx.verifier_run_id;
        let _ = ctx.decision_id;
        let _ = ctx.settlement_id;
        let _ = ctx.generation;
    }
}
