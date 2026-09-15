# ltk_game_data

Shared declaration types, authoring loaders, and bin materialisation for League Toolkit
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
use ltk_game_data::{compile, materialise};

let program = compile("game_data.json", r#"{
  "version": 1,
  "modules": [{"target":"shared", "links": ["mods/example"]}]
}"#, |path| Err(ltk_game_data::Error::new(path, "source unavailable")))?;

// PROP v3 with an empty dependency list and object table.
let base = b"PROP\x03\0\0\0\0\0\0\0\0\0\0\0";
let result = materialise(base, &program.modules[0].steps)?;
assert_eq!(result.dependencies, ["mods/example"]);
# Ok::<(), ltk_game_data::Error>(())
```

Project integration is `ltk_mod_project::game_data::load_layer`. Archive layer metadata
carries `Document`. Overlay providers expose `ModContentProvider::game_data`; build
diagnostics are available through `OverlayBuilder::game_data_reports`.
