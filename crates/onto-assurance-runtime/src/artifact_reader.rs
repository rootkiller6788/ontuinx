//! SealedArtifactReader impl — reads from local staging filesystem.
use onto_protocol::candidate::ArtifactManifest;
use onto_protocol::verifier::SealedArtifactReader;
use std::path::PathBuf;
use sha2::Digest as ShaDigest;
use std::fs;

pub struct LocalArtifactReader { staging_root: PathBuf }

impl LocalArtifactReader {
    pub fn new(root: impl Into<PathBuf>) -> Self { Self { staging_root: root.into() } }
}

impl SealedArtifactReader for LocalArtifactReader {
    fn read_manifest(&self, _ref: &str) -> Result<ArtifactManifest, String> {
        let mut entries = vec![];
        if self.staging_root.exists() {
            let dir = fs::read_dir(&self.staging_root).map_err(|e| e.to_string())?;
            for entry in dir {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if path.is_file() {
                    let rel = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let content = fs::read(&path).map_err(|e| e.to_string())?;
                    let hash = hex::encode(sha2::Sha256::digest(&content));
                    entries.push(onto_protocol::candidate::ArtifactEntry {
                        path: rel, content_hash: hash,
                        size_bytes: content.len() as u64, is_new: true, is_modified: false,
                    });
                }
            }
        }
        Ok(ArtifactManifest { entries })
    }

    fn read_file(&self, _ref: &str, path: &str) -> Result<Vec<u8>, String> {
        fs::read(self.staging_root.join(path)).map_err(|e| format!("read {}: {}", path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn reads_existing_file() {
        let tmp = std::env::temp_dir().join("onto-artifact-test");
        let _ = fs::create_dir_all(&tmp);
        fs::write(tmp.join("hello.txt"), b"world").unwrap();
        let reader = LocalArtifactReader::new(&tmp);
        let data = reader.read_file("x", "hello.txt").unwrap();
        assert_eq!(data, b"world");
        let _ = fs::remove_dir_all(&tmp);
    }
}
