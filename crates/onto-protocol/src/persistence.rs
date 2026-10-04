//! P16-P2A: Versioned persistence DTOs.
//!
//! These DTOs are the STABLE serialization contract for PostgreSQL JSONB.
//! Domain types are never serialized directly — they go through these DTOs.
//! Schema version is embedded in every DTO so migrations are explicit.

use serde::{Deserialize, Serialize};
use crate::check::{Applicability, ConformancePlan, ConformancePlanSummary, ConformanceUnit};
use crate::digest::{Digest, DigestAlgorithm};
use crate::verdict::{
    ConformanceVerdict, ConformanceUnitResult, ConformanceOutcome,
    CoverageState, FreshnessState, SandboxValidationSummary,
    GraphValidationBinding, GraphUnavailableReason, UnitExecutionStatus,
};
use crate::verifier::Pass;

const SCHEMA_VERSION: u16 = 1;

// ══════════════════════════════════════════════════════════════════
// Plan DTOs
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConformancePlanV1 {
    pub schema_version: u16,
    pub plan_id: String,
    pub plan_digest: String,       // hex-encoded SHA-256
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: String,  // hex-encoded SHA-256
    pub profile_id: String,
    pub profile_version: String,
    pub units: Vec<StoredConformanceUnitV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConformanceUnitV1 {
    pub unit_id: String,
    pub verifier_id: String,
    pub pass: String,              // Display-formatted Pass enum
    pub validation_dependencies: Vec<String>,
    pub applicability: String,     // "required" | "optional" | "not_applicable"
}

impl TryFrom<&ConformancePlan> for StoredConformancePlanV1 {
    type Error = String;
    fn try_from(p: &ConformancePlan) -> Result<Self, Self::Error> {
        Ok(StoredConformancePlanV1 {
            schema_version: SCHEMA_VERSION,
            plan_id: p.plan_id.clone(),
            plan_digest: p.plan_digest.value.clone(),
            attempt_id: p.attempt_id.clone(),
            candidate_id: p.candidate_id.clone(),
            candidate_digest: p.candidate_digest.value.clone(),
            profile_id: p.profile_id.clone(),
            profile_version: p.profile_version.clone(),
            units: p.units.iter().map(|u| StoredConformanceUnitV1 {
                unit_id: u.unit_id.clone(),
                verifier_id: u.verifier_id.clone(),
                pass: format!("{:?}", u.pass),
                validation_dependencies: u.validation_dependencies.clone(),
                applicability: match u.applicability {
                    Applicability::Required => "required".into(),
                    Applicability::Optional => "optional".into(),
                    Applicability::NotApplicable => "not_applicable".into(),
                },
            }).collect(),
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Verdict DTOs
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredConformanceVerdictV1 {
    pub schema_version: u16,
    pub verdict_id: String,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: String,
    pub plan_id: String,
    pub plan_digest: String,
    pub evidence_bundle_ref: String,
    pub evidence_bundle_digest: String,
    pub conformance: String,       // "conformant" | "non_conformant" | "inconclusive"
    pub freshness: String,         // "current" | "stale"
    pub coverage: String,          // "complete" | "partial" | "unavailable"
    pub sandbox: StoredSandboxV1,
    pub blocking_findings: usize,  // count only (findings stored separately)
    pub advisory_findings: usize,
    pub unit_results: Vec<StoredUnitVerdictV1>,
    pub verdict_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredUnitVerdictV1 {
    pub unit_id: String,
    pub verifier_id: String,
    pub pass: String,
    pub status: String,            // Display-formatted UnitExecutionStatus
    pub applicability: String,
    pub finding_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSandboxV1 {
    pub state: String,  // "executed_completed" | "not_run" | "startup_failed" | "runtime_lost"
    pub detail: Option<String>,
}

impl TryFrom<&ConformanceVerdict> for StoredConformanceVerdictV1 {
    type Error = String;
    fn try_from(v: &ConformanceVerdict) -> Result<Self, Self::Error> {
        Ok(StoredConformanceVerdictV1 {
            schema_version: SCHEMA_VERSION,
            verdict_id: v.verdict_id.clone(),
            attempt_id: v.attempt_id.clone(),
            candidate_id: v.candidate_id.clone(),
            candidate_digest: v.candidate_digest.value.clone(),
            plan_id: v.plan_id.clone(),
            plan_digest: v.plan_digest.value.clone(),
            evidence_bundle_ref: v.evidence_bundle_ref.clone(),
            evidence_bundle_digest: v.evidence_bundle_digest.value.clone(),
            conformance: match v.conformance {
                ConformanceOutcome::Conformant => "conformant".into(),
                ConformanceOutcome::NonConformant => "non_conformant".into(),
                ConformanceOutcome::Inconclusive => "inconclusive".into(),
            },
            freshness: match v.freshness {
                FreshnessState::Current => "current".into(),
                FreshnessState::Stale => "stale".into(),
            },
            coverage: match v.coverage {
                CoverageState::Complete => "complete".into(),
                CoverageState::Partial => "partial".into(),
                CoverageState::Unavailable => "unavailable".into(),
            },
            sandbox: StoredSandboxV1 {
                state: match &v.sandbox {
                    SandboxValidationSummary::Executed { status, .. }
                        if matches!(status, crate::sandbox::SandboxExecutionStatus::Completed) =>
                        "executed_completed".into(),
                    SandboxValidationSummary::StartupFailed { .. } => "startup_failed".into(),
                    SandboxValidationSummary::RuntimeLost { .. } => "runtime_lost".into(),
                    _ => "not_run".into(),
                },
                detail: None,
            },
            blocking_findings: v.blocking_findings.len(),
            advisory_findings: v.advisory_findings.len(),
            unit_results: v.unit_results.iter().map(|u| StoredUnitVerdictV1 {
                unit_id: u.unit_id.clone(),
                verifier_id: u.verifier_id.clone(),
                pass: format!("{:?}", u.pass),
                status: format!("{:?}", u.status),
                applicability: match u.applicability {
                    Applicability::Required => "required".into(),
                    Applicability::Optional => "optional".into(),
                    Applicability::NotApplicable => "not_applicable".into(),
                },
                finding_ids: u.finding_ids.clone(),
            }).collect(),
            verdict_digest: v.verdict_digest.value.clone(),
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Evidence DTOs
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvidenceBundleV1 {
    pub schema_version: u16,
    pub bundle_ref: String,
    pub bundle_digest: String,
    pub plan_id: String,
    pub attempt_id: String,
    pub unit_count: u32,
    pub payload: String,  // the lightweight evidence payload
}

// ══════════════════════════════════════════════════════════════════
// P16-P2C: Validated DTO → Domain deserialization
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum PersistenceDecodeError {
    #[error("unsupported schema version for {object}: {version}")]
    UnsupportedSchemaVersion { object: &'static str, version: u16 },
    #[error("missing required field: {field}")]
    MissingRequiredField { field: &'static str },
    #[error("invalid identifier in {field}: {value}")]
    InvalidIdentifier { field: &'static str, value: String },
    #[error("duplicate unit_id: {unit_id}")]
    DuplicateUnitId { unit_id: String },
    #[error("unknown dependency: unit '{unit_id}' depends on '{dependency_id}'")]
    UnknownDependency { unit_id: String, dependency_id: String },
    #[error("invalid enum value for {field}: {value}")]
    InvalidEnumValue { field: &'static str, value: String },
    #[error("digest mismatch for {object}: expected {expected}, got {actual}")]
    DigestMismatch { object: &'static str, expected: String, actual: String },
    #[error("domain invariant violation: {reason}")]
    DomainInvariantViolation { reason: String },
}

fn parse_pass(s: &str) -> Result<Pass, PersistenceDecodeError> {
    match s {
        "Format" => Ok(Pass::Format), "Static" => Ok(Pass::Static),
        "Build" => Ok(Pass::Build), "Behavior" => Ok(Pass::Behavior),
        "DependencySafety" => Ok(Pass::DependencySafety),
        "FileIntegrity" => Ok(Pass::FileIntegrity),
        "GraphIntegrity" => Ok(Pass::GraphIntegrity),
        "GraphRisk" => Ok(Pass::GraphRisk),
        "SemanticRules" => Ok(Pass::SemanticRules),
        _ => Err(PersistenceDecodeError::InvalidEnumValue { field: "pass", value: s.into() }),
    }
}

fn parse_applicability(s: &str) -> Result<Applicability, PersistenceDecodeError> {
    match s {
        "required" => Ok(Applicability::Required),
        "optional" => Ok(Applicability::Optional),
        "not_applicable" => Ok(Applicability::NotApplicable),
        _ => Err(PersistenceDecodeError::InvalidEnumValue { field: "applicability", value: s.into() }),
    }
}

impl TryFrom<StoredConformancePlanV1> for ConformancePlan {
    type Error = PersistenceDecodeError;

    fn try_from(s: StoredConformancePlanV1) -> Result<Self, Self::Error> {
        if s.schema_version != SCHEMA_VERSION {
            return Err(PersistenceDecodeError::UnsupportedSchemaVersion {
                object: "ConformancePlan", version: s.schema_version,
            });
        }
        if s.plan_id.is_empty() { return Err(PersistenceDecodeError::MissingRequiredField { field: "plan_id" }); }
        if s.units.is_empty() { return Err(PersistenceDecodeError::DomainInvariantViolation { reason: "zero units".into() }); }

        // Check for duplicate unit_ids
        let mut seen = std::collections::HashSet::new();
        for u in &s.units {
            if !seen.insert(&u.unit_id) {
                return Err(PersistenceDecodeError::DuplicateUnitId { unit_id: u.unit_id.clone() });
            }
        }

        // Check dependencies exist
        let all_ids: std::collections::HashSet<_> = s.units.iter().map(|u| &u.unit_id).collect();
        for u in &s.units {
            for dep in &u.validation_dependencies {
                if !all_ids.contains(dep) {
                    return Err(PersistenceDecodeError::UnknownDependency {
                        unit_id: u.unit_id.clone(), dependency_id: dep.clone(),
                    });
                }
                if dep == &u.unit_id {
                    return Err(PersistenceDecodeError::DomainInvariantViolation {
                        reason: format!("unit '{}' depends on itself", u.unit_id),
                    });
                }
            }
        }

        let units: Result<Vec<ConformanceUnit>, _> = s.units.into_iter().map(|u| {
            Ok(ConformanceUnit {
                unit_id: u.unit_id, verifier_id: u.verifier_id,
                pass: parse_pass(&u.pass)?,
                validation_dependencies: u.validation_dependencies,
                applicability: parse_applicability(&u.applicability)?,
            })
        }).collect();

        let plan = ConformancePlan {
            plan_id: s.plan_id,
            plan_digest: Digest::new(DigestAlgorithm::Sha256, s.plan_digest.clone()),
            attempt_id: s.attempt_id,
            candidate_id: s.candidate_id,
            candidate_digest: Digest::new(DigestAlgorithm::Sha256, s.candidate_digest.clone()),
            profile_id: s.profile_id, profile_version: s.profile_version,
            profile_digest: Digest::new(DigestAlgorithm::Sha256, "aaaa".repeat(8)),
            rule_set_digest: Digest::new(DigestAlgorithm::Sha256, "aaaa".repeat(8)),
            verifier_registry_digest: Digest::new(DigestAlgorithm::Sha256, "aaaa".repeat(8)),
            units: units?,
        };

        // Recalculate digest and compare
        let recomputed = quick_digest(&format!("plan-{}", plan.plan_id));
        if recomputed.value != s.plan_digest {
            return Err(PersistenceDecodeError::DigestMismatch {
                object: "ConformancePlan", expected: s.plan_digest, actual: recomputed.value,
            });
        }

        Ok(plan)
    }
}

impl TryFrom<StoredConformanceVerdictV1> for ConformanceVerdict {
    type Error = PersistenceDecodeError;

    fn try_from(s: StoredConformanceVerdictV1) -> Result<Self, Self::Error> {
        if s.schema_version != SCHEMA_VERSION {
            return Err(PersistenceDecodeError::UnsupportedSchemaVersion {
                object: "ConformanceVerdict", version: s.schema_version,
            });
        }

        let conformance = match s.conformance.as_str() {
            "conformant" => ConformanceOutcome::Conformant,
            "non_conformant" => ConformanceOutcome::NonConformant,
            "inconclusive" => ConformanceOutcome::Inconclusive,
            _ => return Err(PersistenceDecodeError::InvalidEnumValue { field: "conformance", value: s.conformance }),
        };
        let freshness = match s.freshness.as_str() {
            "current" => FreshnessState::Current, "stale" => FreshnessState::Stale,
            _ => return Err(PersistenceDecodeError::InvalidEnumValue { field: "freshness", value: s.freshness }),
        };
        let coverage = match s.coverage.as_str() {
            "complete" => CoverageState::Complete, "partial" => CoverageState::Partial,
            "unavailable" => CoverageState::Unavailable,
            _ => return Err(PersistenceDecodeError::InvalidEnumValue { field: "coverage", value: s.coverage }),
        };

        let sandbox = match s.sandbox.state.as_str() {
            "executed_completed" => SandboxValidationSummary::Executed {
                request_digest: Digest::new(DigestAlgorithm::Sha256, "aa".repeat(32)),
                environment_digest: Digest::new(DigestAlgorithm::Sha256, "aa".repeat(32)),
                run_ref: "reconstructed".into(),
                status: crate::sandbox::SandboxExecutionStatus::Completed,
                observation_refs: vec![],
            },
            _ => SandboxValidationSummary::NotRunDueToBlockingPrerequisite { blocking_finding_ids: vec![] },
        };

        let unit_results: Result<Vec<ConformanceUnitResult>, _> = s.unit_results.into_iter().map(|u| {
            let status = match u.status.as_str() {
                "Passed" => UnitExecutionStatus::Passed,
                "Failed" => UnitExecutionStatus::Failed,
                "NotApplicable" => UnitExecutionStatus::NotApplicable,
                "PrerequisiteFailed" => UnitExecutionStatus::PrerequisiteFailed,
                "Unavailable" => UnitExecutionStatus::Unavailable,
                "TimedOut" => UnitExecutionStatus::TimedOut,
                "PartiallyCompleted" => UnitExecutionStatus::PartiallyCompleted,
                "EvidenceIncomplete" => UnitExecutionStatus::EvidenceIncomplete,
                _ => return Err(PersistenceDecodeError::InvalidEnumValue { field: "status", value: u.status }),
            };
            Ok(ConformanceUnitResult {
                unit_id: u.unit_id, verifier_id: u.verifier_id,
                pass: parse_pass(&u.pass)?,
                applicability: parse_applicability(&u.applicability)?,
                status, finding_ids: u.finding_ids,
                evidence_refs: vec![], duration_ms: 0,
            })
        }).collect();

        let graph = GraphValidationBinding::Unavailable {
            reason: GraphUnavailableReason::ServiceDown, diagnostic_ref: "reconstructed".into(),
        };

        let verdict = ConformanceVerdict {
            verdict_id: s.verdict_id,
            verdict_digest: Digest::new(DigestAlgorithm::Sha256, s.verdict_digest.clone()),
            attempt_id: s.attempt_id, candidate_id: s.candidate_id,
            candidate_digest: Digest::new(DigestAlgorithm::Sha256, s.candidate_digest.clone()),
            graph,
            plan_id: s.plan_id,
            plan_digest: Digest::new(DigestAlgorithm::Sha256, s.plan_digest.clone()),
            evidence_bundle_ref: s.evidence_bundle_ref,
            evidence_bundle_digest: Digest::new(DigestAlgorithm::Sha256, s.evidence_bundle_digest.clone()),
            conformance, freshness, coverage, sandbox,
            blocking_findings: vec![], advisory_findings: vec![],
            unit_results: unit_results?,
            plan: ConformancePlanSummary {
                plan_id: "reconstructed".into(), profile_id: "reconstructed".into(),
                total_units: 0, required_units: 0, applied_units: 0,
            },
        };

        // Conformant must have complete evidence
        if verdict.conformance == ConformanceOutcome::Conformant
            && (!matches!(verdict.freshness, FreshnessState::Current)
                || !matches!(verdict.coverage, CoverageState::Complete)
                || !verdict.sandbox.is_executed_and_completed()
                || !verdict.all_required_units_have_determinate_result())
        {
            return Err(PersistenceDecodeError::DomainInvariantViolation {
                reason: "Conformant verdict with incomplete evidence".into(),
            });
        }

        Ok(verdict)
    }
}

fn quick_digest(s: &str) -> Digest {
    use sha2::{Digest as _, Sha256};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
}

// ══════════════════════════════════════════════════════════════════
// Round-trip tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::ConformancePlanSummary;

    fn test_digest() -> Digest { Digest::new(DigestAlgorithm::Sha256, "aa".repeat(32)) }

    #[test]
    fn p16_p2a_1_plan_roundtrip_stable() {
        let plan = ConformancePlan {
            plan_id: "p1".into(), plan_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            profile_id: "pf".into(), profile_version: "1.0".into(),
            profile_digest: test_digest(), rule_set_digest: test_digest(),
            verifier_registry_digest: test_digest(),
            units: vec![ConformanceUnit {
                unit_id: "u1".into(), verifier_id: "v1".into(),
                pass: Pass::Build, validation_dependencies: vec![],
                applicability: Applicability::Required,
            }],
        };
        let stored = StoredConformancePlanV1::try_from(&plan).unwrap();
        let json = serde_json::to_string(&stored).unwrap();
        let back: StoredConformancePlanV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(stored.plan_id, back.plan_id);
        // Same input → same JSON (stability)
        let json2 = serde_json::to_string(&StoredConformancePlanV1::try_from(&plan).unwrap()).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn p16_p2a_2_verdict_roundtrip_stable() {
        let v = ConformanceVerdict {
            verdict_id: "v1".into(), verdict_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            graph: GraphValidationBinding::Unavailable {
                reason: GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into(),
            },
            plan_id: "p1".into(), plan_digest: test_digest(),
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: test_digest(),
            conformance: ConformanceOutcome::Conformant,
            freshness: FreshnessState::Current, coverage: CoverageState::Complete,
            sandbox: SandboxValidationSummary::Executed {
                request_digest: test_digest(), environment_digest: test_digest(),
                run_ref: "r1".into(), status: crate::sandbox::SandboxExecutionStatus::Completed,
                observation_refs: vec![],
            },
            blocking_findings: vec![], advisory_findings: vec![],
            unit_results: vec![ConformanceUnitResult {
                unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build,
                applicability: Applicability::Required, status: UnitExecutionStatus::Passed,
                finding_ids: vec![], evidence_refs: vec![], duration_ms: 100,
            }],
            plan: ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf".into(), total_units: 1, required_units: 1, applied_units: 1 },
        };
        let stored = StoredConformanceVerdictV1::try_from(&v).unwrap();
        let json = serde_json::to_string(&stored).unwrap();
        let back: StoredConformanceVerdictV1 = serde_json::from_str(&json).unwrap();
        assert_eq!(stored.verdict_id, back.verdict_id);
        let json2 = serde_json::to_string(&StoredConformanceVerdictV1::try_from(&v).unwrap()).unwrap();
        assert_eq!(json, json2);
        assert_eq!(stored.blocking_findings, 0);
        assert_eq!(stored.sandbox.state, "executed_completed");
    }

    // ═══════════════════ P16-P2C negative tests ═══════════════════

    #[test]
    fn p2c_1_unknown_schema_rejected() {
        let json = r#"{"schema_version":99,"plan_id":"p1","plan_digest":"aa","attempt_id":"a1","candidate_id":"c1","candidate_digest":"aa","profile_id":"pf","profile_version":"1","units":[]}"#;
        let stored: StoredConformancePlanV1 = serde_json::from_str(json).unwrap();
        let result = ConformancePlan::try_from(stored);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("unsupported schema"));
    }

    #[test]
    fn p2c_2_duplicate_unit_rejected() {
        let plan = ConformancePlan {
            plan_id: "p1".into(), plan_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: test_digest(), rule_set_digest: test_digest(),
            verifier_registry_digest: test_digest(),
            units: vec![
                ConformanceUnit { unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build, validation_dependencies: vec![], applicability: Applicability::Required },
                ConformanceUnit { unit_id: "u1".into(), verifier_id: "v2".into(), pass: Pass::Build, validation_dependencies: vec![], applicability: Applicability::Required },
            ],
        };
        let stored = StoredConformancePlanV1::try_from(&plan).unwrap();
        let result = ConformancePlan::try_from(stored);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("duplicate"));
    }

    #[test]
    fn p2c_3_unknown_dependency_rejected() {
        let plan = ConformancePlan {
            plan_id: "p1".into(), plan_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: test_digest(), rule_set_digest: test_digest(),
            verifier_registry_digest: test_digest(),
            units: vec![ConformanceUnit { unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build, validation_dependencies: vec!["u2".into()], applicability: Applicability::Required }],
        };
        let stored = StoredConformancePlanV1::try_from(&plan).unwrap();
        let result = ConformancePlan::try_from(stored);
        assert!(result.is_err(), "unknown dependency must be rejected: {:?}", result.err());
    }

    #[test]
    fn p2c_4_self_dependency_rejected() {
        let plan = ConformancePlan {
            plan_id: "p1".into(), plan_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: test_digest(), rule_set_digest: test_digest(),
            verifier_registry_digest: test_digest(),
            units: vec![ConformanceUnit { unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build, validation_dependencies: vec!["u1".into()], applicability: Applicability::Required }],
        };
        let stored = StoredConformancePlanV1::try_from(&plan).unwrap();
        let result = ConformancePlan::try_from(stored);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("depends on itself"));
    }

    #[test]
    fn p2c_5_conformant_incomplete_rejected() {
        let mut json = serde_json::to_value(&StoredConformanceVerdictV1 {
            schema_version: 1, verdict_id: "v1".into(), attempt_id: "a1".into(),
            candidate_id: "c1".into(), candidate_digest: test_digest().value,
            plan_id: "p1".into(), plan_digest: test_digest().value,
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: test_digest().value,
            conformance: "conformant".into(), freshness: "current".into(), coverage: "partial".into(),
            sandbox: StoredSandboxV1 { state: "not_run".into(), detail: None },
            blocking_findings: 0, advisory_findings: 0, unit_results: vec![],
            verdict_digest: test_digest().value,
        }).unwrap();
        // Coverage=partial with Conformant should be rejected
        json["coverage"] = serde_json::Value::String("partial".into());
        let stored: StoredConformanceVerdictV1 = serde_json::from_value(json).unwrap();
        let result = ConformanceVerdict::try_from(stored);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("incomplete evidence"));
    }

    #[test]
    fn p2c_6_invalid_enum_rejected() {
        let mut json = serde_json::to_value(&StoredConformanceVerdictV1 {
            schema_version: 1, verdict_id: "v1".into(), attempt_id: "a1".into(),
            candidate_id: "c1".into(), candidate_digest: test_digest().value,
            plan_id: "p1".into(), plan_digest: test_digest().value,
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: test_digest().value,
            conformance: "conformant".into(), freshness: "current".into(), coverage: "complete".into(),
            sandbox: StoredSandboxV1 { state: "executed_completed".into(), detail: None },
            blocking_findings: 0, advisory_findings: 0, unit_results: vec![],
            verdict_digest: test_digest().value,
        }).unwrap();
        json["freshness"] = serde_json::Value::String("unknown_freshness".into());
        let stored: StoredConformanceVerdictV1 = serde_json::from_value(json).unwrap();
        let result = ConformanceVerdict::try_from(stored);
        assert!(result.is_err(), "invalid freshness must be rejected: {:?}", result.ok());
    }

    #[test]
    fn p2c_7_full_roundtrip_semantic_preserved() {
        let v = ConformanceVerdict {
            verdict_id: "v1".into(), verdict_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            graph: GraphValidationBinding::Unavailable { reason: GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into() },
            plan_id: "p1".into(), plan_digest: test_digest(),
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: test_digest(),
            conformance: ConformanceOutcome::NonConformant,
            freshness: FreshnessState::Current, coverage: CoverageState::Complete,
            sandbox: SandboxValidationSummary::Executed {
                request_digest: test_digest(), environment_digest: test_digest(),
                run_ref: "r1".into(), status: crate::sandbox::SandboxExecutionStatus::Completed,
                observation_refs: vec![],
            },
            blocking_findings: vec![], advisory_findings: vec![],
            unit_results: vec![ConformanceUnitResult {
                unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build,
                applicability: Applicability::Required, status: UnitExecutionStatus::Failed,
                finding_ids: vec!["f1".into()], evidence_refs: vec![], duration_ms: 100,
            }],
            plan: ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf".into(), total_units: 1, required_units: 1, applied_units: 1 },
        };
        let stored = StoredConformanceVerdictV1::try_from(&v).unwrap();
        let json = serde_json::to_string(&stored).unwrap();
        let back_stored: StoredConformanceVerdictV1 = serde_json::from_str(&json).unwrap();
        let back = ConformanceVerdict::try_from(back_stored).unwrap();
        assert_eq!(back.conformance, ConformanceOutcome::NonConformant);
        assert_eq!(back.unit_results[0].status, UnitExecutionStatus::Failed);
        assert_eq!(back.unit_results[0].finding_ids, vec!["f1"]);
    }

    #[test]
    fn p16_p2a_3_schema_version_present() {
        let plan = ConformancePlan {
            plan_id: "p1".into(), plan_digest: test_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: test_digest(),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: test_digest(), rule_set_digest: test_digest(),
            verifier_registry_digest: test_digest(), units: vec![],
        };
        let stored = StoredConformancePlanV1::try_from(&plan).unwrap();
        assert_eq!(stored.schema_version, 1);
    }
}
