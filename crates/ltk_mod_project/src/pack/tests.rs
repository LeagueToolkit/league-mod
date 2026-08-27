//! Driver tests, using a stub format so they run without the `modpkg` or
//! `fantome` features. Format-specific behavior is tested in those modules.

use std::convert::Infallible;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use super::{
    IgnoreMode, PackError, PackFormat, PackOptions, PackPlan, PackReport, PackReporter,
    ProjectPacker,
};
use crate::{ModIgnore, ModProject, ModProjectLayer, PackProgress, PackStage};

/// `(wad, rel_path, source)` for one planned file.
type CapturedFile = (Option<String>, String, Utf8PathBuf);

/// Everything a plan exposes, copied out for assertions.
#[derive(Debug, Default)]
struct Captured {
    /// Per layer: name, then the captured files.
    layers: Vec<(String, Vec<CapturedFile>)>,
    readme: Option<Utf8PathBuf>,
    license: Option<(Utf8PathBuf, &'static str)>,
    thumbnail: Option<Utf8PathBuf>,
}

struct Capture<'a>(&'a mut Captured);

impl PackFormat for Capture<'_> {
    type Error = Infallible;

    fn pack(
        self,
        plan: &PackPlan<'_>,
        _progress: &mut PackReporter<'_>,
    ) -> Result<(), Self::Error> {
        self.0.layers = plan
            .layers()
            .iter()
            .map(|layer| {
                (
                    layer.layer().name.clone(),
                    layer
                        .files()
                        .iter()
                        .map(|file| {
                            (
                                file.wad().map(str::to_owned),
                                file.rel_path().to_owned(),
                                file.source().to_owned(),
                            )
                        })
                        .collect(),
                )
            })
            .collect();
        self.0.readme = plan.readme().map(Utf8Path::to_owned);
        self.0.license = plan
            .license()
            .map(|license| (license.source().to_owned(), license.canonical_name()));
        self.0.thumbnail = plan.thumbnail().map(Utf8Path::to_owned);
        Ok(())
    }
}

fn try_capture(
    project: ModProject,
    root: &Utf8Path,
    options: PackOptions,
) -> Result<(Captured, PackReport), PackError<Infallible>> {
    let mut captured = Captured::default();
    let report = ProjectPacker::new(project, root.to_owned())
        .with_options(options)
        .pack(Capture(&mut captured))?;
    Ok((captured, report))
}

fn capture(project: ModProject, root: &Utf8Path) -> (Captured, PackReport) {
    try_capture(project, root, PackOptions::default()).unwrap()
}

fn test_mod_project(layers: Vec<ModProjectLayer>) -> ModProject {
    ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        layers,
        ..Default::default()
    }
}

fn utf8_tempdir(tmp: &tempfile::TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap()
}

/// Create a file inside `content/{layer}/{rel_path}`, creating directories
/// as needed.
fn create_content_file(root: &Utf8Path, layer: &str, rel_path: &str, data: &[u8]) {
    let full_path = root.join("content").join(layer).join(rel_path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&full_path, data).unwrap();
}

// -- plan contents ----------------------------------------------------------

#[test]
fn plan_carries_wad_loose_and_plain_directory_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Graves.wad.client/data/skin0.bin", b"s");
    create_content_file(&root, "base", "loose.bin", b"l");
    create_content_file(&root, "base", "some_dir/file.bin", b"d");

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(captured.layers.len(), 1);
    let (layer_name, mut files) = captured.layers.into_iter().next().unwrap();
    assert_eq!(layer_name, "base");

    files.sort_by(|a, b| a.1.cmp(&b.1));
    let summary: Vec<(Option<&str>, &str)> = files
        .iter()
        .map(|(wad, rel, _)| (wad.as_deref(), rel.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            // WAD content is relative to the WAD dir, with the WAD carried
            // separately; everything else is relative to the layer dir.
            (Some("Graves.wad.client"), "data/skin0.bin"),
            (None, "loose.bin"),
            (None, "some_dir/file.bin"),
        ]
    );

    assert_eq!(report.ignored_count(), 0);
}

#[test]
fn plan_synthesizes_the_base_layer_when_unconfigured() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let (captured, _) = capture(test_mod_project(vec![]), &root);

    assert_eq!(captured.layers.len(), 1);
    assert_eq!(captured.layers[0].0, "base");
}

#[test]
fn plan_orders_layers_base_first() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    create_content_file(&root, "high-res", "X.wad.client/g.bin", b"y");

    let project = test_mod_project(vec![
        ModProjectLayer {
            name: "high-res".to_string(),
            priority: 1,
            ..Default::default()
        },
        ModProjectLayer::base(),
    ]);

    let (captured, _) = capture(project, &root);

    let names: Vec<&str> = captured.layers.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["base", "high-res"]);
}

#[test]
fn wad_directories_are_detected_case_insensitively() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "Upper.WAD.Client/data.bin", b"d");

    let (captured, _) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let (wad, rel, _) = &captured.layers[0].1[0];
    // Detection folds case; the author's spelling is preserved.
    assert_eq!(wad.as_deref(), Some("Upper.WAD.Client"));
    assert_eq!(rel, "data.bin");
}

#[test]
fn modignore_files_are_never_planned() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "loose.bin", b"b");
    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join("content/base/.modignore"), "# nothing\n").unwrap();
    fs::write(root.join("content/base/X.wad.client/.modignore"), "\n").unwrap();

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let all_rel: Vec<&str> = captured.layers[0]
        .1
        .iter()
        .map(|(_, rel, _)| rel.as_str())
        .collect();
    assert!(!all_rel.iter().any(|rel| rel.contains(".modignore")));
    assert_eq!(all_rel.len(), 2);
    assert_eq!(report.ignored_count(), 0);
}

// -- metadata discovery -----------------------------------------------------

#[test]
fn plan_resolves_readme_license_and_thumbnail() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join("README.md"), "readme").unwrap();
    fs::write(root.join("license.txt"), "terms").unwrap();
    fs::write(root.join("thumbnail.webp"), "not really an image").unwrap();

    let (captured, _) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(captured.readme, Some(root.join("README.md")));
    // The on-disk casing is kept in the path; the canonical name is fixed.
    assert_eq!(
        captured.license,
        Some((root.join("license.txt"), "LICENSE.txt"))
    );
    // An unconfigured thumbnail falls back to thumbnail.webp for every
    // format, so what a GUI previews is what gets packed.
    assert_eq!(captured.thumbnail, Some(root.join("thumbnail.webp")));
}

#[test]
fn plan_omits_absent_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let (captured, _) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(captured.readme, None);
    assert_eq!(captured.license, None);
    assert_eq!(captured.thumbnail, None);
}

#[test]
fn plan_prefers_the_configured_thumbnail_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join("cover.png"), "img").unwrap();
    fs::write(root.join("thumbnail.webp"), "img").unwrap();

    let project = ModProject {
        thumbnail: Some("cover.png".to_string()),
        ..test_mod_project(vec![ModProjectLayer::base()])
    };

    let (captured, _) = capture(project, &root);

    assert_eq!(captured.thumbnail, Some(root.join("cover.png")));
}

// -- validation and errors --------------------------------------------------

#[test]
fn from_dir_loads_the_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    let config = r#"{
        "name": "auto-load-test",
        "display_name": "Auto Load Test",
        "version": "1.0.0",
        "description": "",
        "authors": [],
        "layers": [{"name": "base", "priority": 0}]
    }"#;
    fs::write(root.join("mod.config.json"), config).unwrap();

    let packer = ProjectPacker::from_dir(root.clone()).unwrap();
    assert_eq!(packer.project().name, "auto-load-test");
    assert_eq!(packer.project_root(), root);
}

#[test]
fn from_dir_reports_a_missing_config() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    let err = ProjectPacker::from_dir(root).unwrap_err();
    assert!(
        matches!(err, crate::ModProjectError::ConfigNotFound(_)),
        "expected ConfigNotFound, got: {err}"
    );
}

#[test]
fn missing_layer_directory_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    // "high-res" layer declared but directory not created

    let project = test_mod_project(vec![
        ModProjectLayer::base(),
        ModProjectLayer {
            name: "high-res".to_string(),
            priority: 1,
            ..Default::default()
        },
    ]);

    let err = try_capture(project, &root, PackOptions::default()).unwrap_err();
    assert!(
        matches!(err, PackError::LayerDirMissing { ref layer, .. } if layer == "high-res"),
        "expected LayerDirMissing for high-res, got: {err}"
    );
}

#[test]
fn missing_base_directory_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    let err = try_capture(test_mod_project(vec![]), &root, PackOptions::default()).unwrap_err();
    match err {
        PackError::LayerDirMissing { layer, path } => {
            assert_eq!(layer, "base");
            assert_eq!(path, root.join("content").join("base"));
        }
        other => panic!("expected LayerDirMissing for base, got: {other}"),
    }
}

#[test]
fn wrong_base_priority_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let project = test_mod_project(vec![ModProjectLayer {
        name: "base".to_string(),
        priority: 5,
        ..Default::default()
    }]);

    let err = try_capture(project, &root, PackOptions::default()).unwrap_err();
    assert!(
        matches!(err, PackError::InvalidBaseLayerPriority(5)),
        "expected InvalidBaseLayerPriority(5), got: {err}"
    );
}

#[test]
fn format_errors_pass_through_transparently() {
    struct Failing;

    impl PackFormat for Failing {
        type Error = std::io::Error;

        fn pack(
            self,
            _plan: &PackPlan<'_>,
            _progress: &mut PackReporter<'_>,
        ) -> Result<(), Self::Error> {
            Err(std::io::Error::other("boom"))
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let err = ProjectPacker::new(test_mod_project(vec![]), root)
        .pack(Failing)
        .unwrap_err();

    assert!(matches!(err, PackError::Format(_)));
    // Transparent: the wrapper adds no message of its own.
    assert_eq!(err.to_string(), "boom");
}

// -- ignore handling --------------------------------------------------------

#[test]
fn modignore_excludes_files_and_reports_them() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let rels: Vec<&str> = captured.layers[0]
        .1
        .iter()
        .map(|(_, rel, _)| rel.as_str())
        .collect();
    assert_eq!(rels, ["tex.dds"]);

    assert_eq!(
        report.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("src.psd")]
    );
    assert_eq!(report.ignored_count(), 1);
}

#[test]
fn modignore_directory_pattern_prunes_a_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/data/ok.bin", b"ok");
    create_content_file(&root, "base", "X.wad.client/scratch/a.bin", b"a");
    create_content_file(&root, "base", "X.wad.client/scratch/deep/b.bin", b"b");
    fs::write(root.join(".modignore"), "scratch/\n").unwrap();

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let rels: Vec<&str> = captured.layers[0]
        .1
        .iter()
        .map(|(_, rel, _)| rel.as_str())
        .collect();
    assert_eq!(rels, ["data/ok.bin"]);

    // The directory is recorded once; its files are not enumerated.
    assert_eq!(
        report.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("scratch")]
    );
}

#[test]
fn modignore_negation_reincludes_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/drop.psd", b"d");
    create_content_file(&root, "base", "X.wad.client/keep.psd", b"k");
    create_content_file(&root, "base", "X.wad.client/tex.dds", b"t");
    fs::write(root.join(".modignore"), "*.psd\n!keep.psd\n").unwrap();

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let mut rels: Vec<&str> = captured.layers[0]
        .1
        .iter()
        .map(|(_, rel, _)| rel.as_str())
        .collect();
    rels.sort_unstable();
    assert_eq!(rels, ["keep.psd", "tex.dds"]);

    assert_eq!(
        report.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("drop.psd")]
    );
}

#[test]
fn modignore_filters_layer_root_files_and_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "loose.bin", b"b");
    create_content_file(&root, "base", "loose.psd", b"p");
    create_content_file(&root, "base", "notes/todo.txt", b"t");
    fs::write(root.join(".modignore"), "*.psd\nnotes/\n").unwrap();

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let rels: Vec<&str> = captured.layers[0]
        .1
        .iter()
        .map(|(_, rel, _)| rel.as_str())
        .collect();
    assert_eq!(rels, ["loose.bin"]);
    assert_eq!(report.ignored_count(), 2);
}

#[test]
fn absent_modignore_packs_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    assert_eq!(captured.layers[0].1.len(), 2);
    assert_eq!(report.ignored_count(), 0);
}

/// A directory named like a glob pattern used to be a pack error
/// (`InvalidGlobPattern`); the walker has no pattern to reject.
#[test]
fn glob_special_directory_names_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "[base]/file.bin", b"data");

    let (captured, _) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let (wad, rel, _) = &captured.layers[0].1[0];
    assert_eq!(wad, &None);
    assert_eq!(rel, "[base]/file.bin");
}

#[test]
fn nested_modignore_filters_its_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    create_content_file(&root, "base", "Y.wad.client/src.psd", b"psd");
    fs::write(root.join("content/base/X.wad.client/.modignore"), "*.psd\n").unwrap();

    let (captured, report) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    // The nested filter governs only its own WAD directory.
    let mut entries: Vec<(&str, &str)> = captured.layers[0]
        .1
        .iter()
        .map(|(wad, rel, _)| (wad.as_deref().unwrap(), rel.as_str()))
        .collect();
    entries.sort_unstable();
    assert_eq!(
        entries,
        [("X.wad.client", "tex.dds"), ("Y.wad.client", "src.psd")]
    );

    assert_eq!(
        report.ignored_files(),
        [root
            .join("content")
            .join("base")
            .join("X.wad.client")
            .join("src.psd")]
    );
}

#[test]
fn modignore_matching_is_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/Thumbs.db", b"junk");
    create_content_file(&root, "base", "X.wad.client/tex.dds", b"dds");
    fs::write(root.join(".modignore"), "thumbs.db\n").unwrap();

    let (captured, _) = capture(test_mod_project(vec![ModProjectLayer::base()]), &root);

    let rels: Vec<&str> = captured.layers[0]
        .1
        .iter()
        .map(|(_, rel, _)| rel.as_str())
        .collect();
    assert_eq!(rels, ["tex.dds"]);
}

#[test]
fn ignore_disabled_packs_everything() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let (captured, report) = try_capture(
        test_mod_project(vec![ModProjectLayer::base()]),
        &root,
        PackOptions::default().with_ignore(IgnoreMode::Disabled),
    )
    .unwrap();

    assert_eq!(captured.layers[0].1.len(), 1);
    assert_eq!(report.ignored_count(), 0);
}

#[test]
fn explicit_ignore_is_applied() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/src.psd", b"psd");
    // The project's own file would drop the .psd; the explicit empty filter
    // replaces it.
    fs::write(root.join(".modignore"), "*.psd\n").unwrap();

    let (captured, _) = try_capture(
        test_mod_project(vec![ModProjectLayer::base()]),
        &root,
        PackOptions::default().with_ignore(IgnoreMode::Explicit(ModIgnore::empty(&root))),
    )
    .unwrap();

    assert_eq!(captured.layers[0].1.len(), 1);
}

#[test]
fn explicit_ignore_with_wrong_root_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");

    let elsewhere = root.join("elsewhere");
    let err = try_capture(
        test_mod_project(vec![ModProjectLayer::base()]),
        &root,
        PackOptions::default().with_ignore(IgnoreMode::Explicit(ModIgnore::empty(&elsewhere))),
    )
    .unwrap_err();

    assert!(
        matches!(err, PackError::IgnoreRootMismatch { .. }),
        "expected IgnoreRootMismatch, got: {err}"
    );
}

#[test]
fn invalid_modignore_pattern_fails_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);

    create_content_file(&root, "base", "X.wad.client/f.bin", b"x");
    fs::write(root.join(".modignore"), "a{b\n").unwrap();

    let err = try_capture(
        test_mod_project(vec![ModProjectLayer::base()]),
        &root,
        PackOptions::default(),
    )
    .unwrap_err();

    assert!(
        matches!(err, PackError::Ignore(_)),
        "expected Ignore, got: {err}"
    );
}

// -- progress ---------------------------------------------------------------

/// A format that writes nothing and reports every file the plan holds.
struct ReportEverything;

impl PackFormat for ReportEverything {
    type Error = Infallible;

    fn pack(self, plan: &PackPlan<'_>, progress: &mut PackReporter<'_>) -> Result<(), Self::Error> {
        for layer in plan.layers() {
            for file in layer.files() {
                progress.report_file(file.rel_path());
            }
        }
        Ok(())
    }
}

/// The scan counts layers and the write counts files, because how many files
/// there are is not known until the scan has finished.
#[test]
fn pack_reports_a_layer_per_scan_then_a_file_per_write_then_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "X.wad.client/a.bin", b"a");
    create_content_file(&root, "extra", "X.wad.client/b.bin", b"b");

    let project = test_mod_project(vec![
        ModProjectLayer::base(),
        ModProjectLayer {
            name: "extra".to_string(),
            priority: 1,
            ..Default::default()
        },
    ]);

    let mut reported = Vec::new();
    ProjectPacker::new(project, root)
        .pack_with_progress(ReportEverything, &mut |progress: PackProgress<'_>| {
            reported.push((
                progress.stage,
                progress.current_item.map(str::to_owned),
                progress.current,
                progress.total,
            ));
        })
        .unwrap();

    assert_eq!(
        reported,
        [
            (PackStage::Scanning, Some("base".to_owned()), 0, 2),
            (PackStage::Scanning, Some("extra".to_owned()), 1, 2),
            (PackStage::Writing, Some("a.bin".to_owned()), 0, 2),
            (PackStage::Writing, Some("b.bin".to_owned()), 1, 2),
            (PackStage::Complete, None, 2, 2),
        ]
    );
}

/// A format reporting nothing still completes, and the counters say how many
/// files it was given.
#[test]
fn pack_completes_even_when_the_format_reports_nothing() {
    struct Silent;

    impl PackFormat for Silent {
        type Error = Infallible;

        fn pack(
            self,
            _plan: &PackPlan<'_>,
            _progress: &mut PackReporter<'_>,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "X.wad.client/a.bin", b"a");

    let mut stages = Vec::new();
    ProjectPacker::new(test_mod_project(vec![]), root)
        .pack_with_progress(Silent, &mut |progress: PackProgress<'_>| {
            stages.push(progress.stage);
        })
        .unwrap();

    assert_eq!(stages, [PackStage::Scanning, PackStage::Complete]);
}

/// A pack with no callback is the same pack.
#[test]
fn packing_without_a_callback_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = utf8_tempdir(&tmp);
    create_content_file(&root, "base", "X.wad.client/a.bin", b"a");

    ProjectPacker::new(test_mod_project(vec![]), root)
        .pack(ReportEverything)
        .unwrap();
}
