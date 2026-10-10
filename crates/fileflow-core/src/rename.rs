//! Deterministic rename templates. Supported placeholders:
//!
//! * `{name}`         — original filename with extension (e.g. `invoice.pdf`)
//! * `{original_name}`— alias for `{name}`
//! * `{stem}`         — filename without extension
//! * `{ext}`          — extension without leading dot (lowercase, may be empty)
//! * `{category}`     — deterministic category label ("documents", …)
//! * `{date}`         — modified date `YYYY-MM-DD` (falls back to created, then `now`)
//! * `{year}` / `{month}` / `{day}` — parts of the same date
//! * `{now}`          — the current time, `YYYY-MM-DD_HH-MM-SS`
//!
//! All dates are UTC. Rendered file names are sanitized (see
//! [`sanitize_filename`]); destination folder templates are rendered per
//! path component so `{year}/` style subfolders work.

use crate::error::{Error, Result};
use crate::model::FileMeta;
use std::path::{Component, Path, PathBuf};
use time::OffsetDateTime;

/// Characters disallowed on Windows filenames (also rejected on other
/// platforms so exported rules stay portable).
const INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*', '\0'];

/// Reserved Windows device names — must not appear as the stem, even with an
/// extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const TOKENS: &[&str] = &[
    "name",
    "original_name",
    "stem",
    "ext",
    "category",
    "date",
    "year",
    "month",
    "day",
    "now",
];

enum Piece<'a> {
    Literal(&'a str),
    Token(&'a str),
}

fn pieces(template: &str) -> Result<Vec<Piece<'_>>> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if open > 0 {
            out.push(Piece::Literal(&rest[..open]));
        }
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| Error::InvalidFilename("unclosed '{' in template".into()))?;
        out.push(Piece::Token(&after[..close]));
        rest = &after[close + 1..];
    }
    if !rest.is_empty() {
        out.push(Piece::Literal(rest));
    }
    Ok(out)
}

fn unknown_token(tok: &str) -> Error {
    Error::InvalidFilename(format!("unknown token {{{tok}}}"))
}

/// Checks a file-name template without needing a file.
pub fn validate_template(template: &str) -> Result<()> {
    for piece in pieces(template)? {
        if let Piece::Token(t) = piece {
            if !TOKENS.contains(&t) {
                return Err(unknown_token(t));
            }
        }
    }
    Ok(())
}

/// Checks every component of a destination-folder template.
pub fn validate_dir_template(template: &Path) -> Result<()> {
    for c in template.components() {
        match c {
            Component::ParentDir => {
                return Err(Error::Safety("'..' is not allowed in folder paths".into()))
            }
            Component::Normal(os) => {
                validate_template(os.to_str().ok_or_else(|| {
                    Error::InvalidFilename("folder name is not valid UTF-8".into())
                })?)?
            }
            _ => {}
        }
    }
    Ok(())
}

fn value(tok: &str, f: &FileMeta, ts: OffsetDateTime, now: OffsetDateTime) -> Result<String> {
    Ok(match tok {
        "name" | "original_name" => f.name.clone(),
        "stem" => f.stem.clone(),
        "ext" => f.extension.clone(),
        "category" => f.category.as_str().to_string(),
        "date" => format!(
            "{:04}-{:02}-{:02}",
            ts.year(),
            u8::from(ts.month()),
            ts.day()
        ),
        "year" => format!("{:04}", ts.year()),
        "month" => format!("{:02}", u8::from(ts.month())),
        "day" => format!("{:02}", ts.day()),
        "now" => format!(
            "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
            now.year(),
            u8::from(now.month()),
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        ),
        other => return Err(unknown_token(other)),
    })
}

fn expand(template: &str, file: &FileMeta, now: OffsetDateTime) -> Result<String> {
    let ts = file.modified.or(file.created).unwrap_or(now);
    let mut out = String::with_capacity(template.len() + 16);
    for piece in pieces(template)? {
        match piece {
            Piece::Literal(s) => out.push_str(s),
            Piece::Token(t) => out.push_str(&value(t, file, ts, now)?),
        }
    }
    Ok(out)
}

/// Renders a file-name template and sanitizes the result.
pub fn render(template: &str, file: &FileMeta, now: OffsetDateTime) -> Result<String> {
    sanitize_filename(&expand(template, file, now)?)
}

/// Renders a destination-folder template. Literal components are kept as the
/// user typed them; components that contain tokens are expanded and
/// sanitized, so a token can never inject a path separator or `..`.
pub fn render_dir(template: &Path, file: &FileMeta, now: OffsetDateTime) -> Result<PathBuf> {
    let mut out = PathBuf::new();
    for c in template.components() {
        match c {
            Component::ParentDir => {
                return Err(Error::Safety("'..' is not allowed in folder paths".into()))
            }
            Component::Normal(os) => {
                let s = os.to_str().ok_or_else(|| {
                    Error::InvalidFilename("folder name is not valid UTF-8".into())
                })?;
                if s.contains('{') {
                    out.push(render(s, file, now)?);
                } else {
                    out.push(os);
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Ok(out)
}

/// Replaces characters that are invalid on any supported platform and rejects
/// names that cannot be made valid (empty, reserved device names, too long).
pub fn sanitize_filename(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(Error::InvalidFilename("empty".into()));
    }
    let cleaned: String = name
        .chars()
        .map(|c| {
            if INVALID.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Windows disallows trailing `.` or space.
    let trimmed = cleaned.trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        return Err(Error::InvalidFilename("only invalid characters".into()));
    }
    let stem_upper = crate::paths::split_name(&trimmed).0.to_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        return Err(Error::InvalidFilename(format!("reserved name: {trimmed}")));
    }
    // 255 bytes is the practical limit; stay below it.
    if trimmed.len() > 240 {
        return Err(Error::InvalidFilename("too long".into()));
    }
    Ok(trimmed)
}
