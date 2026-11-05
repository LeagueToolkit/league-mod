//! The modpkg spelling of the embedded-hashtables manifest.
//!
//! Each container spells `ltk_hashtable`'s domain entry its own way; this is
//! the modpkg one, carried in [`ModpkgMetadata::hashtables`] at schema
//! version 3. The manifest is authoritative: a chunk under
//! [`HASHTABLES_CHUNK_DIR`] that no manifest entry declares does not exist
//! for lookup.
//!
//! [`ModpkgMetadata::hashtables`]: crate::ModpkgMetadata::hashtables

use std::fmt;

use serde::{Deserialize, Serialize};

/// The chunk directory hashtable chunks are stored under.
///
/// A table's chunk path is `_meta_/hashes/{file name}`: the file name is the
/// one the mod project keeps under its `hashes/` directory, so project ->
/// modpkg -> project loses no table names.
pub const HASHTABLES_CHUNK_DIR: &str = "_meta_/hashes";

/// One embedded hashtable, as the metadata declares it.
///
/// The modpkg spelling of `ltk_hashtable`'s `HashtableEntry`: snake_case keys
/// in the msgpack metadata chunk, `path` naming the table's chunk. Convert
/// with [`to_entry`](Self::to_entry) and [`from_entry`](Self::from_entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ModpkgHashtable {
    /// The table's chunk path, e.g. `_meta_/hashes/game.hashes.txt`.
    pub path: String,
    /// The lookup domain of the table's names.
    pub category: ltk_hashtable::Category,
    /// The hash function keying the table's names.
    pub algorithm: ltk_hashtable::Algorithm,
    /// The declared key width in bits.
    pub bits: u8,
}

impl ModpkgHashtable {
    /// The entry as `ltk_hashtable`'s domain type.
    ///
    /// `None` when `bits` declares a width no key can have; the standard
    /// requires `1..=64`.
    pub fn to_entry(&self) -> Option<ltk_hashtable::HashtableEntry> {
        let width = ltk_hashtable::KeyWidth::new(self.bits)?;
        Some(ltk_hashtable::HashtableEntry::new(
            self.path.as_str(),
            self.category.clone(),
            self.algorithm.clone(),
            width,
        ))
    }

    /// Spell a domain entry the modpkg way.
    pub fn from_entry(entry: &ltk_hashtable::HashtableEntry) -> Self {
        Self {
            path: entry.path().to_string(),
            category: entry.category().clone(),
            algorithm: entry.algorithm().clone(),
            bits: entry.width().bits(),
        }
    }
}

/// A hashtable declaration that a read skipped, and the reason it was skipped.
///
/// [`Modpkg::skipped_hashtables`](crate::Modpkg::skipped_hashtables) lists
/// these. [`Display`](fmt::Display) renders one as a message for a log.
#[derive(Debug)]
#[non_exhaustive]
pub struct SkippedHashtable {
    /// The declaration, as the metadata holds it.
    pub manifest: ModpkgHashtable,
    /// The reason the read skipped the declaration.
    pub reason: ltk_hashtable::HashtableSkipReason,
}

impl fmt::Display for SkippedHashtable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Skipped the hashtable {}: {}",
            self.manifest.path, self.reason
        )
    }
}

/// The tables a manifest declares, split into the ones read and the ones
/// skipped.
#[derive(Debug, Default)]
struct DeclaredHashtables {
    /// The tables read, each with its declaration, in manifest order.
    tables: Vec<(ltk_hashtable::HashtableEntry, ltk_hashtable::Hashtable)>,
    /// The declarations whose chunk is absent or unreadable, in manifest order.
    skipped: Vec<SkippedHashtable>,
}

/// The UTF-8 byte order mark. Tools in the wild write one at the start of a
/// table file, and the table grammar refuses it.
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

impl<TSource: std::io::Read + std::io::Seek> crate::Modpkg<TSource> {
    /// Loads the hashtables the metadata declares - and only those.
    ///
    /// A chunk under `_meta_/hashes/` that no manifest entry declares is not
    /// a table and is not read. The manifest is authoritative. A declaration
    /// whose width is not one a key can have is skipped. It travels with the
    /// package and answers no lookup here.
    ///
    /// A declaration whose chunk is absent or does not fit the table grammar
    /// is skipped. [`skipped_hashtables`](Self::skipped_hashtables) lists those
    /// declarations. A table chunk that starts with a UTF-8 byte order mark is
    /// read without it.
    ///
    /// The pairs feed `ltk_hashtable::HashtableSet::build` as they are, in
    /// manifest order.
    ///
    /// # Errors
    ///
    /// Returns an error if the metadata cannot be read, a declared chunk
    /// belongs to a layer or a WAD, or a declared chunk cannot be decompressed.
    pub fn load_hashtables(
        &mut self,
    ) -> Result<Vec<(ltk_hashtable::HashtableEntry, ltk_hashtable::Hashtable)>, crate::ModpkgError>
    {
        Ok(self.load_declared_hashtables()?.tables)
    }

    /// Lists the hashtable declarations that [`load_hashtables`](Self::load_hashtables)
    /// skips for an absent or unreadable chunk, in manifest order.
    ///
    /// A declaration whose width no key can have is not listed. Its chunk is
    /// never read.
    ///
    /// # Errors
    ///
    /// Returns an error under the same conditions as
    /// [`load_hashtables`](Self::load_hashtables).
    pub fn skipped_hashtables(&mut self) -> Result<Vec<SkippedHashtable>, crate::ModpkgError> {
        Ok(self.load_declared_hashtables()?.skipped)
    }

    /// Loads the tables the metadata declares, and lists the declarations
    /// whose chunk is absent or unreadable.
    fn load_declared_hashtables(&mut self) -> Result<DeclaredHashtables, crate::ModpkgError> {
        let manifests = self.load_metadata()?.hashtables;
        let mut declared = DeclaredHashtables::default();

        for manifest in manifests {
            let Some(entry) = manifest.to_entry() else {
                continue;
            };

            let chunk = match self.chunk(&manifest.path, None) {
                Ok(chunk) => *chunk,
                Err(crate::ModpkgError::MissingChunk(_)) => {
                    declared.skipped.push(SkippedHashtable {
                        manifest,
                        reason: ltk_hashtable::HashtableSkipReason::Absent,
                    });
                    continue;
                }
                Err(other) => return Err(other),
            };
            if chunk.layer().is_some() || chunk.wad().is_some() {
                return Err(crate::ModpkgError::InvalidMetaChunk);
            }

            let data = self.decoder().load_chunk_decompressed(&chunk)?;
            let data = data.strip_prefix(UTF8_BOM).unwrap_or(&data);
            match ltk_hashtable::Hashtable::from_reader(data) {
                Ok(table) => declared.tables.push((entry, table)),
                Err(source) => declared.skipped.push(SkippedHashtable {
                    manifest,
                    reason: ltk_hashtable::HashtableSkipReason::Unreadable(source),
                }),
            }
        }

        Ok(declared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{ModpkgBuilder, ModpkgLayerBuilder};
    use crate::Modpkg;
    use std::io::Cursor;

    fn manifest(file_name: &str, bits: u8) -> ModpkgHashtable {
        ModpkgHashtable {
            path: format!("{HASHTABLES_CHUNK_DIR}/{file_name}"),
            category: ltk_hashtable::Category::Game,
            algorithm: ltk_hashtable::Algorithm::Xxh64,
            bits,
        }
    }

    fn package(
        tables: impl IntoIterator<Item = (ModpkgHashtable, &'static str)>,
    ) -> Modpkg<Cursor<Vec<u8>>> {
        let mut builder = ModpkgBuilder::default().with_layer(ModpkgLayerBuilder::base());
        for (manifest, data) in tables {
            builder = builder.with_hashtable(manifest, data).unwrap();
        }
        let mut cursor = Cursor::new(Vec::new());
        builder
            .build_to_writer(&mut cursor, |_| Ok(vec![0xAA; 10]))
            .unwrap();
        cursor.set_position(0);
        Modpkg::mount_from_reader(cursor).unwrap()
    }

    /// The pairs come back in manifest order, because `HashtableSet::build`
    /// merges in manifest order and first declaration wins.
    #[test]
    fn declared_tables_load_in_manifest_order() {
        let mut modpkg = package([
            (manifest("game.hashes.txt", 64), "ASSETS/Custom/One.tex\n"),
            (
                manifest("game.imported.hashes.txt", 64),
                "ASSETS/Custom/Two.tex\n",
            ),
        ]);

        let tables = modpkg.load_hashtables().unwrap();

        let loaded: Vec<(&str, Vec<&str>)> = tables
            .iter()
            .map(|(entry, table)| (entry.path().as_str(), table.names().collect()))
            .collect();
        assert_eq!(
            loaded,
            [
                (
                    "_meta_/hashes/game.hashes.txt",
                    vec!["ASSETS/Custom/One.tex"]
                ),
                (
                    "_meta_/hashes/game.imported.hashes.txt",
                    vec!["ASSETS/Custom/Two.tex"]
                ),
            ]
        );
    }

    /// An entry whose width no key can have is skipped, not refused: it still
    /// travels with the package, it just cannot answer a lookup here.
    #[test]
    fn an_impossible_width_is_skipped_rather_than_refused() {
        let mut modpkg = package([
            (manifest("game.hashes.txt", 0), "ASSETS/Custom/One.tex\n"),
            (
                manifest("game.ok.hashes.txt", 64),
                "ASSETS/Custom/Two.tex\n",
            ),
        ]);

        let tables = modpkg.load_hashtables().unwrap();

        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].0.path(), "_meta_/hashes/game.ok.hashes.txt");
    }

    /// Removes the chunk at `path` from a mounted package. The result is a
    /// package whose writer declared a table it did not store.
    fn drop_chunk(modpkg: &mut Modpkg<Cursor<Vec<u8>>>, path: &str) {
        let key = modpkg.resolve_chunk_key(path, None).unwrap();
        modpkg.chunks.remove(&key);
    }

    #[test]
    fn a_declared_chunk_the_package_does_not_hold_is_skipped_and_reported_absent() {
        let absent = manifest("game.harvested.hashes.txt", 64);
        let mut modpkg = package([
            (absent.clone(), "ASSETS/Custom/One.tex\n"),
            (manifest("game.hashes.txt", 64), "ASSETS/Custom/Two.tex\n"),
        ]);
        drop_chunk(&mut modpkg, &absent.path);

        let tables = modpkg.load_hashtables().unwrap();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].0.path(), "_meta_/hashes/game.hashes.txt");

        let skipped = modpkg.skipped_hashtables().unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].manifest, absent);
        assert!(matches!(
            skipped[0].reason,
            ltk_hashtable::HashtableSkipReason::Absent
        ));
        assert_eq!(
            skipped[0].to_string(),
            "Skipped the hashtable _meta_/hashes/game.harvested.hashes.txt: the container holds no file at that path"
        );
    }

    #[test]
    fn a_declared_chunk_outside_the_grammar_is_skipped_and_reported_unreadable() {
        let unreadable = manifest("game.hashes.txt", 64);
        let mut modpkg = package([(unreadable.clone(), "assets\\back\\slash.tex\n")]);

        assert!(modpkg.load_hashtables().unwrap().is_empty());

        let skipped = modpkg.skipped_hashtables().unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].manifest, unreadable);
        assert!(matches!(
            skipped[0].reason,
            ltk_hashtable::HashtableSkipReason::Unreadable(
                ltk_hashtable::HashtableReadError::InvalidName { .. }
            )
        ));
    }

    #[test]
    fn a_table_chunk_with_a_byte_order_mark_reads_without_it() {
        let mut modpkg = package([(
            manifest("game.hashes.txt", 64),
            "\u{feff}ASSETS/Custom/One.tex\nASSETS/Custom/Two.tex\n",
        )]);

        let tables = modpkg.load_hashtables().unwrap();
        assert_eq!(
            tables[0].1.names().collect::<Vec<_>>(),
            ["ASSETS/Custom/One.tex", "ASSETS/Custom/Two.tex"]
        );
        assert!(modpkg.skipped_hashtables().unwrap().is_empty());
    }

    /// A package declaring no tables loads none - including one written
    /// before schema v3.
    #[test]
    fn a_package_without_tables_loads_none() {
        let mut modpkg = package([]);

        assert!(modpkg.load_hashtables().unwrap().is_empty());
    }
}
