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
use std::collections::HashSet;
use std::io::{Cursor, Read, Seek};
use zip::ZipArchive;

/// Content provider that reads directly from a `.fantome` ZIP archive.
///
/// Fantome archives only support a single "base" layer. WAD content is stored
/// under the `WAD/` directory, either as:
/// - **Directory WADs**: `WAD/{name}.wad.client/{file}` — individual override files
/// - **Packed WADs**: `WAD/{name}.wad.client` — complete WAD files unpacked in-memory into overrides
pub struct FantomeContent<R: Read + Seek> {
    archive: ZipArchive<R>,
    archive_path: Option<Utf8PathBuf>,
}

impl<R: Read + Seek> FantomeContent<R> {
    pub fn new(reader: R) -> Result<Self> {
        let archive = ZipArchive::new(reader)
            .map_err(|e| Error::Other(format!("Failed to open fantome archive: {}", e)))?;
        Ok(Self {
            archive,
            archive_path: None,
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
        let mut info_content = String::new();
        let mut found = false;

        for i in 0..self.archive.len() {
            let file = self
                .archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Failed to read archive entry: {}", e)))?;
            let name = file.name().to_lowercase();
            if name == "meta/info.json" {
                drop(file);
                let mut info_file = self
                    .archive
                    .by_index(i)
                    .map_err(|e| Error::Other(format!("Failed to read info.json: {}", e)))?;
                info_file.read_to_string(&mut info_content).map_err(|e| {
                    Error::Other(format!("Failed to read info.json content: {}", e))
                })?;
                found = true;
                break;
            }
        }

        if !found {
            return Err(Error::Other(
                "Missing META/info.json in fantome archive".to_string(),
            ));
        }

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

        let mut dir_wads: HashSet<String> = HashSet::new();
        let mut file_wads: HashSet<String> = HashSet::new();

        for i in 0..self.archive.len() {
            let file = self
                .archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Failed to read archive entry: {}", e)))?;
            let name = file.name().to_string();

            let Some(relative) = name.strip_prefix("WAD/") else {
                continue;
            };
            if relative.is_empty() {
                continue;
            }

            if !relative.contains('/') && !file.is_dir() && is_wad_file_name(relative) {
                // Packed WAD file directly under WAD/
                file_wads.insert(relative.to_string());
            } else if let Some(wad_name) = relative.split('/').next() {
                if is_wad_file_name(wad_name) {
                    dir_wads.insert(wad_name.to_string());
                }
            }
        }

        // Merge both sets — packed WADs not already in dir_wads
        let mut wads: Vec<String> = dir_wads.into_iter().collect();
        for wad_name in file_wads {
            if !wads.contains(&wad_name) {
                wads.push(wad_name);
            }
        }

        Ok(wads)
    }

    fn read_wad_overrides(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        if layer != "base" {
            return Ok(Vec::new());
        }

        let dir_prefix = format!("WAD/{}/", wad_name);
        let packed_path = format!("WAD/{}", wad_name);

        // First check if there are directory-style entries
        let mut results = Vec::new();
        let mut has_dir_entries = false;

        for i in 0..self.archive.len() {
            let file = self
                .archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Failed to read archive entry: {}", e)))?;
            let name = file.name().to_string();

            if name.starts_with(&dir_prefix) && !file.is_dir() {
                has_dir_entries = true;
                let rel = name.strip_prefix(&dir_prefix).unwrap_or(&name);
                if rel.is_empty() {
                    continue;
                }
                drop(file);

                let mut entry = self
                    .archive
                    .by_index(i)
                    .map_err(|e| Error::Other(format!("Failed to read ZIP entry: {}", e)))?;
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|e| Error::Other(format!("Failed to read ZIP entry data: {}", e)))?;
                results.push((Utf8PathBuf::from(rel), bytes));
            }
        }

        if has_dir_entries {
            return Ok(results);
        }

        // No directory entries — try packed WAD file (unpack into individual overrides)
        for i in 0..self.archive.len() {
            let file = self
                .archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Failed to read archive entry: {}", e)))?;
            let name = file.name().to_string();

            if name == packed_path && !file.is_dir() {
                drop(file);
                return self.read_packed_wad_entries(i);
            }
        }

        Ok(Vec::new())
    }

    fn read_raw_overrides(&mut self) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        let raw_prefix = "RAW/";
        let mut results = Vec::new();

        for i in 0..self.archive.len() {
            let file = self
                .archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Failed to read archive entry: {}", e)))?;
            let name = file.name().to_string();

            let Some(relative) = name.strip_prefix(raw_prefix) else {
                continue;
            };
            if relative.is_empty() || file.is_dir() {
                continue;
            }

            drop(file);

            let mut entry = self
                .archive
                .by_index(i)
                .map_err(|e| Error::Other(format!("Failed to read RAW ZIP entry: {}", e)))?;
            let mut bytes = Vec::new();
            entry
                .read_to_end(&mut bytes)
                .map_err(|e| Error::Other(format!("Failed to read RAW ZIP entry data: {}", e)))?;
            results.push((Utf8PathBuf::from(relative), bytes));
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
        let file_stem = Utf8Path::new(rel_path.file_name().unwrap_or(""))
            .file_stem()
            .unwrap_or("");

        if file_stem.len() == 16 && file_stem.chars().all(|c| c.is_ascii_hexdigit()) {
            if let Ok(target_hash) = u64::from_str_radix(file_stem, 16) {
                let packed_path = format!("WAD/{}", wad_name);
                if let Ok(mut entry) = self.archive.by_name(&packed_path) {
                    let mut wad_data = Vec::new();
                    entry.read_to_end(&mut wad_data).map_err(|e| {
                        Error::Other(format!("Failed to read packed WAD data: {}", e))
                    })?;

                    let cursor = Cursor::new(wad_data);
                    let mut wad = Wad::mount(cursor)?;
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
        entry.read_to_end(&mut bytes).map_err(|e| {
            Error::Other(format!("Failed to read RAW ZIP entry data: {}", e))
        })?;
        Ok(bytes)
    }

    fn content_fingerprint(&mut self) -> Result<Option<u64>> {
        match &self.archive_path {
            Some(path) => archive_fingerprint(path),
            None => Ok(None),
        }
    }
}

impl<R: Read + Seek + Send> FantomeContent<R> {
    /// Read a packed WAD from the ZIP archive and return its entries as override files.
    ///
    /// Each WAD entry is returned with a hex-hash filename (e.g., "0123456789abcdef.bin")
    /// which the overlay builder's `resolve_chunk_hash` can interpret directly.
    fn read_packed_wad_entries(&mut self, zip_index: usize) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        let mut entry = self
            .archive
            .by_index(zip_index)
            .map_err(|e| Error::Other(format!("Failed to read packed WAD from ZIP: {}", e)))?;

        let mut wad_data = Vec::new();
        entry
            .read_to_end(&mut wad_data)
            .map_err(|e| Error::Other(format!("Failed to read packed WAD data: {}", e)))?;

        let cursor = Cursor::new(wad_data);
        let mut wad = Wad::mount(cursor)?;

        let path_hashes: Vec<u64> = wad.chunks().iter().map(|c| c.path_hash).collect();
        let mut results = Vec::with_capacity(path_hashes.len());

        for path_hash in path_hashes {
            let chunk = *wad
                .chunks()
                .get(path_hash)
                .ok_or_else(|| Error::Other(format!("WAD chunk {:016x} disappeared", path_hash)))?;

            let bytes = wad.load_chunk_decompressed(&chunk)?.to_vec();

            // Use hex hash as filename — resolve_chunk_hash will parse it correctly
            let hex_name = format!("{:016x}.bin", path_hash);
            results.push((Utf8PathBuf::from(hex_name), bytes));
        }

        Ok(results)
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
