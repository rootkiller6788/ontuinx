## 5. 代码领域实现 — onto-code-pack

### 5.1 目录结构

```
crates/onto-code-pack/src/
├── lib.rs                     # LanguageRegistry 初始化 + 注册所有内置 profiles
├── scope/
│   ├── mod.rs
│   ├── diff.rs                # git diff → VerificationTarget[] (带语言路由)
│   ├── repository_scan.rs     # 全仓库扫描 → VerificationTarget[]
│   └── polyglot.rs            # 多语言仓库: 按语言分片
├── rules/
│   ├── mod.rs                 # RulePackLoader: 按 language 加载规则包
│   ├── rust.rs                # Rust 规则包 (clippy + unsafe + ownership)
│   ├── go.rs                  # Go 规则包 (goroutine leak + nil pointer + defer)
│   ├── python.rs              # Python 规则包 (type safety + async + security)
│   ├── typescript.rs          # TS 规则包 (null safety + promise + injection)
│   ├── java.rs                # Java 规则包 (concurrency + SQL injection + JPA)
│   ├── cpp.rs                 # C++ 规则包 (memory safety + UB + RAII)
│   ├── protobuf.rs            # Protobuf 规则包 (breaking changes + naming)
│   └── generic.rs             # 通用规则包 (所有语言通用的安全/架构规则)
├── verifiers/
│   ├── mod.rs                 # VerifierRegistry: 按 LanguageProfile 选择 Verifier
│   ├── deterministic/
│   │   ├── mod.rs
│   │   ├── build.rs           # 分发: cargo build / go build / tsc / mvn / cmake
│   │   ├── test.rs            # 分发: cargo test / go test / pytest / jest
│   │   ├── lint.rs            # 分发: clippy / golangci-lint / ruff / eslint
│   │   ├── format_check.rs    # 分发: rustfmt / gofmt / black / prettier
│   │   ├── sast.rs            # 分发: semgrep / codeql / bandit / gosec
│   │   └── dependency_audit.rs# 分发: cargo-deny / govulncheck / safety / npm audit
│   ├── semantic/
│   │   ├── mod.rs
│   │   ├── ironclaw_semantic.rs # IronClaw → SemanticVerifierPort (原全能代理)
│   │   ├── generic_llm.rs     # 通用 LLM Verifier (无 IronClaw 时，Claude/GPT 直连)
│   │   └── prompt_builder.rs  # 按语言构建提示: Rust prompt ≠ Go prompt ≠ Python prompt
│   └── registry.rs            # VerifierRegistry: LanguageProfile → [Verifier]
├── location/
│   ├── mod.rs
│   ├── source_code.rs         # 行号定位 (移植自 OCR diff/resolver.go)
│   ├── ast.rs                 # AST 节点定位: tree-sitter (支持所有语言)
│   └── symbol.rs              # 符号级定位: LSP / ctags
└── session/
    ├── mod.rs
    └── jsonl.rs               # JSONL 持久化 (移植自 OCR session/persist.go)
```

### 5.2 文件: `src/lib.rs` — 多语言初始化

```rust
//! onto-code-pack — 代码领域的 Verification Fabric 实现
//!
//! 职责:
//!   1. 注册所有支持语言的 LanguageProfile
//!   2. 注册所有确定性 Verifier 实现
//!   3. 注册语义 Verifier 实现 (IronClaw + 通用 LLM)
//!   4. 暴露统一的 VerificationCoordinator

use onto_assurance_types::language_profile::{LanguageProfile, LanguageRegistry};

pub mod scope;
pub mod rules;
pub mod verifiers;
pub mod location;
pub mod session;

/// 构建默认的 LanguageRegistry — 包含所有内置语言。
pub fn default_language_registry() -> LanguageRegistry {
    LanguageRegistry::new(LanguageProfile::all_builtins())
}

/// 构建仅包含指定语言的 LanguageRegistry。
pub fn language_registry_for(languages: &[&str]) -> LanguageRegistry {
    let all = LanguageProfile::all_builtins();
    let filtered: Vec<_> = all
        .into_iter()
        .filter(|p| languages.contains(&p.language.as_str()))
        .collect();
    LanguageRegistry::new(filtered)
}
```

### 5.3 文件: `src/scope/diff.rs` — 带语言路由的 Diff Scope Provider

```rust
//! Git diff → VerificationTarget[] (带语言路由)
//! 移植自 OCR internal/diff/git.go + 增强多语言支持

use onto_assurance_types::language_profile::LanguageRegistry;
use onto_assurance_types::verification_target::{TargetKind, VerificationTarget};
use onto_assurance_types::hash::ContentHash;
use onto_assurance_types::ids::VerificationTargetId;

pub struct DiffScopeProvider {
    repo_dir: String,
    from_ref: Option<String>,
    to_ref: Option<String>,
    commit: Option<String>,
    /// 语言注册表 — 用于自动检测文件语言
    language_registry: LanguageRegistry,
}

impl DiffScopeProvider {
    pub fn new_workspace(repo_dir: &str, registry: LanguageRegistry) -> Self {
        Self { repo_dir: repo_dir.to_string(), from_ref: None, to_ref: None,
               commit: None, language_registry: registry }
    }

    pub fn new_range(repo_dir: &str, from: &str, to: &str, registry: LanguageRegistry) -> Self {
        Self { repo_dir: repo_dir.to_string(), from_ref: Some(from.into()),
               to_ref: Some(to.into()), commit: None, language_registry: registry }
    }

    pub fn new_commit(repo_dir: &str, commit: &str, registry: LanguageRegistry) -> Self {
        Self { repo_dir: repo_dir.to_string(), from_ref: None, to_ref: None,
               commit: Some(commit.into()), language_registry: registry }
    }

    /// 枚举变更文件 — 返回 VerificationTarget 列表 (带语言信息)
    pub fn enumerate(&self) -> Result<Vec<VerificationTarget>, DiffError> {
        let files = self.run_git_diff()?;
        let ignored = self.aggregate_ignored_patterns();
        let targets: Vec<_> = files
            .into_iter()
            .filter(|f| !ignored.iter().any(|i| f.starts_with(i)))
            .filter_map(|path| self.file_to_target(&path))
            .collect();

        // 统计语言分布 (用于日志/遥测)
        let mut lang_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for t in &targets {
            if let Some(lang) = &t.language {
                *lang_counts.entry(lang.clone()).or_default() += 1;
            }
        }
        tracing::info!(targets = targets.len(), ?lang_counts, "diff scope enumerated");

        Ok(targets)
    }

    fn file_to_target(&self, path: &str) -> Option<VerificationTarget> {
        let full_path = std::path::Path::new(&self.repo_dir).join(path);
        let content = std::fs::read(&full_path).ok()?;

        // 二进制检测 — 移植自 OCR scan provider
        if content.iter().take(8000).any(|&b| b == 0x00) {
            return None;
        }

        // 语言检测 — 通过 LanguageRegistry
        let profile = self.language_registry.resolve_by_path(path);
        let language = profile.map(|p| p.language.clone());

        // 大小检查 — 按语言的 token/byte 比例计算上限
        let tokens_per_byte = profile.map(|p| p.tokens_per_byte).unwrap_or(1.5);
        let max_bytes = (200_000.0 / tokens_per_byte) as u64; // ~200K tokens max
        if content.len() as u64 > max_bytes {
            return None;
        }

        let hash = ContentHash::from_bytes(&sha2::Sha256::digest(&content));

        Some(VerificationTarget {
            target_id: VerificationTargetId::new(),
            target_kind: TargetKind::SourceFile,
            target_ref: path.to_string(),
            content_hash: hash,
            size_bytes: content.len() as u64,
            language,
            risk_level: onto_assurance_types::enums::RiskLevel::Medium,
            metadata: serde_json::json!({
                "extension": std::path::Path::new(path)
                    .extension().and_then(|e| e.to_str()),
            }),
        })
    }

    /// 聚合所有语言的 ignore_patterns + 通用忽略路径
    fn aggregate_ignored_patterns(&self) -> Vec<String> {
        let mut patterns = vec![
            ".idea/".into(), ".vscode/".into(), ".svn/".into(), ".git/".into(),
        ];
        for lang in self.language_registry.languages() {
            if let Some(p) = self.language_registry.get(lang) {
                patterns.extend(p.ignore_patterns.clone());
            }
        }
        patterns
    }

    fn run_git_diff(&self) -> Result<Vec<String>, DiffError> {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-C").arg(&self.repo_dir).arg("diff").arg("--name-only");

        if let Some(commit) = &self.commit {
            cmd.arg(&format!("{}^..{}", commit, commit));
        } else if let (Some(from), Some(to)) = (&self.from_ref, &self.to_ref) {
            cmd.arg(&format!("{}..{}", from, to));
        } else {
            cmd.arg("--staged");
        }

        let output = cmd.output().map_err(|e| DiffError::Git(e.to_string()))?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines().map(|s| s.to_string())
            .filter(|s| !s.is_empty()).collect())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiffError {
    #[error("git error: {0}")]
    Git(String),
    #[error("not a git repository: {0}")]
    NotGitRepo(String),
}
```

### 5.4 各语言规则包 — 具体规则内容

#### 5.4.1 `src/rules/rust.rs`

```rust
//! Rust 规则包 — 移植自 IronClaw 的 review-discipline.md + clippy 规则

use onto_assurance_types::rule_binding::{RulePack, Rule, RuleContent};
use onto_assurance_types::ids::{RulePackId, VerifierId, RuleId};
use onto_assurance_types::hash::{ContentHash, HashDomain, HashPurpose};
use onto_assurance_core::canonical;

pub fn rust_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "no-unwrap-in-production".into(),
            description: "禁止在非测试代码中使用 .unwrap() / .expect()".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "不允许 .unwrap() 和 .expect() 出现在 src/ 的任何文件中。\
                       使用 thiserror 定义错误类型，用 ? 传播错误，\
                       用 match 或 if let 处理 Option。测试代码中的 unwrap 是允许的。".into(),
                examples: vec![
                    "Bad: let x = foo().unwrap();".into(),
                    "Good: let x = foo().map_err(|e| MyError::Foo(e))?;".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "unsafe-audit".into(),
            description: "所有 unsafe 块必须有 SAFETY: 注释".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "每个 unsafe 块必须包含 SAFETY: 注释，解释为什么该块是安全的。\
                       如果你不确定是否可以移除 unsafe，请标记为需要人工审查。".into(),
                examples: vec![
                    "// SAFETY: This pointer is valid because...".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "trait-impl-consistency".into(),
            description: "双后端 (PostgreSQL + libSQL) 的 trait 实现必须一致".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "如果代码库有双后端 (如 PostgreSQL 和 libSQL)，\
                       对 trait 的任何修改必须在两个实现中同步更新。\
                       检查 postgres.rs 和 libsql_backend.rs。".into(),
                examples: vec![],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "clippy-all-warnings".into(),
            description: "cargo clippy --all-targets --all-features -- -D warnings".into(),
            category: "style".into(),
            content: RuleContent::Pattern {
                pattern_type: "clippy".into(),
                pattern: "clippy::all,clippy::pedantic,clippy::nursery".into(),
            },
        },
    ];

    let pack_hash = compute_pack_hash(&rules);
    RulePack {
        pack_id: RulePackId::new(),
        pack_hash,
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}

fn compute_pack_hash(rules: &[Rule]) -> ContentHash {
    let domain = HashDomain::new(HashPurpose::Content, "RULE_PACK");
    canonical::compute_hash(rules, &domain).expect("rule pack hash infallible")
}
```

#### 5.4.2 `src/rules/go.rs`

```rust
//! Go 规则包 — 从 OpenCodeReview 的 Go 审查经验 + golangci-lint 规则

pub fn go_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "goroutine-leak".into(),
            description: "goroutine 泄漏 — 确保每个 goroutine 有明确的退出路径".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查每个 goroutine:\n\
                       1. 是否有 context.Context 可以取消?\n\
                       2. 是否有 channel close 信号?\n\
                       3. 是否有 select + default 防止阻塞?\n\
                       特别注意: HTTP handler 中启动的 goroutine 必须在请求结束时退出。".into(),
                examples: vec![
                    "Bad: go func() { for { doWork() } }()  // never exits".into(),
                    "Good: go func() { for { select { case <-ctx.Done(): return } } }()".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "nil-interface-vs-nil-concrete".into(),
            description: "接口 nil ≠ 具体类型 nil — 返回 interface 的函数容易出错".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "Go 中 interface nil 和 concrete type nil 是不同的:\n\
                       var x *MyType = nil; var i interface{} = x; // i != nil!\n\
                       检查所有返回 interface 类型的函数，确保返回的 nil 是真正的 nil。".into(),
                examples: vec![
                    "Bad: func get() io.Writer { var f *os.File; return f }  // non-nil".into(),
                    "Good: func get() io.Writer { return nil }".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "defer-in-loop".into(),
            description: "循环中的 defer — 在函数结束时才执行，不是迭代结束".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "在 for 循环中使用 defer 会导致资源在函数退出时才释放。\
                       需要立即清理时用匿名函数包装。".into(),
                examples: vec![
                    "Bad: for _, f := range files { f,_ := os.Open(f); defer f.Close() }".into(),
                    "Good: for _, f := range files { func() { f,_ := os.Open(f); defer f.Close() }() }".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "error-not-checked".into(),
            description: "未检查的 error 返回值 — 不允许用 _ 忽略 error".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有返回 error 的函数调用必须检查错误。\
                       不允许使用 _ 忽略 error (除非有明确的注释说明原因)。".into(),
                examples: vec![
                    "Bad: data, _ := ioutil.ReadAll(r)".into(),
                    "Good: data, err := ioutil.ReadAll(r); if err != nil { return err }".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "context-propagation".into(),
            description: "Context 传播 — 所有 I/O 操作都必须传递 context".into(),
            category: "performance".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有网络调用、数据库查询、gRPC/HTTP 调用必须传递 context。\
                       不允许在请求处理器中使用 context.Background()。".into(),
                examples: vec![
                    "Bad: db.Query(\"SELECT ...\")".into(),
                    "Good: db.QueryContext(ctx, \"SELECT ...\")".into(),
                ],
            },
        },
    ];

    RulePack {
        pack_id: RulePackId::new(),
        pack_hash: compute_pack_hash(&rules),
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}
```

#### 5.4.3 `src/rules/python.rs`

```rust
//! Python 规则包 — 从 OCR 的 Python 审查经验 + ruff/bandit 规则

pub fn python_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "type-safety".into(),
            description: "缺少类型注解 — 公共 API 必须有完整类型注解".into(),
            category: "maintainability".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有公共函数和方法必须有类型注解 (参数 + 返回值)。\
                       使用 typing: Optional, Union, TypeVar, Protocol。\
                       不通过 mypy --strict 的代码不应合并。".into(),
                examples: vec![
                    "Bad: def process(data):".into(),
                    "Good: def process(data: list[dict[str, Any]]) -> Result[User, AppError]:".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "async-await-correctness".into(),
            description: "协程泄漏、事件循环阻塞、不正确的并发".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查异步代码:\n\
                       1. 是否在 async def 中用了 time.sleep() 而不是 asyncio.sleep()?\n\
                       2. Task/协程是否被正确 await 或 gather?\n\
                       3. 是否有未关闭的 aiohttp session?\n\
                       4. 异步上下文管理器是否正确处理了异常?".into(),
                examples: vec![
                    "Bad: time.sleep(1)  # blocks event loop in async function".into(),
                    "Good: await asyncio.sleep(1)".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "sql-injection".into(),
            description: "SQL 注入 — 禁止字符串拼接 SQL".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "所有 SQL 查询必须使用参数化查询。\
                       禁止 f-string 或 .format() 拼接 SQL。".into(),
                examples: vec![
                    "Bad: cursor.execute(f\"SELECT * FROM users WHERE id = {uid}\")".into(),
                    "Good: cursor.execute(\"SELECT * FROM users WHERE id = %s\", (uid,))".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "broad-except".into(),
            description: "过度宽泛的 except — 不能裸捕获所有异常".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "不要使用裸 except: 或 except Exception:。\
                       至少让 KeyboardInterrupt 和 SystemExit 传播。".into(),
                examples: vec![
                    "Bad: except: pass".into(),
                    "Good: except ValueError as e: logger.warning(...); raise".into(),
                ],
            },
        },
    ];

    RulePack {
        pack_id: RulePackId::new(),
        pack_hash: compute_pack_hash(&rules),
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}
```

#### 5.4.4 `src/rules/typescript.rs`

```rust
//! TypeScript 规则包

pub fn typescript_rule_pack(
    lint_verifier_id: VerifierId,
    semantic_verifier_id: VerifierId,
) -> RulePack {
    let rules = vec![
        Rule {
            rule_id: RuleId::new(),
            name: "null-safety".into(),
            description: "空值安全 — 避免 undefined is not an object 崩溃".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查 null/undefined 安全:\n\
                       1. 使用 optional chaining (?.) 和 nullish coalescing (??)\n\
                       2. 不要在类型断言 (as / !) 中绕过 null 检查\n\
                       3. Promise.catch 必须处理 rejection\n\
                       4. API response 的 shape 必须在访问前验证".into(),
                examples: vec![
                    "Bad: const name = response.data.user.name;  // may crash".into(),
                    "Good: const name = response?.data?.user?.name ?? 'Unknown';".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "xss-injection".into(),
            description: "XSS 注入 — 用户输入不应直接渲染为 HTML".into(),
            category: "security".into(),
            content: RuleContent::NaturalLanguage {
                text: "检查所有渲染用户输入的地方:\n\
                       1. dangerouslySetInnerHTML 必须先消毒\n\
                       2. innerHTML / document.write 必须消毒输入\n\
                       3. URL 参数必须 encodeURIComponent".into(),
                examples: vec![
                    "Bad: <div dangerouslySetInnerHTML={{__html: userInput}} />".into(),
                    "Good: <div>{sanitize(userInput)}</div>".into(),
                ],
            },
        },
        Rule {
            rule_id: RuleId::new(),
            name: "promise-unhandled".into(),
            description: "未处理的 Promise — 必须有 .catch 或 try/catch".into(),
            category: "correctness".into(),
            content: RuleContent::NaturalLanguage {
                text: "每个 Promise 必须处理失败:\n\
                       1. async/await → try/catch 包裹\n\
                       2. .then() → 必须有 .catch()\n\
                       3. Promise.all: 一个失败时其他仍在运行\n\
                       4. 不要在 forEach 中用 async (不会等待)".into(),
                examples: vec![
                    "Bad: items.forEach(async (item) => { await process(item); });".into(),
                    "Good: await Promise.all(items.map(item => process(item)));".into(),
                ],
            },
        },
    ];

    RulePack {
        pack_id: RulePackId::new(),
        pack_hash: compute_pack_hash(&rules),
        version: "1.0.0".into(),
        rules,
        applicable_target_kinds: vec![
            onto_assurance_types::verification_target::TargetKind::SourceFile,
        ],
        required_verifier_ids: vec![lint_verifier_id, semantic_verifier_id],
        default_severity: onto_assurance_types::finding::Severity::Medium,
    }
}
```

### 5.5 文件: `src/verifiers/deterministic/lint.rs` — 多语言 Lint 分发

```rust
//! Lint Verifier — 按语言分发的确定性 Lint 检查
//!
//! 每种语言有各自的原生 lint 工具:
//!   Rust → clippy
//!   Go   → golangci-lint
//!   Python → ruff
//!   TS/JS → eslint
//!   Java → checkstyle
//!   C++ → clang-tidy
//!   Proto → buf lint
//!   SQL → sqlfluff

use async_trait::async_trait;
use onto_assurance_runtime::verification::{
    DeterministicVerifierPort, DeterministicVerificationRequest,
    DeterministicVerifierError, VerifierCapability,
};
use onto_assurance_types::finding::{FindingCandidate, Severity, EvidenceLocationClaim};
use onto_assurance_types::ids::{FindingId, RuleId, VerifierId};
use onto_assurance_types::hash::ContentHash;

pub struct MultiLanguageLintVerifier {
    capability: VerifierCapability,
    verifier_id: VerifierId,
}

impl MultiLanguageLintVerifier {
    pub fn new() -> Self {
        let vid = VerifierId::new();
        Self {
            verifier_id: vid,
            capability: VerifierCapability {
                verifier_id: vid,
                verifier_version: "1.0.0".into(),
                supported_target_kinds: vec![
                    onto_assurance_types::verification_target::TargetKind::SourceFile,
                ],
                supported_languages: vec![
                    "rust".into(), "go".into(), "python".into(),
                    "typescript".into(), "javascript".into(),
                    "java".into(), "cpp".into(), "protobuf".into(), "sql".into(),
                ],
                max_target_size_bytes: 10 * 1024 * 1024, // 10 MiB
                supports_batching: true,
                max_batch_size: 200,
                cost_per_token_microcents: 0, // 确定性检查，无 LLM token 成本
            },
        }
    }

    /// 按语言路由到正确的 lint 命令和参数。
    fn lint_command_for(&self, lang: &str) -> Option<(&str, Vec<&str>)> {
        match lang {
            "rust" => Some(("cargo", vec!["clippy", "--all-targets", "--all-features",
                "--", "-D", "warnings"])),
            "go" => Some(("golangci-lint", vec!["run", "--out-format", "json"])),
            "python" => Some(("ruff", vec!["check", "--output-format", "json"])),
            "typescript" | "javascript" => Some(("npx", vec!["eslint", "--format", "json"])),
            "java" => Some(("mvn", vec!["checkstyle:check", "-q"])),
            "cpp" | "c" => Some(("clang-tidy", vec![])), // targets passed separately
            "protobuf" => Some(("buf", vec!["lint", "--format", "json"])),
            "sql" => Some(("sqlfluff", vec!["lint", "--format", "json"])),
            _ => None,
        }
    }
}

#[async_trait]
impl DeterministicVerifierPort for MultiLanguageLintVerifier {
    fn capability(&self) -> &VerifierCapability {
        &self.capability
    }

    async fn verify(
        &self,
        request: DeterministicVerificationRequest,
    ) -> Result<Vec<FindingCandidate>, DeterministicVerifierError> {
        // 1. 按语言分组 targets
        // 2. 对每个语言组执行 lint 命令
        // 3. 解析每个语言的输出 → FindingCandidate
        // 4. 合并所有结果
        let mut all_findings = Vec::new();

        // 检测语言 (从第一个 target 的扩展名推断)
        // 生产代码中应从 LanguageRegistry 获取
        let lang = detect_language_from_targets(&request.unit.target_refs);

        if let Some((cmd, args)) = self.lint_command_for(&lang) {
            let mut command = std::process::Command::new(cmd);
            command.args(&args);
            command.current_dir(&request.workspace_path);

            let output = tokio::time::timeout(
                std::time::Duration::from_millis(request.timeout_ms),
                tokio::process::Command::from(command).output(),
            ).await
                .map_err(|_| DeterministicVerifierError::Timeout(request.timeout_ms))?
                .map_err(|e| DeterministicVerifierError::ExecutionFailed(e.to_string()))?;

            let findings = parse_lint_output(
                &lang,
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&output.stderr),
            );
            all_findings.extend(findings);
        }

        Ok(all_findings)
    }
}

fn detect_language_from_targets(targets: &[String]) -> String {
    // 按多数语言判定
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for t in targets {
        let ext = std::path::Path::new(t).extension().and_then(|e| e.to_str()).unwrap_or("");
        let lang = match ext {
            "rs" => "rust",
            "go" => "go",
            "py" | "pyi" => "python",
            "ts" | "tsx" | "js" | "jsx" | "mjs" => "typescript",
            "java" => "java",
            "c" | "h" | "cpp" | "hpp" | "cc" => "cpp",
            "proto" => "protobuf",
            "sql" => "sql",
            _ => "unknown",
        };
        *counts.entry(lang).or_default() += 1;
    }
    counts.into_iter().max_by_key(|(_, c)| *c)
        .map(|(l, _)| l.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn parse_lint_output(lang: &str, stdout: &str, _stderr: &str) -> Vec<FindingCandidate> {
    match lang {
        "rust" => parse_clippy_output(stdout),
        "go" => parse_golangci_lint(stdout),
        "python" => parse_ruff_output(stdout),
        "typescript" | "javascript" => parse_eslint_output(stdout),
        _ => vec![],
    }
}

fn parse_clippy_output(stdout: &str) -> Vec<FindingCandidate> {
    let mut findings = Vec::new();
    for line in stdout.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("");
            if reason != "compiler-message" {
                continue;
            }
            let msg = &v["message"];
            let spans = msg["spans"].as_array();
            let primary = spans.and_then(|s| s.first());
            let file = primary
                .and_then(|s| s["file_name"].as_str()).unwrap_or("");
            let line_num = primary
                .and_then(|s| s["line_start"].as_u64()).unwrap_or(0) as u32;
            let text = msg["message"].as_str().unwrap_or("");

            findings.push(FindingCandidate {
                finding_id: FindingId::new(),
                verifier_id: VerifierId::new(),
                rule_id: RuleId::new(),
                target_ref: file.into(),
                target_content_hash: ContentHash::new(""),
                claimed_location: Some(EvidenceLocationClaim {
                    path: file.into(),
                    start_line: Some(line_num),
                    end_line: Some(line_num),
                    code_snippet: None,
                    symbol_name: None,
                    byte_offset: None,
                }),
                title: format!("clippy: {}", text),
                description: text.into(),
                severity: Severity::Medium,
                verifier_confidence: 1.0, // 确定性检查 = 100% 置信
                suggested_fix: None,
                rationale: None,
                verifier_metadata: v.clone(),
            });
        }
    }
    findings
}

fn parse_golangci_lint(stdout: &str) -> Vec<FindingCandidate> {
    // golangci-lint JSON 格式: [{ "Pos": { "Filename": "...", "Line": N }, "Text": "..." }]
    let mut findings = Vec::new();
    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(stdout) {
        for item in arr {
            let file = item["Pos"]["Filename"].as_str().unwrap_or("");
            let line = item["Pos"]["Line"].as_u64().unwrap_or(0) as u32;
            let text = item["Text"].as_str().unwrap_or("");
            findings.push(FindingCandidate {
                finding_id: FindingId::new(),
                verifier_id: VerifierId::new(),
                rule_id: RuleId::new(),
                target_ref: file.into(),
                target_content_hash: ContentHash::new(""),
                claimed_location: Some(EvidenceLocationClaim {
                    path: file.into(), start_line: Some(line), end_line: Some(line),
                    code_snippet: None, symbol_name: None, byte_offset: None,
                }),
                title: format!("golangci-lint: {}", text),
                description: text.into(),
                severity: Severity::Medium,
                verifier_confidence: 1.0,
                suggested_fix: None,
                rationale: None,
                verifier_metadata: item.clone(),
            });
        }
    }
    findings
}

fn parse_ruff_output(stdout: &str) -> Vec<FindingCandidate> {
    // ruff JSON 格式: [{ "filename": "...", "location": { "row": N }, "message": "..." }]
    serde_json::from_str::<Vec<serde_json::Value>>(stdout)
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            let file = item["filename"].as_str().unwrap_or("");
            let line = item["location"]["row"].as_u64().unwrap_or(0) as u32;
            let code = item["code"].as_str().unwrap_or("");
            let msg = item["message"].as_str().unwrap_or("");
            FindingCandidate {
                finding_id: FindingId::new(),
                verifier_id: VerifierId::new(),
                rule_id: RuleId::new(),
                target_ref: file.into(),
                target_content_hash: ContentHash::new(""),
                claimed_location: Some(EvidenceLocationClaim {
                    path: file.into(), start_line: Some(line), end_line: Some(line),
                    code_snippet: None, symbol_name: None, byte_offset: None,
                }),
                title: format!("ruff({}): {}", code, msg),
                description: msg.into(),
                severity: Severity::Medium,
                verifier_confidence: 1.0,
                suggested_fix: None,
                rationale: None,
                verifier_metadata: item.clone(),
            }
        })
        .collect()
}

fn parse_eslint_output(stdout: &str) -> Vec<FindingCandidate> {
    // eslint JSON 格式: [{ "filePath": "...", "messages": [{ "line": N, "message": "..." }] }]
    serde_json::from_str::<Vec<serde_json::Value>>(stdout)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|file_entry| {
            let file = file_entry["filePath"].as_str().unwrap_or("").to_string();
            let messages = file_entry["messages"].as_array().cloned().unwrap_or_default();
            messages.into_iter().map(move |msg| {
                let line = msg["line"].as_u64().unwrap_or(0) as u32;
                let text = msg["message"].as_str().unwrap_or("");
                let rule = msg["ruleId"].as_str().unwrap_or("");
                FindingCandidate {
                    finding_id: FindingId::new(),
                    verifier_id: VerifierId::new(),
                    rule_id: RuleId::new(),
                    target_ref: file.clone(),
                    target_content_hash: ContentHash::new(""),
                    claimed_location: Some(EvidenceLocationClaim {
                        path: file.clone(), start_line: Some(line), end_line: Some(line),
                        code_snippet: None, symbol_name: None, byte_offset: None,
                    }),
                    title: format!("eslint({}): {}", rule, text),
                    description: text.into(),
                    severity: Severity::Medium,
                    verifier_confidence: 1.0,
                    suggested_fix: None,
                    rationale: None,
                    verifier_metadata: msg.clone(),
                }
            }).collect::<Vec<_>>()
        })
        .collect()
}
```

### 5.6 文件: `src/verifiers/semantic/prompt_builder.rs` — 按语言构建提示

这个模块生成**语言特定的 System Prompt**。Rust 的 prompt 和 Python 的 prompt 不同，因为每种语言有独特的陷阱。

```rust
//! 按语言构建 SemanticVerifier 的 System Prompt。
//!
//! 核心原则: 每种语言有独特的陷阱和习惯用法 — prompt 必须反映这一点。
//! 通用 prompt 会导致遗漏语言特定的关键问题。

use onto_assurance_types::rule_binding::Rule;

/// 为指定语言构建语义审查的 System Prompt。
pub fn build_system_prompt(language: &str, rules: &[Rule]) -> String {
    let lang_guidance = match language {
        "rust" => RUST_SYSTEM,
        "go" => GO_SYSTEM,
        "python" => PYTHON_SYSTEM,
        "typescript" | "javascript" => TS_SYSTEM,
        "java" => JAVA_SYSTEM,
        "cpp" | "c" => CPP_SYSTEM,
        _ => "",
    };

    let rules_text = rules.iter()
        .filter_map(|r| match &r.content {
            onto_assurance_types::rule_binding::RuleContent::NaturalLanguage { text, .. } => {
                Some(format!("- {}: {}", r.name, text))
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("\
You are reviewing {lang} code. Report all defects found.

{guidance}

## Rules

{rules}

## Output Format

For each finding, output a JSON object:
```json
{{
  \"path\": \"file path\",
  \"title\": \"one-line summary\",
  \"description\": \"detailed explanation\",
  \"severity\": \"critical|high|medium|low|info\",
  \"start_line\": line_number,
  \"end_line\": line_number,
  \"existing_code\": \"the problematic code snippet\"
}}
```

Do NOT output a summary or declare success/failure.",
        lang = language, guidance = lang_guidance, rules = rules_text)
}

// ══════════════════════════════════════════════════════════════════
// 语言特定 System Prompt 模板
// ══════════════════════════════════════════════════════════════════

const RUST_SYSTEM: &str = "\
## Rust-Specific Checks

- **Ownership**: Unnecessary clones, borrow checker workarounds hiding bugs,
  lifetime annotations too short or too long
- **Unsafe**: Every unsafe block MUST have a SAFETY comment. Verify invariants.
- **Error Handling**: Errors propagated (not swallowed). ? not used in main()
  without context. Appropriate error types.
- **Concurrency**: Missing Send/Sync bounds. Deadlocks (Mutex lock ordering).
  Data races (unsafe + raw pointers).
- **Macros**: Hygiene — no accidental capture of external names. No silent panics.
- **Patterns**: Exhaustive match arms. No wildcard matches hiding new enum variants.";

const GO_SYSTEM: &str = "\
## Go-Specific Checks

- **Goroutines**: Every goroutine needs a clear exit path (context, channel close,
  select+default). Goroutine leaks in HTTP handlers.
- **Nil Safety**: Interface nil vs concrete nil. Type assertions handle nil.
  Map access checks zero-value correctly.
- **Error Handling**: Every error checked. No _ for errors without comment.
  Error wrapping with %w for unwrap at upper levels.
- **Defer**: Defer in loops = dangerous. Resource cleanup order.
  Named return values with defer — verify intentional modification.
- **Concurrency**: Channel ops — no perpetual blocking. sync.Mutex unlocked
  on all paths (including panic). sync.WaitGroup Add before goroutine launch.
- **Context**: All I/O must pass context. No context.Background() in handlers.";

const PYTHON_SYSTEM: &str = "\
## Python-Specific Checks

- **Type Safety**: Type annotations on all public functions. Optional vs None.
  Union types correct. Protocol conformance validated.
- **Async/Await**: time.sleep() in async = blocks event loop. Unawaited coroutines.
  Missing aiohttp session close.
- **Security**: SQL injection via f-strings. Command injection in subprocess.
  Pickle deserialization of untrusted data. Hardcoded secrets.
- **Exception Handling**: No bare except: or except Exception: without re-raising.
  Custom exception hierarchy — inheritance correct.
- **Performance**: Generator vs list for large data. Unnecessary comprehensions
  building intermediate lists. __del__ with circular references.";

const TS_SYSTEM: &str = "\
## TypeScript-Specific Checks

- **Null Safety**: Optional chaining (?.) and nullish coalescing (??).
  Non-null assertions (!) that may fail. API response shape validation.
- **XSS**: dangerouslySetInnerHTML, innerHTML, document.write — must sanitize.
  URL parameters encoded (encodeURIComponent).
- **Promises**: Unhandled rejections. forEach with async (doesn't await).
  Promise.all — one failure leaves others running.
- **Type System**: any usage must be justified. Type assertions (as) verified.
  Generic type parameter constraints checked.
- **React**: Missing keys in lists. useEffect cleanup. State update after unmount.
  Unnecessary re-renders.
- **Security**: eval(), new Function(). postMessage without origin check.
  localStorage for sensitive data.";

const JAVA_SYSTEM: &str = "\
## Java-Specific Checks

- **Concurrency**: synchronized lock ordering. volatile vs Atomic*.
  ThreadLocal cleanup. ExecutorService shutdown on all paths.
- **JPA/Hibernate**: N+1 queries. Lazy loading outside transaction.
  Missing @Transactional. Entity equals/hashCode inconsistent.
- **Security**: SQL injection (JPQL concatenation). XXE in XML parsers.
  Path traversal. Unsafe deserialization.
- **Resources**: try-with-resources for AutoCloseable. Unclosed streams.
  Connection pool leaks.
- **Null**: @Nullable/@NonNull consistency. Optional.orElse(null) = anti-pattern.";

const CPP_SYSTEM: &str = "\
## C/C++-Specific Checks

- **Memory**: new/delete pairing, use-after-free, double free.
  Memory leaks in exception paths.
- **UB**: Signed overflow, null pointer deref, out-of-bounds access,
  strict aliasing violations.
- **RAII**: Destructors noexcept. Rule of 5/3/0 violations.
  Resource acquisition in constructors — no bare pointers owning memory.
- **Concurrency**: Data races. Missing atomic. Mutex deadlock.
  Condition variable spurious wakeup handled.";
```

### 5.7 文件: `src/verifiers/registry.rs` — 多语言 Verifier 注册表

```rust
//! VerifierRegistry — 根据 LanguageProfile 选择合适的 Verifier 集合

use std::collections::HashMap;
use onto_assurance_types::language_profile::{LanguageProfile, LanguageRegistry};
use onto_assurance_types::ids::VerifierId;
use onto_assurance_runtime::verification::{SemanticVerifierPort, DeterministicVerifierPort};

/// 管理所有已注册的 Verifier，按语言路由。
pub struct VerifierRegistry {
    semantic: HashMap<VerifierId, Box<dyn SemanticVerifierPort>>,
    deterministic: HashMap<VerifierId, Box<dyn DeterministicVerifierPort>>,
}

impl VerifierRegistry {
    pub fn new() -> Self {
        Self { semantic: HashMap::new(), deterministic: HashMap::new() }
    }

    pub fn register_semantic(&mut self, v: Box<dyn SemanticVerifierPort>) {
        self.semantic.insert(v.capability().verifier_id, v);
    }

    pub fn register_deterministic(&mut self, v: Box<dyn DeterministicVerifierPort>) {
        self.deterministic.insert(v.capability().verifier_id, v);
    }

    /// 为给定语言选择所有匹配的语义 Verifier。
    pub fn semantic_for(&self, profile: &LanguageProfile) -> Vec<&dyn SemanticVerifierPort> {
        profile.verifiers.semantic.iter()
            .filter_map(|id| self.semantic.get(id).map(|v| v.as_ref()))
            .collect()
    }

    /// 为给定语言选择所有适用的确定性 Verifier。
    pub fn deterministic_for(&self, profile: &LanguageProfile) -> Vec<&dyn DeterministicVerifierPort> {
        let ids = [
            &profile.verifiers.build, &profile.verifiers.test,
            &profile.verifiers.lint, &profile.verifiers.format_check,
            &profile.verifiers.sast, &profile.verifiers.dependency_audit,
        ];
        let mut verifiers: Vec<&dyn DeterministicVerifierPort> = ids.iter()
            .filter_map(|id| id.as_ref())
            .filter_map(|id| self.deterministic.get(id).map(|v| v.as_ref()))
            .collect();
        for (_, id) in &profile.verifiers.extras {
            if let Some(v) = self.deterministic.get(id) {
                verifiers.push(v.as_ref());
            }
        }
        verifiers
    }
}
```

### 5.8 文件: `src/scope/polyglot.rs` — 多语言仓库处理

```rust
//! Polyglot — 多语言仓库的按语言分片和预算分配

use onto_assurance_types::verification_target::VerificationTarget;
use onto_assurance_types::language_profile::LanguageRegistry;
use std::collections::HashMap;

/// 按语言对 VerificationTarget 分组。
///
/// 一个仓库可能同时有 Rust + TypeScript + Protobuf。
/// 每种语言需要不同的 Verifier 集合。
pub fn group_by_language(
    targets: &[VerificationTarget],
    registry: &LanguageRegistry,
) -> HashMap<String, Vec<VerificationTarget>> {
    let mut groups: HashMap<String, Vec<VerificationTarget>> = HashMap::new();
    for target in targets {
        let lang = target.language.as_deref().unwrap_or("unknown");
        groups.entry(lang.to_string()).or_default().push(target.clone());
    }
    tracing::info!(
        total = targets.len(),
        languages = ?groups.keys().collect::<Vec<_>>(),
        "polyglot scope grouped by language"
    );
    groups
}

/// 计算多语言仓库的 token 估算 — 用于预算分配。
///
/// 不同语言的 token/byte 比例不同:
///   Go:     ~1.5 tokens/byte (长关键字，显式错误处理)
///   Rust:   ~1.3 tokens/byte
///   Python: ~1.1 tokens/byte (简洁语法)
///   TS:     ~1.4 tokens/byte
pub fn estimate_tokens_by_language(
    groups: &HashMap<String, Vec<VerificationTarget>>,
    registry: &LanguageRegistry,
) -> HashMap<String, u64> {
    let mut estimates = HashMap::new();
    for (lang, targets) in groups {
        let tokens_per_byte = registry.get(lang)
            .map(|p| p.tokens_per_byte)
            .unwrap_or(1.5);
        let total_bytes: u64 = targets.iter().map(|t| t.size_bytes).sum();
        estimates.insert(lang.clone(), (total_bytes as f64 * tokens_per_byte) as u64);
    }
    estimates
}

/// 按语言比例分配语义 Verifier 的调用预算。
///
/// 例如: 总预算 50 次语义调用，Rust 30个文件，Go 20个文件
///   → Rust: min(30, 50*0.6) = 30, Go: min(20, 50*0.4) = 20
pub fn allocate_semantic_budget(
    groups: &HashMap<String, Vec<VerificationTarget>>,
    total_budget: u32,
) -> HashMap<String, u32> {
    let total_targets: usize = groups.values().map(|v| v.len()).sum();
    if total_targets == 0 { return HashMap::new(); }

    groups.iter().map(|(lang, targets)| {
        let proportion = targets.len() as f64 / total_targets as f64;
        let allocation = (total_budget as f64 * proportion).ceil() as u32;
        // 不能超过该语言的目标数 (语义审查按文件)
        (lang.clone(), allocation.min(targets.len() as u32))
    }).collect()
}
```

---

## 6. Go→Rust 算法移植指南
