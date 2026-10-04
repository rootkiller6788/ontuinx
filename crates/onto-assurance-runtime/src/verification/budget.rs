use onto_assurance_types::verification_budget::VerificationBudget;
pub struct BudgetTracker { budget: VerificationBudget }
impl BudgetTracker { pub fn new(b: VerificationBudget) -> Self { Self { budget: b } } pub fn can_proceed(&self) -> bool { !self.budget.any_exhausted() } pub fn consume_call(&mut self) { self.budget.record_call(); } pub fn consume_tokens(&mut self, t: u64) { self.budget.add_tokens(t); } pub fn tokens_remaining(&self) -> u64 { self.budget.max_tokens.saturating_sub(self.budget.tokens_used) } pub fn calls_remaining(&self) -> u32 { self.budget.max_verifier_calls.saturating_sub(self.budget.verifier_calls_used) } pub fn budget(&self) -> &VerificationBudget { &self.budget } }
#[cfg(test)] mod tests { use super::*;
    #[test] fn tracks() { let mut t = BudgetTracker::new(VerificationBudget::new(1000, 3600)); assert!(t.can_proceed()); t.consume_call(); t.consume_tokens(500); assert_eq!(t.calls_remaining(), 99); assert_eq!(t.tokens_remaining(), 500); }
    #[test] fn exhausted() { let t = BudgetTracker::new(VerificationBudget{max_tokens:100,tokens_used:100,max_duration_seconds:60,max_verifier_calls:1,verifier_calls_used:1,max_parallel_units:1}); assert!(!t.can_proceed()); }
}
