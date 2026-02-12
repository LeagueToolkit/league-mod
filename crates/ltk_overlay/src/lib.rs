//! WAD overlay builder for League of Legends mods.
//!
//! This crate builds WAD overlay directories from a set of enabled mods. The overlay
//! contains patched copies of game WAD files with mod content applied on top, which a
//! patcher DLL can redirect the game to load instead of the originals.
//!
//! # How It Works
//!
//! The overlay build process has four stages:
//!
//! 1. **Indexing** — Scan the game's `DATA/FINAL` directory and mount every
//!    `.wad.client` file. Build two indexes:
//!    - *Filename index*: WAD filename (case-insensitive) -> filesystem paths
//!    - *Hash index*: chunk path hash (`u64`) -> list of WAD files containing it
//!
//! 2. **Collecting overrides** — For each enabled mod (in order), read its layer
//!    structure and WAD override files through the [`ModContentProvider`] trait.
//!    Each override file is resolved to a `u64` path hash (either parsed from a hex
//!    filename or computed from the normalized path). All overrides are collected
//!    into a single `HashMap<u64, Vec<u8>>`. When multiple mods override the same
//!    hash, the first mod in the list (highest priority) wins.
//!
//! 3. **Distributing to WADs** — Using the hash index, each override is distributed
//!    to *every* game WAD that contains that path hash ("cross-WAD matching"). This
//!    means a single skin texture override will automatically be applied to both
//!    the champion WAD and any map WAD that shares the same asset.
//!
//! 4. **Patching WADs** — For each affected game WAD, a patched copy is built in the
//!    overlay directory. The patched WAD contains all original chunks plus the
//!    overrides, with optimizations for audio files (kept uncompressed) and chunk
//!    deduplication.
//!
//! # Content Provider Abstraction
//!
//! Mod content is accessed through the [`ModContentProvider`] trait, which decouples
//! the builder from any particular storage format. Implementations can read from:
//!
//! - Filesystem directories ([`FsModContent`])
//! - `.modpkg` archives (implemented in `ltk-manager`)
//! - `.fantome` ZIP archives (implemented in `ltk-manager`)
//!
//! # Overlay State Caching
//!
//! After a successful build, an `overlay.json` state file is persisted containing the
//! list of enabled mod IDs and a game directory fingerprint. On the next build, if the
//! state matches and the overlay WAD files are still valid, the build is skipped entirely.
//!
//! # Example
//!
//! ```no_run
//! use ltk_overlay::{OverlayBuilder, EnabledMod, FsModContent};
//! use camino::Utf8PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let game_dir = Utf8PathBuf::from("C:/Riot Games/League of Legends/Game");
//! let overlay_root = Utf8PathBuf::from("C:/Users/.../overlay");
//!
//! let mut builder = OverlayBuilder::new(game_dir, overlay_root)
//!     .with_progress(|progress| {
//!         println!("Stage: {:?}, Progress: {}/{}",
//!             progress.stage, progress.current, progress.total);
//!     });
//!
//! builder.set_enabled_mods(vec![
//!     EnabledMod {
//!         id: "my-mod".to_string(),
//!         content: Box::new(FsModContent::new(Utf8PathBuf::from("/path/to/mod"))),
//!     },
//! ]);
//!
//! let result = builder.build()?;
//! println!("Built {} WADs in {:?}", result.wads_built.len(), result.build_time);
//! # Ok(())
//! # }

pub mod builder;
pub mod content;
pub mod error;
pub mod game_index;
pub mod state;
pub mod utils;
pub mod wad_builder;

// Re-export main public API.
pub use builder::{EnabledMod, OverlayBuildResult, OverlayBuilder, OverlayProgress, OverlayStage};
pub use content::{FsModContent, ModContentProvider};
pub use error::{Error, Result};
pub use game_index::GameIndex;
pub use state::OverlayState;
