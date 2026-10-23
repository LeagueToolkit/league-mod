//! Name sheets: one sheet per name table, cut from a consumer's hash tables.

use std::fmt;

use camino::Utf8Path;
use ltk_hash::WadHash;
use serde::{Deserialize, Serialize};

use super::TermKind;
use super::sheet::{Sheet, SheetBuilder};
use crate::{CacheError, Fingerprint, GameIndex, cache};

/// The cache format version of the name sheets.
///
/// Bumped on any change to [`NameSheets`], [`Sheet`], or the container layout.
pub const NAME_CACHE_FORMAT_VERSION: u32 = 1;

/// The hash tables search reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
#[repr(u8)]
pub enum NameTable {
    /// Object paths, FNV-1a 32.
    Entries = 0,
    /// Class names, FNV-1a 32.
    Types = 1,
    /// Property names, FNV-1a 32.
    Fields = 2,
    /// `hash` values, FNV-1a 32.
    Hashes = 3,
    /// Chunk paths, XXH64.
    Chunks = 4,
}

impl NameTable {
    /// Every table, in declaration order.
    pub const ALL: [NameTable; 5] = [
        NameTable::Entries,
        NameTable::Types,
        NameTable::Fields,
        NameTable::Hashes,
        NameTable::Chunks,
    ];

    /// The width of the table's hashes in bits: 32, or 64 for [`NameTable::Chunks`].
    #[must_use]
    pub const fn bits(self) -> u8 {
        match self {
            NameTable::Chunks => 64,
            NameTable::Entries | NameTable::Types | NameTable::Fields | NameTable::Hashes => 32,
        }
    }

    /// The lowercase name of the table.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            NameTable::Entries => "entries",
            NameTable::Types => "types",
            NameTable::Fields => "fields",
            NameTable::Hashes => "hashes",
            NameTable::Chunks => "chunks",
        }
    }

    /// The hash kinds that resolve through the table, in [`TermKind::ALL`] order.
    pub(crate) const fn kinds(self) -> &'static [TermKind] {
        match self {
            NameTable::Entries => &[TermKind::Object, TermKind::Link],
            NameTable::Types => &[TermKind::Class, TermKind::Embedded],
            NameTable::Fields => &[TermKind::Field],
            NameTable::Hashes => &[TermKind::Hash],
            NameTable::Chunks => &[TermKind::Wad],
        }
    }

    pub(crate) const fn ordinal(self) -> usize {
        self as usize
    }
}

impl fmt::Display for NameTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A consumer's hash tables.
pub trait NameSource {
    /// Identifies the contents of `table`. `None` cuts the sheet on every build and caches
    /// nothing for it.
    ///
    /// Two sources with equal versions for a table name every hash the same. A checksum of
    /// the table file is a version.
    fn version(&self, table: NameTable) -> Option<String>;

    /// Visits every `(hash, name)` of `table`, in any order. A table the source does not have
    /// visits nothing.
    fn for_each_name(&self, table: NameTable, visit: &mut dyn FnMut(u64, &str));
}

/// One table's sheet and the version it was cut under.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct TableSheet {
    version: Option<String>,
    sheet: Sheet,
}

/// One sheet per name table, each with the version it was cut under.
///
/// A sheet's value is the hash its entry names. The [`NameTable::Chunks`] sheet holds only the
/// chunks of the installation it was cut for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameSheets {
    tables: [TableSheet; 5],
}

impl NameSheets {
    /// Cuts every table's sheet from `source`.
    pub fn build(source: &dyn NameSource, game: &GameIndex) -> Self {
        let mut sheets = Self::default();
        for table in NameTable::ALL {
            sheets.tables[table.ordinal()] = cut(source, table, game);
        }
        sheets
    }

    /// The cached sheets, with every table whose cached version differs from the source's,
    /// or whose version is `None`, recut.
    ///
    /// A cache that does not load is an empty set, logged at debug for a missing file and at
    /// warn otherwise. A set with a recut table whose version is not `None` is saved
    /// best-effort, with a warn on failure. A table whose version is `None` is saved empty and
    /// writes nothing on its own.
    pub fn load_or_build(cache_path: &Utf8Path, source: &dyn NameSource, game: &GameIndex) -> Self {
        let mut sheets = match Self::load(cache_path) {
            Ok(sheets) => sheets,
            Err(error) if error.is_missing_file() => {
                tracing::debug!("No name sheet cache at {cache_path}");
                Self::default()
            }
            Err(error) => {
                tracing::warn!("Recutting name sheets: {error}");
                Self::default()
            }
        };
        let mut recut = Vec::new();
        let mut changed = false;
        for table in NameTable::ALL {
            let version = version(source, table, game);
            let cached = &sheets.tables[table.ordinal()];
            if version.is_some() && cached.version == version {
                continue;
            }
            changed |= version.is_some() || cached.version.is_some();
            sheets.tables[table.ordinal()] = cut(source, table, game);
            recut.push(table);
        }
        if recut.is_empty() {
            tracing::info!("Name sheets loaded from {cache_path}");
        } else {
            tracing::info!(?recut, "Name sheets recut");
        }
        if changed && let Err(error) = sheets.save(cache_path) {
            tracing::warn!("Name sheet cache not saved: {error}");
        }
        sheets
    }

    /// Reads the cache at `cache_path`.
    ///
    /// # Errors
    ///
    /// [`CacheError::Read`], [`CacheError::Decode`], and [`CacheError::Version`] for a format
    /// version other than [`NAME_CACHE_FORMAT_VERSION`].
    pub fn load(cache_path: &Utf8Path) -> Result<Self, CacheError> {
        let cache::Document { body, .. } =
            cache::read::<Self>(cache_path, NAME_CACHE_FORMAT_VERSION)?;
        Ok(body)
    }

    /// Writes the sheets to `cache_path` atomically. A table whose version is `None` is
    /// written empty.
    ///
    /// # Errors
    ///
    /// [`CacheError::Write`] and [`CacheError::Encode`].
    pub fn save(&self, cache_path: &Utf8Path) -> Result<(), CacheError> {
        #[derive(Serialize)]
        struct SavedTable<'a> {
            version: Option<&'a str>,
            sheet: &'a Sheet,
        }
        #[derive(Serialize)]
        struct Saved<'a> {
            tables: [SavedTable<'a>; 5],
        }
        let empty = Sheet::default();
        let body = Saved {
            tables: NameTable::ALL.map(|table| {
                let cut = &self.tables[table.ordinal()];
                SavedTable {
                    version: cut.version.as_deref(),
                    sheet: match cut.version {
                        Some(_) => &cut.sheet,
                        None => &empty,
                    },
                }
            }),
        };
        cache::write(
            cache_path,
            NAME_CACHE_FORMAT_VERSION,
            Fingerprint::ZERO,
            &body,
        )
    }

    /// The sheet of `table`. Empty for a table the source does not have.
    #[must_use]
    pub fn sheet(&self, table: NameTable) -> &Sheet {
        &self.tables[table.ordinal()].sheet
    }

    /// The version the sheet of `table` was cut under.
    #[must_use]
    pub fn version(&self, table: NameTable) -> Option<&str> {
        self.tables[table.ordinal()].version.as_deref()
    }

    /// Whether every sheet is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tables.iter().all(|table| table.sheet.is_empty())
    }
}

/// The version `table`'s sheet is stored under for `game`.
///
/// The [`NameTable::Chunks`] version carries the game fingerprint. Its sheet holds the
/// installation's chunks alone.
fn version(source: &dyn NameSource, table: NameTable, game: &GameIndex) -> Option<String> {
    let version = source.version(table)?;
    Some(match table {
        NameTable::Chunks => format!("{version}+{}", game.fingerprint()),
        NameTable::Entries | NameTable::Types | NameTable::Fields | NameTable::Hashes => version,
    })
}

/// Cuts `table`'s sheet from `source`.
fn cut(source: &dyn NameSource, table: NameTable, game: &GameIndex) -> TableSheet {
    let mut builder = SheetBuilder::default();
    match table {
        NameTable::Chunks => source.for_each_name(table, &mut |hash, name| {
            if game.contains(WadHash(hash)) {
                builder.push(name, hash);
            }
        }),
        NameTable::Entries | NameTable::Types | NameTable::Fields | NameTable::Hashes => {
            source.for_each_name(table, &mut |hash, name| builder.push(name, hash));
        }
    }
    TableSheet {
        version: version(source, table, game),
        sheet: builder.finish(),
    }
}
