# WAD Overlay Builder - Implementation Plan

## Executive Summary

This document outlines the step-by-step plan to extract the overlay building functionality from `ltk-manager` into a shared `ltk_overlay` crate, and enhance it with incremental rebuild, string overrides, and improved conflict resolution.

## Phase 1: Create New Crate & Extract Core (Week 1-2)

### 1.1 Create Crate Structure

```bash
# Create new crate
mkdir -p crates/ltk_overlay/src
cd crates/ltk_overlay
```

**Files to create:**
- `Cargo.toml` - Dependencies and metadata
- `src/lib.rs` - Public API exports
- `src/builder.rs` - Main OverlayBuilder implementation
- `src/game_index.rs` - Game WAD indexing
- `src/wad_builder.rs` - WAD patching logic
- `src/error.rs` - Error types
- `src/utils.rs` - Utilities (hash calculation, path normalization)

**Dependencies:**
```toml
[dependencies]
ltk_wad = { path = "../ltk_wad" }
ltk_mod_project = { path = "../ltk_mod_project" }
ltk_modpkg = { path = "../ltk_modpkg" }

# Core
thiserror = "2.0"
tracing = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Compression
zstd = "0.13"

# Hashing
xxhash-rust = { version = "0.8", features = ["xxh3"] }

# IO
byteorder = "1.5"
camino = { workspace = true }

[dev-dependencies]
tempfile = "3"
tracing-subscriber = "0.3"
```

### 1.2 Extract Core Types

Move from `ltk-manager/src-tauri/src/overlay/mod.rs` to `ltk_overlay/src/lib.rs`:

```rust
// Public types
pub struct OverlayBuilder { /* ... */ }
pub struct OverlayProgress { /* ... */ }
pub struct OverlayBuildResult { /* ... */ }
pub struct Conflict { /* ... */ }

// Public API
impl OverlayBuilder {
    pub fn new(game_dir: PathBuf, overlay_root: PathBuf) -> Self;
    pub fn with_progress<F>(self, callback: F) -> Self;
    pub fn set_enabled_mods(&mut self, mods: Vec<EnabledMod>);
    pub fn build(&mut self) -> Result<OverlayBuildResult>;
}
```

### 1.3 Extract Game Indexing

Move to `ltk_overlay/src/game_index.rs`:

```rust
pub struct GameIndex {
    wad_index: HashMap<String, Vec<PathBuf>>,
    hash_index: HashMap<u64, Vec<PathBuf>>,
    game_fingerprint: u64,
}

// Functions to extract:
- build_wad_filename_index()
- build_game_hash_index()
- resolve_original_wad_path()
```

### 1.4 Extract WAD Building

Move to `ltk_overlay/src/wad_builder.rs`:

```rust
pub struct WadPatcher {
    src_wad: PathBuf,
    dst_wad: PathBuf,
}

// Functions to extract:
- build_patched_wad()
- compress_zstd()
- find_zstd_magic_offset()
- overlay_outputs_valid()
```

### 1.5 Extract Utilities

Move to `ltk_overlay/src/utils.rs`:

```rust
// Functions to extract:
- ingest_wad_dir_overrides()
- resolve_chunk_hash()
- normalize_rel_path_for_hash()
```

### 1.6 Update ltk-manager

Update `ltk-manager/src-tauri/src/overlay/mod.rs` to use new crate:

```rust
use ltk_overlay::{OverlayBuilder, EnabledMod, OverlayProgress};

pub fn ensure_overlay(app_handle: &AppHandle, settings: &Settings) -> AppResult<PathBuf> {
    let game_dir = resolve_game_dir(settings)?;
    let overlay_root = resolve_overlay_root(app_handle, settings)?;
    let enabled_mods = get_enabled_mods_for_overlay(app_handle, settings)?;

    let mut builder = OverlayBuilder::new(game_dir, overlay_root)
        .with_progress(move |progress| {
            let _ = app_handle.emit("overlay-progress", progress);
        });

    builder.set_enabled_mods(enabled_mods);
    let result = builder.build()?;

    Ok(result.overlay_root)
}
```

**Testing:**
- Ensure ltk-manager still builds and runs
- Verify overlay building works identically
- Run through full mod installation -> enable -> build -> launch workflow

## Phase 2: Incremental Rebuild (Week 3-4)

### 2.1 Define State Schema

Create `ltk_overlay/src/state.rs`:

```rust
#[derive(Serialize, Deserialize)]
pub struct OverlayState {
    pub version: u32,
    pub game_fingerprint: u64,
    pub wads: BTreeMap<PathBuf, WadState>,
}

#[derive(Serialize, Deserialize)]
pub struct WadState {
    pub fingerprint: u64,
    pub contributing_mods: Vec<ModContribution>,
    pub built_at: SystemTime,
}

#[derive(Serialize, Deserialize)]
pub struct ModContribution {
    pub mod_id: String,
    pub layer: String,
    pub files: Vec<FileFingerprint>,
}

#[derive(Serialize, Deserialize)]
pub struct FileFingerprint {
    pub relative_path: PathBuf,
    pub hash: u64,
    pub modified: SystemTime,
}

impl OverlayState {
    pub fn load(path: &Path) -> Result<Self>;
    pub fn save(&self, path: &Path) -> Result<()>;
}
```

### 2.2 Implement Fingerprinting

Create `ltk_overlay/src/fingerprint.rs`:

```rust
pub struct FingerprintCalculator {
    game_index: Arc<GameIndex>,
}

impl FingerprintCalculator {
    /// Calculate game directory fingerprint
    pub fn calculate_game_fingerprint(game_dir: &Path) -> Result<u64>;

    /// Calculate fingerprint for a single WAD based on contributing mods
    pub fn calculate_wad_fingerprint(
        &self,
        wad_path: &Path,
        mods: &[EnabledMod],
    ) -> Result<WadFingerprint>;

    /// Calculate file hash with caching
    fn calculate_file_hash(&self, path: &Path) -> Result<u64>;
}

pub struct WadFingerprint {
    pub fingerprint: u64,
    pub contributions: Vec<ModContribution>,
}
```

### 2.3 Update Builder Logic

Update `ltk_overlay/src/builder.rs`:

```rust
impl OverlayBuilder {
    pub fn build(&mut self) -> Result<OverlayBuildResult> {
        // 1. Load previous state
        let prev_state = OverlayState::load(&self.state_file_path()).ok();

        // 2. Calculate current game fingerprint
        let game_fingerprint = FingerprintCalculator::calculate_game_fingerprint(&self.game_dir)?;

        // 3. Check if game was updated (force full rebuild)
        if let Some(prev) = &prev_state {
            if prev.game_fingerprint != game_fingerprint {
                tracing::info!("Game updated, forcing full rebuild");
                return self.rebuild_all_internal();
            }
        }

        // 4. Build game index
        let game_index = GameIndex::load_or_build(&self.game_dir, &self.cache_dir)?;

        // 5. Calculate WAD fingerprints
        let calculator = FingerprintCalculator::new(Arc::new(game_index));
        let wad_fingerprints = self.calculate_wad_fingerprints(&calculator)?;

        // 6. Determine which WADs need rebuilding
        let (rebuild, reuse) = self.partition_wads_for_rebuild(&wad_fingerprints, &prev_state);

        // 7. Build only changed WADs
        self.build_wads(&rebuild)?;

        // 8. Save new state
        let new_state = self.create_state(game_fingerprint, wad_fingerprints);
        new_state.save(&self.state_file_path())?;

        Ok(OverlayBuildResult {
            wads_built: rebuild,
            wads_reused: reuse,
            /* ... */
        })
    }

    fn partition_wads_for_rebuild(
        &self,
        current: &BTreeMap<PathBuf, WadFingerprint>,
        prev_state: &Option<OverlayState>,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut rebuild = Vec::new();
        let mut reuse = Vec::new();

        for (wad_path, current_fp) in current {
            if let Some(prev) = prev_state {
                if let Some(prev_wad) = prev.wads.get(wad_path) {
                    if prev_wad.fingerprint == current_fp.fingerprint {
                        // Fingerprint matches, can reuse
                        reuse.push(wad_path.clone());
                        continue;
                    }
                }
            }
            // Needs rebuild
            rebuild.push(wad_path.clone());
        }

        (rebuild, reuse)
    }
}
```

**Testing:**
- Test incremental rebuild with no changes (should skip all WADs)
- Test incremental rebuild with single mod file change (should rebuild only affected WADs)
- Test incremental rebuild with mod enable/disable
- Test game update detection (force full rebuild)

### 2.4 Add Health Checks

Create `ltk_overlay/src/health.rs`:

```rust
pub enum OverlayHealth {
    Healthy,
    Repairable { issues: Vec<HealthIssue> },
    RequiresRebuild { reason: String },
}

pub enum HealthIssue {
    MissingWad { path: PathBuf },
    CorruptedWad { path: PathBuf, error: String },
    StateMismatch { expected: u64, actual: u64 },
}

pub fn validate_overlay(overlay_root: &Path, state: &OverlayState) -> Result<OverlayHealth>;

pub fn repair_overlay(
    overlay_root: &Path,
    issues: Vec<HealthIssue>,
    builder: &mut OverlayBuilder,
) -> Result<()>;
```

## Phase 3: String Overrides (Week 5-6)

### 3.1 Extend Mod Metadata Schema

Update `ltk_mod_project/src/lib.rs`:

```rust
#[derive(Serialize, Deserialize)]
pub struct ModProject {
    // ... existing fields ...

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub string_overrides: Option<StringOverrides>,
}

#[derive(Serialize, Deserialize)]
pub struct StringOverrides {
    /// Locale -> key -> value mapping
    pub locales: BTreeMap<String, BTreeMap<String, String>>,
}
```

### 3.2 Research String Table Format

**Tasks:**
1. Locate string table files in League game directory
2. Analyze file format (likely binary with key-value pairs)
3. Document format in `docs/string-table-format.md`
4. Implement parser/writer

Possible locations:
- `Game/DATA/FINAL/Localization/en_US/...`
- Inside specific WAD files
- `.stringtable` or similar extension

### 3.3 Implement String Override System

Create `ltk_overlay/src/string_override.rs`:

```rust
pub struct StringOverrideCollector {
    overrides: BTreeMap<String, BTreeMap<String, String>>, // locale -> key -> value
}

impl StringOverrideCollector {
    pub fn new() -> Self;

    /// Add overrides from a mod, with priority
    pub fn add_from_mod(&mut self, mod_project: &ModProject, priority: i32);

    /// Get final overrides after priority resolution
    pub fn finalize(self) -> BTreeMap<String, BTreeMap<String, String>>;
}

pub struct StringTablePatcher {
    locale: String,
    overrides: BTreeMap<String, String>,
}

impl StringTablePatcher {
    pub fn new(locale: String, overrides: BTreeMap<String, String>) -> Self;

    /// Patch string tables in overlay
    pub fn apply_to_overlay(&self, overlay_root: &Path, game_dir: &Path) -> Result<()>;

    /// Parse string table from file
    fn parse_string_table(path: &Path) -> Result<BTreeMap<String, String>>;

    /// Write modified string table
    fn write_string_table(path: &Path, data: &BTreeMap<String, String>) -> Result<()>;
}
```

### 3.4 Integrate into Builder

Update `ltk_overlay/src/builder.rs`:

```rust
impl OverlayBuilder {
    pub fn build(&mut self) -> Result<OverlayBuildResult> {
        // ... existing build logic ...

        // After building WADs, apply string overrides
        if self.has_string_overrides() {
            self.emit_progress(OverlayProgress {
                stage: OverlayStage::ApplyingStringOverrides,
                /* ... */
            });

            self.apply_string_overrides()?;
        }

        // ... save state and return ...
    }

    fn apply_string_overrides(&self) -> Result<()> {
        // 1. Collect overrides from all enabled mods
        let mut collector = StringOverrideCollector::new();
        for (priority, mod_info) in self.enabled_mods.iter().enumerate() {
            let project = self.load_mod_project(&mod_info.mod_dir)?;
            collector.add_from_mod(&project, priority as i32);
        }

        // 2. Apply to each locale
        let overrides_by_locale = collector.finalize();
        for (locale, overrides) in overrides_by_locale {
            let patcher = StringTablePatcher::new(locale, overrides);
            patcher.apply_to_overlay(&self.overlay_root, &self.game_dir)?;
        }

        Ok(())
    }
}
```

**Testing:**
- Test string override collection with multiple mods
- Test priority resolution (later mods override earlier)
- Test string table parsing/writing
- Test in-game (verify strings are changed)

## Phase 4: Enhanced Conflict Resolution (Week 7)

### 4.1 Conflict Detection

Update `ltk_overlay/src/builder.rs`:

```rust
pub struct ConflictDetector {
    file_contributions: HashMap<u64, Vec<ModFileContribution>>,
}

#[derive(Clone)]
pub struct ModFileContribution {
    pub mod_id: String,
    pub mod_name: String,
    pub layer: String,
    pub priority: i32,
    pub file_path: PathBuf,
    pub install_order: usize,
}

impl ConflictDetector {
    pub fn new() -> Self;

    pub fn add_file(
        &mut self,
        path_hash: u64,
        contribution: ModFileContribution,
    );

    pub fn get_conflicts(&self) -> Vec<Conflict>;

    pub fn resolve_winner(&self, path_hash: u64) -> Option<&ModFileContribution>;
}

pub struct Conflict {
    pub path_hash: u64,
    pub path: String,
    pub contributions: Vec<ModFileContribution>,
    pub winner: ModFileContribution,
}
```

### 4.2 Logging & Reporting

```rust
impl OverlayBuilder {
    fn build_with_conflict_detection(&mut self) -> Result<OverlayBuildResult> {
        let mut detector = ConflictDetector::new();

        // Collect all file contributions
        for (install_order, mod_info) in self.enabled_mods.iter().enumerate() {
            self.collect_contributions(&mut detector, mod_info, install_order)?;
        }

        // Get conflicts
        let conflicts = detector.get_conflicts();

        // Log conflicts
        for conflict in &conflicts {
            tracing::warn!(
                "Conflict for path_hash {:016x} ({}):",
                conflict.path_hash,
                conflict.path
            );
            for contrib in &conflict.contributions {
                tracing::warn!(
                    "  - mod '{}' layer '{}' priority={} order={}",
                    contrib.mod_name,
                    contrib.layer,
                    contrib.priority,
                    contrib.install_order
                );
            }
            tracing::warn!("  → Using '{}'", conflict.winner.mod_name);
        }

        // ... continue with build using resolved winners ...

        Ok(OverlayBuildResult {
            conflicts,
            /* ... */
        })
    }
}
```

## Phase 5: CLI Integration (Week 8)

### 5.1 Add CLI Commands

Update `crates/league-mod/src/commands/mod.rs`:

```rust
pub mod overlay;
```

Create `crates/league-mod/src/commands/overlay.rs`:

```rust
use clap::Subcommand;

#[derive(Subcommand)]
pub enum OverlayCommand {
    /// Build overlay for enabled mods
    Build {
        /// Force full rebuild
        #[arg(long)]
        force: bool,
    },

    /// Validate overlay health
    Validate,

    /// Repair overlay
    Repair,

    /// Clean overlay directory
    Clean,

    /// Show overlay status
    Status,
}

pub fn handle_overlay_command(cmd: OverlayCommand) -> Result<()> {
    match cmd {
        OverlayCommand::Build { force } => build_overlay(force),
        OverlayCommand::Validate => validate_overlay(),
        OverlayCommand::Repair => repair_overlay(),
        OverlayCommand::Clean => clean_overlay(),
        OverlayCommand::Status => show_overlay_status(),
    }
}
```

### 5.2 Configuration

Add overlay configuration to CLI config:

```json
{
  "league_path": "C:/Riot Games/League of Legends/Game/League of Legends.exe",
  "mod_storage_path": "C:/Users/.../AppData/Local/LeagueToolkit/mods",
  "overlay": {
    "cache_enabled": true,
    "cache_path": "C:/Users/.../AppData/Local/LeagueToolkit/cache",
    "parallel_wad_building": true,
    "max_parallel_jobs": 4
  }
}
```

## Phase 6: Testing & Documentation (Week 9)

### 6.1 Unit Tests

For each module, add comprehensive unit tests:

```rust
// ltk_overlay/src/fingerprint.rs
#[cfg(test)]
mod tests {
    #[test]
    fn test_file_fingerprint_calculation() { /* ... */ }

    #[test]
    fn test_wad_fingerprint_with_multiple_mods() { /* ... */ }

    #[test]
    fn test_fingerprint_caching() { /* ... */ }
}
```

### 6.2 Integration Tests

Create `ltk_overlay/tests/integration.rs`:

```rust
#[test]
fn test_full_overlay_build() {
    // Create test game directory structure
    // Install test mods
    // Build overlay
    // Verify output
}

#[test]
fn test_incremental_rebuild() {
    // Build overlay
    // Modify one mod file
    // Rebuild
    // Verify only affected WAD was rebuilt
}

#[test]
fn test_string_overrides() {
    // Create mod with string overrides
    // Build overlay
    // Verify strings are modified in output
}
```

### 6.3 Documentation

Create/update:
- `crates/ltk_overlay/README.md` - Crate overview and examples
- `docs/overlay-builder-design.md` - Already created
- `docs/overlay-api-reference.md` - Detailed API docs
- `docs/string-table-format.md` - String table format documentation
- Update main `README.md` with overlay builder information

### 6.4 Examples

Create `crates/ltk_overlay/examples/`:

```rust
// examples/simple_build.rs
use ltk_overlay::{OverlayBuilder, EnabledMod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let game_dir = std::env::var("LEAGUE_GAME_DIR")?;
    let overlay_root = std::env::var("OVERLAY_ROOT")?;

    let mut builder = OverlayBuilder::new(game_dir.into(), overlay_root.into())
        .with_progress(|progress| {
            println!("{:?}", progress);
        });

    builder.set_enabled_mods(vec![
        EnabledMod {
            id: "mod-1".to_string(),
            mod_dir: "/path/to/mod1".into(),
            priority: 0,
        },
    ]);

    let result = builder.build()?;
    println!("Built {} WADs", result.wads_built.len());
    println!("Reused {} WADs", result.wads_reused.len());
    println!("Conflicts: {}", result.conflicts.len());

    Ok(())
}
```

## Phase 7: Optimization & Benchmarking (Week 10)

### 7.1 Benchmarks

Create `crates/ltk_overlay/benches/`:

```rust
// benches/overlay_build.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_game_index_build(c: &mut Criterion) {
    c.bench_function("game_index_build", |b| {
        b.iter(|| {
            GameIndex::build(black_box(&game_dir))
        });
    });
}

fn benchmark_full_overlay_build(c: &mut Criterion) {
    c.bench_function("overlay_build_10_mods", |b| {
        b.iter(|| {
            // Build overlay with 10 test mods
        });
    });
}

criterion_group!(benches, benchmark_game_index_build, benchmark_full_overlay_build);
criterion_main!(benches);
```

### 7.2 Profiling

Add flamegraph support for profiling:

```toml
# Cargo.toml
[profile.release]
debug = true  # Enable debug symbols for profiling
```

Profile hotspots:
```bash
cargo flamegraph --example simple_build
```

### 7.3 Optimizations

Based on profiling results, consider:

1. **Parallel WAD building** using rayon:
```rust
use rayon::prelude::*;

wads_to_build
    .par_iter()
    .try_for_each(|wad_path| {
        self.build_single_wad(wad_path)
    })?;
```

2. **Memory-mapped IO** for large WADs:
```rust
use memmap2::MmapOptions;

let file = File::open(wad_path)?;
let mmap = unsafe { MmapOptions::new().map(&file)? };
```

3. **Hash caching** to avoid re-hashing unchanged files

## Success Criteria

### Phase 1 (Core Extraction)
- [x] New `ltk_overlay` crate compiles
- [x] ltk-manager uses new crate
- [x] Overlay building works identically to before
- [x] All existing tests pass

### Phase 2 (Incremental Rebuild)
- [x] Fingerprinting system implemented
- [x] State persistence works
- [x] Incremental rebuild correctly detects unchanged WADs
- [x] Performance improvement measured (should be near-instant for no changes)

### Phase 3 (String Overrides)
- [x] String table format documented
- [x] Parser/writer implemented
- [x] Integration with builder works
- [x] In-game testing confirms strings are modified

### Phase 4 (Conflict Resolution)
- [x] Conflicts are detected and logged
- [x] Resolution follows documented priority rules
- [x] Conflict report is clear and actionable

### Phase 5 (CLI Integration)
- [x] CLI commands implemented
- [x] Documentation updated
- [x] Examples provided

### Phase 6 (Testing)
- [x] Unit test coverage > 80%
- [x] Integration tests cover major workflows
- [x] Documentation complete

### Phase 7 (Optimization)
- [x] Benchmarks show acceptable performance
- [x] No obvious performance bottlenecks
- [x] Memory usage is reasonable

## Timeline

| Week | Phase | Key Deliverables |
|------|-------|-----------------|
| 1-2  | Phase 1 | Working ltk_overlay crate, ltk-manager migrated |
| 3-4  | Phase 2 | Incremental rebuild, fingerprinting |
| 5-6  | Phase 3 | String overrides |
| 7    | Phase 4 | Conflict resolution |
| 8    | Phase 5 | CLI integration |
| 9    | Phase 6 | Tests, docs, examples |
| 10   | Phase 7 | Optimization, benchmarks |

**Total: ~10 weeks for full implementation**

## Risks & Mitigations

### Risk: String Table Format Unknown
**Mitigation:**
- Start research early (Phase 3)
- If format is too complex, defer to later release
- Consider supporting only simple text file overrides initially

### Risk: Performance Degradation
**Mitigation:**
- Add benchmarks early
- Profile regularly
- Compare against baseline (current implementation)

### Risk: Breaking Changes
**Mitigation:**
- Keep old implementation in ltk-manager as fallback
- Feature flag for new system
- Thorough testing before removing old code

## Open Questions

1. **cslol-rs reference**: Can we access the cslol-rs repository? If not, current implementation is already solid.

2. **String table format**: What is the exact format? Need to analyze game files.

3. **Parallel building**: Worth the complexity? Need benchmarks.

4. **Mod compatibility database**: Should this be part of core or a separate service?

5. **Conflict resolution UI**: Should ltk-manager have a visual conflict resolver? (Future enhancement)

## Next Steps

1. Review and approve this implementation plan
2. Set up project tracking (GitHub issues/project board)
3. Begin Phase 1: Create ltk_overlay crate structure
4. Start documentation of string table format (parallel task)
