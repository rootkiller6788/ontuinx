# State Authority

## 核心原则

**OntoRuntime 记录发生了什么。OntoAssure 判断这些事实能证明什么。**

## 成功判定权威

```
OntoRuntime Agent Loop 退出
    ↓
OntoAssure::FinalizationPort::finalize()
    ↓
加载 Evidence
    ↓
reduction::reduce()
    ↓
session_decision::decide_session()
    ↓
SessionResult { task_outcome, budget_outcome, lifecycle_state }
    ↓
DecisionStore::persist()
    ↓
OntoRuntime 必须服从
```

**OntoRuntime 不能自行产生 Success。**

## 副作用结算权威

### Staged Filesystem
```
持久化 Success + Commit
    ↓
CommitPermit (不可伪造)
    ↓
CAS 抢占发布权
    ↓
原子切换 Workspace
    ↓
持久化 PublishReceipt
    ↓
投影 Published / Committed
```

### Transactional Database
```
持久化 Success + Commit
    ↓
DatabaseCommitPermit
    ↓
BEGIN → 执行 → 验证 → COMMIT
    ↓
DatabaseTransactionReceipt
    ↓
Committed
```

## 终态定义

| 终态 | 含义 | 谁可以投影 |
|------|------|-----------|
| TaskOutcome::Success | 任务满足所有阻塞 Criterion | 仅 OntoAssure |
| LifecycleState::Committed | 副作用已安全发布 | 仅 OntoAssure |
| LifecycleState::Escalated | 需要人工或上级处理 | 仅 OntoAssure |
| RunStatus::Completed | OntoRuntime Run 生命周期 | OntoRuntime，但必须服从 OntoAssure 的裁决 |

## 状态优先级

1. **物理事实** > 逻辑状态
2. **持久化 Decision** > 内存计算
3. **Receipt 存在** > state 枚举
4. **磁盘 hash** > 事务记录

当物理事实与逻辑状态不一致时：**Frozen + Escalate**，不猜测。

## 禁止的终态路径

```
❌ Agent::finish() → TaskOutcome::Success
❌ OntoRuntime 默认 Completed
❌ 文件已切换但 Receipt 未存 → Committed
❌ Decision 在内存但未持久化 → Success
❌ 状态不一致 → 猜测 Committed
```

## 已实现的终态控制

| 路径 | M5 | M6-A |
|------|:--:|:----:|
| Agent FinishRequested | ✅ Escalated（无证据） | — |
| Success + Commit | ✅ Committed | — |
| Persist failure → Escalated | ✅ | — |
| 空证据 → Escalated | ✅ | — |
| CAS 竞争 → 单发布者 | — | ✅ |
| Receipt 失败 → 不 Committed | — | ✅ |
| 崩溃 → 磁盘对账 | — | ✅ |
| 状态不可判定 → Frozen | — | ✅ |
