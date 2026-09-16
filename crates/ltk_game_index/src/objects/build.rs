//! The object index build: one job per archive over the chunk index's rows.

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::time::Instant;

use ltk_wad::{ChunkDecoder, Wad, WadHash};

use super::read::{MountedArchive, for_each_declaration, sniffs_as_bin};
use super::{BuildOptions, Declaration, ObjectBuildError, ObjectStats};
use crate::{Archive, ArchiveId, ArchiveReadError, GameIndex, SkippedArchive};

/// One archive's share of the build.
#[derive(Debug)]
struct Job<'a> {
    id: ArchiveId,
    archive: &'a Archive,
    /// Chunks a resolver names with a `.bin` extension, in ascending hash order.
    named: Vec<WadHash>,
    /// Chunks a resolver names without an extension, in ascending hash order.
    bare: Vec<WadHash>,
    /// Chunks no resolver names, in ascending hash order.
    unnamed: Vec<WadHash>,
}

/// What one job read.
#[derive(Debug, Default)]
struct Read {
    declarations: Vec<Declaration>,
    files: u32,
    sniffed: u32,
    sniffed_bins: u32,
    skipped_chunks: u32,
    bytes: u64,
}

/// What the build produced before indexing.
#[derive(Debug)]
pub(super) struct Built {
    pub declarations: Vec<Declaration>,
    pub stats: ObjectStats,
    pub skipped: Vec<SkippedArchive>,
}

impl Job<'_> {
    fn chunk_count(&self) -> usize {
        self.named.len() + self.bare.len() + self.unnamed.len()
    }

    /// Mounts the archive and reads every chunk of the job for its declarations.
    ///
    /// Named chunks first, then bare and unnamed chunks that sniff as a bin.
    fn read(&self) -> Result<Read, ArchiveReadError> {
        let file =
            File::open(self.archive.path.as_std_path()).map_err(|e| ArchiveReadError::open(&e))?;
        let mut wad = Wad::mount(BufReader::new(file)).map_err(|e| ArchiveReadError::mount(&e))?;
        let mut read = Read::default();

        for &hash in &self.named {
            read.files += 1;
            self.read_chunk(&mut wad, hash, &mut read);
        }

        let mut decoder = ChunkDecoder::new();
        for &hash in self.bare.iter().chain(&self.unnamed) {
            read.sniffed += 1;
            if !self.sniff(&mut wad, hash, &mut decoder) {
                continue;
            }
            read.sniffed_bins += 1;
            read.files += 1;
            self.read_chunk(&mut wad, hash, &mut read);
        }
        Ok(read)
    }

    /// Whether `hash` decodes to a bin's magic.
    fn sniff(&self, wad: &mut MountedArchive, hash: WadHash, decoder: &mut ChunkDecoder) -> bool {
        let Some(chunk) = wad.chunks().get(hash).copied() else {
            return false;
        };
        match sniffs_as_bin(wad, &chunk, decoder) {
            Ok(is_bin) => is_bin,
            Err(e) => {
                tracing::debug!("Not sniffing {}/{hash:016x}: {e}", self.archive.name);
                false
            }
        }
    }

    /// Reads `hash` whole and pushes a declaration for every object it declares.
    ///
    /// A chunk that is missing or does not read is counted in `skipped_chunks`.
    fn read_chunk(&self, wad: &mut MountedArchive, hash: WadHash, read: &mut Read) {
        let bytes = match wad.chunks().get(hash).copied() {
            Some(chunk) => wad.load_chunk_decompressed(&chunk),
            None => {
                read.skipped_chunks += 1;
                tracing::debug!(
                    "Skipping {}/{hash:016x}: not in the archive",
                    self.archive.name
                );
                return;
            }
        };
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(e) => {
                read.skipped_chunks += 1;
                tracing::debug!("Skipping {}/{hash:016x}: {e}", self.archive.name);
                return;
            }
        };
        read.bytes += bytes.len() as u64;

        let before = read.declarations.len();
        let outcome = for_each_declaration(Cursor::new(&bytes[..]), |object, class| {
            read.declarations.push(Declaration {
                object,
                class,
                chunk: hash,
                archive: self.id,
            });
        });
        if let Err(e) = outcome {
            read.declarations.truncate(before);
            read.skipped_chunks += 1;
            tracing::debug!("Skipping {}/{hash:016x}: {e}", self.archive.name);
        }
    }
}

/// Whether a chunk path names a bin by its extension.
fn is_bin(path: &str) -> bool {
    path.len() >= 4
        && path
            .get(path.len() - 4..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(".bin"))
}

/// Whether a chunk path's last segment carries no extension.
fn is_bare(path: &str) -> bool {
    camino::Utf8Path::new(path).extension().is_none()
}

/// Partitions the chunk index's rows by first holder and classifies each chunk.
fn jobs<'a>(game: &'a GameIndex, options: &BuildOptions<'_>) -> Vec<Job<'a>> {
    let archives = game.archives();
    let mut named: Vec<Vec<WadHash>> = vec![Vec::new(); archives.len()];
    let mut bare: Vec<Vec<WadHash>> = vec![Vec::new(); archives.len()];
    let mut unnamed: Vec<Vec<WadHash>> = vec![Vec::new(); archives.len()];

    let hashes: Vec<WadHash> = game.chunks().map(|(hash, _)| hash).collect();
    let first_holders: Vec<ArchiveId> = game.chunks().map(|(_, row)| row.first_holder()).collect();

    let mut is_named = vec![false; hashes.len()];
    if let Some(resolver) = options.resolver {
        resolver.for_each_named(&hashes, &mut |index, path| {
            is_named[index] = true;
            let holder = first_holders[index].index();
            if is_bin(path) {
                named[holder].push(hashes[index]);
            } else if is_bare(path) {
                bare[holder].push(hashes[index]);
            }
        });
    }
    for (index, &hash) in hashes.iter().enumerate() {
        if !is_named[index] {
            unnamed[first_holders[index].index()].push(hash);
        }
    }

    named
        .into_iter()
        .zip(bare)
        .zip(unnamed)
        .enumerate()
        .filter(|(_, ((named, bare), unnamed))| {
            !named.is_empty() || !bare.is_empty() || !unnamed.is_empty()
        })
        .map(|(index, ((mut named, mut bare), unnamed))| {
            named.sort_unstable();
            bare.sort_unstable();
            Job {
                id: ArchiveId(index as u32),
                archive: &archives[index],
                named,
                bare,
                unnamed,
            }
        })
        .collect()
}

/// Runs `jobs` on `workers` threads, polling `called_off` before each.
///
/// Outcomes come back in job order. An outcome is `None` where the build was called off
/// before reaching the job.
fn run_jobs(
    jobs: &[Job<'_>],
    workers: usize,
    called_off: &(dyn Fn() -> bool + Sync),
) -> Vec<Option<Result<Read, ArchiveReadError>>> {
    #[cfg(feature = "rayon")]
    {
        use rayon::prelude::*;
        let run = || {
            jobs.par_iter()
                .map(|job| (!called_off()).then(|| job.read()))
                .collect()
        };
        match rayon::ThreadPoolBuilder::new().num_threads(workers).build() {
            Ok(pool) => pool.install(run),
            Err(e) => {
                tracing::warn!("Object index build runs on the global pool: {e}");
                run()
            }
        }
    }
    #[cfg(not(feature = "rayon"))]
    {
        let _ = workers;
        jobs.iter()
            .map(|job| (!called_off()).then(|| job.read()))
            .collect()
    }
}

pub(super) fn build(
    game: &GameIndex,
    options: &BuildOptions<'_>,
) -> Result<Built, ObjectBuildError> {
    let started = Instant::now();
    let jobs = jobs(game, options);
    let workers = options
        .workers
        .map(std::num::NonZeroUsize::get)
        .or_else(|| std::thread::available_parallelism().ok().map(Into::into))
        .unwrap_or(1)
        .clamp(1, jobs.len().max(1));
    let never = || false;
    let called_off: &(dyn Fn() -> bool + Sync) = options.called_off.unwrap_or(&never);

    let outcomes = run_jobs(&jobs, workers, called_off);

    let mut built = Built {
        declarations: Vec::new(),
        stats: ObjectStats {
            archives: jobs.len() as u32,
            workers: workers as u32,
            ..ObjectStats::default()
        },
        skipped: Vec::new(),
    };
    for (job, outcome) in jobs.iter().zip(outcomes) {
        let Some(outcome) = outcome else {
            return Err(ObjectBuildError::CalledOff);
        };
        match outcome {
            Ok(read) => {
                built.stats.files += read.files;
                built.stats.sniffed += read.sniffed;
                built.stats.sniffed_bins += read.sniffed_bins;
                built.stats.declarations += read.declarations.len() as u32;
                built.stats.skipped_chunks += read.skipped_chunks;
                built.stats.bytes += read.bytes;
                built.declarations.extend(read.declarations);
            }
            Err(error) => {
                built.stats.skipped_chunks += job.chunk_count() as u32;
                tracing::warn!("Skipping archive {}: {error}", job.archive.name);
                built.skipped.push(SkippedArchive {
                    archive: job.id,
                    error,
                });
            }
        }
    }
    built.stats.elapsed = started.elapsed();
    tracing::info!(
        archives = built.stats.archives,
        files = built.stats.files,
        sniffed = built.stats.sniffed,
        sniffed_bins = built.stats.sniffed_bins,
        declarations = built.stats.declarations,
        skipped_chunks = built.stats.skipped_chunks,
        bytes = built.stats.bytes,
        elapsed_ms = built.stats.elapsed.as_millis() as u64,
        workers = built.stats.workers,
        "Built the object index"
    );
    Ok(built)
}
