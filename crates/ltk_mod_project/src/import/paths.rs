//! Where an import puts what it writes, asked before it writes anything.

use std::fmt;
use std::ops::Deref;

use camino::{Utf8Path, Utf8PathBuf};

/// Somewhere an import writes, relative to the project directory.
///
/// Behaves as the path it names - it derefs to [`Utf8Path`] and joins onto a
/// directory like one - because that is what a caller almost always wants.
///
/// [`is_unpacked_wad`](Self::is_unpacked_wad) is the exception, and it matters
/// to one caller in particular: a Fantome archive stores a packed WAD as a
/// single entry and unpacks it into a directory, and what lands beneath it is
/// not knowable before the unpack - a chunk is named by whatever resolves its
/// hash, and keeps the hash as a hex name when nothing does. A preflight for
/// the Windows path length limit that measures such a path measures the
/// directory, not the long names that will sit under it, so it has to allow for
/// them itself.
///
/// A `.modpkg` import never produces one: a package stores its chunks
/// individually and names every one of them.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectPath {
    path: Utf8PathBuf,
    unpacked_wad: bool,
}

impl ProjectPath {
    /// A file the import writes at `path`.
    pub fn file(path: impl Into<Utf8PathBuf>) -> Self {
        Self {
            path: path.into(),
            unpacked_wad: false,
        }
    }

    /// A directory the import unpacks a packed WAD into.
    ///
    /// See [`is_unpacked_wad`](Self::is_unpacked_wad) for what that costs a
    /// caller counting what an import writes.
    pub fn unpacked_wad(path: impl Into<Utf8PathBuf>) -> Self {
        Self {
            path: path.into(),
            unpacked_wad: true,
        }
    }

    /// The path itself.
    pub fn path(&self) -> &Utf8Path {
        &self.path
    }

    /// The path, taken by value.
    pub fn into_path(self) -> Utf8PathBuf {
        self.path
    }

    /// Whether more files land beneath this path than the answer can name.
    ///
    /// `true` only for a packed WAD in a Fantome archive, which is unpacked
    /// into this directory rather than written as a file. A caller sizing an
    /// import counts what it can see and allows for names of the resolver's
    /// choosing beneath the ones this answers `true` for.
    pub fn is_unpacked_wad(&self) -> bool {
        self.unpacked_wad
    }
}

impl Deref for ProjectPath {
    type Target = Utf8Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

impl AsRef<Utf8Path> for ProjectPath {
    fn as_ref(&self) -> &Utf8Path {
        &self.path
    }
}

impl From<ProjectPath> for Utf8PathBuf {
    fn from(destination: ProjectPath) -> Self {
        destination.path
    }
}

impl fmt::Display for ProjectPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.path, f)
    }
}

/// Where importing an archive puts what it holds.
///
/// Implemented for the two things that can answer it without unpacking
/// anything: `ltk_modpkg`'s `ExtractionPlan` (see [`modpkg`](crate::modpkg))
/// and `ltk_fantome`'s `FantomeReader` (see [`fantome`](crate::fantome)), so
/// one preflight covers both formats.
pub trait ProjectPaths {
    /// Every path an import writes, relative to the project directory.
    ///
    /// A caller sizing an import - to preflight the Windows path length limit,
    /// say - reads this instead of restating the layout and drifting from it.
    ///
    /// The `mod.config.json` an import writes is not listed: it is the
    /// driver's rather than the archive's, and where it goes does not depend on
    /// what the archive holds.
    ///
    /// A Fantome archive's answer is not complete: see
    /// [`ProjectPath::is_unpacked_wad`].
    fn iter_project_paths(&self) -> impl Iterator<Item = ProjectPath> + '_;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A caller that only wants the path should not have to say so.
    #[test]
    fn a_project_path_behaves_as_the_path_it_names() {
        let path = ProjectPath::file("content/base/x.bin");

        assert_eq!(path.file_name(), Some("x.bin"));
        assert_eq!(
            Utf8Path::new("C:/mods/my-mod").join(&path),
            "C:/mods/my-mod/content/base/x.bin"
        );
        assert_eq!(path.to_string(), "content/base/x.bin");
    }

    /// The one thing the path alone does not say: whether names the answer
    /// could not give will land beneath it.
    #[test]
    fn only_an_unpacked_wad_holds_more_than_the_answer_names() {
        assert!(!ProjectPath::file("content/base/x.bin").is_unpacked_wad());
        assert!(ProjectPath::unpacked_wad("content/base/Aatrox.wad.client").is_unpacked_wad());
    }
}
