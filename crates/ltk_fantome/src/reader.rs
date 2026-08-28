//! [`FantomeReader`]: reads the entries of a Fantome archive.
//!
//! The reader knows the archive's entry conventions (`WAD/`, `RAW/`,
//! `META/`) and how to unpack a packed WAD it finds, but not what a mod
//! project looks like on disk: turning an archive into a project directory
//! is the caller's job (see `ltk_mod_project`'s `fantome` module).

use std::fmt;
use std::io::{self, Cursor, Read, Seek};

use camino::Utf8Path;
use ltk_wad::{NamingPolicy, NoResolver, PathResolver, Wad, WadExtractor};
use zip::ZipArchive;
use zip::read::ZipFile;

use crate::FantomeInfo;
use crate::error::FantomeExtractError;

/// Reads a Fantome archive entry by entry.
pub struct FantomeReader<R: Read + Seek> {
    archive: ZipArchive<R>,
}

impl<R: Read + Seek> fmt::Debug for FantomeReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FantomeReader")
            .field("entries", &self.archive.len())
            .finish_non_exhaustive()
    }
}

/// One WAD's turn in [`FantomeReader::extract_wads`], reported before the WAD
/// is written.
///
/// `index` counts the WADs [`FantomeReader::wad_names`] lists, in the order it
/// lists them, so a caller that showed that list can point at the entry the
/// extraction has reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WadProgress<'a> {
    /// The WAD's name, as its `WAD/` entry spells it.
    pub name: &'a str,
    /// Which WAD of the archive this is, counting from 0.
    pub index: u32,
    /// How many WADs the archive holds.
    pub total: u32,
}

/// How [`FantomeReader::extract_wads`] unpacks what it finds.
///
/// The defaults extract without naming chunks ([`NoResolver`]), under
/// [`NamingPolicy::Descriptive`], and report nothing.
pub struct WadExtractOptions<'a> {
    resolver: &'a dyn PathResolver,
    naming: NamingPolicy,
    progress: Option<&'a mut dyn FnMut(WadProgress<'_>)>,
    cancelled: Option<&'a dyn Fn() -> bool>,
}

impl<'a> WadExtractOptions<'a> {
    /// Extract with every option at its default.
    pub fn new() -> Self {
        Self::default()
    }

    /// Unpack packed WADs through `resolver` so their chunks come out under
    /// their real paths instead of hex hashes.
    ///
    /// See [`FantomeReader::extract_wads`] for what the archive's own bins
    /// name on top of this.
    #[must_use]
    pub fn with_path_resolver(mut self, resolver: &'a dyn PathResolver) -> Self {
        self.resolver = resolver;
        self
    }

    /// Name the chunks of a packed WAD under `naming` rather than
    /// [`NamingPolicy::Descriptive`].
    ///
    /// [`NamingPolicy::Lossless`] is what a caller extracting a mod to edit
    /// and repack wants: it writes every chunk, where the default drops one
    /// whose path another chunk claimed first.
    #[must_use]
    pub fn with_naming_policy(mut self, naming: NamingPolicy) -> Self {
        self.naming = naming;
        self
    }

    /// Report each WAD to `progress` before it is written.
    #[must_use]
    pub fn with_progress(mut self, progress: &'a mut dyn FnMut(WadProgress<'_>)) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Stop the extraction as soon as `cancelled` answers `true`.
    ///
    /// Checked once per archive entry, so a cancellation lands between files
    /// rather than part-way through one. The output directory then holds
    /// however much had been written, which is the caller's to clean up.
    #[must_use]
    pub fn with_cancellation(mut self, cancelled: &'a dyn Fn() -> bool) -> Self {
        self.cancelled = Some(cancelled);
        self
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.is_some_and(|cancelled| cancelled())
    }

    fn report(&mut self, progress: WadProgress<'_>) {
        if let Some(report) = self.progress.as_mut() {
            report(progress);
        }
    }
}

impl Default for WadExtractOptions<'_> {
    fn default() -> Self {
        Self {
            resolver: &NoResolver,
            naming: NamingPolicy::default(),
            progress: None,
            cancelled: None,
        }
    }
}

impl fmt::Debug for WadExtractOptions<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WadExtractOptions")
            .field("naming", &self.naming)
            .field("has_progress", &self.progress.is_some())
            .field("has_cancellation", &self.cancelled.is_some())
            .finish_non_exhaustive()
    }
}

/// Stream one entry into `sink`, without letting the zip crate check its CRC32.
///
/// Fantome tools in the wild write CRC32 values that do not match the bytes
/// they describe, and the zip crate rejects such an entry with "Invalid
/// checksum" rather than handing the bytes over. The check runs on the trailing
/// EOF `read()`, which `Take(size)` never issues, so reading exactly the
/// declared length gets the content out. A short read still fails, so a
/// genuinely truncated entry is not mistaken for a good one.
///
/// Every read of an entry goes through here or [`read_entry`]: an archive that
/// only some of the reads accept is worse than one none of them do.
fn copy_entry(entry: &mut ZipFile<'_>, sink: &mut impl io::Write) -> io::Result<()> {
    let size = entry.size();
    let copied = io::copy(&mut entry.take(size), sink)?;
    if copied != size {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("Archive entry is truncated: {size} bytes declared, {copied} available"),
        ));
    }

    Ok(())
}

/// Read one entry whole, on the same terms as [`copy_entry`].
///
/// The buffer grows as the copy runs rather than being sized from the entry's
/// header, so a declared length nothing backs cannot ask for the allocation.
fn read_entry(entry: &mut ZipFile<'_>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    copy_entry(entry, &mut bytes)?;
    Ok(bytes)
}

impl<R: Read + Seek> FantomeReader<R> {
    /// Create a reader from anything the archive can be read out of.
    ///
    /// The entry table is checked here, once, so that no later call has to:
    /// every method below joins entry names onto a caller's directory, and an
    /// archive holding one that would land outside it is refused rather than
    /// partly extracted.
    ///
    /// # Errors
    ///
    /// Fails with [`FantomeExtractError::EscapingEntry`] for such an archive,
    /// and with [`FantomeExtractError::Zip`] for one that is not a zip at all.
    pub fn new(reader: R) -> Result<Self, FantomeExtractError> {
        let archive = ZipArchive::new(reader)?;

        if let Some(name) = archive.file_names().find(|name| !is_contained(name)) {
            return Err(FantomeExtractError::EscapingEntry {
                name: name.to_owned(),
            });
        }

        Ok(Self { archive })
    }

    /// Read the mod metadata from `META/info.json`.
    pub fn read_info(&mut self) -> Result<FantomeInfo, FantomeExtractError> {
        // Entry names are matched case-insensitively: archives in the wild
        // spell the directory `META`, `meta` and `Meta`.
        let index = (0..self.archive.len()).find(|i| {
            self.archive
                .by_index(*i)
                .is_ok_and(|file| file.name().eq_ignore_ascii_case("META/info.json"))
        });

        let Some(index) = index else {
            return Err(FantomeExtractError::MissingMetadata);
        };

        let info_content = read_entry(&mut self.archive.by_index(index)?)?;
        let info_content = String::from_utf8(info_content)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Strip UTF-8 BOM if present
        let info_content = info_content.trim_start_matches('\u{feff}').trim();

        if info_content.is_empty() {
            return Err(FantomeExtractError::MissingMetadata);
        }

        let info: FantomeInfo = serde_json::from_str(info_content)?;
        Ok(info)
    }

    /// Read the hashtables the manifest declares - and only those.
    ///
    /// An entry in `META/hashes/` that no manifest entry declares is not a
    /// table and is not read; the manifest is authoritative. An entry whose
    /// declared width is not one a key can have is skipped rather than
    /// refused - it still travels with the archive, it just cannot answer a
    /// lookup here.
    ///
    /// The pairs feed `ltk_hashtable::HashtableSet::build` as they are.
    ///
    /// # Errors
    ///
    /// Returns an error if the archive cannot be read, a declared table file
    /// is missing, or one does not fit the table grammar.
    pub fn read_hashtables(
        &mut self,
    ) -> Result<Vec<(ltk_hashtable::HashtableEntry, ltk_hashtable::Hashtable)>, FantomeExtractError>
    {
        let info = self.read_info()?;
        let mut tables = Vec::new();
        for manifest in &info.hashtables {
            let Some(entry) = manifest.to_entry() else {
                continue;
            };
            // Case-insensitive like every other entry lookup here: archives
            // in the wild spell `META/` in any casing.
            let index = (0..self.archive.len()).find(|i| {
                self.archive
                    .by_index(*i)
                    .is_ok_and(|file| file.name().eq_ignore_ascii_case(&manifest.path))
            });
            let Some(index) = index else {
                return Err(FantomeExtractError::MissingHashtable {
                    path: manifest.path.clone(),
                });
            };
            let content = read_entry(&mut self.archive.by_index(index)?)?;
            let table =
                ltk_hashtable::Hashtable::from_reader(content.as_slice()).map_err(|source| {
                    FantomeExtractError::Hashtable {
                        path: manifest.path.clone(),
                        source,
                    }
                })?;
            tables.push((entry, table));
        }
        Ok(tables)
    }

    /// Read the bytes of the packed WAD stored as the single entry
    /// `WAD/{wad_name}`, or `None` when the archive holds no such entry.
    ///
    /// The name is matched case-insensitively, like every entry lookup here.
    /// A WAD stored as a directory of files has no packed bytes and answers
    /// `None`; its files are what [`classify_entry`] calls
    /// [`WadFile`](FantomeEntry::WadFile). Like every entry read, the stored
    /// CRC32 is deliberately not checked.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be read.
    pub fn read_packed_wad(
        &mut self,
        wad_name: &str,
    ) -> Result<Option<Vec<u8>>, FantomeExtractError> {
        let index = (0..self.archive.len()).find(|i| {
            self.archive.by_index(*i).is_ok_and(|file| {
                matches!(
                    classify_entry(file.name()),
                    Some(FantomeEntry::PackedWad(name)) if name.eq_ignore_ascii_case(wad_name)
                )
            })
        });
        let Some(index) = index else {
            return Ok(None);
        };
        Ok(Some(read_entry(&mut self.archive.by_index(index)?)?))
    }

    /// How many entries the archive holds, directory records included.
    pub(crate) fn entry_count(&self) -> usize {
        self.archive.len()
    }

    /// The inner zip archive, for the rewrite's raw entry copies.
    pub(crate) fn zip_archive_mut(&mut self) -> &mut ZipArchive<R> {
        &mut self.archive
    }

    /// The archive's entry names, in archive order.
    ///
    /// Only the entry table is read, so this costs no decompression. Pair it
    /// with [`classify_entry`] to see where the entries would land without
    /// extracting them; the directory entries among them classify as `None`,
    /// so the pairing yields the files alone.
    pub fn entry_names(&self) -> impl Iterator<Item = &str> {
        self.archive.file_names()
    }

    /// The WADs the archive's `WAD/` entries describe, in archive order.
    ///
    /// A packed WAD stored as a single entry and a WAD stored as a directory
    /// of its files both appear once, under the same name. Only entry names
    /// are read, so this costs no decompression: it is what a caller lists
    /// before deciding to import, and where the counters in [`WadProgress`]
    /// come from.
    ///
    /// A WAD is listed when the archive holds a file for it. One named by a
    /// directory record alone is not listed, because there is nothing under it
    /// to unpack.
    pub fn wad_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();

        for entry_name in self.archive.file_names() {
            // Through `classify_entry`, so this and `extract_wads` cannot
            // disagree about what the archive holds: a WAD present only as a
            // directory record has no files to unpack, and listing it would
            // promise a unit no extraction ever reports.
            let relative_path = match classify_entry(entry_name) {
                Some(FantomeEntry::PackedWad(name)) => name,
                Some(FantomeEntry::WadFile(relative_path)) => relative_path,
                _ => continue,
            };
            let Some(wad_name) = wad_name_of(relative_path) else {
                continue;
            };
            if !names.iter().any(|name| name == wad_name) {
                names.push(wad_name.to_owned());
            }
        }

        names
    }

    /// Extract every `WAD/` file into `dest`, preserving the paths beneath
    /// the prefix.
    ///
    /// A packed WAD directly under `WAD/` is unpacked into a directory of its
    /// name rather than written out as a file, naming its chunks through the
    /// resolver [`options`](WadExtractOptions) carry and then through the
    /// WAD's own bins for whatever the resolver could not name. A caller with
    /// no source of names leaves the resolver at its default and gets the bins
    /// alone, which is usually most of a mod. A chunk nothing names keeps its
    /// hash.
    ///
    /// The prefix and the WAD extensions are matched case-insensitively.
    ///
    /// Directories are made as the parents of the files that land in them, so
    /// an entry naming a directory and nothing else leaves nothing behind.
    ///
    /// # Errors
    ///
    /// Besides a malformed archive and a file that could not be written,
    /// fails with [`FantomeExtractError::Cancelled`] when the options carry a
    /// cancellation that answered `true`.
    pub fn extract_wads(
        &mut self,
        dest: &Utf8Path,
        mut options: WadExtractOptions<'_>,
    ) -> Result<(), FantomeExtractError> {
        let wad_names = self.wad_names();
        let total = wad_names.len() as u32;
        let mut reported = vec![false; wad_names.len()];

        for i in 0..self.archive.len() {
            if options.is_cancelled() {
                return Err(FantomeExtractError::Cancelled);
            }

            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            let (relative_path, is_packed) = match classify_entry(&file_name) {
                Some(FantomeEntry::PackedWad(relative_path)) => (relative_path, true),
                Some(FantomeEntry::WadFile(relative_path)) => (relative_path, false),
                _ => continue,
            };

            if let Some(index) = wad_name_of(relative_path)
                .and_then(|wad_name| wad_names.iter().position(|name| name == wad_name))
                && !std::mem::replace(&mut reported[index], true)
            {
                options.report(WadProgress {
                    name: &wad_names[index],
                    index: index as u32,
                    total,
                });
            }

            let output_path = dest.join(relative_path);

            if is_packed {
                extract_packed_wad(&mut file, &output_path, options.resolver, options.naming)?;
            } else {
                extract_entry(&mut file, &output_path)?;
            }
        }

        Ok(())
    }

    /// Extract every `RAW/` file into `dest`, preserving the paths beneath
    /// the prefix.
    ///
    /// The prefix is matched case-insensitively, and directories are made as
    /// the parents of the files that land in them.
    ///
    /// `cancelled` is checked once per archive entry, as
    /// [`extract_wads`](Self::extract_wads) checks its own, so a cancellation
    /// lands between files rather than part-way through one. Pass `None` for an
    /// extraction nothing can stop. A mod carrying most of its content as `RAW/`
    /// entries spends most of an import here, so this is where a caller offering
    /// a cancel button needs it to be read.
    ///
    /// # Errors
    ///
    /// Besides a malformed archive and a file that could not be written, fails
    /// with [`FantomeExtractError::Cancelled`] when `cancelled` answers `true`.
    pub fn extract_raw(
        &mut self,
        dest: &Utf8Path,
        cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<(), FantomeExtractError> {
        for i in 0..self.archive.len() {
            if cancelled.is_some_and(|cancelled| cancelled()) {
                return Err(FantomeExtractError::Cancelled);
            }

            let mut file = self.archive.by_index(i)?;
            let file_name = file.name().to_string();

            let Some(FantomeEntry::Raw(relative_path)) = classify_entry(&file_name) else {
                continue;
            };

            extract_entry(&mut file, &dest.join(relative_path))?;
        }

        Ok(())
    }

    /// Read the `META/README.md` entry, if the archive has one.
    ///
    /// The name is matched case-insensitively, as every `META/` entry is. A
    /// `README.md` at the archive root is read when `META/` holds none, because
    /// tools in the wild write one there and the alternative is dropping the
    /// only prose the archive carries.
    pub fn read_readme(&mut self) -> Result<Option<Vec<u8>>, FantomeExtractError> {
        for entry_name in ["META/README.md", "README.md"] {
            let found = self
                .read_matching_entry(|name| name.eq_ignore_ascii_case(entry_name).then_some(()))?
                .map(|((), bytes)| bytes);
            if found.is_some() {
                return Ok(found);
            }
        }

        Ok(None)
    }

    /// Read the `META/LICENSE*` entry, if the archive has one.
    ///
    /// The entry is matched case-insensitively across the `LICENSE`,
    /// `LICENSE.md` and `LICENSE.txt` spellings; the returned name is the
    /// canonical file name the license should be written to.
    pub fn read_license(&mut self) -> Result<Option<(&'static str, Vec<u8>)>, FantomeExtractError> {
        self.read_matching_entry(|name| match classify_entry(name) {
            Some(FantomeEntry::License(target)) => Some(target),
            _ => None,
        })
    }

    /// Read the `META/image.png` thumbnail entry, if the archive has one.
    ///
    /// The name is matched case-insensitively, as every `META/` entry is. The
    /// bytes are returned as stored; the format keeps thumbnails PNG-encoded.
    pub fn read_image_png(&mut self) -> Result<Option<Vec<u8>>, FantomeExtractError> {
        self.read_matching_entry(|name| name.eq_ignore_ascii_case("META/image.png").then_some(()))
            .map(|found| found.map(|((), bytes)| bytes))
    }

    /// Find the first entry whose name `matcher` accepts and read it whole.
    fn read_matching_entry<T>(
        &mut self,
        matcher: impl Fn(&str) -> Option<T>,
    ) -> Result<Option<(T, Vec<u8>)>, FantomeExtractError> {
        for i in 0..self.archive.len() {
            let mut file = self.archive.by_index(i)?;
            let Some(matched) = matcher(file.name()) else {
                continue;
            };

            return Ok(Some((matched, read_entry(&mut file)?)));
        }

        Ok(None)
    }
}

/// Write one archive entry to `output_path`, creating its parent directories.
fn extract_entry(
    entry: &mut ZipFile<'_>,
    output_path: &Utf8Path,
) -> Result<(), FantomeExtractError> {
    let write_error = |source| FantomeExtractError::write(output_path, source);

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(write_error)?;
    }

    let mut outfile = std::fs::File::create(output_path).map_err(write_error)?;
    copy_entry(entry, &mut outfile).map_err(write_error)?;

    Ok(())
}

/// Where an archive's file entries belong, by their names.
///
/// The format's placement rules in one place: [`FantomeReader`] puts every file
/// it reads where this says, so a caller that has to know where an entry will
/// land before extracting - to preflight a path length limit, say - asks the
/// same question instead of restating the rules and drifting from them.
///
/// Every variant names a file. Directory entries have no variant, because an
/// extraction writes no directory of its own: it makes the parents of the files
/// it writes, and a directory holding no files is one nothing needed.
///
/// Every name is matched case-insensitively, since archives in the wild spell
/// the top-level directories however the tool that wrote them did.
///
/// Deliberately not `#[non_exhaustive]`: mapping every kind of entry onto
/// somewhere is the point of the type, and a caller that has done so wants a
/// compile error if a kind is ever added, not a silent skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FantomeEntry<'a> {
    /// A packed WAD stored as a single entry directly under `WAD/`, which is
    /// unpacked into a directory of this name rather than written as a file.
    PackedWad(&'a str),
    /// A file under `WAD/`, at this path relative to the prefix.
    WadFile(&'a str),
    /// A file under `RAW/`, at this path relative to the prefix.
    Raw(&'a str),
    /// The archive's readme: `META/README.md`, or a `README.md` at the root
    /// when `META/` holds none.
    Readme,
    /// A `META/LICENSE*` entry. The name is the canonical file name its text
    /// should be written to, which preserves the `.md` and `.txt` variants.
    License(&'static str),
    /// `META/image.png`, the archive's thumbnail.
    Image,
    /// `META/info.json`, the archive's metadata.
    Info,
    /// A file under `META/hashes/`, at this path relative to the prefix.
    ///
    /// This is placement, not lookup: it says where the file would land on
    /// disk, which is what a caller preflighting paths needs. Whether the
    /// file is a hashtable is the manifest's to say - only
    /// [`FantomeReader::read_hashtables`], which reads the manifest, ever
    /// produces a table for lookup.
    Hashtable(&'a str),
}

/// Where the file named `entry_name` belongs.
///
/// `None` for an entry the format does not place, which an extraction skips,
/// and `None` for a directory entry: a name ending in a separator is a
/// directory by the zip format's own rule, and calling one a
/// [`WadFile`](FantomeEntry::WadFile) whose path happened to end in `/` gave a
/// caller a destination for something that is not a file. Directories come back
/// as the parents of the files that land in them, so nothing has to place them.
pub fn classify_entry(entry_name: &str) -> Option<FantomeEntry<'_>> {
    if entry_name.ends_with(['/', '\\']) {
        return None;
    }

    if let Some(relative_path) = strip_prefix_ci(entry_name, "WAD/") {
        if relative_path.is_empty() {
            return None;
        }
        return Some(
            if !relative_path.contains('/') && is_wad_file_name(relative_path) {
                FantomeEntry::PackedWad(relative_path)
            } else {
                FantomeEntry::WadFile(relative_path)
            },
        );
    }

    if let Some(relative_path) = strip_prefix_ci(entry_name, "RAW/") {
        return (!relative_path.is_empty()).then_some(FantomeEntry::Raw(relative_path));
    }

    if let Some(relative_path) = strip_prefix_ci(entry_name, "META/hashes/") {
        return (!relative_path.is_empty()).then_some(FantomeEntry::Hashtable(relative_path));
    }

    if let Some(target) = license_entry_target(entry_name) {
        return Some(FantomeEntry::License(target));
    }

    let lowered = entry_name.to_ascii_lowercase();
    match lowered.as_str() {
        "meta/readme.md" | "readme.md" => Some(FantomeEntry::Readme),
        "meta/image.png" => Some(FantomeEntry::Image),
        "meta/info.json" => Some(FantomeEntry::Info),
        _ => None,
    }
}

/// Match a `META/LICENSE*` archive entry case-insensitively and return the file
/// name it should be written to in the extracted project.
///
/// The canonical entry is extensionless `META/LICENSE`, but readers accept the
/// `.md` and `.txt` variants and preserve their extension on the way out.
fn license_entry_target(file_name: &str) -> Option<&'static str> {
    match file_name.to_ascii_lowercase().as_str() {
        "meta/license" => Some("LICENSE"),
        "meta/license.md" => Some("LICENSE.md"),
        "meta/license.txt" => Some("LICENSE.txt"),
        _ => None,
    }
}

/// Whether an entry named `entry_name` stays inside the directory it is
/// extracted to.
///
/// Nothing in the zip format makes an entry name relative: an archive is free
/// to name `../../x` or `/etc/x`, and a reader that joins the name onto its
/// output directory then writes wherever the name says rather than where the
/// caller asked. That is the "zip slip" class of bug, and the only defence is
/// to refuse the name.
///
/// Refused: a `..` component, a name rooted at `/` or `\`, and a name carrying
/// a `:`. Windows reads `\` as a separator and a drive-qualified name like
/// `C:x` relative to that drive rather than to the join, so both are treated as
/// escapes on every platform - an archive one host refuses and another unpacks
/// would be worse than either answer alone.
///
/// A `.` component is kept: it goes nowhere, and tools in the wild write
/// `./WAD/...`.
fn is_contained(entry_name: &str) -> bool {
    !entry_name.starts_with(['/', '\\'])
        && !entry_name.contains(':')
        && entry_name
            .split(['/', '\\'])
            .all(|component| component != "..")
}

/// Strip a leading ASCII prefix case-insensitively, returning the remainder.
///
/// Archives in the wild spell the top-level directories `WAD`, `wad`, `RAW`
/// and `raw`, so an entry is placed by a case-insensitive prefix.
fn strip_prefix_ci<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &name[prefix.len()..])
}

/// The WAD an entry beneath `WAD/` belongs to, if it belongs to one.
///
/// The first path component names it either way: it is the packed WAD itself
/// for `Aatrox.wad.client`, and the WAD a file sits in for
/// `Aatrox.wad.client/data/x.bin`. [`FantomeReader::wad_names`] and the
/// per-WAD progress read the same component, so a listing and an extraction
/// cannot disagree about what the archive holds.
fn wad_name_of(relative_path: &str) -> Option<&str> {
    let head = relative_path.split('/').next()?;
    is_wad_file_name(head).then_some(head)
}

/// Check if a filename looks like a WAD file (ends with .wad.client or similar WAD extensions)
///
/// Matched case-insensitively, for the reason the `WAD/` prefix is: an
/// archive spells its entries however the tool that wrote it did.
fn is_wad_file_name(name: &str) -> bool {
    [".wad.client", ".wad", ".wad.mobile"]
        .iter()
        .any(|extension| ends_with_ci(name, extension))
}

/// Whether `name` ends in `suffix`, compared case-insensitively.
///
/// `get` rather than a slice, so a name whose tail falls mid-character is a
/// non-match rather than a panic.
fn ends_with_ci(name: &str, suffix: &str) -> bool {
    name.len()
        .checked_sub(suffix.len())
        .and_then(|at| name.get(at..))
        .is_some_and(|tail| tail.eq_ignore_ascii_case(suffix))
}

/// Extract a packed WAD file to a directory using WadExtractor
///
/// Name recovery is on. A mod's WAD holds paths the game's own tables never
/// had - the author invented them - and its bins are where those paths are
/// written down, so without the recovery pass those chunks land under their
/// hashes and the project directory is unreadable. The scan is over one small
/// archive, once, at import.
fn extract_packed_wad(
    entry: &mut ZipFile<'_>,
    output_dir: &Utf8Path,
    resolver: &dyn PathResolver,
    naming: NamingPolicy,
) -> Result<(), FantomeExtractError> {
    let cursor = Cursor::new(read_entry(entry)?);
    let mut wad = Wad::mount(cursor)?;

    std::fs::create_dir_all(output_dir)
        .map_err(|source| FantomeExtractError::write(output_dir, source))?;

    WadExtractor::new(resolver)
        .with_naming_policy(naming)
        .with_name_recovery()
        .extract_all(&mut wad, output_dir)?;

    Ok(())
}

#[cfg(test)]
mod tests;
