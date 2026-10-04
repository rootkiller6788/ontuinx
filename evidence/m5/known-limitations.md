# M5 Known Limitations

## Not Yet Proven

1. **OntoRuntime Authority Test (automated)**: Binary smoke test shows authority enforcement works, but there is no automated regression test in the OntoRuntime executor crate verifying that `loop_exit_applier.apply()` is skipped when Onto escalates. The behavior is observable in logs but not yet part of the executor's test suite.

2. **Missing Finalizer = Fail-Fast**: When `onto-assurance` feature is enabled but `run_finalizer = None`, the system should fail-fast at startup rather than silently falling back to OntoRuntime's original completion logic. This guard is not yet implemented.

3. **Real Evidence Pipeline**: The finalizer currently uses in-memory evidence stores pre-loaded by tests. The production path (loading evidence from real Verifier runs, Artifact manifests, and Checkpoint bindings) is not yet proven in an automated test.

4. **Concurrent Run Isolation**: Idempotency is tested sequentially. Concurrent finalization of the same run from multiple executors is not tested.

5. **Crash Recovery**: If the process crashes between `record_runner_failure` and persistent state update, the run may be stranded. Full crash recovery is M6/M7 scope.

## Deliberately Excluded from M5

- M6 side-effect gating (PreExecutionDecision, SettlementDecision enforcement)
- Industry packs beyond Code Pack
- HTTP product entry point
- PostgreSQL persistent stores
- Distributed/OntoFlow orchestration
- Multi-tenant isolation
- Audit trail completeness
