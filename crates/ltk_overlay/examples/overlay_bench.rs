//! Manual overlay build benchmark against a real League install.
//!
//! Synthetic small-WAD benchmarks measure allocator noise rather than the
//! copy-versus-tail effect this crate's build path is tuned for, so the numbers
//! that matter come from a real install and a real mod. This harness is run by
//! hand on a dev machine and its output pasted into a PR.
//!
//! ```text
//! LTK_BENCH_GAME_DIR="C:/Riot Games/League of Legends/Game" \
//! LTK_BENCH_MOD_DIR="X:/mods/my-skin" \
//!     cargo run --release -p ltk_overlay --example overlay_bench
//! ```
//!
//! # Environment
//!
//! - `LTK_BENCH_GAME_DIR` - the game's `Game/` directory (required).
//! - `LTK_BENCH_MOD_DIR` - a mod project directory in the [`FsModContent`]
//!   layout. Copied into the work directory first; the original is never
//!   written to. When unset, a fixture is synthesized from the install itself
//!   (see below), which is what makes runs comparable across machines.
//! - `LTK_BENCH_WAD` - WAD to synthesize the fixture from. Default
//!   `Aatrox.wad.client`.
//! - `LTK_BENCH_CHUNKS` - how many of that WAD's chunks the fixture overrides.
//!   Default 64.
//! - `LTK_BENCH_WORK_DIR` - scratch directory for the mod copy and the
//!   profile. Defaults to `<temp>/ltk-overlay-bench`, and is wiped on start.
//!
//! A synthesized fixture takes the first `LTK_BENCH_CHUNKS` chunks of the named
//! WAD, decompresses them, appends a few bytes so they are real edits rather
//! than lazy copies of the originals, and writes them as hex-named override
//! files under `content/base/<wad>/`.
//!
//! # Scenarios
//!
//! | scenario     | what it measures                                        |
//! | ------------ | ------------------------------------------------------- |
//! | cold         | full rebuild from an empty profile                      |
//! | no-op        | the exact-match skip                                    |
//! | edit (x3)    | one override's bytes change - the iteration inner loop  |
//! | add entry    | a brand-new chunk appears, changing the WAD's entry set  |

use camino::{Utf8Path, Utf8PathBuf};
use ltk_mod_project::{ModProject, ModProjectLayer};
use ltk_overlay::{EnabledMod, FsModContent, OverlayBuildResult, OverlayBuilder};
use ltk_wad::Wad;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let game_dir = required_dir("LTK_BENCH_GAME_DIR")?;
    let work_dir = match std::env::var("LTK_BENCH_WORK_DIR") {
        Ok(dir) => Utf8PathBuf::from(dir),
        Err(_) => utf8(std::env::temp_dir())?.join("ltk-overlay-bench"),
    };

    if work_dir.as_std_path().exists() {
        std::fs::remove_dir_all(work_dir.as_std_path())?;
    }
    let mod_dir = work_dir.join("mod");
    let source_mod = match std::env::var("LTK_BENCH_MOD_DIR") {
        Ok(dir) => {
            let source = Utf8PathBuf::from(dir);
            copy_dir(&source, &mod_dir)?;
            source.to_string()
        }
        Err(_) => synthesize_mod(&game_dir, &mod_dir)?,
    };

    let profile_dir = work_dir.join("profile");
    let overlay_root = profile_dir.join("overlay");

    let override_file = first_override_file(&mod_dir).ok_or(
        "the mod directory has no override files under content/<layer>/<wad>/ or content/raw/",
    )?;

    println!("game:    {game_dir}");
    println!("mod:     {source_mod}");
    println!("profile: {profile_dir}");
    println!("edits:   {}\n", override_file.file_name().unwrap_or("?"));

    let mut runs: Vec<Run> = Vec::new();

    let build = |label: &str| -> Result<Run, Box<dyn std::error::Error>> {
        let mut builder =
            OverlayBuilder::new(game_dir.clone(), overlay_root.clone(), profile_dir.clone());
        builder.set_enabled_mods(vec![EnabledMod {
            id: "bench-mod".to_string(),
            content: Box::new(FsModContent::new(mod_dir.clone())),
            enabled_layers: None,
        }]);

        let start = Instant::now();
        let result = builder.build()?;
        Ok(Run::new(label, start.elapsed(), &result))
    };

    runs.push(build("cold")?);
    runs.push(build("no-op")?);

    for round in 1..=3 {
        touch_bytes(&override_file, round)?;
        runs.push(build(&format!("edit #{round}"))?);
    }

    add_new_entry(&override_file)?;
    runs.push(build("add entry")?);

    println!(
        "\n{:<12} {:>10} {:>7} {:>7}",
        "scenario", "elapsed", "built", "reused"
    );
    println!("{}", "-".repeat(40));
    for run in &runs {
        println!(
            "{:<12} {:>10} {:>7} {:>7}",
            run.label,
            format!("{:.3} s", run.elapsed.as_secs_f64()),
            run.built,
            run.reused,
        );
    }

    Ok(())
}

/// One timed `build()` call.
struct Run {
    label: String,
    elapsed: Duration,
    built: usize,
    reused: usize,
}

impl Run {
    fn new(label: &str, elapsed: Duration, result: &OverlayBuildResult) -> Self {
        Self {
            label: label.to_string(),
            elapsed,
            built: result.wads_built.len(),
            reused: result.wads_reused.len(),
        }
    }
}

fn required_dir(var: &str) -> Result<Utf8PathBuf, Box<dyn std::error::Error>> {
    let path = Utf8PathBuf::from(
        std::env::var(var).map_err(|_| format!("{var} is not set; see this example's docs"))?,
    );
    if !path.as_std_path().is_dir() {
        return Err(format!("{var} is not a directory: {path}").into());
    }
    Ok(path)
}

fn utf8(path: std::path::PathBuf) -> Result<Utf8PathBuf, Box<dyn std::error::Error>> {
    Utf8PathBuf::from_path_buf(path).map_err(|p| format!("non-UTF-8 path: {}", p.display()).into())
}

/// Build a mod fixture at `mod_dir` out of the install's own chunks.
///
/// Returns a one-line description of what the fixture holds.
///
/// The overrides are the WAD's own chunks with bytes appended, so they route
/// exactly like real content (the hashes are the game's) but survive the
/// lazy-override filter, which strips copies identical to the originals.
fn synthesize_mod(
    game_dir: &Utf8Path,
    mod_dir: &Utf8Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let wad_name =
        std::env::var("LTK_BENCH_WAD").unwrap_or_else(|_| "Aatrox.wad.client".to_string());
    let chunk_count: usize = std::env::var("LTK_BENCH_CHUNKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);

    let wad_path = find_wad(game_dir, &wad_name)?;
    let wad_len = std::fs::metadata(wad_path.as_std_path())?.len();
    let mut wad = Wad::mount(std::fs::File::open(wad_path.as_std_path())?)?;
    let total_chunks = wad.chunks().len();
    let picked: Vec<ltk_wad::WadChunk> = wad.chunks().iter().take(chunk_count).copied().collect();

    let wad_dir = mod_dir.join("content").join("base").join(&wad_name);
    std::fs::create_dir_all(wad_dir.as_std_path())?;
    let mut written = 0u64;
    for chunk in &picked {
        let mut bytes = wad.load_chunk_decompressed(chunk)?.to_vec();
        bytes.extend_from_slice(b"ltk-bench");
        written += bytes.len() as u64;
        let name = format!("{:016x}.bin", chunk.path_hash.0);
        std::fs::write(wad_dir.join(name).as_std_path(), &bytes)?;
    }

    let project = ModProject {
        name: "ltk-overlay-bench".to_string(),
        display_name: "ltk-overlay-bench".to_string(),
        version: "1.0.0".to_string(),
        description: "Synthetic fixture for overlay_bench".to_string(),
        authors: vec![],
        license: None,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        transformers: vec![],
        layers: vec![ModProjectLayer {
            name: "base".to_string(),
            display_name: None,
            priority: 0,
            description: None,
            string_overrides: Default::default(),
        }],
        thumbnail: None,
        hashtables: vec![],
    };
    std::fs::write(
        mod_dir.join("mod.config.json").as_std_path(),
        serde_json::to_string_pretty(&project)?,
    )?;

    Ok(format!(
        "synthetic: {} overrides ({:.1} MiB) into {wad_name} ({} chunks, {:.1} MiB)",
        picked.len(),
        written as f64 / (1024.0 * 1024.0),
        total_chunks,
        wad_len as f64 / (1024.0 * 1024.0),
    ))
}

/// Locate `wad_name` under the game's `DATA/FINAL` tree, case-insensitively.
fn find_wad(
    game_dir: &Utf8Path,
    wad_name: &str,
) -> Result<Utf8PathBuf, Box<dyn std::error::Error>> {
    let wanted = wad_name.to_ascii_lowercase();
    walkdir::WalkDir::new(game_dir.join("DATA").join("FINAL").as_std_path())
        .into_iter()
        .flatten()
        .find(|entry| {
            entry.file_type().is_file()
                && entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.to_ascii_lowercase() == wanted)
        })
        .and_then(|entry| Utf8PathBuf::from_path_buf(entry.into_path()).ok())
        .ok_or_else(|| format!("no WAD named '{wad_name}' under {game_dir}/DATA/FINAL").into())
}

/// Copy `src` into `dst` recursively, creating `dst` and its parents.
fn copy_dir(src: &Utf8Path, dst: &Utf8Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dst.as_std_path())?;
    for entry in std::fs::read_dir(src.as_std_path())? {
        let entry = entry?;
        let name = utf8(entry.file_name().into())?;
        let from = src.join(&name);
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(from.as_std_path(), to.as_std_path())?;
        }
    }
    Ok(())
}

/// The first regular file under the mod's `content/` tree, in walk order.
fn first_override_file(mod_dir: &Utf8Path) -> Option<Utf8PathBuf> {
    walkdir::WalkDir::new(mod_dir.join("content").as_std_path())
        .sort_by_file_name()
        .into_iter()
        .flatten()
        .find(|entry| entry.file_type().is_file())
        .and_then(|entry| Utf8PathBuf::from_path_buf(entry.into_path()).ok())
}

/// Change the file's bytes without changing its length class, the way saving a
/// texture from an editor would: a real content edit, same chunk set.
fn touch_bytes(path: &Utf8Path, round: u8) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = std::fs::read(path.as_std_path())?;
    // Append rather than mutate in place: an empty file must still change, and
    // the tail bytes never collide with a format's magic at offset 0.
    bytes.extend_from_slice(&[round; 16]);
    std::fs::write(path.as_std_path(), bytes)?;
    Ok(())
}

/// Add a brand-new file beside `sibling`, which changes the target WAD's entry
/// count and therefore takes the full-rebuild path.
fn add_new_entry(sibling: &Utf8Path) -> Result<(), Box<dyn std::error::Error>> {
    let parent = sibling
        .parent()
        .ok_or("the override file has no parent directory")?;
    std::fs::write(
        parent.join("ltk_bench_new_entry.bin").as_std_path(),
        b"a chunk that was not in the WAD before",
    )?;
    Ok(())
}
