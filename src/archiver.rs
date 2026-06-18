use crate::types::ArchiveMode;
use std::path::Path;

pub struct Archiver {
    pub mode: ArchiveMode,
    pub dry_run: bool,
    pub create_dirs: bool,
}

#[allow(dead_code)]
pub struct ArchiveResult {
    pub source: std::path::PathBuf,
    pub target: std::path::PathBuf,
    pub success: bool,
    pub error: Option<String>,
}

impl Archiver {
    pub fn new(mode: ArchiveMode, dry_run: bool, create_dirs: bool) -> Self {
        Self {
            mode,
            dry_run,
            create_dirs,
        }
    }

    pub fn archive_file(&self, source: &Path, target: &Path) -> ArchiveResult {
        if self.dry_run {
            println!(
                "  [DRY-RUN] {} -> {}",
                source.display(),
                target.display()
            );
            return ArchiveResult {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                success: true,
                error: None,
            };
        }

        if let Some(parent) = target.parent() {
            if !parent.exists() {
                if self.create_dirs {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        return ArchiveResult {
                            source: source.to_path_buf(),
                            target: target.to_path_buf(),
                            success: false,
                            error: Some(format!("Failed to create directory: {}", e)),
                        };
                    }
                } else {
                    return ArchiveResult {
                        source: source.to_path_buf(),
                        target: target.to_path_buf(),
                        success: false,
                        error: Some(format!(
                            "Target directory does not exist: {}",
                            parent.display()
                        )),
                    };
                }
            }
        }

        let final_target = self.resolve_conflict(source, target);

        match self.mode {
            ArchiveMode::Copy => self.copy_file(source, &final_target),
            ArchiveMode::Move => self.move_file(source, &final_target),
            ArchiveMode::Link => self.link_file(source, &final_target),
        }
    }

    fn resolve_conflict(&self, source: &Path, target: &Path) -> std::path::PathBuf {
        if !target.exists() {
            return target.to_path_buf();
        }

        let source_hash = crate::dedup::Deduplicator::compute_sha256(source);
        let target_hash = crate::dedup::Deduplicator::compute_sha256(target);

        if let (Ok(ref sh), Ok(ref th)) = (&source_hash, &target_hash) {
            if sh == th {
                return target.to_path_buf();
            }
        }

        let stem = target
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = target
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
            let new_target = target.with_file_name(new_name);
            if !new_target.exists() {
                return new_target;
            }

            let new_hash = crate::dedup::Deduplicator::compute_sha256(&new_target);
            if let (Ok(ref sh), Ok(ref nh)) = (&source_hash, &new_hash) {
                if sh == nh {
                    return new_target;
                }
            }

            counter += 1;
            if counter > 1000 {
                return new_target;
            }
        }
    }

    fn copy_file(&self, source: &Path, target: &Path) -> ArchiveResult {
        match std::fs::copy(source, target) {
            Ok(_) => ArchiveResult {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                success: true,
                error: None,
            },
            Err(e) => ArchiveResult {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                success: false,
                error: Some(format!("Copy failed: {}", e)),
            },
        }
    }

    fn move_file(&self, source: &Path, target: &Path) -> ArchiveResult {
        if let Err(e) = std::fs::rename(source, target) {
            if let Err(copy_err) = std::fs::copy(source, target) {
                return ArchiveResult {
                    source: source.to_path_buf(),
                    target: target.to_path_buf(),
                    success: false,
                    error: Some(format!(
                        "Move failed (rename: {}, copy: {})",
                        e, copy_err
                    )),
                };
            }
            if let Err(rm_err) = std::fs::remove_file(source) {
                return ArchiveResult {
                    source: source.to_path_buf(),
                    target: target.to_path_buf(),
                    success: false,
                    error: Some(format!("Move failed (remove source: {})", rm_err)),
                };
            }
        }
        ArchiveResult {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
            success: true,
            error: None,
        }
    }

    fn link_file(&self, source: &Path, target: &Path) -> ArchiveResult {
        match std::fs::hard_link(source, target) {
            Ok(_) => ArchiveResult {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                success: true,
                error: None,
            },
            Err(e) => ArchiveResult {
                source: source.to_path_buf(),
                target: target.to_path_buf(),
                success: false,
                error: Some(format!("Hard link failed: {}", e)),
            },
        }
    }
}
