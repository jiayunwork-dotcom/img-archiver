use crate::types::{GpsLocation, ImageFormat, ImageMetadata};
use chrono::NaiveDateTime;

pub fn extract_metadata(path: &std::path::Path, format: ImageFormat) -> ImageMetadata {
    let file_modified = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| {
            let dt: chrono::DateTime<chrono::Local> = t.into();
            Some(dt.naive_local())
        })
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap().and_hms_opt(0, 0, 0).unwrap());

    let (width, height) = read_dimensions(path, format);

    let exif_data = read_exif(path);

    match exif_data {
        Some(ref fields) => {
            let date_time = extract_date_time(fields).unwrap_or(file_modified);
            let camera_brand = get_field(fields, "Make").unwrap_or_else(|| "unknown".to_string());
            let camera_model = get_field(fields, "Model").unwrap_or_else(|| "unknown".to_string());
            let lens_model = get_field(fields, "LensModel").unwrap_or_else(|| "unknown".to_string());
            let iso = get_field(fields, "ISOSpeedRatings").unwrap_or_else(|| "unknown".to_string());
            let exposure_time = get_field(fields, "ExposureTime")
                .unwrap_or_else(|| "unknown".to_string());
            let aperture = get_field(fields, "FNumber").unwrap_or_else(|| "unknown".to_string());
            let focal_length = get_field(fields, "FocalLength")
                .unwrap_or_else(|| "unknown".to_string());
            let gps = extract_gps(fields);

            ImageMetadata {
                date_time,
                camera_brand,
                camera_model,
                lens_model,
                iso,
                exposure_time,
                aperture,
                focal_length,
                width,
                height,
                gps,
                geo: None,
            }
        }
        None => ImageMetadata {
            date_time: file_modified,
            camera_brand: "unknown".to_string(),
            camera_model: "unknown".to_string(),
            lens_model: "unknown".to_string(),
            iso: "unknown".to_string(),
            exposure_time: "unknown".to_string(),
            aperture: "unknown".to_string(),
            focal_length: "unknown".to_string(),
            width,
            height,
            gps: None,
            geo: None,
        },
    }
}

fn read_exif(path: &std::path::Path) -> Option<Vec<exif::Field>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bufreader = std::io::BufReader::new(file);
    let exif_reader = exif::Reader::new();
    let exif_data = exif_reader.read_from_container(&mut bufreader).ok()?;
    Some(exif_data.fields().cloned().collect())
}

fn get_field(fields: &[exif::Field], tag_name: &str) -> Option<String> {
    for field in fields {
        if field.tag.to_string() == tag_name {
            return Some(format_exif_value(&field.value));
        }
    }
    None
}

fn format_exif_value(value: &exif::Value) -> String {
    match value {
        exif::Value::Byte(v) => v.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
        exif::Value::Ascii(v) => {
            let s = v
                .iter()
                .filter_map(|entry| String::from_utf8(entry.clone()).ok())
                .collect::<Vec<_>>()
                .join(", ");
            s.trim_end_matches('\0').trim().to_string()
        }
        exif::Value::Short(v) => v.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(","),
        exif::Value::Long(v) => v.iter().map(|l| l.to_string()).collect::<Vec<_>>().join(","),
        exif::Value::Rational(v) => v
            .iter()
            .map(|r| {
                if r.denom != 0 {
                    let val = r.num as f64 / r.denom as f64;
                    if (val - val.round()).abs() < f64::EPSILON {
                        format!("{}", val as i64)
                    } else {
                        format!("{:.1}", val)
                    }
                } else {
                    "0".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        exif::Value::SRational(v) => v
            .iter()
            .map(|r| {
                if r.denom != 0 {
                    let val = r.num as f64 / r.denom as f64;
                    if (val - val.round()).abs() < f64::EPSILON {
                        format!("{}", val as i64)
                    } else {
                        format!("{:.1}", val)
                    }
                } else {
                    "0".to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        _ => format!("{:?}", value),
    }
}

fn extract_date_time(fields: &[exif::Field]) -> Option<NaiveDateTime> {
    let date_str = get_field(fields, "DateTimeOriginal")
        .or_else(|| get_field(fields, "DateTimeDigitized"))
        .or_else(|| get_field(fields, "DateTime"))?;

    parse_exif_datetime(&date_str)
}

fn parse_exif_datetime(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    let formats = ["%Y:%m:%d %H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y/%m/%d %H:%M:%S"];
    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt);
        }
    }
    None
}

fn extract_gps(fields: &[exif::Field]) -> Option<GpsLocation> {
    let lat_str = get_field(fields, "GPSLatitude")?;
    let lat_ref = get_field(fields, "GPSLatitudeRef")?;
    let lon_str = get_field(fields, "GPSLongitude")?;
    let lon_ref = get_field(fields, "GPSLongitudeRef")?;

    let lat = parse_gps_coordinate(&lat_str, &lat_ref)?;
    let lon = parse_gps_coordinate(&lon_str, &lon_ref)?;

    if lat == 0.0 && lon == 0.0 {
        return None;
    }

    Some(GpsLocation {
        latitude: lat,
        longitude: lon,
    })
}

fn parse_gps_coordinate(coord_str: &str, ref_str: &str) -> Option<f64> {
    let parts: Vec<&str> = coord_str.split(',').collect();
    if parts.len() < 3 {
        return None;
    }

    let degrees: f64 = parts[0].trim().parse().ok()?;
    let minutes: f64 = parts[1].trim().parse().ok()?;
    let seconds: f64 = parts[2].trim().parse().ok()?;

    let mut result = degrees + minutes / 60.0 + seconds / 3600.0;

    if ref_str.trim().starts_with('S') || ref_str.trim().starts_with('W') {
        result = -result;
    }

    Some(result)
}

fn read_dimensions(path: &std::path::Path, format: ImageFormat) -> (u32, u32) {
    if format == ImageFormat::Jpeg || format == ImageFormat::Png || format == ImageFormat::WebP {
        if let Ok(dimensions) = image::image_dimensions(path) {
            return dimensions;
        }
    }

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return (0, 0),
    };
    let mut bufreader = std::io::BufReader::new(file);
    let exif_reader = exif::Reader::new();
    if let Ok(exif_data) = exif_reader.read_from_container(&mut bufreader) {
        for field in exif_data.fields() {
            let tag_str = field.tag.to_string();
            if tag_str == "PixelXDimension" || tag_str == "ImageWidth" {
                if let exif::Value::Long(v) = &field.value {
                    if let Some(&w) = v.first() {
                        for field2 in exif_data.fields() {
                            let tag2 = field2.tag.to_string();
                            if tag2 == "PixelYDimension" || tag2 == "ImageLength" {
                                if let exif::Value::Long(v2) = &field2.value {
                                    if let Some(&h) = v2.first() {
                                        return (w as u32, h as u32);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (0, 0)
}
