//! Editing an archive in place: merge harvested names, replace content, or
//! both, in one pass.
//!
//! The rewrite raw-copies every entry it does not itself replace, so the
//! mismatched CRC32 values Fantome tools in the wild write are carried
//! through rather than recomputed - which is what [`FantomeReader`] already
//! expects, since it deliberately bypasses the check - and no entry is ever
//! decompressed or recompressed.
//!
//! That is what editing buys over repacking. A caller changing a handful of
//! files in a gigabyte archive pays for those files and a byte copy of the
//! rest, where packing the project again re-encodes every chunk in it.

use std::collections::HashSet;
use std::io::{Read, Seek, Write};

use ltk_hashtable::{Category, Hashtable, HashtableSet, Key};

use crate::{
    FantomeExtractError, FantomeHashtable, FantomeReader, FantomeWriteError, FantomeWriter,
};

/// What a rewrite did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteOutcome {
    /// Every harvested name was already declared; nothing was written to the
    /// sink.
    Unchanged,
    /// The sink holds the rewritten archive.
    Rewritten {
        /// How many names the archive gained.
        names_added: usize,
    },
}

/// Failure to rewrite an archive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FantomeRewriteError {
    /// The source archive could not be read.
    #[error(transparent)]
    Read(#[from] FantomeExtractError),

    /// The rewritten archive could not be written.
    #[error(transparent)]
    Write(#[from] FantomeWriteError),
}

/// The first conventional table path for `category` that `taken` does not
/// hold. The `{category}.hashes.txt` name is a convention, not a rule, so a
/// qualifier is added whenever the plain name would land on an entry that is
/// already someone's.
fn free_table_path(category: &Category, taken: &HashSet<String>) -> String {
    let mut candidates =
        std::iter::once(format!("META/hashes/{category}.hashes.txt")).chain((1..).map(|attempt| {
            match attempt {
                1 => format!("META/hashes/{category}.harvested.hashes.txt"),
                n => format!("META/hashes/{category}.harvested{n}.hashes.txt"),
            }
        }));
    candidates
        .find(|candidate| !taken.contains(&candidate.to_ascii_lowercase()))
        .expect("the candidate sequence is unbounded")
}

/// One table file the rewrite will emit, and the manifest entry declaring it.
struct PlannedTable {
    manifest: FantomeHashtable,
    table: Hashtable,
}

/// Merge `harvested` names into the archive `reader` holds, writing the
/// result to `sink`.
///
/// The merge adds and never replaces: a name whose key an archive's declared
/// tables already resolve is not added again, an existing manifest gains
/// entries rather than losing any, and an existing table file gains the
/// category's new names in `LC_ALL=C` order while keeping every name it
/// already held. Every other entry is raw-copied byte-for-byte, wrong CRC32
/// values included.
///
/// When nothing is genuinely new the sink is left untouched and
/// [`RewriteOutcome::Unchanged`] comes back - deciding costs one read, so a
/// covered mod is never rewritten and a rerun is a no-op. The caller owns
/// where the sink lives; a rewrite over a file the user did not ask to lose
/// belongs behind a temp-file-and-rename.
///
/// # Errors
///
/// Returns an error if the source archive cannot be read or the rewritten
/// archive cannot be written. Nothing is written on a read failure.
pub fn add_hashtables<R: Read + Seek, W: Write + Seek>(
    reader: &mut FantomeReader<R>,
    sink: W,
    harvested: &[(Category, Hashtable)],
) -> Result<RewriteOutcome, FantomeRewriteError> {
    replace_entries(reader, sink, &[], harvested)
}

/// Write `entries` in place of what the archive holds, and merge `harvested`.
///
/// Each entry is an archive path and the bytes to store under it, replacing
/// that path where the archive has one and adding it where it has none. Every
/// entry not named here is raw-copied, so the cost is the entries given plus a
/// byte copy - which is what makes editing a large archive worth doing at all
/// over packing its project again.
///
/// `harvested` merges exactly as [`add_hashtables`] describes, because that is
/// this function with no entries. A caller replacing content it hashed a name
/// out of wants both halves in one pass: two passes would write the archive
/// twice to change it once.
///
/// A call with no entries and no genuinely new names leaves the sink untouched
/// and answers [`RewriteOutcome::Unchanged`]. The caller owns where the sink
/// lives, so a rewrite over a file the user did not ask to lose belongs behind
/// a temp-file-and-rename.
///
/// # Errors
///
/// Returns an error if the source archive cannot be read or the rewritten
/// archive cannot be written. Nothing is written on a read failure.
pub fn replace_entries<R: Read + Seek, W: Write + Seek>(
    reader: &mut FantomeReader<R>,
    sink: W,
    entries: &[(&str, &[u8])],
    harvested: &[(Category, Hashtable)],
) -> Result<RewriteOutcome, FantomeRewriteError> {
    let mut info = reader.read_info()?;
    let declared = reader.read_hashtables()?;
    let resolved = HashtableSet::build(declared.iter().cloned());

    // Paths a new entry must not land on: everything the manifest already
    // declares - the entries this tool cannot read included, since unknown is
    // not disposable - and every entry the archive holds.
    let mut taken: HashSet<String> = info
        .hashtables
        .iter()
        .map(|manifest| manifest.path.to_ascii_lowercase())
        .collect();
    taken.extend(reader.entry_names().map(str::to_ascii_lowercase));

    let mut plans: Vec<PlannedTable> = Vec::new();
    let mut names_added = 0;

    for (category, table) in harvested {
        // The shape new keys are judged in: the first declared entry of the
        // category whose keys are computable, else the registry's shape. A
        // category with neither is skipped - its keys mean nothing here.
        let merge_target = info.hashtables.iter().find(|manifest| {
            manifest.category == *category
                && manifest
                    .to_entry()
                    .is_some_and(|entry| Key::of("", entry.algorithm(), entry.width()).is_some())
        });
        let (algorithm, width) = match merge_target {
            Some(manifest) => {
                let entry = manifest.to_entry().expect("merge target validated above");
                (entry.algorithm().clone(), entry.width())
            }
            None => match category.default_shape() {
                Some(shape) => shape,
                None => continue,
            },
        };

        let mut fresh_keys = HashSet::new();
        let fresh: Vec<&str> = table
            .names()
            .filter(|name| {
                let Some(key) = Key::of(name, &algorithm, width) else {
                    return false;
                };
                resolved.resolve(category, key).is_none() && fresh_keys.insert(key)
            })
            .collect();
        if fresh.is_empty() {
            continue;
        }
        names_added += fresh.len();

        let (manifest, base) = match merge_target {
            Some(manifest) => {
                let base = declared
                    .iter()
                    .find(|(entry, _)| entry.path() == manifest.path.as_str())
                    .map(|(_, table)| table.clone())
                    .unwrap_or_default();
                (manifest.clone(), base)
            }
            None => {
                let path = free_table_path(category, &taken);
                taken.insert(path.to_ascii_lowercase());
                let manifest = FantomeHashtable {
                    path,
                    category: category.clone(),
                    algorithm: algorithm.clone(),
                    bits: width.bits(),
                };
                info.hashtables.push(manifest.clone());
                (manifest, Hashtable::default())
            }
        };

        let mut merged = Hashtable::from_names(base.names().chain(fresh.iter().copied()))
            .expect("every name came out of a validated table");
        merged.sort();
        plans.push(PlannedTable {
            manifest,
            table: merged,
        });
    }

    if plans.is_empty() && entries.is_empty() {
        return Ok(RewriteOutcome::Unchanged);
    }

    let mut writer = FantomeWriter::new(sink);
    writer.write_info(&info)?;

    let mut replaced: HashSet<String> = plans
        .iter()
        .map(|plan| plan.manifest.path.to_ascii_lowercase())
        .collect();
    replaced.insert("meta/info.json".to_owned());
    replaced.extend(entries.iter().map(|(path, _)| path.to_ascii_lowercase()));

    for plan in &plans {
        writer.write_hashtable(&plan.manifest, &plan.table)?;
    }

    for (path, bytes) in entries {
        writer.write_entry(path, &mut &bytes[..])?;
    }

    for index in 0..reader.entry_count() {
        let file = reader
            .zip_archive_mut()
            .by_index_raw(index)
            .map_err(FantomeExtractError::from)?;
        if replaced.contains(&file.name().to_ascii_lowercase()) {
            continue;
        }
        writer
            .zip_mut()
            .raw_copy_file(file)
            .map_err(FantomeWriteError::from)?;
    }
    writer.finish()?;

    Ok(RewriteOutcome::Rewritten { names_added })
}
