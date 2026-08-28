//! Preserving a mod's names: harvest what is still readable and rewrite it
//! into the mod's own archive, before a repair makes it unreadable.
//!
//! [`preserve_archive_names`] is deliberately import-shaped: it reads the
//! archive at `source` and writes the preserved archive at `dest`, so the way
//! a mod enters a library *is* the preserve. A repair that only ever runs on
//! library mods then cannot run before the harvest - the ordering the whole
//! feature depends on holds by construction rather than by convention.

use std::fs::{self, File};
use std::io::{BufReader, Cursor};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{
    add_hashtables, classify_entry, FantomeEntry, FantomeExtractError, FantomeReader,
    FantomeRewriteError, RewriteOutcome,
};
use ltk_hashtable::{Category, Hashtable, Key};
use ltk_wad::{is_hex_chunk_path, NameRecovery, NoResolver, PathResolver, Wad, WadError, WadHash};

#[cfg(test)]
mod tests;

/// A preserve's account of itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarvestReport {
    /// Whether the mod was rewritten, and how many names it gained.
    pub outcome: PreserveOutcome,
    /// How many chunks have no recoverable name: hex-named, and named by
    /// nothing the harvest can read. Counted, never guessed at.
    pub unharvestable: usize,
}

/// What a preserve did to the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreserveOutcome {
    /// Every harvested name was already declared or excluded; the archive at
    /// `dest` is a plain copy of the source.
    Unchanged,
    /// The archive at `dest` declares the harvested names.
    Rewritten {
        /// How many names the archive gained.
        names_added: usize,
    },
}

/// Failure to preserve an archive's names.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PreserveError {
    /// A file could not be read or written.
    #[error("Failed to access {path}")]
    Io {
        /// The file that failed.
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The source archive could not be read.
    #[error(transparent)]
    Archive(#[from] FantomeExtractError),

    /// The rewritten archive could not be produced.
    #[error(transparent)]
    Rewrite(#[from] FantomeRewriteError),

    /// A packed WAD inside the archive could not be read for names.
    #[error("Failed to read the packed WAD {wad}")]
    Wad {
        /// The WAD's name, as the archive spells it.
        wad: String,
        #[source]
        source: WadError,
    },
}

impl PreserveError {
    fn io(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Harvest the names of the `.fantome` archive at `source` and write the
/// archive, names embedded, to `dest`.
///
/// The harvest reads the two places a mod's names still survive - the chunk
/// paths of its WAD directories, and the strings inside the bins of its
/// packed WADs - and merges what is genuinely new into the archive's embedded
/// hashtables. `exclusions` names what a reader can recover without the mod's
/// help, in practice the community hashtables; a name it knows is not
/// embedded. Passing `None` embeds every harvested name, which costs size,
/// never correctness.
///
/// When nothing is new the archive at `dest` is a plain copy of the source
/// (or, when `dest` *is* `source`, nothing is written at all) - a covered mod
/// is never rewritten, and running twice is a no-op. When something is new
/// the rewrite lands as a temporary file beside `dest` and is renamed over it
/// only after writing finishes cleanly, so an interrupted preserve never
/// leaves a half-written archive where a mod should be. The source is never
/// modified.
///
/// # Errors
///
/// Returns an error if the source cannot be read, a packed WAD inside it
/// cannot be mounted, or the destination cannot be written. `dest` is left as
/// it was on any error.
pub fn preserve_archive_names(
    source: &Utf8Path,
    dest: &Utf8Path,
    exclusions: Option<&dyn PathResolver>,
) -> Result<HarvestReport, PreserveError> {
    let file = File::open(source.as_std_path()).map_err(|e| PreserveError::io(source, e))?;
    let mut reader = FantomeReader::new(BufReader::new(file))?;

    let exclusions = exclusions.unwrap_or(&NoResolver);
    let (table, unharvestable) = harvest(&mut reader, exclusions)?;

    let parent = match dest.parent() {
        Some(parent) if !parent.as_str().is_empty() => parent,
        _ => Utf8Path::new("."),
    };
    fs::create_dir_all(parent.as_std_path()).map_err(|e| PreserveError::io(parent, e))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent.as_std_path())
        .map_err(|e| PreserveError::io(parent, e))?;

    let outcome = add_hashtables(&mut reader, temp.as_file_mut(), &[(Category::Game, table)])?;
    drop(reader);

    let outcome = match outcome {
        RewriteOutcome::Unchanged => {
            if source == dest {
                drop(temp);
            } else {
                // The plain copy still lands through the temp file and the
                // rename, so an interrupted preserve can no more truncate a
                // covered mod than a rewritten one.
                let mut original =
                    File::open(source.as_std_path()).map_err(|e| PreserveError::io(source, e))?;
                std::io::copy(&mut original, temp.as_file_mut())
                    .map_err(|e| PreserveError::io(dest, e))?;
                temp.persist(dest.as_std_path())
                    .map_err(|e| PreserveError::io(dest, e.error))?;
            }
            PreserveOutcome::Unchanged
        }
        RewriteOutcome::Rewritten { names_added } => {
            temp.persist(dest.as_std_path())
                .map_err(|e| PreserveError::io(dest, e.error))?;
            PreserveOutcome::Rewritten { names_added }
        }
    };

    Ok(HarvestReport {
        outcome,
        unharvestable,
    })
}

/// The WAD-space hash of `name`, computed by the crate that owns hashing so
/// the exclusion check and the embedded table cannot disagree about a key.
fn wad_hash(name: &str) -> WadHash {
    let (algorithm, width) = Category::Game
        .default_shape()
        .expect("game is a known category with a computable shape");
    let key = Key::of(name, &algorithm, width).expect("xxh64 keys are always computable");
    WadHash(key.value())
}

/// Read the names still recoverable from the archive: chunk paths on disk,
/// then names the bins of each packed WAD hold.
fn harvest<R: std::io::Read + std::io::Seek>(
    reader: &mut FantomeReader<R>,
    exclusions: &dyn PathResolver,
) -> Result<(Hashtable, usize), PreserveError> {
    let mut table = Hashtable::default();
    let mut unharvestable = 0;

    let mut packed_wads = Vec::new();
    for entry_name in reader.entry_names() {
        match classify_entry(entry_name) {
            Some(FantomeEntry::WadFile(relative_path)) => {
                let Some((_wad_dir, chunk_path)) = relative_path.split_once('/') else {
                    continue;
                };
                if is_hex_chunk_path(Utf8Path::new(chunk_path)) {
                    unharvestable += 1;
                    continue;
                }
                if exclusions.is_known(wad_hash(chunk_path)) {
                    continue;
                }
                // A name outside the table grammar cannot be embedded;
                // nothing else can carry it either, so it counts as lost.
                if table.push(chunk_path).is_err() {
                    unharvestable += 1;
                }
            }
            Some(FantomeEntry::PackedWad(name)) => packed_wads.push(name.to_owned()),
            _ => {}
        }
    }

    for wad_name in packed_wads {
        let Some(bytes) = reader.read_packed_wad(&wad_name)? else {
            continue;
        };
        let mut wad = Wad::mount(Cursor::new(bytes)).map_err(|source| PreserveError::Wad {
            wad: wad_name.clone(),
            source,
        })?;
        let recovered = NameRecovery::new()
            .run(&mut wad, exclusions)
            .map_err(|source| PreserveError::Wad {
                wad: wad_name.clone(),
                source,
            })?;
        for chunk in wad.chunks() {
            if exclusions.is_known(chunk.path_hash) {
                continue;
            }
            match recovered.get(chunk.path_hash) {
                Some(path) => {
                    if table.push(path).is_err() {
                        unharvestable += 1;
                    }
                }
                None => unharvestable += 1,
            }
        }
    }

    Ok((table, unharvestable))
}
