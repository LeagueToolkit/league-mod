# Lazy Override Loading (Future Optimization)

## Status: Proposed (not implemented)

This document describes a potential memory optimization for the overlay build pipeline that defers loading override bytes until they're actually needed for patching. Implement this if users report memory pressure with large mod sets.

## Problem

The current pipeline loads **all** override bytes from **all** enabled mods into memory during `collect_all_overrides`, before we even know which WADs need rebuilding. For a typical setup (5-10 mods, ~50-200MB of overrides), this is fine. But with many mods containing large textures, peak memory can exceed 500MB+ of override data alone.

On incremental builds, most WADs are reused unchanged -- yet we still loaded all their override bytes just to compute fingerprints and confirm nothing changed.

## Current Pipeline

```
collect_all_overrides()
  For each mod (parallel):
    For each layer/WAD:
      read_wad_overrides() -> Vec<(path, Vec<u8>)>    ← full bytes loaded
      resolve_chunk_hash()
      store in HashMap<u64, Vec<u8>>
  Merge per-mod results (reverse priority order)
  Filter SubChunkTOC (needs hash only)
  Filter lazy overrides (needs xxh3_64 of bytes)        ← requires bytes
  Wrap survivors in Arc<[u8]>

distribute_overrides()
  Route overrides to per-WAD maps via game index

compute_wad_overrides_fingerprint()                     ← requires bytes
  xxh3_64 each override's bytes, hash the sorted list

Partition WADs into rebuild / reuse

Patch WADs (parallel)
  For each WAD: read Arc<[u8]>, compress, write          ← requires bytes
```

Three operations require full bytes before patching: lazy filtering, fingerprinting, and the patching itself. This blocks naive lazy loading.

## Proposed Design: Two-Pass Content-Hash Architecture

### Key Insight

The lazy filter and fingerprinting only need `xxh3_64(bytes)` -- a `u64` content hash -- not the full bytes. If we compute content hashes during a lightweight first pass, we can defer loading full bytes to a second pass that only targets WADs that actually need rebuilding.

### New Pipeline

```
Pass 1: Collect metadata + content hashes (lightweight)
  For each mod (parallel):
    For each layer/WAD:
      read_wad_override_hashes() -> Vec<(path_hash, content_hash)>   ← NEW method
      store in HashMap<u64, u64>  (path_hash -> content_hash)
  Merge per-mod results (reverse priority order)
  Filter SubChunkTOC (hash only, same as today)
  Filter lazy overrides (compare content_hash vs game_index, no bytes needed)
  Distribute to per-WAD maps
  Compute fingerprints from content hashes (no bytes needed)
  Partition WADs into rebuild / reuse

Pass 2: Load bytes only for WADs that need rebuilding
  For each WAD to rebuild:
    Identify which mods contribute overrides to this WAD
    Re-read only those overrides from providers
  Patch WADs (parallel, same as today)
```

### Memory Profile Comparison

| Scenario | Current | Proposed |
|----------|---------|----------|
| Full rebuild (10 mods, 200MB overrides) | ~200MB peak | ~200MB peak (same, but loaded per-WAD) |
| Incremental (1 WAD changed, 200MB total) | ~200MB peak | ~20MB peak (only changed WAD's overrides) |
| Incremental (nothing changed, skip build) | ~200MB peak | ~1MB peak (only content hashes) |

### Required Changes

#### 1. New `ModContentProvider` method

```rust
pub trait ModContentProvider: Send {
    // Existing methods...

    /// Read content hashes for all override files in a WAD, without loading full bytes.
    ///
    /// Returns (path_hash, content_hash) pairs. Implementations should compute
    /// xxh3_64 of each file's bytes during iteration without retaining the bytes.
    ///
    /// Default implementation falls back to read_wad_overrides + hashing.
    fn read_wad_override_hashes(
        &mut self,
        layer: &str,
        wad_name: &str,
    ) -> Result<Vec<(u64, u64)>> {
        let overrides = self.read_wad_overrides(layer, wad_name)?;
        overrides
            .into_iter()
            .map(|(rel_path, bytes)| {
                let path_hash = resolve_chunk_hash(&rel_path, &bytes)?;
                let content_hash = xxh3_64(&bytes);
                Ok((path_hash, content_hash))
            })
            .collect()
    }
}
```

The default implementation provides no memory savings (loads bytes then drops them). But specialized implementations for `ModpkgContent` and `FantomeContent` can stream through the archive computing hashes without accumulating all bytes. `FsModContent` can read and hash files one at a time.

#### 2. Modify `compute_wad_overrides_fingerprint`

Change to work with content hashes directly:

```rust
// Current: takes HashMap<u64, impl AsRef<[u8]>>, hashes bytes internally
// New: takes HashMap<u64, u64> (path_hash -> content_hash), uses content hashes directly
pub fn compute_wad_overrides_fingerprint_from_hashes(
    overrides: &HashMap<u64, u64>,
) -> u64 {
    let mut entries: Vec<(u64, u64)> = overrides.iter().map(|(&k, &v)| (k, v)).collect();
    entries.sort_unstable_by_key(|(path_hash, _)| *path_hash);
    // ... hash the sorted entries
}
```

#### 3. Track override provenance

During pass 1, record which mod + layer + WAD provides each winning override:

```rust
struct OverrideSource {
    mod_index: usize,      // index into enabled_mods
    layer_name: String,
    wad_name: String,
    content_hash: u64,
}
```

During pass 2, use this to re-read only the needed bytes from the correct provider.

#### 4. Provider access for pass 2

Pass 2 needs `&mut` access to providers to re-read bytes. Options:

- **Sequential pass 2**: Read overrides for each WAD-to-rebuild sequentially from providers, then patch WADs in parallel. This preserves parallel patching but makes the loading sequential.
- **Reopen providers**: For archive-backed providers, open a new file handle for pass 2. This allows parallel loading but requires a `reopen()` or `clone()` method on providers.
- **Provider pool**: Create multiple handles per archive (one per rayon thread). Most complex but maximum parallelism.

**Recommended**: Sequential pass 2 with parallel patching. The I/O for loading a single WAD's overrides is fast compared to the compression work during patching. Structure:

```rust
// Sequential: load bytes for each WAD that needs rebuilding
let mut per_wad_bytes: Vec<(Utf8PathBuf, HashMap<u64, Arc<[u8]>>)> = Vec::new();
for wad_path in &wads_to_rebuild {
    let overrides = load_overrides_for_wad(wad_path, &override_sources, &mut enabled_mods)?;
    per_wad_bytes.push((wad_path.clone(), overrides));
}

// Parallel: patch WADs (same as today)
per_wad_bytes.into_par_iter().map(|(path, overrides)| {
    build_patched_wad(...)
}).collect()
```

## Tradeoffs

| Aspect | Current | Proposed |
|--------|---------|----------|
| Peak memory | All override bytes | Only changed WADs' bytes |
| I/O passes | 1 (read all once) | 2 for changed WADs, 1 for unchanged |
| Full rebuild speed | Baseline | Slightly slower (double read for all WADs) |
| Incremental rebuild speed | Same I/O regardless | Much less I/O when few WADs change |
| Implementation complexity | Simple | Moderate (provenance tracking, two passes) |
| Parallel collection | Yes (current) | Yes (pass 1 is parallel, pass 2 is sequential) |
| Parallel patching | Yes | Yes (unchanged) |

## When to Implement

Consider implementing this when:
- Users report high memory usage with 10+ mods enabled
- Mods frequently contain large textures (4K skins, map retextures)
- Incremental builds are the common case (users rarely change their full mod list)

Do NOT implement if:
- Typical mod sets stay under ~200MB of override data
- Full rebuilds are the common case (negates the benefit, adds I/O overhead)

## Migration Path

1. Add `read_wad_override_hashes` with default implementation (backwards compatible)
2. Optimize `FsModContent` implementation (read + hash + drop, one file at a time)
3. Optimize `ModpkgContent` / `FantomeContent` (stream through archive)
4. Restructure `collect_all_overrides` into two passes
5. Add provenance tracking
6. Benchmark incremental vs full rebuild to validate the tradeoff
