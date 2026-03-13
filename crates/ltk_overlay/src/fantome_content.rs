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
use ltk_mod_project::{default_layers, ModProject, ModProjectAuthor};
use ltk_wad::Wad;
use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};
use zip::ZipArchive;

/// Pre-computed index of a fantome archive's contents.
///
/// Built once during construction by scanning all ZIP entry names (metadata only,
/// no decompression). All subsequent lookups use this index + `by_name()` for O(1)
/// access instead of linear scans.
struct FantomeIndex {
    /// The exact entry name for META/info.json (case-insensitive match).
    info_entry: Option<String>,
    /// WAD directory entries: wad_name -> list of (full_zip_path, relative_path).
    wad_dir_entries: HashMap<String, Vec<(String, String)>>,
    /// Packed WAD names (WADs stored as single files, not directories).
    packed_wad_names: Vec<String>,
    /// RAW entries: (full_zip_path, relative_path).
    raw_entries: Vec<(String, String)>,
}

impl FantomeIndex {
    fn build<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Self {
        let mut info_entry = None;
        let mut wad_dir_entries: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut packed_wad_names: Vec<String> = Vec::new();
        let mut raw_entries: Vec<(String, String)> = Vec::new();

        for i in 0..archive.len() {
            let Ok(file) = archive.by_index_raw(i) else {
                continue;
            };
            let name = file.name().to_string();
            let is_dir = file.is_dir();
            drop(file);

            // META/info.json (case-insensitive)
            if info_entry.is_none() && name.to_lowercase() == "meta/info.json" {
                info_entry = Some(name.clone());
                continue;
            }

            // WAD/ entries
            if let Some(relative) = name.strip_prefix("WAD/") {
                if relative.is_empty() || is_dir {
                    continue;
                }

                let relative = relative.to_string();
                if !relative.contains('/') && is_wad_file_name(&relative) {
                    // Packed WAD file directly under WAD/
                    packed_wad_names.push(relative);
                } else if let Some(wad_name) = relative.split('/').next() {
                    if is_wad_file_name(wad_name) {
                        let rel = relative
                            .strip_prefix(wad_name)
                            .and_then(|s| s.strip_prefix('/'))
                            .unwrap_or("");
                        if !rel.is_empty() {
                            let rel = rel.to_string();
                            wad_dir_entries
                                .entry(wad_name.to_string())
                                .or_default()
                                .push((name, rel));
                        }
                    }
                }
                continue;
            }

            // RAW/ entries
            if let Some(relative) = name.strip_prefix("RAW/") {
                if !relative.is_empty() && !is_dir {
                    let relative = relative.to_string();
                    raw_entries.push((name, relative));
                }
            }
        }

        Self {
            info_entry,
            wad_dir_entries,
            packed_wad_names,
            raw_entries,
        }
    }

    fn wad_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.wad_dir_entries.keys().cloned().collect();
        for wad_name in &self.packed_wad_names {
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

        // Mount all packed WADs upfront
        let mut packed_wads: HashMap<String, Wad<Cursor<Vec<u8>>>> = HashMap::new();
        for wad_name in &index.packed_wad_names {
            let zip_path = format!("WAD/{}", wad_name);
            let mut entry = archive.by_name(&zip_path).map_err(|e| {
                Error::Other(format!("Failed to read packed WAD '{}': {}", wad_name, e))
            })?;
            let mut wad_data = Vec::new();
            entry.read_to_end(&mut wad_data).map_err(|e| {
                Error::Other(format!(
                    "Failed to read packed WAD data '{}': {}",
                    wad_name, e
                ))
            })?;

            let wad = Wad::mount(Cursor::new(wad_data))?;
            packed_wads.insert(wad_name.clone(), wad);
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

impl<R: Read + Seek + Send> ModContentProvider for FantomeContent<R> {
    fn mod_project(&mut self) -> Result<ModProject> {
        let info_name =
            self.index.info_entry.as_ref().ok_or_else(|| {
                Error::Other("Missing META/info.json in fantome archive".to_string())
            })?;

        let mut info_content = String::new();
        let mut info_file = self
            .archive
            .by_name(info_name)
            .map_err(|e| Error::Other(format!("Failed to read info.json: {}", e)))?;
        info_file
            .read_to_string(&mut info_content)
            .map_err(|e| Error::Other(format!("Failed to read info.json content: {}", e)))?;

        let info_content = info_content.trim_start_matches('\u{feff}').trim();
        let info: ltk_fantome::FantomeInfo = serde_json::from_str(info_content)
            .map_err(|e| Error::Other(format!("Failed to parse fantome info.json: {}", e)))?;

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
            layers: default_layers(),
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

        // Try directory-style entries first
        if let Some(entries) = self.index.wad_dir_entries.get(wad_name) {
            let entry_names: Vec<(String, String)> = entries.clone();
            let mut results = Vec::with_capacity(entry_names.len());

            for (zip_path, rel_path) in &entry_names {
                let mut entry = self
                    .archive
                    .by_name(zip_path)
                    .map_err(|e| Error::Other(format!("Failed to read ZIP entry: {}", e)))?;
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|e| Error::Other(format!("Failed to read ZIP entry data: {}", e)))?;
                results.push((Utf8PathBuf::from(rel_path), bytes));
            }

            return Ok(results);
        }

        // Try packed WAD — extract all chunks as hex-hash files
        if let Some(wad) = self.packed_wads.get_mut(wad_name) {
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
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
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

        // Try directory-style entry first (O(1) lookup)
        let target_path = format!("WAD/{}/{}", wad_name, rel_path);
        if let Ok(mut entry) = self.archive.by_name(&target_path) {
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| Error::Other(format!("Failed to read ZIP entry data: {}", e)))?;
            return Ok(bytes);
        }

        // Try packed WAD — extract specific chunk by hex hash filename
        if let Some(wad) = self.packed_wads.get_mut(wad_name) {
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
        let target_path = format!("RAW/{}", rel_path);

        let mut entry = self.archive.by_name(&target_path).map_err(|_| {
            Error::Other(format!(
                "RAW override file not found in fantome archive: {}",
                rel_path
            ))
        })?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Other(format!("Failed to read RAW ZIP entry data: {}", e)))?;
        Ok(bytes)
    }

    fn content_fingerprint(&mut self) -> Result<Option<u64>> {
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
        assert!(wads.contains(&"Aatrox.wad.client".to_string()));
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
    fn is_wad_file_name_variants() {
        assert!(is_wad_file_name("test.wad.client"));
        assert!(is_wad_file_name("test.wad"));
        assert!(is_wad_file_name("test.wad.mobile"));
        assert!(!is_wad_file_name("test.txt"));
        assert!(!is_wad_file_name(""));
    }
}
