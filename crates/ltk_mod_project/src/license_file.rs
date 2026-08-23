//! Discovery of the license text file at a mod project root.
//!
//! The license *field* in the manifest names the terms; the license *file*
//! carries them. The two are independent: either, both, or neither may be
//! present.

use camino::{Utf8Path, Utf8PathBuf};

/// License file names recognized at a project root, in precedence order.
pub const LICENSE_FILE_NAMES: [&str; 3] = ["LICENSE", "LICENSE.md", "LICENSE.txt"];

/// Find the project's license file, matched case-insensitively.
///
/// Scans the directory rather than probing paths so the result does not depend
/// on filesystem case sensitivity: a `license.txt` written on Windows must be
/// found on Linux too.
///
/// When several files match, the earliest entry in [`LICENSE_FILE_NAMES`] wins;
/// ties within one name (only possible on a case-sensitive filesystem, e.g.
/// `LICENSE` next to `license`) are broken lexicographically on the on-disk
/// name so the pick is deterministic.
///
/// A file whose own name is not valid UTF-8 is skipped, which costs nothing:
/// every recognized name is ASCII.
///
/// Returns `None` if the directory cannot be read or holds no license file.
pub fn find_license_file(project_root: &Utf8Path) -> Option<Utf8PathBuf> {
    let entries = std::fs::read_dir(project_root).ok()?;

    // Best on-disk name per recognized name, indexed into LICENSE_FILE_NAMES.
    let mut best: Vec<Option<String>> = vec![None; LICENSE_FILE_NAMES.len()];

    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|ty| ty.is_file()) {
            continue;
        }

        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };

        let Some(index) = LICENSE_FILE_NAMES
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(&file_name))
        else {
            continue;
        };

        match &best[index] {
            Some(current) if current.as_str() <= file_name.as_str() => {}
            _ => best[index] = Some(file_name),
        }
    }

    best.into_iter()
        .flatten()
        .next()
        .map(|name| project_root.join(name))
}

/// The canonical spelling of a recognized license file name, if it is one.
///
/// Lets a packer name an archive entry consistently (`license.txt` on disk
/// becomes `LICENSE.txt`) so that pack → extract → pack settles on one name
/// instead of drifting with whatever the author happened to type.
pub fn canonical_license_file_name(file_name: &str) -> Option<&'static str> {
    LICENSE_FILE_NAMES
        .iter()
        .copied()
        .find(|candidate| candidate.eq_ignore_ascii_case(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tmp: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap()
    }

    #[test]
    fn returns_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let root = temp_root(&tmp);

        std::fs::write(root.join("README.md"), "readme").unwrap();

        assert_eq!(find_license_file(&root), None);
    }

    #[test]
    fn finds_plain_license() {
        let tmp = tempfile::tempdir().unwrap();
        let root = temp_root(&tmp);

        std::fs::write(root.join("LICENSE"), "MIT").unwrap();

        assert_eq!(find_license_file(&root), Some(root.join("LICENSE")));
    }

    #[test]
    fn matches_case_insensitively() {
        let tmp = tempfile::tempdir().unwrap();
        let root = temp_root(&tmp);

        std::fs::write(root.join("license.txt"), "MIT").unwrap();

        assert_eq!(find_license_file(&root), Some(root.join("license.txt")));
    }

    #[test]
    fn prefers_earlier_name_in_precedence_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = temp_root(&tmp);

        std::fs::write(root.join("LICENSE.txt"), "txt").unwrap();
        std::fs::write(root.join("LICENSE.md"), "md").unwrap();

        assert_eq!(find_license_file(&root), Some(root.join("LICENSE.md")));

        std::fs::write(root.join("LICENSE"), "plain").unwrap();

        assert_eq!(find_license_file(&root), Some(root.join("LICENSE")));
    }

    #[test]
    fn ignores_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let root = temp_root(&tmp);

        std::fs::create_dir(root.join("LICENSE")).unwrap();
        std::fs::write(root.join("LICENSE.md"), "md").unwrap();

        assert_eq!(find_license_file(&root), Some(root.join("LICENSE.md")));
    }

    #[test]
    fn returns_none_for_missing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = temp_root(&tmp);

        assert_eq!(find_license_file(&root.join("does-not-exist")), None);
    }
}
