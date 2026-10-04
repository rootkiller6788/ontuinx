# IronClaw — Complete System Architecture

```
┌──────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                         IRONCLAW SYSTEM ARCHITECTURE                                   │
│                                       Secure Personal AI Assistant                                    │
└──────────────────────────────────────────────────────────────────────────────────────────────────────┘

═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                                          EXTERNAL INTERFACES
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────┐
  │  WebUI   │  │   CLI    │  │  Slack   │  │ Telegram │  │  Email   │  │ OAuth IdPs │
  │ (React   │  │ (TUI +   │  │ Workspace│  │   Bot    │  │ (future) │  │ (Google,   │
  │  SPA)    │  │  Cmds)   │  │          │  │          │  │          │  │  GitHub…)  │
  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └─────┬──────┘
       │             │             │             │             │             │
       │  HTTP/WS    │  stdin/     │  Events     │  Webhooks   │             │  OAuth 2.0
       │  REST+SSE   │  stdout     │  API        │  API        │             │  / OIDC
       │             │             │             │             │             │
       ▼             ▼             ▼             ▼             ▼             ▼
┌──────────────────────────────────────────────────────────────────────────────────────────────────────┐
│                                     CHANNEL / INGRESS LAYER                                            │
│                                                                                                        │
│  ┌────────────────────┐  ┌────────────────────┐  ┌──────────────────────────────────────────┐         │
│  │ ironclaw_webui     │  │ ironclaw_reborn_cli│  │ ironclaw_extension_host                   │         │
│  │ ────────────────── │  │ ────────────────── │  │ ──────────────────────────────────────── │         │
│  │ • axum HTTP server │  │ • binary "ironclaw"│  │ • ChannelAdapter trait                    │         │
│  │ • /auth/* OAuth    │  │ • CLI commands     │  │ • Ingress verification (Slack sig, etc.)   │         │
│  │ • SessionStore     │  │ • Shell completions│  │ • Delivery coordinator                     │         │
│  │ • Env/Session/OIDC │  │ • Doctor/Home/     │  │ • Per-extension idempotency ledger         │         │
│  │   authenticators   │  │   Profile commands │  │                                            │         │
│  │ • WebSocket/SSE    │  │                    │  │ ironclaw_slack_extension                   │         │
│  │ • CORS middleware   │  │                    │  │ ironclaw_telegram_extension                │         │
│  └────────┬───────────┘  └────────┬───────────┘  └────────────────────┬─────────────────────┘         │
│           │                       │                                   │                               │
│           │                       │         ┌─────────────────────────┘                               │
│           ▼                       ▼         ▼                                                         │
│  ┌──────────────────────────────────────────────────────────────────────────────────────────┐         │
│  │                         ironclaw_reborn_openai_compat                                     │         │
│  │                   OpenAI-compatible Chat/Responses API endpoints                          │         │
│  └──────────────────────────────────────────────────────────────────────────────────────────┘         │
└──────────────────────────────────────────────────────────────────────────────────────────────────────┘
                                           │
                                           │  inbound turn submission
                                           ▼
═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                              PRODUCT LAYER — UX & Workflow Ownership
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌────────────────────────────────────────────────────────────────────────────────────────────────┐     │
  │                              ironclaw_product (Product Workflow Facade)                         │     │
  │  ─────────────────────────────────────────────────────────────────────────────────────────────  │     │
  │  • inbound turns & bindings     • ProductAdapter trait (auth, egress, identity, projection)     │     │
  │  • workflow ledger (durable)    • mission/routine orchestration                                │     │
  │  • Reborn service bridges       • redaction & fake test support                                │     │
  └───────────────────────┬────────────────────────────────────────────────────────────────────────┘     │
                          │                                                                              │
         ┌────────────────┼────────────────┐                                                             │
         ▼                ▼                ▼                                                             │
  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐                                             │
  │ ironclaw_   │  │ ironclaw_   │  │ ironclaw_first_party│                                             │
  │ product_    │  │ product_    │  │ _extensions         │                                             │
  │ workflow    │  │ adapters    │  │ ─────────────────── │                                             │
  │ ────────────│  │ ────────────│  │ • GitHub, Drive…    │                                             │
  │ • missions  │  │ • adapter   │  │ • WASM v2 adapters  │                                             │
  │ • projects  │  │   registry  │  │ • tool manifests    │                                             │
  │ • skills    │  │ • channel   │  └─────────────────────┘                                             │
  │ • routines  │  │   surfaces  │                                                                      │
  │ • approvals │  └─────────────┘                                                                      │
  └──────┬──────┘                                                                                       │
         │                                                                                              │
         │  TurnCoordinator boundary (lock, serialize, one-active-run-per-thread)                       │
         ▼                                                                                              │
═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                          USERLAND LAYER — Agent Loops (Untrusted)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌────────────────────────────────────────────────────────────────────────────────────────────────┐     │
  │                         ironclaw_runner (Loop-Runtime Assembly & Driver Registry)               │     │
  │  ─────────────────────────────────────────────────────────────────────────────────────────────  │     │
  │  • scheduler            • per-run executor          • loop host factory                         │     │
  │  • driver registry      • planned/text driver       • exit-applier wiring                       │     │
  │  • home/profile/doctor   • standalone adapters                                                  │     │
  └───────────────────────┬────────────────────────────────────────────────────────────────────────┘     │
                          │                                                                              │
         ┌────────────────┼────────────────────────┐                                                    │
         ▼                ▼                        ▼                                                    │
  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐                                         │
  │ ironclaw_   │  │ ironclaw_   │  │ ironclaw_loop_host      │                                         │
  │ turns       │  │ agent_loop  │  │ ────────────────────────│                                         │
  │ ────────────│  │ ────────────│  │ • capability/input ports│                                         │
  │ • TurnCoord │  │ • strategy  │  │ • allow sets             │                                         │
  │ • requests/ │  │ • planner   │  │ • input queue            │                                         │
  │   responses │  │ • executor  │  │ • identity/skill context │                                         │
  │ • run profs │  │ • CodeAct   │  │ • cancellation           │                                         │
  │ • turn store│  │ • lightweight│ │ • prompt assembly        │                                         │
  └──────┬──────┘  └──────┬──────┘  └────────────┬────────────┘                                         │
         │                │                      │                                                      │
         │   ironclaw_threads (session thread/transcript contracts, tool-result refs)                   │
         │   ironclaw_conversations (conversation binding, inbound state store, libSQL/PG)              │
         │                                                                                              │
         └────────────────┬──────────────────────┘                                                      │
                          │                                                                              │
                          │  CapabilityHost boundary (every effect passes through kernel gates)          │
                          ▼                                                                              │
═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                       KERNEL LAYER — Authority & Policy Gates (Trusted)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌──────────────────────────────────────────────────────────────────────────────────────────────────┐   │
  │                                  ironclaw_capabilities (CapabilityHost)                           │   │
  │  ────────────────────────────────────────────────────────────────────────────────────────────────  │   │
  │  • invoke / resume / spawn             • obligation seams (prepare/complete/abort)                │   │
  │  • ReplayPayloadStore (gate/auth resume payload, never model-visible)                            │   │
  └───────────────────────┬──────────────────────────────────────────────────────────────────────────┘   │
                          │                                                                              │
   ┌──────────────────────┼──────────────────────────────────────────────────────┐                      │
   │   AUTHORITY GATES    │             EXECUTION GATES                           │   SAFETY GATES       │
   │                      ▼                                                       │                      │
   │  ┌────────────────────────────┐  ┌────────────────────────────────────┐  ┌──────────────────────┐ │
   │  │ ironclaw_authorization     │  │ ironclaw_dispatcher                 │  │ ironclaw_safety      │ │
   │  │ ────────────────────────── │  │ ────────────────────────────────────│  │ ─────────────────────│ │
   │  │ • grant matching (RBAC)    │  │ • RuntimeAdapter routing             │  │ • prompt injection   │ │
   │  │ • lease coordination       │  │ • already-authorized dispatch        │  │ • credential detect  │ │
   │  │ • dispatch/spawn decisions │  │ • redacted dispatch results          │  │ • leak scanning      │ │
   │  └────────────┬───────────────┘  │ • event dispatch contracts           │  │ • sensitive paths    │ │
   │               │                  └────────────────┬───────────────────┘  │ • sanitization       │ │
   │  ┌────────────▼───────────────┐                   │                      └──────────┬───────────┘ │
   │  │ ironclaw_approvals         │                   │                                 │             │
   │  │ ────────────────────────── │                   │                      ┌──────────▼───────────┐ │
   │  │ • exact-invocation leases  │                   │                      │ ironclaw_prompt_      │ │
   │  │ • human approval flows     │                   │                      │ envelope              │ │
   │  │ • auto-approve rules       │                   │                      │ ───────────────────── │ │
   │  │ • resume coordination      │                   │                      │ • source/trust labels │ │
   │  └────────────────────────────┘                   │                      │ • size limits         │ │
   │                                                   │                      │ • hijack rejection    │ │
   │  ┌────────────────────────────┐                   │                      └──────────────────────┘ │
   │  │ ironclaw_trust             │                   │                                               │
   │  │ ────────────────────────── │  ┌────────────────▼──────────────────────────────────────────┐    │
   │  │ • trust classes            │  │               RUNTIME LANES (sandboxed execution)          │    │
   │  │ • policy sources           │  │  ────────────────────────────────────────────────────────  │    │
   │  │ • effective trust calc     │  │                                                           │    │
   │  │ • invalidation             │  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │    │
   │  └────────────────────────────┘  │  │ ironclaw_wasm│  │ ironclaw_mcp │  │ironclaw_     │      │    │
   │                                  │  │ ──────────── │  │ ──────────── │  │scripts       │      │    │
   │  ┌────────────────────────────┐  │  │ • WIT bindgen│  │ • JSON-RPC   │  │ ──────────── │      │    │
   │  │ ironclaw_resources         │  │  │ • component  │  │ • HTTP/stdio │  │ • Bash/Python │      │    │
   │  │ ────────────────────────── │  │  │   model      │  │ • discovery  │  │ • Docker      │      │    │
   │  │ • reservation/reconciliation│ │  │ • host adapt │  │ • tunneling  │  │ • stdio cap   │      │    │
   │  │ • quota accounting         │  │  │ • wasm_      │  │ • resource   │  │ • output parse│      │    │
   │  │ • cost tracking            │  │  │   limiter    │  │   accounting │  └──────────────┘      │    │
   │  └────────────────────────────┘  │  └──────────────┘  └──────────────┘                        │    │
   │                                  └────────────────────────────────────────────────────────────┘    │
   │                                                                                                    │
   │  ┌──────────────────────────────────────────────────────────────────────────────────────────────┐  │
   │  │                         HOST-MEDIATED PRIVILEGED SERVICES                                     │  │
   │  │  ────────────────────────────────────────────────────────────────────────────────────────────  │  │
   │  │                                                                                               │  │
   │  │  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────────────┐ │  │
   │  │  │ ironclaw_secrets │ │ ironclaw_network │ │ironclaw_filesystem│ │ ironclaw_host_runtime    │ │  │
   │  │  │ ──────────────── │ │ ──────────────── │ │──────────────────│ │ ─────────────────────────│ │  │
   │  │  │ • encrypted repos│ │ • policy boundary│ │ • RootFilesystem  │ │ • service composition    │ │  │
   │  │  │ • lease issuance │ │ • URL targets    │ │ • mount catalog   │ │ • obligations            │ │  │
   │  │  │ • one-shot cons. │ │ • hardened HTTP  │ │ • virtual paths   │ │ • HTTP egress            │ │  │
   │  │  │ • injection      │ │ • allow/deny DNS │ │ • scoped access   │ │ • redaction              │ │  │
   │  │  │   handoff        │ │ • host/provider  │ │ • backend routing │ │ • secrets/network/       │ │  │
   │  │  └──────────────────┘ │   egress         │ │ • integrity       │ │   resource mediation     │ │  │
   │  │                       └──────────────────┘ └──────────────────┘ └──────────────────────────┘ │  │
   │  │                                                                                               │  │
   │  │  ┌──────────────────┐ ┌──────────────────┐ ┌──────────────────┐                               │  │
   │  │  │ ironclaw_process │ │ ironclaw_runtime │ │ ironclaw_hooks   │                               │  │
   │  │  │ sandbox          │ │ _policy          │ │ ──────────────── │                               │  │
   │  │  │ ──────────────── │ │ ──────────────── │ │ • loop hooks     │                               │  │
   │  │  │ • process exec   │ │ • profile resolve│ │ • trust-tiered   │                               │  │
   │  │  │ • Docker sandbox │ │ • runtime select │ │ • predicates     │                               │  │
   │  │  │ • mount roots    │ │ • policy enforce │ │ • decision sinks │                               │  │
   │  │  └──────────────────┘ └──────────────────┘ │ • failure policy │                               │  │
   │  │                                             └──────────────────┘                               │  │
   │  └──────────────────────────────────────────────────────────────────────────────────────────────┘  │
   └────────────────────────────────────────────────────────────────────────────────────────────────────┘
                                              │
                                              │  Effect subscription boundary
                                              ▼
═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                     SUBSTRATE LAYER — Durable Primitives (Backend-Agnostic)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │                                      DURABLE STATE & EVENTS                                          │
  │                                                                                                      │
  │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐                        │
  │  │ ironclaw_events      │  │ ironclaw_event_      │  │ ironclaw_event_      │                        │
  │  │ ──────────────────── │  │ projections          │  │ streams              │                        │
  │  │ • immutable audit log│  │ ──────────────────── │  │ ──────────────────── │                        │
  │  │ • typed redacted     │  │ • snapshot compute   │  │ • live/replay subs   │                        │
  │  │   event envelopes    │  │ • cursor/visibility  │  │ • bounded buffers    │                        │
  │  │ • sink/logger traits │  │ • product-facing     │  │ • lag/rebase signals │                        │
  │  │ • durable adapters   │  │   projection views   │  │ • redaction validate │                        │
  │  └──────────┬───────────┘  └──────────┬───────────┘  └──────────┬───────────┘                        │
  │             │                         │                         │                                    │
  │             └─────────────────────────┼─────────────────────────┘                                    │
  │                                       │                                                              │
  │                          ┌────────────▼─────────────┐                                               │
  │                          │ ironclaw_reborn_event_   │                                               │
  │                          │ store                    │                                               │
  │                          │ ──────────────────────── │                                               │
  │                          │ • durable event/audit    │                                               │
  │                          │   store backends         │                                               │
  │                          │ • PostgreSQL + libSQL    │                                               │
  │                          │ • fixtures for testing   │                                               │
  │                          └──────────────────────────┘                                               │
  │                                                                                                      │
  │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────────┐                    │
  │  │ ironclaw_run_state   │  │ ironclaw_threads     │  │ ironclaw_conversations   │                    │
  │  │ ──────────────────── │  │ ──────────────────── │  │ ──────────────────────── │                    │
  │  │ • durable invocation │  │ • thread lifecycle   │  │ • conversation binding   │                    │
  │  │ • approval records   │  │ • metadata store     │  │ • inbound/state store    │                    │
  │  │ • checkpoint storage │  │ • tool-result refs   │  │ • libSQL + PG backends   │                    │
  │  │ • recovery state     │  │ • DB/in-memory store │  │ • session state machine  │                    │
  │  └──────────────────────┘  └──────────────────────┘  └──────────────────────────┘                    │
  │                                                                                                      │
  └─────────────────────────────────────────────────────────────────────────────────────────────────────┘

  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │                                      MEMORY & KNOWLEDGE                                              │
  │                                                                                                      │
  │  ┌──────────────────────────────┐  ┌──────────────────────────────┐                                 │
  │  │ ironclaw_memory              │  │ ironclaw_memory_native       │                                 │
  │  │ ──────────────────────────── │  │ ──────────────────────────── │                                 │
  │  │ • memory_search              │  │ • NativeMemoryService        │                                 │
  │  │ • memory_write               │  │ • document repos             │                                 │
  │  │ • memory_read                │  │ • chunking & embeddings      │                                 │
  │  │ • memory_tree                │  │ • hybrid search (vector+     │                                 │
  │  │ • hybrid search contracts    │  │   keyword)                   │                                 │
  │  │ • filesystem adapter         │  │ • indexer hooks              │                                 │
  │  │ • backend contracts          │  │ • prompt write-safety        │                                 │
  │  └──────────────────────────────┘  └──────────────────────────────┘                                 │
  │                                                                                                      │
  └─────────────────────────────────────────────────────────────────────────────────────────────────────┘

  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │                                IDENTITY, PROJECTS, TRIGGERS, EXTENSIONS                               │
  │                                                                                                      │
  │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐                        │
  │  │ ironclaw_reborn_     │  │ ironclaw_projects    │  │ ironclaw_triggers    │                        │
  │  │ identity             │  │ ──────────────────── │  │ ──────────────────── │                        │
  │  │ ──────────────────── │  │ • Project entity     │  │ • cron schedule      │                        │
  │  │ • UserId ← identity  │  │ • membership ACL     │  │ • timezone validation│                        │
  │  │ • StoredUser profile │  │ • resolve_access     │  │ • poller core        │                        │
  │  │ • filesystem-backed  │  │ • ProjectRepository  │  │ • libSQL/PG repos    │                        │
  │  └──────────────────────┘  └──────────────────────┘  └──────────────────────┘                        │
  │                                                                                                      │
  │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐                        │
  │  │ ironclaw_extensions  │  │ ironclaw_auth         │  │ ironclaw_outbound    │                        │
  │  │ ──────────────────── │  │ ────────────────────  │  │ ──────────────────── │                        │
  │  │ • extension manifests│  │ • product auth flows  │  │ • egress policy      │                        │
  │  │ • capability descs   │  │ • credential exchange │  │ • notification opt-in│                        │
  │  │ • in-memory registry │  │ • provider exchange   │  │ • delivery attempts  │                        │
  │  │ • installation recs  │  │ • continuation        │  │ • projection subs    │                        │
  │  └──────────────────────┘  └──────────────────────┘  └──────────────────────┘                        │
  │                                                                                                      │
  └─────────────────────────────────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                                 LLM, SKILLS, SAFETY — Cross-Cutting
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │                                                                                                      │
  │  ┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐                        │
  │  │ ironclaw_llm         │  │ ironclaw_skills      │  │ ironclaw_safety      │                        │
  │  │ ──────────────────── │  │ ──────────────────── │  │ ──────────────────── │  (kernel layer)        │
  │  │ • Multi-provider     │  │ • SKILL.md parser    │  │ • prompt injection    │                        │
  │  │   abstraction        │  │ • gating & scoring   │  │ • credential detect   │                        │
  │  │ • trait + auth       │  │ • registry & install │  │ • leak scanning        │                        │
  │  │ • retry, failover,   │  │ • selection pipeline │  │ • sensitive paths      │                        │
  │  │   circuit breaker    │  │ • skill learning     │  │ • fuzz + benchmarks    │                        │
  │  │ • tool schemas       │  │ • refinement logic   │  │                        │                        │
  │  │ • reasoning, tracing │  │                      │  │                        │                        │
  │  │ • transcription,     │  │                      │  │                        │                        │
  │  │   vision             │  │                      │  │                        │                        │
  │  └──────────────────────┘  └──────────────────────┘  └──────────────────────┘                        │
  │                                                                                                      │
  │                         Providers: Anthropic │ OpenAI │ Ollama │ Bedrock │ NearAI │ Tinfoil         │
  │                                                                                                      │
  └─────────────────────────────────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                               COMPOSITION & BOOT — Wires Everything Together
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │                                                                                                      │
  │  ┌──────────────────────────────┐  ┌──────────────────────────────┐                                 │
  │  │ ironclaw_reborn_composition  │  │ ironclaw_reborn_config       │                                 │
  │  │ ──────────────────────────── │  │ ──────────────────────────── │                                 │
  │  │ • Facade-shaped production   │  │ • Boot configuration         │                                 │
  │  │   composition root for Reborn│  │ • Config.toml parsing        │                                 │
  │  │ • Wires storage/runtime      │  │ • Defaults & resolution      │                                 │
  │  │   services by profile        │  │ • Profile selection          │                                 │
  │  │ • Dependency injection       │  │                              │                                 │
  │  │ • App builder                │  │                              │                                 │
  │  │ • RebornHostBindings         │  │                              │                                 │
  │  └──────────────────────────────┘  └──────────────────────────────┘                                 │
  │                                                                                                      │
  │  ┌──────────────────────────────┐                                                                   │
  │  │ ironclaw_architecture        │  ← Boundary enforcement tests (no production code)                │
  │  └──────────────────────────────┘                                                                   │
  │                                                                                                      │
  └─────────────────────────────────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                     STORAGE BACKEND LAYER — Dual-Backend Persistence (PostgreSQL + libSQL)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────┐
  │                                                                                                      │
  │   ironclaw_filesystem::RootFilesystem (mount catalog, virtual path authority, backend containment)   │
  │                                          │                                                           │
  │                    ┌─────────────────────┼─────────────────────┐                                     │
  │                    ▼                     ▼                     ▼                                     │
  │             ┌─────────────┐       ┌─────────────┐       ┌─────────────┐                              │
  │             │ PostgreSQL  │       │   libSQL    │       │  Local FS   │                              │
  │             │ ─────────── │       │  (Turso)    │       │  ────────── │                              │
  │             │ • events    │       │ ─────────── │       │ • memory    │                              │
  │             │ • threads   │       │ • events    │       │   native    │                              │
  │             │ • convos    │       │ • threads   │       │ • user files│                              │
  │             │ • run_state │       │ • convos    │       │ • attachmts │                              │
  │             │ • hooks     │       │ • run_state │       │ • config    │                              │
  │             │ • triggers  │       │ • hooks     │       └─────────────┘                              │
  │             │ • projects  │       │ • triggers  │                                                    │
  │             │ • ledger    │       │ • projects  │                                                    │
  │             └─────────────┘       └─────────────┘                                                    │
  │                                                                                                      │
  │   Backend parity enforced by: ironclaw_hooks_parity (cross-backend adversarial conformance suite)     │
  │                                                                                                      │
  └─────────────────────────────────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                         FEATURE FLOW — How a Request Traverses the Stack
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  User Message (WebUI / Slack / CLI / Telegram)
       │
       ▼
  ┌─ Channel Adapter ──────────────────────────────────────────────────────────────────────────────┐
  │  • verify ingress (Slack signature, session cookie, OAuth token, …)                             │
  │  • normalize to typed inbound message                                                           │
  │  • resolve identity → UserId via ironclaw_reborn_identity                                       │
  └──────────────────────────────────┬──────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
  ┌─ Product Workflow (ironclaw_product) ───────────────────────────────────────────────────────────┐
  │  • create turn record with tenant/user/project/agent/thread scope                               │
  │  • write to durable workflow ledger                                                             │
  │  • check existing approvals / resource reservations                                              │
  └──────────────────────────────────┬──────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
  ┌─ TurnCoordinator (ironclaw_turns) ──────────────────────────────────────────────────────────────┐
  │  • enforce one-active-run-per-thread lock                                                        │
  │  • serialize inbound → scoped TurnRequest                                                       │
  │  • select run profile                                                                            │
  └──────────────────────────────────┬──────────────────────────────────────────────────────────────┘
                                     │
                                     ▼
  ┌─ Agent Loop (ironclaw_agent_loop / ironclaw_runner) ────────────────────────────────────────────┐
  │  • load loop strategy (Planned / Text / CodeAct / lightweight)                                   │
  │  • assemble prompt context from authorized memory reads                                          │
  │  • call LLM via ironclaw_llm (provider-agnostic, with retry/circuit-breaker)                     │
  │  • LLM decides: response text? or tool call?                                                     │
  └──────────────────┬──────────────────────────────────────────────────────────────────────────────┘
                     │
                     │  tool call requested
                     ▼
  ┌─ CapabilityHost (ironclaw_capabilities) ────────────────────────────────────────────────────────┐
  │                                                                                                  │
  │   ┌─ Authorization ────► ironclaw_authorization: grant matching, permission check                │
  │   │                                                                                              │
  │   ├─ Trust ────────────► ironclaw_trust: effective trust class calculation                       │
  │   │                                                                                              │
  │   ├─ Approvals ────────► ironclaw_approvals: human sign-off required? exact-invocation lease      │
  │   │                                                                                              │
  │   ├─ Resources ────────► ironclaw_resources: quota check, reservation, cost accounting            │
  │   │                                                                                              │
  │   ├─ Safety ───────────► ironclaw_safety: prompt injection? credential leak? sensitive path?      │
  │   │                                                                                              │
  │   ├─ Secrets (if needed)► ironclaw_secrets: encrypted lease → inject at transit (never inline)    │
  │   │                                                                                              │
  │   ├─ Network (if HTTP) ► ironclaw_network: URL allowlist? DNS policy? hardened egress             │
  │   │                                                                                              │
  │   └─ Filesystem ───────► ironclaw_filesystem: scoped path? mount allowed? integrity check         │
  │                                                                                                  │
  │   ALL GATES PASS → dispatch                                                                      │
  └──────────────────┬───────────────────────────────────────────────────────────────────────────────┘
                     │
                     ▼
  ┌─ Runtime Dispatch (ironclaw_dispatcher) ─────────────────────────────────────────────────────────┐
  │                                                                                                  │
  │   ┌── WASM tool ──────► ironclaw_wasm (component model, WIT bindings, sandboxed, resource-capped)│
  │   ├── MCP server ─────► ironclaw_mcp (JSON-RPC, stdio/HTTP, tool discovery)                      │
  │   ├── Script ─────────► ironclaw_scripts (Bash/Python, Docker-sandboxed, stdio capture)           │
  │   └── Process ────────► ironclaw_processes (lifecycle, output, cancellation)                     │
  │                                                                                                  │
  └──────────────────┬───────────────────────────────────────────────────────────────────────────────┘
                     │
                     │  execution result (redacted)
                     ▼
  ┌─ Result Pipeline ───────────────────────────────────────────────────────────────────────────────┐
  │                                                                                                  │
  │   1. ironclaw_host_runtime: redaction & leak detection                                           │
  │   2. ironclaw_events: append immutable redacted audit event                                       │
  │   3. ironclaw_event_projections: update snapshots (if needed)                                     │
  │   4. ironclaw_event_streams: fan-out to live subscribers (WebUI SSE, notification channels)       │
  │   5. ironclaw_memory: index for future retrieval (embeddings + keyword)                           │
  │   6. ironclaw_outbound: deliver reply/notification to channels (Slack, email, …)                  │
  │   7. ironclaw_turns: persist turn completion, release thread lock                                │
  │                                                                                                  │
  └──────────────────────────────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                         DEPENDENCY FLOW — Acyclic Upward (lower ← is more foundational)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

   ┌──────────────────────────────────────────────────────────────────────────┐
   │                          SURFACES (CLI, WebUI, Channels)                  │
   │  ironclaw_reborn_cli  ironclaw_webui  ironclaw_extension_host             │
   │  ironclaw_reborn_openai_compat                                           │
   └───────────────────────────────┬──────────────────────────────────────────┘
                                   │
   ┌───────────────────────────────▼──────────────────────────────────────────┐
   │                          PRODUCT & COMPOSITION                            │
   │  ironclaw_product  ironclaw_product_workflow  ironclaw_product_adapters   │
   │  ironclaw_first_party_extensions  ironclaw_first_party_extension_ports    │
   │  ironclaw_reborn_composition  ironclaw_reborn_config                      │
   │  ironclaw_reborn_identity  ironclaw_outbound  ironclaw_triggers           │
   └───────────────────────────────┬──────────────────────────────────────────┘
                                   │
   ┌───────────────────────────────▼──────────────────────────────────────────┐
   │                     LOOPS, TURNS, LLM, SKILLS                             │
   │  ironclaw_runner  ironclaw_turns  ironclaw_agent_loop  ironclaw_loop_host │
   │  ironclaw_threads  ironclaw_conversations  ironclaw_llm                   │
   │  ironclaw_skills  ironclaw_embeddings                                     │
   └───────────────────────────────┬──────────────────────────────────────────┘
                                   │
   ┌───────────────────────────────▼──────────────────────────────────────────┐
   │                     KERNEL GATES & RUNTIME LANES                           │
   │  ironclaw_capabilities  ironclaw_dispatcher  ironclaw_authorization       │
   │  ironclaw_approvals  ironclaw_trust  ironclaw_safety  ironclaw_resources  │
   │  ironclaw_wasm  ironclaw_wasm_limiter  ironclaw_mcp  ironclaw_scripts     │
   │  ironclaw_processes  ironclaw_process_sandbox  ironclaw_runtime_policy     │
   │  ironclaw_secrets  ironclaw_network  ironclaw_host_runtime                │
   │  ironclaw_hooks  ironclaw_extensions  ironclaw_auth                       │
   └───────────────────────────────┬──────────────────────────────────────────┘
                                   │
   ┌───────────────────────────────▼──────────────────────────────────────────┐
   │                     DURABLE SUBSTRATE & FILESYSTEM                         │
   │  ironclaw_filesystem  ironclaw_events  ironclaw_event_projections         │
   │  ironclaw_event_streams  ironclaw_reborn_event_store                      │
   │  ironclaw_run_state  ironclaw_memory  ironclaw_memory_native              │
   │  ironclaw_projects  ironclaw_attachments  ironclaw_extractors             │
   └───────────────────────────────┬──────────────────────────────────────────┘
                                   │
   ┌───────────────────────────────▼──────────────────────────────────────────┐
   │                     FOUNDATION (zero IronClaw deps)                        │
   │  ironclaw_common  ironclaw_host_api  ironclaw_prompt_envelope             │
   │  ironclaw_observability  ironclaw_architecture (test-only)                │
   │  ironclaw_reborn_traces                                                   │
   └──────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                      CRATE COUNT BY LAYER (68+ total in crates/)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

   Surfaces & Channels  ██████████  9
   Product & Compose    ██████████ 10
   Loops/Turns/LLM      ██████████ 10
   Kernel & Runtime     ████████████████████ 20
   Durable Substrate    ██████████ 10
   Foundation           █████  5
   Storage Backends     ████  4
                        ────
                         Total: 68


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                            FRONTEND — WebChat v2 SPA (ironclaw_webui/frontend/)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌──────────────────────────────────────────────────────────────────────────┐
  │  main.tsx (React 18 entry)                                                │
  │  ├── app/          Application shell, routing, state management           │
  │  ├── components/   Reusable UI components                                 │
  │  ├── design-system/ Design tokens, theme, component library               │
  │  ├── hooks/        Custom React hooks (useAuth, useThread, useTools…)     │
  │  ├── i18n/         Internationalization                                   │
  │  ├── layout/       Page layouts (chat, settings, admin)                   │
  │  ├── lib/          API client, WebSocket/SSE client, utilities            │
  │  ├── pages/        Route pages (Chat, Settings, Admin, Onboarding)        │
  │  ├── styles/       Global styles, CSS modules                             │
  │  ├── utils/        Pure helpers, formatters                               │
  │  └── test-support/ Test mocks, factories, fixtures                        │
  └──────────────────────────────────────────────────────────────────────────┘
       │
       │  HTTP REST + SSE (Server-Sent Events) + WebSocket
       ▼
  ┌──────────────────────────────────────────────────────────────────────────┐
  │  ironclaw_webui (Rust host)                                               │
  │  ├── webui_v2 route surface (axum handlers)                               │
  │  ├── webui_v2_app gateway assembly + middleware stack                     │
  │  ├── /auth/* OAuth login flow (Env/Session/OIDC authenticators)           │
  │  ├── SessionStore (cookie-based sessions)                                 │
  │  └── listener/serve loop                                                  │
  │                                                                           │
  │  ironclaw_webui_v2_static (thin Rust harness over Vite-built JS SPA)      │
  └──────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                                V1 LEGACY (Maintenance Only — src/)
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  ┌──────────────────────────────────────────────────────────────────────────┐
  │  src/  monolith (~10k LOC) — DEPRECATED, do not add features              │
  │  ─────────────────────────────────────────────────────────────────────── │
  │  ironclaw_engine (v1)   Legacy CodeAct executor, thread runtime, gates    │
  │  ironclaw_gateway (v1)  Legacy HTTP gateway, widget extensions            │
  │  ironclaw_tui (v1)      Legacy Ratatui TUI app, widgets, event loop       │
  │  ironclaw_embeddings     Legacy embedding providers (v1-only consumer)     │
  │                                                                           │
  │  → All new features go in crates/ (Reborn stack)                          │
  │  → v1 phased out under Tier B (docs/plans/2026-07-02-reborn-*.md)        │
  └──────────────────────────────────────────────────────────────────────────┘


═══════════════════════════════════════════════════════════════════════════════════════════════════════════
                                KEY ARCHITECTURAL INVARIANTS
═══════════════════════════════════════════════════════════════════════════════════════════════════════════

  1. The loop is NOT the security perimeter — kernel gates everything
  2. No ambient authority — loops request effects through CapabilityHost ports
  3. Secrets never inline — env var names only; actual secrets injected at transit
  4. Approval leases are exact-invocation scoped — no blanket grants
  5. Event sourcing is immutable — all state computable from events
  6. Dependency acyclic — no circular imports; upward flow only (Foundation → Substrate → Kernel → Products)
  7. One-active-run-per-thread — lock prevents duplicate work
  8. LLM data is never deleted — only marked/filtered, never removed from DB
  9. Test-first discipline — red → green, regression test with every fix
  10. Everything goes through capability dispatch — no back doors for product/WebUI handlers
  11. Filesystem is virtual — RootFilesystem mount catalog, scoped access, backend containment
  12. Dual-backend parity — PostgreSQL and libSQL behaviorally interchangeable (hooks_parity enforced)
  13. Prompt templates in .md files — not hardcoded in Rust strings
  14. No .unwrap() in production — typed errors with context via thiserror
  15. Strong types over strings — newtypes for all domain IDs, enums for fixed sets
```
