mod common;

use common::synth;
use fileflow_core::rename;
use time::OffsetDateTime;

#[test]
fn template_expands_date_and_original_name() {
    let f = synth("invoice.pdf", "/downloads", 10, 1);
    let out = rename::render("{date}-{original_name}", &f, OffsetDateTime::now_utc()).unwrap();
    assert!(out.ends_with("-invoice.pdf"));
    assert_eq!(out.len(), "YYYY-MM-DD-invoice.pdf".len());
}

#[test]
fn template_expands_stem_and_ext() {
    let f = synth("report.docx", "/x", 1, 0);
    let out = rename::render("{stem}.copy.{ext}", &f, OffsetDateTime::now_utc()).unwrap();
    assert_eq!(out, "report.copy.docx");
}

#[test]
fn unknown_token_errors() {
    let f = synth("a.txt", "/x", 1, 0);
    let err = rename::render("{nope}", &f, OffsetDateTime::now_utc()).unwrap_err();
    assert!(format!("{err}").contains("unknown token"));
}

#[test]
fn invalid_chars_are_replaced() {
    let cleaned = rename::sanitize_filename("bad:name?.txt").unwrap();
    assert!(!cleaned.contains(':'));
    assert!(!cleaned.contains('?'));
}

#[test]
fn windows_reserved_names_rejected() {
    assert!(rename::sanitize_filename("CON").is_err());
    assert!(rename::sanitize_filename("nul.txt").is_err());
    assert!(rename::sanitize_filename("COM3.log").is_err());
}

#[test]
fn empty_or_only_invalid_rejected() {
    assert!(rename::sanitize_filename("").is_err());
    assert!(rename::sanitize_filename("   .").is_err());
}
