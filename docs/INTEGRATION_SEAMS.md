# Integration Seams

## 已完成 (M5 + M6-A)

### M5: FinalizationAuthorityPort

```
Seam: RebornTurnRunExecutor::apply_exit()
      → run_finalizer: Option<Arc<dyn RunFinalizationPort>>
```

```
OntoRuntime Agent Loop Exit
    ↓
RebornTurnRunExecutor::apply_exit()
    ↓
RunFinalizationPort::finalize()
    ↓
OntoAssure Kernel (reduction → session_decision → settlement)
    ↓
Outcome logged + authority enforcement
    ↓
if Escalated → record_runner_failure, skip loop_exit_applier
if Committed → proceed to loop_exit_applier.apply()
```

Files:
- `crates/onto-ironclaw-adapter/src/finalizer.rs` — OntoKernelFinalizer
- `ironclaw-main/crates/ironclaw_runner/src/turn_run_executor.rs:437-500` — authority enforcement
- `ironclaw-main/crates/ironclaw_reborn_composition/src/onto_assurance.rs` — composition bridge

### M6-A: StagedSettlementCoordinator

```
Seam: StagedFilesystemPort
      → StagedSettlementCoordinator
      → LocalVersionedFilesystemAdapter
```

```
持久化 Success + Commit
    ↓
StagedSettlementCoordinator::authorize_publish()
    ↓ CommitPermit::issue()
    ↓
CAS: DECIDED → PUBLISHING
    ↓
LocalVersionedFilesystemAdapter::publish(permit)
    ↓
PublishReceiptStore::store_receipt()
    ↓
CAS: PUBLISHING → PUBLISHED → FINALIZED
    ↓
RunStateStore::set_lifecycle(Committed)
```

Files:
- `crates/onto-assurance-runtime/src/staged_settlement.rs` — coordinator
- `crates/onto-assurance-runtime/src/ports.rs` — CommitPermit, TransactionStorePort
- `crates/onto-ironclaw-adapter/src/staged_filesystem.rs` — adapter
- `crates/onto-ironclaw-adapter/src/staged_reconciler.rs` — crash recovery

---

## 待实现

### Phase 2: RunIngressPort

```
Seam: 统一 Run 入口
      Conversation / CLI / HTTP / DirectRun → RunIngressPort
```

```
RunIngressPort::start_run(StartRunRequest { actor, source, input })
    ↓
OntoRuntime 现有 Run 创建逻辑
```

### Phase 3: CapabilityInvocationEnvelope

```
Seam: CapabilityHost 执行边界
      所有 Capability 调用附带统一 Envelope
```

```
CapabilityInvocationEnvelope {
    invocation_id, run_id, actor, capability_id,
    arguments_hash, resource_refs, effect_class
}
    ↓
CapabilityHost → Authorization → Trust → Approval → Lane
```

### Phase 4: RuntimeObservation

```
Seam: CapabilityHost → AfterCapability Observer → RuntimeObservation
```

```
RuntimeObservation → OntoRuntime EventStore (保存引用)
                   → OntoAssure Verifier (生成 Evidence)
```

### Phase 5: DatabaseSettlementCoordinator (M6-B)

```
Seam: TransactionalDatabasePort
      → DatabaseSettlementCoordinator
```

```
Decision + Commit + Transactional
    ↓
DatabaseSettlementCoordinator
    ↓ DatabaseCommitPermit
    ↓
BEGIN → execute → verify → COMMIT → Receipt
```

### Phase 5: ExternalEffectCoordinator (M6-C/D)

```
Seam: External API → CompensationCoordinator / AtMostOnceDispatcher
```

```
PreExecutionDecision
    ↓
Execute → Receipt → Verify
    ↓
Confirm / Compensate / UnknownExternalOutcome
```

### Phase 6: OntoLoop

```
Seam: RuntimeRunPort + AssuranceResultPort + ContinuationIngressPort
```

```
OntoLoop → RunIngressPort → OntoRuntime Run → OntoAssure → OntoLoop
```

### Phase 7: OntoFlow

```
Seam: DurableOrchestrationPort
```

```
OntoFlow WorkItem → OntoLoop Attempt → OntoRuntime Run → OntoFlow 下一 WorkItem
```
