# Phase 3+4 Known Limitations

## Verifier Scope

- V1–V3 (FileExists, FileContent, ProtectedPath) are deterministic, filesystem-based verifiers
- V4 (CommandVerifier: pytest, cargo test, etc.) not yet implemented
- All verifiers operate on a frozen staging snapshot

## Runtime Coverage

- BeforeCapability/AfterCapability hooks are registered and fire when capability invoked
- Tool call success is prompt-dependent (DeepSeek v4-flash may or may not call tools for a given prompt)
- Live LLM E2E requires sandbox configuration for full tool execution chain

## Not Yet Proven

- Real CommandVerifier (pytest/cargo test/npm test) integration
- Verifier timeout/crash handling in the runtime path
- Verifier dependency missing → ENVIRONMENT_ERROR (tested in golden scenario, not runtime path)
- Cross-verifier evidence deduplication

## Deliberately Excluded

- M6-B Database Effect
- M6-C Compensatable Effect
- M6-D Irreversible Effect
- OntoLoop multi-attempt cycle
- OntoFlow orchestration
- Multi-tenant verifier isolation
