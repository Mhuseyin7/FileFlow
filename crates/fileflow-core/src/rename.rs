//! Deterministic rename templates. Supported placeholders:
//!
//! * `{name}`         — original filename with extension (e.g. `invoice.pdf`)
//! * `{original_name}`— alias for `{name}`
//! * `{stem}`         — filename without extension
//! * `{ext}`          — extension without leading dot (lowercase, may be empty)
//! * `{category}`     — deterministic category label ("documents", …)
//! * `{date}`         — modified date `YYYY-MM-DD` (falls back to created; then `now`)
//! * `{year}` / `{month}` / `{day}`
//! * `{now}`          — current time `YYYY-MM-DD_HH-MM-SS`
//!
//! Any character that is invalid on the target platform is stripped or
//! replaced with `_`. See [`sanitize_filename`].

use crate::error::{Error, Result};
use crate::model::FileMeta;
use time::OffsetDateTime;

/// Characters disallowed on Windows filenames (also stripped on other
/// platforms so exported rules stay portable).
const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\0'];

/// Reserved Windows device names — must not appear as the stem, even with an
/// extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn render(template: &str, file: &FileMeta, now: OffsetDateTime) -> Result<String> {
    let ts = file.modified.or(file.created).unwrap_or(now);
    let (y, m, d) = (ts.year(), u8::from(ts.month()), ts.day());
    let mut out = String::with_capacity(template.len() + 16);
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = template[i + 1..].find('}') {
                let key = &template[i + 1..i + 1 + end];
                let replacement = match key {
                    "name" | "original_name" => file.name.clone(),
                    "stem" => file.stem.clone(),
                    "ext" => file.extension.clone(),
                    "category" => format!("{:?}", file.category).to_lowercase(),
                    "date" => format!("{y:04}-{m:02}-{d:02}"),
                    "year" => format!("{y:04}"),
                    "month" => format!("{m:02}"),
                    "day" => format!("{d:02}"),
                    "now" => {
                        let (h, mi, s) = (now.hour(), now.minute(), now.second());
                        format!("{y:04}-{m:02}-{d:02}_{h:02}-{mi:02}-{s:02}")
                    }
                    other => return Err(Error::InvalidFilename(format!("unknown token {{{other}}}"))),
                };
                out.push_str(&replacement);
                i += end + 2;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    sanitize_filename(&out)
}

pub fn sanitize_filename(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(Error::InvalidFilename("empty".into()));
    }
    let cleaned: String = name
        .chars()
        .map(|c| if INVALID.contains(&c) || c.is_control() { '_' } else { c })
        .collect();
    // Windows disallows trailing `.` or space.
    let trimmed = cleaned.trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        return Err(Error::InvalidFilename("only invalid characters".into()));
    }
    let stem_upper = trimmed
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&trimmed)
        .to_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        return Err(Error::InvalidFilename(format!("reserved name: {trimmed}")));
    }
    // MAX_PATH considerations: 255 chars is the practical filesystem limit.
    if trimmed.len() > 240 {
        return Err(Error::InvalidFilename("too long".into()));
    }
    Ok(trimmed)
}
