# Fantome

A Rust library for creating League of Legends mods in the legacy Fantome format.

## Overview

The `fantome` crate provides functionality to pack mod projects into the legacy `.fantome` format (renamed ZIP files) that are compatible with any current (future legacy) mod managers. This format was widely used in the League of Legends modding community before the introduction of the newer `.modpkg` format.

## Fantome Format Structure

The library creates ZIP files with this structure, following the [official Fantome specification](https://github.com/LeagueToolkit/Fantome/wiki/Mod-File-Format):

```
my_mod_1.0.0.fantome
├── META/
│   ├── info.json          # Mod metadata
│   ├── README.md          # Project documentation (optional)
│   └── image.png          # Mod thumbnail (optional)
│                                                                    
├── WAD/                                                                                      
│    ├── Aatrox.wad.client/                                                                                     
│    │   ├── data/                                                                                     
│    │   └── assets/
│ 	 └── Map11.wad.client/
│	     ├── data/
│	     └── assets/       
│																									 
└── WAD_pink/                                                                                                                    
	 └── Aatrox.wad.client/                                                                                                 
		 ├── data/                                                                                                          
		 └── assets/
```

## Usage

### Basic Example

```rust
use fantome::pack_to_fantome;
use mod_project::ModProject;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

// Load your mod project configuration
let mod_project = ModProject::load("mod.config.json")?;
let project_root = Path::new(".");

// Create output file
let file = File::create("my_mod.fantome")?;
let writer = BufWriter::new(file);

// Pack to Fantome format
pack_to_fantome(writer, &mod_project, project_root)?;
```

### Metadata Structure

The `info.json` file contains metadata in the format expected by Fantome:

```json
{
  "Name": "Display Name",
  "Author": "Author Name",
  "Version": "1.0.0",
  "Description": "Mod description"
  "Layers":{
	"pink": {
		"Name": "Pink Chroma"
		"Priority": 0
		"isActive": false
		"group": 1
	}
  }
  "StringOverrides":{
	"field": "new string"
	}
}
```

## Limitations

- **Fixed structure**: Must follow the exact WAD folder structure expected by League of Legends

## Integration with League Mod Toolkit

This crate is primarily used through the `league-mod` CLI tool:

```bash
# Pack to Fantome format
league-mod pack --format fantome

# Pack with custom filename
league-mod pack --format fantome --file-name "my-mod.fantome"
```

When packing to Fantome format, the CLI will warn users if their project contains additional layers that won't be included.

## Project Structure Requirements

For the library to work correctly, your mod project should follow this structure:

```
my-mod/
├── mod.config.json           # Project configuration
├── content/                  # Mod content
│   └── base/                 # Base layer (required)
│       ├── Aatrox.wad.client/
│       └── Map11.wad.client/
├── README.md                 # Optional project documentation
└── thumbnail.webp            # Optional thumbnail (any format)
```

## Contributing

This crate is part of the larger League Mod Toolkit project. See the main project README for contribution guidelines.

## License

Licensed under the same terms as the parent project.
