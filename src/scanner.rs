use crate::types::{ImageFormat, ScanSummary};
use globset::GlobSet;
use std::path::Path;
use walkdir::WalkDir;

const MIN_FILE_SIZE: u64 = 1024;

pub struct Scanner {
    source: std::path::PathBuf,
    exclude_set: Option<GlobSet>,
}

impl Scanner {
    pub fn new(source: &Path, exclude_patterns: &[String]) -> Result<Self, String> {
        if !source.exists() {
            return Err(format!("Source directory does not exist: {}", source.display()));
        }
        if !source.is_dir() {
            return Err(format!("Source path is not a directory: {}", source.display()));
        }

        let exclude_set = if exclude_patterns.is_empty() {
            None
        } else {
            let mut builder = globset::GlobSetBuilder::new();
            for pattern in exclude_patterns {
                let glob = globset::Glob::new(pattern)
                    .map_err(|e| format!("Invalid exclude pattern '{}': {}", pattern, e))?;
                builder.add(glob);
            }
            Some(builder.build().map_err(|e| format!("Failed to build glob set: {}", e))?)
        };

        Ok(Self {
            source: source.to_path_buf(),
            exclude_set,
        })
    }

    pub fn scan(&self) -> Result<Vec<(std::path::PathBuf, u64, ImageFormat)>, String> {
        let mut results = Vec::new();

        for entry in WalkDir::new(&self.source).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let rel = path.strip_prefix(&self.source).unwrap_or(path);
            if let Some(ref ex) = self.exclude_set {
                if ex.is_match(rel) {
                    continue;
                }
            }

            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e,
                None => continue,
            };

            let fmt = match ImageFormat::from_ext(ext) {
                Some(f) => f,
                None => continue,
            };

            let file_size = match std::fs::metadata(path) {
                Ok(m) => m.len(),
                Err(_) => continue,
            };

            if file_size < MIN_FILE_SIZE {
                continue;
            }

            results.push((path.to_path_buf(), file_size, fmt));
        }

        results.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(results)
    }
}

pub fn build_summary(entries: &[(std::path::PathBuf, u64, ImageFormat)]) -> ScanSummary {
    let mut format_counts = std::collections::HashMap::new();
    let mut total_size: u64 = 0;

    for (_, size, fmt) in entries {
        *format_counts.entry(*fmt).or_insert(0) += 1;
        total_size += size;
    }

    ScanSummary {
        total_count: entries.len(),
        total_size,
        format_counts,
    }
}

pub fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

pub fn print_summary(summary: &ScanSummary) {
    println!("╔══════════════════════════════════════╗");
    println!("║        Scan Summary                  ║");
    println!("╠══════════════════════════════════════╣");
    println!("║ Total images: {:>20} ║", summary.total_count);
    println!("║ Total size:   {:>20} ║", format_size(summary.total_size));
    println!("╠══════════════════════════════════════╣");

    let mut formats: Vec<_> = summary.format_counts.iter().collect();
    formats.sort_by_key(|(f, _)| f.as_str());
    for (fmt, count) in &formats {
        println!("║ {:>5}: {:>28} ║", fmt.as_str(), count);
    }

    println!("╚══════════════════════════════════════╝");
}

pub fn confirm() -> bool {
    use std::io::{self, BufRead, Write};
    print!("Proceed with archiving? [y/N] ");
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().lock().read_line(&mut input).ok();
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
