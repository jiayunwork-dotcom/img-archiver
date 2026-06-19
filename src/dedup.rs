use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

use crate::phash;
use crate::types::{DuplicateInfo, DuplicateType};

pub struct Deduplicator {
    sha256_index: HashMap<String, std::path::PathBuf>,
    phash_index: HashMap<u64, std::path::PathBuf>,
}

impl Deduplicator {
    pub fn new() -> Self {
        Self {
            sha256_index: HashMap::new(),
            phash_index: HashMap::new(),
        }
    }

    pub fn load_from_index(&mut self, index: &crate::index::ArchiveIndex) {
        for entry in &index.entries {
            self.sha256_index
                .insert(entry.sha256.clone(), entry.source_path.clone().into());
            self.phash_index.insert(entry.phash, entry.source_path.clone().into());
        }
    }

    pub fn compute_sha256(path: &Path) -> Result<String, String> {
        let mut hasher = Sha256::new();
        let mut file = std::fs::File::open(path)
            .map_err(|e| format!("Failed to open file for hashing: {}", e))?;
        std::io::copy(&mut file, &mut hasher)
            .map_err(|e| format!("Failed to read file for hashing: {}", e))?;
        let result = hasher.finalize();
        Ok(format!("{:x}", result))
    }

    pub fn compute_phash(path: &Path) -> Result<u64, String> {
        phash::compute_phash(path)
    }

    pub fn check_duplicate(&self, sha256: &str, phash: u64) -> DuplicateInfo {
        if let Some(original) = self.sha256_index.get(sha256) {
            return DuplicateInfo {
                dup_type: DuplicateType::Exact,
                original_path: Some(original.clone()),
            };
        }

        for (&existing_hash, original) in &self.phash_index {
            let dist = phash::hamming_distance(existing_hash, phash);
            if dist <= 5 {
                return DuplicateInfo {
                    dup_type: DuplicateType::Suspected,
                    original_path: Some(original.clone()),
                };
            }
        }

        DuplicateInfo {
            dup_type: DuplicateType::None,
            original_path: None,
        }
    }

    pub fn register(&mut self, sha256: &str, phash: u64, path: &Path) {
        self.sha256_index
            .insert(sha256.to_string(), path.to_path_buf());
        self.phash_index.insert(phash, path.to_path_buf());
    }

    pub fn get_phash_for_path(&self, target_path: &Path) -> Option<u64> {
        for (&phash, path) in &self.phash_index {
            if path == target_path {
                return Some(phash);
            }
        }
        None
    }
}
