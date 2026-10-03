use fileflow_core::category::Category;
use fileflow_core::model::FileMeta;
use std::path::PathBuf;
use time::OffsetDateTime;

pub fn synth(name: &str, parent: &str, size: u64, modified_days_ago: i64) -> FileMeta {
    let now = OffsetDateTime::now_utc();
    let modified = now - time::Duration::days(modified_days_ago);
    let (stem, ext) = split(name);
    FileMeta {
        path: PathBuf::from(parent).join(name),
        parent: PathBuf::from(parent),
        name: name.to_string(),
        stem,
        extension: ext.clone(),
        size,
        is_hidden: name.starts_with('.'),
        is_symlink: false,
        created: Some(modified),
        modified: Some(modified),
        mime: fileflow_core::category::guess_mime(&ext),
        category: Category::from_extension(&ext),
    }
}

fn split(name: &str) -> (String, String) {
    if name.starts_with('.') {
        return (name.to_string(), String::new());
    }
    match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), e.to_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}
