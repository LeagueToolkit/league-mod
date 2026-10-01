## Fantome Format Structure

The library creates ZIP files with this structure. `META/`, `WAD/` and `RAW/` follow the
[official Fantome specification](https://github.com/LeagueToolkit/Fantome/wiki/Mod-File-Format).
`WAD_<layer>/`, `META/hashes/` and `META/game_data/` are LeagueToolkit extensions.

```
my_mod_1.0.0.fantome
├── META/
│   ├── info.json              # Mod metadata
│   ├── README.md              # Project documentation (optional)
│   ├── image.png              # Mod thumbnail (optional)
│   ├── hashes/                # Declared hashtables (optional)
│   └── game_data/<layer>/     # Game-data override files (optional)
├── WAD/                       # Base layer
│   ├── Aatrox.wad.client      # A packed WAD, stored
│   └── Map11.wad.client/      # A WAD as a directory of its files
│       ├── data/
│       └── assets/
├── WAD_pink/                  # Layer "pink"
│   └── Aatrox.wad.client
└── RAW/                       # Base-layer files by game asset path (optional)
```

### Layer WAD directories

- `WAD/` holds the base layer's WADs. `WAD_<layer>/` holds the WADs of the layer named
  `<layer>`. `wad_entry_name` spells both.
- A layer name in a WAD directory is one or more ASCII letters, digits, `-` or `_`
  (`is_layer_name`). `WAD_base/` names no layer. An entry under a directory that breaks either
  rule is not placed, and the writer refuses such a layer with `InvalidLayerName`.
- The `WAD/` and `WAD_` prefixes match case-insensitively. A `WAD_<layer>/` directory belongs to
  the `Layers` entry whose name matches `<layer>` case-insensitively. `extract_wads` writes it
  under the declared spelling.
- A `WAD_<layer>/` directory that `Layers` does not declare loads as a layer of that name at
  priority 0 (`FantomeInfo::declare_layers`).
- A packed WAD is stored, not deflated, in every layer's WAD directory.
- `RAW/` belongs to the base layer. No other layer has raw files.
- A reader that predates layers, cslol-manager included, reads `WAD/` and `RAW/` and skips
  `WAD_<layer>/`. It loads the base layer alone and keeps the layer table it does not use.

ADR-0036 records the choice of `WAD_<layer>/`.

### Metadata Structure

The `info.json` file contains metadata in the format expected by Fantome:

```json
{
  "Name": "Display Name",
  "Author": "Author Name",
  "Version": "1.0.0",
  "Description": "Mod description",
  "Generator": "ltk_mod_project 0.9.2",
  "Layers": {
    "pink": {
      "Name": "pink",
      "DisplayName": "Pink Chroma",
      "Priority": 10,
      "StringOverrides": {
        "en_us": { "field3": "New String" }
      }
    }
  }
}
```

- `Generator` names the packing tool and its version. It is a LeagueToolkit extension and
  optional.
- `Layers` is keyed by layer name. `Name` is the layer name and matches the key. A pack declares
  every layer other than base, and the base layer when it carries string overrides.
- `StringOverrides` maps a locale (`en_us`, `ko_kr` or `default`) to field replacements.

## StringOverrides

String overrides define new string values for specific fields.
Full list of fields and their default value are located in file `data/menu/en_us/lol.stringtable`, that is located in `Localized/Global.{locale}.wad.client`
Approach of shipping only overrides in mod is due to need to have `lol.stringtable` file *ALWAYS* up to date.
Tool that allows converting between `.stringtable` and `.json` is [Rion](https://github.com/Roshaless/Rion/releases)

## Normalized archives

A `WAD/` entry may hold a whole packed `.wad.client` rather than a directory of
loose files. The zip format lets such an entry be deflated like any other, and
the tools in the wild write it that way - but a deflated entry has to be
inflated whole before any chunk inside it can be reached, which puts the entire
WAD in memory for the sake of the few chunks a reader wants.

An archive is **normalized** when its packed WADs are held `Stored`: the same
bytes, now a byte range a reader seeks into. Nothing else changes shape - the
metadata, the loose files and the hashtables stay deflated, because they are
read whole or not at all. `normalize_archive` performs the conversion, and only
an importer working on a copy it owns should call it; see
`docs/adr/0002-normalization-happens-at-import-never-at-build.md`.

Both forms are valid Fantome archives, and every reader in this repo accepts
either.

## Limitations

- **Fixed structure**: Must follow the exact WAD folder structure expected by League of Legends
