use std::convert::Infallible;
use std::sync::atomic::AtomicBool;

use camino::{Utf8Path, Utf8PathBuf};

use super::*;
use crate::{Cancellation, ConfigRefusal, ModProject, ProjectImporter};

fn utf8_dir(dir: &tempfile::TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap()
}

fn test_project() -> ModProject {
    ModProject {
        name: "test-mod".to_string(),
        display_name: "Test Mod".to_string(),
        version: "1.0.0".to_string(),
        layers: crate::ModProjectLayer::default_table(),
        ..Default::default()
    }
}

/// A format that decodes nothing and records the directories it was handed.
struct Recorder<'a> {
    saw_base_layer_dir: &'a mut bool,
}

impl ImportFormat for Recorder<'_> {
    type Error = Infallible;

    fn import(
        self,
        target: &ImportTarget<'_>,
        progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error> {
        *self.saw_base_layer_dir = target.base_layer_dir().is_dir();
        progress.set_total(1);
        progress.report_item("only");
        Ok(test_project())
    }
}

/// A format that decodes nothing and declares a layer it wrote no content for,
/// as an archive naming a layer it holds no files for does.
struct DeclaresAnEmptyLayer;

impl ImportFormat for DeclaresAnEmptyLayer {
    type Error = Infallible;

    fn import(
        self,
        _target: &ImportTarget<'_>,
        _progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error> {
        let mut project = test_project();
        project.layers.push(crate::ModProjectLayer {
            name: "skins".to_string(),
            priority: 10,
            ..Default::default()
        });
        Ok(project)
    }
}

/// A format that fails, optionally calling its failure a cancellation.
struct Failing {
    cancelled: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("the format gave up")]
struct FailingError {
    cancelled: bool,
}

impl ImportFormat for Failing {
    type Error = FailingError;

    fn import(
        self,
        _target: &ImportTarget<'_>,
        _progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error> {
        Err(FailingError {
            cancelled: self.cancelled,
        })
    }

    fn is_cancellation(error: &Self::Error) -> bool {
        error.cancelled
    }
}

/// A format that only reports what the target says about the cancellation.
struct Watching<'a> {
    saw_cancelled: &'a mut bool,
}

impl ImportFormat for Watching<'_> {
    type Error = Infallible;

    fn import(
        self,
        target: &ImportTarget<'_>,
        _progress: &mut ImportReporter<'_>,
    ) -> Result<ModProject, Self::Error> {
        *self.saw_cancelled = target.is_cancelled();
        Ok(test_project())
    }
}

/// The progress reports, as owned values a test can compare.
///
/// The match is total on purpose: it is the branching a consumer has to do, and
/// a stage added later fails here rather than being folded into its neighbour.
fn describe(progress: ImportProgress<'_>) -> (String, u32, u32) {
    let stage = match progress.stage {
        ImportStage::Extracting { item } => format!("extracting {item}"),
        ImportStage::WritingMetadata => "writing metadata".to_owned(),
        ImportStage::Complete => "complete".to_owned(),
    };
    (stage, progress.current, progress.total)
}

fn import_into<F: ImportFormat>(
    output_dir: &Utf8Path,
    format: F,
) -> Result<ModProject, ImportError<F::Error>> {
    ProjectImporter::new(output_dir).import(format)
}

/// A format decodes into directories that already exist, so no two of them have
/// to remember to create the same ones.
#[test]
fn the_driver_lays_out_the_project_before_the_format_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp).join("nested").join("project");

    let mut saw_base_layer_dir = false;
    import_into(
        &output,
        Recorder {
            saw_base_layer_dir: &mut saw_base_layer_dir,
        },
    )
    .unwrap();

    assert!(
        saw_base_layer_dir,
        "content/base existed when the format ran"
    );
    assert!(output.join("content/base").is_dir());
}

#[test]
fn the_driver_writes_the_config_the_format_returned() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let mut saw = false;
    let imported = import_into(
        &output,
        Recorder {
            saw_base_layer_dir: &mut saw,
        },
    )
    .unwrap();

    assert_eq!(ModProject::load(&output).unwrap(), imported);
}

/// The config is written once, so what `with_config` sets is what the file on
/// disk says as well as what the call returns.
#[test]
fn with_config_edits_reach_the_written_config() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let mut saw = false;
    let imported = ProjectImporter::new(&output)
        .with_config(|project| {
            project.name = "chosen-slug".to_owned();
            project.display_name = "Chosen Name".to_owned();
        })
        .import(Recorder {
            saw_base_layer_dir: &mut saw,
        })
        .unwrap();

    assert_eq!(imported.name, "chosen-slug");
    assert_eq!(ModProject::load(&output).unwrap(), imported);
}

/// The importer is built before the format is, so a caller can configure it and
/// hand it round without the format having been opened yet.
#[test]
fn an_importer_configured_in_one_statement_imports_in_another() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);
    let flag = AtomicBool::new(false);

    let importer = ProjectImporter::new(&output)
        .with_config(|project| project.name = "later".to_owned())
        .with_cancellation(&flag);

    let mut saw = false;
    let imported = importer
        .import(Recorder {
            saw_base_layer_dir: &mut saw,
        })
        .unwrap();

    assert_eq!(imported.name, "later");
}

/// The stages a format reports and the one the driver reports arrive in one
/// stream, and `Complete` sits at the totals the extraction left.
#[test]
fn the_driver_completes_the_progress_the_format_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let mut saw = false;
    let mut reported = Vec::new();
    ProjectImporter::new(&output)
        .import_with_progress(
            Recorder {
                saw_base_layer_dir: &mut saw,
            },
            &mut |progress| reported.push(describe(progress)),
        )
        .unwrap();

    assert_eq!(
        reported,
        [
            ("extracting only".to_owned(), 0, 1),
            ("complete".to_owned(), 1, 1),
        ]
    );
}

#[test]
fn a_format_failure_surfaces_under_the_format_variant() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let error = import_into(&output, Failing { cancelled: false }).unwrap_err();

    assert!(matches!(error, ImportError::Format(_)), "got {error:?}");
    assert!(
        !output.join("mod.config.json").exists(),
        "the config is the last thing written, so a failed import has none"
    );
}

/// One cancellation has one error, however deep in the import it landed.
#[test]
fn a_format_reporting_a_cancellation_surfaces_as_cancelled() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let error = import_into(&output, Failing { cancelled: true }).unwrap_err();

    assert!(matches!(error, ImportError::Cancelled), "got {error:?}");
}

/// A format that ignores the cancellation still does not get a config written,
/// so a cancelled import never leaves a project that looks complete.
#[test]
fn a_cancellation_a_format_ignored_still_fails_the_import() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);
    let flag = AtomicBool::new(true);

    let mut saw_cancelled = false;
    let error = ProjectImporter::new(&output)
        .with_cancellation(&flag)
        .import(Watching {
            saw_cancelled: &mut saw_cancelled,
        })
        .unwrap_err();

    assert!(matches!(error, ImportError::Cancelled), "got {error:?}");
    assert!(!output.join("mod.config.json").exists());
    assert!(saw_cancelled, "the format was told, and could have stopped");
}

#[test]
fn a_predicate_cancellation_reaches_the_format() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);
    let cancelled = || true;

    let mut saw_cancelled = false;
    let _ = ProjectImporter::new(&output)
        .with_cancellation(Cancellation::predicate(&cancelled))
        .import(Watching {
            saw_cancelled: &mut saw_cancelled,
        });

    assert!(saw_cancelled);
}

/// `ProjectPacker` refuses a project with a declared layer it cannot find, so an
/// import that left one without a directory would write a project that can never
/// be packed again.
#[test]
fn every_layer_the_imported_config_declares_gets_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp).join("project");

    import_into(&output, DeclaresAnEmptyLayer).unwrap();

    assert!(output.join("content/base").is_dir());
    assert!(
        output.join("content/skins").is_dir(),
        "a layer the archive held no content for still needs its directory"
    );
}

/// A layer the caller adds through `with_config` is a layer the written config
/// declares, so it needs a directory too.
#[test]
fn a_layer_added_by_with_config_gets_a_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp).join("project");

    let mut saw = false;
    ProjectImporter::new(&output)
        .with_config(|project| {
            project.layers.push(crate::ModProjectLayer {
                name: "added".to_string(),
                priority: 1,
                ..Default::default()
            });
        })
        .import(Recorder {
            saw_base_layer_dir: &mut saw,
        })
        .unwrap();

    assert!(output.join("content/added").is_dir());
}

/// The decoded project is the first thing a caller can judge, and the last
/// moment it can stop the import before a config is written.
#[test]
fn a_config_hook_can_refuse_what_the_archive_described() {
    #[derive(Debug, thiserror::Error)]
    #[error("a mod named {name} is already installed")]
    struct AlreadyInstalled {
        name: String,
    }

    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let mut saw = false;
    let error = ProjectImporter::new(&output)
        .try_with_config(|project| {
            Err(Box::new(AlreadyInstalled {
                name: project.name.clone(),
            }) as ConfigRefusal)
        })
        .import(Recorder {
            saw_base_layer_dir: &mut saw,
        })
        .unwrap_err();

    let ImportError::Refused(refusal) = &error else {
        panic!("expected a refusal, got {error:?}");
    };
    // The caller's own error comes back, so it can act on more than a message.
    assert_eq!(
        refusal.downcast_ref::<AlreadyInstalled>().unwrap().name,
        "test-mod"
    );
    assert!(
        !output.join("mod.config.json").exists(),
        "a refused import leaves no config, as a cancelled one does"
    );
}

/// A hook that only edits stays infallible, so the common call site does not
/// have to spell out a success it cannot fail to reach.
#[test]
fn an_infallible_config_hook_needs_no_result() {
    let tmp = tempfile::tempdir().unwrap();
    let output = utf8_dir(&tmp);

    let mut saw = false;
    let imported = ProjectImporter::new(&output)
        .with_config(|project| project.name = "renamed".to_owned())
        .import(Recorder {
            saw_base_layer_dir: &mut saw,
        })
        .unwrap();

    assert_eq!(imported.name, "renamed");
}
