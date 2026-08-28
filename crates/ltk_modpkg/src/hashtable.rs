//! The modpkg spelling of the embedded-hashtables manifest.
//!
//! Each container spells `ltk_hashtable`'s domain entry its own way; this is
//! the modpkg one, carried in [`ModpkgMetadata::hashtables`] at schema
//! version 3. The manifest is authoritative: a chunk under
//! [`HASHTABLES_CHUNK_DIR`] that no manifest entry declares does not exist
//! for lookup.
//!
//! [`ModpkgMetadata::hashtables`]: crate::ModpkgMetadata::hashtables

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

impl<TSource: std::io::Read + std::io::Seek> crate::Modpkg<TSource> {
    /// Load the hashtables the metadata declares - and only those.
    ///
    /// A chunk under `_meta_/hashes/` that no manifest entry declares is not
    /// a table and is not read; the manifest is authoritative. An entry whose
    /// declared width is not one a key can have is skipped rather than
    /// refused - it still travels with the package, it just cannot answer a
    /// lookup here.
    ///
    /// The pairs feed `ltk_hashtable::HashtableSet::build` as they are, in
    /// manifest order.
    ///
    /// # Errors
    ///
    /// Returns an error if the package cannot be read, a declared table chunk
    /// is missing, or one does not fit the table grammar.
    pub fn load_hashtables(
        &mut self,
    ) -> Result<Vec<(ltk_hashtable::HashtableEntry, ltk_hashtable::Hashtable)>, crate::ModpkgError>
    {
        let manifests = self.load_metadata()?.hashtables;
        let mut tables = Vec::new();
        for manifest in manifests {
            let Some(entry) = manifest.to_entry() else {
                continue;
            };
            let chunk = *self.chunk(&manifest.path, None).map_err(|error| {
                // Named by path rather than hash: the manifest holds the
                // path, and a hex hash names nothing to a user.
                match error {
                    crate::ModpkgError::MissingChunk(_) => crate::ModpkgError::MissingHashtable {
                        path: manifest.path.clone(),
                    },
                    other => other,
                }
            })?;
            if chunk.layer().is_some() || chunk.wad().is_some() {
                return Err(crate::ModpkgError::InvalidMetaChunk);
            }
            let data = self.decoder().load_chunk_decompressed(&chunk)?;
            let table = ltk_hashtable::Hashtable::from_reader(&*data).map_err(|source| {
                crate::ModpkgError::InvalidHashtable {
                    path: manifest.path.clone(),
                    source,
                }
            })?;
            tables.push((entry, table));
        }
        Ok(tables)
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

    /// A declared chunk that does not fit the table grammar fails the load,
    /// named by the path the manifest declared.
    #[test]
    fn a_table_outside_the_grammar_fails_by_its_declared_path() {
        let mut modpkg = package([(manifest("game.hashes.txt", 64), "\u{feff}bom.tex\n")]);

        let error = modpkg.load_hashtables().unwrap_err();

        assert!(matches!(
            error,
            crate::ModpkgError::InvalidHashtable { ref path, .. }
                if path == "_meta_/hashes/game.hashes.txt"
        ));
    }

    /// A package declaring no tables loads none - including one written
    /// before schema v3.
    #[test]
    fn a_package_without_tables_loads_none() {
        let mut modpkg = package([]);

        assert!(modpkg.load_hashtables().unwrap().is_empty());
    }
}
