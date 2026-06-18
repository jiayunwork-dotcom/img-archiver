use crate::types::ImageInfo;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct Stats {
    pub by_camera: HashMap<String, usize>,
    pub by_year_month: HashMap<String, usize>,
    pub by_format: HashMap<String, usize>,
    pub total_with_gps: usize,
    pub total: usize,
    pub resolution_dist: ResolutionDist,
}

#[derive(Debug, Default)]
pub struct ResolutionDist {
    pub ultra_hd: usize,
    pub full_hd: usize,
    pub hd: usize,
    pub sd: usize,
    pub other: usize,
}

impl Stats {
    pub fn from_images(images: &[ImageInfo]) -> Self {
        let mut stats = Stats::default();
        stats.total = images.len();

        for img in images {
            let camera = if img.metadata.camera_model != "unknown" {
                if img.metadata.camera_brand != "unknown" {
                    format!("{} {}", img.metadata.camera_brand, img.metadata.camera_model)
                } else {
                    img.metadata.camera_model.clone()
                }
            } else {
                "unknown".to_string()
            };
            *stats.by_camera.entry(camera).or_insert(0) += 1;

            let ym = format!("{}", img.metadata.date_time.format("%Y-%m"));
            *stats.by_year_month.entry(ym).or_insert(0) += 1;

            let fmt = img.format.as_str().to_string();
            *stats.by_format.entry(fmt).or_insert(0) += 1;

            if img.metadata.gps.is_some() {
                stats.total_with_gps += 1;
            }

            let w = img.metadata.width;
            let h = img.metadata.height;
            let max_dim = w.max(h);

            if max_dim >= 3840 {
                stats.resolution_dist.ultra_hd += 1;
            } else if max_dim >= 1920 {
                stats.resolution_dist.full_hd += 1;
            } else if max_dim >= 1280 {
                stats.resolution_dist.hd += 1;
            } else if max_dim >= 720 {
                stats.resolution_dist.sd += 1;
            } else {
                stats.resolution_dist.other += 1;
            }
        }

        stats
    }

    pub fn print_stats(&self) {
        println!("\n=== Statistics ===\n");

        println!("Camera Model Distribution:");
        println!("{:-<40}", "");
        let mut cameras: Vec<_> = self.by_camera.iter().collect();
        cameras.sort_by(|a, b| b.1.cmp(a.1));
        for (camera, count) in &cameras {
            println!("  {:<30} {:>5}", camera, count);
        }

        println!("\nYear-Month Distribution:");
        println!("{:-<40}", "");
        let mut months: Vec<_> = self.by_year_month.iter().collect();
        months.sort_by(|a, b| a.0.cmp(b.0));
        let max_count = months.iter().map(|(_, c)| **c).max().unwrap_or(1);
        for (ym, count) in &months {
            let bar_len = (**count as f64 / max_count as f64 * 30.0).ceil() as usize;
            let bar: String = "█".repeat(bar_len);
            println!("  {} {:>4} {}", ym, count, bar);
        }

        println!("\nResolution Distribution:");
        println!("{:-<40}", "");
        let total = self.total.max(1) as f64;
        let rd = &self.resolution_dist;
        println!(
            "  4K/UHD (≥3840):  {:>5} ({:.1}%)",
            rd.ultra_hd,
            rd.ultra_hd as f64 / total * 100.0
        );
        println!(
            "  1080P  (≥1920):  {:>5} ({:.1}%)",
            rd.full_hd,
            rd.full_hd as f64 / total * 100.0
        );
        println!(
            "  720P   (≥1280):  {:>5} ({:.1}%)",
            rd.hd,
            rd.hd as f64 / total * 100.0
        );
        println!(
            "  SD     (≥720):   {:>5} ({:.1}%)",
            rd.sd,
            rd.sd as f64 / total * 100.0
        );
        println!(
            "  Other  (<720):   {:>5} ({:.1}%)",
            rd.other,
            rd.other as f64 / total * 100.0
        );

        println!(
            "\nGPS Coverage: {}/{} ({:.1}%)",
            self.total_with_gps,
            self.total,
            self.total_with_gps as f64 / total * 100.0
        );
    }
}

#[derive(serde::Serialize)]
pub struct JsonReport {
    pub summary: JsonSummary,
    pub by_camera: HashMap<String, usize>,
    pub by_year_month: HashMap<String, usize>,
    pub by_format: HashMap<String, usize>,
    pub resolution_distribution: HashMap<String, usize>,
    pub gps_coverage: GpsCoverage,
    pub archived: Vec<JsonImageEntry>,
    pub skipped_duplicates: Vec<JsonImageEntry>,
    pub suspected_duplicates: Vec<JsonImageEntry>,
    pub failed: Vec<JsonImageEntry>,
}

#[derive(serde::Serialize)]
pub struct JsonSummary {
    pub total_images: usize,
    pub archived: usize,
    pub skipped_duplicates: usize,
    pub suspected_duplicates: usize,
    pub failed: usize,
}

#[derive(serde::Serialize)]
pub struct GpsCoverage {
    pub with_gps: usize,
    pub without_gps: usize,
    pub percentage: f64,
}

#[derive(serde::Serialize, Clone)]
pub struct JsonImageEntry {
    pub source_path: String,
    pub archive_path: Option<String>,
    pub camera: String,
    pub date: String,
    pub sha256: String,
    pub reason: Option<String>,
}

impl JsonReport {
    pub fn new() -> Self {
        Self {
            summary: JsonSummary {
                total_images: 0,
                archived: 0,
                skipped_duplicates: 0,
                suspected_duplicates: 0,
                failed: 0,
            },
            by_camera: HashMap::new(),
            by_year_month: HashMap::new(),
            by_format: HashMap::new(),
            resolution_distribution: HashMap::new(),
            gps_coverage: GpsCoverage {
                with_gps: 0,
                without_gps: 0,
                percentage: 0.0,
            },
            archived: Vec::new(),
            skipped_duplicates: Vec::new(),
            suspected_duplicates: Vec::new(),
            failed: Vec::new(),
        }
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize report: {}", e))?;
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write report: {}", e))
    }
}

pub struct RunStats {
    pub archived: usize,
    pub skipped_duplicates: usize,
    pub suspected_duplicates: usize,
    pub failed: usize,
    pub failures: Vec<(String, String)>,
}

impl RunStats {
    pub fn new() -> Self {
        Self {
            archived: 0,
            skipped_duplicates: 0,
            suspected_duplicates: 0,
            failed: 0,
            failures: Vec::new(),
        }
    }

    pub fn print_report(&self) {
        println!("\n=== Archive Report ===");
        println!("  Archived:            {}", self.archived);
        println!("  Skipped (duplicates): {}", self.skipped_duplicates);
        println!("  Suspected duplicates: {}", self.suspected_duplicates);
        println!("  Failed:              {}", self.failed);

        if !self.failures.is_empty() {
            println!("\n  Failed files:");
            for (path, reason) in &self.failures {
                println!("    {} - {}", path, reason);
            }
        }
    }
}
