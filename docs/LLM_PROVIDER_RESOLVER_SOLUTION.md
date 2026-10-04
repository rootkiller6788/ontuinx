# LLM Model Provider Resolver 解决方案

> 给 OntoRuntime 补一个缺失的 Model → Provider → Capability Resolver。  
> 不改架构，不换语言，不引入 Python 依赖。

---

## 一、问题根因

### 当前 OntoRuntime LLM 路由逻辑

```
providers.json  →  26 个厂商注册
registry.rs     →  遍历 providers，找到第一个有 API Key 的就选它
rig_adapter.rs  →  OntoRuntime ChatMessage ↔ rig message 转换
rig-core 0.33   →  每个厂商是独立的 Client 类型，无自动路由
```

### 两个独立问题

**问题 A：Provider 选择错误**

```
deepseek-v4-flash
→ OPENAI_API_KEY 被先匹配到
→ 走 openai provider (protocol=open_ai_completions)
→ rig-core OpenAI Client
→ 不理解 DeepSeek 的 reasoning_content
```

**问题 B：Reasoning Tool Roundtrip**

```
第一轮：assistant 返回 reasoning_content + tool_call
→ 工具执行
→ 第二轮：reasoning_content 未正确回传
→ DeepSeek HTTP 400
```

OntoRuntime 已有专用 `deepseek` provider（`protocol=deep_seek`，走 rig-core DeepSeek 专有 Client），description 明确写了 **"preserves reasoning_content for thinking-mode models"**。当前只是路由没走到它。

参考：
- [OntoRuntime LLM Providers 文档](https://github.com/nearai/ironclaw/blob/staging/docs/capabilities/llm-providers.md)
- [DeepSeek reasoning_content 400 issue](https://github.com/nearai/ironclaw/issues/3436)
- [LiteLLM 自定义 provider 模型发现](https://github.com/BerriAI/litellm/issues/20064)

---

## 二、三方路由方案对比

| 特性 | OpenHands (Python) | OntoRuntime (Rust) | Grok-Build (Rust) |
|------|-------------------|-----------------|-------------------|
| 路由方式 | litellm `get_llm_provider()` | providers.json + env var 扫描 | 硬编码单厂商 |
| 厂商数 | 100+ (litellm 内置) | 26 (自维护) | 1 (xAI 自家) |
| 填模型名自动路由 | ✅ | ❌ | ❌ |
| LLM 抽象层 | litellm | rig-core 0.33 | 无 |
| reasoning 支持 | litellm 原生处理 | rig-core DeepSeek 专有 Client | 自家 API |

**OpenHands 的 litellm 模式值得参考，但不能照抄：**
- litellm 是 Python 库，OntoRuntime 是 Rust，不应引入跨语言依赖
- litellm 有数千模型的巨大目录，OntoRuntime 只需要覆盖已支持的 26 个 Provider
- litellm 也不能可靠识别全部自定义/无前缀模型，未知模型仍可能要求显式 Provider

---

## 三、解决方案架构

### 不改动现有架构，只插入一个 Resolver

```
Agent Loop
    ↓
LLM Adapter (不变)
    ↓
ModelProviderResolver   ← 新增（300~500 行）
    ↓
ProviderFactory         ← 小改（50~100 行）
    ↓
rig-core (不变)
    ↓
OpenAI / DeepSeek / Claude / ...
```

OntoAssure 完全不感知 provider routing、reasoning_content 等 LLM 基础设施细节。

### 新增模块

```
ironclaw_llm/
├── provider_registry.rs        ← 现有
├── provider_resolver.rs        ← 新增：解析逻辑
├── provider_factory.rs         ← 新增：收敛 Client 构造
├── provider_roundtrip.rs       ← 新增：reasoning_content 等不透明状态
├── providers.json              ← 扩展：model_aliases, model_prefixes, capabilities
└── rig_adapter.rs              ← 小改：接收 ResolvedProvider
```

### 改动规模

```
新增：300~500 行 Rust
修改：200~400 行 Rust
总计：500~900 行 Rust（小功能扩展，非架构重构）
```

---

## 四、Provider 解析优先级

**原则：显式配置优先，模型推断只用于首次配置和无歧义场景。**

```text
优先级 1：用户显式 Provider（LLM_BACKEND=deepseek）
优先级 2：模型中的 Provider 前缀（deepseek/deepseek-v4-flash）
优先级 3：已保存的模型绑定（persisted model_bindings）
优先级 4：Endpoint Host 绑定（api.deepseek.com → deepseek）
优先级 5：精确模型 Alias（完整字符串匹配）
优先级 6：唯一模型家族匹配（^deepseek-.* 且只有一个候选）
优先级 7：无法唯一确定 → fail-fast（报错，不静默回退）
```

### 优先级 1：显式 Provider 最高

```bash
LLM_BACKEND=deepseek
LLM_MODEL=deepseek-v4-flash
→ provider_id=deepseek, protocol=deep_seek
```

模型名不能推翻用户显式选择。

### 优先级 2：Provider 前缀

```
deepseek/deepseek-v4-flash  → deepseek
openai/gpt-5.5              → openai
anthropic/claude-sonnet-4   → anthropic
openrouter/deepseek/deepseek-v3 → openrouter
```

注意：只拆最前面的已注册 Provider ID。OpenRouter 内部仍含 `/`，不按所有 `/` 拆分。

### 优先级 3：持久化绑定

首次解析成功后写入配置，下次直接使用：

```yaml
model_bindings:
  deepseek-v4-flash: deepseek
  my-local-deepseek: openai_compatible
```

### 优先级 4：Endpoint 判断

```
api.deepseek.com → deepseek
api.openai.com   → openai
openrouter.ai    → openrouter
localhost / 127.0.0.1 → 不自动判断，要求显式 provider
```

自定义 Gateway 不能根据模型名改路由：

```bash
LLM_BASE_URL=http://localhost:4000/v1
LLM_MODEL=deepseek-v4
→ 应走 openai_compatible，因为真正通信对象是本地 Gateway
```

### 优先级 5：精确 Alias

```json
{
  "id": "deepseek",
  "model_aliases": [
    "deepseek-chat",
    "deepseek-reasoner", 
    "deepseek-v4-flash",
    "deepseek-v4-pro"
  ]
}
```

### 优先级 6：模型家族匹配

```
^deepseek-.* → candidate: deepseek
```

需同时满足：存在对应 API Key、无自定义 Base URL、无其他 Provider 声明同一 Alias、匹配唯一。

### 优先级 7：歧义 fail-fast

```bash
DEEPSEEK_API_KEY=xxx
OPENROUTER_API_KEY=xxx
LLM_MODEL=deepseek-v4-flash
```

不"取第一个有 Key 的 Provider"。返回：

```
Model `deepseek-v4-flash` can be served by multiple configured providers:
- deepseek
- openrouter

Select one explicitly:
  LLM_BACKEND=deepseek
  or
  LLM_BACKEND=openrouter
```

---

## 五、核心类型定义

```rust
/// 解析后的 Provider 信息
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProvider {
    pub provider_id: ProviderId,
    pub protocol: ProviderProtocol,
    pub model: String,
    pub source: ResolutionSource,
    pub confidence: ResolutionConfidence,
}

/// 解析来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionSource {
    ExplicitBackend,     // LLM_BACKEND=deepseek
    QualifiedModel,      // deepseek/deepseek-v4-flash
    PersistedBinding,    // model_bindings 缓存
    EndpointHost,        // api.deepseek.com
    ExactAlias,          // model_aliases 精确匹配
    ModelFamily,         // ^deepseek-.* 前缀匹配
}

/// 置信度
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionConfidence {
    Explicit,   // 用户明确指定
    Exact,      // 精确匹配
    Inferred,   // 推断（ModelFamily）
}

/// 解析请求
pub struct ProviderResolutionRequest<'a> {
    pub explicit_provider: Option<&'a str>,
    pub model: &'a str,
    pub base_url: Option<&'a str>,
    pub available_credentials: &'a CredentialAvailability,
    pub persisted_binding: Option<&'a ProviderId>,
}

/// 解析接口
pub trait ModelProviderResolver {
    fn resolve(
        &self,
        request: ProviderResolutionRequest<'_>,
    ) -> Result<ResolvedProvider, ProviderResolutionError>;
}
```

### 解析伪代码

```rust
pub fn resolve_provider(
    request: &ProviderResolutionRequest<'_>,
    registry: &ProviderRegistry,
) -> Result<ResolvedProvider, ProviderResolutionError> {
    // 1. 显式 Provider
    if let Some(provider) = request.explicit_provider {
        return registry.resolve_explicit(provider, request.model);
    }

    // 2. Provider 前缀
    if let Some(result) = registry.resolve_qualified_model(request.model)? {
        return Ok(result);
    }

    // 3. 持久化绑定
    if let Some(binding) = request.persisted_binding {
        return registry.resolve_binding(binding, request.model);
    }

    // 4. Endpoint Host
    if let Some(base_url) = request.base_url {
        if let Some(result) = registry.resolve_endpoint(base_url)? {
            return Ok(result);
        }
    }

    // 5. 精确 Alias
    if let Some(result) = registry.resolve_exact_alias(request.model)? {
        return Ok(result);
    }

    // 6. 模型家族
    let candidates = registry.resolve_model_family(
        request.model,
        request.available_credentials,
    );

    match candidates.as_slice() {
        [single] => Ok(single.clone()),
        [] => Err(ProviderResolutionError::NoMatch),
        many => Err(ProviderResolutionError::Ambiguous {
            candidates: many.to_vec(),
        }),
    }
}
```

---

## 六、ProviderFactory — 构造真正的专用 Client

Resolver 只负责选择，Factory 负责构造正确的 rig Client：

```rust
match resolved.protocol {
    ProviderProtocol::DeepSeek => {
        build_deepseek_client(credentials, resolved.model)
    }
    ProviderProtocol::OpenAi => {
        build_openai_client(credentials, resolved.model)
    }
    ProviderProtocol::OpenAiCompatible => {
        build_openai_compatible_client(
            credentials,
            base_url.ok_or(BuildError::MissingBaseUrl)?,
            resolved.model,
        )
    }
    ProviderProtocol::Anthropic => {
        build_anthropic_client(credentials, resolved.model)
    }
    // ... existing providers
}
```

**关键：不能** `deepseek` provider 只改 Base URL 走 OpenAI Client。必须构造 rig-core 的专用 `deepseek::Client`。

启动日志必须输出：

```text
provider_resolution:
  provider_id=deepseek
  protocol=deep_seek
  model=deepseek-v4-flash
  source=exact_alias
  confidence=exact
  client_type=rig::providers::deepseek::Client
```

---

## 七、Reasoning Roundtrip 设计

即使 Provider 路由正确，也必须验证 reasoning_content 在两轮请求间正确传递。

### 不透明 Provider 状态（推荐方案）

不要把 DeepSeek 的 `reasoning_content` 直接写进 OntoRuntime 核心 Message：

```rust
/// 不透明 Provider 状态，只用于协议 roundtrip
pub struct ProviderRoundtripState {
    pub provider: ProviderId,
    pub opaque_payload: serde_json::Value,
}
```

生命周期：

```text
Rig/provider 响应
→ 提取 reasoning_content 等字段 → ProviderRoundtripState
→ 与 assistant tool-call message 绑定
→ 下一轮请求时，对应 Provider serializer 重新注入
→ 不进入 Prompt、Memory、Agent 可见内容、普通日志、Onto Evidence
```

**要求：**
- 不进入 Prompt
- 不进入 Memory
- 不向 Agent 暴露
- 不写普通日志
- 不进入 Onto Evidence
- 只用于 Provider 协议 roundtrip

---

## 八、providers.json 扩展

```json
{
  "id": "deepseek",
  "protocol": "deep_seek",
  "api_key_env": "DEEPSEEK_API_KEY",
  "model_env": "DEEPSEEK_MODEL",
  "default_model": "deepseek-chat",

  "model_aliases": [
    "deepseek-chat",
    "deepseek-reasoner",
    "deepseek-v4-flash",
    "deepseek-v4-pro"
  ],

  "model_prefixes": [
    "deepseek-"
  ],

  "base_url_hosts": [
    "api.deepseek.com"
  ],

  "capabilities": {
    "tool_calling": true,
    "reasoning": true,
    "reasoning_roundtrip": true
  }
}
```

其他 Provider 逐步补：

```json
{
  "id": "anthropic",
  "model_prefixes": ["claude-"],
  "model_aliases": [
    "claude-sonnet-4-20250514",
    "claude-opus-4-20250514"
  ],
  "capabilities": {
    "tool_calling": true,
    "reasoning": true
  }
}
```

第一版只覆盖 OntoRuntime 已支持的 26 个 Provider。不维护 LiteLLM 式的数千模型目录。

---

## 九、测试矩阵

### 1. Router 单元测试

| # | 场景 | 预期 |
|---|------|------|
| R1 | 显式 `LLM_BACKEND=deepseek` | → deepseek |
| R2 | `deepseek/deepseek-v4-flash` | → deepseek |
| R3 | 精确 alias `deepseek-v4-flash` | → deepseek |
| R4 | 自定义 Base URL + deepseek 模型名 | → openai_compatible |
| R5 | 同时有 DeepSeek/OpenRouter 凭据 | → Ambiguous Error |
| R6 | 显式 openrouter 覆盖模型家族推断 | → openrouter |
| R7 | 未知模型 | → NoMatch，不静默回退 |
| R8 | 持久化绑定优先于环境扫描 | → persisted |
| R9 | localhost endpoint | → 不自动判断 |

### 2. ProviderFactory 测试

| # | 场景 | 预期 |
|---|------|------|
| F1 | protocol=deep_seek | 构造 DeepSeek 专用 Client |
| F2 | protocol=open_ai_completions | 构造 OpenAI Client |
| F3 | protocol=anthropic | 构造 Anthropic Client |

### 3. DeepSeek reasoning roundtrip 测试

使用本地 HTTP Mock Server 捕获两轮请求。

第一轮响应包含 `reasoning_content` + `tool_calls`。第二轮必须断言：

```text
assistant message:
  reasoning_content_present = true
  tool_calls_present = true

tool message:
  tool_call_id 匹配
```

不断言或打印具体思维内容，只断言存在性、长度、hash。

### 4. OntoRuntime 真实 E2E

```text
DeepSeek
→ tool call
→ BeforeCapability (Phase 3 hook)
→ CapabilityHost
→ AfterCapability (Phase 4 hook)
→ RuntimeObservation
→ AfterLoopExit
→ OntoAssure Finalization + Authority Enforcement
```

完成后才把 Phase 3+4 标为 ✅。

---

## 十、实施顺序

### Step 1：诊断日志（立即）

给 Provider 选择增加诊断日志：`provider_id / protocol / model / source / client_type`

### Step 2：显式路由测试

```bash
LLM_BACKEND=deepseek
DEEPSEEK_API_KEY=...
DEEPSEEK_MODEL=deepseek-v4-flash
```

确认构造的是 DeepSeek 专用 Client。

### Step 3：清理旧绑定

清除数据库中的旧 provider 绑定，确认运行时不再回退到 openai provider。

### Step 4：Reasoning roundtrip 测试

使用本地 Mock Server 测试两轮 reasoning + tool call 的完整性。

### Step 5：判定根因

- **如果专用 Client 通过**：根因 = OntoRuntime 路由。实现 ModelProviderResolver。
- **如果专用 Client 仍失败**：根因 = Rig/消息转换。补 rig-core 测试并修复，临时 Cargo patch。

### Step 6：实现 Phase 1 Provider Resolver

```text
Phase 1（立即）：DeepSeek alias + 显式日志 + 测试
Phase 2（后续）：补 claude-, gpt-, gemini-, mistral- 等常见前缀
Phase 3（远期）：model capability registry，给 Onto 做能力约束
```

### Step 7：真实 E2E

完成 Phase 3+4 运行时验证。

---

## 十一、用户体验

用户只需输入：

```
Model: deepseek-v4-flash
API Key: sk-...
```

Onboarding 执行：

```text
模型名
→ Resolver 给出推荐 Provider
→ UI 显示：

  Detected provider: DeepSeek
  Protocol: Native DeepSeek
  Reasoning tool roundtrip: supported
```

用户确认后持久化：

```bash
LLM_BACKEND=deepseek
LLM_MODEL=deepseek-v4-flash
```

**自动推断只发生在配置阶段，运行时使用已持久化的明确 Provider。**

---

## 十二、改动不影响的部分

```
✅ Agent Loop          — 不变
✅ Capability Runtime  — 不变
✅ Hook System         — 不变
✅ OntoAssure          — 不变
✅ Conversation        — 不变
✅ EventStore          — 不变
✅ rig-core            — 不变（只通过 ProviderFactory 选择正确的 Client）
```

**这是给 OntoRuntime LLM Adapter 补一个缺失的路由层，不是架构重构。**
