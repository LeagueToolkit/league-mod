//! The chunk index build: enumerate, mount, merge.

use std::fs::File;
use std::io::BufReader;

use camino::{Utf8Path, Utf8PathBuf};
use ltk_wad::{Wad, WadHash};

use crate::{
    Archive, ArchiveId, ArchiveReadError, BuildError, ChunkCopy, ChunkRow, GameIndex,
    SkippedArchive, archive, fingerprint,
};

/// One chunk as one archive's table of contents lists it.
#[derive(Debug, Clone, Copy)]
struct Listed {
    hash: WadHash,
    archive: ArchiveId,
    size: u64,
    checksum: u64,
}

/// The `DATA/FINAL` of `game_dir`.
///
/// # Errors
///
/// [`BuildError::MissingDataFinal`] when the directory is absent.
pub(crate) fn data_final(game_dir: &Utf8Path) -> Result<Utf8PathBuf, BuildError> {
    let root = game_dir.join("DATA").join("FINAL");
    if !root.is_dir() {
        return Err(BuildError::MissingDataFinal {
            path: game_dir.to_path_buf(),
        });
    }
    Ok(root)
}

/// Every archive under `game_dir/DATA/FINAL`, sorted by name.
pub(crate) fn enumerate_archives(game_dir: &Utf8Path) -> Result<Vec<Archive>, BuildError> {
    let root = data_final(game_dir)?;
    let paths = archive::enumerate(&root)?;
    archives_under(&root, &paths)
}

/// The given files as archives named relative to `root`, sorted by name.
pub(crate) fn archives_under(
    root: &Utf8Path,
    paths: &[Utf8PathBuf],
) -> Result<Vec<Archive>, BuildError> {
    let mut archives = paths
        .iter()
        .map(|path| archive::archive_under(root, path))
        .collect::<Result<Vec<_>, _>>()?;
    archives.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
    Ok(archives)
}

pub(crate) fn build(game_dir: &Utf8Path) -> Result<GameIndex, BuildError> {
    tracing::info!("Building game index from {game_dir}");
    let archives = enumerate_archives(game_dir)?;
    index_archives(archives)
}

pub(crate) fn build_from_archives(
    root: &Utf8Path,
    paths: &[Utf8PathBuf],
) -> Result<GameIndex, BuildError> {
    let archives = archives_under(root, paths)?;
    index_archives(archives)
}

/// Fingerprints and mounts every archive and merges the tables of contents into rows.
///
/// The fingerprint is taken before any archive is mounted. A cache carries the state of the
/// files as they were read.
fn index_archives(archives: Vec<Archive>) -> Result<GameIndex, BuildError> {
    let fingerprint = fingerprint::of_archives(&archives)?;
    let mounted = mount_all(&archives);

    let mut skipped = Vec::new();
    let mut listed: Vec<Listed> = Vec::new();
    for (index, outcome) in mounted.into_iter().enumerate() {
        match outcome {
            Ok(chunks) => listed.extend(chunks),
            Err(error) => {
                tracing::warn!("Skipping archive {}: {error}", archives[index].name);
                skipped.push(SkippedArchive {
                    archive: ArchiveId(index as u32),
                    error,
                });
            }
        }
    }

    listed.sort_unstable_by_key(|entry| (entry.hash, entry.archive));
    listed.dedup_by_key(|entry| (entry.hash, entry.archive));

    let mut hashes = Vec::new();
    let mut rows = Vec::new();
    for group in listed.chunk_by(|a, b| a.hash == b.hash) {
        let copies = group
            .iter()
            .map(|entry| ChunkCopy {
                archive: entry.archive,
                checksum: entry.checksum,
            })
            .collect();
        hashes.push(group[0].hash);
        rows.push(ChunkRow::new(group[0].size, copies));
    }

    tracing::info!(
        "Game index built: {} archives, {} skipped, {} chunks, fingerprint {fingerprint}",
        archives.len(),
        skipped.len(),
        rows.len()
    );
    Ok(GameIndex::from_parts(
        archives,
        hashes,
        rows,
        skipped,
        fingerprint,
    ))
}

type MountOutcome = Result<Vec<Listed>, ArchiveReadError>;

/// Mounts every archive, in parallel with the `rayon` feature. Outcomes are in id order.
fn mount_all(archives: &[Archive]) -> Vec<MountOutcome> {
    let mount_at = |(index, archive): (usize, &Archive)| mount(ArchiveId(index as u32), archive);
    #[cfg(feature = "rayon")]
    {
        use rayon::prelude::*;
        archives.par_iter().enumerate().map(mount_at).collect()
    }
    #[cfg(not(feature = "rayon"))]
    {
        archives.iter().enumerate().map(mount_at).collect()
    }
}

/// The table of contents of the archive `id`, one [`Listed`] per chunk.
fn mount(id: ArchiveId, archive: &Archive) -> MountOutcome {
    let file = File::open(archive.path.as_std_path()).map_err(|e| ArchiveReadError::open(&e))?;
    let wad = Wad::mount(BufReader::new(file)).map_err(|e| ArchiveReadError::mount(&e))?;
    Ok(wad
        .chunks()
        .iter()
        .map(|chunk| Listed {
            hash: chunk.path_hash,
            archive: id,
            size: chunk.uncompressed_size as u64,
            checksum: chunk.checksum,
        })
        .collect())
}
