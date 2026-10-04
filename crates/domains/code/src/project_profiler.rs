//! Project profiler — detects language, build system, and test framework.
//!
//! Deterministic: scans workspace files and returns a profile.
//! No I/O beyond what the caller provides via file listings.

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════
// ProjectProfile
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectProfile {
    pub language: Language,
    pub build_system: BuildSystem,
    pub test_framework: TestFramework,
    pub source_files: Vec<String>,
    pub entry_point: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    C,
    Cpp,
    Rust,
    Python,
    Go,
    Java,
    JavaScript,
    TypeScript,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildSystem {
    Gcc,
    Gpp,
    Cargo,
    Make,
    Cmake,
    Meson,
    Bazel,
    Pip,
    Npm,
    GoBuild,
    Maven,
    Gradle,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestFramework {
    CTest,
    CargoTest,
    Pytest,
    Jest,
    GoTest,
    JUnit,
    None,
}

// ══════════════════════════════════════════════════════════════════
// Profiler
// ══════════════════════════════════════════════════════════════════

/// Profile a project from its file listing.
///
/// The caller provides the list of files visible in the workspace.
/// The profiler is deterministic — same files → same profile.
pub fn profile_project(files: &[String]) -> ProjectProfile {
    let extensions: std::collections::HashSet<&str> = files
        .iter()
        .filter_map(|f| f.rsplit('.').next())
        .collect();

    let language = detect_language(&extensions, files);
    let build_system = detect_build_system(files);
    let test_framework = detect_test_framework(&language, files);
    let source_files: Vec<String> = files.iter()
        .filter(|f| is_source_file(f, &language))
        .cloned()
        .collect();
    let entry_point = detect_entry_point(files, &language);

    ProjectProfile {
        language,
        build_system,
        test_framework,
        source_files,
        entry_point,
    }
}

fn detect_language(extensions: &std::collections::HashSet<&str>, files: &[String]) -> Language {
    if extensions.contains("rs") || files.iter().any(|f| f.as_str() == "Cargo.toml") {
        return Language::Rust;
    }
    if extensions.contains("c") && !extensions.contains("cpp") && !extensions.contains("cc") {
        return Language::C;
    }
    if extensions.contains("cpp") || extensions.contains("cc") || extensions.contains("cxx") {
        return Language::Cpp;
    }
    if extensions.contains("py") {
        return Language::Python;
    }
    if extensions.contains("go") {
        return Language::Go;
    }
    if extensions.contains("java") {
        return Language::Java;
    }
    if extensions.contains("js") || extensions.contains("mjs") {
        return Language::JavaScript;
    }
    if extensions.contains("ts") {
        return Language::TypeScript;
    }
    Language::Unknown
}

fn detect_build_system(files: &[String]) -> BuildSystem {
    if files.iter().any(|f| f.as_str() == "Cargo.toml") { return BuildSystem::Cargo; }
    if files.iter().any(|f| matches!(f.as_str(), "Makefile" | "makefile" | "GNUmakefile")) { return BuildSystem::Make; }
    if files.iter().any(|f| f.as_str() == "CMakeLists.txt") { return BuildSystem::Cmake; }
    if files.iter().any(|f| f.as_str() == "meson.build") { return BuildSystem::Meson; }
    if files.iter().any(|f| matches!(f.as_str(), "BUILD" | "BUILD.bazel")) { return BuildSystem::Bazel; }
    if files.iter().any(|f| matches!(f.as_str(), "setup.py" | "pyproject.toml")) { return BuildSystem::Pip; }
    if files.iter().any(|f| f.as_str() == "package.json") { return BuildSystem::Npm; }
    if files.iter().any(|f| f.as_str() == "go.mod") { return BuildSystem::GoBuild; }
    if files.iter().any(|f| f.as_str() == "pom.xml") { return BuildSystem::Maven; }
    if files.iter().any(|f| matches!(f.as_str(), "build.gradle" | "build.gradle.kts")) { return BuildSystem::Gradle; }
    BuildSystem::None
}

fn detect_test_framework(language: &Language, _files: &[String]) -> TestFramework {
    match language {
        Language::C | Language::Cpp => TestFramework::CTest,
        Language::Rust => TestFramework::CargoTest,
        Language::Python => TestFramework::Pytest,
        Language::JavaScript | Language::TypeScript => TestFramework::Jest,
        Language::Go => TestFramework::GoTest,
        Language::Java => TestFramework::JUnit,
        _ => TestFramework::None,
    }
}

fn is_source_file(path: &str, language: &Language) -> bool {
    match language {
        Language::C => path.ends_with(".c") || path.ends_with(".h"),
        Language::Cpp => path.ends_with(".cpp") || path.ends_with(".cc") || path.ends_with(".hpp") || path.ends_with(".h"),
        Language::Rust => path.ends_with(".rs"),
        Language::Python => path.ends_with(".py"),
        Language::Go => path.ends_with(".go"),
        Language::Java => path.ends_with(".java"),
        Language::JavaScript => path.ends_with(".js") || path.ends_with(".mjs"),
        Language::TypeScript => path.ends_with(".ts"),
        Language::Unknown => false,
    }
}

fn detect_entry_point(files: &[String], language: &Language) -> Option<String> {
    match language {
        Language::C | Language::Cpp => {
            files.iter().find(|f| matches!(f.as_str(), "main.c" | "src/main.c")).cloned()
        }
        Language::Rust => {
            files.iter().find(|f| f.as_str() == "src/main.rs").cloned()
        }
        Language::Python => {
            files.iter().find(|f| matches!(f.as_str(), "main.py" | "__main__.py" | "app.py")).cloned()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_c_project() {
        let files = vec!["main.c".into(), "util.c".into(), "util.h".into(), "Makefile".into()];
        let profile = profile_project(&files);
        assert_eq!(profile.language, Language::C);
        assert_eq!(profile.build_system, BuildSystem::Make);
        assert_eq!(profile.test_framework, TestFramework::CTest);
        assert_eq!(profile.source_files.len(), 3);
        assert_eq!(profile.entry_point, Some("main.c".into()));
    }

    #[test]
    fn detect_rust_project() {
        let files = vec!["Cargo.toml".into(), "src/main.rs".into(), "src/lib.rs".into()];
        let profile = profile_project(&files);
        assert_eq!(profile.language, Language::Rust);
        assert_eq!(profile.build_system, BuildSystem::Cargo);
        assert_eq!(profile.test_framework, TestFramework::CargoTest);
        assert_eq!(profile.entry_point, Some("src/main.rs".into()));
    }

    #[test]
    fn unknown_project() {
        let files: Vec<String> = vec![];
        let profile = profile_project(&files);
        assert_eq!(profile.language, Language::Unknown);
        assert_eq!(profile.build_system, BuildSystem::None);
    }

    #[test]
    fn detect_cpp_project() {
        let files = vec![
            "main.cpp".into(), "util.cpp".into(), "util.hpp".into(),
            "CMakeLists.txt".into(),
        ];
        let profile = profile_project(&files);
        assert_eq!(profile.language, Language::Cpp);
        assert_eq!(profile.build_system, BuildSystem::Cmake);
        assert_eq!(profile.test_framework, TestFramework::CTest);
        assert_eq!(profile.source_files.len(), 3);
    }

    #[test]
    fn detect_javascript_project() {
        let files = vec![
            "package.json".into(), "index.js".into(), "util.mjs".into(),
        ];
        let profile = profile_project(&files);
        assert_eq!(profile.language, Language::JavaScript);
        assert_eq!(profile.build_system, BuildSystem::Npm);
        assert_eq!(profile.test_framework, TestFramework::Jest);
        assert_eq!(profile.source_files.len(), 2);
    }

    #[test]
    fn python_project_detected() {
        let files = vec![
            "pyproject.toml".into(), "main.py".into(), "test_main.py".into(),
        ];
        let profile = profile_project(&files);
        assert_eq!(profile.language, Language::Python);
        assert_eq!(profile.build_system, BuildSystem::Pip);
        assert_eq!(profile.test_framework, TestFramework::Pytest);
        assert_eq!(profile.entry_point, Some("main.py".into()));
    }
}
