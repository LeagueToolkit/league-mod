//! Content provider for `.fantome` ZIP archives.
//!
//! Fantome archives only support a single "base" layer. WAD content is stored
//! under the `WAD/` directory, either as:
//! - **Directory WADs**: `WAD/{name}.wad.client/{file}` - individual override files
//! - **Packed WADs**: `WAD/{name}.wad.client` - complete WAD files unpacked in-memory into overrides
//!
//! Raw overrides (game asset paths not pre-organized into WAD directories) are stored
//! under the `RAW/` directory.

use crate::content::{CompressedChunk, ModContentProvider, archive_fingerprint};
use crate::error::{Error, ModContentError, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectAuthor, ModProjectLayer, ModProjectLicense};
use ltk_wad::{Wad, WadChunk, WadHash, is_hex_chunk_path};
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Seek};
use zip::ZipArchive;
use zip::read::ZipFile;

/// Read a ZIP entry's uncompressed bytes, bypassing the zip crate's CRC32 check.
///
/// Some Fantome tools write bad CRC32 values, making `read_to_end` reject the
/// archive with "Invalid checksum". The check only fires on the trailing EOF
/// `read()`, which `Take(size)` never issues. `Take` also caps the read at the
/// declared `uncompressed_size` so a bogus (huge) size can't drive an unbounded
/// allocation - we use `Vec::new()` (not `with_capacity`) for the same reason.
///
/// Integrity is intentionally **not** verified (that's the whole point of this
/// helper). We also do not require the byte count to equal the declared size:
/// some packers over-declare it, and rejecting those would discard data that is
/// fully present and usable. Genuine corruption of the compressed stream still
/// surfaces as a decompression error from `read_to_end`.
fn read_zip_entry_bytes(entry: &mut ZipFile<'_>) -> io::Result<Vec<u8>> {
    let size = entry.size();
    let mut data = Vec::new();

    entry.take(size).read_to_end(&mut data)?;

    Ok(data)
}

/// Pre-computed index of a fantome archive's contents.
///
/// Built once during construction by scanning all ZIP entry names (metadata only,
/// no decompression). All subsequent lookups use this index + `by_name()` for O(1)
/// access instead of linear scans.
struct FantomeIndex {
    /// The exact entry name for META/info.json (case-insensitive match).
    info_entry: Option<String>,
    /// Lowercase WAD name -> [(full_zip_path, relative_path)]. Lowercase keys
    /// make lookups case-insensitive; the stored path keeps the real casing.
    wad_dir_entries: HashMap<String, Vec<(String, String)>>,
    /// Lowercase WAD name -> full_zip_path, for WADs stored as single files.
    packed_wad_paths: HashMap<String, String>,
    /// RAW entries: (full_zip_path, relative_path).
    raw_entries: Vec<(String, String)>,
}

impl FantomeIndex {
    fn build<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Self {
        let mut info_entry = None;
        let mut wad_dir_entries: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut packed_wad_paths: HashMap<String, String> = HashMap::new();
        let mut raw_entries: Vec<(String, String)> = Vec::new();

        for i in 0..archive.len() {
            let Ok(file) = archive.by_index_raw(i) else {
                continue;
            };
            let name = file.name().to_string();
            let is_dir = file.is_dir();
            drop(file);

            // META/info.json (case-insensitive)
            if info_entry.is_none() && name.eq_ignore_ascii_case("META/info.json") {
                info_entry = Some(name.clone());
                continue;
            }

            // WAD/ entries (prefix matched case-insensitively, e.g. `wad/`)
            if let Some(relative) = strip_prefix_ci(&name, "WAD/") {
                if relative.is_empty() || is_dir {
                    continue;
                }

                if !relative.contains('/') && is_wad_file_name(relative) {
                    // Packed WAD file directly under WAD/.
                    let key = relative.to_ascii_lowercase();
                    packed_wad_paths.insert(key, name);
                } else if let Some(wad_name) = relative.split('/').next()
                    && is_wad_file_name(wad_name)
                {
                    let rel = relative
                        .strip_prefix(wad_name)
                        .and_then(|s| s.strip_prefix('/'))
                        .unwrap_or("");
                    if !rel.is_empty() {
                        // Own the key/rel so `name` is free to move below.
                        let key = wad_name.to_ascii_lowercase();
                        let rel = rel.to_string();
                        wad_dir_entries.entry(key).or_default().push((name, rel));
                    }
                }
                continue;
            }

            // RAW/ entries (prefix matched case-insensitively)
            if let Some(relative) = strip_prefix_ci(&name, "RAW/")
                && !relative.is_empty()
                && !is_dir
            {
                let relative = relative.to_string();
                raw_entries.push((name, relative));
            }
        }

        Self {
            info_entry,
            wad_dir_entries,
            packed_wad_paths,
            raw_entries,
        }
    }

    /// All WAD names in the archive, as lowercase keys.
    fn wad_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.wad_dir_entries.keys().cloned().collect();
        for wad_name in self.packed_wad_paths.keys() {
            if !names.contains(wad_name) {
                names.push(wad_name.clone());
            }
        }
        names
    }
}

/// Content provider that reads directly from a `.fantome` ZIP archive.
///
/// Fantome archives only support a single "base" layer. WAD content is stored
/// under the `WAD/` directory, either as:
/// - **Directory WADs**: `WAD/{name}.wad.client/{file}` - individual override files
/// - **Packed WADs**: `WAD/{name}.wad.client` - complete WAD files unpacked in-memory into overrides
pub struct FantomeContent<R: Read + Seek> {
    archive: ZipArchive<R>,
    index: FantomeIndex,
    archive_path: Option<Utf8PathBuf>,
    /// Packed WADs mounted so far, keyed by lowercase WAD name. Filled lazily
    /// by `packed_wad`: the exact-match skip path never needs the bytes, and
    /// eagerly mounting every packed WAD would charge a full archive read to
    /// builds that end up reusing everything.
    packed_wads: HashMap<String, Wad<Cursor<Vec<u8>>>>,
}

impl<R: Read + Seek> FantomeContent<R> {
    pub fn new(reader: R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)?;
        let index = FantomeIndex::build(&mut archive);

        Ok(Self {
            archive,
            index,
            archive_path: None,
            packed_wads: HashMap::new(),
        })
    }

    /// Set the archive file path, enabling content fingerprinting for the metadata cache.
    pub fn with_archive_path(mut self, path: Utf8PathBuf) -> Self {
        self.archive_path = Some(path);
        self
    }

    /// The packed WAD for `wad_key`, mounting it on first access.
    ///
    /// Returns `Ok(None)` when the archive holds no packed WAD under that key.
    /// An entry that exists but cannot be read or mounted is an error.
    fn packed_wad(&mut self, wad_key: &str) -> Result<Option<&mut Wad<Cursor<Vec<u8>>>>> {
        if !self.packed_wads.contains_key(wad_key) {
            let Some(zip_path) = self.index.packed_wad_paths.get(wad_key) else {
                return Ok(None);
            };

            let mut entry = self
                .archive
                .by_name(zip_path)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            let wad_data = read_zip_entry_bytes(&mut entry)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            drop(entry);

            let wad = Wad::mount(Cursor::new(wad_data))?;
            self.packed_wads.insert(wad_key.to_string(), wad);
        }

        Ok(self.packed_wads.get_mut(wad_key))
    }

    /// The TOC entry `rel_path` names inside this archive's packed WAD, if any.
    ///
    /// Packed chunks are addressed by the hex hash their filename spells, which
    /// is how [`read_wad_overrides`](ModContentProvider::read_wad_overrides)
    /// surfaces them. `None` covers every way the archive can fail to name one:
    /// a directory-style entry, a WAD it holds no packed copy of, or a hash that
    /// WAD does not carry.
    fn packed_chunk(&mut self, wad_key: &str, rel_path: &Utf8Path) -> Result<Option<WadChunk>> {
        if !is_hex_chunk_path(rel_path) {
            return Ok(None);
        }
        let Ok(path_hash) = WadHash::from_str_radix(rel_path.file_stem().unwrap_or(""), 16) else {
            return Ok(None);
        };
        let Some(wad) = self.packed_wad(wad_key)? else {
            return Ok(None);
        };

        Ok(wad.chunks().get(path_hash).copied())
    }
}

impl<R: Read + Seek + Send + Sync> ModContentProvider for FantomeContent<R> {
    fn mod_project(&mut self) -> Result<ModProject> {
        let info_name = self
            .index
            .info_entry
            .as_ref()
            .ok_or(ModContentError::FantomeInfoMissing)?;

        let mut info_file = self
            .archive
            .by_name(info_name)
            .map_err(|source| Error::archive_entry(info_name.as_str(), source))?;
        let info_bytes = read_zip_entry_bytes(&mut info_file)
            .map_err(|source| Error::archive_entry(info_name.as_str(), source))?;

        // Some Fantome tools write a UTF-8 BOM, which serde_json rejects.
        // Invalid UTF-8 past that point surfaces as a parse error.
        let info_json = info_bytes
            .strip_prefix(b"\xEF\xBB\xBF")
            .unwrap_or(&info_bytes);
        let info: ltk_fantome::FantomeInfo = serde_json::from_slice(info_json)?;

        // Map declared layers so per-layer string overrides survive; fantome WAD
        // content itself is still base-layer only.
        let mut layers: Vec<ModProjectLayer> = info
            .layers
            .iter()
            .map(|(key, layer)| ModProjectLayer {
                name: if layer.name.is_empty() {
                    key.clone()
                } else {
                    layer.name.clone()
                },
                display_name: layer.display_name.clone(),
                priority: layer.priority,
                description: None,
                string_overrides: layer.string_overrides.clone(),
            })
            .collect();
        layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));
        if !layers.iter().any(|l| l.name == "base") {
            layers = ModProjectLayer::default_table()
                .into_iter()
                .chain(layers)
                .collect();
        }

        Ok(ModProject {
            name: slug::slugify(&info.name),
            display_name: info.name,
            version: info.version,
            description: info.description,
            authors: vec![ModProjectAuthor::Name(info.author)],
            license: info.license.map(ModProjectLicense::from),
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
            transformers: Vec::new(),
            layers,
            thumbnail: None,
            hashtables: vec![],
        })
    }

    fn list_layer_wads(&mut self, layer: &str) -> Result<Vec<String>> {
        if layer != "base" {
            return Ok(Vec::new());
        }
        Ok(self.index.wad_names())
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        if layer != "base" {
            return Ok(Vec::new());
        }

        let wad_key = wad_name.to_ascii_lowercase();

        // Try directory-style entries first
        if let Some(entries) = self.index.wad_dir_entries.get(&wad_key) {
            let entry_names: Vec<(String, String)> = entries.clone();
            let mut results = Vec::with_capacity(entry_names.len());

            for (zip_path, rel_path) in &entry_names {
                let mut entry = self
                    .archive
                    .by_name(zip_path)
                    .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
                let bytes = read_zip_entry_bytes(&mut entry)
                    .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
                results.push((Utf8PathBuf::from(rel_path), bytes));
            }

            return Ok(results);
        }

        // Try packed WAD - extract all chunks as hex-hash files.
        if let Some(wad) = self.packed_wad(&wad_key)? {
            // The entries are copied out whole rather than re-looked-up by hash:
            // loading a chunk needs `&mut wad`, and carrying the entry is what
            // keeps "the TOC listed it but does not hold it" unrepresentable.
            let chunks: Vec<WadChunk> = wad.chunks().iter().copied().collect();
            let mut results = Vec::with_capacity(chunks.len());

            for chunk in chunks {
                let bytes = wad.load_chunk_decompressed(&chunk)?.to_vec();
                let hex_name = format!("{:016x}.bin", chunk.path_hash);
                results.push((Utf8PathBuf::from(hex_name), bytes));
            }

            return Ok(results);
        }

        Ok(Vec::new())
    }

    fn visit_wad_override(
        &mut self,
        layer: &str,
        wad_name: &str,
        visit: &mut dyn FnMut(Utf8PathBuf, Vec<u8>) -> Result<()>,
    ) -> Result<()> {
        if layer != "base" {
            return Ok(());
        }

        let wad_key = wad_name.to_ascii_lowercase();

        if let Some(entries) = self.index.wad_dir_entries.get(&wad_key) {
            let entry_names: Vec<(String, String)> = entries.clone();
            for (zip_path, rel_path) in &entry_names {
                let mut entry = self
                    .archive
                    .by_name(zip_path)
                    .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
                let bytes = read_zip_entry_bytes(&mut entry)
                    .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
                drop(entry);
                visit(Utf8PathBuf::from(rel_path), bytes)?;
            }
            return Ok(());
        }

        if let Some(wad) = self.packed_wad(&wad_key)? {
            // One chunk's decompressed bytes live at a time - the whole reason
            // this method exists next to the bulk read.
            let chunks: Vec<WadChunk> = wad.chunks().iter().copied().collect();
            for chunk in chunks {
                // Re-borrow per iteration: `visit` must not run under the
                // `&mut Wad` borrow, and the map entry is stable once mounted.
                let wad = self
                    .packed_wads
                    .get_mut(&wad_key)
                    .expect("packed WAD mounted above");
                let bytes = wad.load_chunk_decompressed(&chunk)?.to_vec();
                let hex_name = format!("{:016x}.bin", chunk.path_hash);
                visit(Utf8PathBuf::from(hex_name), bytes)?;
            }
        }

        Ok(())
    }

    fn visit_raw_override(
        &mut self,
        visit: &mut dyn FnMut(Utf8PathBuf, Vec<u8>) -> Result<()>,
    ) -> Result<()> {
        let entries: Vec<(String, String)> = self.index.raw_entries.clone();
        for (zip_path, rel_path) in &entries {
            let mut entry = self
                .archive
                .by_name(zip_path)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            let bytes = read_zip_entry_bytes(&mut entry)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            drop(entry);
            visit(Utf8PathBuf::from(rel_path), bytes)?;
        }
        Ok(())
    }

    fn read_raw_overrides(&mut self) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        let entries: Vec<(String, String)> = self.index.raw_entries.clone();
        let mut results = Vec::with_capacity(entries.len());

        for (zip_path, rel_path) in &entries {
            let mut entry = self
                .archive
                .by_name(zip_path)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            let bytes = read_zip_entry_bytes(&mut entry)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            results.push((Utf8PathBuf::from(rel_path), bytes));
        }

        Ok(results)
    }

    fn read_wad_override_file(
        &mut self,
        layer: &str,
        wad_name: &str,
        rel_path: &Utf8Path,
    ) -> Result<Vec<u8>> {
        if layer != "base" {
            return Err(ModContentError::FantomeLayerUnsupported {
                layer: layer.to_string(),
            }
            .into());
        }

        let wad_key = wad_name.to_ascii_lowercase();

        // Look up the stored entry path rather than reconstructing it, since the
        // archive's real casing (e.g. a lowercase `wad/` folder) may differ.
        let want = rel_path.as_str().replace('\\', "/");
        let zip_path = self
            .index
            .wad_dir_entries
            .get(&wad_key)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|(_, rel)| rel.replace('\\', "/") == want)
                    .map(|(zip_path, _)| zip_path.clone())
            });
        if let Some(zip_path) = zip_path {
            let mut entry = self
                .archive
                .by_name(&zip_path)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            let bytes = read_zip_entry_bytes(&mut entry)
                .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
            return Ok(bytes);
        }

        // Try packed WAD - extract specific chunk by hex hash filename
        if is_hex_chunk_path(rel_path)
            && let Ok(target_hash) = WadHash::from_str_radix(rel_path.file_stem().unwrap_or(""), 16)
            && let Some(wad) = self.packed_wad(&wad_key)?
        {
            let chunk =
                *wad.chunks()
                    .get(target_hash)
                    .ok_or(ModContentError::PackedChunkMissing {
                        path_hash: target_hash,
                    })?;
            return Ok(wad.load_chunk_decompressed(&chunk)?.to_vec());
        }

        Err(ModContentError::FantomeOverrideMissing {
            wad_name: wad_name.to_string(),
            rel_path: rel_path.to_path_buf(),
        }
        .into())
    }

    fn read_wad_override_compressed(
        &mut self,
        layer: &str,
        wad_name: &str,
        rel_path: &Utf8Path,
    ) -> Result<Option<CompressedChunk>> {
        if layer != "base" {
            return Ok(None);
        }

        // Only a packed WAD holds chunks in a WAD's stored form; a
        // directory-style entry is a loose file the ZIP holds on its own terms,
        // so there is nothing to copy through.
        let wad_key = wad_name.to_ascii_lowercase();
        let Some(chunk) = self.packed_chunk(&wad_key, rel_path)? else {
            return Ok(None);
        };

        let wad = self
            .packed_wads
            .get_mut(&wad_key)
            .expect("packed WAD mounted above");

        Ok(Some(CompressedChunk {
            compressed: wad.load_chunk_raw(&chunk)?.into_vec(),
            compression: chunk.compression_type,
            uncompressed_size: chunk.uncompressed_size,
            claimed_checksum: chunk.checksum,
        }))
    }

    fn read_raw_override_file(&mut self, rel_path: &Utf8Path) -> Result<Vec<u8>> {
        // Look up the stored entry path (the `RAW/` folder may be cased differently).
        let want = rel_path.as_str().replace('\\', "/");
        let zip_path = self
            .index
            .raw_entries
            .iter()
            .find(|(_, rel)| rel.replace('\\', "/") == want)
            .map(|(zip_path, _)| zip_path.clone());

        let zip_path = zip_path.ok_or_else(|| ModContentError::FantomeRawOverrideMissing {
            rel_path: rel_path.to_path_buf(),
        })?;

        let mut entry = self
            .archive
            .by_name(&zip_path)
            .map_err(|source| Error::archive_entry(zip_path.as_str(), source))?;
        read_zip_entry_bytes(&mut entry)
            .map_err(|source| Error::archive_entry(zip_path.as_str(), source))
    }

    fn content_fingerprint(&self) -> Result<Option<u64>> {
        match &self.archive_path {
            Some(path) => archive_fingerprint(path),
            None => Ok(None),
        }
    }
}

fn is_wad_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".wad.client") || lower.ends_with(".wad") || lower.ends_with(".wad.mobile")
}

/// Strip a leading ASCII prefix case-insensitively, returning the remainder.
fn strip_prefix_ci<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let head = name.get(..prefix.len())?;
    if head.eq_ignore_ascii_case(prefix) {
        Some(&name[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
