//! A name table as a query scans it.

use super::{GameSearch, NameTable, Sheet};
use crate::DeclarationId;

/// The posting key of an entry under a kind no declaration holds the entry's hash as.
const NO_KEY: u32 = u32::MAX;

/// A name table as a query scans it: a sheet, and per entry the posting key of each of the
/// table's kinds.
///
/// An entry whose hash no declaration holds under any of the table's kinds never matches.
/// When such entries are more than a quarter of the sheet's bytes, the scan sheet is a copy
/// holding the other entries alone. Otherwise the table's own sheet is scanned.
#[derive(Debug)]
pub(super) struct ScanTable {
    /// The entries a query scans. `None` for the table's own sheet.
    sheet: Option<Sheet>,
    /// `keys[entry * stride + slot]` is the posting key of the entry's hash under the table's
    /// kind at `slot`, or [`NO_KEY`].
    keys: Vec<u32>,
    stride: usize,
}

impl ScanTable {
    pub(super) fn cut(search: &GameSearch, table: NameTable) -> Self {
        let names = search.names().sheet(table);
        let kinds = table.kinds();
        let stride = kinds.len();
        let terms = search.terms();
        let mut keys = Vec::with_capacity(names.len() * stride);
        let mut live_bytes = 0;
        for entry in 0..names.len() as u32 {
            let hash = names.value(entry);
            let start = keys.len();
            keys.extend(
                kinds
                    .iter()
                    .map(|&kind| terms.hash_key(kind, hash).unwrap_or(NO_KEY)),
            );
            if keys[start..].iter().any(|&key| key != NO_KEY) {
                live_bytes += names.span(entry).len() + 1;
            }
        }
        if live_bytes * 4 >= names.text().len() * 3 {
            return Self {
                sheet: None,
                keys,
                stride,
            };
        }
        let mut sheet = Sheet::default();
        let mut live = Vec::new();
        for (entry, entry_keys) in keys.chunks_exact(stride).enumerate() {
            if entry_keys.iter().all(|&key| key == NO_KEY) {
                continue;
            }
            let entry = entry as u32;
            sheet.push_unchecked(
                &names.entry(entry),
                names.entry_lower(entry),
                names.value(entry),
            );
            live.extend_from_slice(entry_keys);
        }
        Self {
            sheet: Some(sheet),
            keys: live,
            stride,
        }
    }

    /// The sheet a query scans: the copy, or `names`, the table's own sheet.
    pub(super) fn sheet<'a>(&'a self, names: &'a Sheet) -> &'a Sheet {
        self.sheet.as_ref().unwrap_or(names)
    }

    /// The posting of `entry` of the scan sheet under the table's kind at `slot`.
    pub(super) fn posting<'a>(
        &self,
        search: &'a GameSearch,
        entry: u32,
        slot: usize,
    ) -> &'a [DeclarationId] {
        match self.keys[entry as usize * self.stride + slot] {
            NO_KEY => &[],
            key => search.terms().hash_posting(key),
        }
    }
}
