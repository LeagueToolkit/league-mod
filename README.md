# League Mod Toolkit

A comprehensive Rust-based toolkit for creating, managing, and distributing League of Legends mods using the modpkg format.

## 🔧 Installation

### Windows (Recommended)

**Quick install (PowerShell, no admin):**
```powershell
irm https://raw.githubusercontent.com/LeagueToolkit/league-mod/main/scripts/install-league-mod.ps1 | iex
```
This downloads the latest release, installs it to `%LOCALAPPDATA%\LeagueToolkit\league-mod`, and adds it to your user `PATH`.

**Via GitHub Releases:**
1. Download the latest release from [GitHub Releases](https://github.com/LeagueToolkit/league-mod/releases)
2. Extract the ZIP file to your preferred location
3. Add the extracted directory to your [PATH environment variable](https://www.architectryan.com/2018/03/17/add-to-the-path-on-windows-10/)

## 🚀 Features

- **Project Management**: Initialize and manage mod projects with layered structure
- **Efficient Packaging**: Create compressed `.modpkg` files for distribution
- **Layer System**: Support for multiple mod layers with priority-based overrides
- **Metadata Management**: Rich metadata including authors, licenses, and descriptions
- **Cross-format Support**: Both JSON and TOML configuration formats
- **File Transformation**: Extensible transformer system for asset processing

## 📦 Packages

This workspace contains the following crates:

### `league-mod` - CLI Tool

The main command-line interface for mod developers and users.

**Features:**
- Initialize new mod projects with interactive prompts
- Pack mod projects into distributable `.modpkg` files
- Extract existing `.modpkg` files for inspection or modification
- Display detailed information about mod packages

**Usage:**
```bash
# Initialize a new mod project
league-mod init --name my-awesome-mod --display-name "My Awesome Mod"

# Pack a mod project
league-mod pack --config-path ./mod.config.json --output-dir ./build

# Extract a mod package
league-mod extract --file-path ./my-mod.modpkg --output-dir ./extracted

# Show mod package information
league-mod info --file-path ./my-mod.modpkg

# Configure League installation path
league-mod config auto-detect
league-mod config set-league-path "C:/Riot Games/League of Legends/Game/League of Legends.exe"
league-mod config show
```

### `ltk_modpkg` - Binary Format Library

Library for reading, writing, and manipulating the modpkg binary format.

**Features:**
- Reading and Writing
- Zstd compression
- Layer-based file organization
- Chunk-based data storage with metadata
- High-level project packing (with `project` feature)

### `ltk_mod_core` - Shared Core Library

Shared utilities used by both the CLI and GUI applications.

**Features:**
- League of Legends installation detection (registry, running processes, common paths)
- Cross-platform path utilities

### `ltk_mod_project` - Configuration Library

Handles mod project configuration files and metadata structures.

**Features:**
- JSON/TOML config support
- Layer system
- Author, license, readme and distribution metadata
- File transformer configuration

### `ltk-manager` - Desktop GUI Application

A graphical desktop application for managing League of Legends mods, built with [Tauri](https://tauri.app/).

**Features:**
- Visual mod library management
- Drag & drop mod installation
- Enable/disable mods with toggles
- Automatic League of Legends detection
- Creator Workshop for building mods (coming soon)

**Status:** In active development. See the [design document](crates/ltk-manager/DESIGN.md) for the full roadmap.

**Configuration Example:**
```json
{
  "name": "old-summoners-rift",
  "display_name": "Old Summoners Rift",
  "version": "1.0.0",
  "description": "Brings back the classic Summoners Rift map",
  "authors": [
    "TheKillerey",
    { "name": "Crauzer", "role": "Contributor" }
  ],
  "license": "MIT",
  "layers": [
    {
      "name": "base",
      "priority": 0,
      "description": "Base layer of the mod"
    },
    {
      "name": "high_res",
      "priority": 10,
      "description": "High resolution textures"
    }
  ],
  "transformers": [
    {
      "name": "tex-converter",
      "patterns": ["**/*.dds", "**/*.png"]
    }
  ]
}
```

## 🏗️ Project Structure

A typical mod project follows this structure:

```
my-mod/
├── mod.config.json           # Project configuration
├── content/                  # Mod content organized by layers
│   ├── base/                 # Base layer (priority 0)
|   |   ├── Aatrox.wad.client # Mods for the Aatrox wad file
│   │   |   ├── data/
│   │   │   └── assets/
|   |   ├── Map11.wad.client  # Mods for the Map11 (SR) wad file
│   │   |   ├── data/
│   │   │   └── assets/
│   ├── high_res/             # High resolution layer
│   └── gameplay/             # Gameplay modifications layer
├── build/                    # Output directory for .modpkg files
└── README.md                 # Project documentation/description
```

## 📖 Quick Start

### 1. Create a New Mod Project
```bash
league-mod init
# Follow the interactive prompts
```

### 2. Add Your Content
Place your mod files in the appropriate layer directories:
```bash
reworked-aatrox/content/base/data/characters/aatrox/skins/skin0.bin
reworked-aatrox/content/base/assets/characters/aatrox/skins/base/aatrox_base_tx_cm.tex
```

### 3. Configure Your Mod
Edit `mod.config.json` to add metadata, authors, and configure layers:

```json
{
  "name": "aatrox-rework",
  "display_name": "Aatrox Visual Rework",
  "version": "1.0.0",
  "description": "A complete visual overhaul for Aatrox",
  "authors": ["Your Name"],
  "layers": [
    {
      "name": "base",
      "priority": 0,
    }
  ]
}
```

### 4. Pack Your Mod
```bash
league-mod pack
# Creates aatrox-rework_1.0.0.modpkg in the build/ directory
```

## 🔄 Layer System

The layer system allows for modular and overrideable mod content:

- **Base Layer**: Always present, contains core mod files
- **Custom Layers**: Additional layers with configurable priorities
- **Override Behavior**: Higher priority layers override lower priority layers for the same files
- **Selective Installation**: Users can potentially choose which layers to install

Example layer configuration:
```json
{
  "layers": [
    {
      "name": "base",
      "priority": 0,
      "description": "Core modifications"
    },
    {
      "name": "optional_sounds",
      "priority": 10,
      "description": "Optional sound replacements"
    },
    {
      "name": "experimental",
      "priority": 20,
      "description": "Experimental features"
    }
  ]
}
```

## 🔗 File Transformers

Transformers allow preprocessing of files during the packing process:

```json
{
  "transformers": [
    {
      "name": "tex-converter",
      "patterns": ["**/*.png", "**/*.jpg"],
      "options": {
        "format": "dds",
        "compression": "bc7"
      }
    },
    ...
  ]
}
```
## Building from Source

**Prerequisites:**
- Rust 1.70+ (2021 edition)
- Git

**Build steps:**
```bash
git clone https://github.com/LeagueToolkit/league-mod.git
cd league-mod
cargo build --release
```

The compiled binary will be available at `target/release/league-mod.exe`

## 📜 License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE).

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

### Commit Message Format

This project uses [Conventional Commits](https://www.conventionalcommits.org/) for automated changelog generation and semantic versioning:

```bash
# Features (minor version bump)
git commit -m "feat: add support for custom transformers"

# Bug fixes (patch version bump)  
git commit -m "fix: resolve file path handling on Windows"

# Breaking changes (major version bump)
git commit -m "feat!: change configuration file format"

# Other types: docs, style, refactor, test, chore
git commit -m "docs: update installation instructions"
```

### Development Workflow

All contributions go through our CI pipeline:

1. **Create a PR** - All changes must be submitted via pull request
2. **CI Checks** - Automated checks run on every PR:
   - ✅ Code compilation on Linux, Windows, and macOS
   - ✅ Test suite execution
   - ✅ Clippy linting for code quality
   - ✅ Code formatting verification
   - ✅ Security audit for vulnerabilities
   - ✅ License and dependency checks
3. **Review & Merge** - Maintainer review and merge approved PRs

### Release Process

Releases are automated using [release-plz](https://release-plz.dev/docs):

1. Make commits using conventional commit format
2. Push to main branch
3. Release-plz creates a Release PR with version bump and changelog
4. Merge the PR to trigger automatic release with Windows binaries

## 📚 Documentation

For detailed documentation about the modpkg format and advanced usage, visit our [GitHub Wiki](https://github.com/LeagueToolkit/league-mod/wiki).

## 🙋‍♀️ Support

If you encounter any issues or have questions:
1. Check the [GitHub Issues](https://github.com/LeagueToolkit/league-mod/issues)
2. Consult the [Wiki documentation](https://github.com/LeagueToolkit/league-mod/wiki)
3. Join our community discussions

---

Made with ❤️ for the League of Legends modding community.
