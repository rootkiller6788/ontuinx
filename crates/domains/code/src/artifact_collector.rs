//! Artifact collector — collects and hashes build/test outputs.
//! Deterministic: same files → same hashes.

use onto_pack_sdk::ArtifactDescriptor;
use sha2::{Sha256, Digest};

/// Collect artifacts from a workspace directory listing.
/// The caller provides file paths and their contents as byte slices.
pub struct ArtifactCollector;

impl ArtifactCollector {
    /// Hash a single file's content.
    pub fn hash_content(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        hex::encode(hasher.finalize())
    }

    /// Collect descriptors for a list of files with their contents.
    pub fn collect(files: &[(&str, &[u8])]) -> Vec<ArtifactDescriptor> {
        files.iter().map(|(path, content)| {
            let content_hash = Self::hash_content(content);
            let kind = classify_artifact(path);
            ArtifactDescriptor {
                path: (*path).into(),
                content_hash,
                size_bytes: content.len() as u64,
                kind,
            }
        }).collect()
    }
}

fn classify_artifact(path: &str) -> onto_pack_sdk::ArtifactKind {
    if path.ends_with(".c") || path.ends_with(".rs") || path.ends_with(".py") || path.ends_with(".cpp") {
        onto_pack_sdk::ArtifactKind::SourceFile
    } else if path.ends_with(".o") || path.ends_with(".so") || path.contains("target/release") {
        onto_pack_sdk::ArtifactKind::Binary
    } else if path.contains("test") && (path.ends_with(".xml") || path.ends_with(".json")) {
        onto_pack_sdk::ArtifactKind::TestReport
    } else if path.ends_with(".log") || path.contains("build") {
        onto_pack_sdk::ArtifactKind::BuildLog
    } else if path.ends_with(".diff") || path.ends_with(".patch") {
        onto_pack_sdk::ArtifactKind::Diff
    } else {
        onto_pack_sdk::ArtifactKind::Custom(path.rsplit('.').next().unwrap_or("").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_deterministic() {
        let h1 = ArtifactCollector::hash_content(b"hello");
        let h2 = ArtifactCollector::hash_content(b"hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_different_content() {
        let h1 = ArtifactCollector::hash_content(b"hello");
        let h2 = ArtifactCollector::hash_content(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn collect_and_classify() {
        let files = vec![
            ("src/main.rs", b"fn main() {}" as &[u8]),
            ("src/lib.rs", b"pub fn add(a: i32, b: i32) -> i32 { a + b }" as &[u8]),
            ("target/test-report.xml", b"<testsuite tests='9'/>" as &[u8]),
        ];
        let artifacts = ArtifactCollector::collect(&files);
        assert_eq!(artifacts.len(), 3);
        assert!(matches!(artifacts[0].kind, onto_pack_sdk::ArtifactKind::SourceFile));
        assert!(matches!(artifacts[2].kind, onto_pack_sdk::ArtifactKind::TestReport));
    }
}
