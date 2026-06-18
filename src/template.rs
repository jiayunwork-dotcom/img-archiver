use crate::types::ImageMetadata;

pub fn render_template(
    template: &str,
    metadata: &ImageMetadata,
    seq: usize,
    seq_digits: usize,
    unknown_placeholder: &str,
) -> String {
    let dt = metadata.date_time;
    let year = format!("{}", dt.format("%Y"));
    let month = format!("{}", dt.format("%m"));
    let day = format!("{}", dt.format("%d"));
    let camera = resolve_unknown(&metadata.camera_model, unknown_placeholder);
    let lens = resolve_unknown(&metadata.lens_model, unknown_placeholder);
    let ext = get_ext_from_path_placeholder(&metadata);
    let width = format!("{}", metadata.width);
    let height = format!("{}", metadata.height);
    let seq_str = format!("{:0>width$}", seq, width = seq_digits);

    let province = metadata
        .geo
        .as_ref()
        .map(|g| g.province.clone())
        .unwrap_or_else(|| unknown_placeholder.to_string());
    let city = metadata
        .geo
        .as_ref()
        .map(|g| g.city.clone())
        .unwrap_or_else(|| unknown_placeholder.to_string());

    let mut result = template.to_string();

    result = result.replace("{year}", &year);
    result = result.replace("{month}", &month);
    result = result.replace("{day}", &day);
    result = result.replace("{camera}", &camera);
    result = result.replace("{lens}", &lens);
    result = result.replace("{city}", &city);
    result = result.replace("{province}", &province);
    result = result.replace("{ext}", &ext);
    result = result.replace("{width}", &width);
    result = result.replace("{height}", &height);
    result = result.replace("{seq}", &seq_str);

    if cfg!(windows) {
        result = result.replace('/', "\\");
    } else {
        result = result.replace('\\', "/");
    }

    result = sanitize_path(&result);

    result
}

fn resolve_unknown(value: &str, placeholder: &str) -> String {
    if value == "unknown" || value.is_empty() {
        placeholder.to_string()
    } else {
        value.to_string()
    }
}

fn get_ext_from_path_placeholder(_metadata: &ImageMetadata) -> String {
    "jpg".to_string()
}

pub fn get_file_ext(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".to_string())
}

fn sanitize_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => result.push('_'),
            _ => result.push(ch),
        }
    }
    result
}

#[allow(dead_code)]
pub fn extract_template_dir(template: &str) -> Option<String> {
    let sep = if cfg!(windows) { '\\' } else { '/' };
    if let Some(pos) = template.rfind(sep) {
        Some(template[..pos].to_string())
    } else if let Some(pos) = template.rfind('/') {
        Some(template[..pos].to_string())
    } else {
        None
    }
}
