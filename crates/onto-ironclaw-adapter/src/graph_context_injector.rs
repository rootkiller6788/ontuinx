//! GraphContextInjector — P9
//!
//! Deterministic context injection (NOT Agent decision).
//! Injects limited graph context into Agent's session before execution.

use crate::graph_read_capability::RepositoryGraphReadCapability;
use std::sync::Arc;

const MAX_CONTEXT_TOKENS: usize = 500;
const MAX_CALLERS: usize = 10;
const MAX_TESTS: usize = 5;
const MAX_HISTORY_FAILURES: usize = 3;

/// Result of context injection — ready to append to Agent prompt.
#[derive(Debug, Clone)]
pub struct InjectedContext {
    pub injected: bool,
    pub context_text: String,
    pub token_estimate: usize,
}

/// Injection trigger conditions.
#[derive(Debug, Clone)]
pub enum InjectionTrigger {
    /// First time modifying an unfamiliar module.
    FirstTouchUnfamiliar { entity_key: String },
    /// Modifying a public API.
    PublicApiChange { entity_keys: Vec<String> },
    /// Modifying a high-risk file.
    HighRiskFile { file_path: String },
    /// Consecutive verification failures.
    ConsecutiveFailures { attempt_number: u32, failure_count: u32 },
}

/// Determines what context to inject.
pub struct GraphContextInjector {
    capability: Arc<RepositoryGraphReadCapability>,
    default_repo: String,
}

impl GraphContextInjector {
    pub fn new(capability: Arc<RepositoryGraphReadCapability>, default_repo: String) -> Self {
        Self { capability, default_repo }
    }

    /// Evaluate trigger and return injected context.
    /// Returns empty InjectedContext if no injection is needed.
    pub async fn evaluate(
        &self,
        trigger: &InjectionTrigger,
    ) -> InjectedContext {
        match trigger {
            InjectionTrigger::FirstTouchUnfamiliar { entity_key } => {
                self.inject_symbol_with_neighbors(entity_key, 5).await
            }
            InjectionTrigger::PublicApiChange { entity_keys } => {
                self.inject_api_callers(entity_keys, MAX_CALLERS).await
            }
            InjectionTrigger::HighRiskFile { file_path } => {
                self.inject_impact_summary(file_path).await
            }
            InjectionTrigger::ConsecutiveFailures { failure_count, .. } => {
                self.inject_failure_context(*failure_count).await
            }
        }
    }

    /// First touch: inject target symbol + 1 layer of neighbors.
    async fn inject_symbol_with_neighbors(&self, entity_key: &str, max_neighbors: usize) -> InjectedContext {
        let ctx = self.capability.get_symbol_context(&self.default_repo, entity_key).await;
        if !ctx.success {
            return InjectedContext { injected: false, context_text: String::new(), token_estimate: 0 };
        }

        let callers = ctx.data.get("callers").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        let callees = ctx.data.get("callees").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);

        let text = format!(
            "[OntoCodeGraph] First modification of '{entity_key}' — {} caller(s), {} callee(s).",
            callers.min(max_neighbors), callees.min(max_neighbors)
        );
        let tokens = text.len() / 4; // rough estimate
        InjectedContext { injected: true, context_text: truncate(text, MAX_CONTEXT_TOKENS), token_estimate: tokens }
    }

    /// Public API change: list direct callers.
    async fn inject_api_callers(&self, entity_keys: &[String], max_callers: usize) -> InjectedContext {
        let mut all_callers: Vec<String> = Vec::new();
        for key in entity_keys.iter().take(3) {
            // limit: at most 3 API symbols
            let ctx = self.capability.get_callers(&self.default_repo, key).await;
            if ctx.success {
                if let Some(arr) = ctx.data.get("symbols").and_then(|v| v.as_array()) {
                    for s in arr.iter().take(max_callers) {
                        if let Some(name) = s.get("qualified_name").and_then(|v| v.as_str()) {
                            all_callers.push(name.to_string());
                        }
                    }
                }
            }
        }

        if all_callers.is_empty() {
            return InjectedContext { injected: false, context_text: String::new(), token_estimate: 0 };
        }

        let text = format!(
            "[OntoCodeGraph] Public API change: {} direct caller(s) affected — {}",
            all_callers.len(),
            all_callers.iter().take(max_callers).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
        let tokens = text.len() / 4;
        InjectedContext { injected: true, context_text: truncate(text, MAX_CONTEXT_TOKENS), token_estimate: tokens }
    }

    /// High-risk file: impact summary.
    async fn inject_impact_summary(&self, file_path: &str) -> InjectedContext {
        let ctx = self.capability.get_previous_changes(&self.default_repo, file_path).await;
        let change_count = ctx.data.get("changes").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);

        let text = format!(
            "[OntoCodeGraph] High-risk file '{file_path}' — {} previous change(s) in recorded history.",
            change_count
        );
        let tokens = text.len() / 4;
        InjectedContext { injected: true, context_text: truncate(text, MAX_CONTEXT_TOKENS), token_estimate: tokens }
    }

    /// Consecutive failures: related tests + history.
    async fn inject_failure_context(&self, failure_count: u32) -> InjectedContext {
        let text = format!(
            "[OntoCodeGraph] {} consecutive verification failure(s). Consider reviewing related tests and previous failure episodes.",
            failure_count
        );
        let tokens = text.len() / 4;
        InjectedContext { injected: true, context_text: text, token_estimate: tokens }
    }
}

/// Truncate text to roughly max_tokens (4 chars ≈ 1 token).
fn truncate(text: String, max_tokens: usize) -> String {
    let max_chars = max_tokens * 4;
    if text.len() <= max_chars { text }
    else { format!("{}...", &text[..max_chars - 3]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_respects_limit() {
        let long = "a".repeat(2500);
        let truncated = truncate(long, 500);
        assert!(truncated.len() <= 2000); // 500 tokens * 4 chars
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_short_text_not_truncated() {
        let short = "hello".to_string();
        assert_eq!(truncate(short.clone(), 500), short);
    }

    #[tokio::test]
    async fn test_injector_returns_empty_when_graph_down() {
        let cap = Arc::new(RepositoryGraphReadCapability::new("localhost:50051".into()));
        let injector = GraphContextInjector::new(cap, "test-repo".into());
        let result = injector.evaluate(
            &InjectionTrigger::FirstTouchUnfamiliar { entity_key: "main".into() }
        ).await;
        // When graph is down, injection should be empty — never block agent
        assert!(!result.injected);
    }
}
