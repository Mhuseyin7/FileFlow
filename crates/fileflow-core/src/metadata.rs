//! Optional metadata readers for photos and PDFs. Purely local reads — no
//! network, no OCR. Behind the `metadata` feature so the core stays lean
//! when these conditions aren't used.

use std::path::Path;
use time::OffsetDateTime;

/// Structured photo/EXIF facts. `None` on any field means "unknown", never
/// "false".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhotoMeta {
    pub capture_date: Option<OffsetDateTime>,
    pub camera: Option<String>,
}

/// Structured PDF facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PdfMeta {
    pub page_count: Option<u32>,
    pub title: Option<String>,
}

#[cfg(feature = "metadata")]
pub fn read_photo(path: &Path) -> Option<PhotoMeta> {
    use exif::{In, Tag};
    let file = std::fs::File::open(path).ok()?;
    let mut buf = std::io::BufReader::new(&file);
    let exif = exif::Reader::new().read_from_container(&mut buf).ok()?;

    let capture_date = exif
        .get_field(Tag::DateTimeOriginal, In::PRIMARY)
        .and_then(|f| parse_exif_datetime(&f.display_value().to_string()));
    let camera = exif
        .get_field(Tag::Model, In::PRIMARY)
        .map(|f| f.display_value().with_unit(&exif).to_string().trim_matches('"').to_string());

    Some(PhotoMeta { capture_date, camera })
}

#[cfg(not(feature = "metadata"))]
pub fn read_photo(_path: &Path) -> Option<PhotoMeta> { None }

#[cfg(feature = "metadata")]
pub fn read_pdf(path: &Path) -> Option<PdfMeta> {
    let doc = lopdf::Document::load(path).ok()?;
    let page_count = Some(doc.get_pages().len() as u32);
    let title = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|info_ref| info_ref.as_reference().ok())
        .and_then(|id| doc.get_object(id).ok())
        .and_then(|obj| obj.as_dict().ok())
        .and_then(|dict| dict.get(b"Title").ok())
        .and_then(|obj| obj.as_str().ok())
        .and_then(|bytes| std::str::from_utf8(bytes).ok().map(|s| s.to_string()));
    Some(PdfMeta { page_count, title })
}

#[cfg(not(feature = "metadata"))]
pub fn read_pdf(_path: &Path) -> Option<PdfMeta> { None }

#[cfg(feature = "metadata")]
fn parse_exif_datetime(s: &str) -> Option<OffsetDateTime> {
    // EXIF DateTimeOriginal: "YYYY:MM:DD HH:MM:SS" (quoted by display_value).
    let cleaned = s.trim_matches('"').trim();
    let (date_part, time_part) = cleaned.split_once(' ')?;
    let mut d = date_part.split(':');
    let y: i32 = d.next()?.parse().ok()?;
    let m: u8 = d.next()?.parse().ok()?;
    let day: u8 = d.next()?.parse().ok()?;
    let mut t = time_part.split(':');
    let hh: u8 = t.next()?.parse().ok()?;
    let mm: u8 = t.next()?.parse().ok()?;
    let ss: u8 = t.next()?.parse().ok()?;
    let month = time::Month::try_from(m).ok()?;
    let date = time::Date::from_calendar_date(y, month, day).ok()?;
    let tim = time::Time::from_hms(hh, mm, ss).ok()?;
    Some(time::PrimitiveDateTime::new(date, tim).assume_utc())
}
