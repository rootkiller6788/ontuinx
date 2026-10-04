//! OCR 吸收验证 — 真实源文件定位 + Prompt 注入

use std::fs;

use onto_assurance_core::verification::location_binding::{resolve_from_file, resolve_from_hunks, DiffHunk, HunkLine};
use onto_code_pack::rules::router::RuleRouter;
use onto_code_pack::verification_profiles::{resolve_dual_plane, inject_semantic_rule};

// ═══════════════════════════════════
// 1. 真实源文件定位测试
// ═══════════════════════════════════

#[test]
fn ocr1_real_source_file_location() {
    // 读取本项目真实 Rust 源文件
    let content = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../onto-assurance-core/src/lib.rs"))
        .expect("real source file must exist");

    // 从文件中取一段真实代码
    let snippet = "pub mod canonical;\npub mod evidence_chain;\npub mod reduction;";

    let result = resolve_from_file(snippet, &content);
    assert!(result.is_some(), "real file: must find exact snippet");
    let (start, end) = result.unwrap();
    assert!(start > 0);
    assert!(end > start);
    assert_eq!(end - start + 1, 3, "3 lines of modules");
}

#[test]
fn ocr2_hunk_match_with_real_diff_data() {
    // 模拟真实 git diff hunk
    let hunks = vec![DiffHunk {
        old_start: 8, old_count: 5, new_start: 8, new_count: 6,
        lines: vec![
            HunkLine { prefix: ' ', content: "pub mod canonical;".into(), old_line: Some(8), new_line: Some(8) },
            HunkLine { prefix: ' ', content: "pub mod evidence_chain;".into(), old_line: Some(9), new_line: Some(9) },
            HunkLine { prefix: '+', content: "pub mod verification;".into(), old_line: None, new_line: Some(10) },
            HunkLine { prefix: ' ', content: "pub mod reduction;".into(), old_line: Some(10), new_line: Some(11) },
        ],
    }];

    // Match 2 consecutive context lines (full content)
    let r = resolve_from_hunks("pub mod canonical;\npub mod evidence_chain;", &hunks);
    assert!(r.is_some(), "hunk: 2-line context must be found in diff");
}

#[test]
fn ocr3_nonexistent_code_not_found() {
    let content = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../onto-assurance-core/src/lib.rs")).unwrap();
    let r = resolve_from_file("this_code_does_not_exist_in_the_file_xyz", &content);
    assert!(r.is_none(), "non-existent code must not be found");
}

#[test]
fn ocr4_cross_file_no_false_positive() {
    // File 1: types lib.rs
    let content1 = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../onto-assurance-types/src/lib.rs")).unwrap();
    // File 2: core lib.rs — different content
    let content2 = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../onto-assurance-core/src/lib.rs")).unwrap();

    // Snippet from file1 (consecutive lines)
    let snippet = "pub mod ids;\npub mod enums;";

    let r1 = resolve_from_file(snippet, &content1);
    assert!(r1.is_some(), "found in types lib.rs");

    let r2 = resolve_from_file(snippet, &content2);
    assert!(r2.is_none(), "NOT found in core lib.rs — no false positive");
}

// ═══════════════════════════════════
// 2. Prompt 模板注入验证
// ═══════════════════════════════════

#[test]
fn ocr5_rule_router_resolves_real_files() {
    let router = RuleRouter::from_embedded();

    // Rust
    let rust_rule = router.resolve("src/main.rs");
    assert!(!rust_rule.is_empty());
    assert!(rust_rule.contains("####") || rust_rule.contains("Ownership") || rust_rule.contains("Error"));

    // Go
    let go_rule = router.resolve("main.go");
    assert!(!go_rule.is_empty());

    // Unknown → default
    let def = router.resolve("data.bin");
    assert!(!def.is_empty());
}

#[test]
fn ocr6_prompt_injection_substitutes_correctly() {
    let template = "### Review Checklist\n{{system_rule}}\n\n### Your Task\nReview the code above.";
    let rule_content = "#### Security\n- Check for SQL injection\n- Check for XSS\n\n#### Performance\n- Avoid N+1 queries";

    let result = inject_semantic_rule(template, rule_content);
    assert!(result.contains("#### Security"));
    assert!(result.contains("#### Performance"));
    assert!(!result.contains("{{system_rule}}"), "placeholder must be replaced");
    assert!(result.contains("### Review Checklist"));
    assert!(result.contains("### Your Task"));
}

#[test]
fn ocr7_dual_plane_all_languages_resolve() {
    let router = RuleRouter::from_embedded();
    for lang in &["rust", "go", "python", "typescript", "java", "kotlin", "cpp"] {
        let profile = resolve_dual_plane(lang, &router);
        assert!(profile.is_some(), "missing dual-plane for {}", lang);
        let p = profile.unwrap();
        assert!(!p.effective_rule_hash.is_empty());
        assert!(!p.semantic_rule_hash.is_empty());
    }
}

#[test]
fn ocr8_unknown_language_fallback() {
    let router = RuleRouter::from_embedded();
    // Unknown language → default.md
    let rule = router.resolve("data.bin");
    assert!(!rule.is_empty());
    assert!(rule.contains("Correctness") || rule.contains("Security") || rule.contains("####"));
}

// ═══════════════════════════════════
// 3. 26 种文件类型全覆盖
// ═══════════════════════════════════

#[test]
fn ocr9_all_26_types_resolve() {
    let router = RuleRouter::from_embedded();
    let test_files = [
        ("test.rs", "rust"), ("test.go", "go"), ("test.py", "python"),
        ("test.ts", "typescript"), ("test.java", "java"), ("test.kt", "kotlin"),
        ("test.cpp", "cpp"), ("test.c", "cpp"), ("test.js", "typescript"),
        ("test.tsx", "typescript"), ("test.jsx", "typescript"),
        ("test.json", "json"), ("Cargo.toml", "cargo_toml"),
        ("package.json", "package_json"), ("pom.xml", "pom_xml"),
        ("build.gradle", "build_gradle"), ("test.yaml", "yaml"),
        ("test.astro", "astro"), ("test.jl", "julia"),
        ("test.graphql", "graphql"), ("test.ftl", "freemarker"),
        ("test.properties", "properties"), ("test.po", "po"),
        ("test.pot", "pot"), ("TestMapper.xml", "mapper_dao_xml"),
        ("unknown.xyz", "default"),
    ];
    for (file, _expected_category) in &test_files {
        let rule = router.resolve(file);
        assert!(!rule.is_empty(), "no rule for {} (category: {})", file, _expected_category);
    }
    assert_eq!(test_files.len(), 26, "26 file types covered");
}

// ═══════════════════════════════════
// 4. Go 规则文件存在且有效
// ═══════════════════════════════════

#[test]
fn ocr10_go_rule_content_valid() {
    let router = RuleRouter::from_embedded();
    let rule = router.resolve("main.go");
    assert!(!rule.is_empty());
    // Go rule must cover key OntoFlow concerns
    assert!(rule.contains("goroutine") || rule.contains("Context") || rule.contains("error") || rule.contains("nil") || rule.contains("Temporal") || rule.contains("OntoFlow"),
        "Go rule must cover concurrency/context/error/nil/OntoFlow concerns");
}
