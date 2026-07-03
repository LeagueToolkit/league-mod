//! Content provider for `.fantome` ZIP archives.
//!
//! Fantome archives only support a single "base" layer. WAD content is stored
//! under the `WAD/` directory, either as:
//! - **Directory WADs**: `WAD/{name}.wad.client/{file}` — individual override files
//! - **Packed WADs**: `WAD/{name}.wad.client` — complete WAD files unpacked in-memory into overrides
//!
//! Raw overrides (game asset paths not pre-organized into WAD directories) are stored
//! under the `RAW/` directory.

use crate::content::{archive_fingerprint, ModContentProvider};
use crate::error::{Error, Result};
use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{default_layers, ModProject, ModProjectAuthor, ModProjectLayer};
use ltk_wad::Wad;
use std::collections::HashMap;
use std::io::{self, Cursor, Read, Seek};
use zip::read::ZipFile;
use zip::ZipArchive;

/// Read a ZIP entry's uncompressed bytes, bypassing the zip crate's CRC32 check.
///
/// Some Fantome tools write bad CRC32 values, making `read_to_end` reject the
/// archive with "Invalid checksum". The check only fires on the trailing EOF
/// `read()`, which `Take(size)` never issues. `Take` also caps the read at the
/// declared `uncompressed_size` so a bogus (huge) size can't drive an unbounded
/// allocation — we use `Vec::new()` (not `with_capacity`) for the same reason.
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
                } else if let Some(wad_name) = relative.split('/').next() {
                    if is_wad_file_name(wad_name) {
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
                }
                continue;
            }

            // RAW/ entries (prefix matched case-insensitively)
            if let Some(relative) = strip_prefix_ci(&name, "RAW/") {
                if !relative.is_empty() && !is_dir {
                    let relative = relative.to_string();
                    raw_entries.push((name, relative));
                }
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
/// - **Directory WADs**: `WAD/{name}.wad.client/{file}` — individual override files
/// - **Packed WADs**: `WAD/{name}.wad.client` — complete WAD files unpacked in-memory into overrides
pub struct FantomeContent<R: Read + Seek> {
    archive: ZipArchive<R>,
    index: FantomeIndex,
    archive_path: Option<Utf8PathBuf>,
    packed_wads: HashMap<String, Wad<Cursor<Vec<u8>>>>,
}

impl<R: Read + Seek> FantomeContent<R> {
    pub fn new(reader: R) -> Result<Self> {
        let mut archive = ZipArchive::new(reader)
            .map_err(|e| Error::Other(format!("Failed to open fantome archive: {}", e)))?;
        let index = FantomeIndex::build(&mut archive);

        // Mount all packed WADs upfront, reading via the stored entry path.
        let mut packed_wads: HashMap<String, Wad<Cursor<Vec<u8>>>> = HashMap::new();
        for (wad_key, zip_path) in &index.packed_wad_paths {
            let mut entry = archive.by_name(zip_path).map_err(|e| {
                Error::Other(format!("Failed to read packed WAD '{}': {}", zip_path, e))
            })?;
            let wad_data = read_zip_entry_bytes(&mut entry).map_err(|e| {
                Error::Other(format!(
                    "Failed to read packed WAD data '{}': {}",
                    zip_path, e
                ))
            })?;

            let wad = Wad::mount(Cursor::new(wad_data))?;
            packed_wads.insert(wad_key.clone(), wad);
        }

        Ok(Self {
            archive,
            index,
            archive_path: None,
            packed_wads,
        })
    }

    /// Set the archive file path, enabling content fingerprinting for the metadata cache.
    pub fn with_archive_path(mut self, path: Utf8PathBuf) -> Self {
        self.archive_path = Some(path);
        self
    }
}

impl<R: Read + Seek + Send + Sync> ModContentProvider for FantomeContent<R> {
    fn mod_project(&mut self) -> Result<ModProject> {
        let info_name =
            self.index.info_entry.as_ref().ok_or_else(|| {
                Error::Other("Missing META/info.json in fantome archive".to_string())
            })?;

        let mut info_file = self
            .archive
            .by_name(info_name)
            .map_err(|e| Error::Other(format!("Failed to read info.json: {}", e)))?;
        let info_bytes = read_zip_entry_bytes(&mut info_file)
            .map_err(|e| Error::Other(format!("Failed to read info.json content: {}", e)))?;
        let info_content = String::from_utf8(info_bytes)
            .map_err(|e| Error::Other(format!("info.json is not valid UTF-8: {}", e)))?;

        let info_content = info_content.trim_start_matches('\u{feff}').trim();
        let info: ltk_fantome::FantomeInfo = serde_json::from_str(info_content)
            .map_err(|e| Error::Other(format!("Failed to parse fantome info.json: {}", e)))?;

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
            layers = default_layers().into_iter().chain(layers).collect();
        }

        Ok(ModProject {
            name: slug::slugify(&info.name),
            display_name: info.name,
            version: info.version,
            description: info.description,
            authors: vec![ModProjectAuthor::Name(info.author)],
            license: None,
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
            transformers: Vec::new(),
            layers,
            thumbnail: None,
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
                    .map_err(|e| Error::Other(format!("Failed to read ZIP entry: {}", e)))?;
                let bytes = read_zip_entry_bytes(&mut entry)
                    .map_err(|e| Error::Other(format!("Failed to read ZIP entry data: {}", e)))?;
                results.push((Utf8PathBuf::from(rel_path), bytes));
            }

            return Ok(results);
        }

        // Try packed WAD — extract all chunks as hex-hash files
        if let Some(wad) = self.packed_wads.get_mut(&wad_key) {
            let path_hashes: Vec<u64> = wad.chunks().iter().map(|c| c.path_hash).collect();
            let mut results = Vec::with_capacity(path_hashes.len());

            for path_hash in path_hashes {
                let chunk = *wad.chunks().get(path_hash).ok_or_else(|| {
                    Error::Other(format!("WAD chunk {:016x} disappeared", path_hash))
                })?;
                let bytes = wad.load_chunk_decompressed(&chunk)?.to_vec();
                let hex_name = format!("{:016x}.bin", path_hash);
                results.push((Utf8PathBuf::from(hex_name), bytes));
            }

            return Ok(results);
        }

        Ok(Vec::new())
    }

    fn read_raw_overrides(&mut self) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        let entries: Vec<(String, String)> = self.index.raw_entries.clone();
        let mut results = Vec::with_capacity(entries.len());

        for (zip_path, rel_path) in &entries {
            let mut entry = self
                .archive
                .by_name(zip_path)
                .map_err(|e| Error::Other(format!("Failed to read RAW ZIP entry: {}", e)))?;
            let bytes = read_zip_entry_bytes(&mut entry)
                .map_err(|e| Error::Other(format!("Failed to read RAW ZIP entry data: {}", e)))?;
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
            return Err(Error::Other(format!(
                "Fantome archives only support 'base' layer, got '{}'",
                layer
            )));
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
                .map_err(|e| Error::Other(format!("Failed to read ZIP entry: {}", e)))?;
            let bytes = read_zip_entry_bytes(&mut entry)
                .map_err(|e| Error::Other(format!("Failed to read ZIP entry data: {}", e)))?;
            return Ok(bytes);
        }

        // Try packed WAD — extract specific chunk by hex hash filename
        if let Some(wad) = self.packed_wads.get_mut(&wad_key) {
            let file_stem = Utf8Path::new(rel_path.file_name().unwrap_or(""))
                .file_stem()
                .unwrap_or("");

            if file_stem.len() == 16 && file_stem.chars().all(|c| c.is_ascii_hexdigit()) {
                if let Ok(target_hash) = u64::from_str_radix(file_stem, 16) {
                    let chunk = *wad.chunks().get(target_hash).ok_or_else(|| {
                        Error::Other(format!(
                            "WAD chunk {:016x} not found in packed WAD",
                            target_hash
                        ))
                    })?;
                    return Ok(wad.load_chunk_decompressed(&chunk)?.to_vec());
                }
            }
        }

        Err(Error::Other(format!(
            "Override file not found in fantome archive: WAD/{}/{}",
            wad_name, rel_path
        )))
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

        let zip_path = zip_path.ok_or_else(|| {
            Error::Other(format!(
                "RAW override file not found in fantome archive: {}",
                rel_path
            ))
        })?;

        let mut entry = self.archive.by_name(&zip_path).map_err(|e| {
            Error::Other(format!(
                "Failed to read RAW ZIP entry '{}': {}",
                zip_path, e
            ))
        })?;
        read_zip_entry_bytes(&mut entry)
            .map_err(|e| Error::Other(format!("Failed to read RAW ZIP entry data: {}", e)))
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
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn make_fantome_zip(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        let mut cursor = zip.finish().unwrap();
        cursor.set_position(0);
        cursor
    }

    /// Build a ZIP (entries are Deflated) and overwrite every CRC32 field with
    /// `0xDEADBEEF`, simulating Fantome creators that emit incorrect CRCs.
    ///
    /// The blind signature scan should hit exactly one local and one central
    /// header per entry; asserting the count catches a signature that spuriously
    /// matched inside compressed data (which would clobber unrelated bytes).
    fn make_fantome_zip_corrupt_crc(entries: &[(&str, &[u8])]) -> Cursor<Vec<u8>> {
        let cursor = make_fantome_zip(entries);
        let mut bytes = cursor.into_inner();

        let mut local_patched = 0usize;
        let mut central_patched = 0usize;
        let mut i = 0usize;
        while i + 4 <= bytes.len() {
            let sig = u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
            match sig {
                // Local file header: CRC32 is at +14
                0x0403_4b50 => {
                    if i + 18 <= bytes.len() {
                        bytes[i + 14..i + 18].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
                        local_patched += 1;
                    }
                    i += 4;
                }
                // Central directory header: CRC32 is at +16
                0x0201_4b50 => {
                    if i + 20 <= bytes.len() {
                        bytes[i + 16..i + 20].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
                        central_patched += 1;
                    }
                    i += 4;
                }
                _ => i += 1,
            }
        }

        assert_eq!(
            local_patched,
            entries.len(),
            "expected exactly one local-header CRC per entry (spurious/missing signature match)"
        );
        assert_eq!(
            central_patched,
            entries.len(),
            "expected exactly one central-header CRC per entry (spurious/missing signature match)"
        );

        Cursor::new(bytes)
    }

    /// Build a minimal in-memory packed WAD containing a single uncompressed
    /// chunk, for exercising the packed-WAD code paths.
    fn make_packed_wad_bytes(payload: &[u8]) -> Vec<u8> {
        use ltk_wad::{WadBuilder, WadChunkBuilder, WadChunkCompression};

        let payload = payload.to_vec();
        let mut cursor = Cursor::new(Vec::new());
        WadBuilder::default()
            .with_chunk(
                WadChunkBuilder::default()
                    .with_path("packed/file.bin")
                    .with_force_compression(WadChunkCompression::None),
            )
            .build_to_writer(&mut cursor, move |_hash, c| {
                c.write_all(&payload)?;
                Ok(())
            })
            .expect("build packed WAD");
        cursor.into_inner()
    }

    fn make_info_json(name: &str) -> Vec<u8> {
        serde_json::to_vec(&ltk_fantome::FantomeInfo {
            name: name.to_string(),
            author: "Author".to_string(),
            version: "1.0.0".to_string(),
            description: "Desc".to_string(),
            tags: Vec::new(),
            champions: Vec::new(),
            maps: Vec::new(),
            layers: std::collections::HashMap::new(),
        })
        .unwrap()
    }

    #[test]
    fn new_with_valid_zip() {
        let cursor = make_fantome_zip(&[("META/info.json", &make_info_json("Test"))]);
        assert!(FantomeContent::new(cursor).is_ok());
    }

    #[test]
    fn new_with_invalid_data() {
        let cursor = Cursor::new(b"not a zip".to_vec());
        assert!(FantomeContent::new(cursor).is_err());
    }

    #[test]
    fn mod_project_reads_info_json() {
        let cursor = make_fantome_zip(&[("META/info.json", &make_info_json("My Mod"))]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let project = content.mod_project().unwrap();
        assert_eq!(project.display_name, "My Mod");
        assert_eq!(project.version, "1.0.0");
    }

    #[test]
    fn mod_project_missing_info_json() {
        let cursor = make_fantome_zip(&[("WAD/test.wad.client/file", b"data")]);
        let mut content = FantomeContent::new(cursor).unwrap();
        assert!(content.mod_project().is_err());
    }

    #[test]
    fn mod_project_handles_bom() {
        let info_str = format!(
            "\u{feff}{}",
            serde_json::to_string(&ltk_fantome::FantomeInfo {
                name: "BOM Mod".to_string(),
                author: "Author".to_string(),
                version: "1.0.0".to_string(),
                description: "Desc".to_string(),
                tags: Vec::new(),
                champions: Vec::new(),
                maps: Vec::new(),
                layers: std::collections::HashMap::new(),
            })
            .unwrap()
        );
        let cursor = make_fantome_zip(&[("META/info.json", info_str.as_bytes())]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let project = content.mod_project().unwrap();
        assert_eq!(project.display_name, "BOM Mod");
    }

    #[test]
    fn list_layer_wads_finds_directory_wads() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("WAD/Aatrox.wad.client/file1", b"data1"),
            ("WAD/Aatrox.wad.client/file2", b"data2"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let wads = content.list_layer_wads("base").unwrap();
        assert_eq!(wads.len(), 1);
        // WAD names are canonicalized to lowercase for case-insensitive matching.
        assert!(wads.contains(&"aatrox.wad.client".to_string()));
    }

    #[test]
    fn read_wad_overrides_lowercase_wad_folder() {
        // Some creators package content under a lowercase `wad/` folder. The
        // archive scanner must recognize it case-insensitively, otherwise the
        // mod's entire WAD content is silently dropped and it never loads.
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Lowercase")),
            ("wad/Aatrox.wad.client/file1.bin", b"data1"),
            ("wad/Aatrox.wad.client/sub/file2.bin", b"data2"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();

        let wads = content.list_layer_wads("base").unwrap();
        assert_eq!(wads, vec!["aatrox.wad.client"]);

        let overrides = content
            .read_wad_overrides("base", "Aatrox.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 2);
        let paths: Vec<&str> = overrides.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"file1.bin"));
        assert!(paths.contains(&"sub/file2.bin"));

        // Pass-2 single-file read must also resolve via the lowercase folder.
        let bytes = content
            .read_wad_override_file("base", "aatrox.wad.client", Utf8Path::new("file1.bin"))
            .unwrap();
        assert_eq!(bytes, b"data1");
    }

    #[test]
    fn read_raw_overrides_lowercase_raw_folder() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Lowercase")),
            ("raw/assets/characters/aatrox/skin0.bin", b"raw_data"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();

        let overrides = content.read_raw_overrides().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(
            overrides[0].0.as_str(),
            "assets/characters/aatrox/skin0.bin"
        );
        assert_eq!(overrides[0].1, b"raw_data");

        let bytes = content
            .read_raw_override_file(Utf8Path::new("assets/characters/aatrox/skin0.bin"))
            .unwrap();
        assert_eq!(bytes, b"raw_data");
    }

    #[test]
    fn read_wad_overrides_lowercase_packed_wad_folder() {
        // Packed WAD directly under a lowercase `wad/` folder.
        let wad_bytes = make_packed_wad_bytes(b"packed");
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Lowercase Packed")),
            ("wad/Packed.wad.client", &wad_bytes),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();

        let wads = content.list_layer_wads("base").unwrap();
        assert_eq!(wads, vec!["packed.wad.client"]);

        let overrides = content
            .read_wad_overrides("base", "Packed.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].1, b"packed");
    }

    #[test]
    fn strip_prefix_ci_matches_case_insensitively() {
        assert_eq!(strip_prefix_ci("WAD/foo", "WAD/"), Some("foo"));
        assert_eq!(strip_prefix_ci("wad/foo", "WAD/"), Some("foo"));
        assert_eq!(strip_prefix_ci("Wad/foo", "WAD/"), Some("foo"));
        assert_eq!(strip_prefix_ci("RAW/foo", "RAW/"), Some("foo"));
        assert_eq!(strip_prefix_ci("raw/foo", "RAW/"), Some("foo"));
        assert_eq!(strip_prefix_ci("META/foo", "WAD/"), None);
        assert_eq!(strip_prefix_ci("wa", "WAD/"), None);
    }

    #[test]
    fn list_layer_wads_non_base_returns_empty() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("WAD/Aatrox.wad.client/file1", b"data1"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let wads = content.list_layer_wads("chroma").unwrap();
        assert!(wads.is_empty());
    }

    #[test]
    fn read_wad_overrides_directory_style() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
            ("WAD/Aatrox.wad.client/sub/file2.bin", b"data2"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let overrides = content
            .read_wad_overrides("base", "Aatrox.wad.client")
            .unwrap();
        assert_eq!(overrides.len(), 2);
        let paths: Vec<&str> = overrides.iter().map(|(p, _)| p.as_str()).collect();
        assert!(paths.contains(&"file1.bin"));
        assert!(paths.contains(&"sub/file2.bin"));
    }

    #[test]
    fn read_wad_overrides_non_base_returns_empty() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let overrides = content
            .read_wad_overrides("chroma", "Aatrox.wad.client")
            .unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn read_raw_overrides_from_raw_dir() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("RAW/assets/characters/aatrox/skin0.bin", b"raw_data"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let overrides = content.read_raw_overrides().unwrap();
        assert_eq!(overrides.len(), 1);
        assert_eq!(
            overrides[0].0.as_str(),
            "assets/characters/aatrox/skin0.bin"
        );
        assert_eq!(overrides[0].1, b"raw_data");
    }

    #[test]
    fn read_raw_override_file_single() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("RAW/assets/characters/aatrox/skin0.bin", b"raw_data"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let bytes = content
            .read_raw_override_file(Utf8Path::new("assets/characters/aatrox/skin0.bin"))
            .unwrap();
        assert_eq!(bytes, b"raw_data");
    }

    #[test]
    fn read_wad_override_file_directory_style() {
        let cursor = make_fantome_zip(&[
            ("META/info.json", &make_info_json("Test")),
            ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
        ]);
        let mut content = FantomeContent::new(cursor).unwrap();
        let bytes = content
            .read_wad_override_file("base", "Aatrox.wad.client", Utf8Path::new("file1.bin"))
            .unwrap();
        assert_eq!(bytes, b"data1");
    }

    #[test]
    fn loads_archive_with_bad_crc32() {
        // Some Fantome creators emit incorrect CRC32 values in the ZIP central
        // directory. The zip crate's CRC check would otherwise reject these
        // archives with "Invalid checksum" — verify we tolerate that and read
        // the underlying data correctly.
        let cursor = make_fantome_zip_corrupt_crc(&[
            ("META/info.json", &make_info_json("Bad CRC Mod")),
            ("WAD/Aatrox.wad.client/file1.bin", b"data1"),
            ("RAW/assets/raw1.bin", b"raw_data"),
        ]);
        let mut content = FantomeContent::new(cursor).expect("FantomeContent::new");

        let project = content.mod_project().expect("mod_project");
        assert_eq!(project.display_name, "Bad CRC Mod");

        let overrides = content
            .read_wad_overrides("base", "Aatrox.wad.client")
            .expect("read_wad_overrides");
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].1, b"data1");

        let raw = content.read_raw_overrides().expect("read_raw_overrides");
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].1, b"raw_data");
    }

    #[test]
    fn loads_packed_wad_with_bad_crc32() {
        // A packed WAD is mounted via Wad::mount during FantomeContent::new — the
        // downstream "WAD mounting" path the fix targets. Verify it and the packed
        // branch of read_wad_override_file tolerate a corrupt CRC.
        const PACKED_PAYLOAD: &[u8] = b"packed_payload_bytes";
        let wad_bytes = make_packed_wad_bytes(PACKED_PAYLOAD);
        let cursor = make_fantome_zip_corrupt_crc(&[
            ("META/info.json", &make_info_json("Packed Bad CRC")),
            ("WAD/Packed.wad.client", &wad_bytes),
        ]);
        let mut content = FantomeContent::new(cursor).expect("FantomeContent::new");

        let overrides = content
            .read_wad_overrides("base", "Packed.wad.client")
            .expect("read_wad_overrides");
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].1, PACKED_PAYLOAD);

        // Packed chunks are exposed as hex-hash filenames; round-trip a lookup.
        let hex_name = overrides[0].0.clone();
        let single = content
            .read_wad_override_file("base", "Packed.wad.client", &hex_name)
            .expect("read_wad_override_file");
        assert_eq!(single, PACKED_PAYLOAD);
    }

    #[test]
    fn is_wad_file_name_variants() {
        assert!(is_wad_file_name("test.wad.client"));
        assert!(is_wad_file_name("test.wad"));
        assert!(is_wad_file_name("test.wad.mobile"));
        assert!(!is_wad_file_name("test.txt"));
        assert!(!is_wad_file_name(""));
    }
}
