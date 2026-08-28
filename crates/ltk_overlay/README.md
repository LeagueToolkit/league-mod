# ltk_overlay

WAD overlay/profile builder for League of Legends mods.

## Overview

`ltk_overlay` is a Rust library that builds WAD overlay directories from enabled mods, allowing the League of Legends patcher to load modded assets. It provides:

- **Incremental rebuilds**: Rebuild only the WADs that changed, and rebuild those
  by rewriting the bytes the mod supplies rather than recopying the WAD
- **Cross-WAD matching**: Distribute mod files to all affected WADs (e.g., champion assets in Map WADs)
- **Layer system**: Respect mod layer priorities for proper override resolution
- **String overrides**: Apply metadata-driven string table modifications
- **Load-order resolution**: When several mods override one chunk, the mod
  earliest in the enabled list wins

## Architecture

The overlay builder runs two passes over the mods, with routing in between:

1. **Index game files**: Scan the `DATA/FINAL` directory and build:
   - WAD filename index (case-insensitive lookup)
   - Hash index (path_hash → list of WADs containing that chunk)

2. **Pass 1 - metadata**: Walk enabled mods, hash every override file, then drop
   the bytes. A per-mod cache skips mods that have not changed.

3. **Distribute to WADs**: Use the hash index to find all WADs that need each
   override, and compare per-WAD fingerprints to decide which need rebuilding.

4. **Pass 2 - bytes**: Re-read override bytes only for WADs being rebuilt, and
   compress each distinct content once.

5. **Write WADs**, in parallel, each one of two ways:
   - **Full rebuild**: copy the game WAD's data region as one block, append the
     overrides as a tail, write the TOC, and rename into place.
   - **Tail rewrite**: when the WAD's chunk set is unchanged and its recorded
     layout checks out against both files, keep the copied region and rewrite
     only the tail and the TOC. This is what makes editing a mod cost the mod's
     own bytes rather than the WAD's size.

6. **Apply string overrides**: Modify string tables based on mod metadata

`docs/overlay-builder-design.md` covers the file layout, the trust rules behind
the tail rewrite, and the state files.

## Usage

```rust
use ltk_overlay::{OverlayBuilder, EnabledMod, FsModContent};
use camino::Utf8PathBuf;

let game_dir = Utf8PathBuf::from("C:/Riot Games/League of Legends/Game");
let profile_dir = Utf8PathBuf::from("C:/Users/.../profiles/default");
let overlay_root = profile_dir.join("overlay");

let mut builder = OverlayBuilder::new(game_dir, overlay_root, profile_dir)
    .with_progress(|progress| {
        println!("Stage: {:?}, Progress: {}/{}",
            progress.stage, progress.current, progress.total);
    });

builder.set_enabled_mods(vec![
    EnabledMod {
        id: "my-mod".to_string(),
        content: Box::new(FsModContent::new(Utf8PathBuf::from("/path/to/mod"))),
        enabled_layers: None,
    },
]);

let result = builder.build()?;
println!("Built {} WADs in {:?}", result.wads_built.len(), result.build_time);
```

## Integration

This crate is used by:

- **league-mod**: CLI tool for mod developers, in this workspace
- **ltk-manager**: Tauri GUI mod manager, in
  [its own repository](https://github.com/LeagueToolkit/ltk-manager), which
  depends on the published crate

## Implementation status

Game indexing, WAD patching, incremental rebuild, string overrides and the
linked-bin pre-flight all ship. Conflict *detection* between mods is not
implemented: `OverlayBuildResult::conflicts` is always empty, and overlapping
overrides resolve by load order (first mod in the list wins).

TOC slack is disabled (`TOC_SLACK_ENTRIES = 0`), so a mod that adds or removes a
chunk from a WAD takes the full-rebuild path. Enabling it needs an in-game test
that the client tolerates a gap between the last TOC entry and the first data
byte.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
