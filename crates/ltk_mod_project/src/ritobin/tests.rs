use std::io::Cursor;

use ltk_meta::Bin;

use super::*;

const MINIMAL: &str = r#"#PROP_text
type: string = "PROP"
version: u32 = 3
linked: list[string] = { }
entries: map[hash, embed] = { }
"#;

#[test]
fn detects_sources_case_insensitively() {
    assert!(is_ritobin_source("a/b/skin0.py"));
    assert!(is_ritobin_source("a/b/Skin0.RITOBIN"));
    assert!(!is_ritobin_source("a/b/skin0.bin"));
    assert!(!is_ritobin_source("a/b/noext"));
}

#[test]
fn rewrites_extension_to_bin() {
    assert_eq!(bin_rel_path("data/skin0.py"), "data/skin0.bin");
    assert_eq!(bin_rel_path("data/skin0.ritobin"), "data/skin0.bin");
    assert_eq!(bin_rel_path("data/skin0.bin"), "data/skin0.bin");
    assert_eq!(bin_rel_path("data/tex.dds"), "data/tex.dds");
}

#[test]
fn transforms_minimal_text_to_valid_bin() {
    let bytes = transform_text(MINIMAL).expect("minimal ritobin transforms");
    assert_eq!(&bytes[..4], b"PROP");
    Bin::from_reader(&mut Cursor::new(bytes)).expect("output is a readable bin");
}

#[test]
fn syntax_error_fails_the_transform() {
    let err = transform_text("test: u32 = 4!!2").unwrap_err();
    assert!(matches!(
        err,
        RitobinTransformError::Syntax(_) | RitobinTransformError::Invalid(_)
    ));
}

#[test]
fn missing_file_reports_read_error() {
    let err = transform_file(camino::Utf8Path::new("does/not/exist.py")).unwrap_err();
    assert!(matches!(err, RitobinTransformError::Read { .. }));
}