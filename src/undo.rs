use crate::types::ArchiveMode;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoEntry {
    pub source_path: String,
    pub archive_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoRecord {
    pub version: String,
    pub mode: String,
    pub timestamp: String,
    pub entries: Vec<UndoEntry>,
}

impl UndoRecord {
    pub fn new(mode: ArchiveMode) -> Self {
        let mode_str = match mode {
            ArchiveMode::Copy => "copy",
            ArchiveMode::Move => "move",
            ArchiveMode::Link => "link",
        };
        Self {
            version: "1.0".to_string(),
            mode: mode_str.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, source_path: String, archive_path: String) {
        self.entries.push(UndoEntry {
            source_path,
            archive_path,
        });
    }

    pub fn load(output_dir: &Path) -> Result<Self, String> {
        let undo_path = output_dir.join(".archive-undo.json");
        if !undo_path.exists() {
            return Err("No undo record found (.archive-undo.json does not exist)".to_string());
        }
        let content = std::fs::read_to_string(&undo_path)
            .map_err(|e| format!("Failed to read undo record: {}", e))?;
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse undo record: {}", e))
    }

    pub fn save(&self, output_dir: &Path) -> Result<(), String> {
        let undo_path = output_dir.join(".archive-undo.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize undo record: {}", e))?;
        std::fs::write(&undo_path, content)
            .map_err(|e| format!("Failed to write undo record: {}", e))
    }

    pub fn delete(output_dir: &Path) -> Result<(), String> {
        let undo_path = output_dir.join(".archive-undo.json");
        if undo_path.exists() {
            std::fs::remove_file(&undo_path)
                .map_err(|e| format!("Failed to delete undo record: {}", e))?;
        }
        Ok(())
    }

    pub fn execute(&self) -> Result<(usize, usize), String> {
        let mut restored = 0usize;
        let mut failed = 0usize;

        let mode = ArchiveMode::from_str(&self.mode)
            .map_err(|e| format!("Invalid mode in undo record: {}", e))?;

        for entry in &self.entries {
            let src = Path::new(&entry.source_path);
            let dst = Path::new(&entry.archive_path);

            let result = match mode {
                ArchiveMode::Copy => undo_copy(dst),
                ArchiveMode::Move => undo_move(src, dst),
                ArchiveMode::Link => undo_link(dst),
            };

            match result {
                Ok(_) => restored += 1,
                Err(e) => {
                    eprintln!("  Failed to undo {}: {}", dst.display(), e);
                    failed += 1;
                }
            }
        }

        Ok((restored, failed))
    }
}

fn undo_copy(archive_path: &Path) -> Result<(), String> {
    if archive_path.exists() {
        std::fs::remove_file(archive_path)
            .map_err(|e| format!("Failed to remove copied file: {}", e))?;
    }
    Ok(())
}

fn undo_move(source_path: &Path, archive_path: &Path) -> Result<(), String> {
    if !archive_path.exists() {
        return Err("Archived file does not exist".to_string());
    }

    if let Some(parent) = source_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create source directory: {}", e))?;
        }
    }

    let mut final_source = source_path.to_path_buf();
    if source_path.exists() {
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = source_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let mut counter = 2u32;
        loop {
            let new_name = if ext.is_empty() {
                format!("{}_{}", stem, counter)
            } else {
                format!("{}_{}.{}", stem, counter, ext)
            };
            final_source = source_path.with_file_name(new_name);
            if !final_source.exists() {
                break;
            }
            counter += 1;
            if counter > 1000 {
                return Err("Could not find a unique filename for restored file".to_string());
            }
        }
    }

    std::fs::rename(archive_path, &final_source)
        .map_err(|e| format!("Failed to move file back: {}", e))?;

    Ok(())
}

fn undo_link(archive_path: &Path) -> Result<(), String> {
    if archive_path.exists() {
        std::fs::remove_file(archive_path)
            .map_err(|e| format!("Failed to remove hard link: {}", e))?;
    }
    Ok(())
}
