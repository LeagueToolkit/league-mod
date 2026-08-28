//! WAD overlay builder for League of Legends mods.
//!
//! This crate builds WAD overlay directories from a set of enabled mods. The overlay
//! contains patched copies of game WAD files with mod content applied on top, which a
//! patcher DLL can redirect the game to load instead of the originals.
//!
//! # How It Works
//!
//! A build runs two passes over the mods, with the routing decisions in between.
//! `docs/overlay-builder-design.md` covers the strategies and the patched-WAD
//! file layout in full; the shape is:
//!
//! 1. **Indexing** - Scan the game's `DATA/FINAL` directory and mount every
//!    `.wad.client` file. Build two indexes:
//!    - *Filename index*: WAD filename (case-insensitive) -> filesystem paths
//!    - *Hash index*: chunk path hash ([`WadHash`](ltk_wad::WadHash)) -> the WADs
//!      holding it
//!
//! 2. **Pass 1, metadata** - For each enabled mod (in order), read its layer
//!    structure and WAD override files through the [`ModContentProvider`] trait.
//!    Each override file is resolved to a [`WadHash`](ltk_wad::WadHash) path hash
//!    (either parsed from a hex filename or computed from the normalized path),
//!    then hashed and *dropped*: what survives is its content hash, its size and
//!    where to re-read it from, so a build's memory does not scale with the mods
//!    it holds. When multiple mods override the same hash, the first mod in the
//!    list (highest priority) wins.
//!
//! 3. **Distributing to WADs** - Using the hash index, each override is distributed
//!    to *every* game WAD that contains that path hash ("cross-WAD matching"). This
//!    means a single skin texture override will automatically be applied to both
//!    the champion WAD and any map WAD that shares the same asset.
//!
//! 4. **Pass 2, bytes** - Only the overrides the chosen WADs actually need are
//!    re-read and compressed, once per distinct content, and shared across every
//!    WAD they route to. For each affected game WAD a patched copy is then
//!    written into the overlay directory - or, when only override bytes changed,
//!    the existing copy keeps its data region and has only its tail rewritten.
//!
//! # Content Provider Abstraction
//!
//! Mod content is accessed through the [`ModContentProvider`] trait, which decouples
//! the builder from any particular storage format. Implementations can read from:
//!
//! - Filesystem directories ([`FsModContent`])
//! - `.modpkg` archives ([`ModpkgContent`])
//! - `.fantome` ZIP archives ([`FantomeContent`])
//!
//! # Incremental Rebuild
//!
//! After a successful build, an `overlay.json` state file is persisted (in the
//! *state directory*) containing the list of enabled mod IDs, per-mod content
//! fingerprints, a game directory fingerprint, and per-WAD override
//! fingerprints. On the next build:
//!
//! - **Exact match**: mod list, per-mod content fingerprints, and game
//!   fingerprint match, and every overlay WAD exists on disk - the build is
//!   skipped entirely. Content fingerprints participate so that mutable
//!   sources (e.g. a workshop project directory edited between test runs)
//!   invalidate the skip even though their mod ID is unchanged.
//! - **Incremental**: game fingerprint matches but the mod list or some mod's
//!   content changed. Per-WAD override fingerprints are compared and only WADs
//!   whose inputs changed are re-patched. Stale WADs (no longer needed) are
//!   removed. A WAD whose override *bytes* changed but whose chunk set did not
//!   keeps its file and has only its tail and TOC rewritten, so the cost is the
//!   mod's own bytes rather than the WAD's size.
//! - **Full rebuild**: game fingerprint or state version changed - all overlay WADs
//!   are wiped and rebuilt from scratch.
//!
//! Every shortcut is guarded: the recorded state is treated as a hint, verified
//! against the files themselves, and any doubt costs a full rebuild of that WAD
//! rather than a wrong one. See `docs/overlay-builder-design.md` for the layout
//! and the trust rules.
//!
//! The game index (`GameIndex`) is also cached to disk to avoid re-mounting every
//! WAD file on subsequent builds when the game hasn't been patched.
//!
//! # Example
//!
//! ```no_run
//! use ltk_overlay::{OverlayBuilder, EnabledMod, FsModContent};
//! use camino::Utf8PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let game_dir = Utf8PathBuf::from("C:/Riot Games/League of Legends/Game");
//! let profile_dir = Utf8PathBuf::from("C:/Users/.../profiles/default");
//! let overlay_root = profile_dir.join("overlay");
//!
//! let mut builder = OverlayBuilder::new(game_dir, overlay_root, profile_dir)
//!     .with_progress(|progress| {
//!         println!("Stage: {:?}, Progress: {}/{}",
//!             progress.stage, progress.current, progress.total);
//!     });
//!
//! builder.set_enabled_mods(vec![
//!     EnabledMod {
//!         id: "my-mod".to_string(),
//!         content: Box::new(FsModContent::new(Utf8PathBuf::from("/path/to/mod"))),
//!         enabled_layers: None,
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
pub mod fantome_content;
pub mod game_index;
pub mod linked_bins;
pub mod meta_cache;
pub mod modpkg_content;
pub mod state;
pub mod strings;
pub mod utils;
pub mod wad_builder;

#[cfg(test)]
mod test_support;

// Re-export main public API.
pub use builder::{
    AffectedWad, BASE_LAYER_NAME, EnabledMod, ModWadReport, OverlayBuildResult, OverlayBuilder,
    OverlayProgress, OverlayStage,
};
pub use content::{FsModContent, ModContentProvider};
pub use error::{
    CacheError, CorruptionError, Error, GameDirError, Invariant, ModContentError, Result,
    WadLimitError, WadRegion,
};
pub use fantome_content::FantomeContent;
pub use game_index::GameIndex;
pub use linked_bins::LinkedBinOffender;
pub use modpkg_content::ModpkgContent;
pub use state::OverlayState;
pub use strings::StringOverrideMode;
pub use utils::ContentHash;
