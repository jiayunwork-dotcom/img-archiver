use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub source_path: String,
    pub archive_path: String,
    pub sha256: String,
    pub phash: u64,
    pub archived_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveIndex {
    pub version: String,
    pub entries: Vec<IndexEntry>,
}

impl ArchiveIndex {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            entries: Vec::new(),
        }
    }

    pub fn load(output_dir: &Path) -> Result<Self, String> {
        let index_path = output_dir.join(".archive-index.json");
        if !index_path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(&index_path)
            .map_err(|e| format!("Failed to read archive index: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse archive index: {}", e))
    }

    pub fn save(&self, output_dir: &Path) -> Result<(), String> {
        let index_path = output_dir.join(".archive-index.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize archive index: {}", e))?;
        std::fs::write(&index_path, content)
            .map_err(|e| format!("Failed to write archive index: {}", e))
    }

    #[allow(dead_code)]
    pub fn contains_sha256(&self, sha256: &str) -> bool {
        self.entries.iter().any(|e| e.sha256 == sha256)
    }

    pub fn add_entry(&mut self, entry: IndexEntry) {
        self.entries.push(entry);
    }

    pub fn remove_by_archive_path(&mut self, archive_path: &str) {
        self.entries.retain(|e| e.archive_path != archive_path);
    }

    pub fn sha256_set(&self) -> std::collections::HashSet<String> {
        self.entries.iter().map(|e| e.sha256.clone()).collect()
    }
}
