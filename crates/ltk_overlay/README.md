# ltk_overlay

WAD overlay/profile builder for League of Legends mods.

## Overview

`ltk_overlay` is a Rust library that builds WAD overlay directories from enabled mods, allowing the League of Legends patcher to load modded assets. It provides:

- **Incremental rebuilds**: Only rebuild WADs that have changed
- **Cross-WAD matching**: Distribute mod files to all affected WADs (e.g., champion assets in Map WADs)
- **Layer system**: Respect mod layer priorities for proper override resolution
- **String overrides**: Apply metadata-driven string table modifications
- **Conflict resolution**: Detect and resolve conflicts between multiple mods

## Architecture

The overlay builder follows this workflow:

1. **Index game files**: Scan the `DATA/FINAL` directory and build:
   - WAD filename index (case-insensitive lookup)
   - Hash index (path_hash → list of WADs containing that chunk)

2. **Collect mod overrides**: Walk enabled mods and collect all override files with their hashes

3. **Distribute to WADs**: Use the hash index to find all WADs that need each override

4. **Build WADs**: For each affected WAD:
   - Load base WAD from game directory
   - Apply overrides
   - Write patched WAD to overlay directory
   - Preserve compression types and metadata

5. **Apply string overrides**: Modify string tables based on mod metadata

## Usage

```rust
use ltk_overlay::{OverlayBuilder, EnabledMod};
use std::path::PathBuf;

let game_dir = PathBuf::from("C:/Riot Games/League of Legends/Game");
let overlay_root = PathBuf::from("C:/Users/.../overlay");

let mut builder = OverlayBuilder::new(game_dir, overlay_root)
    .with_progress(|progress| {
        println!("Stage: {:?}, Progress: {}/{}",
            progress.stage, progress.current, progress.total);
    });

builder.set_enabled_mods(vec![
    EnabledMod {
        id: "my-mod".to_string(),
        mod_dir: PathBuf::from("/path/to/mod"),
        priority: 0,
    },
]);

let result = builder.build()?;
println!("Built {} WADs, reused {}",
    result.wads_built.len(), result.wads_reused.len());
```

## Integration

This crate is used by:

- **ltk-manager**: Tauri-based GUI mod manager
- **league-mod**: CLI tool for mod developers

## Implementation Status

- [x] Core types and API design
- [ ] Game indexing (Phase 1)
- [ ] WAD patching (Phase 1)
- [ ] Incremental rebuild (Phase 2)
- [ ] String overrides (Phase 3)
- [ ] Conflict resolution (Phase 4)
- [ ] Health checks (Phase 4)

See `docs/overlay-implementation-plan.md` for the full roadmap.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
