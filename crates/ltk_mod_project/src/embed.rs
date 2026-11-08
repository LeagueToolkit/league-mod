//! Embedding a mod's names: read the names that survive in a mod and write
//! them into the mod's own archive, before a repair makes them unreadable.
//!
//! [`embed_archive_names`] is import-shaped: it reads the archive at `source`
//! and writes the archive with its names embedded at `dest`. The way a mod
//! enters a library is the embed. A repair that runs only on library mods runs
//! after the names are read, by construction.

use std::fs::{self, File};
use std::io::{BufReader, Cursor};

use camino::{Utf8Path, Utf8PathBuf};
use ltk_fantome::{
    add_hashtables, classify_entry, FantomeEntry, FantomeExtractError, FantomeReader,
    FantomeRewriteError, RewriteOutcome, SkippedHashtable,
};
use ltk_hashtable::{Category, Hashtable, Key};
use ltk_wad::{is_hex_chunk_path, NameRecovery, NoResolver, PathResolver, Wad, WadError, WadHash};

#[cfg(test)]
mod tests;

/// The read size of the plain copy of an unchanged archive. `io::copy` reads a
/// bare `File` 8 KiB at a time outside Linux.
const COPY_BUFFER_SIZE: usize = 1024 * 1024;

/// An embed's account of itself.
#[derive(Debug)]
#[non_exhaustive]
pub struct EmbedReport {
    /// Whether the mod was rewritten, and how many names it gained.
    pub outcome: EmbedOutcome,
    /// How many chunks have no recoverable name: hex-named, and named by
    /// nothing the embed can read. Counted, never guessed at.
    pub unnamed: usize,
    /// The source archive's hashtable declarations whose file is absent or
    /// unreadable, in manifest order.
    ///
    /// The embed skips each one. A rewritten archive does not declare it.
    pub skipped: Vec<SkippedHashtable>,
}

/// What an embed did to the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedOutcome {
    /// Every name was already declared or excluded, and the archive at `dest`
    /// is a plain copy of the source.
    Unchanged,
    /// The archive at `dest` declares the mod's names.
    Rewritten {
        /// How many names the archive gained.
        embedded: usize,
    },
}

/// Failure to embed an archive's names.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EmbedError {
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

impl EmbedError {
    fn io(path: impl Into<Utf8PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// Reads the names of the `.fantome` archive at `source` and writes the
/// archive, names embedded, to `dest`.
///
/// The embed reads the two places a mod's names survive - the chunk paths of
/// its WAD directories, and the strings inside the bins of its packed WADs -
/// and merges what is genuinely new into the archive's embedded hashtables.
/// `exclusions` names what a reader can recover without the mod's help, in
/// practice the community hashtables. A name it knows is not embedded.
/// Passing `None` embeds every name, which costs size, never correctness.
///
/// A hashtable declaration whose file is absent or unreadable is skipped and
/// listed in [`EmbedReport::skipped`]. It fails nothing.
///
/// When nothing is new the archive at `dest` is a plain copy of the source
/// (or, when `dest` *is* `source`, nothing is written at all) - a covered mod
/// is never rewritten, and running twice is a no-op. When something is new
/// the rewrite lands as a temporary file beside `dest` and is renamed over it
/// only after writing finishes cleanly. An interrupted embed leaves no
/// half-written archive at `dest`. The source is never modified.
///
/// # Errors
///
/// Returns an error if the source cannot be read, a packed WAD inside it
/// cannot be mounted, or the destination cannot be written. `dest` is left as
/// it was on any error.
pub fn embed_archive_names(
    source: &Utf8Path,
    dest: &Utf8Path,
    exclusions: Option<&dyn PathResolver>,
) -> Result<EmbedReport, EmbedError> {
    let file = File::open(source.as_std_path()).map_err(|e| EmbedError::io(source, e))?;
    let mut reader = FantomeReader::new(BufReader::new(file))?;

    let exclusions = exclusions.unwrap_or(&NoResolver);
    let (table, unnamed) = read_names(&mut reader, exclusions)?;
    let skipped = reader.skipped_hashtables()?;

    let parent = match dest.parent() {
        Some(parent) if !parent.as_str().is_empty() => parent,
        _ => Utf8Path::new("."),
    };
    fs::create_dir_all(parent.as_std_path()).map_err(|e| EmbedError::io(parent, e))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent.as_std_path())
        .map_err(|e| EmbedError::io(parent, e))?;

    let outcome = add_hashtables(&mut reader, temp.as_file_mut(), &[(Category::Game, table)])?;
    drop(reader);

    let outcome = match outcome {
        RewriteOutcome::Unchanged => {
            if source == dest {
                drop(temp);
            } else {
                // The plain copy still lands through the temp file and the
                // rename, so an interrupted embed can no more truncate a
                // covered mod than a rewritten one.
                let original =
                    File::open(source.as_std_path()).map_err(|e| EmbedError::io(source, e))?;
                let mut original = BufReader::with_capacity(COPY_BUFFER_SIZE, original);
                std::io::copy(&mut original, temp.as_file_mut())
                    .map_err(|e| EmbedError::io(dest, e))?;
                temp.persist(dest.as_std_path())
                    .map_err(|e| EmbedError::io(dest, e.error))?;
            }
            EmbedOutcome::Unchanged
        }
        RewriteOutcome::Rewritten { names_added } => {
            temp.persist(dest.as_std_path())
                .map_err(|e| EmbedError::io(dest, e.error))?;
            EmbedOutcome::Rewritten {
                embedded: names_added,
            }
        }
    };

    Ok(EmbedReport {
        outcome,
        unnamed,
        skipped,
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

/// Reads the names still recoverable from the archive: chunk paths on disk,
/// then names the bins of each packed WAD hold.
///
/// Returns the names and the count of chunks that have none.
fn read_names<R: std::io::Read + std::io::Seek>(
    reader: &mut FantomeReader<R>,
    exclusions: &dyn PathResolver,
) -> Result<(Hashtable, usize), EmbedError> {
    let mut table = Hashtable::default();
    let mut unnamed = 0;

    let mut packed_wads = Vec::new();
    for entry_name in reader.entry_names() {
        match classify_entry(entry_name) {
            Some(FantomeEntry::WadFile {
                path: relative_path,
                ..
            }) => {
                let Some((_wad_dir, chunk_path)) = relative_path.split_once('/') else {
                    continue;
                };
                if is_hex_chunk_path(Utf8Path::new(chunk_path)) {
                    unnamed += 1;
                    continue;
                }
                if exclusions.is_known(wad_hash(chunk_path)) {
                    continue;
                }
                // A name outside the table grammar cannot be embedded.
                // Nothing else can carry it either, and it counts as unnamed.
                if table.push(chunk_path).is_err() {
                    unnamed += 1;
                }
            }
            Some(FantomeEntry::PackedWad { layer, name }) => {
                packed_wads.push((layer.to_owned(), name.to_owned()));
            }
            _ => {}
        }
    }

    for (layer, wad_name) in packed_wads {
        let Some(bytes) = reader.read_packed_wad(&layer, &wad_name)? else {
            continue;
        };
        let mut wad = Wad::mount(Cursor::new(bytes)).map_err(|source| EmbedError::Wad {
            wad: wad_name.clone(),
            source,
        })?;
        let recovered = NameRecovery::new()
            .run(&mut wad, exclusions)
            .map_err(|source| EmbedError::Wad {
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
                        unnamed += 1;
                    }
                }
                None => unnamed += 1,
            }
        }
    }

    Ok((table, unnamed))
}
