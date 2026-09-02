# CLAUDE.md

Rules for Rust code under `crates/`. The repo-wide rules are in the root `CLAUDE.md`.

## Path handling with camino
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
