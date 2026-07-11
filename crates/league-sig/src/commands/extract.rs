//! Statement extraction: sign exactly the surface the verifier enforces.
//!
//! For every champion WAD whose `skin0.bin` a mod modifies, resolve the
//! `Characters/{Champ}/Skins/Skin0` entry the way the game (and verifier)
//! does — but over the *virtual merged WAD* (mod overrides on top of the
//! original) — and extract the three mesh references the base-skin check
//! looks at (`SkinMeshProperties.{Skeleton, SimpleSkin, Texture}`). Only
//! the mod-overridden ones among those go into the statement; references
//! left vanilla verify as `Unmodified` and need no attestation. WADs whose
//! `skin0.bin` the mod does not touch get no statement at all — the
//! verifier skips them entirely.
//!
//! Faithfulness to the overlay build:
//!
//! - Overrides are collected per layer in ascending priority order; later
//!   layers overwrite earlier ones by chunk hash (`ltk_overlay`'s pass 1).
//! - Each hash is routed to *every* game WAD containing it (cross-WAD
//!   matching); hashes in no game WAD fall back to the declared WAD target,
//!   then to the mod's dominant WAD (the builder's heuristics).
//! - Compressed checksums/digests come from
//!   [`ltk_overlay::wad_builder::prepare_override_chunk`], the exact
//!   function the WAD patcher uses to write override chunks.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, Write};

use camino::{Utf8Path, Utf8PathBuf};
use miette::{IntoDiagnostic, WrapErr, miette};
use serde::Serialize;
use sha2::Digest as _;
use xxhash_rust::xxh3::xxh3_64;

use ltk_hash::Hash as _;
use ltk_overlay::utils::resolve_chunk_hash;
use ltk_overlay::{FantomeContent, GameIndex, ModContentProvider};
use ltk_sig::base_skin::{self, BinChunkSource, BinHash, WadHash};
use ltk_sig::io::statement::{FileEntry, Statement, StatementParams};
use ltk_wad::Wad;

use crate::util::encode_hex;

#[derive(Serialize)]
struct IndexFile {
    mods: Vec<IndexEntry>,
    failures: Vec<IndexFailure>,
}

#[derive(Serialize)]
struct IndexEntry {
    mod_file: String,
    statements: Vec<IndexStatement>,
}

#[derive(Serialize)]
struct IndexFailure {
    mod_file: String,
    error: String,
}

#[derive(Serialize)]
struct IndexStatement {
    wad: String,
    champion: String,
    token_hash: String,
    entries: usize,
}

/// The virtual merged WAD: mod overrides win, everything else comes from
/// the original game WAD — the same view the verifier will walk.
struct MergedSource<'a, T: Read + Seek> {
    overrides: &'a HashMap<u64, (Vec<u8>, Option<Utf8PathBuf>)>,
    wad: &'a mut Wad<T>,
}

impl<T: Read + Seek> BinChunkSource for MergedSource<'_, T> {
    fn contains(&mut self, name_hash: u64) -> bool {
        self.overrides.contains_key(&name_hash) || self.wad.chunks().contains(name_hash)
    }

    fn load(&mut self, name_hash: u64) -> Option<Vec<u8>> {
        if let Some((bytes, _)) = self.overrides.get(&name_hash) {
            return Some(bytes.clone());
        }
        let chunk = self.wad.chunks().get(name_hash).copied()?;
        match self.wad.load_chunk_decompressed(&chunk) {
            Ok(data) => Some(data.into_vec()),
            Err(e) => {
                tracing::warn!("failed to decompress chunk {name_hash:016x}: {e}");
                None
            }
        }
    }
}

pub fn run(
    game_dir: &Utf8Path,
    output: &Utf8Path,
    mods_dir: Option<&Utf8Path>,
    mods: &[Utf8PathBuf],
) -> miette::Result<()> {
    let mut mod_paths: Vec<Utf8PathBuf> = mods.to_vec();
    if let Some(dir) = mods_dir {
        for entry in walkdir::WalkDir::new(dir.as_std_path()) {
            let entry = entry.into_diagnostic()?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(path) = Utf8PathBuf::from_path_buf(entry.path().to_path_buf()) else {
                eprintln!("SKIP non-UTF-8 path: {}", entry.path().display());
                continue;
            };
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("fantome"))
            {
                mod_paths.push(path);
            }
        }
    }
    mod_paths.sort();
    mod_paths.dedup();
    if mod_paths.is_empty() {
        return Err(miette!("no mod archives to extract"));
    }

    std::fs::create_dir_all(output.as_std_path()).into_diagnostic()?;

    println!("indexing game at {game_dir} ...");
    let game_index = GameIndex::load_or_build(game_dir, &output.join(".game_index.bin"))
        .into_diagnostic()
        .wrap_err("building game index")?;

    let mut index = IndexFile {
        mods: Vec::new(),
        failures: Vec::new(),
    };
    let mut statement_count = 0usize;

    for mod_path in &mod_paths {
        // Mods are untrusted input; isolate each one so neither an error
        // nor a panic inside the archive/WAD/bin parsers can take down the
        // rest of the batch.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            extract_one(mod_path, &game_index, game_dir, output)
        }));
        let error = match outcome {
            Ok(Ok(statements)) => {
                println!("{mod_path}:");
                if statements.is_empty() {
                    println!("  (no base-skin modifications; nothing to sign)");
                }
                for stmt in &statements {
                    println!(
                        "  {} ({}) -> {} ({} entries)",
                        stmt.wad, stmt.champion, stmt.token_hash, stmt.entries
                    );
                }
                statement_count += statements.len();
                index.mods.push(IndexEntry {
                    mod_file: mod_path.to_string(),
                    statements,
                });
                continue;
            }
            Ok(Err(e)) => format!("{e:?}"),
            Err(panic) => {
                let message = panic
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "opaque panic payload".to_owned());
                format!("panicked: {message}")
            }
        };
        eprintln!("FAILED {mod_path}: {error}");
        index.failures.push(IndexFailure {
            mod_file: mod_path.to_string(),
            error,
        });
    }

    let index_path = output.join("index.json");
    std::fs::write(
        index_path.as_std_path(),
        serde_json::to_string_pretty(&index).into_diagnostic()?,
    )
    .into_diagnostic()?;

    println!(
        "\n{} mod(s) processed: {} extracted ({statement_count} statement(s)), {} failed; \
         index written to {index_path}",
        mod_paths.len(),
        index.mods.len(),
        index.failures.len()
    );
    for failure in &index.failures {
        eprintln!("  failed: {}", failure.mod_file);
    }
    Ok(())
}

fn extract_one(
    mod_path: &Utf8Path,
    game_index: &GameIndex,
    game_dir: &Utf8Path,
    output: &Utf8Path,
) -> miette::Result<Vec<IndexStatement>> {
    let file = File::open(mod_path.as_std_path())
        .into_diagnostic()
        .wrap_err_with(|| format!("opening {mod_path}"))?;
    let mut provider = FantomeContent::new(BufReader::new(file))
        .into_diagnostic()
        .wrap_err("opening fantome archive")?;

    // ---- pass 1: collect overrides, later layers overriding earlier ----
    let project = provider.mod_project().into_diagnostic()?;
    let mut layers = project.layers.clone();
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    // chunk hash -> (uncompressed bytes, fallback game WAD for new entries)
    let mut overrides: HashMap<u64, (Vec<u8>, Option<Utf8PathBuf>)> = HashMap::new();

    for layer in &layers {
        for wad_name in provider.list_layer_wads(&layer.name).into_diagnostic()? {
            let files = provider
                .read_wad_overrides(&layer.name, &wad_name)
                .into_diagnostic()?;
            let entries: Vec<(u64, Vec<u8>)> = files
                .into_iter()
                .map(|(rel_path, bytes)| {
                    let hash = resolve_chunk_hash(&rel_path, &bytes).into_diagnostic()?;
                    Ok((hash, bytes))
                })
                .collect::<miette::Result<_>>()?;

            // Fallback target for hashes not present in any game WAD: the
            // declared WAD if the game has it, else the game WAD with the
            // most overlapping chunks (the builder's heuristics).
            let fallback = match game_index.find_wad(&wad_name) {
                Ok(abs) => Some(
                    abs.strip_prefix(game_dir)
                        .map_err(|_| miette!("WAD path {abs} is not under {game_dir}"))?
                        .to_path_buf(),
                ),
                Err(ltk_overlay::Error::WadNotFound(_)) => {
                    let hashes: Vec<u64> = entries.iter().map(|(h, _)| *h).collect();
                    game_index.find_best_matching_wad(&hashes)
                }
                Err(other) => return Err(other).into_diagnostic(),
            };

            for (hash, bytes) in entries {
                overrides.insert(hash, (bytes, fallback.clone()));
            }
        }
    }

    for (rel_path, bytes) in provider.read_raw_overrides().into_diagnostic()? {
        let hash = resolve_chunk_hash(&rel_path, &bytes).into_diagnostic()?;
        overrides.insert(hash, (bytes, None));
    }

    // Dominant-WAD routing for overrides with no fallback yet.
    if overrides.values().any(|(_, fallback)| fallback.is_none()) {
        let all_hashes: Vec<u64> = overrides.keys().copied().collect();
        if let Some(dominant) = game_index.find_best_matching_wad(&all_hashes) {
            for (_, fallback) in overrides.values_mut() {
                if fallback.is_none() {
                    *fallback = Some(dominant.clone());
                }
            }
        }
    }

    // ---- distribute: route each hash to every affected game WAD ----
    let mut per_wad: BTreeMap<Utf8PathBuf, Vec<u64>> = BTreeMap::new();
    for (&hash, (_, fallback)) in &overrides {
        if let Some(wads) = game_index.find_wads_with_hash(hash) {
            for wad in wads {
                per_wad.entry(wad.clone()).or_default().push(hash);
            }
        } else if let Some(fallback) = fallback {
            per_wad.entry(fallback.clone()).or_default().push(hash);
        }
    }

    // ---- sign only the enforced surface, per champion WAD ----
    let skin_class = BinHash::hash_str("SkinCharacterDataProperties");
    let mut results = Vec::new();

    for (wad_rel, hashes) in per_wad {
        let Some(wad_file_name) = wad_rel.file_name() else {
            continue;
        };
        let Some(champion) = base_skin::champion_from_wad_filename(wad_file_name) else {
            continue;
        };
        let root_bin_path = format!("data/characters/{champion}/skins/skin0.bin");
        // The verifier skips WADs whose skin0.bin is untouched — no
        // statement needed for them either.
        if !hashes.contains(&*WadHash::hash_str(&root_bin_path)) {
            continue;
        }

        let wad_path = game_dir.join(&wad_rel);
        let wad_file = File::open(wad_path.as_std_path())
            .into_diagnostic()
            .wrap_err_with(|| format!("opening game WAD {wad_path}"))?;
        let mut wad = Wad::mount(BufReader::new(wad_file))
            .into_diagnostic()
            .wrap_err_with(|| format!("mounting game WAD {wad_path}"))?;
        let toc_digest = base_skin::wad_toc_digest(&wad);

        let entry_hash = BinHash::hash_str(format!("characters/{champion}/skins/skin0"));
        let resolved = match base_skin::resolve_bin_entry_with(
            &mut MergedSource {
                overrides: &overrides,
                wad: &mut wad,
            },
            &root_bin_path,
            entry_hash,
            Some(skin_class),
        ) {
            Ok(resolved) => resolved,
            Err(e) => {
                eprintln!(
                    "  WARNING {wad_file_name}: modified skin0.bin but the base-skin \
                     entry cannot be resolved ({e}); the mod will not verify cleanly"
                );
                continue;
            }
        };

        let mesh_refs = base_skin::skin_mesh_refs(&resolved.object);
        let mut entries: Vec<FileEntry> = Vec::new();
        for path in mesh_refs.paths() {
            let name_hash = *WadHash::hash_str(path);
            if let Some((bytes, _)) = overrides.get(&name_hash) {
                let compressed = {
                    let mut out = Vec::new();
                    let mut encoder =
                        zstd::Encoder::new(BufWriter::new(&mut out), 3).into_diagnostic()?;
                    encoder.write_all(bytes).into_diagnostic()?;
                    encoder.finish().into_diagnostic()?;
                    out
                };
                entries.push(FileEntry {
                    name_hash,
                    checksum_compressed: xxh3_64(&compressed),
                    checksum_uncompressed: xxh3_64(bytes),
                    digest_decompressed: sha2::Sha256::digest(bytes).into(),
                });
            } else if !wad.chunks().contains(name_hash) {
                eprintln!(
                    "  WARNING {wad_file_name}: base skin references '{path}' which is \
                     neither in the mod nor the game WAD; it will verify as Missing"
                );
            }
            // Vanilla references verify as Unmodified — nothing to sign.
        }

        if entries.is_empty() {
            println!(
                "  {wad_file_name} ({champion}): skin0.bin modified but all mesh \
                 references are vanilla; nothing to sign"
            );
            continue;
        }

        entries.sort_unstable_by_key(|e| e.name_hash);
        entries.dedup_by_key(|e| e.name_hash);

        let statement = Statement::seal(&StatementParams {
            wad_name: Some(wad_file_name),
            wad_toc_digest: Some(toc_digest),
            entries: &entries,
        })
        .into_diagnostic()
        .wrap_err_with(|| format!("sealing statement for {wad_file_name}"))?;

        let token_hash = encode_hex(&statement.token_hash());
        let token_path = output.join(format!("{token_hash}.token"));
        std::fs::write(token_path.as_std_path(), statement.as_bytes()).into_diagnostic()?;

        results.push(IndexStatement {
            wad: wad_file_name.to_owned(),
            champion,
            token_hash,
            entries: entries.len(),
        });
    }
    Ok(results)
}
