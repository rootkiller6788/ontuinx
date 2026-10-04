# OntoFlow Integration Closure

> 准确口径：实现与集成验收框架完成。真实运行时封口待 R0–R3。

---

## 当前状态

```
OntoFlow v0.1 Implementation                ✅ COMPLETE
OntoFlow v0.1 Integration Harness Acceptance ✅ COMPLETE
OntoFlow v0.1 Real Runtime Acceptance        ⏳ R0–R3
```

## 已完成：实现 + 集成验收框架

| 阶段 | 名称 | 状态 |
|------|------|------|
| I0 | 代码迁入 temporal/chasm/lib/ontoflow/ | ✅ |
| I1 | 编译与回归（63 Go + ~400 Rust） | ✅ |
| I2-A | Temporal Server + CHASM Runtime | ✅ 197MB binary, v1.32.0, OntoFlow registered |
| I2-B | WorkItem Dispatch Semantics | ✅ OutcomeReported ≠ Committed |
| I3 | OntoLoop Runtime Integration | ✅ 2-attempt, Decision+Checkpoint bound |
| I4 | Authority Resolution Logic + Bindings | ✅ 12 tests: Decision bindings, negative cases |
| I5 | DAG & Concurrency Semantics | ✅ Sequence, Fan-in/out, cycle detection |
| I6 | Recovery State Machine + Idempotency | ✅ Lease, idempotency, generation guard |

**已证明的不变量：**
- `ActivityTaskCompleted ≠ WorkItem Committed`
- `Worker reported Committed ≠ WorkItem Committed`
- `AuthorityVerified = WorkItem Committed`
- 一个 WorkItem = 一个稳定 loop_id
- 旧 generation envelope 拒绝
- 双 Worker Lease 保护
- Commit 响应丢失 → 幂等 re-Respond

## 待完成：真实运行时封口 R0–R3

| 阶段 | 内容 | 阻塞 |
|------|------|------|
| R0 | 真实 OntoFlow Rust Worker（gRPC 长轮询替代文件/stdin） | 需 OntoFlow Rust SDK |
| R1 | 真实 OntoRuntime Attempt 执行（Mock→RuntimeRunPort） | 需 OntoRuntime Agent Loop 运行环境 |
| R2 | 真实 PG Authority Resolution（Mock→PostgreSQL） | 需 PG Decision/Receipt Store 建表 |
| R3 | 真实多进程故障与 DAG | 需 R0+R1+R2 先完成 |

## 不是"未完成"

完成了架构、协议、状态机、执行权威、编排算法和单节点集成验证。剩下的是把 mock 和测试桥替换成真实网络、真实 Worker、真实 OntoRuntime 与真实 PostgreSQL 后，再证明一次同样的不变量。
