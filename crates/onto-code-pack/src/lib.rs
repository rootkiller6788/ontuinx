//! Onto Code Assurance Pack — software project verifiers.
//!
//! Provides build verification, test execution, and artifact collection
//! for common programming language toolchains.  Implements `PackVerifier`
//! from `onto-pack-sdk`.

pub mod build_verifier;
pub mod test_verifier;
pub mod lint_verifier;
pub mod machine_verifiers;   // ★ 7语言 × 4层 机器验证矩阵
pub mod artifact_collector;
pub mod project_profiler;
pub mod file_verifiers;
pub mod scope;
pub mod rules;
pub mod verifiers;
pub mod domain_pack;
pub mod location;
pub mod verification_profiles;
pub mod pg_stores;
pub mod artifact_store;
pub mod observability;
pub mod workflow_pack;
pub mod document_pack;
pub mod data_pack;

/// VerifierCapability — 声明一个 Verifier 能处理哪些语言和规则集。
/// Phase 2: 类型定义。Phase 3 会有 registry 和实际实现。
#[derive(Debug, Clone)]
pub struct VerifierCapability {
    pub verifier_id: String,
    pub verifier_name: String,
    pub is_deterministic: bool,
    pub languages: Vec<String>,
    pub rule_sets: Vec<String>,
}
