# OntoFirmwareGraph — 独立魔改与 OntoOS 拼接计划

> **定位**：通用多语言代码图谱系统，以固件和安全关键软件为首要深化方向。  
> 基于 CBM 深度魔改的独立、通用、多语言代码仓库事实与执行记忆图谱系统，优先面向固件、嵌入式、遗留系统和安全关键软件。
>
> 基于 2026-07-28 全代码库现状调查生成。调查范围：CBM main、OntoOS、IronClaw、OpenCodeReview。

---

## 最终命名体系

| 层级 | 名称 |
|------|------|
| 系统名 | **OntoFirmwareGraph** |
| 服务名 | `ontofirmwaregraphd` |
| 数据库 | `onto_firmware_graph` |
| 表前缀 | `ofg_` |
| C 计算内核 | `ofg-core` |
| Rust 服务层 | `ofg-service` |
| 客户端 | `onto-firmware-graph-client` |
| 协议 | `onto_firmware_graph.proto` |

---

## 最终关系

```
OntoOS
= 可信执行、验证、裁决、结算

OntoFirmwareGraph
= 通用仓库事实、Candidate 分析、执行记忆

Embedded Firmware Pack
= OntoFirmwareGraph 最重要的领域增强包
```

---

## 目录

1. [现状基线](#一现状基线)
2. [总架构](#二总架构)
3. [第一周期：独立完成 OntoFirmwareGraph](#三第一周期独立完成-ontofirmwaregraph)
   - [P0：Fork、上游冻结、Golden 基线](#p0fork上游冻结golden-基线)
   - [P1：功能开关隔离、最小 Headless Target](#p1功能开关隔离最小-headless-target)
   - [P2：抽取纯 C 计算内核、GraphSink 抽象](#p2抽取纯-c-计算内核graphsink-抽象)
   - [P2.5：物理删除产品壳](#p25物理删除产品壳)
   - [P3：独立 Rust Service、故障隔离](#p3独立-rust-service故障隔离)
   - [P4：PostgreSQL 替换 SQLite（修正模型）](#p4postgresql-替换-sqlite修正模型)
   - [P5：快照、增量、Merkle Hash、一致性](#p5快照增量merkle-hash一致性)
   - [P6：通用语言能力合同与独立验收](#p6通用语言能力合同与独立验收)
4. [第二周期：拼接 OntoOS](#四第二周期拼接-ontoos)
   - [P7：OntoAssure Graph Verifier 接入](#p7ontoassure-graph-verifier-接入)
   - [P8：权威 Outbox 与图谱留存](#p8权威-outbox-与图谱留存)
   - [P9：Runtime 可选 GraphRead 与策略注入](#p9runtime-可选-graphread-与策略注入)
   - [P10：OpenCodeReview Semantic Verifier 与领域包](#p10opencodereview-semantic-verifier-与领域包)
5. [风险与依赖](#五风险与依赖)
6. [附录：删除与保留清单](#六附录删除与保留清单)

---

## 一、现状基线

### 1.1 CBM (codebase-memory-mcp) 当前状态

| 维度 | 现状 |
|------|------|
| 语言 | Pure C，单二进制编译，零运行时依赖 |
| 构建 | Makefile.cbm，所有 .c 一次性编译为单一二进制 |
| 语法支持 | 158+ tree-sitter 语言，编译进二进制 |
| 存储 | SQLite (WAL mode)，8 表 + FTS5，自定义 B-tree 页写入器 |
| 解析管线 | 多 Pass 架构：discover → extract → resolve → calls → usages → semantic → post-passes |
| 覆盖报告 | 已有完整系统（#963）：`cbm_coverage_row_t` + `cbm_coverage_meta_t` + shadow project |
| 增量索引 | 完整实现：file_hashes 比较 → 三类分拣 → purge → re-extract → re-link inbound edges |
| Daemon | 8 层架构：IPC → Runtime → Coordinator → Service → Bootstrap → Host → Application → Frontend |
| 产品功能 | UI (3D 可视化)、MCP Server、43 种 Agent 自动配置、自动升级、Hooks/Skills 注入 |
| 分发 | 9 个包管理器，都是下载二进制 shim |
| 已补功能 | `pass_c_fnptr_bindings.c` (691 行)：CallbackSlot + POINTS_TO 边 |
| **关键风险** | main.c ~2200 行，daemon、MCP、store、pipeline 交叉依赖未识别；单二进制物理删除风险极高 |

### 1.2 OntoOS 当前状态

| 维度 | 现状 |
|------|------|
| 语言 | Rust workspace，10 个 member crate |
| 核心 crate | `onto-assurance-types` (95%), `onto-assurance-core` (88%), `onto-assurance-runtime` (80%) |
| 适配器 | `onto-ironclaw-adapter` (35%), `onto-temporal-adapter` (中等) |
| 验证 | `onto-code-pack` (55%)：7 语言 × 4 级 MachineVerifier 真实实现 |
| 语义验证 | **3 个 SemanticVerifierPort 实现全是硬编码 stub** |
| 数据库 | raw `tokio_postgres`，OntoOS 4 表 + onto_outbox；IronClaw 32 个 Flyway 迁移 |
| Outbox | 已实现：`onto_outbox` 表 + write/list/mark_published + 测试 |
| Redis | InMemoryRedisAdapter 完整实现，无真实 Redis 连接 |
| 能力网关 | CapabilityGateway 真实实现：16 种 capability，5 种只读，Lane A/B 隔离 |
| 三本总账 | ScopeLedger + RuleCoverageLedger + VerifierExecutionLedger |
| 生产就绪 | 评估 **25-35%** |

### 1.3 OpenCodeReview 当前状态

| 维度 | 现状 |
|------|------|
| 语言 | Go 1.25.5，Apache-2.0 |
| 架构 | "确定性工程 × Agent 混合" |
| 语言支持 | 75+ 文件扩展名，26 个 rule_docs |
| 与 OntoOS 关系 | **零集成** |

---

## 二、总架构

```
╔══════════════════════════════════════════════════════════════════════╗
║                              OntoOS                                  ║
║            AI Coding Agent 可信执行与验证控制系统                     ║
╚══════════════════════════════════════════════════════════════════════╝

L4  OntoFlow · Go      工作流/DAG/Barrier/Saga
L3  OntoLoop · Rust     Attempt/Continuation/Checkpoint
L2  OntoRuntime · Rust  Agent Loop, Lane A/B, CapabilityGateway
L1  OntoAssure · Rust   Discovery→Scope→Rules→Plan→Evidence→Decision

        OntoAssure 确定性调用（P7 起）
              │
              ▼
┌─────────────────────────────────────────────────────────────────────┐
│ OntoFirmwareGraph（独立系统，非 MCP 工具）                            │
│                                                                     │
│ Internal API Layer                                                  │
│ Snapshot Engine (Baseline / Candidate / Seal / Promote / Discard)    │
│ Query Engine (Symbol / CallGraph / BlastRadius / ExecutionHistory)  │
│ Analysis Pipeline (ofg-core C compute kernel)                       │
│ PostgreSQL Graph Store (独立数据库 onto_firmware_graph)              │
└─────────────────────────────────────────────────────────────────────┘
```

**OntoFirmwareGraph 不是**：MCP 工具、Agent 插件、可视化前端、OntoAssure 替代品。

**OntoFirmwareGraph 负责**：Repository Graph、Candidate Analysis、Query & Context、Execution Memory Projection。

**OntoFirmwareGraph 不负责**：PASS/FAIL 裁决（属于 OntoAssure）、Agent 自主决策、实时监控。

---

## 三、第一周期：独立完成 OntoFirmwareGraph

> 核心原则：P0–P6 期间不修改 OntoRuntime 和 OntoAssure 主链。P6 完成前不进入 OntoOS 对接。

```
P0  上游冻结、Golden基线
        ↓
P1  功能开关、最小 Headless Target
        ↓
P2  抽取纯C计算内核、GraphSink
        ↓
P2.5 物理删除UI/MCP/安装器/原daemon
        ↓
P3  独立Rust Service、故障隔离
        ↓
P4  PostgreSQL Graph Store（修正模型）
        ↓
P5  Baseline/Candidate/Snapshot/Merkle Hash
        ↓
P6  通用语言能力合同与独立验收

════════ 独立完成，允许进入对接 ════════
```

---

### P0：Fork、上游冻结、Golden 基线

**目标**：先证明不破坏任何上游能力。

**工作**：
1. Fork CBM，锁定上游 commit，建立 `upstream` 分支
2. 记录 License 与全部第三方依赖（mimalloc, sqlite3, yyjson, xxhash, tre, nomic）
3. 跑通原始构建：`make -f Makefile.cbm cbm`（已验证通过）
4. 跑通原始测试：`make -f Makefile.cbm test`
5. 建立至少 4 种优先语言的样本仓库 fixture（C/C++、Python、TypeScript、Java）
6. 记录全量索引性能基线（时间、RSS、DB 大小）
7. 记录增量索引性能基线
8. 保存 Golden 输出（节点数、边数、查询结果）→ 可重放比较

**退出标准**：
- [ ] 原始项目稳定构建、核心测试全部通过
- [ ] ≥4 种优先语言有固定 fixture
- [ ] 节点、边、查询结果可重放比较
- [ ] 性能基线已记录

---

### P1：功能开关隔离、最小 Headless Target

**目标**：先隔离，再删除。通过编译开关得到一个不依赖 UI/MCP/daemon 的最小构建目标。

**不做物理删除。只做编译目标隔离。**

**编译开关**：
```c
#define OFG_ENABLE_UI          0   // src/ui/ 全部
#define OFG_ENABLE_MCP         0   // src/mcp/ 全部
#define OFG_ENABLE_DAEMON      0   // src/daemon/ 全部
#define OFG_ENABLE_INSTALLER   0   // src/cli/ install/uninstall/update
#define OFG_ENABLE_TELEMETRY   0   // src/telemetry/ 全部
#define OFG_ENABLE_EMBEDDING   0   // semantic embedding, nomic（可选）
```

**最小目标链路**：
```
最小CLI → discover → parse → extract → resolve → graph buffer → SQLite 兼容输出
```

**验证**：
- 关闭全部开关后编译通过
- 全量索引功能正常
- 增量索引功能正常
- 覆盖报告功能正常
- 与原版 Golden 输出逐项对比一致

**随时可以打开开关与上游基线对照。物理删除只在 P2.5 执行。**

**退出标准**：
- [ ] 五个 OFG_ENABLE_* = 0 时编译通过、链接通过
- [ ] 最小链路三功能（全量/增量/覆盖）Golden 一致
- [ ] main.c 中受影响的初始化路径已用 `#if OFG_ENABLE_*` 包裹
- [ ] 未删除任何源文件

---

### P2：抽取纯 C 计算内核、GraphSink 抽象

**目标**：定义 C Core 与外部世界的唯一边界。

#### 2a. GraphSink 抽象

C Core 不再直接写 SQLite。所有输出通过函数指针表：

```c
typedef struct ofg_snapshot_meta {
    const char *repository_id;
    const char *base_commit_sha;
    const char *candidate_checkpoint_hash;
    uint64_t    execution_generation;
    const char *analysis_profile_hash;
    const char *extractor_version;
    uint32_t    graph_schema_version;
} ofg_snapshot_meta_t;

typedef struct ofg_graph_sink {
    void *ctx;

    int (*begin_snapshot)(void *ctx, const ofg_snapshot_meta_t *meta);
    int (*write_nodes)(void *ctx, const ofg_node_batch_t *batch);
    int (*write_edges)(void *ctx, const ofg_edge_batch_t *batch);
    int (*write_deletions)(void *ctx, const ofg_delete_batch_t *batch);
    int (*write_coverage)(void *ctx, const ofg_coverage_batch_t *batch);
    int (*write_diagnostics)(void *ctx, const ofg_diagnostic_batch_t *batch);
    int (*commit_snapshot)(void *ctx);
    int (*rollback_snapshot)(void *ctx);
} ofg_graph_sink_t;
```

#### 2b. 两个适配器

```
P2–P3：SQLiteCompatibilitySink
    → 写入原 SQLite store
    → Golden 对比基准

P4 以后：PostgreSQLBatchSink
    → COPY 批量写入
    → 单事务 MERGE
```

**切换存储后端不需要第二次拆毁 Pipeline。**

#### 2c. C Core 生命周期 API

```c
ofg_core_t*  ofg_core_create(void);
int          ofg_core_load_profile(ofg_core_t *c, const char *profile_json);
int          ofg_core_analyze_baseline(ofg_core_t *c, const char *repo_path,
                                       const char *project_name,
                                       ofg_graph_sink_t *sink);
int          ofg_core_analyze_delta(ofg_core_t *c, const char *repo_path,
                                    const char *project_name,
                                    const char **changed_files, int changed_count,
                                    ofg_graph_sink_t *sink);
void         ofg_core_destroy(ofg_core_t *c);
```

**退出标准**：
- [ ] GraphSink 接口定义稳定
- [ ] SQLiteCompatibilitySink 与原版输出 Golden 一致
- [ ] C Core 不包含任何 SQLite 写入代码（写入只在 Sink 实现中）
- [ ] C Core 不引用 UI、MCP、daemon 符号
- [ ] 可作为独立 .a / .so 编译

---

### P2.5：物理删除产品壳

**目标**：在内核抽离并验证后，安全删除产品代码。

**删除清单**：

| 删除项 | 目录/文件 |
|--------|----------|
| 3D 图谱 UI + HTTP Server | `src/ui/` |
| 前端静态资源 | `graph-ui/` |
| MCP Server 全部 | `src/mcp/` (mcp.c ~11,279 行) |
| 43 种 Agent 自动配置 | `src/cli/agent_clients.c`, `agent_profiles.c` |
| Hooks/Skills 注入 | `src/cli/hook_augment.c` |
| 原 daemon 全部 | `src/daemon/` (8 层全部) |
| 自动升级 | `src/upgrade/` |
| 遥测 | `src/telemetry/` |
| 产品安装/配置编辑器 | `src/cli/` install/uninstall/update, `config_*.c` |
| 分发包装 | `pkg/` |
| Cypher SQLite executor | `src/cypher/` 中与 SQLite 表访问、FTS5 绑定部分 |

**保留（从生产构建移除，迁移到 devtools/）**：
```
tools/tree-sitter-form    → devtools/grammars/
tools/tree-sitter-magma   → devtools/grammars/
```

**保留（只保留 parser，重写 executor）**：
```
src/cypher/               # Cypher lexer, parser, AST 保留
                           # SQLite executor 重写 / 生产 API 完全领域化
                           # Cypher 只保留为开发调试接口
```

**退出标准**：
- [ ] 删除后编译通过、全量索引 Golden 一致
- [ ] 删除后增量索引 Golden 一致
- [ ] P1 中定义的 OFG_ENABLE_* 宏全部移除
- [ ] devtools/ 中保留了 grammar 维护工具

---

### P3：独立 Rust Service、共享内存数据平面与故障隔离

**目标**：建立独立的 `ontofirmwaregraphd` 服务，通过共享内存连接 CBM-derived C 计算内核，在保持进程隔离的同时避免 JSON、CLI 文件和 RPC 承载大规模图谱批次。

P3 期间不修改 OntoRuntime 和 OntoAssure。父进程是独立的 `ontofirmwaregraphd`，不是 OntoOS Worker。

最终技术组合：

```
memfd_create + mmap + SPSC ring buffer + 两个eventfd + 固定Wire ABI
+ Rust父进程控制PostgreSQL事务 + pidfd/资源限制/超时
```

#### 3a. 总体架构

```
┌──────────────────────────────────────────────────────────────┐
│ ontofirmwaregraphd · Rust父进程                              │
│                                                              │
│ Repository/Snapshot Lifecycle                                │
│ PostgreSQL Graph Store                                       │
│ Query Engine                                                 │
│ Resource Control                                             │
│                                                              │
│ memfd共享内存管理                                             │
│ SPSC Ring Consumer                                           │
│ PostgreSQL COPY Writer                                       │
│ Child Supervisor                                             │
└──────────────────────────────┬───────────────────────────────┘
                               │
              memfd + mmap + eventfd + inherited FDs
                               │
┌──────────────────────────────▼───────────────────────────────┐
│ ofg-core · C子进程                                           │
│                                                              │
│ discover → parse → extract → resolve → pipeline              │
│                                      │                       │
│                                      ▼                       │
│                          SharedMemoryGraphSink               │
│                          SPSC Ring Producer                  │
└──────────────────────────────────────────────────────────────┘
```

P7 后才是：

```
OntoAssure
    ↓ 内部API
ofg-service
    ↓ 共享内存
ofg-core
```

#### 3b. 共享内存建立

Rust父进程负责：

1. `memfd_create("ofg-batch-ring")`
2. `ftruncate` 到配置容量
3. `mmap` 为共享读写区域
4. 创建 `data_ready_eventfd`
5. 创建 `space_ready_eventfd`
6. 初始化 Control Header 与 Ring Metadata
7. 启动 ofg-core 子进程并传递 FD 编号

默认配置：

```
Ring容量：256 MiB
最小容量：64 MiB
最大容量：可配置
Producer：单个ofg-core进程
Consumer：单个ontofirmwaregraphd任务
模型：SPSC Ring Buffer
```

共享内存容量是流式窗口，不要求容纳整个仓库图谱。Ring满时，C Producer等待Rust释放空间。

使用 `memfd_create` 创建匿名、RAM-backed、可 `mmap` 的文件描述符；最后一个引用关闭后，内核自动释放，不需要清理 `/dev/shm` 名称。可加 `F_SEAL_GROW` / `F_SEAL_SHRINK` 阻止子进程改变共享区容量，但不能加 `F_SEAL_WRITE`（C 子进程仍需写入）。

#### 3c. 共享内存布局

```
┌──────────────────────────────────────────────────────────────┐
│ Control Header                                               │
│ magic / ABI version / state / cancel / error                 │
│ snapshot_epoch / producer_seq / consumer_seq                 │
├──────────────────────────────────────────────────────────────┤
│ Ring Metadata                                                │
│ capacity / write_offset / read_offset / wrap_generation      │
├──────────────────────────────────────────────────────────────┤
│ Frame 1                                                      │
├──────────────────────────────────────────────────────────────┤
│ Frame 2                                                      │
├──────────────────────────────────────────────────────────────┤
│ ...                                                          │
└──────────────────────────────────────────────────────────────┘
```

所有共享结构必须：

- 使用固定宽度整数
- 明确字节对齐
- 不包含任何原生指针
- 不包含 `size_t`、`long` 或编译器相关枚举
- 采用相对 `offset + length` 表示字符串和数组
- 包含 `magic`、ABI version、长度和 checksum

#### 3d. Frame 类型与 Wire ABI

Frame 类型：

```
SNAPSHOT_BEGIN
NODE_BATCH
EDGE_BATCH
DELETE_BATCH
COVERAGE_BATCH
DIAGNOSTIC_BATCH
SNAPSHOT_END
FATAL_ERROR
HEARTBEAT
```

Frame Header：

```c
typedef struct ofg_frame_header {
    uint32_t magic;
    uint16_t abi_version;
    uint16_t record_kind;

    uint64_t sequence;
    uint64_t snapshot_epoch;
    uint64_t payload_length;

    uint32_t record_count;
    uint32_t checksum;
} ofg_frame_header_t;
```

Payload 内全部使用固定宽度整数 + offset + length + UTF-8 字节区。例如 Node Wire Record：

```c
typedef struct {
    uint8_t  entity_id[16];
    uint32_t kind;
    uint32_t language;
    uint32_t stable_key_offset;
    uint32_t stable_key_length;
    uint32_t qualified_name_offset;
    uint32_t qualified_name_length;
    uint32_t properties_offset;
    uint32_t properties_length;
    uint32_t start_line;
    uint32_t end_line;
} ofg_node_wire_t;
```

Ring 中禁止出现任何原生指针。该机制定义为：

> 共享内存零拷贝传输＋最小定长帧编码。

不宣称直接共享 CBM 内部 C 结构体。

#### 3e. 同步与背压

```
C写入完整Frame
    ↓ Release发布write_offset
eventfd_write(data_ready_fd)
    ↓
Rust从epoll收到通知
    ↓ Acquire读取write_offset
Rust校验Frame
    ↓
PostgreSQL COPY / Staging
    ↓ Release发布read_offset
eventfd_write(space_ready_fd)
```

共享控制字段使用 C11 原子和 Rust 原子，以 **Producer Release / Consumer Acquire** 建立可见性。不能依赖普通读写碰巧可见。

Ring空间不足时：C 不覆盖未消费数据 → 等待 `space_ready_fd` → Rust 释放空间后继续。

取消与强制终止使用 `Atomic cancel flag` + `pidfd` / signal。超时后通过进程描述符发信号，避免仅依赖可复用的裸 PID。

#### 3f. PostgreSQL 事务边界

Rust父进程拥有全部数据库事务权。

```
SNAPSHOT_BEGIN
    ↓
创建BUILDING Snapshot + 打开Staging/COPY写入

NODE/EDGE/COVERAGE BATCH
    ↓
持续写入临时表或BUILDING快照分区

SNAPSHOT_END
    ↓
校验Batch计数 → 校验Merkle输入 → 校验Coverage → 原子MERGE → Snapshot→SEALED
```

C 子进程崩溃、超时或输出损坏时：

- 终止当前 COPY
- 回滚当前事务或将 Snapshot 标记 INVALID
- 不产生 SEALED 快照
- 不影响已有 Baseline
- Rust 父进程继续运行

> C Core 崩溃只会使当前构建中的 Snapshot 失败，不会污染 Canonical Graph，也不会拖垮 Rust Service。

#### 3g. 子进程安全控制

Rust 父进程负责：

- 关闭无关文件描述符
- 只传递共享内存和 eventfd
- 设置 `no_new_privs`
- 设置 CPU / RSS / NOFILE 限制
- 设置任务超时
- 监控 Heartbeat
- 支持 cancel flag
- 超时后通过 pidfd 终止
- 回收共享内存和子进程

仓库访问权限由任务类型决定：

```
Baseline构图：只读仓库快照
Candidate构图：只读 Staged Workspace 快照

任何 ofg-core：
  不能连接 PostgreSQL
  不能访问 Redis
  不能开放网络
  不能修改 OntoOS 状态
```

#### 3h. CLI 处理

不再通过 CLI 传输：NodeBatch、EdgeBatch、CoverageBatch、大规模 JSON、临时图谱文件。

CLI 仅保留开发调试用途。生产子进程启动参数最多保留：

```
--worker
--shm-fd=<fd>
--data-event-fd=<fd>
--space-event-fd=<fd>
```

Repository、Snapshot、Profile 和任务元数据写入共享内存 Control Header 或启动 Manifest，不通过命令行暴露大量业务参数。

#### 3i. 退出标准

- [ ] `ontofirmwaregraphd` 可独立启动
- [ ] Rust 可创建 memfd 共享区并启动 ofg-core
- [ ] NodeBatch / EdgeBatch 可通过 Ring 稳定传输
- [ ] 共享 Wire ABI 不包含任何指针
- [ ] Ring 具有背压，不覆盖未消费数据
- [ ] eventfd 通知可接入 epoll
- [ ] C Core 崩溃不拖垮 Rust Service
- [ ] 崩溃中的 Snapshot 不会变成 SEALED
- [ ] PostgreSQL 断连可以终止任务并安全回滚
- [ ] 超时、取消、子进程回收测试通过
- [ ] 共享内存吞吐显著高于 JSONL / CLI 基线
- [ ] SQLiteCompatibilitySink 仍可用于 Golden 对照

#### 3j. 性能预期

传输层不进行 JSON 序列化、Payload 无需从内核复制到 Rust 堆对象、批量数据可直接从 mmap 切片写入 COPY 编码器、每个 Batch 仅产生常数级通知开销。

不能承诺"与 FFI 几乎一样快"——仍存在 CBM 内部结构 → Wire Batch 写入、eventfd 系统调用、进程调度、Rust 解析 Frame Header、PostgreSQL COPY 编码。但会明显快于 JSONL stdout、临时文件、逐条 RPC。

---

### P4：PostgreSQL 替换 SQLite（修正模型）

**目标**：修正原文档中三个结构性缺陷。

#### 4a. 稳定实体 + 快照版本事实（修正缺陷 1）

原设计把可变属性放进稳定实体表。**修正后分离**：

```sql
-- 稳定实体（跨快照不变）
CREATE TABLE ofg_entity (
    entity_id UUID PRIMARY KEY,
    repository_id UUID NOT NULL,
    stable_key TEXT NOT NULL,          -- 基于语法位置的稳定标识
    entity_kind TEXT NOT NULL,         -- Function / Class / Struct / Variable / ...
    language TEXT,
    UNIQUE(repository_id, stable_key)
);

-- 快照版本事实（随快照变化）
CREATE TABLE ofg_entity_version (
    snapshot_id UUID NOT NULL REFERENCES ofg_snapshot,
    entity_id UUID NOT NULL REFERENCES ofg_entity,
    qualified_name TEXT,               -- 可能因作用域变化
    file_id UUID REFERENCES ofg_file,  -- 可能因文件重命名
    start_line INT,                    -- 必然随快照变化
    end_line INT,                      -- 必然随快照变化
    structural_hash TEXT NOT NULL,     -- 确定实体内容是否变化
    properties JSONB NOT NULL DEFAULT '{}',
    tombstone BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(snapshot_id, entity_id)
);
```

**每个快照完整保存实体版本内容，或保存可确定重建的 Delta。operation = ADDED/MODIFIED/REMOVED 不足以恢复任意快照。**

#### 4b. 边稳定键（修正缺陷 2）

原唯一约束 `UNIQUE(source_node_id, target_node_id, type)` 会错误合并：

- 同一函数中两个不同调用点
- 两条不同条件分支
- 两个不同 callback slot
- 不同 build profile 下的关系

**修正后**：

```sql
CREATE TABLE ofg_edge (
    edge_id UUID PRIMARY KEY,
    repository_id UUID NOT NULL,
    snapshot_id UUID NOT NULL,
    source_entity_id UUID NOT NULL,
    target_entity_id UUID NOT NULL,
    edge_kind TEXT NOT NULL,
    callsite_key TEXT,                 -- 调用点位置 hash
    semantic_slot TEXT,                -- 函数指针槽位 / callback slot ID
    analysis_profile TEXT NOT NULL,    -- extractor_version + profile_hash
    properties JSONB NOT NULL DEFAULT '{}',

    -- 稳定键：hash(source, target, edge_kind, callsite_key, semantic_slot)
    stable_key TEXT NOT NULL,
    UNIQUE(snapshot_id, stable_key)
);
```

#### 4c. 全文检索修正（修正缺陷 3）

```sql
-- 代码标识符不能被 English stemmer 词干化
CREATE INDEX idx_ofg_entity_version_fts
    ON ofg_entity_version
    USING GIN (to_tsvector('simple', coalesce(qualified_name, '') || ' ' || coalesce(stable_key, '')));

-- 模糊符号名称、路径片段用 pg_trgm
CREATE EXTENSION IF NOT EXISTS pg_trgm;
CREATE INDEX idx_ofg_entity_version_name_trgm ON ofg_entity_version USING GIN (qualified_name gin_trgm_ops);
CREATE INDEX idx_ofg_entity_stable_trgm ON ofg_entity USING GIN (stable_key gin_trgm_ops);
```

#### 4d. 其余表

```sql
CREATE TABLE ofg_repository (
    repository_id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    root_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE ofg_snapshot (
    snapshot_id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES ofg_repository,
    base_commit_sha TEXT NOT NULL,
    candidate_checkpoint_hash TEXT,
    execution_generation BIGINT DEFAULT 0,
    analysis_profile_hash TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    graph_schema_version INT NOT NULL,
    graph_content_hash TEXT NOT NULL,
    node_count BIGINT NOT NULL DEFAULT 0,
    edge_count BIGINT NOT NULL DEFAULT 0,
    coverage_status TEXT NOT NULL DEFAULT 'BUILDING',
    state TEXT NOT NULL DEFAULT 'BUILDING',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    sealed_at TIMESTAMPTZ
);

CREATE TABLE ofg_file (
    file_id UUID PRIMARY KEY,
    repository_id UUID NOT NULL REFERENCES ofg_repository,
    rel_path TEXT NOT NULL,
    UNIQUE(repository_id, rel_path)
);

CREATE TABLE ofg_file_version (
    snapshot_id UUID NOT NULL REFERENCES ofg_snapshot,
    file_id UUID NOT NULL REFERENCES ofg_file,
    content_sha256 TEXT NOT NULL,
    size_bytes BIGINT,
    language TEXT,
    PRIMARY KEY(snapshot_id, file_id)
);

CREATE TABLE ofg_candidate_delta (
    delta_id UUID PRIMARY KEY,
    snapshot_id UUID NOT NULL REFERENCES ofg_snapshot,
    attempt_id TEXT NOT NULL,
    execution_generation BIGINT NOT NULL,
    candidate_checkpoint_hash TEXT NOT NULL,
    added_files UUID[],
    changed_files UUID[],
    deleted_files UUID[],
    renamed_files JSONB
);

CREATE TABLE ofg_pass_coverage (
    snapshot_id UUID NOT NULL REFERENCES ofg_snapshot,
    entity_id UUID NOT NULL REFERENCES ofg_entity,
    pass_name TEXT NOT NULL,
    status TEXT NOT NULL,
    detail JSONB,
    PRIMARY KEY(snapshot_id, entity_id, pass_name)
);

CREATE TABLE ofg_diagnostic (
    diagnostic_id BIGSERIAL PRIMARY KEY,
    snapshot_id UUID NOT NULL REFERENCES ofg_snapshot,
    entity_id UUID REFERENCES ofg_entity,
    pass_name TEXT NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    detail JSONB
);

CREATE TABLE ofg_execution_projection (
    projection_id UUID PRIMARY KEY,
    event_id TEXT NOT NULL UNIQUE,
    event_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}',
    projected_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

**硬约束**：
- 不同数据库账号（`ontofirmwaregraph` vs `ontoos`）
- 不同 Migration、不同连接池
- 无跨库外键、无跨数据库事务

**退出标准**：
- [ ] 生产路径不依赖 SQLite
- [ ] 版本事实表可独立恢复任意快照
- [ ] 边不因 stable_key 冲突而丢失
- [ ] FTS 对代码标识符搜索准确

---

### P5：快照、增量、Merkle Hash、一致性

**目标**：版本化图谱 + 高效全图完整性证明。

#### 5a. 分层 Merkle Hash（替代全图 SHA-256）

```
FileFactHash = hash(file_content)
    ↓
EntityVersionHash = hash(entity facts in file)
    ↓
FileVersionHash = hash(FileFactHash, EntityVersionHashes[])
    ↓
Snapshot Merkle Root
```

**Candidate 只重新计算变化分区**：changed files + 受影响跨文件关系 + 上层 Merkle 路径。

**最终的 graph_content_hash 是 Merkle Root，仍然可以绑定 Evidence，但不需要每次扫描全图。**

#### 5b. 核心概念

| 概念 | 说明 |
|------|------|
| **Baseline Snapshot** | 对应已提交仓库状态 |
| **Candidate Delta** | 对应当前 Attempt |
| **Sealed GraphSnapshot** | 不可变。只有 SEALED 才能用于 Assurance |
| **Promote** | Candidate → 新 Baseline |
| **Discard** | 丢弃，不污染 Baseline |

#### 5c. 可信条件

```
文件变化 ≠ 图谱可用

GraphSnapshot SEALED + 输入 Hash 一致 = 图谱可用
```

**Watcher 只用于预热**。`WatcherUpdated ≠ SnapshotReady`。Assurance 仍然只认可显式 `PrepareCandidate → SealSnapshot`。

**退出标准**：
- [ ] Merkle Root 实现、Candidate 只重算变化分区
- [ ] 同一 Baseline 可产生多个隔离 Candidate
- [ ] 增量不污染 Baseline / Discard 无残留 / Promote 形成新 Baseline
- [ ] 全量与增量 Golden 一致

---

### P6：通用语言能力合同与独立验收

**目标**：从笼统 "✅" 升级为可测量能力矩阵。

#### 6a. 能力矩阵

每种语言逐项标记：

| 能力 | 指标 |
|------|------|
| **Parsing** | 成功解析文件比例 |
| **Symbol extraction** | Precision / Recall |
| **Cross-file resolution** | 唯一解析率、歧义率 |
| **Call resolution** | Direct / Inferred / Unresolved 比例 |
| **Type resolution** | Toolchain-resolved 覆盖率 |
| **Impact analysis** | 已知影响集召回率 |
| **Coverage** | Complete / Partial / Unsupported / ParseFailed |

#### 6b. Tier 0 退出标准

- [ ] 每项指标达到预设阈值
- [ ] 所有未解析关系显式标记（Unresolved / Inferred / Ambiguous）
- [ ] 没有将推测关系伪装成确定关系
- [ ] Coverage 报告中无静默跳过

#### 6c. 分两个发布点

**v0.1**（优先交付）：
- C/C++ / Java / Python / TypeScript（4 种）
- Baseline + Candidate + Snapshot
- 核心查询
- PostgreSQL
- 无 UI/MCP

**v0.2**：
- Go / Rust / JavaScript / C#
- Execution Projection
- 更多语言语义
- 性能强化

**P6 退出标准**：
- [ ] v0.1 4 种语言能力矩阵全部达标
- [ ] v0.2 4 种语言能力矩阵全部达标
- [ ] 无 MCP / 无 UI / 无 SQLite 生产依赖
- [ ] Coverage 可观测、故障可恢复

> **只有 P6 全部退出标准满足，才允许进入 OntoOS 对接。**

---

## 四、第二周期：拼接 OntoOS

> 正确优先级：**安全可控 → 结果可靠留存 → 提高 Agent 执行效率**。  
> 不能先给 Agent 增加可选认知工具，再补安全主链。

```
P7  Assurance GraphIntegrity / GraphRisk
        ↓
P8  Outbox Publisher → Redis Streams → 图谱投影
        ↓
P9  Runtime 可选 GraphRead 与策略注入
        ↓
P10 OpenCodeReview SemanticVerifier 与领域包
```

---

### P7：OntoAssure Graph Verifier 接入

**调用权完全属于 OntoAssure VerifierScheduler，不依赖 Agent 决策。**

#### GraphIntegrityVerifier (DeterministicVerifierPort)

**所有代码变更强制执行**：

- [ ] Candidate 已建立图谱快照
- [ ] 快照为 SEALED
- [ ] checkpoint hash 一致
- [ ] execution_generation 一致
- [ ] changed files 已处理
- [ ] required passes 覆盖完整

**图谱不可用时必须显式产生**：

```
ENVIRONMENT_ERROR
VERIFICATION_INCOMPLETE
STALE_SNAPSHOT
UNSUPPORTED_COVERAGE
```

**绝不能自动降级成 PASS。**

#### GraphRiskVerifier (DeterministicVerifierPort)

按策略调度：调用链影响、依赖闭包、受影响测试、架构边界、共享状态、语言专项风险（unsafe/反射/动态导入/函数指针表）、历史失败关联。

**调度规则**：

| 变更类型 | Graph Integrity | Graph Risk |
|----------|----------------|------------|
| README / 文档 | 跳过 | 跳过 |
| 普通配置 | 按规则 | 按规则 |
| 测试代码 | 必须 | Advisory |
| 普通业务代码 | 必须 | Required |
| 公共 API | 必须 | 强化 Required |
| 权限/支付/认证 | 必须 | 强化 Required + 语言专项 |
| 安全关键代码 | 必须 | 语言专项强化 |

**退出标准**：
- [ ] GraphIntegrity 对所有代码变更强制执行
- [ ] Verifier Scheduler 确定性调用
- [ ] STALE / PARTIAL / UNSUPPORTED 不能进入 Positive Evidence
- [ ] 每个 gap 显式记录在三本总账中

---

### P8：权威 Outbox 与图谱留存

**目标**：图谱投影不直接读取 OntoOS 数据库。

#### 8a. 解耦方案

```
OntoOS Settlement
    ↓ 同事务
onto_outbox (PostgreSQL)
    ↓
OntoOS Outbox Publisher (OntoOS 自己负责发布)
    ↓
Redis Streams: ontoos.authoritative-events.v1
    ↓
OntoFirmwareGraph Projector (Consumer Group: ontofirmwaregraph-projector)
    ↓
Dead Letter: ontoos.authoritative-events.dlq
```

**关键边界**：
- OntoFirmwareGraph **不读取 OntoOS 数据库**
- OntoOS **自己负责发布权威事件**
- Redis Streams **只承担传输**（不是权威存储）
- PostgreSQL Outbox **仍是权威未发布记录**
- 图谱投影可**幂等重放**

#### 8b. 投影语义

```
COMMIT    → Candidate Graph 提升为 Canonical Graph
CONTINUE  → 不污染 Canonical，记录为 INCOMPLETE Episode
ROLLBACK  → 不污染 Canonical，进入 Failed Episode
ESCALATE  → 保持非 Canonical，标记 PendingAuthority
```

#### 8c. 投影保证

异步、幂等、持久重试（有退避）、可重放（从 Redis Stream offset）、有 Dead Letter、有监控。

**退出标准**：
- [ ] Outbox 与 Settlement 同事务（已有）
- [ ] Redis Streams 传输已接通
- [ ] COMMIT 才提升 Canonical / 失败不污染
- [ ] Dead Letter 有监控

---

### P9：Runtime 可选 GraphRead 与策略注入

**此时才增加 Runtime 图谱查询能力。**

#### RepositoryGraphReadCapability

Agent 可自主查询（只读），经 CapabilityGateway。

**策略注入**（确定性，非 Agent 决策）：
- 首次修改陌生模块 → 目标符号 + 一层 callers/callees
- 修改公共 API → 所有调用者
- 修改高风险文件 → 影响摘要
- 连续验证失败 → 相关测试和历史失败

**注入限制**：≤10 相关测试、≤5 最近失败、≤500 tokens 影响摘要。

**图谱查询失败不直接阻断 Agent 执行，但最终 Candidate 仍会在 P7 GraphIntegrityVerifier 被强制检查。**

**退出标准**：
- [ ] 图谱不可用不阻断 Agent 执行
- [ ] 查询经过 CapabilityGateway
- [ ] 上下文注入有大小和审计限制

---

### P10：OpenCodeReview Semantic Verifier 与领域包

```
SemanticReviewVerifier (impl SemanticVerifierPort)
  ├── 输入：Diff + Rules + OntoFirmwareGraph 上下文 + 历史执行记忆
  ├── 方式：调 ocr CLI（非 MCP、非 Agent 自主）
  └── 输出：FindingCandidate[]
```

**领域包**：

| Pack | 优先级 |
|------|--------|
| **Embedded Firmware Pack** | P10（最重要的领域增强） |
| C/C++ Legacy Pack | P10 |
| Java Enterprise Pack | 后期 |
| Web Security Pack | 后期 |
| Python Data Pack | 后期 |
| Rust Safety Pack | 后期 |

---

## 五、风险与依赖

### 技术风险

| 风险 | 缓解 |
|------|------|
| CBM main.c 交叉依赖 | P1 编译隔离 → P2 抽核 → P2.5 物理删除，逐阶段验证 |
| SQLite → PG 迁移 | GraphSink 抽象 + SQLiteCompatibilitySink 对照 |
| 混合 LSP headless 可用性 | P0 基线记录当前精度 |
| C Core 内存泄漏 | 子进程池 + max_requests 重启 |
| Merkle Hash 性能 | 分层 + 变化分区增量 |

### 工期估算（修正后）

| 阶段 | 工期 | 关键路径 |
|------|------|---------|
| P0 | 1–2 周 | Golden fixture |
| P1 | 2–3 周 | main.c ifdef 隔离 |
| P2 | 3–5 周 | GraphSink + 内核 API |
| P2.5 | 1–2 周 | 物理删除 + 验证 |
| P3 | 3–5 周 | Rust Service + 故障隔离 |
| P4 | 5–8 周 | PG schema + COPY + 查询 |
| P5 | 4–6 周 | Merkle + 状态机 + 一致性 |
| P6 | 4–8 周 | v0.1 (4 语言) → v0.2 (8 语言) |
| **第一周期合计** | **22–37 周** | |
| P7 | 2–3 周 | GraphIntegrity + GraphRisk |
| P8 | 2–3 周 | Outbox Publisher + Streams + Projector |
| P9 | 1–2 周 | Runtime GraphRead |
| P10 | 2–4 周 | OCR 集成 + Embedded Firmware Pack |
| **总计** | **29–49 周** | |

**15–21 周的旧估算成立的前提（不成立）**：只保证 4 种语言、不做 Cypher、不做 Execution Memory、不做 Embedding、不持续追踪上游。因此修正为 22–37 周，分两个发布点。

---

## 六、附录：删除与保留清单

### P2.5 物理删除

```
src/ui/                 # HTTP server, 3D layout, 嵌入式前端
graph-ui/               # React/Three.js 前端
src/mcp/                # MCP 全部
src/daemon/             # daemon 全部（8 层）
src/upgrade/            # 自动升级
src/telemetry/          # 遥测
src/cli/agent_clients.c       # 43 种 Agent 自动配置
src/cli/agent_profiles.c      # Agent profiles
src/cli/hook_augment.c        # Hooks/Skills 注入
src/cli/config_*.c            # 配置编辑
src/cli/activation_transaction.c  # install/uninstall/update
pkg/                    # npm/PyPI/Go 等分发包装
```

### 迁移到 devtools/（不进入生产二进制）

```
tools/tree-sitter-form     → devtools/grammars/
tools/tree-sitter-magma    → devtools/grammars/
```

### Cypher 拆分

```
保留：lexer, parser, AST, 只读查询规划语义
重写：SQLite executor, SQLite 表访问, FTS5 绑定
生产 API 完全领域化，Cypher 只保留为开发调试接口
```

### P4 完成后删除

```
src/store/                    # SQLite store
internal/cbm/sqlite_writer.c  # B-tree 页写入器
vendored/sqlite3/             # 嵌入式 SQLite
```

### 可选功能（默认关闭）

```
OCG_ENABLE_EMBEDDING=OFF     # semantic embedding, nomic
                              # 初期只做结构图谱和验证
                              # 需要语义搜索时再启用
```

### 贯穿始终保留

```
internal/cbm/grammar_*.c      # 158+ tree-sitter
internal/cbm/extract_*.c      # 提取器
internal/cbm/helpers.c        # AST 工具
internal/cbm/lang_specs.c     # 语言规格
internal/cbm/cbm.h / cbm.c    # 核心 API
internal/cbm/ac.c             # Aho-Corasick 多模式匹配
internal/cbm/lz4_store.c      # LZ4 压缩
internal/cbm/zstd_store.c     # Zstd 压缩
src/pipeline/                 # 全部 Pass（含 pass_c_fnptr_bindings.c）
src/graph_buffer/             # 内存图谱缓冲
src/discover/                 # 文件发现
src/foundation/               # 全部基础设施
src/semantic/                 # 11-signal 语义引擎（OFG_ENABLE_EMBEDDING=OFF 默认关闭）
src/simhash/                  # MinHash 结构相似度指纹
src/cypher/                   # Cypher lexer/parser/AST 保留
                              # SQLite executor 后续重写或领域化
src/git/                      # Git 集成（变更检测、diff）
src/watcher/                  # 文件监控（只用于 Baseline 预热）
vendored/mimalloc/            # 内存分配器
vendored/yyjson/              # JSON 解析
vendored/xxhash/              # 哈希
vendored/tre/                 # 正则
vendored/nomic/               # 嵌入向量（OFG_ENABLE_EMBEDDING=OFF 默认关闭）
```

---

> 文档版本：v2.0  
> 生成日期：2026-07-28  
> **第一周期预估：22–37 周（v0.1 约 14–22 周 + v0.2 约 8–15 周）**  
> **总计：29–49 周**  
> 基于代码现状：CBM main (构建已验证)、OntoOS (10 crates)、IronClaw (32 migrations)、OpenCodeReview (93 测试)
