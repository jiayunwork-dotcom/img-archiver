use crate::index::{ArchiveIndex, IndexEntry};
use crate::types::GpsLocation;
use chrono::{NaiveDate, NaiveDateTime};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumMeta {
    pub last_run: String,
    pub version: String,
}

impl AlbumMeta {
    pub fn load(album_dir: &Path) -> Option<Self> {
        let meta_path = album_dir.join(".album-meta.json");
        if !meta_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&meta_path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, album_dir: &Path) -> Result<(), String> {
        let meta_path = album_dir.join(".album-meta.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize album meta: {}", e))?;
        std::fs::write(&meta_path, content)
            .map_err(|e| format!("Failed to write album meta: {}", e))
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PhotoEntry {
    pub index: usize,
    pub entry: IndexEntry,
    pub archived_dt: Option<NaiveDateTime>,
    pub photo_dt: Option<NaiveDateTime>,
    pub archive_path_abs: PathBuf,
}

impl PhotoEntry {
    pub fn sort_dt(&self) -> NaiveDateTime {
        self.photo_dt
            .or(self.archived_dt)
            .unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap())
    }

    pub fn date(&self) -> NaiveDate {
        self.sort_dt().date()
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Album {
    pub name: String,
    pub display_name: String,
    pub photos: Vec<PhotoEntry>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub location: String,
}

fn parse_datetime(s: &str) -> Option<NaiveDateTime> {
    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
    ];
    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt);
        }
    }
    None
}

pub fn parse_time_gap(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if s.len() < 2 {
        return Err(format!("Invalid time gap format: {}. Expected e.g. '4h', '2d'", s));
    }
    let unit = s.chars().last().unwrap().to_ascii_lowercase();
    let num_str: String = s.chars().take(s.len() - 1).collect();
    let num: i64 = num_str
        .parse()
        .map_err(|_| format!("Invalid time gap number: {}", num_str))?;

    let seconds = match unit {
        's' => num,
        'm' => num * 60,
        'h' => num * 3600,
        'd' => num * 86400,
        _ => return Err(format!("Invalid time gap unit: '{}'. Use s/m/h/d", unit)),
    };
    Ok(seconds)
}

pub fn haversine_km(a: &GpsLocation, b: &GpsLocation) -> f64 {
    const R: f64 = 6371.0;
    let lat1 = a.latitude.to_radians();
    let lat2 = b.latitude.to_radians();
    let dlat = (b.latitude - a.latitude).to_radians();
    let dlon = (b.longitude - a.longitude).to_radians();

    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * h.sqrt().asin();
    R * c
}

fn enrich_entries(index: &ArchiveIndex, input_dir: &Path) -> Vec<PhotoEntry> {
    index
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let archive_path_abs = if Path::new(&e.archive_path).is_absolute() {
                PathBuf::from(&e.archive_path)
            } else {
                input_dir.join(&e.archive_path)
            };
            PhotoEntry {
                index: i,
                entry: e.clone(),
                archived_dt: parse_datetime(&e.archived_at),
                photo_dt: e.date_time.as_deref().and_then(parse_datetime),
                archive_path_abs,
            }
        })
        .collect()
}

fn cluster_by_time(photos: &[PhotoEntry], gap_sec: i64) -> Vec<Vec<PhotoEntry>> {
    let mut sorted: Vec<PhotoEntry> = photos.to_vec();
    sorted.sort_by_key(|p| p.sort_dt());

    if sorted.is_empty() {
        return Vec::new();
    }

    let mut groups: Vec<Vec<PhotoEntry>> = Vec::new();
    let mut current: Vec<PhotoEntry> = vec![sorted[0].clone()];
    let mut prev_dt = sorted[0].sort_dt();
    let mut prev_date = sorted[0].date();

    for p in sorted.iter().skip(1) {
        let cur_dt = p.sort_dt();
        let cur_date = p.date();

        let diff = (cur_dt - prev_dt).num_seconds();
        let cross_day = cur_date != prev_date;

        if cross_day || diff > gap_sec {
            groups.push(current);
            current = vec![p.clone()];
        } else {
            current.push(p.clone());
        }
        prev_dt = cur_dt;
        prev_date = cur_date;
    }
    groups.push(current);
    groups
}

fn cluster_by_geo(time_group: &[PhotoEntry], radius_km: f64) -> Vec<Vec<PhotoEntry>> {
    if time_group.is_empty() {
        return Vec::new();
    }

    let (with_gps, without_gps): (Vec<PhotoEntry>, Vec<PhotoEntry>) =
        time_group.iter().cloned().partition(|p| p.entry.gps.is_some());

    let mut result: Vec<Vec<PhotoEntry>> = Vec::new();

    if !with_gps.is_empty() {
        let mut current: Vec<PhotoEntry> = vec![with_gps[0].clone()];
        let mut prev_gps = with_gps[0].entry.gps.as_ref().unwrap();

        for p in with_gps.iter().skip(1) {
            let cur_gps = p.entry.gps.as_ref().unwrap();
            let dist = haversine_km(prev_gps, cur_gps);
            if dist > radius_km {
                result.push(current);
                current = vec![p.clone()];
            } else {
                current.push(p.clone());
            }
            prev_gps = cur_gps;
        }
        result.push(current);
    }

    if !without_gps.is_empty() {
        result.push(without_gps);
    }

    result
}

fn determine_location(photos: &[PhotoEntry]) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut has_geo = false;
    for p in photos {
        if let Some(ref geo) = p.entry.geo {
            has_geo = true;
            *counts.entry(geo.province.clone()).or_insert(0) += 1;
        }
    }
    if !has_geo {
        return "misc".to_string();
    }
    counts
        .into_iter()
        .max_by_key(|(_, c)| *c)
        .map(|(k, _)| k)
        .unwrap_or_else(|| "misc".to_string())
}

fn sanitize_name(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' | ' ' => '_',
            _ => c,
        })
        .collect()
}

fn name_albums(clusters: &[Vec<PhotoEntry>]) -> Vec<Album> {
    let mut name_counters: HashMap<String, usize> = HashMap::new();
    let mut albums: Vec<Album> = Vec::new();

    for cluster in clusters {
        if cluster.is_empty() {
            continue;
        }

        let start_date = cluster.iter().map(|p| p.date()).min().unwrap();
        let end_date = cluster.iter().map(|p| p.date()).max().unwrap();
        let location = determine_location(cluster);
        let date_str = format!("{}", start_date.format("%Y-%m-%d"));
        let base_name = format!("{}_{}", date_str, sanitize_name(&location));

        let count = name_counters.entry(base_name.clone()).or_insert(0);
        *count += 1;

        let (display_name, folder_name) = if *count > 1 {
            (
                format!("{}_{}", base_name, count),
                format!("{}_{}", base_name, count),
            )
        } else {
            (base_name.clone(), base_name.clone())
        };

        albums.push(Album {
            name: folder_name,
            display_name,
            photos: cluster.clone(),
            start_date,
            end_date,
            location,
        });
    }

    albums
}

fn make_gray_placeholder(path: &Path) -> Result<(), String> {
    let size = 300;
    let mut img_buf = image::RgbImage::new(size, size);
    for pixel in img_buf.pixels_mut() {
        *pixel = image::Rgb([128, 128, 128]);
    }
    img_buf
        .save_with_format(path, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to save placeholder: {}", e))
}

fn gen_thumbnail(src: &Path, dst: &Path) -> Result<(), String> {
    let img = image::open(src).map_err(|e| format!("Failed to open image: {}", e))?;

    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("Image has zero dimensions".to_string());
    }

    let target: u32 = 300;
    let scale = if w <= h {
        target as f32 / w as f32
    } else {
        target as f32 / h as f32
    };

    let new_w = (w as f32 * scale).round() as u32;
    let new_h = (h as f32 * scale).round() as u32;

    let resized = image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Lanczos3);

    let cropped = if new_w == target && new_h == target {
        resized
    } else {
        let x = if new_w > target { (new_w - target) / 2 } else { 0 };
        let y = if new_h > target { (new_h - target) / 2 } else { 0 };
        image::imageops::crop_imm(&resized, x, y, target, target).to_image()
    };

    let rgb = image::DynamicImage::ImageRgba8(cropped).to_rgb8();

    let file = std::fs::File::create(dst)
        .map_err(|e| format!("Failed to create thumbnail file: {}", e))?;
    let mut buf_writer = std::io::BufWriter::new(file);
    let jpeg_encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf_writer, 80);
    rgb.write_with_encoder(jpeg_encoder)
        .map_err(|e| format!("Failed to encode JPEG: {}", e))?;
    Ok(())
}

fn thumbnail_filename(photo: &PhotoEntry) -> String {
    let stem = photo
        .archive_path_abs
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let hash_len = photo.entry.sha256.len().min(10);
    let hash_prefix = if hash_len > 0 {
        &photo.entry.sha256[..hash_len]
    } else {
        "unknown"
    };
    format!("{}_{}.jpg", hash_prefix, stem)
}

fn render_css() -> String {
    r#"* { margin: 0; padding: 0; box-sizing: border-box; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; background: #f5f7fa; color: #333; padding: 24px; }
h1 { font-size: 28px; margin-bottom: 8px; color: #1a202c; }
h2 { font-size: 18px; margin: 0 0 8px; color: #2d3748; }
.header { text-align: center; margin-bottom: 32px; }
.stats { font-size: 15px; color: #718096; }
.albums-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 20px; }
.album-card { background: white; border-radius: 10px; overflow: hidden; box-shadow: 0 2px 8px rgba(0,0,0,0.08); cursor: pointer; transition: transform 0.15s, box-shadow 0.15s; }
.album-card:hover { transform: translateY(-3px); box-shadow: 0 6px 16px rgba(0,0,0,0.12); }
.album-cover { width: 100%; height: 220px; background: #e2e8f0; overflow: hidden; display: flex; align-items: center; justify-content: center; }
.album-cover img { width: 100%; height: 100%; object-fit: cover; }
.album-info { padding: 16px; }
.album-name { font-weight: 700; font-size: 16px; color: #1a202c; margin-bottom: 6px; }
.album-meta { font-size: 13px; color: #718096; line-height: 1.6; }
.photos-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 12px; }
.photo-item { background: white; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 4px rgba(0,0,0,0.06); }
.photo-thumb { width: 100%; aspect-ratio: 1/1; background: #e2e8f0; overflow: hidden; display: flex; align-items: center; justify-content: center; }
.photo-thumb img { width: 100%; height: 100%; object-fit: cover; }
.photo-info { padding: 8px 10px; font-size: 12px; line-height: 1.5; }
.photo-filename { color: #2d3748; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.photo-date { color: #a0aec0; }
.back-link { display: inline-block; margin-bottom: 20px; padding: 6px 14px; background: #4299e1; color: white; border-radius: 6px; text-decoration: none; font-size: 14px; }
.back-link:hover { background: #3182ce; }
.empty { text-align: center; color: #a0aec0; padding: 48px; font-style: italic; }
a { text-decoration: none; color: inherit; }"#
        .to_string()
}

fn render_index_page(albums: &[Album], total_photos: usize, thumbs_dir: &str) -> String {
    let album_cards: Vec<String> = albums
        .iter()
        .enumerate()
        .map(|(i, album)| {
            let cover_thumb = album
                .photos
                .first()
                .map(|p| format!("{}/{}", thumbs_dir, thumbnail_filename(p)))
                .unwrap_or_else(|| "".to_string());
            let date_range = if album.start_date == album.end_date {
                format!("{}", album.start_date.format("%Y-%m-%d"))
            } else {
                format!(
                    "{} ~ {}",
                    album.start_date.format("%Y-%m-%d"),
                    album.end_date.format("%Y-%m-%d")
                )
            };
            format!(
                r#"<a href="album_{}.html">
  <div class="album-card">
    <div class="album-cover">
      <img src="{}" alt="{}" onerror="this.style.display='none'"/>
    </div>
    <div class="album-info">
      <div class="album-name">{}</div>
      <div class="album-meta">{} 张照片</div>
      <div class="album-meta">{}</div>
    </div>
  </div>
</a>"#,
                i + 1,
                cover_thumb,
                album.display_name,
                album.display_name,
                album.photos.len(),
                date_range
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>相册索引</title>
<style>
{}
</style>
</head>
<body>
<div class="header">
  <h1>📸 相册画廊</h1>
  <p class="stats">共 {} 个相册，{} 张照片</p>
</div>
<div class="albums-grid">
{}
</div>
</body>
</html>"#,
        render_css(),
        albums.len(),
        total_photos,
        if album_cards.is_empty() {
            "<div class=\"empty\">暂无相册</div>".to_string()
        } else {
            album_cards.join("\n")
        }
    )
}

fn render_album_detail(album: &Album, _idx: usize, thumbs_dir: &str) -> String {
    let photo_items: Vec<String> = album
        .photos
        .iter()
        .map(|p| {
            let thumb = format!("{}/{}", thumbs_dir, thumbnail_filename(p));
            let filename = p
                .archive_path_abs
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let date = format!("{}", p.date().format("%Y-%m-%d"));
            format!(
                r#"<div class="photo-item">
  <div class="photo-thumb">
    <img src="{}" alt="{}" onerror="this.style.display='none'"/>
  </div>
  <div class="photo-info">
    <div class="photo-filename" title="{}">{}</div>
    <div class="photo-date">{}</div>
  </div>
</div>"#,
                thumb, filename, filename, filename, date
            )
        })
        .collect();

    let date_range = if album.start_date == album.end_date {
        format!("{}", album.start_date.format("%Y-%m-%d"))
    } else {
        format!(
            "{} ~ {}",
            album.start_date.format("%Y-%m-%d"),
            album.end_date.format("%Y-%m-%d")
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{}</title>
<style>
{}
</style>
</head>
<body>
<a href="index.html" class="back-link">← 返回相册列表</a>
<div class="header">
  <h1>{}</h1>
  <p class="stats">{} 张照片 · {}</p>
</div>
<div class="photos-grid">
{}
</div>
</body>
</html>"#,
        album.display_name,
        render_css(),
        album.display_name,
        album.photos.len(),
        date_range,
        if photo_items.is_empty() {
            "<div class=\"empty\">暂无照片</div>".to_string()
        } else {
            photo_items.join("\n")
        }
    )
}

pub fn run_album(
    input: &Path,
    output_album: &Path,
    time_gap_str: &str,
    geo_radius: f64,
    rebuild: bool,
) -> Result<(), String> {
    let gap_sec = parse_time_gap(time_gap_str)?;

    let index = ArchiveIndex::load(input).map_err(|e| {
        format!(
            "归档索引解析失败: {}。文件可能已损坏，请检查 .archive-index.json",
            e
        )
    })?;

    if index.entries.is_empty() {
        return Err("归档索引为空，没有可处理的图片".to_string());
    }

    if !output_album.exists() {
        std::fs::create_dir_all(output_album)
            .map_err(|e| format!("Failed to create album output directory: {}", e))?;
    }

    let thumbs_dir = output_album.join("thumbs");
    if !thumbs_dir.exists() {
        std::fs::create_dir_all(&thumbs_dir)
            .map_err(|e| format!("Failed to create thumbs directory: {}", e))?;
    }

    let last_run = if rebuild {
        None
    } else {
        AlbumMeta::load(output_album)
            .and_then(|m| parse_datetime(&m.last_run))
    };

    let all_photos = enrich_entries(&index, input);

    let new_photo_count: usize = if let Some(last_dt) = last_run {
        eprintln!(
            "增量模式：上次运行时间 {}，检测新归档的图片",
            last_dt.format("%Y-%m-%d %H:%M:%S")
        );
        all_photos
            .iter()
            .filter(|p| {
                p.archived_dt
                    .map(|d| d > last_dt)
                    .unwrap_or(true)
            })
            .count()
    } else {
        all_photos.len()
    };

    if new_photo_count == 0 && !rebuild {
        eprintln!("没有新图片需要处理，任务结束。");
        let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let meta = AlbumMeta {
            last_run: now_str,
            version: "1.0".to_string(),
        };
        meta.save(output_album)?;
        return Ok(());
    }

    eprintln!(
        "共载入 {} 条索引记录，本次新增 {} 条（全量重建相册）",
        all_photos.len(),
        new_photo_count
    );

    let time_groups = cluster_by_time(&all_photos, gap_sec);
    eprintln!("时间聚类：分为 {} 个时间组", time_groups.len());

    let mut geo_clusters: Vec<Vec<PhotoEntry>> = Vec::new();
    for tg in &time_groups {
        let sub = cluster_by_geo(tg, geo_radius);
        geo_clusters.extend(sub);
    }
    eprintln!("地理聚类：最终分为 {} 个相册", geo_clusters.len());

    let mut albums = name_albums(&geo_clusters);
    albums.sort_by(|a, b| b.start_date.cmp(&a.start_date).then_with(|| b.name.cmp(&a.name)));

    let total_photos: usize = albums.iter().map(|a| a.photos.len()).sum();
    let thumbs_to_generate: usize = albums
        .iter()
        .flat_map(|a| a.photos.iter())
        .filter(|p| {
            let thumb_name = thumbnail_filename(p);
            let thumb_path = thumbs_dir.join(&thumb_name);
            rebuild || !thumb_path.exists()
        })
        .count();
    eprintln!(
        "开始生成缩略图（共 {} 张，需生成 {} 张，跳过已存在 {} 张）",
        total_photos,
        thumbs_to_generate,
        total_photos - thumbs_to_generate
    );

    let pb = ProgressBar::new(thumbs_to_generate.max(1) as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );

    let mut failed_thumbs: Vec<(String, String)> = Vec::new();

    for album in &albums {
        for photo in &album.photos {
            let thumb_name = thumbnail_filename(photo);
            let thumb_path = thumbs_dir.join(&thumb_name);

            if thumb_path.exists() && !rebuild {
                continue;
            }

            let src_path = &photo.archive_path_abs;
            if !src_path.exists() {
                failed_thumbs.push((
                    photo.entry.archive_path.clone(),
                    format!("源文件不存在: {}", src_path.display()),
                ));
                if let Err(e) = make_gray_placeholder(&thumb_path) {
                    eprintln!("  警告：生成占位图失败 {}: {}", thumb_name, e);
                }
                pb.inc(1);
                continue;
            }

            match gen_thumbnail(src_path, &thumb_path) {
                Ok(_) => {}
                Err(e) => {
                    failed_thumbs.push((
                        photo.entry.archive_path.clone(),
                        format!("缩略图失败: {}", e),
                    ));
                    if let Err(pe) = make_gray_placeholder(&thumb_path) {
                        eprintln!("  警告：生成占位图失败 {}: {}", thumb_name, pe);
                    }
                }
            }
            pb.inc(1);
        }
    }
    pb.finish_with_message("缩略图处理完成");

    eprintln!("清理旧 HTML 页面...");
    if let Ok(entries) = std::fs::read_dir(output_album) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let is_old_index = name == "index.html";
                let is_old_album = name.starts_with("album_") && name.ends_with(".html");
                if is_old_index || is_old_album {
                    if let Err(e) = std::fs::remove_file(&path) {
                        eprintln!("  警告：删除旧文件失败 {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    eprintln!("生成 HTML 页面...");

    let thumbs_rel = "thumbs";

    let index_html = render_index_page(&albums, total_photos, thumbs_rel);
    let index_path = output_album.join("index.html");
    std::fs::write(&index_path, index_html)
        .map_err(|e| format!("Failed to write index.html: {}", e))?;

    for (i, album) in albums.iter().enumerate() {
        let detail_html = render_album_detail(album, i + 1, thumbs_rel);
        let detail_path = output_album.join(format!("album_{}.html", i + 1));
        std::fs::write(&detail_path, detail_html)
            .map_err(|e| format!("Failed to write album_{}.html: {}", i + 1, e))?;
    }

    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let meta = AlbumMeta {
        last_run: now_str,
        version: "1.0".to_string(),
    };
    meta.save(output_album)?;

    println!("\n=== 相册生成报告 ===");
    println!("  相册数量:          {}", albums.len());
    println!("  照片总数:          {}", total_photos);
    println!("  输出目录:          {}", output_album.display());
    println!("  索引页面:          {}", index_path.display());
    println!("  缩略图目录:        {}", thumbs_dir.display());

    if !failed_thumbs.is_empty() {
        println!("\n  缩略图生成失败 ({} 张):", failed_thumbs.len());
        for (path, reason) in &failed_thumbs {
            println!("    {} - {}", path, reason);
        }
    } else {
        println!("\n  所有缩略图生成成功。");
    }

    Ok(())
}
