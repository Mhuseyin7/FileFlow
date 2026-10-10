use fileflow_core::category::Category;
use fileflow_core::model::FileMeta;
use fileflow_core::paths::split_name;
use std::path::PathBuf;
use time::OffsetDateTime;

pub fn synth(name: &str, parent: &str, size: u64, modified_days_ago: i64) -> FileMeta {
    let now = OffsetDateTime::now_utc();
    let modified = now - time::Duration::days(modified_days_ago);
    let (stem, ext) = split_name(name);
    let ext = ext.to_lowercase();
    FileMeta {
        path: PathBuf::from(parent).join(name),
        parent: PathBuf::from(parent),
        name: name.to_string(),
        stem: stem.to_string(),
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
