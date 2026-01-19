use crate::error::{AppError, AppResult};
use crate::mods::get_enabled_mods_for_overlay;
use crate::state::{get_app_data_dir, Settings};
use byteorder::{WriteBytesExt, LE};
use ltk_wad::{Wad, WadChunk, WadChunkCompression};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use xxhash_rust::xxh3::xxh3_64;

/// Progress event emitted during overlay building.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayProgress {
    pub stage: String,
    pub current_file: Option<String>,
    pub current: u32,
    pub total: u32,
}

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverlayState {
    version: u32,
    enabled_mods: Vec<String>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            version: 2,
            enabled_mods: Vec::new(),
        }
    }
}

/// Ensure the overlay exists and is up-to-date for the current enabled mod set.
///
/// Returns the overlay root directory (the prefix passed to the legacy patcher).
pub fn ensure_overlay(app_handle: &AppHandle, settings: &Settings) -> AppResult<PathBuf> {
    let storage_dir = resolve_storage_dir(app_handle, settings)?;
    let overlay_root = storage_dir.join("overlay");
    let overlay_state_path = overlay_root.join("overlay.json");

    tracing::info!("Overlay: storage_dir={}", storage_dir.display());
    tracing::info!("Overlay: overlay_root={}", overlay_root.display());

    let enabled_mods = get_enabled_mods_for_overlay(app_handle, settings)?;
    let enabled_ids = enabled_mods
        .iter()
        .map(|m| m.id.clone())
        .collect::<Vec<_>>();
    tracing::info!(
        "Overlay: enabled_mods={} ids=[{}]",
        enabled_ids.len(),
        enabled_ids.join(", ")
    );

    let mut should_rebuild = true;
    if overlay_state_path.exists() {
        if let Ok(contents) = fs::read_to_string(&overlay_state_path) {
            if let Ok(state) = serde_json::from_str::<OverlayState>(&contents) {
                if state.version == 2 && state.enabled_mods == enabled_ids {
                    // Only reuse if there are actually overlay outputs present and they look valid.
                    match overlay_outputs_valid(&overlay_root) {
                        Ok(true) => {
                            tracing::info!(
                                "Overlay: reusing existing overlay (enabled mods unchanged)"
                            );
                            should_rebuild = false;
                        }
                        Ok(false) => {
                            tracing::info!(
                                "Overlay: overlay state matched but outputs invalid; forcing rebuild"
                            );
                        }
                        Err(e) => {
                            tracing::info!(
                                "Overlay: overlay state matched but validation failed ({}); forcing rebuild",
                                e
                            );
                        }
                    }
                } else if state.version != 2 && state.enabled_mods == enabled_ids {
                    tracing::info!(
                        "Overlay: overlay state matched but version changed ({} -> 2); forcing rebuild",
                        state.version
                    );
                }
            }
        }
    }

    if !should_rebuild {
        return Ok(overlay_root);
    }

    tracing::info!("Overlay: rebuilding overlay...");

    // Emit start event
    let _ = app_handle.emit(
        "overlay-progress",
        OverlayProgress {
            stage: "indexing".to_string(),
            current_file: None,
            current: 0,
            total: 0,
        },
    );

    // Clean overlay and rebuild.
    if overlay_root.exists() {
        fs::remove_dir_all(&overlay_root)?;
    }
    fs::create_dir_all(&overlay_root)?;

    let game_dir = resolve_game_dir(settings)?;
    let data_final_dir = game_dir.join("DATA").join("FINAL");
    tracing::info!("Overlay: game_dir={}", game_dir.display());
    tracing::info!("Overlay: data_final_dir={}", data_final_dir.display());
    if !data_final_dir.exists() {
        return Err(AppError::ValidationFailed(format!(
            "League path does not contain Game/DATA/FINAL. Resolved game_dir='{}'",
            game_dir.display()
        )));
    }

    // Index all FINAL WADs by filename (case-insensitive).
    let wad_index = build_wad_filename_index(&data_final_dir)?;
    tracing::info!("Overlay: indexed {} wad.client filenames", wad_index.len());

    // Build game-wide hash index: path_hash -> list of WAD relative paths containing it.
    // This enables cross-WAD matching (e.g., champion assets used in Map WADs).
    let game_hash_index = build_game_hash_index(&game_dir, &data_final_dir)?;

    // Collect ALL mod overrides as a flat map: path_hash -> bytes.
    // We collect all overrides first, then distribute to all affected WADs.
    let mut all_overrides: HashMap<u64, Vec<u8>> = HashMap::new();
    // Full WAD replacement files (already-built .wad.client) keyed by target relative game path.
    let mut wad_replacements: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();

    for enabled_mod in &enabled_mods {
        tracing::info!(
            "Overlay: processing mod id={} dir={}",
            enabled_mod.id,
            enabled_mod.mod_dir.display()
        );
        let project = load_mod_project(&enabled_mod.mod_dir)?;
        let mut layers = project.layers.clone();
        layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

        for layer in &layers {
            let layer_dir = enabled_mod.mod_dir.join("content").join(&layer.name);
            if !layer_dir.exists() {
                tracing::debug!(
                    "Overlay: mod={} layer='{}' dir missing, skipping: {}",
                    enabled_mod.id,
                    layer.name,
                    layer_dir.display()
                );
                continue;
            }
            tracing::info!(
                "Overlay: mod={} layer='{}' dir={}",
                enabled_mod.id,
                layer.name,
                layer_dir.display()
            );

            // Each subdirectory ending with .wad.client is a WAD overlay root.
            for entry in fs::read_dir(&layer_dir)? {
                let entry = entry?;
                let wad_path = entry.path();
                let Some(wad_name) = wad_path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !wad_name.to_ascii_lowercase().ends_with(".wad.client") {
                    continue;
                }

                let original_wad_path = resolve_original_wad_path(&wad_index, wad_name)?;
                let relative_game_path = original_wad_path
                    .strip_prefix(&game_dir)
                    .map_err(|_| {
                        AppError::Other(format!(
                            "WAD path is not under Game/: {}",
                            original_wad_path.display()
                        ))
                    })?
                    .to_path_buf();

                tracing::info!(
                    "Overlay: wad='{}' resolved original={} relative={}",
                    wad_name,
                    original_wad_path.display(),
                    relative_game_path.display()
                );

                if wad_path.is_dir() {
                    // Collect overrides into the global override map.
                    let before = all_overrides.len();
                    ingest_wad_dir_overrides(&wad_path, &mut all_overrides)?;
                    let after = all_overrides.len();
                    tracing::info!(
                        "Overlay: wad='{}' overrides added={} total_all_overrides={}",
                        wad_name,
                        after.saturating_sub(before),
                        after
                    );
                } else if wad_path.is_file() {
                    if let Some(prev) =
                        wad_replacements.insert(relative_game_path.clone(), wad_path.clone())
                    {
                        tracing::warn!(
                            "Overlay: wad='{}' replacement overridden by later mod/layer: prev={} new={}",
                            wad_name,
                            prev.display(),
                            wad_path.display()
                        );
                    } else {
                        tracing::info!(
                            "Overlay: wad='{}' using full replacement file={}",
                            wad_name,
                            wad_path.display()
                        );
                    }
                }
            }
        }
    }

    tracing::info!(
        "Overlay: collected {} unique override hashes from all mods",
        all_overrides.len()
    );

    // Write full replacement WADs first.
    for (relative_game_path, src_wad_path) in &wad_replacements {
        let dst_wad_path = overlay_root.join(relative_game_path);
        if let Some(parent) = dst_wad_path.parent() {
            fs::create_dir_all(parent)?;
        }
        tracing::info!(
            "Overlay: copying wad replacement src={} dst={}",
            src_wad_path.display(),
            dst_wad_path.display()
        );
        fs::copy(src_wad_path, &dst_wad_path)?;
    }

    // Distribute overrides to ALL affected WADs using the game hash index.
    // This implements cross-WAD matching like cslol-manager's add_overlay_mod.
    let mut wad_overrides: BTreeMap<PathBuf, HashMap<u64, Vec<u8>>> = BTreeMap::new();
    for (path_hash, override_bytes) in &all_overrides {
        if let Some(wad_paths) = game_hash_index.get(path_hash) {
            for wad_path in wad_paths {
                wad_overrides
                    .entry(wad_path.clone())
                    .or_default()
                    .insert(*path_hash, override_bytes.clone());
            }
        }
    }

    tracing::info!(
        "Overlay: distributed overrides to {} affected WAD files",
        wad_overrides.len()
    );

    // Build patched WADs for all affected game WADs.
    let total_wads = wad_overrides.len() as u32;
    for (idx, (relative_game_path, overrides)) in wad_overrides.into_iter().enumerate() {
        let current_wad = (idx + 1) as u32;
        let wad_name = relative_game_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        // Emit progress event
        let _ = app_handle.emit(
            "overlay-progress",
            OverlayProgress {
                stage: "patching".to_string(),
                current_file: Some(wad_name.to_string()),
                current: current_wad,
                total: total_wads,
            },
        );
        if wad_replacements.contains_key(&relative_game_path) {
            tracing::info!(
                "Overlay: skipping patch build for {} (covered by full replacement)",
                relative_game_path.display()
            );
            continue;
        }
        let src_wad_path = game_dir.join(&relative_game_path);
        let dst_wad_path = overlay_root.join(&relative_game_path);
        if let Some(parent) = dst_wad_path.parent() {
            fs::create_dir_all(parent)?;
        }
        tracing::info!(
            "Overlay: writing patched wad src={} dst={} overrides={}",
            src_wad_path.display(),
            dst_wad_path.display(),
            overrides.len()
        );
        build_patched_wad(&src_wad_path, &dst_wad_path, &overrides)?;
    }

    // Persist overlay state for reuse.
    let state = OverlayState {
        version: 2,
        enabled_mods: enabled_ids,
    };
    fs::write(
        &overlay_state_path,
        serde_json::to_string_pretty(&state).map_err(AppError::from)?,
    )?;

    // Emit completion event
    let _ = app_handle.emit(
        "overlay-progress",
        OverlayProgress {
            stage: "complete".to_string(),
            current_file: None,
            current: total_wads,
            total: total_wads,
        },
    );

    Ok(overlay_root)
}

fn overlay_outputs_valid(overlay_root: &Path) -> AppResult<bool> {
    let data_dir = overlay_root.join("DATA");
    if !data_dir.exists() {
        return Ok(false);
    }

    let mut stack = vec![data_dir];
    let mut wad_files = Vec::new();
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.to_ascii_lowercase().ends_with(".wad.client") {
                wad_files.push(path);
            }
        }
    }

    if wad_files.is_empty() {
        return Ok(false);
    }

    // Sanity check: overlay WADs should be mountable.
    for wad_path in wad_files {
        let file = fs::File::open(&wad_path)?;
        Wad::mount(file).map_err(|e| {
            AppError::Other(format!(
                "Overlay WAD is not mountable '{}': {}",
                wad_path.display(),
                e
            ))
        })?;
    }

    Ok(true)
}

fn resolve_storage_dir(app_handle: &AppHandle, settings: &Settings) -> AppResult<PathBuf> {
    settings
        .mod_storage_path
        .clone()
        .or_else(|| get_app_data_dir(app_handle).map(|d| d.join("mods")))
        .ok_or_else(|| AppError::Other("Failed to resolve mod storage directory".to_string()))
}

fn resolve_game_dir(settings: &Settings) -> AppResult<PathBuf> {
    let league_root = settings
        .league_path
        .clone()
        .ok_or_else(|| AppError::ValidationFailed("League path is not configured".to_string()))?;

    // Users might configure either the install root (…/League of Legends) or the Game dir (…/League of Legends/Game).
    // Accept both, and log what we resolved to in `ensure_overlay`.
    let game_dir = league_root.join("Game");
    if game_dir.exists() {
        return Ok(game_dir);
    }
    if league_root.join("DATA").exists() {
        return Ok(league_root);
    }

    Err(AppError::ValidationFailed(format!(
        "League path does not look like an install root or a Game directory: {}",
        league_root.display()
    )))
}

fn build_wad_filename_index(root: &Path) -> AppResult<HashMap<String, Vec<PathBuf>>> {
    let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }
            index
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push(path);
        }
    }
    Ok(index)
}

/// Build a game-wide index mapping path_hash -> list of (WAD relative path, original chunk).
/// This enables finding ALL WAD files that contain a given chunk hash (cross-WAD matching).
fn build_game_hash_index(
    game_dir: &Path,
    data_final_dir: &Path,
) -> AppResult<HashMap<u64, Vec<PathBuf>>> {
    let mut hash_to_wads: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut wad_count = 0;
    let mut chunk_count = 0;

    let mut stack = vec![data_final_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !name.to_ascii_lowercase().ends_with(".wad.client") {
                continue;
            }

            // Get relative path from game_dir
            let relative_path = match path.strip_prefix(game_dir) {
                Ok(p) => p.to_path_buf(),
                Err(_) => continue,
            };

            // Mount WAD and index all chunk hashes
            let file = match fs::File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("Failed to open WAD '{}': {}", path.display(), e);
                    continue;
                }
            };
            let wad = match Wad::mount(file) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to mount WAD '{}': {}", path.display(), e);
                    continue;
                }
            };

            wad_count += 1;
            for path_hash in wad.chunks().keys() {
                hash_to_wads
                    .entry(*path_hash)
                    .or_default()
                    .push(relative_path.clone());
                chunk_count += 1;
            }
        }
    }

    tracing::info!(
        "Overlay: built game hash index - {} WADs, {} total chunk entries, {} unique hashes",
        wad_count,
        chunk_count,
        hash_to_wads.len()
    );

    Ok(hash_to_wads)
}

fn resolve_original_wad_path(
    wad_index: &HashMap<String, Vec<PathBuf>>,
    wad_name: &str,
) -> AppResult<PathBuf> {
    let key = wad_name.to_ascii_lowercase();
    let Some(candidates) = wad_index.get(&key) else {
        return Err(AppError::Other(format!(
            "Could not find original WAD '{}' under Game/DATA/FINAL",
            wad_name
        )));
    };
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    Err(AppError::Other(format!(
        "Ambiguous WAD '{}': found multiple candidates:\n{}",
        wad_name,
        candidates
            .iter()
            .map(|p| format!(" - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}

fn ingest_wad_dir_overrides(wad_dir: &Path, out: &mut HashMap<u64, Vec<u8>>) -> AppResult<()> {
    let mut stack = vec![wad_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path.strip_prefix(wad_dir).unwrap_or(&path);
            let bytes = fs::read(&path)?;
            let path_hash = resolve_chunk_hash(rel, &bytes)?;
            out.insert(path_hash, bytes);
        }
    }
    Ok(())
}

fn resolve_chunk_hash(rel_path: &Path, bytes: &[u8]) -> AppResult<u64> {
    let file_name = rel_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let file_stem = Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    // If this is a hex-hash filename (as emitted by HexPathResolver), use it directly.
    if file_stem.len() == 16 && file_stem.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(v) = u64::from_str_radix(file_stem, 16) {
            return Ok(v);
        }
    }

    // Handle `.ltk` suffix used by ltk_wad extractor for extensionless/colliding paths.
    let normalized_rel = normalize_rel_path_for_hash(rel_path, bytes);
    Ok(ltk_modpkg::utils::hash_chunk_name(&normalized_rel))
}

fn normalize_rel_path_for_hash(rel_path: &Path, bytes: &[u8]) -> String {
    let mut parts = rel_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return String::new();
    }

    // Normalize separators and casing.
    // Special case: strip `.ltk` suffix patterns from the filename.
    let last = parts.pop().unwrap();
    let stripped = if let Some(idx) = last.to_ascii_lowercase().find(".ltk.") {
        last[..idx].to_string()
    } else if last.to_ascii_lowercase().ends_with(".ltk") {
        last[..last.len().saturating_sub(4)].to_string()
    } else {
        // If file had no extension originally, extractor may have added `.ltk.<ext>` based on data.
        // We already handled `.ltk.` above, so keep as-is.
        last
    };
    parts.push(stripped);

    // Join using '/'.
    let joined = parts.join("/");

    // If we stripped to empty (rare), fall back to original filename.
    if joined.is_empty() {
        let _ = bytes; // keep signature to allow future heuristics if needed
        return rel_path.to_string_lossy().replace('\\', "/");
    }

    joined.replace('\\', "/")
}

fn load_mod_project(mod_dir: &Path) -> AppResult<ltk_mod_project::ModProject> {
    let config_path = mod_dir.join("mod.config.json");
    let contents = fs::read_to_string(&config_path)?;
    serde_json::from_str(&contents).map_err(AppError::from)
}

fn build_patched_wad(
    src_wad_path: &Path,
    dst_wad_path: &Path,
    overrides: &HashMap<u64, Vec<u8>>,
) -> AppResult<()> {
    let start = std::time::Instant::now();
    let file = fs::File::open(src_wad_path)?;
    let mut wad = Wad::mount(file).map_err(|e| {
        AppError::Other(format!(
            "Failed to mount WAD '{}': {}",
            src_wad_path.display(),
            e
        ))
    })?;
    let (mut decoder, chunks) = wad.decode();

    // Only write chunks that already exist in the base WAD.
    // Some extraction layouts may include non-chunk files; treating those as new chunks
    // can produce WADs League considers invalid.
    let ordered = {
        let mut hashes = chunks.keys().copied().collect::<Vec<_>>();
        hashes.sort_unstable();
        hashes
    };

    let unknown_override_hashes = overrides
        .keys()
        .filter(|h| !chunks.contains_key(h))
        .copied()
        .collect::<Vec<_>>();
    if !unknown_override_hashes.is_empty() {
        tracing::warn!(
            "Overlay: ignoring {} override chunk(s) not present in base WAD (src={} dst={})",
            unknown_override_hashes.len(),
            src_wad_path.display(),
            dst_wad_path.display()
        );
        tracing::debug!(
            "Overlay: unknown override hashes (first 16) = [{}]",
            unknown_override_hashes
                .iter()
                .take(16)
                .map(|h| format!("{:016x}", h))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let mut out_file = fs::File::create(dst_wad_path)?;
    let mut writer = std::io::BufWriter::new(&mut out_file);

    // Header (v3.4) with dummy signature/checksum.
    writer.write_u16::<LE>(0x5752)?; // \"RW\"
    writer.write_u8(3)?; // major
    writer.write_u8(4)?; // minor
    writer.write_all(&[0u8; 256])?; // ecdsa signature placeholder
    writer.write_u64::<LE>(0)?; // data checksum placeholder
    writer.write_u32::<LE>(ordered.len() as u32)?; // chunk count

    let toc_offset = writer.stream_position()?;
    writer.write_all(&vec![0u8; 32 * ordered.len()])?;

    let mut final_chunks: Vec<WadChunk> = Vec::with_capacity(ordered.len());

    for path_hash in &ordered {
        let orig = *chunks
            .get(path_hash)
            .ok_or_else(|| AppError::Other(format!("Missing base chunk {:016x}", path_hash)))?;

        let (compressed, uncompressed_size, compression_type, frame_count, start_frame, checksum) =
            if let Some(bytes) = overrides.get(path_hash) {
                match orig.compression_type {
                    WadChunkCompression::None => {
                        let checksum = xxh3_64(bytes);
                        (
                            bytes.clone(),
                            bytes.len(),
                            WadChunkCompression::None,
                            0,
                            0,
                            checksum,
                        )
                    }
                    WadChunkCompression::Zstd => {
                        let compressed = compress_zstd(bytes)?;
                        let checksum = xxh3_64(&compressed);
                        (
                            compressed,
                            bytes.len(),
                            WadChunkCompression::Zstd,
                            0,
                            0,
                            checksum,
                        )
                    }
                    WadChunkCompression::ZstdMulti => {
                        // Preserve the original uncompressed prefix length by finding the first zstd frame in the base chunk.
                        // League uses ZstdMulti for chunks that contain some raw bytes before the zstd stream.
                        let raw = decoder
                            .load_chunk_raw(&orig)
                            .map_err(|e| {
                                AppError::Other(format!(
                                    "Failed to read raw chunk {:016x}: {}",
                                    path_hash, e
                                ))
                            })?
                            .into_vec();
                        let prefix_len = find_zstd_magic_offset(&raw).unwrap_or(0);

                        if prefix_len > 0 && bytes.len() >= prefix_len {
                            let mut combined = Vec::with_capacity(prefix_len + bytes.len());
                            combined.extend_from_slice(&bytes[..prefix_len]);
                            let rest = compress_zstd(&bytes[prefix_len..])?;
                            combined.extend_from_slice(&rest);
                            let checksum = xxh3_64(&combined);
                            (
                                combined,
                                bytes.len(),
                                WadChunkCompression::ZstdMulti,
                                orig.frame_count,
                                orig.start_frame,
                                checksum,
                            )
                        } else {
                            // Fallback: encode as plain zstd and clear frame metadata.
                            let compressed = compress_zstd(bytes)?;
                            let checksum = xxh3_64(&compressed);
                            (
                                compressed,
                                bytes.len(),
                                WadChunkCompression::Zstd,
                                0,
                                0,
                                checksum,
                            )
                        }
                    }
                    other => {
                        tracing::warn!(
                            "Overlay: overriding chunk {:016x} with unsupported compression {:?}; encoding as zstd",
                            path_hash,
                            other
                        );
                        let compressed = compress_zstd(bytes)?;
                        let checksum = xxh3_64(&compressed);
                        (
                            compressed,
                            bytes.len(),
                            WadChunkCompression::Zstd,
                            0,
                            0,
                            checksum,
                        )
                    }
                }
            } else {
                let raw = decoder
                    .load_chunk_raw(&orig)
                    .map_err(|e| {
                        AppError::Other(format!(
                            "Failed to read raw chunk {:016x}: {}",
                            path_hash, e
                        ))
                    })?
                    .into_vec();
                // Raw bytes already match the stored checksum in the TOC.
                (
                    raw,
                    orig.uncompressed_size,
                    orig.compression_type,
                    orig.frame_count,
                    orig.start_frame,
                    orig.checksum,
                )
            };

        let data_offset = writer.stream_position()? as usize;
        writer.write_all(&compressed)?;

        final_chunks.push(WadChunk {
            path_hash: *path_hash,
            data_offset,
            compressed_size: compressed.len(),
            uncompressed_size,
            compression_type,
            is_duplicated: false,
            frame_count,
            start_frame,
            checksum,
        });
    }

    // Write TOC
    writer.seek(SeekFrom::Start(toc_offset))?;
    for chunk in &final_chunks {
        chunk
            .write_v3_4(&mut writer)
            .map_err(|e| AppError::Other(format!("Failed to write TOC: {}", e)))?;
    }

    writer.flush()?;

    tracing::info!(
        "Overlay: patched wad complete dst={} chunks_written={} overrides={} elapsed_ms={}",
        dst_wad_path.display(),
        final_chunks.len(),
        overrides.len(),
        start.elapsed().as_millis()
    );
    Ok(())
}

fn find_zstd_magic_offset(raw: &[u8]) -> Option<usize> {
    raw.windows(ZSTD_MAGIC.len()).position(|w| w == ZSTD_MAGIC)
}

fn compress_zstd(data: &[u8]) -> AppResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder =
        zstd::Encoder::new(std::io::BufWriter::new(&mut out), 3).map_err(AppError::from)?;
    encoder.write_all(data).map_err(AppError::from)?;
    encoder.finish().map_err(AppError::from)?;
    Ok(out)
}
