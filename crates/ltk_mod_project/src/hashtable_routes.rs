//! Routing hashtable declarations between a container and the project,
//! shared by the fantome and modpkg sides.
//!
//! A route maps one declared table to where it lands on the other side. The
//! pieces of that story live here: the escape-proofed placement rules
//! ([`is_plain_tail`], [`file_name_of`]), the one-destination-per-source
//! rule ([`NameClaims`], refusing with [`DuplicateHashtableName`]), and the
//! two pairing shapes the directions produce - [`HashtableRoute`] for an
//! import's keyed join against what it reads out of the archive,
//! [`PlannedRoute`] for a pack's by-construction pairing with the plan's
//! tables. Each container module owns its own placement rule and builds its
//! own manifest type; what is identical across containers is here.

use std::collections::HashMap;

use camino::Utf8Path;

/// Whether a tail can be carried under the destination directory as it is:
/// nothing but plain path components, so a manifest cannot steer a write
/// with `..`, an absolute path, a drive prefix or a backslash. A tail that
/// fails this lands by its file name instead.
///
/// The backslash and colon checks are byte checks rather than component
/// checks on purpose: what counts as a separator or a prefix differs by
/// platform, and a mapped path must mean the same file everywhere.
pub(crate) fn is_plain_tail(tail: &str) -> bool {
    !tail.is_empty()
        && !tail.contains(['\\', ':'])
        && Utf8Path::new(tail)
            .components()
            .all(|component| matches!(component, camino::Utf8Component::Normal(_)))
}

/// The final `/`-separated component of `path`.
///
/// For a table declared outside the conventional directory (or one whose
/// tail cannot be carried). Split on `/` alone rather than
/// [`Utf8Path::file_name`], which reads `\` as a separator on Windows only -
/// the mapping must answer the same on every platform. The constant is the
/// last resort for a path with no usable final component - all but
/// unreachable, but a mapping used by conversions has to be total.
pub(crate) fn file_name_of(path: &str) -> &str {
    path.rsplit('/')
        .next()
        .filter(|name| is_plain_tail(name))
        .unwrap_or("unnamed.hashes.txt")
}

/// Two different hashtable files land on one destination name.
///
/// Conversions flatten manifest-declared table paths into one destination
/// directory, so tables declared in different places can collide on a file
/// name. The colliding pair is refused rather than renamed: on a pack the
/// author can rename a file; on an import an ambiguous archive must not be
/// guessed at (and writing both would clobber one with the other).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("hashtable files {first} and {second} both land on {destination}")]
pub struct DuplicateHashtableName {
    destination: String,
    first: String,
    second: String,
}

impl DuplicateHashtableName {
    /// The destination path both declarations land on.
    pub fn destination(&self) -> &str {
        &self.destination
    }

    /// The declared path of the first table to land there.
    pub fn first(&self) -> &str {
        &self.first
    }

    /// The declared path of the second table, the one that collided.
    pub fn second(&self) -> &str {
        &self.second
    }
}

/// One hashtable declaration's mapping: the path it was declared at, and
/// the manifest entry it becomes on the other side.
///
/// The pairing is the mapping's one guarantee: whoever writes the mapped
/// files pairs what it read at `source` with where `manifest` lands, so the
/// written files and the written manifest cannot drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HashtableRoute<M> {
    /// The declared path the entry was mapped from.
    pub(crate) source: String,
    /// The manifest entry it becomes, holding the destination path.
    pub(crate) manifest: M,
}

/// One planned table's mapping: the container manifest entry it becomes,
/// attached to the planned table it declares.
///
/// Built in one pass over the plan, so a manifest entry cannot be paired
/// with another table's content - there is no second sequence to correlate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlannedRoute<'a, M> {
    /// The container manifest entry, holding the destination path.
    pub(crate) manifest: M,
    /// The planned table the entry declares.
    pub(crate) planned: &'a crate::pack::PlannedHashtable,
}

/// Claims destination paths for the manifest mappings, one per source file.
///
/// One source path always gets one answer, so a file two manifest entries
/// declare stays one file; a *different* source whose tail lands on a
/// claimed path is a [`DuplicateHashtableName`] error. Destinations compare
/// case-insensitively, matching how archive entries are looked up; sources
/// compare exactly, so case-variant spellings of one path count as two
/// files (whether they are one file is the filesystem's secret, and an
/// ambiguous pair must not pack differently on different platforms).
#[derive(Default)]
pub(crate) struct NameClaims {
    /// Lowercased destination path, to the source path that claimed it.
    claimed: HashMap<String, String>,
}

impl NameClaims {
    /// `{dir}/{tail}`, unless a different source already claimed it.
    ///
    /// # Errors
    ///
    /// [`DuplicateHashtableName`] when the destination is claimed by a
    /// different source path.
    pub(crate) fn claim(
        &mut self,
        dir: &str,
        tail: &str,
        source: &str,
    ) -> Result<String, DuplicateHashtableName> {
        let destination = format!("{dir}/{tail}");
        match self.claimed.get(&destination.to_ascii_lowercase()) {
            Some(claimant) if claimant != source => Err(DuplicateHashtableName {
                destination,
                first: claimant.clone(),
                second: source.to_owned(),
            }),
            Some(_) => Ok(destination),
            None => {
                self.claimed
                    .insert(destination.to_ascii_lowercase(), source.to_owned());
                Ok(destination)
            }
        }
    }
}
