//! Error types for overlay operations.
//!
//! All fallible functions in this crate return [`Result<T>`], which uses
//! [`Error`](enum@Error) as the error type. External error types
//! (`std::io::Error`, `serde_json::Error`, WAD errors) are automatically
//! converted via `From` impls.

use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::WadHash;
use thiserror::Error;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during overlay building.
///
/// This crate's own domain failures are grouped into four detail enums -
/// [`GameDirError`], [`ModContentError`], [`WadLimitError`] and
/// [`CorruptionError`] - so a caller can branch on the four categories it will
/// actually act on differently, and drill into the detail only where it wants a
/// specific message. Nothing here carries a pre-formatted string: rendering is
/// the caller's decision.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// A file or directory could not be read.
    #[error("Failed to read {path}")]
    Read {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A file or directory could not be written.
    #[error("Failed to write {path}")]
    Write {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// An IO failure with no single path to blame.
    ///
    /// Prefer [`Read`](Self::Read) or [`Write`](Self::Write) wherever there is
    /// a path: `io::Error` never carries one.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Failed to parse or serialize JSON (overlay state, mod config).
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// Error from the `ltk_wad` crate when mounting or reading a WAD file.
    #[error(transparent)]
    Wad(#[from] ltk_wad::WadError),

    /// Error from the `ltk_wad` WAD builder when writing a patched WAD.
    #[error(transparent)]
    WadBuilder(#[from] ltk_wad::WadBuilderError),

    /// A ZIP archive (`.fantome` mod content) could not be opened or read.
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),

    /// One entry inside a ZIP archive could not be read.
    ///
    /// Separate from [`Zip`](Self::Zip) because `ZipError` never names the
    /// entry, and "file not found in archive" is useless without it.
    #[error("Failed to read `{entry}` from the archive")]
    ArchiveEntry {
        entry: String,
        #[source]
        source: zip::result::ZipError,
    },

    /// Error from the `ltk_modpkg` crate when reading `.modpkg` mod content.
    #[error(transparent)]
    Modpkg(#[from] ltk_modpkg::ModpkgError),

    /// A cache file could not be written.
    #[error("Failed to write the cache at {path}")]
    CacheWrite {
        path: Utf8PathBuf,
        #[source]
        source: CacheError,
    },

    /// A mod references a WAD file that doesn't exist in the game directory.
    #[error("WAD file not found: {0}")]
    WadNotFound(Utf8PathBuf),

    /// A WAD filename matches multiple files in the game directory.
    #[error("Ambiguous WAD '{name}': found {count} candidates")]
    AmbiguousWad { name: String, count: usize },

    /// A mod directory is missing or inaccessible (used by [`FsModContent`](crate::FsModContent)).
    #[error("Invalid mod directory: {0}")]
    InvalidModDir(Utf8PathBuf),

    /// A mod project's `.modignore` could not be loaded (used by
    /// [`FsModContent`](crate::FsModContent)). Failing beats filtering
    /// differently than packing would: what the overlay injects for testing
    /// must be what the package ships.
    #[error(transparent)]
    Ignore(#[from] ltk_mod_project::ModIgnoreError),

    /// The game installation cannot be used for a build.
    #[error(transparent)]
    GameDir(#[from] GameDirError),

    /// A mod's content could not be used.
    #[error(transparent)]
    ModContent(#[from] ModContentError),

    /// The output would exceed what the WAD v3.4 format can represent.
    #[error(transparent)]
    WadLimit(#[from] WadLimitError),

    /// A file is not what its own metadata says it is.
    #[error(transparent)]
    Corrupt(#[from] CorruptionError),

    /// An invariant this crate is supposed to maintain was broken.
    ///
    /// Not reachable from any input a caller controls: getting one means a bug
    /// in `ltk_overlay`. It is an error rather than a panic because builds run
    /// inside a parallel patch loop driven by a GUI, where taking the process
    /// down costs the user more than a failed build does.
    #[error("{0} - this is a bug in ltk_overlay, please report it")]
    Bug(Invariant),
}

/// An internal guarantee of this crate that did not hold.
///
/// Each of these sits where one build step consumes what an earlier one
/// produced, and names the guarantee between them. A caller cannot provoke any
/// of them; they are enumerated rather than described in prose so a bug report
/// can name the exact one without quoting a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Error)]
#[non_exhaustive]
pub enum Invariant {
    /// Pass 1 recorded an override whose mod is not in the enabled list.
    #[error("an override's metadata names a mod that is not enabled")]
    OverrideNamesUnenabledMod,

    /// A chunk was classified as a string patch but no plan was built for it.
    #[error("a chunk classified as a string patch has no plan")]
    StringPatchWithoutPlan,

    /// A writer asked for override bytes pass 2 never prepared.
    #[error("a writer asked for an override this build never prepared")]
    OverrideNeverPrepared,

    /// A string patch reached the per-mod read loop, which groups by mod id.
    ///
    /// String patches are synthesized from several mods at once, so they are
    /// resolved by their own pass and never carry a mod to be grouped under.
    #[error("a string patch was grouped with a single mod's overrides")]
    StringPatchGroupedByMod,
}

/// Why a game directory cannot be used for a build.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum GameDirError {
    /// The directory has no `DATA/FINAL`, so it is not a League installation.
    #[error("{path} is not a League installation: it has no DATA/FINAL directory")]
    MissingDataFinal { path: Utf8PathBuf },

    /// A WAD the build resolved lies outside the game directory.
    #[error("WAD {wad} is not under the game directory {game_dir}")]
    WadOutsideGameDir {
        game_dir: Utf8PathBuf,
        wad: Utf8PathBuf,
    },

    /// A stringtable chunk the build expected is not in the game WAD, which is
    /// what a game update that moved or renamed it looks like.
    #[error("game WAD {wad} does not hold stringtable chunk {chunk_hash:016x}")]
    StringtableChunkMissing {
        wad: Utf8PathBuf,
        chunk_hash: WadHash,
    },
}

/// Why a mod's content could not be used.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ModContentError {
    /// A `.fantome` archive has no `META/info.json`.
    #[error("the fantome archive has no META/info.json")]
    FantomeInfoMissing,

    /// A `.fantome` archive was asked for a layer it cannot carry.
    #[error("fantome archives carry only a 'base' layer, not '{layer}'")]
    FantomeLayerUnsupported { layer: String },

    /// An override is in neither the archive's WAD folder nor its packed WAD.
    #[error("override WAD/{wad_name}/{rel_path} is not in the fantome archive")]
    FantomeOverrideMissing {
        wad_name: String,
        rel_path: Utf8PathBuf,
    },

    /// A raw override is not in the archive's `RAW` folder.
    #[error("raw override {rel_path} is not in the fantome archive")]
    FantomeRawOverrideMissing { rel_path: Utf8PathBuf },

    /// A hex-named override does not name a chunk the packed WAD holds.
    #[error("chunk {path_hash:016x} is not in the archive's packed WAD")]
    PackedChunkMissing { path_hash: WadHash },

    /// `.modpkg` content has no raw-override concept.
    #[error("the modpkg format has no raw overrides")]
    ModpkgRawUnsupported,

    /// A mod's string overrides produced a table that will not serialize.
    #[error("the '{locale}' string overrides produced a stringtable that cannot be written")]
    StringOverrideUnencodable {
        locale: String,
        #[source]
        source: ltk_rst::RstError,
    },
}

/// A limit of the WAD v3.4 format the output would have exceeded.
///
/// All of these are content problems the user can act on by splitting a mod,
/// not failures of the build itself.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WadLimitError {
    /// A region would end past the format's 4 GiB addressable limit.
    #[error(
        "{wad} exceeds the 4 GiB limit of the WAD v3.4 format: {region} ends at offset {offset}"
    )]
    FileTooLarge {
        wad: Utf8PathBuf,
        region: WadRegion,
        offset: u64,
    },

    /// More chunks than the format's `u32` chunk count can index.
    #[error("{wad} has {count} chunks, more than the WAD v3.4 format can index")]
    TooManyChunks { wad: Utf8PathBuf, count: usize },

    /// A chunk's size overflows the format's `u32` size fields.
    #[error(
        "chunk {path_hash:016x} is too large for the WAD v3.4 format \
         (compressed {compressed} / uncompressed {uncompressed} bytes)"
    )]
    ChunkTooLarge {
        path_hash: WadHash,
        compressed: usize,
        uncompressed: usize,
    },

    /// A chunk shifted into the copied region falls outside the format's `u32`
    /// offset fields.
    #[error("chunk {path_hash:016x} cannot be addressed at offset {offset}")]
    ChunkUnaddressable { path_hash: WadHash, offset: i64 },

    /// The rebuild needs more TOC entries than the file reserved.
    ///
    /// While `TOC_SLACK_ENTRIES` is zero this also fires when the set *shrinks*,
    /// because capacity is then exactly the entry count.
    #[error("{wad} reserved {reserved} TOC entries, not the {needed} this rebuild needs")]
    TocCapacity {
        wad: Utf8PathBuf,
        needed: usize,
        reserved: u32,
    },
}

/// Which part of a patched WAD ran past a format limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WadRegion {
    /// The game WAD's data region, copied intact.
    SourceRegion,
    /// The override tail appended after it.
    OverrideTail,
}

impl std::fmt::Display for WadRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceRegion => f.write_str("the copied source data region"),
            Self::OverrideTail => f.write_str("the override tail"),
        }
    }
}

/// A file whose contents disagree with its own metadata.
///
/// Inside a build these are absorbed: the WAD in question is rebuilt in full.
/// One reaching a caller means the *game's* own files are the ones at fault.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CorruptionError {
    /// A chunk's bytes do not hash to the checksum its TOC entry records.
    #[error(
        "chunk {path_hash:016x} does not match its recorded checksum \
         (found {found:016x}, expected {expected:016x})"
    )]
    ChunkChecksum {
        path_hash: WadHash,
        found: u64,
        expected: u64,
    },

    /// A recorded layout's own numbers do not hang together.
    #[error(
        "incoherent WAD layout: {toc_capacity} TOC entries, \
         region at {data_region_offset}, tail at {tail_offset}"
    )]
    IncoherentLayout {
        toc_capacity: u32,
        data_region_offset: u64,
        tail_offset: u64,
    },

    /// A WAD's chunks reach past the end of the file.
    #[error("{wad} is truncated: its chunks reach offset {reach} but the file is {len} bytes")]
    TruncatedWad {
        wad: Utf8PathBuf,
        reach: usize,
        len: usize,
    },

    /// The game's stringtable for a locale could not be parsed.
    #[error("the game's '{locale}' stringtable could not be parsed")]
    StringtableParse {
        locale: String,
        #[source]
        source: ltk_rst::RstError,
    },
}

impl Error {
    /// A failure reading the file or directory at `path`.
    pub(crate) fn read(path: impl AsRef<Utf8Path>, source: std::io::Error) -> Self {
        Self::Read {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// A failure writing the file or directory at `path`.
    pub(crate) fn write(path: impl AsRef<Utf8Path>, source: std::io::Error) -> Self {
        Self::Write {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    /// A failure reading `entry` from a ZIP archive.
    ///
    /// Takes `impl Into<ZipError>` so an `io::Error` from decompressing an
    /// entry lands here too.
    pub(crate) fn archive_entry(
        entry: impl Into<String>,
        source: impl Into<zip::result::ZipError>,
    ) -> Self {
        Self::ArchiveEntry {
            entry: entry.into(),
            source: source.into(),
        }
    }

    /// A failure writing the cache file at `path`, whatever the step.
    pub(crate) fn cache_write(path: impl Into<Utf8PathBuf>, source: impl Into<CacheError>) -> Self {
        Self::CacheWrite {
            path: path.into(),
            source: source.into(),
        }
    }
}

/// Failure to access one of this crate's on-disk caches.
///
/// Separates a disk problem the user can act on (no space, no permission)
/// from an encoding failure, which is a bug to report.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CacheError {
    /// A filesystem failure on the cache file or its parent directory.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The cache could not be encoded.
    ///
    /// The source is boxed rather than named, so the encoding the caches
    /// happen to use is not part of this crate's public API.
    #[error("Failed to encode the cache")]
    Encode(#[source] Box<dyn std::error::Error + Send + Sync>),
}

// Kept as a `From` impl so `?` and `Error::cache_write` work across both cache
// writers. The conversion names `rmp_serde`, but the variant does not, so
// matching on an encode failure never forces a caller to depend on it.
impl From<rmp_serde::encode::Error> for CacheError {
    fn from(error: rmp_serde::encode::Error) -> Self {
        Self::Encode(Box::new(error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Consumers log these with `{e}` alone rather than walking the chain, so a
    /// pass-through variant must display its cause rather than a category name.
    #[test]
    fn pass_through_display_carries_the_cause() {
        let error = Error::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "overlay is locked",
        ));

        assert!(error.to_string().contains("overlay is locked"), "{error}");
    }

    /// `io::Error` never carries a path, so "the system cannot find the file
    /// specified" is unactionable on its own.
    #[test]
    fn file_errors_name_the_file() {
        let read = Error::read(
            "content/base/Aatrox.wad.client/skin0.bin",
            std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        let write = Error::write(
            "overlay/DATA/FINAL",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );

        assert!(read.to_string().contains("skin0.bin"), "{read}");
        assert!(write.to_string().contains("overlay/DATA/FINAL"), "{write}");
    }

    /// The path goes in the message, so the source must not repeat it or a
    /// chain walker prints the file twice.
    #[test]
    fn file_errors_do_not_embed_their_source() {
        let error = Error::read("state/overlay.json", std::io::Error::other("inner detail"));
        let source = std::error::Error::source(&error).unwrap().to_string();

        assert!(!error.to_string().contains(&source), "{error}");
    }

    /// `ZipError::FileNotFound` renders without naming a file, so the variant
    /// that wraps it has to supply the entry itself.
    #[test]
    fn archive_entry_names_the_entry() {
        let error = Error::archive_entry("META/info.json", zip::result::ZipError::FileNotFound);

        assert!(error.to_string().contains("META/info.json"), "{error}");
    }

    /// The two cache writers share one variant, so the path is the only thing
    /// that says which cache failed.
    #[test]
    fn cache_write_names_the_file() {
        let error = Error::cache_write(
            Utf8PathBuf::from("cache/game_index.bin"),
            std::io::Error::other("disk full"),
        );

        assert!(
            error.to_string().contains("cache/game_index.bin"),
            "{error}"
        );
    }

    /// A full disk is worth surfacing to the user; a failed encode is a bug to
    /// report. The caller can only tell them apart if the variants differ.
    #[test]
    fn cache_write_separates_disk_from_encoding() {
        let error = Error::cache_write("cache/meta.bin", std::io::Error::other("disk full"));

        assert!(
            matches!(
                error,
                Error::CacheWrite {
                    source: CacheError::Io(_),
                    ..
                }
            ),
            "{error}"
        );
    }

    /// A caller can tell a corrupt archive from a missing game file.
    #[test]
    fn wrapped_sources_stay_matchable() {
        let error = Error::from(zip::result::ZipError::FileNotFound);

        assert!(matches!(error, Error::Zip(_)), "{error}");
    }

    /// The four detail enums exist so a caller can branch on the category it
    /// will act on - prompt a reinstall, blame a mod, report a size limit -
    /// without reading a message. Each must survive the conversion into
    /// [`Error`] as its own variant.
    #[test]
    fn domain_failures_are_matchable_by_category() {
        let game_dir = Error::from(GameDirError::MissingDataFinal {
            path: "C:/Riot Games/League of Legends/Game".into(),
        });
        let mod_content = Error::from(ModContentError::ModpkgRawUnsupported);
        let limit = Error::from(WadLimitError::TooManyChunks {
            wad: "Map11.wad.client".into(),
            count: u32::MAX as usize + 1,
        });
        let corrupt = Error::from(CorruptionError::ChunkChecksum {
            path_hash: WadHash(0x1234),
            found: 1,
            expected: 2,
        });

        assert!(matches!(game_dir, Error::GameDir(_)), "{game_dir}");
        assert!(matches!(mod_content, Error::ModContent(_)), "{mod_content}");
        assert!(matches!(limit, Error::WadLimit(_)), "{limit}");
        assert!(matches!(corrupt, Error::Corrupt(_)), "{corrupt}");
    }

    /// The values a caller would want to render - a path, a count, a limit -
    /// are fields, not text parsed back out of a message.
    #[test]
    fn detail_variants_carry_data_not_prose() {
        let error = Error::from(WadLimitError::FileTooLarge {
            wad: "DATA/FINAL/Maps/Map11.wad.client".into(),
            region: WadRegion::OverrideTail,
            offset: 5_000_000_000,
        });

        let Error::WadLimit(WadLimitError::FileTooLarge {
            wad,
            region,
            offset,
        }) = &error
        else {
            panic!("expected a FileTooLarge limit, got {error}");
        };
        assert_eq!(wad, "DATA/FINAL/Maps/Map11.wad.client");
        assert_eq!(*region, WadRegion::OverrideTail);
        assert_eq!(*offset, 5_000_000_000);
    }

    /// A detail enum is wrapped transparently, so consumers that only log get
    /// the specific message rather than a category name.
    #[test]
    fn category_display_carries_the_detail() {
        let error = Error::from(GameDirError::MissingDataFinal {
            path: "D:/Games/League".into(),
        });

        assert!(error.to_string().contains("D:/Games/League"), "{error}");
        assert!(error.to_string().contains("DATA/FINAL"), "{error}");
    }
}
