# OntoFlow Go 侧实现规格

> Rust 侧已完成。本文档是 Go 侧 `chasm/lib/ontoflow/` 的施工规格。
> 基于 `docs/TEMPORAL_INTEGRATION_PLAN.md` 产出的 Rust 协议类型。
>
> ## 施工进度
>
> | 阶段 | 状态 | 交付物 |
> |------|------|--------|
> | G0 Protocol Freeze | ✅ | 4 schemas + 6 golden vectors + PROTOCOL.md |
> | G1 CHASM Component Foundation | ✅ | go-reference/: types, transitions, component, library, 8 tests |
> | G2 Real Activity/Matching Bridge | ⏳ | |
> | G3 Real Rust OntoLoop Execution | ⏳ | |
> | G4 Authority Outcome Resolution | ⏳ | |
> | G5 Static DAG | ⏳ | |
> | G6 Durable Recovery | ⏳ | |
> | G7 Batch Scheduling | ⏳ | |
> | G8 Dynamic Planning | ⏳ | |
> | G9 Deliberation | ⏳ | |

---

## 〇、前置：Rust 侧已提供的协议

Go 通过 OntoFlow ActivityTask 与 Rust Worker 通信。所有协议类型由 Rust 定义（JSON 序列化）。

### 输入 (Go → Rust)

```json
{
  "schema_version": 1,
  "flow_id": "flow-abc",
  "work_item_id": "wi-1",
  "loop_id": "loop-wi-1-gen1",
  "task_spec_ref": "spec/build-service",
  "contract_ref": "contract/default",
  "policy_ref": "policy/default",
  "input_artifact_refs": ["artifact/output-from-A"],
  "budget_grant_ref": "grant-wi-1",
  "budget_grant_hash": "abc123",
  "execution_generation": 1,
  "idempotency_key": "idem-flow-abc-wi-1-gen1",
  "request_binding_hash": "..."
}
```

### 输出 (Rust → Go)

```json
{
  "schema_version": 1,
  "flow_id": "flow-abc",
  "work_item_id": "wi-1",
  "loop_id": "loop-wi-1-gen1",
  "execution_generation": 1,
  "request_binding_hash": "...",
  "reported_terminal_state": "committed",
  "decision_id": "dec-xyz",
  "decision_hash": "...",
  "output_checkpoint_hash": "ckpt-...",
  "settlement_receipt_ref": "receipt-1",
  "outcome_binding_hash": "...",
  "total_attempts": 2,
  "terminal_reason": "task committed after 2 attempts"
}
```

---

## 一、文件结构

```
chasm/lib/ontoflow/
├── library.go              — OntoFlowLibrary 注册 (fx/provider)
├── types.go                — FlowSpec, WorkItemSpec, DependencySpec
├── flow_component.go       — OntoFlowExecutionComponent (CHASM 根组件)
├── work_item.go            — WorkItemRuntimeState
├── graph.go                — DAG 依赖解锁 (拓扑排序)
├── scheduler.go            — PureTask: 计算 Ready WorkItems
├── activity_scheduler.go   — 通过 chasm/lib/activity 调度 ExecuteOntoLoop
├── outcome_resolver.go     — 调用 AuthorityProjectionPort (gRPC)
├── barrier.go              — Fan-in / Round 同步
├── transitions.go          — 状态转换
├── cancellation.go         — Cancel / Signal / 超时处理
├── visibility.go           — OntoFlow 搜索属性
└── recovery.go             — 崩溃恢复 + 幂等
```

---

## 二、核心类型

### FlowSpec (不可变, 存 Artifact)

```go
type FlowSpec struct {
    FlowID       string
    Nodes        []FlowNode
    Dependencies []DependencyEdge
    MaxConcurrency uint32
    GlobalBudget BudgetSpec
}

type FlowNode struct {
    NodeID           string
    TaskSpecRef      string   // 不可变 Artifact 引用
    ContractRef      string
    PolicyRef        string
    ResourceClass    string
    RiskClass        string
}

type DependencyEdge struct {
    From string
    To   string
}
```

### OntoFlowState (持久化运行时状态)

```go
type OntoFlowState struct {
    FlowID          string
    Phase           FlowPhase
    FlowSpecRef     string
    GraphHash       string

    ActiveWorkItems  map[string]*WorkItemRuntimeState
    CompletedSummary FlowCompletionSummary

    MaxConcurrency   uint32
    RunningCount     uint32

    // 预算
    TotalBudget    BudgetAmount
    ConsumedBudget BudgetAmount

    // v0.1 边界
    // MaxInlineWorkItems = 256
    // MaxInlineDependencies = 1024
}

type FlowPhase string
const (
    FlowPhaseCreated    FlowPhase = "created"
    FlowPhaseRunning    FlowPhase = "running"
    FlowPhaseCompleted  FlowPhase = "completed"
    FlowPhaseFailed     FlowPhase = "failed"
    FlowPhaseCancelled  FlowPhase = "cancelled"
)
```

### WorkItemRuntimeState

```go
type WorkItemRuntimeState struct {
    WorkItemID string
    LoopID     string

    Phase WorkItemPhase

    NodeSpec    FlowNode
    InputRefs   []string
    OutputRefs  []string

    ExecutionGeneration uint64

    CurrentActivityID             string
    CurrentActivityScheduleEventID int64

    TerminalEnvelopeHash string
    DecisionID           string
    VerifiedOutcome      string  // "committed" | "not_committed" | "binding_mismatch"

    BudgetGrant BudgetGrant

    LastError  string
    RetryCount int
}

type WorkItemPhase string
const (
    WIPhaseBlocked           WorkItemPhase = "blocked"
    WIPhaseReady             WorkItemPhase = "ready"
    WIPhaseDispatched        WorkItemPhase = "dispatched"
    WIPhaseRunning           WorkItemPhase = "running"
    WIPhaseOutcomeReported   WorkItemPhase = "outcome_reported"
    WIPhaseAuthorityVerifying WorkItemPhase = "authority_verifying"
    WIPhaseCommitted         WorkItemPhase = "committed"
    WIPhaseEscalated         WorkItemPhase = "escalated"
    WIPhaseBudgetExhausted   WorkItemPhase = "budget_exhausted"
    WIPhaseCancelled         WorkItemPhase = "cancelled"
)
```

---

## 三、CHASM 组件

### OntoFlowExecutionComponent

```go
type OntoFlowExecutionComponent struct {
    component.BaseComponent

    FlowSpec  *FlowSpec
    State     *OntoFlowState
    WorkItems map[string]*WorkItemComponent
    Graph     *DependencyGraph
    Barriers  []*BarrierState
}
```

### CHASM Engine 集成

```go
func (c *OntoFlowExecutionComponent) Execute(ctx context.Context) error {
    switch c.State.Phase {
    case FlowPhaseCreated:
        return c.initialize(ctx)
    case FlowPhaseRunning:
        return c.orchestrate(ctx)
    case FlowPhaseCompleted, FlowPhaseFailed, FlowPhaseCancelled:
        return nil // terminal
    }
    return nil
}

func (c *OntoFlowExecutionComponent) orchestrate(ctx context.Context) error {
    // 1. 处理已完成的 WorkItem 结果
    for _, wi := range c.WorkItems {
        if wi.Phase == WIPhaseOutcomeReported {
            c.resolveOutcome(ctx, wi)
        }
    }

    // 2. 检查 Flow 是否完成
    if c.allTerminal() {
        c.State.Phase = FlowPhaseCompleted
        return nil
    }

    // 3. 解锁依赖已满足的 WorkItem
    ready := c.Graph.UnlockedNodes(c.State)

    // 4. 调度 Ready WorkItem（受并发限制）
    for _, nodeID := range ready {
        if c.State.RunningCount >= c.State.MaxConcurrency {
            break
        }
        c.dispatchWorkItem(ctx, nodeID)
    }

    return nil
}
```

---

## 四、核心算法

### DAG 依赖解锁 (graph.go)

```go
type DependencyGraph struct {
    nodes    map[string]*FlowNode
    edges    []DependencyEdge
    inDegree map[string]int
    children map[string][]string
}

// UnlockedNodes returns nodes whose dependencies are all committed.
func (g *DependencyGraph) UnlockedNodes(state *OntoFlowState) []string {
    var ready []string
    for nodeID := range g.nodes {
        wi, exists := state.ActiveWorkItems[nodeID]
        if !exists || wi.Phase == WIPhaseBlocked {
            if g.allDepsCommitted(nodeID, state) {
                ready = append(ready, nodeID)
            }
        }
    }
    return ready
}

func (g *DependencyGraph) allDepsCommitted(nodeID string, state *OntoFlowState) bool {
    for _, edge := range g.edges {
        if edge.To == nodeID {
            wi, exists := state.ActiveWorkItems[edge.From]
            if !exists || wi.Phase != WIPhaseCommitted {
                return false
            }
        }
    }
    return true
}
```

### Outcome Resolver (outcome_resolver.go)

Go 不直接读 Rust 数据库。通过 gRPC 调用 `AuthorityProjectionPort::resolve_loop_outcome()`。

```go
type OutcomeResolver struct {
    authorityClient AuthorityProjectionClient // gRPC
}

type ResolveLoopOutcomeRequest struct {
    FlowID              string
    WorkItemID          string
    LoopID              string
    ExecutionGeneration uint64
    TerminalEnvelopeHash string
}

type VerifiedLoopOutcome struct {
    LoopID              string
    Outcome             string // "committed" | "not_committed" | "decision_not_found" | "binding_mismatch"
    DecisionID          string
    DecisionHash        string
    OutputCheckpointHash string
    SettlementReceiptRef string
    AuthorityBindingHash string
}

func (r *OutcomeResolver) Resolve(ctx context.Context, req ResolveLoopOutcomeRequest) (*VerifiedLoopOutcome, error) {
    // gRPC call to Rust AuthorityProjectionPort
    return r.authorityClient.ResolveLoopOutcome(ctx, req)
}
```

### Activity Scheduler (activity_scheduler.go)

通过 CHASM Activity 库调度 ExecuteOntoLoop，**不走 SideEffectHandler 直连**。

```go
type ActivityScheduler struct {
    activityLib ActivityLibrary // chasm/lib/activity
}

func (s *ActivityScheduler) ScheduleExecuteOntoLoop(
    ctx context.Context,
    wi *WorkItemRuntimeState,
) error {
    payload := buildLoopInvocationRequest(wi)

    activity := s.activityLib.NewActivity("execute_onto_loop", payload)

    // 走标准 OntoFlow 分发: ActivityTask → TransferTask → Matching → Worker Poll
    err := s.activityLib.Schedule(ctx, activity)
    if err != nil {
        return err
    }

    wi.Phase = WIPhaseDispatched
    wi.CurrentActivityID = activity.ID
    return nil
}
```

---

## 五、状态转换表

```
WorkItem 状态转换:
─────────────────────────────────────────────
Blocked  → Ready          (所有依赖 Committed)
Ready    → Dispatched     (ActivityTask 已创建)
Dispatched → Running      (Worker 领取了任务)
Running  → OutcomeReported (Worker 返回了 Envelope)
OutcomeReported → AuthorityVerifying (开始验证)
AuthorityVerifying → Committed    (Authority 确认)
AuthorityVerifying → Escalated    (Authority 拒绝)
Any      → Cancelled     (外部 Cancel 信号)
Any      → BudgetExhausted (Flow 预算耗尽)

Flow 状态转换:
─────────────────────────────────────────────
Created   → Running      (初始化完成)
Running   → Completed    (所有 WorkItem Committed)
Running   → Failed       (关键 WorkItem Escalated)
Running   → Cancelled    (外部 Cancel 信号)
```

---

## 六、故障恢复 (recovery.go)

```go
func (c *OntoFlowExecutionComponent) Recover(ctx context.Context) error {
    // 1. 从 Temporal History 重放 Flow 状态
    // 2. 对每个 Dispatched/Running 的 WorkItem:
    //    - 查询 Activity 状态
    //    - 如果 Activity 已完成 → 处理 Envelope
    //    - 如果 Activity 仍在运行 → 等待
    //    - 如果 Activity 超时/丢失 → 重新派发 (相同 loop_id + generation)
    // 3. 重新计算依赖解锁
    for _, wi := range c.State.ActiveWorkItems {
        switch wi.Phase {
        case WIPhaseDispatched, WIPhaseRunning:
            status := c.queryActivityStatus(ctx, wi.CurrentActivityID)
            switch status {
            case ActivityCompleted:
                c.handleActivityCompletion(ctx, wi)
            case ActivityFailed, ActivityTimedOut:
                c.retryActivity(ctx, wi)
            case ActivityRunning:
                // continue waiting
            }
        }
    }
    return nil
}
```

---

## 七、v0.1 验收清单 (20 项)

**桥接 (5)**
- [ ] OntoFlowLibrary 注册到 CHASM Engine
- [ ] WorkItem 创建 ExecuteOntoLoop ActivityTask
- [ ] ActivityTask 通过 Matching 分发
- [ ] Rust Worker 可以通过 Poll 获取
- [ ] Completion 返回到正确 Component

**单 Loop (4)**
- [ ] 单 WorkItem 两 Attempt 后 Committed
- [ ] Worker 返回成功但 Decision 缺失 → 不得 Committed
- [ ] Decision 绑定错误 loop_id → 不得 Committed
- [ ] AuthorityProjectionPort 验证通过 → WorkItem Committed

**恢复 (5)**
- [ ] Activity 重试复用同一 loop_id
- [ ] Worker 崩溃不重复已完成 Attempt
- [ ] Commit 后响应丢失不重复副作用
- [ ] 双 Worker 竞争只有一个持有租约
- [ ] 重复 Completion 只应用一次

**DAG + 控制 (6)**
- [ ] A Committed 后解锁 B
- [ ] A 未 Committed 不解锁 B
- [ ] B、C 均 Committed 后解锁 D
- [ ] MaxConcurrency 严格限制
- [ ] Cancel 停止所有未完成 WorkItem
- [ ] Escalated 不投影为 Flow Success

---

## 八、Rust Worker 集成

Go 不需要了解 Rust 内部。唯一接口：

```protobuf
// AuthorityProjectionPort (gRPC)
service AuthorityProjection {
    rpc ResolveLoopOutcome(ResolveLoopOutcomeRequest) returns (VerifiedLoopOutcome);
}
```

ActivityTask 通过标准 OntoFlow 协议分发，Rust Worker 实现：

```
PollActivityTaskQueue("onto-workers")
→ 收到 LoopInvocationRequest
→ run_or_resume_to_terminal()
→ RespondActivityTaskCompleted(LoopTerminalEnvelope)
```

Go Outcome Resolver 收到 Envelope 后调用 gRPC 验证，验证通过后才标记 Committed。

---

## 九、服务器集成 (少量修改)

```go
// temporal/server/main.go 或等效启动文件
import "chasm/lib/ontoflow"

func main() {
    // ... existing setup ...

    // 注册 OntoFlow Library
    ontoflow.Register(fx.Options(
        ontoflow.NewLibrary(),
        // ... 其他 provider ...
    ))

    // ... rest of server startup ...
}
```

修改范围：
- CHASM 核心 (component/task/engine/registry): **0 行修改**
- OntoFlow 核心服务 (History/Matching/Frontend): **0 行修改**
- 启动注册: **~5–30 行**
- OntoFlow 新增: **~800 行 Go**
