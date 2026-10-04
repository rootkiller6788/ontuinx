//! Real file verifiers — M6-A evidence producers.
//!
//! V1: FileExists, FileContent (exact match), ProtectedPath.
//! Each verifier checks a staging directory and produces a VerifierReport.

use std::fs;
use std::path::Path;

use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind};
use onto_assurance_types::ids::{CriterionId, EvidenceId, TransactionId};

/// Result of running a single verifier against a staging snapshot.
pub struct VerifierReport {
    pub criterion_id: CriterionId,
    pub satisfied: bool,
    pub detail: String,
    pub evidence: EvidenceRecord,
}

fn make_evidence(criterion_id: CriterionId, passed: bool, detail: &str) -> EvidenceRecord {
    EvidenceRecord {
        evidence_id: EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id,
        kind: EvidenceRecordKind::VerifierReport,
        payload: serde_json::json!({"passed": passed, "detail": detail}),
        recorded_at: chrono::Utc::now(),
    }
}

// ══════════════════════════════════════════════════════════════════
// V1: FileExistsVerifier
// ══════════════════════════════════════════════════════════════════

/// Checks that a specific file exists in the staging directory.
pub struct FileExistsVerifier {
    pub expected_path: String,
    pub proto_desc: VerifierDescriptor,
}

impl FileExistsVerifier {
    pub fn new(path: impl Into<String>) -> Self { Self { expected_path: path.into(), proto_desc: file_integrity_descriptor("file-exists") } }

    pub fn verify(&self, staging_root: &Path, criterion_id: CriterionId) -> VerifierReport {
        let full_path = staging_root.join(&self.expected_path);
        let exists = full_path.exists() && full_path.is_file();
        let detail = if exists {
            let size = fs::metadata(&full_path).map(|m| m.len()).unwrap_or(0);
            format!("file '{}' exists ({} bytes)", self.expected_path, size)
        } else {
            format!("file '{}' does NOT exist", self.expected_path)
        };
        VerifierReport {
            criterion_id,
            satisfied: exists,
            detail: detail.clone(),
            evidence: make_evidence(criterion_id, exists, &detail),
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// V2: FileContentVerifier
// ══════════════════════════════════════════════════════════════════

/// Checks file content against expected value.
pub struct FileContentVerifier {
    pub file_path: String,
    pub expected_content: String,
    pub mode: ContentMatchMode,
    pub proto_desc: VerifierDescriptor,
}

pub enum ContentMatchMode { Exact, Contains, NotContains }

impl FileContentVerifier {
    pub fn new_exact(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self { file_path: path.into(), expected_content: content.into(),
            mode: ContentMatchMode::Exact, proto_desc: file_integrity_descriptor("file-content") }
    }
    pub fn new_contains(path: impl Into<String>, substring: impl Into<String>) -> Self {
        Self { file_path: path.into(), expected_content: substring.into(),
            mode: ContentMatchMode::Contains, proto_desc: file_integrity_descriptor("file-content") }
    }
    pub fn new_not_contains(path: impl Into<String>, forbidden: impl Into<String>) -> Self {
        Self { file_path: path.into(), expected_content: forbidden.into(),
            mode: ContentMatchMode::NotContains, proto_desc: file_integrity_descriptor("file-content") }
    }

    pub fn verify(&self, staging_root: &Path, criterion_id: CriterionId) -> VerifierReport {
        let full_path = staging_root.join(&self.file_path);
        let content = match fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                let detail = format!("cannot read '{}': {}", self.file_path, e);
                return VerifierReport { criterion_id, satisfied: false, detail: detail.clone(), evidence: make_evidence(criterion_id, false, &detail) };
            }
        };

        let (satisfied, detail) = match self.mode {
            ContentMatchMode::Exact => {
                let ok = content == self.expected_content;
                (ok, if ok { format!("content matches exactly ({} bytes)", content.len()) } else { format!("content mismatch: expected {} bytes, got {} bytes", self.expected_content.len(), content.len()) })
            }
            ContentMatchMode::Contains => {
                let ok = content.contains(&self.expected_content);
                (ok, if ok { format!("content contains '{}'", &self.expected_content[..self.expected_content.len().min(50)]) } else { format!("content does NOT contain required string") })
            }
            ContentMatchMode::NotContains => {
                let ok = !content.contains(&self.expected_content);
                (ok, if ok { "forbidden content not found".into() } else { "forbidden content FOUND in file".into() })
            }
        };

        VerifierReport {
            criterion_id, satisfied, detail: detail.clone(),
            evidence: make_evidence(criterion_id, satisfied, &detail),
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// V3: ProtectedPathVerifier
// ══════════════════════════════════════════════════════════════════

/// Checks that protected paths were not modified.
pub struct ProtectedPathVerifier {
    pub protected_paths: Vec<String>,
    pub baseline_hashes: Vec<(String, String)>,
    pub proto_desc: VerifierDescriptor,
}

impl ProtectedPathVerifier {
    pub fn new(protected: Vec<String>) -> Self {
        Self { protected_paths: protected, baseline_hashes: vec![], proto_desc: file_integrity_descriptor("protected-path") }
    }
    pub fn with_baseline(mut self, hashes: Vec<(String, String)>) -> Self {
        self.baseline_hashes = hashes; self
    }

    pub fn verify(&self, staging_root: &Path, criterion_id: CriterionId) -> VerifierReport {
        let mut violations = Vec::new();

        for path in &self.protected_paths {
            let full = staging_root.join(path);
            if !full.exists() {
                violations.push(format!("{}: missing (was deleted)", path));
                continue;
            }
            // Check against baseline if available
            if let Some((_, expected_hash)) = self.baseline_hashes.iter().find(|(p, _)| p == path) {
                if let Ok(content) = fs::read(&full) {
                    let actual_hash = hex::encode(sha2::Sha256::digest(&content));
                    if &actual_hash != expected_hash {
                        violations.push(format!("{}: hash changed (MODIFIED)", path));
                    }
                }
            }
        }

        let satisfied = violations.is_empty();
        let detail = if satisfied { "all protected paths unchanged".into() } else { violations.join("; ") };

        VerifierReport {
            criterion_id, satisfied, detail: detail.clone(),
            evidence: make_evidence(criterion_id, satisfied, &detail),
        }
    }
}

use sha2::Digest;

// ── New unified Verifier trait impls (added to existing structs) ──

use async_trait::async_trait;
use onto_protocol::verifier::{Verifier, VerifierDescriptor, Pass, VerificationStage, VerificationMode, VerifierResult, VerifierStatus, VerifierServices};
use onto_protocol::context::VerificationContext;
use onto_protocol::check::ExternalCheckRequirement;
use onto_protocol::sandbox::RawCheckResult;

fn file_integrity_descriptor(id: &str) -> VerifierDescriptor {
    VerifierDescriptor {
        verifier_id: id.to_string(), pass: Pass::FileIntegrity,
        stage: VerificationStage::PreGraph, mode: VerificationMode::Internal,
        supported_rules: vec!["file_integrity".to_string()],
    }
}

#[async_trait]
impl Verifier for FileExistsVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.proto_desc }
    fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
    async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
        VerifierResult {
            verifier_id: "file-exists".to_string(), pass: Pass::FileIntegrity,
            status: VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None,
        }
    }
}

#[async_trait]
impl Verifier for FileContentVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.proto_desc }
    fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
    async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
        VerifierResult {
            verifier_id: "file-content".to_string(), pass: Pass::FileIntegrity,
            status: VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None,
        }
    }
}

#[async_trait]
impl Verifier for ProtectedPathVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.proto_desc }
    fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
    async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
        VerifierResult {
            verifier_id: "protected-path".to_string(), pass: Pass::FileIntegrity,
            status: VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::io::Write;

    fn setup_staging() -> (TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let staging = tmp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        (tmp, staging)
    }

    #[test]
    fn file_exists_when_present() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("hello.txt"), "hello").unwrap();
        let v = FileExistsVerifier::new("hello.txt");
        let report = v.verify(&staging, CriterionId::new());
        assert!(report.satisfied);
    }

    #[test]
    fn file_exists_when_missing() {
        let (_tmp, staging) = setup_staging();
        let v = FileExistsVerifier::new("missing.txt");
        let report = v.verify(&staging, CriterionId::new());
        assert!(!report.satisfied);
    }

    #[test]
    fn content_exact_match() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("out.txt"), "hello from onto\n").unwrap();
        let v = FileContentVerifier::new_exact("out.txt", "hello from onto\n");
        let report = v.verify(&staging, CriterionId::new());
        assert!(report.satisfied);
    }

    #[test]
    fn content_mismatch_detected() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("out.txt"), "wrong").unwrap();
        let v = FileContentVerifier::new_exact("out.txt", "hello from onto\n");
        let report = v.verify(&staging, CriterionId::new());
        assert!(!report.satisfied);
    }

    #[test]
    fn content_contains_detects_substring() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("code.py"), "def hello():\n    return 'Hello World'\n").unwrap();
        let v = FileContentVerifier::new_contains("code.py", "Hello World");
        let report = v.verify(&staging, CriterionId::new());
        assert!(report.satisfied);
    }

    #[test]
    fn forbidden_content_detected() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("code.py"), "import os; os.system('rm -rf /')").unwrap();
        let v = FileContentVerifier::new_not_contains("code.py", "os.system");
        let report = v.verify(&staging, CriterionId::new());
        assert!(!report.satisfied);
    }

    #[test]
    fn protected_path_unchanged() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("locked.txt"), "secret").unwrap();
        let v = ProtectedPathVerifier::new(vec!["locked.txt".into()])
            .with_baseline(vec![("locked.txt".into(), hex::encode(sha2::Sha256::digest(b"secret")))]);
        let report = v.verify(&staging, CriterionId::new());
        assert!(report.satisfied);
    }

    #[test]
    fn protected_path_modified_detected() {
        let (_tmp, staging) = setup_staging();
        fs::write(staging.join("locked.txt"), "hacked").unwrap();
        let v = ProtectedPathVerifier::new(vec!["locked.txt".into()])
            .with_baseline(vec![("locked.txt".into(), hex::encode(sha2::Sha256::digest(b"secret")))]);
        let report = v.verify(&staging, CriterionId::new());
        assert!(!report.satisfied);
    }
}
