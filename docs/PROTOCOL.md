# OntoFlow Cross-Language Protocol — v1 (FROZEN)

> G0 Protocol Freeze. Rust and Go MUST produce identical values for all hashes.

---

## Protocol Version: 1

Schema base: `https://ontoos.dev/schemas/v1/`

## Rust Side

Status: **FROZEN**. Only allowed changes:
- Protocol-compatibility bug fixes
- Real-integration bug fixes
- Security fixes

No new features. No field additions without schema version bump.

## Four Message Types

| Type | Direction | Schema |
|------|-----------|--------|
| `LoopInvocationRequest` | Go → Rust | `schemas/v1/loop_invocation_request.schema.json` |
| `LoopTerminalEnvelope` | Rust → Go | `schemas/v1/loop_terminal_envelope.schema.json` |
| `OntoLoopHeartbeat` | Rust → OntoFlow | `schemas/v1/onto_loop_heartbeat.schema.json` |
| `VerifiedLoopOutcome` | Rust → Go (gRPC) | `schemas/v1/verified_loop_outcome.schema.json` |

## Canonical Hash Specification

### request_binding_hash

Input fields (in order):
```
flow_id + ":" + work_item_id + ":" + loop_id + ":" +
task_spec_ref + ":" + contract_ref + ":" + policy_ref + ":" +
input_artifact_refs.join(",") + ":" +
budget_grant_ref + ":" + budget_grant_hash + ":" +
execution_generation
```
Algorithm: DefaultHasher(SipHash-1-3), hex-encoded 16 chars.

### outcome_binding_hash

Input fields (in order):
```
request_binding_hash + ":" +
reported_terminal_state (snake_case) + ":" +
decision_id | "" + ":" +
decision_hash | "" + ":" +
output_checkpoint_hash | "" + ":" +
settlement_receipt_ref | "" + ":" +
output_artifact_refs.join(",") + ":" +
execution_generation + ":" + total_attempts + ":" + schema_version
```
Algorithm: DefaultHasher(SipHash-1-3), hex-encoded 16 chars.

### authority_binding_hash

Input fields (in order):
```
loop_id + ":" + decision_id + ":" + decision_hash + ":" +
output_checkpoint_hash + ":" + settlement_receipt_ref | ""
```
Algorithm: DefaultHasher(SipHash-1-3), hex-encoded 16 chars.

## Golden Vectors

Reference implementation: `crates/onto-temporal-adapter/tests/golden_vectors.rs`

Go implementation MUST produce identical hashes for the same inputs. See test file for the exact JSON shapes.

## Key Invariants

1. `request_binding_hash` is computed by Go BEFORE sending; Rust verifies unchanged
2. `outcome_binding_hash` binds `request_binding_hash` + all outcome data
3. `reported_terminal_state` is Worker's CLAIM — Go verifies via `AuthorityProjectionPort`
4. `ActivityTaskCompleted` is NOT `Committed` — Go MUST call authority before accepting
5. `LoopBudgetExhausted` ≠ `FlowBudgetExhausted` — Go owns Flow budget
6. Heartbeat does NOT participate in success determination

## Protocol Evolution

- v1: Current. JSON over OntoFlow ActivityTask.
- v2 (future): Protobuf native AgentWorkItemTask.
- Schema version bump required for any incompatible change.
