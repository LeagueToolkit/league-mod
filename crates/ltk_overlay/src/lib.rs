//! WAD overlay/profile builder for League of Legends mods.
//!
//! This crate provides functionality to build WAD overlays from enabled mods,
//! allowing the League patcher to load modded assets. It supports:
//!
//! - **Incremental rebuilds**: Only rebuild WADs that have changed
//! - **Cross-WAD matching**: Distribute mod files to all affected WADs
//! - **Layer system**: Respect mod layer priorities
//! - **String overrides**: Apply metadata-driven string table modifications
//! - **Conflict resolution**: Detect and resolve conflicts between mods
//!
//! # Example
//!
//! ```no_run
//! use ltk_overlay::{OverlayBuilder, EnabledMod};
//! use std::path::PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let game_dir = PathBuf::from("C:/Riot Games/League of Legends/Game");
//! let overlay_root = PathBuf::from("C:/Users/.../overlay");
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
//!         mod_dir: PathBuf::from("/path/to/mod"),
//!         priority: 0,
//!     },
//! ]);
//!
//! let result = builder.build()?;
//! println!("Built {} WADs, reused {}",
//!     result.wads_built.len(), result.wads_reused.len());
//! # Ok(())
//! # }
//! ```

pub mod builder;
pub mod error;
pub mod game_index;
pub mod utils;

// Re-export main types
pub use builder::{EnabledMod, OverlayBuildResult, OverlayBuilder, OverlayProgress, OverlayStage};
pub use error::{Error, Result};
pub use game_index::GameIndex;
