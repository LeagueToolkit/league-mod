//! Where a Fantome archive's entries land in an imported project.

use std::io::{Read, Seek};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{classify_entry, FantomeEntry, FantomeReader};

use crate::{ModProjectLayer, ProjectPath, ProjectPaths};

/// A Fantome archive answers where an import would put every entry it holds.
///
/// The mapping [`FantomeImporter`](super::FantomeImporter) applies, asked as a
/// question. A caller that has to know where files will land before unpacking -
/// to preflight the Windows path length limit, say - asks here rather than
/// restating the rules, which is the only way the estimate and the import stay
/// in step when either changes.
///
/// Reads the archive's entry names and no entry's content, so asking costs no
/// decompression.
///
/// Unlike a `.modpkg`, the answer is not complete: a packed WAD comes back as a
/// path [`is_unpacked_wad`](ProjectPath::is_unpacked_wad) answers `true` for,
/// because what lands beneath it is not knowable before the unpack. A caller
/// sizing the result has to allow for names of the resolver's choosing under
/// those directories.
///
/// # Example
///
/// ```no_run
/// use ltk_fantome::FantomeReader;
/// use ltk_mod_project::ProjectPaths;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = FantomeReader::new(std::fs::File::open("my-mod.fantome")?)?;
///
/// for path in reader.iter_project_paths() {
///     match path.is_unpacked_wad() {
///         true => println!("unpacks a WAD into {path}"),
///         false => println!("writes {path}"),
///     }
/// }
/// # Ok(())
/// # }
/// ```
impl<R: Read + Seek> ProjectPaths for FantomeReader<R> {
    fn iter_project_paths(&self) -> impl Iterator<Item = ProjectPath> + '_ {
        self.entry_names().filter_map(project_path)
    }
}

/// Where the entry named `entry_name` lands in an imported project.
///
/// `None` for an entry the import writes no file for: `META/info.json` becomes
/// `mod.config.json` by way of the metadata rather than as a file, and an entry
/// the format does not place is skipped. A directory entry is `None` too -
/// directories come back as the parents of the files that land in them.
pub(crate) fn project_path(entry_name: &str) -> Option<ProjectPath> {
    let path = match classify_entry(entry_name)? {
        FantomeEntry::PackedWad(name) => {
            return Some(ProjectPath::unpacked_wad(base_layer_dir().join(name)))
        }
        FantomeEntry::WadFile(relative_path) => base_layer_dir().join(relative_path),
        FantomeEntry::Raw(relative_path) => raw_dir().join(relative_path),
        FantomeEntry::Readme => Utf8PathBuf::from("README.md"),
        FantomeEntry::License(file_name) => Utf8PathBuf::from(file_name),
        FantomeEntry::Image => Utf8PathBuf::from("thumbnail.webp"),
        FantomeEntry::Info => return None,
    };

    Some(ProjectPath::file(path))
}

/// `content/base`, as a project-relative path.
fn base_layer_dir() -> Utf8PathBuf {
    ModProjectLayer::content_path(Utf8Path::new(""), ModProjectLayer::BASE_NAME)
}

/// `content/base/raw`, as a project-relative path.
fn raw_dir() -> Utf8PathBuf {
    ModProjectLayer::raw_content_path(Utf8Path::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(entry_name: &str) -> Utf8PathBuf {
        match project_path(entry_name) {
            Some(path) if !path.is_unpacked_wad() => path.into_path(),
            other => panic!("expected a file for {entry_name}, got {other:?}"),
        }
    }

    #[test]
    fn wad_files_land_in_the_base_layer() {
        assert_eq!(
            file("WAD/Aatrox.wad.client/assets/x.bin"),
            "content/base/Aatrox.wad.client/assets/x.bin"
        );
    }

    #[test]
    fn a_packed_wad_is_a_directory_whose_contents_are_not_yet_known() {
        assert_eq!(
            project_path("WAD/Aatrox.wad.client"),
            Some(ProjectPath::unpacked_wad("content/base/Aatrox.wad.client"))
        );
    }

    #[test]
    fn raw_entries_land_under_the_raw_directory() {
        assert_eq!(file("RAW/assets/x.bin"), "content/base/raw/assets/x.bin");
    }

    #[test]
    fn meta_entries_land_at_the_project_root() {
        assert_eq!(file("META/README.md"), "README.md");
        assert_eq!(file("README.md"), "README.md");
        assert_eq!(file("META/LICENSE.txt"), "LICENSE.txt");
        assert_eq!(file("META/image.png"), "thumbnail.webp");
    }

    /// A directory entry is not a file, so it has no destination. A caller
    /// sizing an import counts what lands, and nothing lands for a directory.
    #[test]
    fn a_directory_entry_has_no_destination() {
        assert_eq!(project_path("WAD/Aatrox.wad.client/"), None);
        assert_eq!(project_path("RAW/assets/"), None);
        assert_eq!(
            file("WAD/Aatrox.wad.client/assets/x.bin"),
            "content/base/Aatrox.wad.client/assets/x.bin",
            "the files beneath it still land"
        );
    }

    /// The metadata becomes `mod.config.json` through the conversion, not as a
    /// copied file.
    #[test]
    fn the_metadata_entry_writes_no_file_of_its_own() {
        assert_eq!(project_path("META/info.json"), None);
        assert_eq!(project_path("something-else.txt"), None);
    }

    #[test]
    fn prefixes_are_matched_the_way_the_extraction_matches_them() {
        assert_eq!(
            file("wad/Aatrox.wad.client/x.bin"),
            file("WAD/Aatrox.wad.client/x.bin")
        );
        assert_eq!(file("raw/x.bin"), file("RAW/x.bin"));
        assert_eq!(file("meta/LICENSE"), "LICENSE");
    }
}
