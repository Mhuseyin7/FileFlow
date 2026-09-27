//! Optional metadata-condition tests. Only run when the `metadata` feature
//! is enabled (the default). We build a valid single-page PDF with lopdf
//! itself so xref offsets are always correct.

#![cfg(feature = "metadata")]

use fileflow_core::metadata::{read_pdf, read_photo};
use lopdf::dictionary;
use lopdf::{Document, Object};
use std::io::Write;
use tempfile::NamedTempFile;

fn build_mini_pdf(path: &std::path::Path) -> anyhow::Result<()> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    let info_id = doc.add_object(dictionary! {
        "Title" => Object::string_literal("FileFlow test"),
    });
    doc.trailer.set("Root", catalog_id);
    doc.trailer.set("Info", info_id);
    doc.save(path)?;
    Ok(())
}

#[test]
fn read_pdf_returns_page_count() -> anyhow::Result<()> {
    let f = NamedTempFile::new()?;
    build_mini_pdf(f.path())?;
    let meta = read_pdf(f.path()).expect("pdf parses");
    assert_eq!(meta.page_count, Some(1));
    Ok(())
}

#[test]
fn read_photo_returns_none_for_non_image() {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(b"hello").unwrap();
    f.flush().unwrap();
    assert!(read_photo(f.path()).is_none());
}
