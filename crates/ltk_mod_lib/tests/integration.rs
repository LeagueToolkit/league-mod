use std::collections::HashMap;
use std::fs;
use std::io::Write;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_lib::{LibraryError, LibraryIndex, NoOpReporter};

fn temp_storage() -> (tempfile::TempDir, Utf8PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
    (dir, path)
}

/// Create a minimal .fantome archive (ZIP with META/info.json).
fn create_test_fantome(dir: &Utf8Path, name: &str) -> Utf8PathBuf {
    let file_path = dir.join(format!("{}.fantome", name));
    let file = fs::File::create(file_path.as_std_path()).unwrap();
    let mut zip = zip::ZipWriter::new(file);

    let info = serde_json::json!({
        "Name": name,
        "Author": "Test Author",
        "Version": "1.0.0",
        "Description": format!("A test mod called {}", name),
    });

    zip.start_file("META/info.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info.to_string().as_bytes()).unwrap();
    zip.finish().unwrap();

    file_path
}

// ---------------------------------------------------------------------------
// Install flow
// ---------------------------------------------------------------------------

#[test]
fn install_single_mod() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let fantome = create_test_fantome(&src_path, "test-skin");
    let mut index = LibraryIndex::load(&storage).unwrap();

    let installed = index.install_mod(&storage, &fantome).unwrap();
    assert_eq!(installed.display_name, "test-skin");
    assert_eq!(installed.version, "1.0.0");
    assert_eq!(installed.authors, vec!["Test Author"]);
    assert!(installed.enabled);

    assert_eq!(index.mods.len(), 1);
    let profile = index.active_profile().unwrap();
    assert_eq!(profile.enabled_mods.len(), 1);
    assert_eq!(profile.mod_order.len(), 1);
}

#[test]
fn install_mod_copies_archive() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let fantome = create_test_fantome(&src_path, "copy-test");
    let mut index = LibraryIndex::load(&storage).unwrap();

    let installed = index.install_mod(&storage, &fantome).unwrap();
    let archive = storage
        .join("archives")
        .join(format!("{}.fantome", installed.id));
    assert!(archive.exists());
}

#[test]
fn install_mod_extracts_metadata() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let fantome = create_test_fantome(&src_path, "meta-test");
    let mut index = LibraryIndex::load(&storage).unwrap();

    let installed = index.install_mod(&storage, &fantome).unwrap();
    let config = storage
        .join("mods")
        .join(&installed.id)
        .join("mod.config.json");
    assert!(config.exists());

    let contents = fs::read_to_string(config.as_std_path()).unwrap();
    let project: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(project["display_name"], "meta-test");
    assert_eq!(project["version"], "1.0.0");
}

#[test]
fn install_mod_creates_base_layer() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let fantome = create_test_fantome(&src_path, "layer-test");
    let mut index = LibraryIndex::load(&storage).unwrap();

    let installed = index.install_mod(&storage, &fantome).unwrap();
    assert_eq!(installed.layers.len(), 1);
    assert_eq!(installed.layers[0].name, "base");
    assert!(installed.layers[0].enabled);
}

#[test]
fn install_mod_auto_enables_at_top() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mod1 = create_test_fantome(&src_path, "mod1");
    let mod2 = create_test_fantome(&src_path, "mod2");
    let mut index = LibraryIndex::load(&storage).unwrap();

    let first = index.install_mod(&storage, &mod1).unwrap();
    let second = index.install_mod(&storage, &mod2).unwrap();

    let profile = index.active_profile().unwrap();
    // Second mod should be at position 0 (highest priority)
    assert_eq!(profile.enabled_mods[0], second.id);
    assert_eq!(profile.enabled_mods[1], first.id);
    assert_eq!(profile.mod_order[0], second.id);
    assert_eq!(profile.mod_order[1], first.id);
}

#[test]
fn install_mod_nonexistent_file_fails() {
    let (_dir, storage) = temp_storage();
    let mut index = LibraryIndex::load(&storage).unwrap();

    let result = index.install_mod(&storage, Utf8Path::new("/nonexistent/mod.fantome"));
    assert!(matches!(result.unwrap_err(), LibraryError::InvalidPath(_)));
}

// ---------------------------------------------------------------------------
// Batch install
// ---------------------------------------------------------------------------

#[test]
fn batch_install_mods() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mod1 = create_test_fantome(&src_path, "batch1");
    let mod2 = create_test_fantome(&src_path, "batch2");
    let mod3 = create_test_fantome(&src_path, "batch3");

    let mut index = LibraryIndex::load(&storage).unwrap();
    let result = index
        .install_mods_batch(&storage, &[mod1, mod2, mod3], &NoOpReporter)
        .unwrap();

    assert_eq!(result.installed.len(), 3);
    assert!(result.failed.is_empty());
    assert_eq!(index.mods.len(), 3);
}

#[test]
fn batch_install_partial_failure() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let good = create_test_fantome(&src_path, "good");
    let bad = Utf8PathBuf::from("/nonexistent/bad.fantome");
    let good2 = create_test_fantome(&src_path, "good2");

    let mut index = LibraryIndex::load(&storage).unwrap();
    let result = index
        .install_mods_batch(&storage, &[good, bad, good2], &NoOpReporter)
        .unwrap();

    assert_eq!(result.installed.len(), 2);
    assert_eq!(result.failed.len(), 1);
    assert!(result.failed[0].file_path.contains("bad.fantome"));
}

#[test]
fn batch_install_empty() {
    let (_dir, storage) = temp_storage();
    let mut index = LibraryIndex::load(&storage).unwrap();
    let result = index
        .install_mods_batch(&storage, &[], &NoOpReporter)
        .unwrap();
    assert!(result.installed.is_empty());
    assert!(result.failed.is_empty());
}

// ---------------------------------------------------------------------------
// Query installed mods
// ---------------------------------------------------------------------------

#[test]
fn get_installed_mods_returns_all() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let mod1 = create_test_fantome(&src_path, "query1");
    let mod2 = create_test_fantome(&src_path, "query2");
    index.install_mod(&storage, &mod1).unwrap();
    index.install_mod(&storage, &mod2).unwrap();

    let mods = index.get_installed_mods(&storage).unwrap();
    assert_eq!(mods.len(), 2);
}

#[test]
fn get_installed_mods_respects_mod_order() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let m1 = create_test_fantome(&src_path, "first");
    let m2 = create_test_fantome(&src_path, "second");
    let i1 = index.install_mod(&storage, &m1).unwrap();
    let i2 = index.install_mod(&storage, &m2).unwrap();

    // mod_order is [i2, i1] because each install prepends
    let mods = index.get_installed_mods(&storage).unwrap();
    assert_eq!(mods[0].id, i2.id);
    assert_eq!(mods[1].id, i1.id);
}

#[test]
fn get_installed_mods_shows_enabled_status() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let m1 = create_test_fantome(&src_path, "enabled");
    let m2 = create_test_fantome(&src_path, "disabled");
    let i1 = index.install_mod(&storage, &m1).unwrap();
    let _i2 = index.install_mod(&storage, &m2).unwrap();

    // Disable the first mod
    index.toggle_mod(&i1.id, false).unwrap();

    let mods = index.get_installed_mods(&storage).unwrap();
    let enabled_mod = mods.iter().find(|m| m.display_name == "disabled").unwrap();
    let disabled_mod = mods.iter().find(|m| m.display_name == "enabled").unwrap();
    assert!(enabled_mod.enabled);
    assert!(!disabled_mod.enabled);
}

#[test]
fn get_installed_mods_layer_states() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let m1 = create_test_fantome(&src_path, "layers");
    let i1 = index.install_mod(&storage, &m1).unwrap();

    // Default: all layers enabled
    let mods = index.get_installed_mods(&storage).unwrap();
    assert!(mods[0].layers[0].enabled);

    // Set base layer to disabled
    let mut states = HashMap::new();
    states.insert("base".to_string(), false);
    index.set_layer_states(&i1.id, states).unwrap();

    let mods = index.get_installed_mods(&storage).unwrap();
    assert!(!mods[0].layers[0].enabled);
}

#[test]
fn get_installed_mods_filters_empty_description() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    // Create a fantome with empty description
    let file_path = src_path.join("empty-desc.fantome");
    let file = fs::File::create(file_path.as_std_path()).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let info = serde_json::json!({
        "Name": "empty-desc",
        "Author": "Test",
        "Version": "1.0.0",
        "Description": "",
    });
    zip.start_file("META/info.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info.to_string().as_bytes()).unwrap();
    zip.finish().unwrap();

    let mut index = LibraryIndex::load(&storage).unwrap();
    index.install_mod(&storage, &file_path).unwrap();

    let mods = index.get_installed_mods(&storage).unwrap();
    assert!(mods[0].description.is_none());
}

// ---------------------------------------------------------------------------
// Uninstall flow
// ---------------------------------------------------------------------------

#[test]
fn uninstall_mod_full_flow() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let m1 = create_test_fantome(&src_path, "uninstall-test");
    let installed = index.install_mod(&storage, &m1).unwrap();

    let mod_dir = storage.join("mods").join(&installed.id);
    let archive = storage
        .join("archives")
        .join(format!("{}.fantome", installed.id));
    assert!(mod_dir.exists());
    assert!(archive.exists());

    index.uninstall_mod(&storage, &installed.id).unwrap();

    assert!(index.mods.is_empty());
    assert!(!mod_dir.exists());
    assert!(!archive.exists());

    let profile = index.active_profile().unwrap();
    assert!(profile.enabled_mods.is_empty());
    assert!(profile.mod_order.is_empty());
}

// ---------------------------------------------------------------------------
// Full workflow: install → toggle → reorder → switch profile → uninstall
// ---------------------------------------------------------------------------

#[test]
fn full_workflow() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();

    // Install 3 mods
    let m1 = create_test_fantome(&src_path, "skin-a");
    let m2 = create_test_fantome(&src_path, "skin-b");
    let m3 = create_test_fantome(&src_path, "skin-c");
    let i1 = index.install_mod(&storage, &m1).unwrap();
    let i2 = index.install_mod(&storage, &m2).unwrap();
    let i3 = index.install_mod(&storage, &m3).unwrap();

    assert_eq!(index.mods.len(), 3);

    // All enabled by default
    let profile = index.active_profile().unwrap();
    assert_eq!(profile.enabled_mods.len(), 3);

    // Disable one mod
    index.toggle_mod(&i2.id, false).unwrap();
    let profile = index.active_profile().unwrap();
    assert_eq!(profile.enabled_mods.len(), 2);
    assert!(!profile.enabled_mods.contains(&i2.id));

    // Reorder: reverse the order
    index
        .reorder_mods(vec![i1.id.clone(), i2.id.clone(), i3.id.clone()])
        .unwrap();
    let profile = index.active_profile().unwrap();
    assert_eq!(
        profile.mod_order,
        vec![i1.id.clone(), i2.id.clone(), i3.id.clone()]
    );
    // enabled_mods should still exclude i2 but now in order [i1, i3]
    assert_eq!(profile.enabled_mods, vec![i1.id.clone(), i3.id.clone()]);

    // Re-enable i2
    index.toggle_mod(&i2.id, true).unwrap();
    let profile = index.active_profile().unwrap();
    assert_eq!(
        profile.enabled_mods,
        vec![i1.id.clone(), i2.id.clone(), i3.id.clone()]
    );

    // Create and switch to new profile
    let ranked = index
        .create_profile(&storage, "Ranked".to_string())
        .unwrap();
    assert!(ranked.enabled_mods.is_empty());
    assert_eq!(ranked.mod_order.len(), 3);

    index.switch_profile(&ranked.id).unwrap();
    assert_eq!(index.active_profile_id, ranked.id);

    // Enable only one mod in ranked profile
    index.toggle_mod(&i1.id, true).unwrap();
    let profile = index.active_profile().unwrap();
    assert_eq!(profile.enabled_mods, vec![i1.id.clone()]);

    // Switch back and verify default profile unchanged
    let default_id = index.profiles[0].id.clone();
    index.switch_profile(&default_id).unwrap();
    let profile = index.active_profile().unwrap();
    assert_eq!(profile.enabled_mods.len(), 3);

    // Uninstall a mod — should affect all profiles
    index.uninstall_mod(&storage, &i1.id).unwrap();
    assert_eq!(index.mods.len(), 2);

    // Check both profiles were updated
    for profile in &index.profiles {
        assert!(!profile.enabled_mods.contains(&i1.id));
        assert!(!profile.mod_order.contains(&i1.id));
    }

    // Save and reload — verify persistence
    index.save(&storage).unwrap();
    let reloaded = LibraryIndex::load(&storage).unwrap();
    assert_eq!(reloaded.mods.len(), 2);
    assert_eq!(reloaded.profiles.len(), 2);
}

// ---------------------------------------------------------------------------
// Persistence round-trip with complex state
// ---------------------------------------------------------------------------

#[test]
fn persistence_preserves_profile_state() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let m1 = create_test_fantome(&src_path, "persist-test");
    let installed = index.install_mod(&storage, &m1).unwrap();

    // Set up complex state
    let mut states = HashMap::new();
    states.insert("base".to_string(), false);
    index
        .set_layer_states(&installed.id, states.clone())
        .unwrap();
    index
        .create_profile(&storage, "Ranked".to_string())
        .unwrap();

    index.save(&storage).unwrap();
    let loaded = LibraryIndex::load(&storage).unwrap();

    assert_eq!(loaded.profiles.len(), 2);
    let default_profile = loaded.active_profile().unwrap();
    let layer_states = default_profile.layer_states.get(&installed.id).unwrap();
    assert_eq!(layer_states.get("base"), Some(&false));
}

// ---------------------------------------------------------------------------
// Fantome edge cases
// ---------------------------------------------------------------------------

#[test]
fn install_fantome_with_minimal_info() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    // Create fantome with minimal fields
    let file_path = src_path.join("minimal.fantome");
    let file = fs::File::create(file_path.as_std_path()).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let info = serde_json::json!({});
    zip.start_file("META/info.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(info.to_string().as_bytes()).unwrap();
    zip.finish().unwrap();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let installed = index.install_mod(&storage, &file_path).unwrap();

    assert_eq!(installed.display_name, "unknown");
    assert_eq!(installed.version, "1.0.0");
    assert_eq!(installed.authors, vec!["Unknown"]);
}

#[test]
fn install_fantome_without_meta_fails() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    // Create an empty ZIP with no META/info.json
    let file_path = src_path.join("empty.fantome");
    let file = fs::File::create(file_path.as_std_path()).unwrap();
    let zip = zip::ZipWriter::new(file);
    zip.finish().unwrap();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let result = index.install_mod(&storage, &file_path);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Profile isolation
// ---------------------------------------------------------------------------

#[test]
fn profile_changes_are_isolated() {
    let (_dir, storage) = temp_storage();
    let (_src_dir, src_path) = temp_storage();

    let mut index = LibraryIndex::load(&storage).unwrap();
    let m1 = create_test_fantome(&src_path, "isolation-test");
    let installed = index.install_mod(&storage, &m1).unwrap();

    // Create second profile and switch
    let ranked = index
        .create_profile(&storage, "Ranked".to_string())
        .unwrap();
    index.switch_profile(&ranked.id).unwrap();

    // Enable mod in ranked
    index.toggle_mod(&installed.id, true).unwrap();

    // Set layer states in ranked
    let mut states = HashMap::new();
    states.insert("base".to_string(), false);
    index
        .set_layer_states(&installed.id, states.clone())
        .unwrap();

    // Switch back to default
    let default_id = index.profiles[0].id.clone();
    index.switch_profile(&default_id).unwrap();

    // Default profile layer states should be unaffected
    let profile = index.active_profile().unwrap();
    assert!(
        !profile.layer_states.contains_key(&installed.id)
            || !profile.layer_states[&installed.id].contains_key("base")
            || profile.layer_states[&installed.id]["base"]
    );
}

// ---------------------------------------------------------------------------
// Storage lock
// ---------------------------------------------------------------------------

#[test]
fn storage_lock_basic() {
    let (_dir, storage) = temp_storage();
    let lock = ltk_mod_lib::StorageLock::acquire(&storage).unwrap();
    assert!(storage.join("library.lock").exists());
    drop(lock);
}
