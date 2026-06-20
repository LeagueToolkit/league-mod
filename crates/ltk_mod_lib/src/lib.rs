//! Mod library management for LeagueToolkit.
//!
//! This crate provides the core business logic for managing League of Legends mods:
//! installing/uninstalling mods, managing profiles, building overlays, and
//! coordinating concurrent access via file locks.
//!
//! It has no Tauri dependency and is designed to be consumed by both the
//! `ltk-mod` CLI and the `ltk-manager` desktop app.
//!
//! # Usage
//!
//! ```no_run
//! use camino::Utf8Path;
//! use ltk_mod_lib::LibraryIndex;
//!
//! let storage = Utf8Path::new("/path/to/mods");
//! let mut index = LibraryIndex::load(storage).unwrap();
//!
//! index.install_mod(storage, Utf8Path::new("skin.modpkg")).unwrap();
//! index.create_profile(storage, "ranked".to_string()).unwrap();
//! index.toggle_mod("mod-id", true).unwrap();
//!
//! index.save(storage).unwrap();
//! ```

pub mod error;
pub mod index;
pub(crate) mod install;
pub mod lock;
pub mod overlay;
pub mod profile;
pub mod progress;
pub(crate) mod query;

pub use error::{LibraryError, LibraryResult};
pub use index::{LibraryIndex, LibraryModEntry, ModArchiveFormat};
pub use install::{BulkInstallError, BulkInstallResult};
pub use lock::StorageLock;
pub use overlay::OverlayConfig;
pub use profile::{Profile, ProfileSlug};
pub use progress::{
    InstallProgress, NoOpReporter, OverlayProgress, OverlayStage, ProgressReporter,
};
pub use query::{InstalledMod, ModLayer};
