use camino::Utf8PathBuf;
use ltk_mod_project::{default_layers, ModProject, ModProjectAuthor};
use ltk_overlay::content::ModContentProvider;
use ltk_overlay::error::Result;
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
}

impl<R: Read + Seek> FantomeContent<R> {
    pub fn new(reader: R) -> Result<Self> {
        let archive = ZipArchive::new(reader).map_err(|e| {
            ltk_overlay::Error::Other(format!("Failed to open fantome archive: {}", e))
        })?;
        Ok(Self { archive })
    }
}

impl<R: Read + Seek + Send> ModContentProvider for FantomeContent<R> {
    fn mod_project(&mut self) -> Result<ModProject> {
        let mut info_content = String::new();
        let mut found = false;

        for i in 0..self.archive.len() {
            let file = self.archive.by_index(i).map_err(|e| {
                ltk_overlay::Error::Other(format!("Failed to read archive entry: {}", e))
            })?;
            let name = file.name().to_lowercase();
            if name == "meta/info.json" {
                drop(file);
                let mut info_file = self.archive.by_index(i).map_err(|e| {
                    ltk_overlay::Error::Other(format!("Failed to read info.json: {}", e))
                })?;
                info_file.read_to_string(&mut info_content).map_err(|e| {
                    ltk_overlay::Error::Other(format!("Failed to read info.json content: {}", e))
                })?;
                found = true;
                break;
            }
        }

        if !found {
            return Err(ltk_overlay::Error::Other(
                "Missing META/info.json in fantome archive".to_string(),
            ));
        }

        let info_content = info_content.trim_start_matches('\u{feff}').trim();
        let info: ltk_fantome::FantomeInfo = serde_json::from_str(info_content).map_err(|e| {
            ltk_overlay::Error::Other(format!("Failed to parse fantome info.json: {}", e))
        })?;

        Ok(ModProject {
            name: slug::slugify(&info.name),
            display_name: info.name,
            version: info.version,
            description: info.description,
            authors: vec![ModProjectAuthor::Name(info.author)],
            license: None,
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
            let file = self.archive.by_index(i).map_err(|e| {
                ltk_overlay::Error::Other(format!("Failed to read archive entry: {}", e))
            })?;
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
            let file = self.archive.by_index(i).map_err(|e| {
                ltk_overlay::Error::Other(format!("Failed to read archive entry: {}", e))
            })?;
            let name = file.name().to_string();

            if name.starts_with(&dir_prefix) && !file.is_dir() {
                has_dir_entries = true;
                let rel = name.strip_prefix(&dir_prefix).unwrap_or(&name);
                if rel.is_empty() {
                    continue;
                }
                drop(file);

                let mut entry = self.archive.by_index(i).map_err(|e| {
                    ltk_overlay::Error::Other(format!("Failed to read ZIP entry: {}", e))
                })?;
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).map_err(|e| {
                    ltk_overlay::Error::Other(format!("Failed to read ZIP entry data: {}", e))
                })?;
                results.push((Utf8PathBuf::from(rel), bytes));
            }
        }

        if has_dir_entries {
            return Ok(results);
        }

        // No directory entries — try packed WAD file (unpack into individual overrides)
        for i in 0..self.archive.len() {
            let file = self.archive.by_index(i).map_err(|e| {
                ltk_overlay::Error::Other(format!("Failed to read archive entry: {}", e))
            })?;
            let name = file.name().to_string();

            if name == packed_path && !file.is_dir() {
                drop(file);
                return self.read_packed_wad_entries(i);
            }
        }

        Ok(Vec::new())
    }
}

impl<R: Read + Seek + Send> FantomeContent<R> {
    /// Read a packed WAD from the ZIP archive and return its entries as override files.
    ///
    /// Each WAD entry is returned with a hex-hash filename (e.g., "0123456789abcdef.bin")
    /// which the overlay builder's `resolve_chunk_hash` can interpret directly.
    fn read_packed_wad_entries(&mut self, zip_index: usize) -> Result<Vec<(Utf8PathBuf, Vec<u8>)>> {
        let mut entry = self.archive.by_index(zip_index).map_err(|e| {
            ltk_overlay::Error::Other(format!("Failed to read packed WAD from ZIP: {}", e))
        })?;

        let mut wad_data = Vec::new();
        entry.read_to_end(&mut wad_data).map_err(|e| {
            ltk_overlay::Error::Other(format!("Failed to read packed WAD data: {}", e))
        })?;

        let cursor = Cursor::new(wad_data);
        let mut wad = Wad::mount(cursor)?;

        let path_hashes: Vec<u64> = wad.chunks().iter().map(|c| c.path_hash).collect();
        let mut results = Vec::with_capacity(path_hashes.len());

        for path_hash in path_hashes {
            let chunk = *wad.chunks().get(path_hash).ok_or_else(|| {
                ltk_overlay::Error::Other(format!("WAD chunk {:016x} disappeared", path_hash))
            })?;

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
