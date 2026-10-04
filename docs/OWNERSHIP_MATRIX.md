# Ownership Matrix

## 谁拥有什么

| 概念 | 所有者 | 说明 |
|------|--------|------|
| User / Principal | OntoRuntime | 身份、认证 |
| Actor (Human/Agent/Service/Device) | OntoRuntime | Phase 2 统一入口引入 |
| Run / Session | OntoRuntime | 执行生命周期 |
| Agent Loop | OntoRuntime | LLM 调用循环 |
| Conversation / Thread | OntoRuntime | 聊天会话 |
| Message | OntoRuntime | 聊天消息 |
| LLM / Model Gateway | OntoRuntime | 模型调用 |
| Skill | OntoRuntime | 认知方法、Prompt 组合 |
| Capability | OntoRuntime | 对现实资源的操作 |
| CapabilityHost | OntoRuntime | 能力调度与执行 |
| Authorization | OntoRuntime | 授权判定 |
| Trust | OntoRuntime | 信任等级 |
| Approval | OntoRuntime | 审批流程 |
| Lease | OntoRuntime | 执行租约 |
| Secret | OntoRuntime | 密钥管理 |
| Network | OntoRuntime | 网络访问控制 |
| Filesystem | OntoRuntime | 文件系统访问控制 |
| Resource | OntoRuntime | 资源配额与限制 |
| Runtime Lane | OntoRuntime | 执行环境 (WASM/Docker/MCP) |
| EventStore | OntoRuntime | 事件持久化 |
| Memory | OntoRuntime | 持久化记忆 |
| CLI / WebUI | OntoRuntime | 用户界面 |
| | | |
| ExecutionContract | OntoAssure | 任务合约 |
| Criterion | OntoAssure | 验收标准 |
| VerifierBinding | OntoAssure | 验证器绑定 |
| Evidence | OntoAssure | 证据记录 |
| Verdict | OntoAssure | 证据归约结果 |
| SessionDecision | OntoAssure | 会话裁决 |
| SettlementDecision | OntoAssure | 副作用结算 |
| CommitPermit | OntoAssure | 发布授权令牌 |
| PublishReceipt | OntoAssure | 发布回执 |
| Effect Transaction | OntoAssure | 副作用事务 |
| Replay Integrity | OntoAssure | 复现验证 |
| | | |
| Attempt | OntoLoop | 尝试循环 |
| Continuation | OntoLoop | 续跑决策 |
| Repair | OntoLoop | 修复策略 |
| Progress | OntoLoop | 进度追踪 |
| Loop Budget | OntoLoop | 循环预算 |
| | | |
| Workflow | OntoFlow | 工作流定义 |
| WorkItem DAG | OntoFlow | 任务依赖图 |
| Timer | OntoFlow | 定时器 |
| Signal | OntoFlow | 信号 |
| Saga | OntoFlow | 补偿事务编排 |
| 长期等待 | OntoFlow | 人工/外部等待 |
| Worker 调度 | OntoFlow | 分布式 Worker |

## 交叉依赖规则

```
OntoRuntime 可以读取 OntoAssure 的 Decision
OntoRuntime 不能自行产生 Decision

OntoAssure 可以读取 OntoRuntime 的 RuntimeObservation
OntoAssure 不能控制 OntoRuntime 的执行

OntoLoop 可以调用 OntoRuntime 的 RunIngressPort
OntoLoop 不能进入 Agent Loop 内部控制

OntoFlow 可以调用 OntoLoop 的 Attempt
OntoFlow 不自产 TaskOutcome
```
