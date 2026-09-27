use fileflow_core::category::{guess_mime, Category};

#[test]
fn documents_covers_common_types() {
    for ext in ["pdf", "docx", "md", "txt", "xlsx"] {
        assert_eq!(Category::from_extension(ext), Category::Documents, "{ext}");
    }
}

#[test]
fn installers_covers_platform_installers() {
    for ext in ["exe", "msi", "dmg", "pkg", "deb", "rpm", "appimage", "apk"] {
        assert_eq!(Category::from_extension(ext), Category::Installers, "{ext}");
    }
}

#[test]
fn unknown_falls_back_to_other() {
    assert_eq!(Category::from_extension("wtf"), Category::Other);
    assert_eq!(Category::from_extension(""), Category::Other);
}

#[test]
fn mime_hint_defined_for_common_types() {
    assert_eq!(guess_mime("pdf").as_deref(), Some("application/pdf"));
    assert_eq!(guess_mime("txt").as_deref(), Some("text/plain"));
    assert_eq!(guess_mime("wtf"), None);
}
