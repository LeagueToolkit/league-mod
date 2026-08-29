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
//! - `LTK_BENCH_MOD_FANTOME` - one or more `.fantome` archives separated by
//!   `;`, read through [`FantomeContent`] exactly as a manager enabling them
//!   would. Each is copied into the work directory first. Their overrides live
//!   inside the archives, so the scenarios that edit an override file are
//!   skipped. Takes precedence over `LTK_BENCH_MOD_DIR`. Passing several
//!   archives is what measures the N-mod no-op case, where per-mod opening
//!   cost dominates.
//! - `LTK_BENCH_MOD_DIR` - a mod project directory in the [`FsModContent`]
//!   layout. Copied into the work directory first; the original is never
//!   written to. When unset, a fixture is synthesized from the install itself
//!   (see below), which is what makes runs comparable across machines.
//! - `LTK_BENCH_WAD` - WAD to synthesize the fixture from. Default
//!   `Aatrox.wad.client`.
//! - `LTK_BENCH_EXPLODE` - set to `1` to unpack a `.fantome` into a mod project
//!   directory instead of reading it as an archive, which makes the edit
//!   scenarios available for a real distributed mod.
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
use ltk_overlay::{EnabledMod, FantomeContent, FsModContent, OverlayBuildResult, OverlayBuilder};
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
    let explode = optional_var("LTK_BENCH_EXPLODE", 0u8)? != 0;
    let fixture = match std::env::var("LTK_BENCH_MOD_FANTOME") {
        Ok(path_list) => {
            let sources: Vec<Utf8PathBuf> = path_list
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(Utf8PathBuf::from)
                .collect();
            if sources.is_empty() {
                return Err("LTK_BENCH_MOD_FANTOME holds no archive paths".into());
            }
            std::fs::create_dir_all(work_dir.as_std_path())?;
            if explode {
                let [source] = sources.as_slice() else {
                    return Err("LTK_BENCH_EXPLODE only supports a single archive".into());
                };
                explode_fantome(source, &mod_dir)?;
                let override_file = first_override_file(&mod_dir)
                    .ok_or("the archive holds no WAD overrides to explode")?;
                Fixture::Dir {
                    mod_dir,
                    override_file,
                    source: format!("{source} (exploded)"),
                }
            } else {
                let archives = sources
                    .into_iter()
                    .enumerate()
                    .map(|(i, source)| {
                        let copy = work_dir.join(format!("mod-{i}.fantome"));
                        std::fs::copy(source.as_std_path(), copy.as_std_path())?;
                        Ok(FantomeArchive {
                            archive: copy,
                            source,
                        })
                    })
                    .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
                Fixture::Fantome { archives }
            }
        }
        Err(_) => {
            let source = match std::env::var("LTK_BENCH_MOD_DIR") {
                Ok(dir) => {
                    let source = Utf8PathBuf::from(dir.trim());
                    copy_dir(&source, &mod_dir)?;
                    source.to_string()
                }
                Err(_) => synthesize_mod(&game_dir, &mod_dir)?,
            };
            let override_file = first_override_file(&mod_dir).ok_or(
                "the mod directory has no override files under content/<layer>/<wad>/ or content/raw/",
            )?;
            Fixture::Dir {
                mod_dir,
                override_file,
                source,
            }
        }
    };

    let profile_dir = work_dir.join("profile");
    let overlay_root = profile_dir.join("overlay");

    println!("game:    {game_dir}");
    match &fixture {
        Fixture::Dir { source, .. } => println!("mod:     {source}"),
        Fixture::Fantome { archives } => {
            for archive in archives {
                println!("mod:     {}", archive.source);
            }
        }
    }
    println!("profile: {profile_dir}");
    match &fixture {
        Fixture::Dir { override_file, .. } => {
            println!("edits:   {}\n", override_file.file_name().unwrap_or("?"))
        }
        Fixture::Fantome { .. } => {
            println!("edits:   n/a - a .fantome's overrides live inside the archive\n")
        }
    }

    let mut runs: Vec<Run> = Vec::new();

    let build = |label: &str| -> Result<Run, Box<dyn std::error::Error>> {
        // Timed from provider construction, not from `build()`: opening the mod
        // is work a consumer pays on every build, and for an archive provider it
        // is not small. `opening` breaks it out so the two are separable.
        let start = Instant::now();
        let enabled_mods = fixture.enabled_mods()?;
        let opening = start.elapsed();

        let mut builder =
            OverlayBuilder::new(game_dir.clone(), overlay_root.clone(), profile_dir.clone());
        builder.set_enabled_mods(enabled_mods);

        let result = builder.build()?;
        Ok(Run::new(label, start.elapsed(), opening, &result))
    };

    runs.push(build("cold")?);
    runs.push(build("no-op")?);

    // The remaining scenarios mutate an override file in place, which only a
    // directory fixture has. A `.fantome` is read as the manager reads it.
    let Fixture::Dir { override_file, .. } = &fixture else {
        report(&runs);
        report_peak_memory();
        return Ok(());
    };

    for round in 1..=3 {
        touch_bytes(override_file, round)?;
        runs.push(build(&format!("edit #{round}"))?);
    }

    add_new_entry(override_file)?;
    runs.push(build("add entry")?);

    report(&runs);
    report_peak_memory();
    Ok(())
}

/// Print the process's two memory peaks across every scenario in the run.
///
/// `peak commit` is the number that matters: private, pagefile-backed
/// allocations the OS cannot reclaim without writing them out - the kind that
/// freezes low-RAM machines. `peak working set` also counts clean file-backed
/// pages (the builder memory-maps game WADs to copy them), which the OS
/// evicts for free, so it overstates pressure by roughly the mapped WAD sizes.
///
/// The OS reports one process-lifetime peak, not one per scenario, so this is
/// a trailing line rather than a table column.
#[cfg(windows)]
fn report_peak_memory() {
    // Field layout from PROCESS_MEMORY_COUNTERS in psapi.h; cb (in),
    // PeakWorkingSetSize and PeakPagefileUsage are consumed.
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(
            process: isize,
            counters: *mut ProcessMemoryCounters,
            cb: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY: GetCurrentProcess returns a pseudo-handle that needs no closing,
    // and the out-pointer is a live, correctly sized ProcessMemoryCounters
    // whose cb tells the API how much it may write.
    let ok =
        unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) != 0 };
    if ok {
        println!(
            "\npeak commit: {:.0} MiB (private; the pressure that freezes low-RAM machines)",
            counters.peak_pagefile_usage as f64 / (1024.0 * 1024.0)
        );
        println!(
            "peak working set: {:.0} MiB (includes evictable mapped game-WAD pages)",
            counters.peak_working_set_size as f64 / (1024.0 * 1024.0)
        );
    }
}

#[cfg(not(windows))]
fn report_peak_memory() {}

/// Unpack a `.fantome`'s WAD content into a mod project directory.
///
/// A fantome ships its overrides inside a packed WAD, which no scenario can
/// edit in place. Writing them out as the loose files a workshop project holds
/// is what makes the edit scenarios available for a real distributed mod - the
/// same content, read the way its author would be iterating on it.
fn explode_fantome(
    archive: &Utf8Path,
    mod_dir: &Utf8Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use ltk_overlay::ModContentProvider as _;

    let mut content = FantomeContent::new(std::fs::File::open(archive.as_std_path())?)?;
    let project = content.mod_project()?;

    let mut chunks = 0usize;
    for wad_name in content.list_layer_wads("base")? {
        let wad_dir = mod_dir.join("content").join("base").join(&wad_name);
        for (rel_path, bytes) in content.read_wad_overrides("base", &wad_name)? {
            let file = wad_dir.join(&rel_path);
            std::fs::create_dir_all(file.parent().expect("override has a parent").as_std_path())?;
            std::fs::write(file.as_std_path(), &bytes)?;
            chunks += 1;
        }
    }

    std::fs::write(
        mod_dir.join("mod.config.json").as_std_path(),
        serde_json::to_string_pretty(&project)?,
    )?;
    println!("exploded {chunks} override(s) from {archive}");

    Ok(())
}

/// The mod this run builds from, and how it has to be read.
enum Fixture {
    /// A mod project directory, which every scenario can edit in place.
    Dir {
        mod_dir: Utf8PathBuf,
        override_file: Utf8PathBuf,
        source: String,
    },
    /// One or more `.fantome` archives, read exactly as a manager enabling
    /// them would.
    ///
    /// Each is copied into the work directory first, so the originals are
    /// never touched. Their overrides live inside zipped WADs rather than as
    /// files on disk, so the scenarios that edit one do not apply.
    Fantome { archives: Vec<FantomeArchive> },
}

/// A `.fantome` archive under benchmark: its work-directory copy and where it
/// was copied from.
struct FantomeArchive {
    archive: Utf8PathBuf,
    source: Utf8PathBuf,
}

impl Fixture {
    /// Fresh enabled mods for one build, since a build consumes the providers
    /// it is given.
    fn enabled_mods(&self) -> Result<Vec<EnabledMod>, Box<dyn std::error::Error>> {
        Ok(match self {
            Self::Dir { mod_dir, .. } => vec![EnabledMod {
                id: "bench-mod".to_string(),
                content: Box::new(FsModContent::new(mod_dir.clone())),
                enabled_layers: None,
            }],
            Self::Fantome { archives } => archives
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    Ok(EnabledMod {
                        id: format!("bench-mod-{i}"),
                        // The archive path is what lets the metadata cache
                        // fingerprint this mod, which the no-op scenario needs
                        // to hit the skip.
                        content: Box::new(
                            FantomeContent::new(std::fs::File::open(a.archive.as_std_path())?)?
                                .with_archive_path(a.archive.clone()),
                        ),
                        enabled_layers: None,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        })
    }
}

/// Print the timing table.
fn report(runs: &[Run]) {
    println!(
        "\n{:<12} {:>10} {:>10} {:>7} {:>7}",
        "scenario", "elapsed", "opening", "built", "reused"
    );
    println!("{}", "-".repeat(51));
    for run in runs {
        println!(
            "{:<12} {:>10} {:>10} {:>7} {:>7}",
            run.label,
            format!("{:.3} s", run.elapsed.as_secs_f64()),
            format!("{:.3} s", run.opening.as_secs_f64()),
            run.built,
            run.reused,
        );
    }
}

/// One timed build, from opening the mod to the finished overlay.
struct Run {
    label: String,
    elapsed: Duration,
    /// Share of `elapsed` spent constructing the content provider.
    opening: Duration,
    built: usize,
    reused: usize,
}

impl Run {
    fn new(label: &str, elapsed: Duration, opening: Duration, result: &OverlayBuildResult) -> Self {
        Self {
            label: label.to_string(),
            elapsed,
            opening,
            built: result.wads_built.len(),
            reused: result.wads_reused.len(),
        }
    }
}

fn required_dir(var: &str) -> Result<Utf8PathBuf, Box<dyn std::error::Error>> {
    let path = Utf8PathBuf::from(required_var(var)?);
    if !path.as_std_path().is_dir() {
        return Err(format!("{var} is not a directory: {path}").into());
    }
    Ok(path)
}

/// Read `var`, trimmed, refusing an unset or blank one.
fn required_var(var: &str) -> Result<String, Box<dyn std::error::Error>> {
    let value = std::env::var(var)
        .map_err(|_| format!("{var} is not set; see this example's docs"))?
        .trim()
        .to_string();
    if value.is_empty() {
        return Err(format!("{var} is empty; see this example's docs").into());
    }
    Ok(value)
}

/// Read an optional `var`, or `default` when it is unset or blank.
///
/// A value that is set but unusable is an error rather than a fallback: these
/// numbers end up in benchmark results, and silently benchmarking something
/// other than what was asked for is worse than refusing to run.
fn optional_var<T>(var: &str, default: T) -> Result<T, Box<dyn std::error::Error>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match std::env::var(var) {
        Err(_) => Ok(default),
        Ok(raw) if raw.trim().is_empty() => Ok(default),
        Ok(raw) => raw
            .trim()
            .parse()
            .map_err(|e| format!("{var} is not usable: {e}").into()),
    }
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
    let wad_name = optional_var("LTK_BENCH_WAD", "Aatrox.wad.client".to_string())?;
    let chunk_count: usize = optional_var("LTK_BENCH_CHUNKS", 64)?;
    if chunk_count == 0 {
        return Err("LTK_BENCH_CHUNKS is 0, which would build a mod with no overrides".into());
    }

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
