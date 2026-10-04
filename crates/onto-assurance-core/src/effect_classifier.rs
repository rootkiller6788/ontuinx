//! EffectClassifier — trusted classification of capability side effects.
//!
//! This is the ONLY path to determine EffectClass.  The Agent or LLM
//! cannot set or downgrade the classification.  Unknown → Irreversible
//! (fail-closed).
//!
//! Pure deterministic logic — no I/O, no OntoRuntime types.

use onto_assurance_types::contract::EffectClassification;
use onto_assurance_types::enums::{EffectClass, RiskLevel};

// ══════════════════════════════════════════════════════════════════
// EffectClassifier trait
// ══════════════════════════════════════════════════════════════════

/// Trusted classifier.  Runs at the kernel boundary, NOT in the Agent loop.
///
/// The classifier receives capability metadata (name, kind, arguments) and
/// produces an `EffectClassification`.  The Agent CANNOT override this.
///
/// ## Extensibility
///
/// This is a TRAIT — every Industry Pack can supply its own implementation.
/// `RulesBasedClassifier` is the built-in default.  Industry packs should
/// implement this trait to provide domain-specific classification:
///
/// ```ignore
/// struct OpsPackClassifier;
/// impl EffectClassifier for OpsPackClassifier {
///     fn classify(&self, kind: &str, args: &str, declared: Option<EffectClass>,
///                 risk: RiskLevel) -> EffectClassification {
///         // Domain-specific logic: terraform_apply → Irreversible;
///         // ansible_playbook → Compensatable; etc.
///     }
/// }
/// ```
///
/// The trait is injected at composition time:
/// `TransactionCoordinator::new(auth, approval, ..., Box::new(OpsPackClassifier))`
///
/// ## Safety
///
/// The `EffectClassification` returned by this trait is TRUSTED.  It MUST be
/// implemented with fail-closed semantics: unknown capability → Irreversible.
/// Never return `Pure` or `ReadOnly` for an unrecognized capability.
pub trait EffectClassifier: Send + Sync {
    /// Classify a capability invocation.
    ///
    /// `capability_kind` is a string like "file_write", "http_post", "sql_execute".
    /// `arguments_hint` provides argument-level signals (e.g. SQL text that
    /// can be parsed for DROP TABLE detection).
    fn classify(
        &self,
        capability_kind: &str,
        arguments_hint: &str,
        declared_class: Option<EffectClass>,
        risk_level: RiskLevel,
    ) -> EffectClassification;

    /// Validate that the declared EffectClass is consistent with the
    /// classifier's own assessment.  The declared class can only be
    /// upgraded, never downgraded.
    fn validate_contract_class(
        &self,
        declared_class: Option<EffectClass>,
        capability_kind: &str,
        arguments_hint: &str,
        risk_level: RiskLevel,
    ) -> EffectClassification {
        self.classify(capability_kind, arguments_hint, declared_class, risk_level)
    }
}

// ══════════════════════════════════════════════════════════════════
// RulesBasedClassifier — deterministic, no external data
// ══════════════════════════════════════════════════════════════════

/// A deterministic, rules-based EffectClassifier.
///
/// Classification priority:
///   1. Known destructive patterns → Irreversible or Deny
///   2. Known staged patterns → Staged
///   3. Known transactional patterns → Transactional
///   4. Known readonly patterns → ReadOnly
///   5. Unknown → Irreversible (fail-closed)
pub struct RulesBasedClassifier;

impl RulesBasedClassifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RulesBasedClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl EffectClassifier for RulesBasedClassifier {
    fn classify(
        &self,
        capability_kind: &str,
        arguments_hint: &str,
        declared_class: Option<EffectClass>,
        risk_level: RiskLevel,
    ) -> EffectClassification {
        let assessed = classify_by_rules(capability_kind, arguments_hint);

        // Policy escalation: risk level can upgrade the class
        let escalated = escalate_by_risk(assessed, risk_level);

        // The final class is the more restrictive of assessed, escalated, and declared.
        // declared_class can only be UPGRADED, never downgraded.
        let mut final_class = more_restrictive(assessed, escalated);
        let upgraded_from = match declared_class {
            Some(declared) => {
                let after_declared = more_restrictive(final_class, declared);
                let was_upgraded = after_declared != declared;
                final_class = after_declared;
                if was_upgraded { Some(declared) } else { None }
            }
            None => {
                if final_class != assessed { Some(assessed) } else { None }
            }
        };

        EffectClassification {
            effect_class: final_class,
            rationale: format!(
                "assessed={:?} escalated={:?} declared={:?} → {:?}",
                assessed, escalated, declared_class, final_class
            ),
            upgraded_from,
            requires_approval: requires_approval(final_class, risk_level),
            pre_verification_required: final_class == EffectClass::Irreversible,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Classification rules (LOCKED — cross-language)
// ══════════════════════════════════════════════════════════════════

fn classify_by_rules(kind: &str, args: &str) -> EffectClass {
    let kind_lower = kind.to_lowercase();
    let args_lower = args.to_lowercase();

    // ── Irreversible patterns ──
    if contains_any(&kind_lower, &["deploy", "publish", "release", "send_email", "send_message",
        "payment", "charge", "refund", "delete_cluster", "delete_bucket", "drop_database",
        "terminate", "shutdown", "reboot", "actuate", "control_robot"])
    {
        return EffectClass::Irreversible;
    }

    // ── Destructive SQL → Irreversible unless wrapped in transaction ──
    if kind_lower.contains("sql") || kind_lower.contains("database") {
        if contains_any(&args_lower, &["drop table", "drop database", "drop schema",
            "truncate", "delete from", "alter table"])
            && !args_lower.contains("begin")
        {
            return EffectClass::Irreversible;
        }
        if args_lower.contains("begin") || args_lower.contains("transaction") {
            return EffectClass::Transactional;
        }
        return EffectClass::Transactional;
    }

    // ── Staged patterns ──
    if contains_any(&kind_lower, &["file_write", "file_edit", "file_create", "file_delete",
        "write_file", "edit_file", "create_file", "apply_patch", "git_commit",
        "write", "edit", "patch"])
    {
        return EffectClass::Staged;
    }

    // ── ReadOnly patterns ──
    if contains_any(&kind_lower, &["file_read", "read_file", "glob", "grep", "search",
        "list", "ls", "cat", "read", "get", "fetch", "query", "select"])
    {
        return EffectClass::ReadOnly;
    }

    // ── Compensatable patterns ──
    if contains_any(&kind_lower, &["create_resource", "provision", "allocate",
        "create_server", "create_instance", "create_bucket"])
    {
        return EffectClass::Compensatable;
    }

    // ── Default: Pure (no known side effects) ──
    if contains_any(&kind_lower, &["echo", "ping", "health", "status", "version", "help"]) {
        return EffectClass::Pure;
    }

    // ── Unknown → fail-closed as Irreversible ──
    EffectClass::Irreversible
}

fn escalate_by_risk(class: EffectClass, risk: RiskLevel) -> EffectClass {
    match risk {
        RiskLevel::Low => class,
        RiskLevel::Medium => class,
        RiskLevel::High => {
            // High risk: Staged → Irreversible, Transactional stays, ReadOnly stays
            match class {
                EffectClass::Staged => EffectClass::Compensatable,
                EffectClass::Compensatable => EffectClass::Irreversible,
                other => other,
            }
        }
        RiskLevel::Critical => EffectClass::Irreversible,
    }
}

fn requires_approval(class: EffectClass, risk: RiskLevel) -> bool {
    match class {
        EffectClass::Irreversible => true,
        EffectClass::Compensatable => risk >= RiskLevel::High,
        _ => risk >= RiskLevel::Critical,
    }
}

fn more_restrictive(a: EffectClass, b: EffectClass) -> EffectClass {
    // Higher index = more restrictive
    fn rank(c: EffectClass) -> u8 {
        match c {
            EffectClass::Pure => 0,
            EffectClass::ReadOnly => 1,
            EffectClass::Staged => 2,
            EffectClass::Transactional => 3,
            EffectClass::Compensatable => 4,
            EffectClass::Irreversible => 5,
        }
    }
    if rank(a) >= rank(b) { a } else { b }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| {
        // For multi-word patterns (containing spaces), use substring match
        // (SQL statements like "DROP TABLE" are unambiguous enough).
        if n.contains(' ') {
            return haystack.contains(n);
        }
        // For single-word patterns, use word-boundary-aware matching
        // to avoid false positives (e.g., "read" matching "thread").
        haystack == *n
            || haystack.starts_with(&format!("{}_", n))
            || haystack.ends_with(&format!("_{}", n))
            || haystack.contains(&format!("_{}_", n))
            || haystack.starts_with(&format!("{} ", n))
            || haystack.contains(&format!(" {} ", n))
            || haystack.ends_with(&format!(" {}", n))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_write_is_staged() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("file_write", "", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Staged);
        assert!(!result.requires_approval);
    }

    #[test]
    fn read_is_readonly() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("file_read", "", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::ReadOnly);
    }

    #[test]
    fn deploy_is_irreversible() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("deploy", "", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Irreversible);
        assert!(result.requires_approval);
        assert!(result.pre_verification_required);
    }

    #[test]
    fn sql_with_begin_is_transactional() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("sql_execute", "BEGIN; UPDATE users SET ...", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Transactional);
    }

    #[test]
    fn drop_table_without_transaction_is_irreversible() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("sql_execute", "DROP TABLE users", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Irreversible);
    }

    #[test]
    fn unknown_capability_is_irreversible() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("some_new_tool_v42", "", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Irreversible);
    }

    #[test]
    fn declared_class_cannot_downgrade() {
        let c = RulesBasedClassifier::new();
        // Classifier says Staged, but someone declared Pure → should stay Staged
        let result = c.classify("file_write", "", Some(EffectClass::Pure), RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Staged);
        assert_eq!(result.upgraded_from, Some(EffectClass::Pure));
    }

    #[test]
    fn critical_risk_escalates_to_irreversible() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("file_write", "", None, RiskLevel::Critical);
        assert_eq!(result.effect_class, EffectClass::Irreversible);
    }

    #[test]
    fn more_restrictive_ordering() {
        assert_eq!(
            more_restrictive(EffectClass::Pure, EffectClass::Irreversible),
            EffectClass::Irreversible
        );
        assert_eq!(
            more_restrictive(EffectClass::Staged, EffectClass::ReadOnly),
            EffectClass::Staged
        );
    }

    #[test]
    fn send_email_is_irreversible() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("send_email", "to: user@example.com", None, RiskLevel::High);
        assert_eq!(result.effect_class, EffectClass::Irreversible);
        assert!(result.requires_approval);
    }

    #[test]
    fn rewrite_not_matched_as_write() {
        // "rewrite" contains "write" as substring but isn't "write" — word boundary check
        let c = RulesBasedClassifier::new();
        let result = c.classify("rewrite", "", None, RiskLevel::Medium);
        // "rewrite" is unknown → Irreversible (fail-closed), NOT Staged
        assert_eq!(result.effect_class, EffectClass::Irreversible);
    }

    #[test]
    fn thread_not_matched_as_read() {
        // "thread" contains "read" as substring but isn't a read operation
        let c = RulesBasedClassifier::new();
        let result = c.classify("thread", "", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Irreversible);
    }

    #[test]
    fn file_write_underscore_is_staged() {
        let c = RulesBasedClassifier::new();
        let result = c.classify("file_write", "", None, RiskLevel::Medium);
        assert_eq!(result.effect_class, EffectClass::Staged);
    }
}
