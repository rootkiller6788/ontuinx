# OntoFlow 集成分析

> 基于 `/home/admin1/temporal` 真实代码库

## 代码库概况

```
temporal/                         — Go 项目 (go.temporal.io/server)
├── chasm/                        — 自定义组件库 (不存在于上游 OntoFlow)
│   ├── workflow.go               — Workflow 定义
│   ├── task.go                   — Task 处理
│   ├── component.go              — 组件生命周期
│   ├── engine.go                 — 引擎
│   ├── visibility.go             — 可见性
│   └── statemachine.go           — 状态机
├── client/                       — Go SDK
│   ├── admin/
│   ├── frontend/
│   ├── history/
│   └── matching/
├── cmd/server/                   — 服务器入口
├── proto/                        — Protobuf 协议定义
├── api/                          — API 层
└── common/                       — 共享工具
```

## 与 OntoOS 的关系

当前已完成：
```
OntoFlow Adapter (我的假代码)     ❌ 没有看真实代码
真实 Temporal Fork               ⏳ 尚未分析集成点
```

## 下一步（正确顺序）

1. **分析 chasm/** — 理解自定义组件模型
2. **理解 Workflow 定义方式** — Go SDK workflow/activity 模式
3. **找到 Rust ↔ Go 边界** — 是 REST/gRPC/FFI 还是进程调用
4. **定义集成方案** — 给出具体文件级修改计划
5. **实现** — 基于真实代码修改
