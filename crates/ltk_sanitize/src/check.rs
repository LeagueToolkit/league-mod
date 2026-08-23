//! The composed base-skin correctness check.
//!
//! Judges the merged view of one champion WAD (mod content over the
//! original) against the closed-world assertion the in-game verifier
//! enforces: the base skin's mesh references must resolve inside this WAD.
//!
//! Every state the check can end in is a [`SkinCheckOutcome`] variant —
//! nothing is optional inside any of them:
//!
//! - [`SkinCheckOutcome::SkippedUnmodified`] — the merged root bin is
//!   byte-identical (by decompressed content) to the original; nothing to
//!   check.
//! - [`SkinCheckOutcome::BaselineAnomaly`] — never the mod's problem: the
//!   **original** game WAD violates an assumption (corrupt install, or a
//!   game patch broke an assumption this crate bakes in, e.g. the
//!   required-slot rule). These should be logged loudly and looked at by
//!   us, not shown as a mod diagnostic.
//! - [`SkinCheckOutcome::ModAnomaly`] — the mod's problem, the mirror of
//!   the baseline judgment applied to the merged side: unresolvable skin
//!   entry (reported as the corrupt bin in the skin graph when that is
//!   what hid it), missing required slot, or a mesh reference that is not
//!   usable from this WAD.
//! - [`SkinCheckOutcome::Modified`] — the mod modified the base skin and
//!   it satisfies the assertion; carries the parsed entries of both sides,
//!   the merged fingerprints, and any bins the resolve walk could not read
//!   but did not need.

use ltk_hash::{BinHash, Hash as _, WadHash};
use ltk_meta::BinObject;
use sha2::{Digest, Sha256};
use std::fmt;
use thiserror::Error;

use crate::resolve::{
    CorruptBin, ResolveError, ResolveOutcome, ResolvedBinObject, resolve_bin_entry_with,
};
use crate::skin::{
    MeshSlot, SkinMeshRefs, skin_character_data_class, skin_mesh_refs, skin0_bin_path,
    skin0_entry_hash,
};
use crate::source::ChunkSource;

/// Why a referenced asset is missing, as far as the caller's world
/// knowledge can tell.
///
/// Renders as the tail of a "{slot} '{path}' …" sentence (see
/// [`ModAnomaly::RefMissing`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefMissingKind {
    /// No world lookup was provided — all that is known is that the chunk
    /// is not in this WAD (the in-game verifier's view).
    Unknown,
    /// The chunk exists nowhere the world lookup knows of: the mod is
    /// broken or outdated (e.g. it references a vanilla asset that was
    /// removed from the game in a past patch).
    Everywhere,
    /// The chunk exists, but in other WADs — the mod shipped it to the
    /// wrong WAD (commonly a localized WAD instead of the champion WAD).
    Misplaced { found_in: Vec<String> },
    /// In the TOC, but its bytes could not be read (load/decompression
    /// failure) — present in name only, so the closed world is violated
    /// just the same.
    Unreadable { reason: String },
}

impl fmt::Display for RefMissingKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefMissingKind::Unknown => write!(f, "is missing from this WAD"),
            RefMissingKind::Everywhere => write!(
                f,
                "is missing from this WAD and everywhere else in the game and overlay — \
                 the mod is likely broken or outdated (the asset may have been removed \
                 from the game)"
            ),
            RefMissingKind::Misplaced { found_in } => write!(
                f,
                "is not in this WAD but exists in {} — shipped to the wrong WAD",
                found_in.join(", ")
            ),
            RefMissingKind::Unreadable { reason } => {
                write!(f, "is in the WAD but cannot be read: {reason}")
            }
        }
    }
}

/// How a present, readable reference relates to the original entry's same
/// slot. A reference that is *not* usable never gets a status — it fails
/// the whole check as [`ModAnomaly::RefMissing`].
///
/// The comparison is **slot-to-slot**: the merged reference is read at
/// *its* path in the merged view, the vanilla counterpart at the
/// **original entry's** path for the same slot, and the two contents are
/// compared. It is never path-in-original: looking the merged path up in
/// the original WAD would classify a repointed slot — `skin0` aimed at
/// another vanilla asset already in this WAD, the skin-unlock shape — as
/// Unmodified, because the bytes at that path *are* vanilla. They are
/// just not the bytes the original slot renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefStatus {
    /// Renders the same *content* the original entry's same slot renders,
    /// proven by comparing decompressed bytes. TOC checksums are never
    /// read at all — they are declared by the untrusted WAD, and zstd is
    /// not canonical — so neither a hostile declared checksum nor a
    /// repack of untouched bytes can affect classification. Content
    /// decides, not the path — a repoint that lands on a byte-identical
    /// chunk still renders exactly what vanilla renders.
    Unmodified,
    /// Present but rendering different content than the original entry's
    /// same slot: the bytes changed, or the reference was repointed at
    /// another asset (vanilla or not — the skin-unlock shape lands here).
    /// Not a correctness violation. Also lands here when the original
    /// side could not prove equality (its chunk unreadable).
    Modified {
        /// SHA-256 of the decompressed chunk bytes — the fingerprint
        /// consumers attest modified assets with (e.g. against a
        /// known-fingerprint set) without re-fetching the chunk. Computed
        /// from the actual bytes, never taken from anything the WAD
        /// declares, and collision-resistant so a crafted chunk cannot
        /// impersonate a known asset.
        sha256: [u8; 32],
    },
}

/// One checked mesh reference of a modified skin: present and readable in
/// the merged WAD (anything else is a [`ModAnomaly::RefMissing`], so it
/// never appears here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshRef {
    /// The path exactly as referenced by the bin.
    pub path: String,
    /// xxh64 chunk hash of the (lowercased) path.
    pub name_hash: u64,
    pub status: RefStatus,
}

/// The **original** game WAD violated an assumption of the check. This is
/// never the mod's fault: it points at a corrupt game install, or at a game
/// patch invalidating an assumption this crate bakes in. Report it where
/// developers will see it (logs), not as a mod diagnostic.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BaselineAnomaly {
    #[error("original WAD has no '{bin_path}' chunk")]
    OriginalRootMissing { bin_path: String },

    #[error("merged WAD is missing '{bin_path}', which the original WAD contains")]
    MergedRootMissing { bin_path: String },

    /// A bin in the original's skin graph could not be read or parsed, and
    /// the baseline entry was never found. Corruption the walk survived is
    /// only logged, never this anomaly.
    #[error("original WAD has a corrupt bin '{}': {}", .0.bin_path, .0.reason)]
    OriginalCorruptBin(CorruptBin),

    #[error("original WAD: {0}")]
    OriginalResolve(ResolveError),

    #[error("original skin0 entry has no {0} mesh property")]
    OriginalMissingRequiredSlot(MeshSlot),

    #[error("original skin0 {slot} '{path}' is missing from the original WAD")]
    OriginalRefUnresolved { slot: MeshSlot, path: String },
}

/// The mod broke the closed-world assertion — its base skin would fail
/// in-game verification. The merged-side mirror of [`BaselineAnomaly`]:
/// the first violation encountered, judged in the same fail-closed order
/// the baseline uses (unresolvable entry, missing required slot, unusable
/// reference).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ModAnomaly {
    /// A bin in the merged skin graph could not be read or parsed, and the
    /// `skin0` entry was never found — the unreadable bin is the likeliest
    /// place it went, so it is reported in place of the bare
    /// [`ResolveError::EntryNotFound`].
    ///
    /// Corruption on its own is never this anomaly: the walk does not stop
    /// for an unreadable bin, so an entry that resolved from a readable one
    /// is judged normally and the unreadable bins ride along on
    /// [`ModifiedSkin::corrupt_bins`].
    #[error("corrupt property bin '{}' in the skin graph: {}", .0.bin_path, .0.reason)]
    CorruptBin(CorruptBin),

    #[error("base-skin entry could not be resolved: {0}")]
    Resolve(ResolveError),

    #[error("skin0 entry has no {0} mesh property")]
    MissingRequiredSlot(MeshSlot),

    /// A set mesh reference that is not usable from the merged WAD —
    /// absent, or present but unreadable. `kind` refines why, as far as
    /// the caller's world knowledge can tell.
    #[error("{slot} '{path}' {kind}")]
    RefMissing {
        slot: MeshSlot,
        /// The path exactly as referenced by the bin.
        path: String,
        kind: RefMissingKind,
    },
}

/// A modified base skin that satisfies the closed-world assertion: the
/// entry resolved, every required mesh slot is set, and every reference is
/// present and readable in the merged WAD. Any failure on the way is a
/// [`ModAnomaly`] instead, so nothing here is optional — the one field
/// that can be empty is [`corrupt_bins`](Self::corrupt_bins), the
/// corruption the resolve walk survived.
#[derive(Debug, Clone, PartialEq)]
pub struct ModifiedSkin {
    /// The bin the entry was found in (`skin0.bin` itself or a linked bin).
    pub bin_path: String,
    /// xxh64 chunk hash of `bin_path`.
    pub bin_name_hash: u64,
    /// The parsed `skin0` entry as the merged WAD defines it. Carried so a
    /// consumer can read properties this check does not model
    /// (`SkinClassification`, material overrides, VFX) without
    /// re-resolving and re-parsing the skin graph.
    pub object: BinObject,
    /// The parsed `skin0` entry as the original game WAD defines it: the
    /// vanilla baseline each slot was classified against (see
    /// [`RefStatus`]).
    pub original_object: BinObject,
    /// The `Skeleton` reference and how it relates to vanilla.
    pub skeleton: MeshRef,
    /// The `SimpleSkin` reference and how it relates to vanilla.
    pub simple_skin: MeshRef,
    /// Bins in the merged skin graph that could not be read or parsed, in
    /// walk order — usually empty. The entry resolved without them, so they
    /// are not a correctness violation (see [`ModAnomaly::CorruptBin`]),
    /// but they are parts of the graph this check could not see into: a
    /// consumer that must vouch for the whole skin (the in-game verifier's
    /// fast track) should read a non-empty list as "cannot vouch" and fall
    /// through to its full scan, while a reporting consumer can surface
    /// them as warnings.
    pub corrupt_bins: Vec<CorruptBin>,
}

/// Outcome of checking one champion WAD.
#[derive(Debug, Clone, PartialEq)]
pub enum SkinCheckOutcome {
    /// The merged `skin0.bin` chunk decompresses to the same bytes as the
    /// original — a vanilla base skin, nothing to check. Decided by
    /// loading both roots and comparing content, so a merely
    /// re-compressed root bin skips too. (Assumption shared with the
    /// in-game verifier: an unmodified root bin means an unmodified base
    /// skin; a mod would have to modify only a *linked* bin to sidestep
    /// it, which in practice does not happen for skin swaps.)
    ///
    /// Note the referenced mesh chunks are not inspected either: a mod
    /// can replace the chunk *contents* at the stock mesh paths while
    /// keeping `skin0.bin` byte-identical. Harmless for the correctness
    /// lane (nothing can be missing), but a consumer that gates further
    /// scanning on this outcome inherits that blind spot.
    SkippedUnmodified,
    /// The original WAD violated an assumption — the check could not judge
    /// the mod at all. Logged via `tracing::error` with a stable
    /// `base-skin baseline anomaly` prefix.
    BaselineAnomaly(BaselineAnomaly),
    /// The mod broke the closed-world assertion and would fail in-game
    /// verification; its rendered message is the user-facing diagnostic.
    /// Logged via `tracing::warn`.
    ModAnomaly(ModAnomaly),
    /// The base skin is modified and satisfies the assertion. Boxed to
    /// keep this enum small: the payload carries two parsed bin entries,
    /// and without the indirection every outcome — including the common
    /// [`SkippedUnmodified`](Self::SkippedUnmodified) — would pay their
    /// size.
    Modified(Box<ModifiedSkin>),
}

/// Check one champion WAD's base skin in its merged view against the
/// original game WAD.
///
/// * `original` — the unpatched game WAD.
/// * `merged` — the merged view the game will load: a built overlay WAD, or
///   a [`VirtualMerge`](crate::source::VirtualMerge) of mod content over
///   `original`.
/// * `champion` — lowercase champion directory name (see
///   [`champion_from_wad_path`](crate::skin::champion_from_wad_path)).
/// * `world` — optional lookup answering "which other WADs (game or
///   overlay) contain this chunk hash?", used to refine missing references
///   into [`RefMissingKind::Everywhere`] vs [`RefMissingKind::Misplaced`].
pub fn check_base_skin(
    original: &mut dyn ChunkSource,
    merged: &mut dyn ChunkSource,
    champion: &str,
    world: Option<&dyn Fn(u64) -> Vec<String>>,
) -> SkinCheckOutcome {
    let root_bin_path = skin0_bin_path(champion);
    let root_hash = *WadHash::hash_str(&root_bin_path);
    let entry_hash = skin0_entry_hash(champion);
    let skin_class = skin_character_data_class();

    if !original.contains(root_hash) {
        return anomaly(BaselineAnomaly::OriginalRootMissing {
            bin_path: root_bin_path,
        });
    }
    if !merged.contains(root_hash) {
        return anomaly(BaselineAnomaly::MergedRootMissing {
            bin_path: root_bin_path,
        });
    }
    // The vanilla-skin skip compares decompressed content (TOC checksums
    // are never read — see [`RefStatus`]). A root that cannot be read
    // falls through to the full check, which maps the corruption to its
    // proper anomaly (baseline for the original side, mod for the merged
    // side).
    if let (Ok(original_data), Ok(merged_data)) = (original.load(root_hash), merged.load(root_hash))
        && original_data == merged_data
    {
        tracing::debug!("'{root_bin_path}' is unmodified; base-skin check skipped");
        return SkinCheckOutcome::SkippedUnmodified;
    }

    // The original WAD must satisfy every assumption this check enforces
    // before the mod can be judged against it. Its mesh refs are kept as
    // the comparison baseline: every merged slot is judged against the
    // chunk the ORIGINAL entry's same slot references.
    let (original_object, original_refs) =
        match validate_baseline(original, &root_bin_path, entry_hash, skin_class) {
            Ok(baseline) => baseline,
            Err(baseline) => return anomaly(baseline),
        };

    match judge_mod(
        original,
        merged,
        &root_bin_path,
        entry_hash,
        skin_class,
        world,
        original_object,
        &original_refs,
    ) {
        Ok(modified) => SkinCheckOutcome::Modified(Box::new(modified)),
        Err(mod_anomaly) => {
            tracing::warn!("Base-skin violation for {champion}: {mod_anomaly}");
            SkinCheckOutcome::ModAnomaly(mod_anomaly)
        }
    }
}

/// Resolve and judge the merged side, mirroring [`validate_baseline`]'s
/// fail-closed order: unresolvable entry, missing required slot, unusable
/// reference. Corruption the resolve walk survived is carried on the
/// result rather than reported (see [`resolve_or_explain`]).
#[expect(clippy::too_many_arguments)]
fn judge_mod(
    original: &mut dyn ChunkSource,
    merged: &mut dyn ChunkSource,
    root_bin_path: &str,
    entry_hash: BinHash,
    skin_class: BinHash,
    world: Option<&dyn Fn(u64) -> Vec<String>>,
    original_object: BinObject,
    original_refs: &SkinMeshRefs,
) -> Result<ModifiedSkin, ModAnomaly> {
    let outcome = resolve_bin_entry_with(merged, root_bin_path, entry_hash, Some(skin_class));
    let (resolved, corrupt_bins) = resolve_or_explain(outcome).map_err(|cause| match cause {
        Unresolved::Corrupt(corrupt) => ModAnomaly::CorruptBin(corrupt),
        Unresolved::Resolve(err) => ModAnomaly::Resolve(err),
    })?;

    let merged_refs = skin_mesh_refs(&resolved.object);
    if let Some(&slot) = merged_refs.missing_required_slots().first() {
        return Err(ModAnomaly::MissingRequiredSlot(slot));
    }

    let mut classify =
        |slot| classify_slot(original, merged, world, &merged_refs, original_refs, slot);
    let skeleton = classify(MeshSlot::Skeleton)?;
    let simple_skin = classify(MeshSlot::SimpleSkin)?;

    Ok(ModifiedSkin {
        bin_path: resolved.bin_path,
        bin_name_hash: resolved.bin_name_hash,
        object: resolved.object,
        original_object,
        skeleton,
        simple_skin,
        corrupt_bins,
    })
}

/// Why a resolve walk produced no entry.
enum Unresolved {
    Corrupt(CorruptBin),
    Resolve(ResolveError),
}

/// Split a resolve outcome into the entry plus the corruption it survived,
/// or the failure to report.
///
/// Corruption is never a verdict on its own. The walk does not stop for an
/// unreadable bin ([`resolve_bin_entry_with`]), so an entry defined by a
/// readable bin is judged normally and the unreadable ones ride along for
/// the caller to decide about. Corruption becomes the reported failure only
/// when the entry was never found — an unreadable bin may well have been
/// the one defining it, and "entry not found" would name the wrong cause.
/// Every other resolve error is a definitive verdict the walk reached on
/// its own (the entry was found with the wrong class, the root is absent,
/// the graph is absurd) and must not be masked by incidental corruption
/// elsewhere in the graph.
fn resolve_or_explain(
    outcome: ResolveOutcome,
) -> Result<(ResolvedBinObject, Vec<CorruptBin>), Unresolved> {
    match outcome.entry {
        Ok(resolved) => Ok((resolved, outcome.corrupt)),
        Err(err) => Err(match outcome.corrupt.into_iter().next() {
            Some(corrupt) if matches!(err, ResolveError::EntryNotFound { .. }) => {
                Unresolved::Corrupt(corrupt)
            }
            _ => Unresolved::Resolve(err),
        }),
    }
}

/// Classify one required slot of the merged entry against the original
/// entry's same slot, or fail with the [`ModAnomaly`] it evidences.
///
/// Two paths, read separately: the merged reference at ITS path in the
/// merged view, the vanilla counterpart at the ORIGINAL entry's path for
/// the same slot (see the [`RefStatus`] docs — comparing at the merged
/// path would bless repointed slots).
fn classify_slot(
    original: &mut dyn ChunkSource,
    merged: &mut dyn ChunkSource,
    world: Option<&dyn Fn(u64) -> Vec<String>>,
    merged_refs: &SkinMeshRefs,
    original_refs: &SkinMeshRefs,
    slot: MeshSlot,
) -> Result<MeshRef, ModAnomaly> {
    let Some(path) = merged_refs.slot_path(slot) else {
        return Err(ModAnomaly::MissingRequiredSlot(slot));
    };
    let name_hash = *WadHash::hash_str(path);

    if !merged.contains(name_hash) {
        return Err(ModAnomaly::RefMissing {
            slot,
            path: path.to_owned(),
            kind: match world {
                None => RefMissingKind::Unknown,
                Some(lookup) => {
                    let found_in = lookup(name_hash);
                    if found_in.is_empty() {
                        RefMissingKind::Everywhere
                    } else {
                        RefMissingKind::Misplaced { found_in }
                    }
                }
            },
        });
    }
    let data = match merged.load(name_hash) {
        Ok(data) => data,
        // Present in name only: fail closed — a stored chunk exists that
        // the check could not inspect.
        Err(reason) => {
            return Err(ModAnomaly::RefMissing {
                slot,
                path: path.to_owned(),
                kind: RefMissingKind::Unreadable { reason },
            });
        }
    };
    // One digest serves both purposes: equality against the original
    // slot's content, and the fingerprint carried on Modified. Baseline
    // validation guarantees the original entry sets every checked slot;
    // the None arm is defensive.
    let sha256: [u8; 32] = Sha256::digest(&data).into();
    let status = match original_refs.slot_path(slot) {
        Some(original_path)
            if original_sha256(original, *WadHash::hash_str(original_path)) == Some(sha256) =>
        {
            RefStatus::Unmodified
        }
        _ => RefStatus::Modified { sha256 },
    };
    Ok(MeshRef {
        path: path.to_owned(),
        name_hash,
        status,
    })
}

/// SHA-256 of the original WAD's chunk at `name_hash` — the **original
/// entry's** slot reference, not the merged path. Comparing these digests
/// of decompressed content is the sole equality test; TOC checksums are
/// never consulted (declared by an untrusted WAD, and not canonical under
/// re-compression). `None` when the original lacks the chunk or cannot
/// read it — never a match, so the merged side classifies as
/// [`RefStatus::Modified`] (an unreadable *original* is never the mod's
/// problem to report, so it maps to no error and no
/// [`RefMissingKind::Unreadable`], which describes the merged side).
fn original_sha256(original: &mut dyn ChunkSource, name_hash: u64) -> Option<[u8; 32]> {
    if !original.contains(name_hash) {
        return None;
    }
    match original.load(name_hash) {
        Ok(data) => Some(Sha256::digest(&data).into()),
        Err(reason) => {
            tracing::debug!(
                "original chunk {name_hash:016x} is unreadable ({reason}); \
                 classifying the merged chunk as modified"
            );
            None
        }
    }
}

/// Resolve and extract the original WAD's base skin, mapping every failure
/// to the [`BaselineAnomaly`] it evidences. On success, returns the parsed
/// baseline entry and its mesh refs — the slot-to-slot comparison baseline
/// for classifying the merged entry's references.
fn validate_baseline(
    original: &mut dyn ChunkSource,
    root_bin_path: &str,
    entry_hash: BinHash,
    skin_class: BinHash,
) -> Result<(BinObject, SkinMeshRefs), BaselineAnomaly> {
    let outcome = resolve_bin_entry_with(original, root_bin_path, entry_hash, Some(skin_class));
    let (resolved, corrupt_bins) = resolve_or_explain(outcome).map_err(|cause| match cause {
        Unresolved::Corrupt(corrupt) => BaselineAnomaly::OriginalCorruptBin(corrupt),
        Unresolved::Resolve(err) => BaselineAnomaly::OriginalResolve(err),
    })?;
    // Corruption the baseline walk survived is not carried anywhere: an
    // unreadable bin in the *original* is never the mod's problem to answer
    // for (the same call `original_sha256` makes for an unreadable original
    // chunk), and the entry resolved without it. It is still a corrupt
    // install, so say so where we will see it.
    for corrupt in corrupt_bins {
        tracing::warn!(
            "original WAD has a corrupt bin '{}' ({}); the base-skin entry resolved without it",
            corrupt.bin_path,
            corrupt.reason
        );
    }

    let refs = skin_mesh_refs(&resolved.object);
    if let Some(&slot) = refs.missing_required_slots().first() {
        return Err(BaselineAnomaly::OriginalMissingRequiredSlot(slot));
    }
    for (slot, path) in refs.slots() {
        if !original.contains(*WadHash::hash_str(path)) {
            return Err(BaselineAnomaly::OriginalRefUnresolved {
                slot,
                path: path.to_owned(),
            });
        }
    }
    Ok((resolved.object, refs))
}

fn anomaly(baseline: BaselineAnomaly) -> SkinCheckOutcome {
    tracing::error!("base-skin baseline anomaly: {baseline}");
    SkinCheckOutcome::BaselineAnomaly(baseline)
}
