use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
#[command(name = "img-archiver", version, about = "Batch image metadata extraction and intelligent archiving tool")]
pub struct Cli {
    #[arg(help = "Source directory to scan for images")]
    pub source: PathBuf,

    #[arg(long, help = "Output directory for archived images")]
    pub output: PathBuf,

    #[arg(long, default_value = "{year}/{month}/{day}_{camera}_{seq}.{ext}", help = "Archive path template")]
    pub template: String,

    #[arg(long, default_value = "copy", help = "Archive mode: copy, move, link")]
    pub mode: String,

    #[arg(long, help = "Exclude paths matching glob pattern (repeatable)")]
    pub exclude: Vec<String>,

    #[arg(long, help = "Skip confirmation prompt")]
    pub yes: bool,

    #[arg(long, help = "Dry run mode - preview without executing")]
    pub dry_run: bool,

    #[arg(long, help = "Auto-create output directories")]
    pub create_dirs: bool,

    #[arg(long, default_value = "3", help = "Number of digits for sequence number")]
    pub seq_digits: usize,

    #[arg(long, default_value = "misc", help = "Placeholder for unknown variable values")]
    pub unknown_placeholder: String,

    #[arg(long, help = "YAML config file path")]
    pub config: Option<PathBuf>,

    #[arg(long, help = "Show extra statistics")]
    pub stats: bool,

    #[arg(long, help = "Export report as JSON to specified path")]
    pub report_json: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub source: Option<String>,
    pub output: Option<String>,
    pub template: Option<String>,
    pub mode: Option<String>,
    pub exclude: Option<Vec<String>>,
    pub seq_digits: Option<usize>,
    pub unknown_placeholder: Option<String>,
    pub create_dirs: Option<bool>,
    pub rules: Option<Vec<ArchiveRule>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveRule {
    pub name: String,
    pub filter_format: Option<Vec<String>>,
    pub filter_camera: Option<Vec<String>>,
    pub template: String,
}

impl AppConfig {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse config YAML: {}", e))
    }

    pub fn apply_to_cli(&self, cli: &mut Cli) {
        if let Some(ref template) = self.template {
            cli.template = template.clone();
        }
        if let Some(ref mode) = self.mode {
            cli.mode = mode.clone();
        }
        if let Some(ref exclude) = self.exclude {
            if cli.exclude.is_empty() {
                cli.exclude = exclude.clone();
            }
        }
        if let Some(digits) = self.seq_digits {
            cli.seq_digits = digits;
        }
        if let Some(ref ph) = self.unknown_placeholder {
            cli.unknown_placeholder = ph.clone();
        }
        if let Some(true) = self.create_dirs {
            cli.create_dirs = true;
        }
    }
}
