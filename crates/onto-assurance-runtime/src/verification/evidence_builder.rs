use onto_assurance_core::verification::finding_normalization;
use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::evidence::{EvidenceRecord,EvidenceRecordKind};
use onto_assurance_types::ids::{EvidenceId,TransactionId,CriterionId};
pub struct EvidenceBuilder;
impl EvidenceBuilder { pub fn new() -> Self { Self }
    pub fn build(&self, tx: TransactionId, cid: CriterionId, fs: &[FindingCandidate], threshold: f64) -> Vec<EvidenceRecord> {
        let filtered = finding_normalization::filter_by_confidence(fs, threshold);
        let mut deduped = finding_normalization::deduplicate(&filtered);
        finding_normalization::sort_by_severity(&mut deduped);
        deduped.into_iter().map(|f| EvidenceRecord{evidence_id:EvidenceId::new(),transaction_id:tx,criterion_id:cid,kind:if f.is_blocking(){EvidenceRecordKind::VerifierReport}else{EvidenceRecordKind::TestOutput},payload:serde_json::to_value(&f).unwrap_or_default(),recorded_at:chrono::Utc::now()}).collect()
    }
    pub fn summary(&self, fs: &[FindingCandidate]) -> finding_normalization::FindingSummary { finding_normalization::summary(fs) }
}
#[cfg(test)] mod tests { use super::*; use onto_assurance_types::finding::{FindingCategory,FindingSeverity,SourceLocation};
    fn mf(id:&str,line:u32,sev:FindingSeverity) -> FindingCandidate { FindingCandidate{finding_id:id.into(),target_id:"t".into(),rule_id:"r".into(),verifier_id:"v".into(),severity:sev,category:FindingCategory::Other,location:Some(SourceLocation{file_path:"f.rs".into(),start_line:line,end_line:line,start_column:None,end_column:None,existing_code:None,suggestion_code:None}),message:"m".into(),suggestion_code:None,confidence:0.95} }
    #[test] fn builds_evidence() { let b = EvidenceBuilder::new(); let fs = vec![mf("a",10,FindingSeverity::Critical), mf("b",20,FindingSeverity::High), mf("c",10,FindingSeverity::Critical)]; let e = b.build(TransactionId::new(),CriterionId::new(),&fs,0.8); assert_eq!(e.len(),2); }
}
