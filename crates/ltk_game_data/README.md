# ltk_game_data

Shared declaration types, authoring loaders, and BIN application for League Toolkit
consumers. The [game-data spec](../../docs/design/game-data.md) defines the supported surface
and pipeline contract.

## Example

`content/base/game_data.yaml`:

```yaml
version: 1
modules:
  - target: data/characters/teemo/skins/skin0.bin
    links:
      - mods/example/particles.bin
```

```rust
use ltk_game_data::{load_declarations, apply};

let declarations = load_declarations("game_data.json", r#"{
  "version": 1,
  "modules": [{"target":"shared", "links": ["mods/example"]}]
}"#, |path| Err(ltk_game_data::Error::new(path, "source unavailable")))?;

// PROP v3 with an empty dependency list and object table.
let base = b"PROP\x03\0\0\0\0\0\0\0\0\0\0\0";
let result = apply(base, &declarations.modules[0].steps)?;
assert_eq!(result.dependencies, ["mods/example"]);
# Ok::<(), ltk_game_data::Error>(())
```

Project integration is `ltk_mod_project::game_data::load_layer`. Archive layer metadata
carries `DeclarationDocument`. Overlay providers expose `ModContentProvider::game_data_declarations`; build
diagnostics are available through `OverlayBuildResult::game_data_diagnostics`.
