use super::*;
use camino::Utf8PathBuf;
use std::fs;
use tempfile::tempdir;

fn create_test_mod_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let mod_dir = dir.path();

    // Create mod.config.json
    let project = ltk_mod_project::ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        description: "A test mod".to_string(),
        authors: vec![],
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        transformers: vec![],
        layers: ltk_mod_project::ModProjectLayer::default_table(),
        thumbnail: None,
        hashtables: vec![],
    };
    fs::write(
        mod_dir.join("mod.config.json"),
        serde_json::to_string_pretty(&project).unwrap(),
    )
    .unwrap();

    // Create content/base/Test.wad.client/ with some files
    let wad_dir = mod_dir.join("content/base/Test.wad.client");
    fs::create_dir_all(&wad_dir).unwrap();
    fs::write(wad_dir.join("file1.bin"), b"data1").unwrap();

    let sub_dir = wad_dir.join("subdir");
    fs::create_dir_all(&sub_dir).unwrap();
    fs::write(sub_dir.join("file2.bin"), b"data2").unwrap();

    dir
}

#[test]
fn test_fs_mod_project() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let mut provider = FsModContent::new(mod_dir);

    let project = provider.mod_project().unwrap();
    assert_eq!(project.name, "test-mod");
    assert_eq!(project.display_name, "Test Mod");
}

#[test]
fn test_fs_list_layer_wads() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let mut provider = FsModContent::new(mod_dir);

    let wads = provider.list_layer_wads("base").unwrap();
    assert_eq!(wads.len(), 1);
    assert_eq!(wads[0], "Test.wad.client");
}

#[test]
fn test_fs_list_layer_wads_missing_layer() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let mut provider = FsModContent::new(mod_dir);

    let wads = provider.list_layer_wads("nonexistent").unwrap();
    assert!(wads.is_empty());
}

#[test]
fn test_fs_read_wad_overrides() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    let mut provider = FsModContent::new(mod_dir);

    let overrides = provider
        .read_wad_overrides("base", "Test.wad.client")
        .unwrap();
    assert_eq!(overrides.len(), 2);

    // Check that both files are present (order may vary)
    let paths: Vec<String> = overrides
        .iter()
        .map(|(p, _)| p.as_str().replace('\\', "/"))
        .collect();
    assert!(paths.contains(&"file1.bin".to_string()));
    assert!(paths.contains(&"subdir/file2.bin".to_string()));
}

#[test]
fn test_modignore_filters_wad_overrides() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    fs::write(
        mod_dir.join("content/base/Test.wad.client/source.psd"),
        b"working file",
    )
    .unwrap();
    fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();

    let mut provider = FsModContent::new(mod_dir);
    let overrides = provider
        .read_wad_overrides("base", "Test.wad.client")
        .unwrap();

    let paths: Vec<String> = overrides
        .iter()
        .map(|(p, _)| p.as_str().replace('\\', "/"))
        .collect();
    assert!(paths.contains(&"file1.bin".to_string()));
    assert!(paths.contains(&"subdir/file2.bin".to_string()));
    assert!(!paths.contains(&"source.psd".to_string()));
}

#[test]
fn test_modignore_hides_a_fully_ignored_wad() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    fs::write(mod_dir.join(".modignore"), "Test.wad.client/\n").unwrap();

    let mut provider = FsModContent::new(mod_dir);
    assert!(provider.list_layer_wads("base").unwrap().is_empty());
    assert!(
        provider
            .read_wad_overrides("base", "Test.wad.client")
            .unwrap()
            .is_empty()
    );
}

/// Editing an ignored file must not invalidate the rebuild cache: the
/// fingerprint uses the same filter as the read.
#[test]
fn test_fingerprint_is_stable_across_ignored_file_edits() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    let psd = mod_dir.join("content/base/Test.wad.client/source.psd");
    fs::write(&psd, b"v1").unwrap();
    fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();

    let provider = FsModContent::new(mod_dir);
    let before = provider.content_fingerprint().unwrap();

    // A different size, so this does not depend on mtime granularity.
    fs::write(&psd, b"version two, considerably larger").unwrap();
    let after = provider.content_fingerprint().unwrap();

    assert_eq!(before, after);
}

#[test]
fn test_fingerprint_changes_when_modignore_changes() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    // One provider per build: the filter is a per-provider snapshot.
    let before = FsModContent::new(mod_dir.clone())
        .content_fingerprint()
        .unwrap();

    fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();
    let after = FsModContent::new(mod_dir.clone())
        .content_fingerprint()
        .unwrap();

    assert_ne!(before, after);
}

#[test]
fn test_nested_modignore_filters_wad_overrides() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    fs::write(
        mod_dir.join("content/base/Test.wad.client/source.psd"),
        b"working file",
    )
    .unwrap();
    fs::write(
        mod_dir.join("content/base/Test.wad.client/.modignore"),
        "*.psd\n",
    )
    .unwrap();

    let mut provider = FsModContent::new(mod_dir);
    let overrides = provider
        .read_wad_overrides("base", "Test.wad.client")
        .unwrap();

    let paths: Vec<String> = overrides
        .iter()
        .map(|(p, _)| p.as_str().replace('\\', "/"))
        .collect();
    assert!(paths.contains(&"file1.bin".to_string()));
    assert!(!paths.contains(&"source.psd".to_string()));
    // The ignore file itself is filter metadata, not an override.
    assert!(!paths.contains(&".modignore".to_string()));
}

#[test]
fn test_fingerprint_changes_when_a_nested_modignore_appears() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    let before = FsModContent::new(mod_dir.clone())
        .content_fingerprint()
        .unwrap();

    // The walk never yields ignore files, so without explicit statting
    // this edit would be invisible to the cache.
    fs::write(
        mod_dir.join("content/base/Test.wad.client/.modignore"),
        "*.psd\n",
    )
    .unwrap();
    let after = FsModContent::new(mod_dir.clone())
        .content_fingerprint()
        .unwrap();

    assert_ne!(before, after);
}

fn write_raw_file(mod_dir: &Utf8Path, rel_path: &str, data: &[u8]) {
    let path = mod_dir.join("content/base/raw").join(rel_path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

/// `raw/` is Fantome-import compatibility, not a directory an authored
/// project gives up, so a provider that was not asked for it reads none.
#[test]
fn test_raw_overrides_are_off_by_default() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    write_raw_file(&mod_dir, "assets/maps/map11/scene.bin", b"scene");

    let mut provider = FsModContent::new(mod_dir);
    assert!(provider.read_raw_overrides().unwrap().is_empty());
}

#[test]
fn test_read_raw_overrides_from_the_base_layer() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    write_raw_file(&mod_dir, "assets/maps/map11/scene.bin", b"scene");
    write_raw_file(&mod_dir, "data/menu/main.bin", b"menu");

    let mut provider = FsModContent::new(mod_dir).with_raw_overrides();
    let overrides = provider.read_raw_overrides().unwrap();

    let mut paths: Vec<&str> = overrides.iter().map(|(p, _)| p.as_str()).collect();
    paths.sort_unstable();
    assert_eq!(paths, ["assets/maps/map11/scene.bin", "data/menu/main.bin"]);

    let bytes = provider
        .read_raw_override_file(Utf8Path::new("assets/maps/map11/scene.bin"))
        .unwrap();
    assert_eq!(bytes, b"scene");

    // The directory sits beside the layer's WAD targets without becoming one.
    assert_eq!(
        provider.list_layer_wads("base").unwrap(),
        ["Test.wad.client"]
    );
}

#[test]
fn test_read_raw_overrides_without_a_raw_dir() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

    let mut provider = FsModContent::new(mod_dir).with_raw_overrides();
    assert!(provider.read_raw_overrides().unwrap().is_empty());
}

#[test]
fn test_modignore_filters_raw_overrides() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    write_raw_file(&mod_dir, "assets/maps/map11/scene.bin", b"scene");
    write_raw_file(&mod_dir, "assets/maps/map11/scene.psd", b"working file");
    fs::write(mod_dir.join(".modignore"), "*.psd\n").unwrap();

    let mut provider = FsModContent::new(mod_dir).with_raw_overrides();
    let overrides = provider.read_raw_overrides().unwrap();

    let paths: Vec<&str> = overrides.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(paths, ["assets/maps/map11/scene.bin"]);
}

#[test]
fn test_fingerprint_changes_when_raw_content_changes() {
    let dir = create_test_mod_dir();
    let mod_dir = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    write_raw_file(&mod_dir, "assets/maps/map11/scene.bin", b"v1");

    let provider = FsModContent::new(mod_dir.clone()).with_raw_overrides();
    let before = provider.content_fingerprint().unwrap();

    // A different size, so this does not depend on mtime granularity.
    write_raw_file(
        &mod_dir,
        "assets/maps/map11/scene.bin",
        b"version two, considerably larger",
    );
    let after = provider.content_fingerprint().unwrap();

    assert_ne!(before, after);
}
