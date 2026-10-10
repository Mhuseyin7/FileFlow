//! Deterministic file categorization. No AI, no network. Extension-first,
//! MIME-guess as a hint. Missing / unknown extensions map to `Other`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Documents,
    Images,
    Video,
    Audio,
    Archives,
    Installers,
    Code,
    Data,
    Other,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Documents => "documents",
            Category::Images => "images",
            Category::Video => "video",
            Category::Audio => "audio",
            Category::Archives => "archives",
            Category::Installers => "installers",
            Category::Code => "code",
            Category::Data => "data",
            Category::Other => "other",
        }
    }

    pub fn from_extension(ext: &str) -> Self {
        // Extension is expected to be lowercase, without a leading dot.
        match ext {
            // Documents
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "tex" | "epub" | "mobi"
            | "pages" | "xls" | "xlsx" | "ods" | "csv" | "ppt" | "pptx" | "odp" | "key" => {
                Category::Documents
            }
            // Images
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tiff" | "tif" | "webp" | "heic" | "heif"
            | "svg" | "ico" | "raw" | "cr2" | "nef" | "arw" | "dng" => Category::Images,
            // Video
            "mp4" | "mov" | "mkv" | "avi" | "webm" | "wmv" | "flv" | "m4v" | "mpeg" | "mpg" => {
                Category::Video
            }
            // Audio
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "opus" | "aiff" => {
                Category::Audio
            }
            // Archives
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "7z" | "rar" | "xz" | "zst" | "iso" => {
                Category::Archives
            }
            // Installers
            "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" | "apk" => {
                Category::Installers
            }
            // Code
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "kt" | "swift" | "c"
            | "h" | "cpp" | "hpp" | "cs" | "rb" | "php" | "sh" | "ps1" | "sql" | "lua" | "vim"
            | "toml" | "yaml" | "yml" => Category::Code,
            // Structured data
            "json" | "xml" | "parquet" | "avro" | "arrow" | "ndjson" | "jsonl" => Category::Data,
            _ => Category::Other,
        }
    }
}

/// Very small extension → MIME table. This is a *hint* for rules that want to
/// match on MIME; we deliberately don't sniff content.
pub fn guess_mime(ext: &str) -> Option<String> {
    let m = match ext {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "json" => "application/json",
        "csv" => "text/csv",
        "txt" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "exe" => "application/vnd.microsoft.portable-executable",
        "msi" => "application/x-msi",
        _ => return None,
    };
    Some(m.to_string())
}
