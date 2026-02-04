# WAD Overlay/Profile Builder Design

## Overview

This document outlines the design for a WAD overlay/profile builder that will allow the league-mod toolkit to install and run mods with the patcher. The overlay builder will be implemented as a shared Rust crate (`ltk_overlay`) that can be used by both the CLI and GUI applications.

## Current State

The `ltk-manager` already has a working overlay implementation in `crates/ltk-manager/src-tauri/src/overlay/mod.rs` with the following features:

- **Hash-based incremental rebuild**: Detects when enabled mods haven't changed to avoid rebuilding
- **Cross-WAD matching**: Distributes overrides to all affected WAD files using a game-wide hash index
- **Layer support**: Respects mod layer priorities
- **Compression handling**: Properly handles different WAD chunk compression types (None, Zstd, ZstdMulti)
- **Full WAD replacement**: Supports pre-built .wad.client files

## Architecture Goals

### 1. Shared Crate (`ltk_overlay`)

Extract the overlay building logic into a new workspace crate that can be shared between:
- `ltk-manager` (Tauri GUI application)
- `league-mod` (CLI tool)
- Future tools

**Key Design Principles:**
- **No GUI dependencies**: Core logic should be independent of Tauri/UI frameworks
- **Progress callbacks**: Support optional progress reporting via callbacks/channels
- **Testable**: Core logic should be unit-testable
- **Minimal dependencies**: Keep the dependency tree lean

### 2. Incremental Rebuild System

The overlay builder should intelligently detect changes and rebuild only what's necessary:

#### Current Implementation
- Compares enabled mod IDs against previous state
- Rebuilds entire overlay if mods changed

#### Proposed Improvements

**Per-WAD Fingerprinting:**
```rust
struct WadFingerprint {
    wad_path: PathBuf,
    contributing_mods: Vec<ModContribution>,
    hash: u64, // Combined hash of all contributing mod files
}

struct ModContribution {
    mod_id: String,
    layer: String,
    files: Vec<FileFingerprint>,
}

struct FileFingerprint {
    path: PathBuf,
    hash: u64,
    modified: SystemTime,
}
```

**Rebuild Strategy:**
1. On startup, load previous overlay state
2. For each WAD that would be built:
   - Calculate fingerprint based on contributing mod files
   - Compare with previous fingerprint
   - Skip rebuild if fingerprint matches
3. Only rebuild WADs with changed fingerprints
4. Verify rebuilt WADs are mountable (health check)

**State Persistence:**
```json
{
  "version": 3,
  "game_fingerprint": "hash of game DATA/FINAL structure",
  "wads": {
    "DATA/FINAL/Champions/Aatrox.wad.client": {
      "fingerprint": "0x123abc...",
      "contributing_mods": [
        {
          "mod_id": "uuid-1",
          "layer": "base",
          "files": [
            {"path": "data/characters/aatrox/...", "hash": "0x456..."}
          ]
        }
      ]
    }
  }
}
```

### 3. Static String Overrides

Add support for metadata-driven string table overrides to allow creators to change i18n strings in a patch-resistant way.

**Mod Metadata Extension:**
```json
{
  "name": "my-mod",
  "version": "1.0.0",
  "string_overrides": {
    "en_US": {
      "champion_aatrox_name": "The Darkin Blade (Custom)",
      "item_1001_name": "Custom Boots Name"
    },
    "zh_CN": {
      "champion_aatrox_name": "暗裔剑魔（自定义）"
    }
  }
}
```

**Implementation Approach:**
1. Collect string overrides from all enabled mods during overlay build
2. Apply overrides based on mod priority/ordering
3. Locate game string table files (`.stringtable` or similar)
4. Parse, modify, and rebuild string tables
5. Include modified string tables in overlay WADs

**Priority Resolution:**
- Mods installed later override earlier mods
- Within a mod, higher priority layers override lower priority layers
- Could add explicit priority field in metadata if needed

### 4. Cross-WAD Matching

The current implementation already supports this well. Maintain and enhance:

**Game Hash Index:**
```rust
struct GameHashIndex {
    // Maps path_hash -> list of WADs containing that chunk
    hash_to_wads: HashMap<u64, Vec<PathBuf>>,
    // Total game state fingerprint for invalidation
    game_fingerprint: u64,
}
```

**Building the Index:**
1. On first run or when game is updated, scan all WAD files in `DATA/FINAL`
2. For each WAD, extract all chunk path hashes
3. Build reverse index: `path_hash -> [wad1, wad2, ...]`
4. Cache to disk with game fingerprint

**Usage:**
When processing mod overrides:
1. Collect all override files from enabled mods
2. For each override file, get its path hash
3. Look up all WADs containing that hash
4. Distribute override to all matching WADs

This enables proper handling of assets used in multiple WADs (e.g., champion assets referenced in Map11.wad.client).

### 5. Health Checks & Repairs

Implement validation to ensure overlay integrity:

**On Startup:**
```rust
fn validate_overlay(overlay_root: &Path) -> OverlayHealth {
    // 1. Check overlay state file exists and is valid
    // 2. Verify all referenced WAD files exist
    // 3. Verify WAD files are mountable
    // 4. Check fingerprints match (for incremental rebuild)
    // 5. Validate game hasn't been patched (game fingerprint)
}

enum OverlayHealth {
    Healthy,
    Repairable { issues: Vec<Issue> },
    RequiresRebuild { reason: String },
}
```

**Repair Strategies:**
- Missing WAD: Rebuild just that WAD
- Corrupted WAD: Rebuild just that WAD
- Game patch detected: Full rebuild required
- Mod removed: Remove WAD from overlay if no longer needed

### 6. Conflict Resolution

When multiple mods modify the same file, provide clear conflict resolution:

**Current Behavior:**
- Last mod wins (based on installation order and layer priority)

**Proposed Enhancements:**

**Conflict Detection:**
```rust
struct Conflict {
    path_hash: u64,
    path: String,
    contributing_mods: Vec<ModContribution>,
}

struct ModContribution {
    mod_id: String,
    mod_name: String,
    layer: String,
    priority: i32,
}
```

**Resolution Options:**
1. **Automatic (current)**: Use priority system
2. **User prompt**: GUI could show conflicts and let user choose
3. **Mod compatibility metadata**: Mods could declare conflicts/requirements

**Logging:**
Log all conflicts with details:
```
[INFO] Conflict for data/characters/aatrox/aatrox.bin:
  - mod "aatrox-rework" (base, priority=0)
  - mod "aatrox-animation-fix" (base, priority=0)
  → Using "aatrox-animation-fix" (installed later)
```

## API Design

### Core Overlay Builder

```rust
pub struct OverlayBuilder {
    game_dir: PathBuf,
    overlay_root: PathBuf,
    enabled_mods: Vec<EnabledMod>,
    progress_callback: Option<Box<dyn Fn(OverlayProgress)>>,
}

pub struct EnabledMod {
    pub id: String,
    pub mod_dir: PathBuf,
    pub priority: i32, // Global mod priority for conflict resolution
}

#[derive(Clone)]
pub struct OverlayProgress {
    pub stage: OverlayStage,
    pub current_file: Option<String>,
    pub current: u32,
    pub total: u32,
}

#[derive(Clone)]
pub enum OverlayStage {
    Indexing,
    CollectingOverrides,
    BuildingWad { name: String },
    ApplyingStringOverrides,
    Complete,
}

impl OverlayBuilder {
    pub fn new(game_dir: PathBuf, overlay_root: PathBuf) -> Self;

    pub fn with_progress<F>(mut self, callback: F) -> Self
    where F: Fn(OverlayProgress) + 'static;

    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>);

    /// Build or update overlay, using incremental rebuild when possible
    pub fn build(&mut self) -> Result<OverlayBuildResult>;

    /// Force full rebuild, ignoring cached state
    pub fn rebuild_all(&mut self) -> Result<OverlayBuildResult>;

    /// Validate overlay health
    pub fn validate(&self) -> Result<OverlayHealth>;

    /// Repair overlay based on health check results
    pub fn repair(&mut self, health: OverlayHealth) -> Result<()>;
}

pub struct OverlayBuildResult {
    pub overlay_root: PathBuf,
    pub wads_built: Vec<PathBuf>,
    pub wads_reused: Vec<PathBuf>,
    pub conflicts: Vec<Conflict>,
    pub build_time: Duration,
}
```

### Game Index

```rust
pub struct GameIndex {
    game_dir: PathBuf,
    wad_index: HashMap<String, Vec<PathBuf>>, // lowercase filename -> paths
    hash_index: HashMap<u64, Vec<PathBuf>>,    // path_hash -> wad paths
    game_fingerprint: u64,
}

impl GameIndex {
    /// Build index from game directory
    pub fn build(game_dir: &Path) -> Result<Self>;

    /// Load cached index if valid, otherwise rebuild
    pub fn load_or_build(game_dir: &Path, cache_path: &Path) -> Result<Self>;

    /// Save index to cache
    pub fn save(&self, cache_path: &Path) -> Result<()>;

    /// Find original WAD path by filename (case-insensitive)
    pub fn find_wad(&self, filename: &str) -> Option<&PathBuf>;

    /// Find all WADs containing a specific path hash
    pub fn find_wads_with_hash(&self, hash: u64) -> Option<&[PathBuf]>;
}
```

### String Override System

```rust
pub struct StringOverrideBuilder {
    overrides: BTreeMap<String, BTreeMap<String, String>>, // locale -> key -> value
}

impl StringOverrideBuilder {
    pub fn new() -> Self;

    /// Add overrides from mod metadata
    pub fn add_from_metadata(&mut self, metadata: &ModMetadata, priority: i32);

    /// Apply overrides to game string tables
    pub fn apply_to_overlay(&self, overlay_root: &Path, game_dir: &Path) -> Result<()>;
}
```

## Migration Plan

### Phase 1: Extract Core Logic
1. Create new `crates/ltk_overlay` crate
2. Move core overlay building logic from `ltk-manager/src-tauri/src/overlay/mod.rs`
3. Remove Tauri dependencies (AppHandle, Emitter)
4. Add progress callback system
5. Update `ltk-manager` to use new crate

### Phase 2: Incremental Rebuild
1. Implement per-WAD fingerprinting
2. Add overlay state persistence (v3 format with fingerprints)
3. Implement incremental rebuild logic
4. Add tests for fingerprint calculation

### Phase 3: String Overrides
1. Extend mod metadata schema with `string_overrides`
2. Research game string table format (`.stringtable` or similar)
3. Implement string table parser/writer
4. Integrate into overlay builder
5. Update mod project config schema

### Phase 4: Enhanced Features
1. Implement health check system
2. Add repair functionality
3. Improve conflict detection and reporting
4. Add CLI commands for overlay management

### Phase 5: Integration
1. Update `league-mod` CLI to use new crate
2. Add CLI commands: `league-mod overlay build`, `league-mod overlay validate`, etc.
3. Update documentation
4. Add examples

## File Structure

```
crates/
├── ltk_overlay/
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs                 # Public API
│   │   ├── builder.rs             # OverlayBuilder implementation
│   │   ├── game_index.rs          # Game indexing logic
│   │   ├── wad.rs                 # WAD patching logic
│   │   ├── string_override.rs     # String override system
│   │   ├── fingerprint.rs         # Fingerprint calculation
│   │   ├── health.rs              # Health check & repair
│   │   ├── error.rs               # Error types
│   │   └── utils.rs               # Shared utilities
│   └── tests/
│       └── integration.rs
```

## Testing Strategy

### Unit Tests
- Fingerprint calculation
- Hash index building
- String override resolution
- Conflict detection

### Integration Tests
- Full overlay build with test mods
- Incremental rebuild scenarios
- Cross-WAD matching
- Health checks and repairs

### Benchmarks
- Overlay build time with varying mod counts
- Game index build time
- Fingerprint calculation performance

## Performance Considerations

### Current Implementation Performance
From the existing code comments, the current approach is already quite efficient:
- Hash comparison for skipping unchanged WADs (8 bytes read per WAD)
- SSDs eliminate seek penalty
- Minimal IO for checking state

### Optimizations to Maintain
1. **Parallel WAD building**: Consider using rayon to build multiple WADs in parallel
2. **Lazy game indexing**: Cache and reuse game index between runs
3. **Memory-mapped IO**: For large WAD files, consider memory mapping
4. **Zstd compression level**: Use level 3 (fast) as currently implemented

### Additional Optimizations
1. **Chunked processing**: Process mod files in chunks to reduce memory usage
2. **Fingerprint caching**: Cache file hashes to avoid re-reading unchanged files
3. **WAD TOC caching**: Cache parsed TOCs for faster lookups

## Compatibility Considerations

### cslol Compatibility
The implementation should maintain compatibility with:
- cslol-manager mod formats (when possible)
- Fantome mod format (already supported)
- modpkg format (native)

### Game Updates
When League patches:
- Detect game fingerprint change
- Invalidate overlay state
- Force full rebuild
- Preserve mod installation state

## Error Handling

### Recoverable Errors
- Missing optional mod files (skip with warning)
- Non-critical WAD mounting failures (log and continue)
- Corrupted cache (rebuild)

### Fatal Errors
- Game directory not found
- Overlay directory not writable
- Required mod files missing
- WAD building failures for required game files

## Logging Strategy

Use structured logging with different levels:

```rust
// INFO: User-facing progress
tracing::info!("Building overlay for {} mods", mod_count);

// DEBUG: Implementation details
tracing::debug!("WAD '{}' fingerprint: {:016x}", wad_name, fingerprint);

// WARN: Non-fatal issues
tracing::warn!("Mod '{}' has conflicting file: {}", mod_id, path);

// ERROR: Fatal errors
tracing::error!("Failed to mount base WAD '{}': {}", wad_path, error);
```

## Future Enhancements

### Advanced Conflict Resolution
- Visual conflict resolver in GUI
- Mod compatibility database
- Automatic compatibility patches

### Performance Profiling
- Built-in performance metrics
- Overlay build reports
- Bottleneck identification

### Mod Dependencies
- Mod can declare dependencies on other mods
- Automatic dependency resolution
- Load order optimization

### Hot Reload
- Watch mod directories for changes
- Rebuild only affected WADs on change
- Useful for mod development workflow

## References

- **Current Implementation**: `crates/ltk-manager/src-tauri/src/overlay/mod.rs`
- **cslol-rs**: Reference Rust implementation (if accessible)
- **WAD Format**: `ltk_wad` crate
- **Mod Package Format**: `ltk_modpkg` crate

## Sources

- [LeagueToolkit/cslol-manager](https://github.com/LeagueToolkit/cslol-manager) - Original C++ implementation
- Discord conversation discussing architecture requirements
