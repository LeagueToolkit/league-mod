//! Mod correctness verification for League of Legends overlays.
//!
//! The in-game verifier asserts a **closed world per WAD**: every asset a
//! champion's base skin (`skin0`) references must be present in the WAD that
//! references it, and a reference that does not resolve is a hard failure.
//! Real mods violate this in mundane, non-hostile ways — assets that are
//! simply missing, assets shipped into the wrong WAD (e.g. a localized WAD
//! instead of the champion WAD), or outdated mods whose skin bin references
//! vanilla assets that were removed from the game in a past patch.
//!
//! This crate is the shared implementation of that check, usable ahead of
//! time (mod managers, CLI tools, upload validation) and by the in-game
//! verifier itself, so the assertion cannot drift between implementations:
//!
//! - [`ChunkSource`](source::ChunkSource) abstracts where chunks come from —
//!   a mounted WAD ([`WadChunkSource`](source::WadChunkSource)), or a mod's
//!   archive entries virtually merged over the original game WAD
//!   ([`VirtualMerge`](source::VirtualMerge)) so archives never need to be
//!   extracted to disk to be checked.
//! - [`resolve_bin_entry_with`](resolve::resolve_bin_entry_with) resolves a
//!   bin entry the way the game does (root bin, then `linked` bins), and
//!   *records* corrupt bins it encounters instead of only logging them.
//! - [`check_base_skin`](check::check_base_skin) produces the verdict:
//!   either a per-mod [`SkinIntegrity`](check::SkinIntegrity) report, or a
//!   [`BaselineAnomaly`](check::BaselineAnomaly) when the **original** game
//!   WAD violates the assumptions — which is never the mod's fault and must
//!   be reported separately (corrupt install, or a game patch broke an
//!   assumption this crate bakes in).
//!
//! Report types expose hashes as plain integers — `u64` xxh64 chunk hashes,
//! `u32` fnv1a bin-entry hashes — never as [`ltk_hash`] wrapper types, so a
//! consumer pinned to a different `ltk_hash` version never hits a type
//! clash consuming them. The deliberate exception is the parsed skin
//! entries a report carries — [`SkinIntegrity::object`] (merged) and
//! [`SkinIntegrity::original_object`] (vanilla baseline) — which are
//! [`BinObject`]s, so a consumer that reads them *is* coupled to this
//! crate's `ltk_meta`. For that reason [`ltk_meta`] and [`BinObject`] are
//! re-exported here: go through this crate's re-export rather than
//! depending on `ltk_meta` separately. Consumers that only read the
//! summarized fields stay decoupled as before. The re-exported
//! [`BinHash`]/[`WadHash`] are for constructing *inputs* to the
//! resolve/skin helpers; treat this crate's re-export as the source of
//! truth there.

pub mod check;
pub mod resolve;
pub mod skin;
pub mod source;

pub use check::{
    BaselineAnomaly, ChunkChecksums, RefMissingKind, RefReport, RefStatus, SkinCheckOutcome,
    SkinIntegrity, SkinPolicy, check_base_skin,
};
pub use resolve::{
    CorruptBin, MAX_LINKED_BINS, ResolveError, ResolveOutcome, ResolvedBinObject,
    resolve_bin_entry_with,
};
pub use skin::{
    MeshSlot, SkinMeshRefs, champion_from_wad_path, skin_character_data_class, skin_mesh_refs,
    skin0_bin_name_hash, skin0_bin_path, skin0_entry_hash,
};
pub use source::{ChunkSource, VirtualMerge, WadChunkSource};

pub use ltk_hash::{BinHash, Hash, WadHash};
pub use ltk_meta::{self, BinObject};
