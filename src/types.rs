use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Tiff,
    WebP,
    Heic,
}

impl ImageFormat {
    pub fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "tif" | "tiff" => Some(Self::Tiff),
            "webp" => Some(Self::WebP),
            "heic" | "heif" => Some(Self::Heic),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::Tiff => "TIFF",
            Self::WebP => "WebP",
            Self::Heic => "HEIC",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpsLocation {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub struct GeoLocation {
    pub province: String,
    pub city: String,
    pub district: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ImageMetadata {
    pub date_time: chrono::NaiveDateTime,
    pub camera_brand: String,
    pub camera_model: String,
    pub lens_model: String,
    pub iso: String,
    pub exposure_time: String,
    pub aperture: String,
    pub focal_length: String,
    pub width: u32,
    pub height: u32,
    pub gps: Option<GpsLocation>,
    pub geo: Option<GeoLocation>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ImageInfo {
    pub path: PathBuf,
    pub file_size: u64,
    pub format: ImageFormat,
    pub metadata: ImageMetadata,
    pub phash: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveMode {
    Copy,
    Move,
    Link,
}

impl ArchiveMode {
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "copy" => Ok(Self::Copy),
            "move" => Ok(Self::Move),
            "link" => Ok(Self::Link),
            _ => Err(format!("Invalid archive mode: {}. Use copy, move, or link.", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateType {
    Exact,
    Suspected,
    None,
}

#[derive(Debug, Clone)]
pub struct DuplicateInfo {
    pub dup_type: DuplicateType,
    pub original_path: Option<PathBuf>,
}

#[derive(Debug)]
pub struct ScanSummary {
    pub total_count: usize,
    pub total_size: u64,
    pub format_counts: std::collections::HashMap<ImageFormat, usize>,
}
