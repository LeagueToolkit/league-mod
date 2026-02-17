# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

League Mod Toolkit - A Rust workspace containing CLI tools and libraries for creating, managing, and distributing League of Legends mods using the `.modpkg` format.

> **Note:** The LTK Manager desktop app has moved to [LeagueToolkit/ltk-manager](https://github.com/LeagueToolkit/ltk-manager).

## Quick Commands

### Rust (CLI and Libraries)

```bash
# Build all crates
cargo build --release

# Run CLI
cargo run --bin league-mod -- <command>

# Run tests
cargo test

# Run tests for specific crate
cargo test -p ltk_modpkg

# Lint
cargo clippy

# Format
cargo fmt
```

## Architecture Overview

### Workspace Structure

This is a Cargo workspace with the following crates:

- **`league-mod`** - CLI tool for mod developers (init, pack, extract, info)
- **`ltk_modpkg`** - Binary format library for `.modpkg` files (reading, writing, compression)
- **`ltk_mod_project`** - Configuration library (JSON/TOML config, metadata structures)
- **`ltk_mod_core`** - Shared utilities (League path detection, cross-platform utilities)
- **`ltk_fantome`** - Fantome archive format support (`.fantome` files)
- **`ltk_overlay`** - Overlay building engine (WAD patching, game file indexing)
- **`ltk_pki`** - Public Key Infrastructure (mod signing/verification)

## Critical Development Patterns

### Path Handling with Camino
**ALWAYS use `camino::Utf8Path` / `Utf8PathBuf` instead of `std::path::Path` / `PathBuf` for path handling in Rust code.** Camino provides UTF-8 guaranteed paths that are more robust, ergonomic, and consistent across platforms.

The workspace defines a shared version in the root `Cargo.toml` (currently `camino = "1.1"`). Prefer `camino = { workspace = true }` in crate `Cargo.toml` files.

**Key patterns:**
```rust
use camino::{Utf8Path, Utf8PathBuf};

// Function parameters: use &Utf8Path
fn process_file(path: &Utf8Path) -> Result<()> { ... }

// Owned paths: use Utf8PathBuf
struct Config {
    league_path: Option<Utf8PathBuf>,
}

// Construction
let path = Utf8PathBuf::from("some/path");
let joined = path.join("subdir");

// Converting FROM std::path (e.g., from OS APIs)
let std_path: PathBuf = std::env::current_exe()?;
let utf8_path = Utf8PathBuf::from_path_buf(std_path)
    .map_err(|p| format!("Non-UTF-8 path: {}", p.display()))?;

// Converting TO std::path (e.g., for std::fs APIs)
std::fs::File::open(utf8_path.as_std_path())?;
std::fs::read_dir(utf8_path.as_std_path())?;

// Direct string access (no lossy conversion needed)
println!("Path: {}", utf8_path.as_str());
```

**When to use `as_std_path()`:** At FFI boundaries where `std::fs` or external crates require `&Path` / `PathBuf`. Keep camino types throughout internal logic and convert only at the edges.

**Feature flags:** Add `serde1` feature when paths need serialization (e.g., in config structs):
```toml
camino = { workspace = true, features = ["serde1"] }
```

### Input Validation
**ALWAYS validate on backend, NEVER rely solely on frontend validation.**
- Trim and validate string inputs
- Check for empty/whitespace strings
- Return descriptive errors from backend

## Mod Format Reference

### Project Structure
```
my-mod/
├── mod.config.json           # Project configuration
├── content/                  # Mod content by layer
│   ├── base/                 # Base layer (priority 0)
│   │   ├── Aatrox.wad.client # Files for Aatrox WAD
│   │   └── Map11.wad.client  # Files for Summoner's Rift
│   └── high_res/             # Optional layer
└── build/                    # Output .modpkg files
```

### Layer System
- Layers have priorities (higher = loaded later)
- Higher priority layers override lower priority layers
- Base layer always present (priority 0)
- Additional layers are optional

## CI/CD

All contributions go through CI:
- Code compilation (Linux, Windows, macOS)
- Test suite execution
- Clippy linting
- Format verification
- Security audit
- License checks

**Commit Message Format:**
This project uses [Conventional Commits](https://www.conventionalcommits.org/):
```bash
feat: add support for custom transformers      # Minor version bump
fix: resolve file path handling on Windows     # Patch version bump
feat!: change configuration file format        # Major version bump (breaking)
docs: update installation instructions         # No version bump
```

