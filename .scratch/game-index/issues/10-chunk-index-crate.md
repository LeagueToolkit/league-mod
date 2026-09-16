---
issue: 229
title: "Game index: chunk index crate"
labels: enhancement
---

Part of #228 (design: [`docs/design/game-index.md`](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md)
[section 4](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s4) to [section 6](https://github.com/LeagueToolkit/league-mod/blob/main/docs/design/game-index.md#s6)). Adds the
`ltk_game_index` workspace crate with the chunk index, its archive identity, its lookups, and
its build. No cache and no object index.

## Proposed surface

```rust
pub struct ArchiveId(u32);
pub struct Archive { pub name: String, pub path: Utf8PathBuf }
pub struct ChunkCopy { pub archive: ArchiveId, pub checksum: u64 }
pub struct ChunkRow { /* size, copies */ }
impl ChunkRow {
    pub fn size(&self) -> u64;
    pub fn copies(&self) -> &[ChunkCopy];
    pub fn holders(&self) -> impl Iterator<Item = ArchiveId> + '_;
    pub fn first_holder(&self) -> ArchiveId;
    pub fn is_consistent(&self) -> bool;
}
pub struct GameIndex { /* archives, rows, filename index, skipped */ }
impl GameIndex {
    pub fn build(game_dir: &Utf8Path) -> Result<Self, BuildError>;
    pub fn build_from_archives(root: &Utf8Path, archives: &[Utf8PathBuf]) -> Result<Self, BuildError>;
    pub fn archives(&self) -> &[Archive];
    pub fn archive(&self, id: ArchiveId) -> &Archive;
    pub fn archive_by_file_name(&self, file_name: &str) -> Result<ArchiveId, ArchiveLookupError>;
    pub fn skipped(&self) -> &[SkippedArchive];
    pub fn row(&self, hash: WadHash) -> Option<&ChunkRow>;
    pub fn row_by_path(&self, path: &str) -> Option<&ChunkRow>;
    pub fn holders(&self, hash: WadHash) -> impl Iterator<Item = ArchiveId> + '_;
    pub fn contains(&self, hash: WadHash) -> bool;
    pub fn chunks(&self) -> impl ExactSizeIterator<Item = (WadHash, &ChunkRow)>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn dominant_holder(&self, hashes: &[WadHash]) -> Option<ArchiveId>;
}
pub fn chunk_hash(path: &str) -> WadHash;
pub enum BuildError { MissingDataFinal, Enumerate, ArchiveOutsideRoot, Metadata }
pub enum ArchiveReadError { Open, Mount }
pub struct SkippedArchive { pub archive: ArchiveId, pub error: ArchiveReadError }
pub enum ArchiveLookupError { Absent, Ambiguous }
```

- Archives sort by name in byte order; ids are dense from zero. Enumeration matches
  `.wad.client` case-insensitively and nothing else.
- An archive that fails to open or mount is recorded in `skipped` and keeps its id. Per-archive
  failures never fail a build.
- `rayon` is a default feature; the serial build produces an identical index.
- Fields are private. Tests build fixture archives with `ltk_wad` in a temporary tree.
- Crate manifest mirrors `ltk_overlay`'s metadata block, Apache-2.0, and is publishable through
  release-plz.

Blocked by nothing.

- [ ] `cargo build -p ltk_game_index` with and without `--no-default-features`
- [ ] Two fixture archives sharing a chunk report two holders in id order and `is_consistent` reflects equal or unequal checksums
- [ ] `archive_by_file_name(\"aatrox.wad.client\")` finds `Champions/Aatrox.wad.client`; a duplicate file name in two directories is `Ambiguous` with both ids; an unknown name is `Absent`
- [ ] A truncated archive appears in `skipped()` with `Mount`, keeps its id, and the other archives index
- [ ] `build_from_archives` with a path outside `root` is `ArchiveOutsideRoot`
- [ ] `chunk_hash` equals `ltk_wad::WadHash::hash_str` for mixed-case input
- [ ] `dominant_holder` breaks a tie toward the lower id and is `None` for absent hashes
- [ ] `cargo clippy --all-targets --all-features`, `cargo fmt --check`, `cargo doc --no-deps` clean
