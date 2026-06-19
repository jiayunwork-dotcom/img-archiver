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

    pub fn save_html(
        &self,
        path: &std::path::Path,
        suspected_duplicates_with_distance: &[(String, String, u32)],
    ) -> Result<(), String> {
        let html = self.render_html(suspected_duplicates_with_distance);
        std::fs::write(path, html)
            .map_err(|e| format!("Failed to write HTML report: {}", e))
    }

    fn render_html(&self, suspected_duplicates_with_distance: &[(String, String, u32)]) -> String {
        let total = self.summary.total_images;
        let archived = self.summary.archived;
        let skipped = self.summary.skipped_duplicates;
        let suspected = self.summary.suspected_duplicates;
        let failed = self.summary.failed;

        let mut sorted_months: Vec<(&String, &usize)> = self.by_year_month.iter().collect();
        sorted_months.sort_by(|a, b| a.0.cmp(b.0));
        let line_chart = render_line_chart(&sorted_months);

        let mut sorted_cameras: Vec<(&String, &usize)> = self.by_camera.iter().collect();
        sorted_cameras.sort_by(|a, b| b.1.cmp(a.1));
        let pie_chart = render_pie_chart(&sorted_cameras);

        let dup_list = render_suspected_duplicates(suspected_duplicates_with_distance);

        format!(
            "<!DOCTYPE html>
<html lang=\"zh-CN\">
<head>
<meta charset=\"UTF-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">
<title>Image Archive Report</title>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, Helvetica, Arial, sans-serif; background: #f5f7fa; color: #333; padding: 24px; }}
h1 {{ font-size: 28px; margin-bottom: 24px; color: #1a202c; }}
h2 {{ font-size: 20px; margin: 32px 0 16px; color: #2d3748; border-bottom: 2px solid #e2e8f0; padding-bottom: 8px; }}
.stats-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 16px; margin-bottom: 24px; }}
.stat-card {{ background: white; border-radius: 8px; padding: 20px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); text-align: center; }}
.stat-card .label {{ font-size: 13px; color: #718096; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 8px; }}
.stat-card .value {{ font-size: 32px; font-weight: 700; }}
.stat-card.total .value {{ color: #4299e1; }}
.stat-card.archived .value {{ color: #48bb78; }}
.stat-card.skipped .value {{ color: #ed8936; }}
.stat-card.suspected .value {{ color: #ecc94b; }}
.stat-card.failed .value {{ color: #f56565; }}
.chart-container {{ background: white; border-radius: 8px; padding: 20px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); margin-bottom: 24px; overflow-x: auto; }}
.chart-container svg {{ display: block; margin: 0 auto; }}
.dup-list {{ background: white; border-radius: 8px; padding: 16px; box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
.dup-item {{ display: flex; justify-content: space-between; align-items: center; padding: 12px 16px; border-bottom: 1px solid #edf2f7; }}
.dup-item:last-child {{ border-bottom: none; }}
.dup-item .filename {{ font-family: monospace; font-size: 14px; color: #2d3748; }}
.dup-item .distance {{ padding: 4px 12px; border-radius: 12px; font-size: 12px; font-weight: 600; }}
.distance-low {{ background: #fed7d7; color: #c53030; }}
.distance-medium {{ background: #feebc8; color: #c05621; }}
.distance-high {{ background: #c6f6d5; color: #276749; }}
.empty {{ text-align: center; color: #a0aec0; padding: 24px; font-style: italic; }}
</style>
</head>
<body>
<h1>Image Archive Report</h1>

<h2>Overview</h2>
<div class=\"stats-grid\">
  <div class=\"stat-card total\">
    <div class=\"label\">Total</div>
    <div class=\"value\">{total}</div>
  </div>
  <div class=\"stat-card archived\">
    <div class=\"label\">Archived</div>
    <div class=\"value\">{archived}</div>
  </div>
  <div class=\"stat-card skipped\">
    <div class=\"label\">Skipped</div>
    <div class=\"value\">{skipped}</div>
  </div>
  <div class=\"stat-card suspected\">
    <div class=\"label\">Suspected Duplicates</div>
    <div class=\"value\">{suspected}</div>
  </div>
  <div class=\"stat-card failed\">
    <div class=\"label\">Failed</div>
    <div class=\"value\">{failed}</div>
  </div>
</div>

<h2>Archive Count by Month</h2>
<div class=\"chart-container\">
  {line_chart}
</div>

<h2>Camera Model Distribution</h2>
<div class=\"chart-container\">
  {pie_chart}
</div>

<h2>Suspected Duplicate Images</h2>
<div class=\"dup-list\">
  {dup_list}
</div>

</body>
</html>
"
        )
    }
}

fn render_line_chart(data: &[(&String, &usize)]) -> String {
    if data.is_empty() {
        return "<div class=\"empty\">No data available</div>".to_string();
    }

    const COLOR_GRID: &str = "#e2e8f0";
    const COLOR_AXIS: &str = "#cbd5e0";
    const COLOR_TEXT: &str = "#718096";
    const COLOR_LINE: &str = "#4299e1";
    const COLOR_AREA: &str = "rgba(66, 153, 225, 0.1)";
    const COLOR_BG: &str = "#f7fafc";

    let width = 800;
    let height = 350;
    let padding_left = 60;
    let padding_right = 20;
    let padding_top = 20;
    let padding_bottom = 60;
    let chart_width = width - padding_left - padding_right;
    let chart_height = height - padding_top - padding_bottom;

    let max_y = data.iter().map(|(_, &v)| v).max().unwrap_or(1).max(1);
    let points: Vec<(f64, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, (_, &v))| {
            let x = if data.len() == 1 {
                padding_left as f64 + chart_width as f64 / 2.0
            } else {
                padding_left as f64 + (i as f64 / (data.len() - 1) as f64) * chart_width as f64
            };
            let y = padding_top as f64 + chart_height as f64
                - (v as f64 / max_y as f64) * chart_height as f64;
            (x, y)
        })
        .collect();

    let path_d = points
        .iter()
        .enumerate()
        .map(|(i, (x, y))| {
            if i == 0 {
                format!("M {},{}", x, y)
            } else {
                format!(" L {},{}", x, y)
            }
        })
        .collect::<Vec<_>>()
        .join("");

    let area_d = format!(
        "{} L {},{} L {},{} Z",
        path_d,
        points.last().unwrap().0,
        padding_top as f64 + chart_height as f64,
        points.first().unwrap().0,
        padding_top as f64 + chart_height as f64
    );

    let y_ticks = 5;
    let y_axis: Vec<String> = (0..=y_ticks)
        .map(|i| {
            let value = (max_y as f64 * i as f64 / y_ticks as f64).round() as usize;
            let y = padding_top as f64 + chart_height as f64
                - (i as f64 / y_ticks as f64) * chart_height as f64;
            format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\"/><text x=\"{}\" y=\"{}\" font-size=\"11\" fill=\"{}\" text-anchor=\"end\" dominant-baseline=\"middle\">{}</text>",
                padding_left, y, width - padding_right, y, COLOR_GRID, padding_left - 8, y, COLOR_TEXT, value
            )
        })
        .collect();

    let x_axis: Vec<String> = data
        .iter()
        .enumerate()
        .map(|(i, (label, _))| {
            let x = if data.len() == 1 {
                padding_left as f64 + chart_width as f64 / 2.0
            } else {
                padding_left as f64 + (i as f64 / (data.len() - 1) as f64) * chart_width as f64
            };
            let y = padding_top as f64 + chart_height as f64;
            let show_label = data.len() <= 12 || i % (data.len() / 12 + 1) == 0;
            if show_label {
                format!(
                    "<text x=\"{}\" y=\"{}\" font-size=\"11\" fill=\"{}\" text-anchor=\"end\" transform=\"rotate(-45, {}, {})\">{}</text>",
                    x + 4.0,
                    y + 16.0,
                    COLOR_TEXT,
                    x + 4.0,
                    y + 16.0,
                    label
                )
            } else {
                String::new()
            }
        })
        .collect();

    let circles: Vec<String> = points
        .iter()
        .map(|(x, y)| {
            format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"{}\" stroke=\"white\" stroke-width=\"2\"/>",
                x, y, COLOR_LINE
            )
        })
        .collect();

    format!(
        "<svg width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" xmlns=\"http://www.w3.org/2000/svg\">
  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>
  {}
  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>
  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>
  {}
  <path d=\"{}\" fill=\"{}\" stroke=\"none\"/>
  <path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>
  {}
</svg>",
        width, height, width, height,
        padding_left, padding_top, chart_width, chart_height, COLOR_BG,
        y_axis.join(""),
        padding_left, padding_top, padding_left, padding_top + chart_height, COLOR_AXIS,
        padding_left, padding_top + chart_height, width - padding_right, padding_top + chart_height, COLOR_AXIS,
        x_axis.join(""),
        area_d,
        COLOR_AREA,
        path_d,
        COLOR_LINE,
        circles.join("")
    )
}

fn render_pie_chart(data: &[(&String, &usize)]) -> String {
    if data.is_empty() {
        return "<div class=\"empty\">No data available</div>".to_string();
    }

    const COLOR_TEXT: &str = "#2d3748";

    let total: usize = data.iter().map(|(_, &v)| v).sum();
    if total == 0 {
        return "<div class=\"empty\">No data available</div>".to_string();
    }

    let cx = 200.0;
    let cy = 200.0;
    let r = 140.0;
    let legend_x = 400;

    let colors = [
        "#4299e1", "#48bb78", "#ed8936", "#9f7aea", "#f56565",
        "#38b2ac", "#667eea", "#ecc94b", "#68d391", "#fc8181",
        "#63b3ed", "#b794f4", "#f6ad55", "#4fd1c5", "#fc8181",
    ];

    let max_items = 10;
    let other_str = "Others".to_string();
    let display_data: Vec<(&String, usize, f64)> = if data.len() > max_items {
        let mut top: Vec<(&String, usize, f64)> = data
            .iter()
            .take(max_items - 1)
            .map(|(k, &v)| (*k, v, v as f64 / total as f64 * 360.0))
            .collect();
        let other_count: usize = data.iter().skip(max_items - 1).map(|(_, &v)| v).sum();
        top.push((&other_str, other_count, other_count as f64 / total as f64 * 360.0));
        top
    } else {
        data.iter()
            .map(|(k, &v)| (*k, v, v as f64 / total as f64 * 360.0))
            .collect()
    };

    let mut start_angle = -90.0;
    let mut paths: Vec<String> = Vec::new();
    let mut legends: Vec<String> = Vec::new();

    for (i, (label, count, angle)) in display_data.iter().enumerate() {
        let color = colors[i % colors.len()];
        let end_angle = start_angle + angle;

        if *angle >= 359.99 {
            paths.push(format!(
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>",
                cx, cy, r, color
            ));
        } else {
            let start_rad = start_angle.to_radians();
            let end_rad = end_angle.to_radians();
            let x1 = cx + r * start_rad.cos();
            let y1 = cy + r * start_rad.sin();
            let x2 = cx + r * end_rad.cos();
            let y2 = cy + r * end_rad.sin();
            let large_arc = if *angle > 180.0 { 1 } else { 0 };

            paths.push(format!(
                "<path d=\"M {},{} A {},{} 0 {},1 {},{} L {},{} Z\" fill=\"{}\" stroke=\"white\" stroke-width=\"2\"/>",
                x1, y1, r, r, large_arc, x2, y2, cx, cy, color
            ));
        }

        let percentage = *count as f64 / total as f64 * 100.0;
        let legend_y = 40 + i * 28;
        legends.push(format!(
            "<rect x=\"{}\" y=\"{}\" width=\"16\" height=\"16\" fill=\"{}\" rx=\"3\"/><text x=\"{}\" y=\"{}\" font-size=\"13\" fill=\"{}\" dominant-baseline=\"hanging\">{} ({}, {:.1}%)</text>",
            legend_x, legend_y, color, legend_x + 24, legend_y,
            COLOR_TEXT,
            if label.len() > 25 {
                let truncated: String = label.chars().take(22).collect();
                format!("{}...", truncated)
            } else {
                (*label).clone()
            },
            count,
            percentage
        ));

        start_angle = end_angle;
    }

    let svg_width = 700;
    let svg_height = 400 + (display_data.len().saturating_sub(8)) * 28;

    format!(
        "<svg width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" xmlns=\"http://www.w3.org/2000/svg\">
  <text x=\"{}\" y=\"{}\" font-size=\"14\" font-weight=\"600\" fill=\"{}\" text-anchor=\"middle\">Total: {}</text>
  {}
  {}
</svg>",
        svg_width, svg_height, svg_width, svg_height,
        cx, cy + r + 30.0, COLOR_TEXT, total,
        paths.join(""),
        legends.join("")
    )
}

fn render_suspected_duplicates(dups: &[(String, String, u32)]) -> String {
    if dups.is_empty() {
        return "<div class=\"empty\">No suspected duplicates found</div>".to_string();
    }

    const COLOR_TEXT: &str = "#718096";

    let items: Vec<String> = dups
        .iter()
        .map(|(filename, original_path, distance)| {
            let class = if *distance <= 2 {
                "distance-low"
            } else if *distance <= 4 {
                "distance-medium"
            } else {
                "distance-high"
            };
            format!(
                "<div class=\"dup-item\">
  <div>
    <div class=\"filename\">{}</div>
    <div style=\"font-size:12px;color:{};margin-top:4px;\">Similar to: {}</div>
  </div>
  <span class=\"distance {}\">Hamming: {}</span>
</div>",
                filename, COLOR_TEXT, original_path, class, distance
            )
        })
        .collect();

    items.join("")
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
