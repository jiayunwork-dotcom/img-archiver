use crate::index::{ArchiveIndex, IndexEntry};
use crate::types::GpsLocation;
use chrono::{NaiveDate, NaiveDateTime};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRecord {
    pub timestamp: String,
    pub source_albums: Vec<String>,
    pub target_album: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitRecord {
    pub timestamp: String,
    pub source_album: String,
    pub split_point: usize,
    pub first_half: String,
    pub second_half: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumRecord {
    pub name: String,
    pub display_name: String,
    pub photo_sha256: Vec<String>,
    pub start_date: String,
    pub end_date: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumMeta {
    pub last_run: String,
    pub version: String,
    #[serde(default)]
    pub merges: Vec<MergeRecord>,
    #[serde(default)]
    pub splits: Vec<SplitRecord>,
    #[serde(default)]
    pub albums: Vec<AlbumRecord>,
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

    pub fn get_album_names(&self) -> Vec<String> {
        self.albums.iter().map(|a| a.name.clone()).collect()
    }

    pub fn find_album(&self, name: &str) -> Option<&AlbumRecord> {
        self.albums.iter().find(|a| a.name == name)
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

fn build_photo_map(all_photos: &[PhotoEntry]) -> HashMap<String, PhotoEntry> {
    let mut map = HashMap::new();
    for p in all_photos {
        map.insert(p.entry.sha256.clone(), p.clone());
    }
    map
}

fn load_albums_from_meta(
    meta: &AlbumMeta,
    photo_map: &HashMap<String, PhotoEntry>,
) -> Vec<Album> {
    let mut albums = Vec::new();
    for rec in &meta.albums {
        let mut photos = Vec::new();
        for sha in &rec.photo_sha256 {
            if let Some(p) = photo_map.get(sha) {
                photos.push(p.clone());
            }
        }
        photos.sort_by_key(|p| p.sort_dt());
        let start_date = NaiveDate::parse_from_str(&rec.start_date, "%Y-%m-%d").unwrap_or_else(|_| {
            photos.first().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
        });
        let end_date = NaiveDate::parse_from_str(&rec.end_date, "%Y-%m-%d").unwrap_or_else(|_| {
            photos.last().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap())
        });
        albums.push(Album {
            name: rec.name.clone(),
            display_name: rec.display_name.clone(),
            photos,
            start_date,
            end_date,
            location: rec.location.clone(),
        });
    }
    albums
}

fn albums_to_records(albums: &[Album]) -> Vec<AlbumRecord> {
    albums
        .iter()
        .map(|a| AlbumRecord {
            name: a.name.clone(),
            display_name: a.display_name.clone(),
            photo_sha256: a.photos.iter().map(|p| p.entry.sha256.clone()).collect(),
            start_date: format!("{}", a.start_date.format("%Y-%m-%d")),
            end_date: format!("{}", a.end_date.format("%Y-%m-%d")),
            location: a.location.clone(),
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
.photo-album { color: #4299e1; font-size: 11px; margin-top: 2px; }
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

fn render_album_detail(album: &Album, _idx: usize, thumbs_dir: &str, show_album_name: bool) -> String {
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
            let album_tag = if show_album_name {
                format!("<div class=\"photo-album\">📁 {}</div>", album.display_name)
            } else {
                String::new()
            };
            format!(
                r#"<div class="photo-item">
  <div class="photo-thumb">
    <img src="{}" alt="{}" onerror="this.style.display='none'"/>
  </div>
  <div class="photo-info">
    <div class="photo-filename" title="{}">{}</div>
    <div class="photo-date">{}</div>
    {}
  </div>
</div>"#,
                thumb, filename, filename, filename, date, album_tag
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

fn render_search_detail(title: &str, photos: &[(&PhotoEntry, Option<String>)], thumbs_dir: &str, total_albums: usize, total_photos: usize) -> String {
    let photo_items: Vec<String> = photos
        .iter()
        .map(|(p, album_name)| {
            let thumb = format!("{}/{}", thumbs_dir, thumbnail_filename(p));
            let filename = p
                .archive_path_abs
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let date = format!("{}", p.date().format("%Y-%m-%d"));
            let album_tag = if let Some(an) = album_name {
                format!("<div class=\"photo-album\">📁 {}</div>", an)
            } else {
                String::new()
            };
            format!(
                r#"<div class="photo-item">
  <div class="photo-thumb">
    <img src="{}" alt="{}" onerror="this.style.display='none'"/>
  </div>
  <div class="photo-info">
    <div class="photo-filename" title="{}">{}</div>
    <div class="photo-date">{}</div>
    {}
  </div>
</div>"#,
                thumb, filename, filename, filename, date, album_tag
            )
        })
        .collect();

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
<div class="header">
  <h1>🔍 {}</h1>
  <p class="stats">共在 {} 个相册中找到 {} 张匹配照片</p>
</div>
<div class="photos-grid">
{}
</div>
</body>
</html>"#,
        title,
        render_css(),
        title,
        total_albums,
        total_photos,
        if photo_items.is_empty() {
            "<div class=\"empty\">未找到匹配的照片</div>".to_string()
        } else {
            photo_items.join("\n")
        }
    )
}

fn generate_all_html(albums: &[Album], output_album: &Path, thumbs_dir: &Path) -> Result<(), String> {
    let total_photos: usize = albums.iter().map(|a| a.photos.len()).sum();
    let thumbs_rel = "thumbs";

    let index_html = render_index_page(albums, total_photos, thumbs_rel);
    let index_path = output_album.join("index.html");
    std::fs::write(&index_path, index_html)
        .map_err(|e| format!("Failed to write index.html: {}", e))?;

    for (i, album) in albums.iter().enumerate() {
        let detail_html = render_album_detail(album, i + 1, thumbs_rel, false);
        let detail_path = output_album.join(format!("album_{}.html", i + 1));
        std::fs::write(&detail_path, detail_html)
            .map_err(|e| format!("Failed to write album_{}.html: {}", i + 1, e))?;
    }

    let _ = thumbs_dir;
    Ok(())
}

fn generate_thumbnails_for_photos(
    photos: &[PhotoEntry],
    thumbs_dir: &Path,
    rebuild: bool,
) -> Result<Vec<(String, String)>, String> {
    let mut failed: Vec<(String, String)> = Vec::new();

    for photo in photos {
        let thumb_name = thumbnail_filename(photo);
        let thumb_path = thumbs_dir.join(&thumb_name);

        if thumb_path.exists() && !rebuild {
            continue;
        }

        let src_path = &photo.archive_path_abs;
        if !src_path.exists() {
            failed.push((
                photo.entry.archive_path.clone(),
                format!("源文件不存在: {}", src_path.display()),
            ));
            let _ = make_gray_placeholder(&thumb_path);
            continue;
        }

        if let Err(e) = gen_thumbnail(src_path, &thumb_path) {
            failed.push((photo.entry.archive_path.clone(), format!("缩略图失败: {}", e)));
            let _ = make_gray_placeholder(&thumb_path);
        }
    }

    Ok(failed)
}

fn delete_album_html_files(output_album: &Path) {
    if let Ok(entries) = std::fs::read_dir(output_album) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let is_old_index = name == "index.html";
                let is_old_album = name.starts_with("album_") && name.ends_with(".html");
                if is_old_index || is_old_album {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

fn get_photo_album_map(albums: &[Album]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for album in albums {
        for p in &album.photos {
            map.insert(p.entry.sha256.clone(), album.display_name.clone());
        }
    }
    map
}

fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("日期格式错误 '{}': {}. 请使用 YYYY-MM-DD 格式", s, e))
}

pub fn parse_date_range(s: &str) -> Result<(NaiveDate, NaiveDate), String> {
    let parts: Vec<&str> = s.split('~').collect();
    if parts.len() != 2 {
        return Err(format!("日期范围格式错误 '{}'. 请使用 'YYYY-MM-DD~YYYY-MM-DD' 格式", s));
    }
    let start = parse_date(parts[0].trim())?;
    let end = parse_date(parts[1].trim())?;
    if start > end {
        return Err(format!("开始日期不能晚于结束日期: {} > {}", start, end));
    }
    Ok((start, end))
}

fn province_match(province: &str, keyword: &str) -> bool {
    let kw = keyword.trim();
    if kw.is_empty() {
        return true;
    }
    let prov_lower = province.to_lowercase();
    let kw_lower = kw.to_lowercase();
    if prov_lower.contains(&kw_lower) {
        return true;
    }
    if kw_lower.contains(&prov_lower) {
        return true;
    }
    let prov_stripped: String = prov_lower.chars().filter(|c| !"省市自治区特别行政区".contains(*c)).collect();
    let kw_stripped: String = kw_lower.chars().filter(|c| !"省市自治区特别行政区".contains(*c)).collect();
    prov_stripped.contains(&kw_stripped) || kw_stripped.contains(&prov_stripped)
}

pub struct SearchCriteria<'a> {
    pub date_range: Option<(NaiveDate, NaiveDate)>,
    pub location: Option<&'a str>,
    pub name_keyword: Option<&'a str>,
}

impl<'a> SearchCriteria<'a> {
    pub fn is_empty(&self) -> bool {
        self.date_range.is_none() && self.location.is_none() && self.name_keyword.is_none()
    }

    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some((s, e)) = self.date_range {
            parts.push(format!("日期 {}~{}", s.format("%Y-%m-%d"), e.format("%Y-%m-%d")));
        }
        if let Some(loc) = self.location {
            parts.push(format!("地点 '{}'", loc));
        }
        if let Some(kw) = self.name_keyword {
            parts.push(format!("文件名 '{}'", kw));
        }
        if parts.is_empty() {
            "全部照片".to_string()
        } else {
            parts.join(" · ")
        }
    }

    pub fn matches(&self, photo: &PhotoEntry) -> bool {
        if let Some((start, end)) = self.date_range {
            let d = photo.date();
            if d < start || d > end {
                return false;
            }
        }
        if let Some(loc) = self.location {
            let matched = match &photo.entry.geo {
                Some(geo) => province_match(&geo.province, loc),
                None => false,
            };
            if !matched {
                return false;
            }
        }
        if let Some(kw) = self.name_keyword {
            let filename = photo
                .archive_path_abs
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let kw_lower = kw.to_lowercase();
            if !filename.contains(&kw_lower) {
                return false;
            }
        }
        true
    }
}

pub fn run_search(
    input: &Path,
    output_album: &Path,
    criteria: SearchCriteria,
    search_output: Option<&Path>,
) -> Result<(), String> {
    let index = ArchiveIndex::load(input).map_err(|e| {
        format!("归档索引解析失败: {}。请先执行归档操作。", e)
    })?;

    let index_path_check = input.join(".archive-index.json");
    if !index_path_check.exists() {
        return Err(format!("未找到归档索引文件: {}。请先执行归档操作。", index_path_check.display()));
    }

    if index.entries.is_empty() {
        return Err("归档索引为空，没有可搜索的照片".to_string());
    }

    if criteria.is_empty() {
        return Err("未指定任何搜索条件。请使用 --search-date、--search-location 或 --search-name".to_string());
    }

    let all_photos = enrich_entries(&index, input);

    let existing_albums = AlbumMeta::load(output_album)
        .map(|meta| {
            let photo_map = build_photo_map(&all_photos);
            load_albums_from_meta(&meta, &photo_map)
        })
        .unwrap_or_default();
    let photo_album_map = get_photo_album_map(&existing_albums);

    let mut matched: Vec<(PhotoEntry, Option<String>)> = Vec::new();
    for p in &all_photos {
        if criteria.matches(p) {
            let album_name = photo_album_map.get(&p.entry.sha256).cloned();
            matched.push((p.clone(), album_name));
        }
    }

    matched.sort_by(|(a, _), (b, _)| a.sort_dt().cmp(&b.sort_dt()));

    let mut by_album: HashMap<Option<String>, Vec<&(PhotoEntry, Option<String>)>> = HashMap::new();
    for item in &matched {
        by_album.entry(item.1.clone()).or_default().push(item);
    }

    let total_albums = by_album.len();
    let total_photos_count = matched.len();

    println!("\n=== 搜索结果 ===");
    println!("搜索条件: {}", criteria.summary());
    println!("匹配照片: {} 张，分布在 {} 个相册中\n", total_photos_count, total_albums);

    let mut album_keys: Vec<Option<String>> = by_album.keys().cloned().collect();
    album_keys.sort_by(|a, b| {
        let a_str = a.as_deref().unwrap_or("(未归类)");
        let b_str = b.as_deref().unwrap_or("(未归类)");
        a_str.cmp(b_str)
    });

    for album_key in &album_keys {
        let album_name = album_key.as_deref().unwrap_or("(未归类)");
        let items = by_album.get(album_key).unwrap();
        println!("【相册】{} ({} 张)", album_name, items.len());
        for (p, _) in items {
            let filename = p
                .archive_path_abs
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            let date = format!("{}", p.date().format("%Y-%m-%d"));
            println!("  - {}  [{}]", filename, date);
        }
        println!();
    }

    println!("共在 {} 个相册中找到 {} 张匹配照片", total_albums, total_photos_count);

    if let Some(out_dir) = search_output {
        if !out_dir.exists() {
            std::fs::create_dir_all(out_dir)
                .map_err(|e| format!("创建搜索结果目录失败: {}", e))?;
        }

        let thumbs_dir = out_dir.join("thumbs");
        if !thumbs_dir.exists() {
            std::fs::create_dir_all(&thumbs_dir)
                .map_err(|e| format!("创建缩略图目录失败: {}", e))?;
        }

        let src_thumbs_dir = output_album.join("thumbs");
        eprintln!("\n复制缩略图到搜索结果目录...");
        let pb = ProgressBar::new(matched.len().max(1) as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})",
            )
            .unwrap()
            .progress_chars("#>-"),
        );

        let mut new_gen_count = 0usize;
        for (p, _) in &matched {
            let thumb_name = thumbnail_filename(p);
            let dst_thumb = thumbs_dir.join(&thumb_name);

            if !dst_thumb.exists() {
                let src_thumb = src_thumbs_dir.join(&thumb_name);
                if src_thumb.exists() {
                    let _ = std::fs::copy(&src_thumb, &dst_thumb);
                } else {
                    let src_path = &p.archive_path_abs;
                    if src_path.exists() {
                        let _ = gen_thumbnail(src_path, &dst_thumb);
                        new_gen_count += 1;
                    } else {
                        let _ = make_gray_placeholder(&dst_thumb);
                    }
                }
            }
            pb.inc(1);
        }
        pb.finish_with_message(format!("缩略图处理完成 (新生成 {} 张)", new_gen_count));

        let search_photos_ref: Vec<(&PhotoEntry, Option<String>)> = matched
            .iter()
            .map(|(p, a)| (p, a.clone()))
            .collect();

        let title = format!("搜索结果: {}", criteria.summary());
        let html = render_search_detail(&title, &search_photos_ref, "thumbs", total_albums, total_photos_count);
        let html_path = out_dir.join("search_result.html");
        std::fs::write(&html_path, html)
            .map_err(|e| format!("写入搜索结果HTML失败: {}", e))?;

        println!("\n搜索结果已导出:");
        println!("  目录:   {}", out_dir.display());
        println!("  页面:   {}", html_path.display());
        println!("  缩略图: {}", thumbs_dir.display());
    }

    Ok(())
}

pub fn run_merge(
    input: &Path,
    output_album: &Path,
    merge_arg: &str,
) -> Result<(), String> {
    let album_names: Vec<&str> = merge_arg.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if album_names.len() < 2 {
        return Err(format!("合并需要至少2个相册名称，当前传入: {} 个。请用逗号分隔多个相册名", album_names.len()));
    }

    let index = ArchiveIndex::load(input).map_err(|e| format!("归档索引解析失败: {}", e))?;
    let all_photos = enrich_entries(&index, input);
    let photo_map = build_photo_map(&all_photos);

    let mut meta = AlbumMeta::load(output_album).ok_or_else(|| {
        let avail: Vec<String> = Vec::new();
        format!(
            "未找到相册元数据。请先运行 album 命令生成相册。可用相册: {}",
            if avail.is_empty() { "(无)".to_string() } else { avail.join(", ") }
        )
    })?;

    let available = meta.get_album_names();

    let mut missing: Vec<&str> = Vec::new();
    for name in &album_names {
        if !available.contains(&name.to_string()) {
            missing.push(name);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "以下相册不存在: {}\n\n可用相册列表:\n{}",
            missing.join(", "),
            if available.is_empty() {
                "  (无可用相册，请先运行 album 命令生成)".to_string()
            } else {
                available.iter().map(|n| format!("  - {}", n)).collect::<Vec<_>>().join("\n")
            }
        ));
    }

    let target_name = album_names[0].to_string();
    let source_names: Vec<String> = album_names.iter().skip(1).map(|s| s.to_string()).collect();

    let target_rec = meta.find_album(&target_name).unwrap().clone();
    let mut all_sha: Vec<String> = target_rec.photo_sha256.clone();
    for src_name in &source_names {
        if let Some(src) = meta.find_album(src_name) {
            all_sha.extend(src.photo_sha256.iter().cloned());
        }
    }

    let mut merged_photos: Vec<PhotoEntry> = all_sha
        .iter()
        .filter_map(|sha| photo_map.get(sha).cloned())
        .collect();
    merged_photos.sort_by_key(|p| p.sort_dt());

    let thumbs_dir = output_album.join("thumbs");
    if !thumbs_dir.exists() {
        std::fs::create_dir_all(&thumbs_dir)
            .map_err(|e| format!("创建缩略图目录失败: {}", e))?;
    }

    eprintln!("确认缩略图存在（跳过已生成）...");
    let _ = generate_thumbnails_for_photos(&merged_photos, &thumbs_dir, false)?;

    let start_date = merged_photos.first().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
    let end_date = merged_photos.last().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
    let location = determine_location(&merged_photos);

    let merged_album = Album {
        name: target_name.clone(),
        display_name: target_name.clone(),
        photos: merged_photos,
        start_date,
        end_date,
        location,
    };

    let mut remaining_albums: Vec<Album> = Vec::new();
    let loaded = load_albums_from_meta(&meta, &photo_map);
    for alb in loaded {
        if alb.name == target_name {
            continue;
        }
        if source_names.contains(&alb.name) {
            continue;
        }
        remaining_albums.push(alb);
    }
    remaining_albums.insert(0, merged_album);
    remaining_albums.sort_by(|a, b| b.start_date.cmp(&a.start_date).then_with(|| b.name.cmp(&a.name)));

    delete_album_html_files(output_album);
    generate_all_html(&remaining_albums, output_album, &thumbs_dir)?;

    meta.albums = albums_to_records(&remaining_albums);

    let merge_record = MergeRecord {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source_albums: source_names.clone(),
        target_album: target_name.clone(),
    };
    meta.merges.push(merge_record);

    meta.last_run = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    meta.save(output_album)?;

    println!("\n=== 相册合并完成 ===");
    println!("目标相册:        {}", target_name);
    println!("被合并相册:      {}", source_names.join(", "));
    println!("合并后照片数:    {}", remaining_albums.iter().find(|a| a.name == target_name).map(|a| a.photos.len()).unwrap_or(0));
    println!("相册总数:        {}", remaining_albums.len());
    println!("已删除源相册目录与详情页，已重建 index.html");

    Ok(())
}

pub fn run_split(
    input: &Path,
    output_album: &Path,
    split_arg: &str,
) -> Result<(), String> {
    let parts: Vec<&str> = split_arg.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(format!("拆分参数格式错误 '{}'。请使用 '相册名:N' 格式，例如 '2024-01-01_北京:5'", split_arg));
    }
    let album_name = parts[1].trim();
    let split_n_str = parts[0].trim();
    let split_n: usize = split_n_str
        .parse()
        .map_err(|_| format!("拆分点必须是正整数，当前: '{}'", split_n_str))?;

    if album_name.is_empty() {
        return Err("相册名不能为空".to_string());
    }

    let index = ArchiveIndex::load(input).map_err(|e| format!("归档索引解析失败: {}", e))?;
    let all_photos = enrich_entries(&index, input);
    let photo_map = build_photo_map(&all_photos);

    let mut meta = AlbumMeta::load(output_album).ok_or_else(|| {
        "未找到相册元数据。请先运行 album 命令生成相册".to_string()
    })?;

    let available = meta.get_album_names();
    let album_rec = meta.find_album(album_name).ok_or_else(|| {
        format!(
            "相册 '{}' 不存在。\n\n可用相册列表:\n{}",
            album_name,
            if available.is_empty() {
                "  (无可用相册，请先运行 album 命令生成)".to_string()
            } else {
                available.iter().map(|n| format!("  - {}", n)).collect::<Vec<_>>().join("\n")
            }
        )
    })?;

    let total = album_rec.photo_sha256.len();
    if split_n < 1 {
        return Err(format!("拆分点N必须 >= 1，当前 N={}。有效范围: 1 ~ {}", split_n, total.max(1) - 1));
    }
    if split_n >= total {
        return Err(format!("拆分点N必须 < 照片总数。当前 N={}, 照片总数={}。有效范围: 1 ~ {}", split_n, total, total.max(1) - 1));
    }

    let thumbs_dir = output_album.join("thumbs");
    if !thumbs_dir.exists() {
        std::fs::create_dir_all(&thumbs_dir)
            .map_err(|e| format!("创建缩略图目录失败: {}", e))?;
    }

    let all_album_photos: Vec<PhotoEntry> = album_rec
        .photo_sha256
        .iter()
        .filter_map(|sha| photo_map.get(sha).cloned())
        .collect();

    eprintln!("确认缩略图存在（跳过已生成）...");
    let _ = generate_thumbnails_for_photos(&all_album_photos, &thumbs_dir, false)?;

    let first_photos: Vec<PhotoEntry> = all_album_photos.iter().take(split_n).cloned().collect();
    let second_photos: Vec<PhotoEntry> = all_album_photos.iter().skip(split_n).cloned().collect();

    let first_start = first_photos.first().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
    let first_end = first_photos.last().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
    let second_start = second_photos.first().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());
    let second_end = second_photos.last().map(|p| p.date()).unwrap_or_else(|| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());

    let first_album = Album {
        name: album_name.to_string(),
        display_name: album_name.to_string(),
        photos: first_photos,
        start_date: first_start,
        end_date: first_end,
        location: determine_location(&album_rec.photo_sha256.iter().take(split_n).filter_map(|sha| photo_map.get(sha)).cloned().collect::<Vec<_>>()),
    };

    let second_name = format!("{}_split", album_name);
    let second_display = second_name.clone();
    let second_album = Album {
        name: second_name.clone(),
        display_name: second_display,
        photos: second_photos,
        start_date: second_start,
        end_date: second_end,
        location: determine_location(&album_rec.photo_sha256.iter().skip(split_n).filter_map(|sha| photo_map.get(sha)).cloned().collect::<Vec<_>>()),
    };

    let mut remaining_albums: Vec<Album> = Vec::new();
    let loaded = load_albums_from_meta(&meta, &photo_map);
    let mut inserted = false;
    for alb in loaded {
        if alb.name == album_name {
            remaining_albums.push(first_album.clone());
            remaining_albums.push(second_album.clone());
            inserted = true;
        } else {
            remaining_albums.push(alb);
        }
    }
    if !inserted {
        remaining_albums.push(first_album.clone());
        remaining_albums.push(second_album.clone());
    }
    remaining_albums.sort_by(|a, b| b.start_date.cmp(&a.start_date).then_with(|| b.name.cmp(&a.name)));

    delete_album_html_files(output_album);
    generate_all_html(&remaining_albums, output_album, &thumbs_dir)?;

    meta.albums = albums_to_records(&remaining_albums);

    let split_record = SplitRecord {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        source_album: album_name.to_string(),
        split_point: split_n,
        first_half: album_name.to_string(),
        second_half: second_name.clone(),
    };
    meta.splits.push(split_record);

    meta.last_run = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    meta.save(output_album)?;

    println!("\n=== 相册拆分完成 ===");
    println!("原相册:          {}", album_name);
    println!("拆分点:          第 {} 张之后", split_n);
    println!("前半部分:        {} ({} 张)", album_name, first_album.photos.len());
    println!("后半部分:        {} ({} 张)", second_name, second_album.photos.len());
    println!("相册总数:        {}", remaining_albums.len());
    println!("已重建两个详情页与 index.html");

    Ok(())
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

    let mut existing_meta = AlbumMeta::load(output_album);
    let last_run = if rebuild {
        None
    } else {
        existing_meta
            .as_ref()
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
        let mut meta = existing_meta.unwrap_or_else(|| AlbumMeta {
            last_run: String::new(),
            version: "1.0".to_string(),
            merges: Vec::new(),
            splits: Vec::new(),
            albums: Vec::new(),
        });
        meta.last_run = now_str;
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
    delete_album_html_files(output_album);

    eprintln!("生成 HTML 页面...");
    generate_all_html(&albums, output_album, &thumbs_dir)?;

    let now_str = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let mut meta = existing_meta.take().unwrap_or_else(|| AlbumMeta {
        last_run: String::new(),
        version: "1.0".to_string(),
        merges: Vec::new(),
        splits: Vec::new(),
        albums: Vec::new(),
    });
    meta.last_run = now_str.clone();
    meta.version = "1.0".to_string();
    meta.albums = albums_to_records(&albums);
    meta.save(output_album)?;

    println!("\n=== 相册生成报告 ===");
    println!("  相册数量:          {}", albums.len());
    println!("  照片总数:          {}", total_photos);
    println!("  输出目录:          {}", output_album.display());
    println!("  索引页面:          {}", output_album.join("index.html").display());
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
