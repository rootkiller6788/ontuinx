# Domain Plugins

Each subdirectory is a domain plugin that implements `onto_pack::DomainPlugin`.

## Convention

```
domains/<name>/
├── Cargo.toml    ← name = "onto-assurance-domain-<name>"
└── src/
    └── lib.rs    ← impl DomainPlugin { fn register(&self, registry) }
```

## Adding a domain

1. Copy `stubs/` as a template
2. Implement at least one of: `LocationResolver`, `CriterionMapper`, `RuleSelector`, `ScopeProvider`
3. Implement `DomainPlugin::register()` to wire everything into the registry
4. Add `"crates/domains/<name>"` to workspace `Cargo.toml`

## Current domains

| Directory | Status | Description |
|-----------|--------|-------------|
| `code/`   | Reference | Code verification (7 languages, git diff, 26 rule files, dual-channel location) |
| `stubs/`  | Stubs | 20 domain stubs proving SPI extensibility |

## Future domains (examples)

| Domain | Target kinds | Key verifiers |
|--------|-------------|---------------|
| `document/` | Markdown, OpenAPI, ADR | Section completeness, link validity, schema conformance |
| `dataset/`  | PostgreSQL schema, Parquet | PII detection, constraint check, schema drift |
| `workflow/` | Temporal/OntoFlow DAG | Reachability, deadlock, idempotency, authority gate |
| `chip/`     | RTL, gate-level netlist | Timing closure, power integrity, DFT coverage |
| `medical/`  | Device spec, clinical data | Biocompatibility, traceability, IEC 62304 |
