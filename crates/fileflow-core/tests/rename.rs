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
    assert!(rename::sanitize_filename("..").is_err());
}

#[test]
fn non_ascii_text_is_preserved() {
    let f = synth("fatura.pdf", "/x", 1, 0);
    let out = rename::render("şirket-çğıöü-{stem}.{ext}", &f, OffsetDateTime::now_utc()).unwrap();
    assert_eq!(out, "şirket-çğıöü-fatura.pdf");
}

#[test]
fn now_token_is_all_current_time() {
    // The file is a year old; {now} must not borrow its date.
    let f = synth("a.txt", "/x", 1, 365);
    let now = OffsetDateTime::now_utc();
    let out = rename::render("{now}", &f, now).unwrap();
    assert!(out.starts_with(&format!("{:04}-", now.year())), "{out}");
}

#[test]
fn unclosed_brace_is_an_error() {
    let f = synth("a.txt", "/x", 1, 0);
    assert!(rename::render("{name", &f, OffsetDateTime::now_utc()).is_err());
    assert!(rename::validate_template("{name").is_err());
}

#[test]
fn validate_template_checks_tokens() {
    assert!(rename::validate_template("{date}-{stem}.{ext}").is_ok());
    assert!(rename::validate_template("{nope}").is_err());
}

#[test]
fn render_dir_expands_tokens_per_component() {
    let f = synth("a.txt", "/x", 1, 0);
    let now = OffsetDateTime::now_utc();
    let tmpl = std::path::Path::new("/archive/{year}/{category}");
    let out = rename::render_dir(tmpl, &f, now).unwrap();
    let expect = std::path::Path::new("/archive")
        .join(format!("{:04}", now.year()))
        .join("documents");
    assert_eq!(out, expect);
}

#[test]
fn render_dir_rejects_parent_dir_and_bad_tokens() {
    let f = synth("a.txt", "/x", 1, 0);
    let now = OffsetDateTime::now_utc();
    assert!(rename::render_dir(std::path::Path::new("/a/../b"), &f, now).is_err());
    assert!(rename::render_dir(std::path::Path::new("/a/{nope}"), &f, now).is_err());
    assert!(rename::validate_dir_template(std::path::Path::new("/a/../b")).is_err());
    assert!(rename::validate_dir_template(std::path::Path::new("/a/{year}")).is_ok());
}
