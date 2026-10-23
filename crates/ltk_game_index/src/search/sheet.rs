//! A sheet: entries in one lowercased text buffer with a case mask, searched by substring.

use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;

use memchr::memmem;
use serde::{Deserialize, Serialize, Serializer};

use super::bytes;

/// Entries in one text buffer, lowercased for scanning, each with a `u64` value.
///
/// The buffer holds every entry ASCII-lowercased and followed by `\n`. The case mask holds one
/// bit per byte of the buffer, set where the byte was an ASCII uppercase letter. An entry reads
/// back as it was given through [`Sheet::entry`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(try_from = "RawSheet")]
pub struct Sheet {
    /// Every entry, ASCII-lowercased, each followed by `\n`.
    text: String,
    /// One bit per byte of `text`, least significant bit first.
    cases: Vec<u8>,
    /// The byte offset of each entry's first byte.
    starts: Vec<u32>,
    /// The value of each entry.
    values: Vec<u64>,
}

/// One occurrence of a needle in a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Occurrence {
    /// The entry holding the occurrence.
    pub entry: u32,
    /// The byte offset of the occurrence in the entry.
    pub start: u32,
    /// The byte offset past the occurrence in the entry.
    pub end: u32,
}

impl Sheet {
    /// A sheet of `entries` as `(entry, value)` pairs.
    ///
    /// A repeated entry, compared lowercased, keeps its first value. An empty entry and an
    /// entry holding a newline are dropped.
    pub fn build<S: AsRef<str>>(entries: impl IntoIterator<Item = (S, u64)>) -> Self {
        let mut builder = SheetBuilder::default();
        for (entry, value) in entries {
            builder.push(entry.as_ref(), value);
        }
        builder.finish()
    }

    /// The sheet of a buffer of lowercase entries, each followed by `\n` and starting at its
    /// offset in `starts`. The value of each entry is its index. Nothing is checked.
    pub(super) fn from_lowercase(text: String, starts: Vec<u32>) -> Self {
        Self {
            cases: vec![0; text.len().div_ceil(8)],
            values: (0..starts.len() as u64).collect(),
            text,
            starts,
        }
    }

    /// Appends `entry`, whose lowercase form is `lowered`. Neither is checked.
    pub(super) fn push_unchecked(&mut self, entry: &str, lowered: &str, value: u64) {
        let start = self.text.len();
        self.starts.push(start as u32);
        self.text.push_str(lowered);
        self.text.push('\n');
        self.values.push(value);
        self.cases.resize(self.text.len().div_ceil(8), 0);
        for (offset, byte) in entry.bytes().enumerate() {
            if byte.is_ascii_uppercase() {
                let at = start + offset;
                self.cases[at / 8] |= 1 << (at % 8);
            }
        }
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the sheet holds no entry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The entry at `index` as it was given, without its newline.
    ///
    /// The entry borrows the buffer when it holds no ASCII uppercase letter. An index past the
    /// end is an empty entry.
    #[must_use]
    pub fn entry(&self, index: u32) -> Cow<'_, str> {
        let span = self.span(index);
        let lower = self.text.get(span.clone()).unwrap_or_default();
        if !span.clone().any(|at| self.is_upper(at)) {
            return Cow::Borrowed(lower);
        }
        let entry = lower
            .char_indices()
            .map(|(offset, c)| match self.is_upper(span.start + offset) {
                true => c.to_ascii_uppercase(),
                false => c,
            })
            .collect();
        Cow::Owned(entry)
    }

    /// The entry at `index` lowercased, without its newline. Empty for an index past the end.
    #[must_use]
    pub fn entry_lower(&self, index: u32) -> &str {
        self.text.get(self.span(index)).unwrap_or_default()
    }

    /// The value of the entry at `index`.
    ///
    /// # Panics
    ///
    /// When `index` is past the end.
    #[must_use]
    pub fn value(&self, index: u32) -> u64 {
        self.values[index as usize]
    }

    /// Every value, in entry order.
    #[must_use]
    pub fn values(&self) -> &[u64] {
        &self.values
    }

    /// The lowercased buffer: every entry followed by `\n`.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The entry holding byte `offset` of [`text`](Self::text).
    ///
    /// An offset on an entry's newline is that entry's. An offset past the buffer is the last
    /// entry's.
    #[must_use]
    pub fn entry_at(&self, offset: usize) -> u32 {
        let offset = offset as u32;
        match self.starts.binary_search(&offset) {
            Ok(index) => index as u32,
            Err(index) => index.saturating_sub(1) as u32,
        }
    }

    /// The byte range of entry `index` in [`text`](Self::text), without its newline.
    ///
    /// Empty for an index past the end.
    #[must_use]
    pub fn span(&self, index: u32) -> Range<usize> {
        let index = index as usize;
        let Some(&start) = self.starts.get(index) else {
            return 0..0;
        };
        let end = self
            .starts
            .get(index + 1)
            .map_or(self.text.len(), |&next| next as usize);
        start as usize..end - 1
    }

    /// Every entry whose text contains `needle`, ascending, each once.
    ///
    /// `needle` is lowercased first. An empty needle and a needle holding a newline match
    /// nothing.
    #[must_use]
    ///
    /// With the `rayon` feature, a sheet of more than 1 MiB scans in segments of whole entries
    /// in parallel.
    pub fn scan(&self, needle: &str) -> Vec<u32> {
        let Some(finder) = finder(needle) else {
            return Vec::new();
        };
        #[cfg(feature = "rayon")]
        if self.text.len() > PARALLEL_SCAN_BYTES {
            use rayon::prelude::*;
            let segments = rayon::current_num_threads().max(1) * 4;
            let mut bounds: Vec<u32> = (0..segments)
                .map(|at| self.entry_at(self.text.len() * at / segments))
                .collect();
            bounds.push(self.len() as u32);
            bounds.dedup();
            return bounds
                .par_windows(2)
                .flat_map_iter(|pair| self.scan_entries(&finder, pair[0]..pair[1]))
                .collect();
        }
        self.scan_entries(&finder, 0..self.len() as u32)
    }

    /// Every entry of `entries` holding `finder`'s needle, ascending. One find per entry
    /// holding it; the search resumes at the next entry.
    fn scan_entries(&self, finder: &memmem::Finder<'_>, entries: Range<u32>) -> Vec<u32> {
        if entries.is_empty() {
            return Vec::new();
        }
        let text = self.text.as_bytes();
        let end = self.span(entries.end - 1).end;
        let mut hits = Vec::new();
        let mut from = self.starts[entries.start as usize] as usize;
        let mut entry = entries.start;
        while let Some(found) = text.get(from..end).and_then(|rest| finder.find(rest)) {
            entry = self.entry_after(entry, from + found);
            hits.push(entry);
            from = self.span(entry).end + 1;
        }
        hits
    }

    /// Every occurrence of `needle`, in buffer order. Occurrences in one entry do not overlap.
    ///
    /// `needle` is lowercased first. An empty needle and a needle holding a newline match
    /// nothing.
    pub fn occurrences(&self, needle: &str) -> impl Iterator<Item = Occurrence> + '_ {
        let finder = finder(needle);
        let len = finder.as_ref().map_or(0, |finder| finder.needle().len());
        let mut from = 0;
        let mut entry = 0;
        std::iter::from_fn(move || {
            let finder = finder.as_ref()?;
            let at = from + finder.find(self.text.as_bytes().get(from..)?)?;
            from = at + len;
            entry = self.entry_after(entry, at);
            let start = at - self.starts[entry as usize] as usize;
            Some(Occurrence {
                entry,
                start: start as u32,
                end: (start + len) as u32,
            })
        })
    }

    /// The byte ranges of `finder`'s needle in entry `index`, `[start, end)`, without overlap.
    pub(super) fn ranges(&self, index: u32, finder: &memmem::Finder<'_>) -> Vec<(u32, u32)> {
        let len = finder.needle().len() as u32;
        finder
            .find_iter(self.entry_lower(index).as_bytes())
            .map(|at| (at as u32, at as u32 + len))
            .collect()
    }

    /// The entry holding byte `at` of the buffer, searched forward from entry `from`, which
    /// starts at or before `at`.
    fn entry_after(&self, from: u32, at: usize) -> u32 {
        let at = at as u32;
        let starts = &self.starts[from as usize..];
        let (mut low, mut step) = (0, 1);
        while low + step < starts.len() && starts[low + step] <= at {
            low += step;
            step *= 2;
        }
        let high = (low + step).min(starts.len());
        let within = starts[low..high].partition_point(|&start| start <= at);
        from + (low + within).saturating_sub(1) as u32
    }

    /// Whether byte `at` of the buffer was an ASCII uppercase letter.
    fn is_upper(&self, at: usize) -> bool {
        self.cases
            .get(at / 8)
            .is_some_and(|bits| bits & (1 << (at % 8)) != 0)
    }
}

/// The size above which a sheet scans in parallel with the `rayon` feature.
#[cfg(feature = "rayon")]
const PARALLEL_SCAN_BYTES: usize = 1 << 20;

/// A finder for `needle` lowercased. `None` for an empty needle or one holding a newline.
pub(super) fn finder(needle: &str) -> Option<memmem::Finder<'static>> {
    if needle.is_empty() || needle.contains('\n') {
        return None;
    }
    let lowered = needle.to_ascii_lowercase();
    Some(memmem::Finder::new(lowered.as_bytes()).into_owned())
}

/// Entries pushed one at a time into a [`Sheet`], with [`Sheet::build`]'s rules.
#[derive(Debug, Default)]
pub(super) struct SheetBuilder {
    sheet: Sheet,
    seen: HashSet<Box<str>>,
    lowered: String,
}

impl SheetBuilder {
    /// Appends `entry` unless it is empty, holds a newline, or repeats an entry lowercased.
    pub fn push(&mut self, entry: &str, value: u64) {
        if entry.is_empty() || entry.contains('\n') {
            return;
        }
        self.lowered.clear();
        self.lowered.push_str(entry);
        self.lowered.make_ascii_lowercase();
        if self.seen.contains(self.lowered.as_str()) {
            return;
        }
        self.seen.insert(self.lowered.as_str().into());
        self.sheet.push_unchecked(entry, &self.lowered, value);
    }

    pub fn finish(self) -> Sheet {
        self.sheet
    }
}

impl Serialize for Sheet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct RawSheetRef<'a> {
            #[serde(with = "bytes")]
            text: &'a [u8],
            #[serde(with = "bytes")]
            cases: &'a [u8],
            #[serde(with = "bytes")]
            starts: &'a [u32],
            #[serde(with = "bytes")]
            values: &'a [u64],
        }
        RawSheetRef {
            text: self.text.as_bytes(),
            cases: &self.cases,
            starts: &self.starts,
            values: &self.values,
        }
        .serialize(serializer)
    }
}

/// The serialized form of a [`Sheet`]: every array as one byte string.
#[derive(Serialize, Deserialize)]
struct RawSheet {
    #[serde(with = "bytes")]
    text: Vec<u8>,
    #[serde(with = "bytes")]
    cases: Vec<u8>,
    #[serde(with = "bytes")]
    starts: Vec<u32>,
    #[serde(with = "bytes")]
    values: Vec<u64>,
}

impl From<Sheet> for RawSheet {
    fn from(sheet: Sheet) -> Self {
        Self {
            text: sheet.text.into_bytes(),
            cases: sheet.cases,
            starts: sheet.starts,
            values: sheet.values,
        }
    }
}

impl TryFrom<RawSheet> for Sheet {
    type Error = String;

    fn try_from(raw: RawSheet) -> Result<Self, Self::Error> {
        let text = String::from_utf8(raw.text).map_err(|e| format!("sheet text: {e}"))?;
        if raw.starts.len() != raw.values.len() {
            return Err(format!(
                "sheet has {} entries and {} values",
                raw.starts.len(),
                raw.values.len()
            ));
        }
        if raw.cases.len() != text.len().div_ceil(8) {
            return Err(format!(
                "sheet case mask is {} bytes for {} bytes of text",
                raw.cases.len(),
                text.len()
            ));
        }
        let bytes = text.as_bytes();
        let mut expected = 0usize;
        for &start in &raw.starts {
            if start as usize != expected {
                return Err(format!(
                    "sheet entry starts at {start}, expected {expected}"
                ));
            }
            let Some(newline) = memchr::memchr(b'\n', &bytes[expected..]) else {
                return Err(format!("sheet entry at {start} has no newline"));
            };
            expected += newline + 1;
        }
        if expected != bytes.len() {
            return Err(format!(
                "sheet text runs to {} bytes, entries end at {expected}",
                bytes.len()
            ));
        }
        Ok(Self {
            text,
            cases: raw.cases,
            starts: raw.starts,
            values: raw.values,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Sheet {
        Sheet::build([
            ("Characters/Aatrox/Skins/Skin0", 1),
            ("ASSETS/Characters/Aatrox/aatrox.skn", 2),
            ("characters/ahri", 3),
            ("Characters/Ahri", 4),
            ("", 5),
            ("two\nlines", 6),
        ])
    }

    #[test]
    fn a_repeated_entry_keeps_its_first_value_and_bad_entries_are_dropped() {
        let s = sheet();
        assert_eq!(s.len(), 3);
        assert_eq!(s.value(2), 3);
        assert_eq!(s.values(), [1, 2, 3]);
    }

    #[test]
    fn an_entry_reads_back_as_written_and_borrows_when_lowercase() {
        let s = sheet();
        assert_eq!(s.entry(0), "Characters/Aatrox/Skins/Skin0");
        assert_eq!(s.entry(1), "ASSETS/Characters/Aatrox/aatrox.skn");
        assert!(matches!(s.entry(2), Cow::Borrowed("characters/ahri")));
        assert_eq!(s.entry_lower(0), "characters/aatrox/skins/skin0");
        assert_eq!(s.entry(9), "");
    }

    #[test]
    fn a_scan_returns_every_containing_entry_once() {
        let s = sheet();
        assert_eq!(s.scan("AATROX"), [0, 1]);
        assert_eq!(s.scan("a"), [0, 1, 2]);
        assert_eq!(s.scan("ahri"), [2]);
        assert!(s.scan("").is_empty());
    }

    #[test]
    fn a_needle_spanning_an_entry_boundary_matches_nothing() {
        let s = sheet();
        assert!(s.scan("skin0assets").is_empty());
        assert!(s.scan("0\na").is_empty());
    }

    #[test]
    fn occurrences_report_offsets_into_the_entry() {
        let s = sheet();
        let found: Vec<(u32, u32, u32)> = s
            .occurrences("aatrox")
            .map(|o| (o.entry, o.start, o.end))
            .collect();
        assert_eq!(found, [(0, 11, 17), (1, 18, 24), (1, 25, 31)]);
        let entry = s.entry(1);
        assert_eq!(&entry[18..24], "Aatrox");
        assert_eq!(&entry[25..31], "aatrox");
        assert_eq!(s.occurrences("aa").filter(|o| o.entry == 0).count(), 1);
    }

    #[test]
    fn scan_and_occurrences_agree_with_a_naive_search_over_many_entries() {
        let entries: Vec<String> = (0..500u32)
            .map(|n| {
                format!(
                    "Dir{}/A{}a/{}",
                    n % 7,
                    n.wrapping_mul(2_654_435_761) % 97,
                    n
                )
            })
            .collect();
        let s = Sheet::build(entries.iter().map(|e| (e.as_str(), 0)));
        for needle in ["a", "1a", "dir3/", "9", "a/1", "aa"] {
            let lower: Vec<String> = (0..s.len() as u32)
                .map(|i| s.entry_lower(i).to_owned())
                .collect();
            let naive: Vec<u32> = (0..s.len() as u32)
                .filter(|&i| lower[i as usize].contains(needle))
                .collect();
            assert_eq!(s.scan(needle), naive, "{needle}");
            let finder = finder(needle).unwrap();
            let mut expected = Vec::new();
            for &i in &naive {
                for (start, end) in s.ranges(i, &finder) {
                    assert_eq!(&lower[i as usize][start as usize..end as usize], needle);
                    expected.push((i, start, end));
                }
            }
            let found: Vec<(u32, u32, u32)> = s
                .occurrences(needle)
                .map(|o| (o.entry, o.start, o.end))
                .collect();
            assert_eq!(found, expected, "{needle}");
        }
    }

    #[test]
    fn entry_at_and_span_agree() {
        let s = sheet();
        for index in 0..s.len() as u32 {
            let span = s.span(index);
            assert_eq!(&s.text()[span.clone()], s.entry_lower(index));
            assert_eq!(s.entry_at(span.start), index);
            assert_eq!(s.entry_at(span.end), index);
        }
        assert_eq!(s.span(3), 0..0);
    }

    #[test]
    fn a_non_ascii_entry_is_kept_and_matched_as_given() {
        let s = Sheet::build([("Ünïcode/Name", 1)]);
        assert_eq!(s.entry(0), "Ünïcode/Name");
        assert_eq!(s.scan("Ünïcode/NAME"), [0]);
        assert!(s.scan("ünïcode").is_empty());
    }

    #[test]
    fn a_sheet_round_trips_through_its_cache_form() {
        let s = sheet();
        let bytes = rmp_serde::to_vec(&s).unwrap();
        let loaded: Sheet = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(loaded, s);
        assert_eq!(loaded.entry(0), "Characters/Aatrox/Skins/Skin0");
    }

    #[test]
    fn a_sheet_whose_starts_disagree_with_its_text_does_not_load() {
        let mut raw = RawSheet::from(sheet());
        raw.starts[1] += 1;
        let bytes = rmp_serde::to_vec(&raw).unwrap();
        assert!(rmp_serde::from_slice::<Sheet>(&bytes).is_err());
    }
}
