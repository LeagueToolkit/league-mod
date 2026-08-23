//! The composed base-skin correctness check.
//!
//! Judges the merged view of one champion WAD (mod content over the
//! original) against the closed-world assertion the in-game verifier
//! enforces: the base skin's mesh references must resolve inside this WAD.
//!
//! Failures split into two classes with different owners:
//!
//! - [`SkinIntegrity`] — the mod's problem: unresolvable skin entry, a set
//!   mesh property whose asset is missing (straight-up broken, shipped to
//!   the wrong WAD, or referencing an asset the game removed in a past
//!   patch), corrupt bins in the skin graph.
//! - [`BaselineAnomaly`] — never the mod's problem: the **original** game
//!   WAD violates an assumption (corrupt install, or a game patch broke an
//!   assumption this crate bakes in, e.g. the required-slot rule). These
//!   should be logged loudly and looked at by us, not shown as a mod
//!   diagnostic.

use ltk_hash::{BinHash, Hash as _, WadHash};
use ltk_meta::BinObject;
use thiserror::Error;
use xxhash_rust::xxh3::xxh3_64;

use crate::resolve::{CorruptBin, ResolveError, resolve_bin_entry_with};
use crate::skin::{
    MeshSlot, SkinMeshRefs, skin_character_data_class, skin_mesh_refs, skin0_bin_path,
    skin0_entry_hash,
};
use crate::source::ChunkSource;

/// Why a referenced asset is missing, as far as the caller's world
/// knowledge can tell.
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
    /// just the same. Never tolerated (see
    /// [`SkinPolicy::allow_dangling_texture`]): unlike a dangling
    /// reference, a stored chunk exists and admits attacker-controlled
    /// bytes.
    Unreadable { reason: String },
}

/// Both fingerprints of one merged chunk, as attested against
/// known-fingerprint sets: the TOC checksum and the xxh3 of the
/// decompressed bytes (stable across re-compression).
///
/// They only travel as a pair: the TOC checksum is *declared* by the
/// (untrusted) merged WAD, not recomputed from the stored bytes, so a
/// chunk whose bytes cannot even be read must never become attestable
/// via its declared checksum alone — such a chunk classifies as
/// [`RefMissingKind::Unreadable`] instead of carrying half a
/// fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkChecksums {
    /// TOC checksum as declared by the merged WAD (xxh3 of the stored
    /// chunk bytes, see [`ChunkSource::checksum`]).
    pub checksum: u64,
    /// xxh3 of the decompressed chunk bytes.
    pub decompressed_checksum: u64,
}

/// How a referenced asset relates to the merged and original WADs.
///
/// The Unmodified/Modified comparison is **slot-to-slot**: the merged
/// reference is read at *its* path in the merged view, the vanilla
/// counterpart at the **original entry's** path for the same slot, and the
/// two contents are compared. It is never path-in-original: looking the
/// merged path up in the original WAD would classify a repointed slot —
/// `skin0` aimed at another vanilla asset already in this WAD, the
/// skin-unlock shape — as Unmodified, because the bytes at that path *are*
/// vanilla. They are just not the bytes the original slot renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefStatus {
    /// Renders the same *content* the original entry's same slot renders,
    /// proven by the decompressed bytes — never by TOC checksums, which
    /// prove nothing in either direction: the merged one is declared by an
    /// untrusted WAD (it could declare the vanilla value over different
    /// bytes), and zstd is not canonical (a repack re-encodes untouched
    /// bytes into a different TOC checksum). Content decides, not the
    /// path — a repoint that lands on a byte-identical chunk still renders
    /// exactly what vanilla renders.
    Unmodified,
    /// Present but rendering different content than the original entry's
    /// same slot: the bytes changed, the reference was repointed at
    /// another asset (vanilla or not — the skin-unlock shape lands here),
    /// or the original entry does not set this slot at all. Not a
    /// correctness violation. Carries the merged chunk's
    /// fingerprints so consumers that attest modified assets (e.g.
    /// against a known-fingerprint set) never re-fetch it. A mere
    /// re-compression of untouched bytes does not land here — content
    /// comparison classifies it [`RefStatus::Unmodified`] — so modified
    /// means the slot's rendered bytes really differ from vanilla's, or
    /// the original side could not prove them equal (slot unset there, or
    /// its chunk unreadable).
    Modified {
        /// The merged chunk's fingerprints. Always real: a chunk whose
        /// bytes cannot be read classifies as
        /// [`RefMissingKind::Unreadable`], never as modified.
        checksums: ChunkChecksums,
    },
    /// Not usable from the merged WAD — absent, or present but
    /// unreadable ([`RefMissingKind::Unreadable`]). A closed-world
    /// violation.
    Missing(RefMissingKind),
}

/// One referenced mesh asset and its classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefReport {
    pub slot: MeshSlot,
    /// The path exactly as referenced by the bin.
    pub path: String,
    /// xxh64 chunk hash of the (lowercased) path.
    pub name_hash: u64,
    pub status: RefStatus,
}

impl RefReport {
    /// Whether this reference counts as a violation under `policy`. A
    /// tolerated reference keeps its [`RefStatus::Missing`] status in the
    /// report — only the verdict changes.
    pub fn violates(&self, policy: SkinPolicy) -> bool {
        match &self.status {
            // Present in name only: a stored chunk exists, so the
            // dangling-texture tolerance's rationale (no bytes admitted)
            // never applies.
            RefStatus::Missing(RefMissingKind::Unreadable { .. }) => true,
            RefStatus::Missing(_) => {
                !(policy.allow_dangling_texture && self.slot == MeshSlot::Texture)
            }
            RefStatus::Unmodified | RefStatus::Modified { .. } => false,
        }
    }
}

/// Verdict policy for judging a [`SkinIntegrity`] report.
///
/// Classification is always the full truth; the policy only decides which
/// facts count as violations. **Every consumer of the check — mod manager,
/// CLI, and the in-game verifier once it bootstraps from this crate — must
/// use the same policy**, or ahead-of-time predictions diverge from in-game
/// enforcement.
///
/// The policy is deliberately defined on the per-WAD view (does the
/// reference resolve *in this WAD*), because that is all the in-game
/// verifier can ever see.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinPolicy {
    /// Tolerate a **set** texture slot whose reference does not resolve in
    /// this WAD. This is a known authoring idiom (a dangling path
    /// suppresses the vanilla base texture) and is safe to wave through: a
    /// dangling reference admits no attacker-controlled bytes — there is no
    /// chunk to load, scan, or swap — so the closed-world anti-evasion
    /// property is unaffected. For the same reason the tolerance never
    /// covers a reference that is present but unreadable
    /// ([`RefMissingKind::Unreadable`]) — there a stored chunk does exist.
    ///
    /// The sibling idiom — *removing* the texture property from the skin
    /// entry — needs no policy at all: texture is not a required slot
    /// (see [`MeshSlot::is_required`]), so an unset texture is clean even
    /// under [`SkinPolicy::strict`]. It is equally safe (no bytes
    /// admitted), and it never vouches as a stock slot either — an unset
    /// slot produces no [`RefReport`] to read [`RefStatus::Unmodified`]
    /// from.
    ///
    /// Never extended to skeleton/simple-skin (dangling ones are malformed
    /// skins), and never applied to the original-WAD baseline. A tolerated
    /// missing texture also never counts as a *stock* slot for any
    /// "unchanged slot vouches" rule — it is modified-but-tolerated.
    pub allow_dangling_texture: bool,
}

impl Default for SkinPolicy {
    /// The blessed shared default: dangling texture references tolerated.
    fn default() -> Self {
        Self {
            allow_dangling_texture: true,
        }
    }
}

impl SkinPolicy {
    /// Fail-closed judgment: every missing reference is a violation
    /// regardless of slot. For audits and for matching the legacy in-game
    /// behavior.
    pub fn strict() -> Self {
        Self {
            allow_dangling_texture: false,
        }
    }
}

/// Correctness report for one champion WAD's base skin. Empty
/// [`violations`](Self::violations) means the mod is fine.
#[derive(Debug, Clone, PartialEq)]
pub struct SkinIntegrity {
    /// Lowercase champion directory name.
    pub champion: String,
    /// The bin the skin entry was found in (`skin0.bin` itself or a linked
    /// bin), or the root bin path when resolution failed.
    pub bin_path: String,
    /// xxh64 chunk hash of `bin_path`.
    pub bin_name_hash: u64,
    /// Why the skin entry could not be resolved, when it could not.
    pub resolve_error: Option<ResolveError>,
    /// The parsed `skin0` entry as the **merged** WAD defines it — the
    /// object [`refs`](Self::refs) and
    /// [`missing_required`](Self::missing_required) were derived from.
    /// `None` exactly when [`resolve_error`](Self::resolve_error) is
    /// `Some`, since then there was no entry to parse.
    ///
    /// Carried so a consumer can read properties this check does not model
    /// (`SkinClassification`, material overrides, VFX, the rest of the
    /// `SkinMeshProperties` embed) without re-resolving and re-parsing the
    /// skin graph — the walk that produced it already followed the
    /// `linked` bins.
    pub object: Option<BinObject>,
    /// The parsed `skin0` entry as the **original** game WAD defines it:
    /// the vanilla baseline each merged slot was classified against (see
    /// [`RefStatus`]). Always present — a report only exists once the
    /// baseline validated, and a baseline that does not is a
    /// [`BaselineAnomaly`] instead of a report — so a consumer can diff
    /// merged against vanilla without mounting the original WAD again.
    pub original_object: BinObject,
    /// Required mesh slots the skin entry does not set at all.
    pub missing_required: Vec<MeshSlot>,
    /// Every set mesh reference and its status.
    pub refs: Vec<RefReport>,
    /// Corrupt bins encountered while walking the merged skin graph.
    pub corrupt: Vec<CorruptBin>,
}

impl SkinIntegrity {
    /// Whether anything about this base skin violates the closed-world
    /// assertion under `policy` (and would fail in-game verification).
    pub fn is_broken(&self, policy: SkinPolicy) -> bool {
        self.resolve_error.is_some()
            || !self.missing_required.is_empty()
            || !self.corrupt.is_empty()
            || self.refs.iter().any(|r| r.violates(policy))
    }

    /// Human-readable violation lines under `policy`, suitable for logs and
    /// user-facing diagnostics. Empty when the mod is fine.
    pub fn violations(&self, policy: SkinPolicy) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(err) = &self.resolve_error {
            out.push(format!("base-skin entry could not be resolved: {err}"));
        }
        for slot in &self.missing_required {
            out.push(format!("skin0 entry has no {slot} mesh property"));
        }
        for corrupt in &self.corrupt {
            out.push(format!(
                "corrupt property bin '{}' in the skin graph: {}",
                corrupt.bin_path, corrupt.reason
            ));
        }
        for r in &self.refs {
            if !r.violates(policy) {
                continue;
            }
            match &r.status {
                RefStatus::Unmodified | RefStatus::Modified { .. } => {}
                RefStatus::Missing(RefMissingKind::Everywhere) => out.push(format!(
                    "{} '{}' is missing from this WAD and everywhere else in the game \
                     and overlay — the mod is likely broken or outdated (the asset may \
                     have been removed from the game)",
                    r.slot, r.path
                )),
                RefStatus::Missing(RefMissingKind::Misplaced { found_in }) => out.push(format!(
                    "{} '{}' is not in this WAD but exists in {} — shipped to the wrong WAD",
                    r.slot,
                    r.path,
                    found_in.join(", ")
                )),
                RefStatus::Missing(RefMissingKind::Unknown) => {
                    out.push(format!("{} '{}' is missing from this WAD", r.slot, r.path))
                }
                RefStatus::Missing(RefMissingKind::Unreadable { reason }) => out.push(format!(
                    "{} '{}' is in the WAD but cannot be read: {reason}",
                    r.slot, r.path
                )),
            }
        }
        out
    }
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

    #[error("original WAD has a corrupt bin '{}': {}", .0.bin_path, .0.reason)]
    OriginalCorruptBin(CorruptBin),

    #[error("original WAD: {0}")]
    OriginalResolve(ResolveError),

    #[error("original skin0 entry has no {0} mesh property")]
    OriginalMissingRequiredSlot(MeshSlot),

    #[error("original skin0 {slot} '{path}' is missing from the original WAD")]
    OriginalRefUnresolved { slot: MeshSlot, path: String },
}

/// Outcome of checking one champion WAD.
#[derive(Debug, Clone, PartialEq)]
pub enum SkinCheckOutcome {
    /// The merged `skin0.bin` chunk decompresses to the same bytes as the
    /// original — a vanilla base skin, nothing to check. Decided by
    /// loading both roots and comparing content, never by the declared
    /// TOC checksum (an untrusted WAD could declare the vanilla value
    /// over a modified root bin and skip the whole check); a merely
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
    /// The base skin was checked; see [`SkinIntegrity::is_broken`].
    ///
    /// Boxed to keep this enum small: a report carries two parsed bin
    /// entries, and without the indirection every `SkinCheckOutcome` —
    /// including the common `SkippedUnmodified` — would pay their size.
    /// Deref makes reading the report through the box transparent.
    Report(Box<SkinIntegrity>),
    /// The original WAD violated an assumption — the check could not judge
    /// the mod at all. See [`BaselineAnomaly`].
    BaselineAnomaly(BaselineAnomaly),
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
///
/// Violations are additionally logged via `tracing::warn`; baseline
/// anomalies are logged via `tracing::error` with a stable
/// `base-skin baseline anomaly` prefix so they can be found in logs.
pub fn check_base_skin(
    original: &mut dyn ChunkSource,
    merged: &mut dyn ChunkSource,
    champion: &str,
    world: Option<&dyn Fn(u64) -> Vec<String>>,
    policy: SkinPolicy,
) -> SkinCheckOutcome {
    let root_bin_path = skin0_bin_path(champion);
    let root_hash = *WadHash::hash_str(&root_bin_path);
    let entry_hash = skin0_entry_hash(champion);
    let skin_class = skin_character_data_class();

    if original.checksum(root_hash).is_none() {
        return anomaly(BaselineAnomaly::OriginalRootMissing {
            bin_path: root_bin_path,
        });
    }
    if merged.checksum(root_hash).is_none() {
        return anomaly(BaselineAnomaly::MergedRootMissing {
            bin_path: root_bin_path,
        });
    }
    // The vanilla-skin skip is decided by decompressed content, never by
    // TOC equality: the merged checksum is declared by an untrusted WAD,
    // which could declare the vanilla value over a modified root bin and
    // skip the whole check. A root that cannot be read falls through to
    // the full check, which maps the corruption to its proper diagnostic
    // (baseline anomaly for the original side, corrupt-bin report for the
    // merged side).
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

    let outcome = resolve_bin_entry_with(merged, &root_bin_path, entry_hash, Some(skin_class));
    let resolved = match outcome.entry {
        Ok(resolved) => resolved,
        Err(err) => {
            return report(
                SkinIntegrity {
                    champion: champion.to_owned(),
                    bin_path: root_bin_path.clone(),
                    bin_name_hash: root_hash,
                    resolve_error: Some(err),
                    object: None,
                    original_object,
                    missing_required: Vec::new(),
                    refs: Vec::new(),
                    corrupt: outcome.corrupt,
                },
                policy,
            );
        }
    };

    let mesh_refs = skin_mesh_refs(&resolved.object);
    let mut refs = Vec::new();
    for (slot, path) in mesh_refs.slots() {
        let name_hash = *WadHash::hash_str(path);
        // Two paths, read separately: the merged reference at ITS path in
        // the merged view, the vanilla counterpart at the ORIGINAL entry's
        // path for the same slot (see the [`RefStatus`] docs — comparing
        // at the merged path would bless repointed slots).
        let original_slot = original_refs
            .slot_path(slot)
            .map(|path| *WadHash::hash_str(path));
        let status = match merged.checksum(name_hash) {
            Some(checksum) => match merged.load(name_hash) {
                Ok(data) => {
                    let checksums = ChunkChecksums {
                        checksum,
                        decompressed_checksum: xxh3_64(&data),
                    };
                    // Only the decompressed bytes can prove equality, in
                    // both directions: the declared TOC checksum is
                    // untrusted (a hostile WAD can declare the vanilla
                    // value over different bytes), and zstd is not
                    // canonical (a repack re-encodes untouched bytes into
                    // a different TOC checksum). A slot the original entry
                    // does not set has no counterpart to be equal to —
                    // always Modified.
                    match original_slot {
                        Some(hash) if original_content_matches(original, hash, &checksums) => {
                            RefStatus::Unmodified
                        }
                        _ => RefStatus::Modified { checksums },
                    }
                }
                Err(reason) => RefStatus::Missing(RefMissingKind::Unreadable { reason }),
            },
            None => RefStatus::Missing(match world {
                None => RefMissingKind::Unknown,
                Some(lookup) => {
                    let found_in = lookup(name_hash);
                    if found_in.is_empty() {
                        RefMissingKind::Everywhere
                    } else {
                        RefMissingKind::Misplaced { found_in }
                    }
                }
            }),
        };
        refs.push(RefReport {
            slot,
            path: path.to_owned(),
            name_hash,
            status,
        });
    }

    report(
        SkinIntegrity {
            champion: champion.to_owned(),
            bin_path: resolved.bin_path,
            bin_name_hash: resolved.bin_name_hash,
            resolve_error: None,
            object: Some(resolved.object),
            original_object,
            missing_required: mesh_refs.missing_required_slots(),
            refs,
            corrupt: outcome.corrupt,
        },
        policy,
    )
}

/// Whether the original WAD's chunk at `name_hash` — the **original
/// entry's** slot reference, not the merged path — decompresses to the same
/// content the merged chunk was fingerprinted with. This is the sole
/// equality test (TOC checksums prove nothing: declared by an untrusted
/// WAD, and not canonical under re-compression). An original that lacks
/// the chunk, or whose chunk cannot be read, does not match: the merged
/// side then classifies as
/// [`RefStatus::Modified`] (an unreadable *original* is never the mod's
/// problem to report, so it maps to no error and no
/// [`RefMissingKind::Unreadable`], which describes the merged side).
fn original_content_matches(
    original: &mut dyn ChunkSource,
    name_hash: u64,
    merged: &ChunkChecksums,
) -> bool {
    if !original.contains(name_hash) {
        return false;
    }
    match original.load(name_hash) {
        Ok(data) => xxh3_64(&data) == merged.decompressed_checksum,
        Err(reason) => {
            tracing::debug!(
                "original chunk {name_hash:016x} is unreadable ({reason}); \
                 classifying the merged chunk as modified"
            );
            false
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
    if let Some(corrupt) = outcome.corrupt.into_iter().next() {
        return Err(BaselineAnomaly::OriginalCorruptBin(corrupt));
    }
    let resolved = outcome.entry.map_err(BaselineAnomaly::OriginalResolve)?;

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

fn report(integrity: SkinIntegrity, policy: SkinPolicy) -> SkinCheckOutcome {
    for line in integrity.violations(policy) {
        tracing::warn!("Base-skin violation for {}: {line}", integrity.champion);
    }
    SkinCheckOutcome::Report(Box::new(integrity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::WadChunkSource;
    use indexmap::IndexMap;
    use ltk_meta::property::{NoMeta, values};
    use ltk_meta::{Bin, BinObject, PropertyValueEnum};
    use ltk_wad::Wad;
    use std::collections::BTreeMap;
    use std::io::{Cursor, Write};

    const CHAMP: &str = "testchamp";
    const ROOT: &str = "data/characters/testchamp/skins/skin0.bin";
    const CONCAT: &str = "data/testchamp_skin0_concat.bin";
    const SKL: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skl";
    const SKN: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skn";
    const TEX: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body_TX_CM.tex";

    fn h(name: &str) -> BinHash {
        BinHash::hash_str(name)
    }

    fn chunk_hash(path: &str) -> u64 {
        *WadHash::hash_str(path)
    }

    /// A skin entry whose mesh embed points at the given slot paths; `scale`
    /// varies the bytes so tests can produce a "modded" variant.
    fn skin_entry(
        skeleton: Option<&str>,
        simple_skin: Option<&str>,
        texture: Option<&str>,
        scale: f32,
    ) -> BinObject {
        let mut properties = IndexMap::new();
        if let Some(path) = skeleton {
            properties.insert(h("Skeleton"), values::String::from(path).into());
        }
        if let Some(path) = simple_skin {
            properties.insert(h("SimpleSkin"), values::String::from(path).into());
        }
        if let Some(path) = texture {
            properties.insert(h("Texture"), values::String::from(path).into());
        }
        properties.insert(h("SkinScale"), values::F32::new(scale).into());

        let mesh = values::Embedded(values::Struct {
            class_hash: h("SkinMeshDataProperties"),
            properties,
            meta: NoMeta,
        });

        BinObject::<NoMeta>::builder(
            h("Characters/Testchamp/Skins/Skin0"),
            h("SkinCharacterDataProperties"),
        )
        .property(h("SkinMeshProperties"), mesh)
        .build()
    }

    fn skin_bin(
        skeleton: Option<&str>,
        simple_skin: Option<&str>,
        texture: Option<&str>,
        scale: f32,
    ) -> Vec<u8> {
        bin_bytes(
            &Bin::builder()
                .object(skin_entry(skeleton, simple_skin, texture, scale))
                .build(),
        )
    }

    fn bin_bytes(bin: &Bin) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        bin.to_writer(&mut cursor).unwrap();
        cursor.into_inner()
    }

    /// Build an in-memory WAD from `path hash -> uncompressed contents`.
    fn build_wad(contents: &BTreeMap<u64, Vec<u8>>) -> Wad<Cursor<Vec<u8>>> {
        use ltk_wad::{WadBuilder, WadChunkBuilder};

        let mut builder = WadBuilder::default();
        for &hash in contents.keys() {
            builder = builder.with_chunk(WadChunkBuilder::default().with_hash(hash));
        }
        let mut out = Cursor::new(Vec::new());
        builder
            .build_to_writer(&mut out, |hash, cursor| {
                cursor.write_all(&contents[&hash]).unwrap();
                Ok(())
            })
            .unwrap();
        out.set_position(0);
        Wad::mount(out).unwrap()
    }

    /// A valid original: skin0 references all three slots and every asset
    /// is present.
    fn original_contents() -> BTreeMap<u64, Vec<u8>> {
        BTreeMap::from([
            (
                chunk_hash(ROOT),
                skin_bin(Some(SKL), Some(SKN), Some(TEX), 1.0),
            ),
            (chunk_hash(SKL), b"skeleton-data".to_vec()),
            (chunk_hash(SKN), b"mesh-data".to_vec()),
            (chunk_hash(TEX), b"texture-data".to_vec()),
        ])
    }

    fn check(
        original_contents: &BTreeMap<u64, Vec<u8>>,
        merged_contents: &BTreeMap<u64, Vec<u8>>,
        world: Option<&dyn Fn(u64) -> Vec<String>>,
        policy: SkinPolicy,
    ) -> SkinCheckOutcome {
        let mut original = build_wad(original_contents);
        let mut merged = build_wad(merged_contents);
        check_base_skin(
            &mut WadChunkSource(&mut original),
            &mut WadChunkSource(&mut merged),
            CHAMP,
            world,
            policy,
        )
    }

    const NO_WORLD: Option<&dyn Fn(u64) -> Vec<String>> = None;

    fn empty_world(_: u64) -> Vec<String> {
        Vec::new()
    }

    #[test]
    fn unmodified_skin0_is_skipped() {
        let original = original_contents();
        let mut merged = original.clone();
        // Even a modified texture does not trigger the check when the skin
        // bin itself is vanilla.
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        assert_eq!(
            check(&original, &merged, NO_WORLD, SkinPolicy::default()),
            SkinCheckOutcome::SkippedUnmodified
        );
    }

    #[test]
    fn modified_texture_with_all_refs_present_is_clean() {
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        // The Modified status must carry both fingerprints of the merged
        // chunk: the TOC checksum exactly as a ChunkSource over the merged
        // WAD reports it, and the xxh3 of the decompressed bytes.
        let mut merged_wad = build_wad(&merged);
        let tex_checksum = WadChunkSource(&mut merged_wad)
            .checksum(chunk_hash(TEX))
            .expect("merged texture chunk");

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::default()));
        assert!(report.violations(SkinPolicy::default()).is_empty());
        assert_eq!(report.bin_path, ROOT);
        let statuses: Vec<_> = report
            .refs
            .iter()
            .map(|r| (r.slot, r.status.clone()))
            .collect();
        assert_eq!(
            statuses,
            vec![
                (MeshSlot::Skeleton, RefStatus::Unmodified),
                (MeshSlot::SimpleSkin, RefStatus::Unmodified),
                (
                    MeshSlot::Texture,
                    RefStatus::Modified {
                        checksums: ChunkChecksums {
                            checksum: tex_checksum,
                            decompressed_checksum: xxh3_64(b"MODDED-texture"),
                        }
                    }
                ),
            ]
        );
    }

    /// Delegates to a WAD but refuses to load one chunk, simulating a
    /// chunk that is present in the TOC but unreadable.
    struct BrokenChunk<S> {
        inner: S,
        broken: u64,
    }

    impl<S: ChunkSource> ChunkSource for BrokenChunk<S> {
        fn checksum(&mut self, name_hash: u64) -> Option<u64> {
            self.inner.checksum(name_hash)
        }
        fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
            if name_hash == self.broken {
                return Err("simulated unreadable chunk".to_string());
            }
            self.inner.load(name_hash)
        }
    }

    #[test]
    fn unreadable_chunk_is_missing_and_never_tolerated() {
        // The TOC checksum is declared by the untrusted WAD; a chunk whose
        // bytes cannot be read is present in name only, so it classifies
        // as Missing(Unreadable) — a violation even for a texture slot
        // under the default policy, because unlike a dangling reference a
        // stored chunk exists.
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        let mut original_wad = build_wad(&original);
        let mut merged_wad = build_wad(&merged);
        let mut merged_source = BrokenChunk {
            inner: WadChunkSource(&mut merged_wad),
            broken: chunk_hash(TEX),
        };

        let SkinCheckOutcome::Report(report) = check_base_skin(
            &mut WadChunkSource(&mut original_wad),
            &mut merged_source,
            CHAMP,
            NO_WORLD,
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(report.is_broken(SkinPolicy::default()));
        assert!(matches!(
            &report.refs[2].status,
            RefStatus::Missing(RefMissingKind::Unreadable { .. })
        ));
        let violations = report.violations(SkinPolicy::default());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("cannot be read"));
    }

    /// Delegates to a WAD but reports a perturbed TOC checksum for one
    /// chunk while `load` still returns the same bytes — the shape a
    /// repack produces for an untouched asset (zstd is not canonical).
    struct RecompressedChunk<S> {
        inner: S,
        repacked: u64,
    }

    impl<S: ChunkSource> ChunkSource for RecompressedChunk<S> {
        fn checksum(&mut self, name_hash: u64) -> Option<u64> {
            let checksum = self.inner.checksum(name_hash)?;
            Some(checksum ^ u64::from(name_hash == self.repacked))
        }
        fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
            self.inner.load(name_hash)
        }
    }

    #[test]
    fn recompressed_untouched_asset_reads_unmodified() {
        // The mod repacked the WAD: skin0.bin is genuinely modified, but
        // the texture chunk carries the original bytes under a different
        // TOC checksum. Content comparison must classify it Unmodified.
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );

        let mut original_wad = build_wad(&original);
        let mut merged_wad = build_wad(&merged);
        let mut merged_source = RecompressedChunk {
            inner: WadChunkSource(&mut merged_wad),
            repacked: chunk_hash(TEX),
        };

        let SkinCheckOutcome::Report(report) = check_base_skin(
            &mut WadChunkSource(&mut original_wad),
            &mut merged_source,
            CHAMP,
            NO_WORLD,
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::default()));
        let statuses: Vec<_> = report
            .refs
            .iter()
            .map(|r| (r.slot, r.status.clone()))
            .collect();
        assert_eq!(
            statuses,
            vec![
                (MeshSlot::Skeleton, RefStatus::Unmodified),
                (MeshSlot::SimpleSkin, RefStatus::Unmodified),
                (MeshSlot::Texture, RefStatus::Unmodified),
            ]
        );
    }

    /// Delegates to a WAD but declares a chosen TOC checksum for one
    /// chunk — the shape of a hostile WAD lying about its stored bytes.
    struct SpoofedChecksum<S> {
        inner: S,
        spoofed: u64,
        declared: u64,
    }

    impl<S: ChunkSource> ChunkSource for SpoofedChecksum<S> {
        fn checksum(&mut self, name_hash: u64) -> Option<u64> {
            if name_hash == self.spoofed {
                return Some(self.declared);
            }
            self.inner.checksum(name_hash)
        }
        fn load(&mut self, name_hash: u64) -> Result<Vec<u8>, String> {
            self.inner.load(name_hash)
        }
    }

    #[test]
    fn spoofed_toc_checksum_on_asset_reads_modified() {
        // The merged texture ships different bytes but DECLARES the
        // original slot's TOC checksum. Declared checksums are untrusted;
        // only the decompressed comparison may conclude Unmodified, so
        // the spoof must classify Modified — with the real decompressed
        // fingerprint on record, not the lie.
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        let mut original_wad = build_wad(&original);
        let vanilla_tex_checksum = WadChunkSource(&mut original_wad)
            .checksum(chunk_hash(TEX))
            .expect("original texture chunk");

        let mut merged_wad = build_wad(&merged);
        let mut merged_source = SpoofedChecksum {
            inner: WadChunkSource(&mut merged_wad),
            spoofed: chunk_hash(TEX),
            declared: vanilla_tex_checksum,
        };

        let SkinCheckOutcome::Report(report) = check_base_skin(
            &mut WadChunkSource(&mut original_wad),
            &mut merged_source,
            CHAMP,
            NO_WORLD,
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(matches!(
            &report.refs[2].status,
            RefStatus::Modified { checksums }
                if checksums.decompressed_checksum == xxh3_64(b"MODDED-texture")
        ));
    }

    #[test]
    fn spoofed_toc_checksum_on_root_bin_does_not_skip() {
        // A repointed skin0.bin that DECLARES the vanilla root checksum
        // must not skip the check as SkippedUnmodified — the skip is
        // decided by decompressed content, or the whole assertion could
        // be evaded by one lied-about TOC field.
        let alt_skl = "ASSETS/Characters/Testchamp/Skins/Skin02/alt.skl";
        let mut original = original_contents();
        original.insert(chunk_hash(alt_skl), b"alt-skeleton-data".to_vec());

        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(alt_skl), Some(SKN), Some(TEX), 2.0),
        );

        let mut original_wad = build_wad(&original);
        let vanilla_root_checksum = WadChunkSource(&mut original_wad)
            .checksum(chunk_hash(ROOT))
            .expect("original root bin");

        let mut merged_wad = build_wad(&merged);
        let mut merged_source = SpoofedChecksum {
            inner: WadChunkSource(&mut merged_wad),
            spoofed: chunk_hash(ROOT),
            declared: vanilla_root_checksum,
        };

        let SkinCheckOutcome::Report(report) = check_base_skin(
            &mut WadChunkSource(&mut original_wad),
            &mut merged_source,
            CHAMP,
            NO_WORLD,
            SkinPolicy::default(),
        ) else {
            panic!("expected a report, not SkippedUnmodified");
        };
        assert!(matches!(
            &report.refs[0].status,
            RefStatus::Modified { checksums }
                if checksums.decompressed_checksum == xxh3_64(b"alt-skeleton-data")
        ));
    }

    #[test]
    fn recompressed_root_bin_is_skipped() {
        // The counterpart direction: an untouched skin0.bin whose TOC
        // checksum changed under a repack still decompresses to vanilla
        // bytes — content comparison skips it as unmodified.
        let original = original_contents();
        let merged = original.clone();

        let mut original_wad = build_wad(&original);
        let mut merged_wad = build_wad(&merged);
        let mut merged_source = RecompressedChunk {
            inner: WadChunkSource(&mut merged_wad),
            repacked: chunk_hash(ROOT),
        };

        assert_eq!(
            check_base_skin(
                &mut WadChunkSource(&mut original_wad),
                &mut merged_source,
                CHAMP,
                NO_WORLD,
                SkinPolicy::default(),
            ),
            SkinCheckOutcome::SkippedUnmodified
        );
    }

    #[test]
    fn repointed_slots_to_other_vanilla_assets_read_modified() {
        // The skin-unlock shape (e.g. "Goth Annie"): the mod ships ONLY a
        // rewritten skin0.bin whose mesh slots point at another skin's
        // assets, which already sit in the vanilla WAD. Every reference
        // resolves (the correctness verdict stays clean) and every
        // referenced chunk holds vanilla bytes — but not the bytes the
        // original SLOT renders. Slot-to-slot comparison must read
        // Modified; comparing the merged path against the original WAD's
        // same path would read Unmodified and let every repointed slot
        // vouch as stock.
        let alt_skl = "ASSETS/Characters/Testchamp/Skins/Skin02/alt.skl";
        let alt_skn = "ASSETS/Characters/Testchamp/Skins/Skin02/alt.skn";
        let alt_tex = "ASSETS/Characters/Testchamp/Skins/Skin02/alt_TX_CM.tex";

        let mut original = original_contents();
        original.insert(chunk_hash(alt_skl), b"alt-skeleton-data".to_vec());
        original.insert(chunk_hash(alt_skn), b"alt-mesh-data".to_vec());
        original.insert(chunk_hash(alt_tex), b"alt-texture-data".to_vec());

        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(alt_skl), Some(alt_skn), Some(alt_tex), 2.0),
        );

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::strict()));
        for r in &report.refs {
            assert!(
                matches!(r.status, RefStatus::Modified { .. }),
                "{} must read Modified, got {:?}",
                r.slot,
                r.status
            );
        }
        assert!(matches!(
            &report.refs[0].status,
            RefStatus::Modified { checksums }
                if checksums.decompressed_checksum == xxh3_64(b"alt-skeleton-data")
        ));
    }

    #[test]
    fn repointed_slot_with_identical_content_reads_unmodified() {
        // A repoint that lands on a chunk holding byte-identical content
        // renders exactly what vanilla renders — content decides, not the
        // path.
        let alias = "ASSETS/Characters/Testchamp/Skins/Skin02/copy.skl";
        let mut original = original_contents();
        original.insert(chunk_hash(alias), b"skeleton-data".to_vec());

        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(alias), Some(SKN), Some(TEX), 2.0),
        );

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::strict()));
        assert_eq!(report.refs[0].status, RefStatus::Unmodified);
    }

    #[test]
    fn texture_set_where_original_unsets_it_reads_modified() {
        // Material-override champions (Evelynn, Mel, Yuumi) set no
        // texture. A mod that sets one — even pointing at a vanilla chunk
        // already in the WAD — has no vanilla counterpart to be equal to:
        // Modified, never Unmodified.
        let mut original = original_contents();
        original.insert(chunk_hash(ROOT), skin_bin(Some(SKL), Some(SKN), None, 1.0));

        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::strict()));
        assert!(matches!(
            &report.refs[2].status,
            RefStatus::Modified { checksums }
                if checksums.decompressed_checksum == xxh3_64(b"texture-data")
        ));
    }

    #[test]
    fn unreadable_original_side_still_classifies_modified() {
        // When the ORIGINAL chunk cannot be read, content equality cannot
        // be proven — the merged chunk stays Modified with its
        // fingerprints intact. Never an error, never Missing(Unreadable):
        // that kind describes the merged side.
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        let mut original_wad = build_wad(&original);
        let mut merged_wad = build_wad(&merged);
        let mut original_source = BrokenChunk {
            inner: WadChunkSource(&mut original_wad),
            broken: chunk_hash(TEX),
        };

        let SkinCheckOutcome::Report(report) = check_base_skin(
            &mut original_source,
            &mut WadChunkSource(&mut merged_wad),
            CHAMP,
            NO_WORLD,
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::default()));
        assert!(matches!(
            &report.refs[2].status,
            RefStatus::Modified { checksums } if checksums.decompressed_checksum == xxh3_64(b"MODDED-texture")
        ));
    }

    #[test]
    fn removed_texture_property_is_clean_even_under_strict() {
        // The sibling idiom to a dangling texture reference: the mod
        // *unsets* the texture property entirely. Texture is not a
        // required slot, so this is clean under both policies — and it
        // leaves no texture ref on record at all, so it can never vouch
        // as a stock slot either.
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(chunk_hash(ROOT), skin_bin(Some(SKL), Some(SKN), None, 2.0));

        let SkinCheckOutcome::Report(report) =
            check(&original, &merged, Some(&empty_world), SkinPolicy::strict())
        else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::strict()));
        assert!(!report.is_broken(SkinPolicy::default()));
        assert!(report.missing_required.is_empty());
        let slots: Vec<_> = report.refs.iter().map(|r| r.slot).collect();
        assert_eq!(slots, vec![MeshSlot::Skeleton, MeshSlot::SimpleSkin]);
    }

    #[test]
    fn dangling_texture_is_a_violation_under_strict_policy() {
        // The mod's skin bin references a texture path that exists nowhere —
        // a broken/outdated mod (e.g. the asset was removed from the game).
        let stale = "ASSETS/Characters/Testchamp/Skins/Base/gone.tex";
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(stale), 2.0),
        );

        let SkinCheckOutcome::Report(report) =
            check(&original, &merged, Some(&empty_world), SkinPolicy::strict())
        else {
            panic!("expected a report");
        };
        assert!(report.is_broken(SkinPolicy::strict()));
        assert_eq!(
            report.refs[2].status,
            RefStatus::Missing(RefMissingKind::Everywhere)
        );
        assert_eq!(report.violations(SkinPolicy::strict()).len(), 1);
    }

    #[test]
    fn dangling_texture_is_tolerated_by_default_policy() {
        // Same setup under the blessed default: a dangling texture is a
        // known authoring idiom — still reported as Missing, but not a
        // violation.
        let stale = "ASSETS/Characters/Testchamp/Skins/Base/gone.tex";
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(stale), 2.0),
        );

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::default()));
        assert!(report.violations(SkinPolicy::default()).is_empty());
        // The fact is still on record — only the verdict changed.
        assert_eq!(
            report.refs[2].status,
            RefStatus::Missing(RefMissingKind::Everywhere)
        );
    }

    #[test]
    fn dangling_skeleton_is_never_tolerated() {
        let stale = "ASSETS/Characters/Testchamp/Skins/Base/gone.skl";
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(stale), Some(SKN), Some(TEX), 2.0),
        );

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(report.is_broken(SkinPolicy::default()));
        assert_eq!(report.violations(SkinPolicy::default()).len(), 1);
    }

    #[test]
    fn misplaced_reference_names_the_wads_that_have_it() {
        let custom = "ASSETS/Characters/Testchamp/Skins/Base/custom.tex";
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(custom), 2.0),
        );

        let world = |hash: u64| {
            if hash == chunk_hash(custom) {
                vec!["Testchamp.en_US.wad.client".to_string()]
            } else {
                Vec::new()
            }
        };
        let SkinCheckOutcome::Report(report) =
            check(&original, &merged, Some(&world), SkinPolicy::strict())
        else {
            panic!("expected a report");
        };
        assert!(report.is_broken(SkinPolicy::strict()));
        assert_eq!(
            report.refs[2].status,
            RefStatus::Missing(RefMissingKind::Misplaced {
                found_in: vec!["Testchamp.en_US.wad.client".to_string()]
            })
        );
        assert!(report.violations(SkinPolicy::strict())[0].contains("wrong WAD"));
        // The policy is per-WAD (the in-game view cannot tell Misplaced from
        // Everywhere), so the default tolerates a misplaced texture too.
        assert!(!report.is_broken(SkinPolicy::default()));
    }

    #[test]
    fn missing_reference_without_world_is_unknown() {
        let stale = "ASSETS/Characters/Testchamp/Skins/Base/gone.tex";
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(stale), 2.0),
        );

        let SkinCheckOutcome::Report(report) =
            check(&original, &merged, NO_WORLD, SkinPolicy::default())
        else {
            panic!("expected a report");
        };
        assert_eq!(
            report.refs[2].status,
            RefStatus::Missing(RefMissingKind::Unknown)
        );
    }

    #[test]
    fn unset_required_slot_is_a_violation() {
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(chunk_hash(ROOT), skin_bin(None, Some(SKN), Some(TEX), 2.0));

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(report.is_broken(SkinPolicy::default()));
        assert_eq!(report.missing_required, vec![MeshSlot::Skeleton]);
    }

    #[test]
    fn garbage_merged_skin_bin_is_reported_with_corruption() {
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(chunk_hash(ROOT), b"not a property bin".to_vec());

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(report.is_broken(SkinPolicy::default()));
        assert!(matches!(
            report.resolve_error,
            Some(ResolveError::EntryNotFound { .. })
        ));
        assert_eq!(report.corrupt.len(), 1);
    }

    /// `SkinMeshProperties.SkinScale`, which the fixtures vary per side —
    /// the cheapest proof that a parsed entry came from the side it claims.
    fn skin_scale(object: &BinObject) -> f32 {
        let Some(PropertyValueEnum::Embedded(mesh)) =
            object.properties.get(&h("SkinMeshProperties"))
        else {
            panic!("skin entry has no SkinMeshProperties embed");
        };
        match mesh.0.properties.get(&h("SkinScale")) {
            Some(PropertyValueEnum::F32(scale)) => scale.value,
            other => panic!("unexpected SkinScale property: {other:?}"),
        }
    }

    #[test]
    fn report_carries_the_parsed_merged_and_original_skin_entries() {
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };

        let object = report.object.as_ref().expect("merged skin entry");
        assert_eq!(object.path_hash, h("Characters/Testchamp/Skins/Skin0"));
        assert_eq!(object.class_hash, h("SkinCharacterDataProperties"));
        // Both sides are handed out, and they are genuinely the two
        // parses — not the same object twice.
        assert_eq!(skin_scale(object), 2.0);
        assert_eq!(skin_scale(&report.original_object), 1.0);
        // The summarized fields stay derivable from the object handed out,
        // so a consumer never has to re-resolve to go deeper.
        assert_eq!(skin_mesh_refs(object).texture.as_deref(), Some(TEX));
    }

    #[test]
    fn unresolvable_entry_carries_no_object_but_keeps_the_baseline() {
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(chunk_hash(ROOT), b"not a property bin".to_vec());

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        // `object` is None exactly when the entry could not be resolved,
        // but the validated baseline is always there.
        assert!(report.resolve_error.is_some());
        assert!(report.object.is_none());
        assert_eq!(skin_scale(&report.original_object), 1.0);
    }

    #[test]
    fn entry_found_via_linked_bin() {
        // The mod replaces skin0.bin with one that only links a concat bin;
        // the skin entry (and a modded texture) live there.
        let original = original_contents();
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            bin_bytes(&Bin::builder().dependency(CONCAT).build()),
        );
        merged.insert(
            chunk_hash(CONCAT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(TEX), b"MODDED-texture".to_vec());

        let SkinCheckOutcome::Report(report) = check(
            &original,
            &merged,
            Some(&empty_world),
            SkinPolicy::default(),
        ) else {
            panic!("expected a report");
        };
        assert!(!report.is_broken(SkinPolicy::default()));
        assert_eq!(report.bin_path, CONCAT);
    }

    // ------------------------------------------------------------ baseline

    #[test]
    fn original_without_root_bin_is_an_anomaly() {
        let mut original = original_contents();
        original.remove(&chunk_hash(ROOT));
        let merged = original_contents();

        assert!(matches!(
            check(&original, &merged, NO_WORLD, SkinPolicy::default()),
            SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::OriginalRootMissing { .. })
        ));
    }

    #[test]
    fn merged_that_lost_the_root_bin_is_an_anomaly() {
        let original = original_contents();
        let mut merged = original_contents();
        merged.remove(&chunk_hash(ROOT));

        assert!(matches!(
            check(&original, &merged, NO_WORLD, SkinPolicy::default()),
            SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::MergedRootMissing { .. })
        ));
    }

    #[test]
    fn corrupt_original_bin_is_an_anomaly() {
        let mut original = original_contents();
        original.insert(chunk_hash(ROOT), b"garbage original".to_vec());
        let merged = original_contents();

        assert!(matches!(
            check(&original, &merged, NO_WORLD, SkinPolicy::default()),
            SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::OriginalCorruptBin(_))
        ));
    }

    #[test]
    fn original_missing_required_slot_is_an_anomaly() {
        // The 172/172 assumption: if a game patch ships a skin0 without a
        // skeleton, we want to hear about it — not blame the mod.
        let mut original = original_contents();
        original.insert(chunk_hash(ROOT), skin_bin(None, Some(SKN), Some(TEX), 1.0));
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );

        assert!(matches!(
            check(&original, &merged, NO_WORLD, SkinPolicy::default()),
            SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::OriginalMissingRequiredSlot(
                MeshSlot::Skeleton
            ))
        ));
    }

    #[test]
    fn original_with_unresolvable_ref_is_an_anomaly() {
        let mut original = original_contents();
        original.remove(&chunk_hash(SKL));
        let mut merged = original.clone();
        merged.insert(
            chunk_hash(ROOT),
            skin_bin(Some(SKL), Some(SKN), Some(TEX), 2.0),
        );
        merged.insert(chunk_hash(SKL), b"modded-skeleton".to_vec());

        assert!(matches!(
            check(&original, &merged, NO_WORLD, SkinPolicy::default()),
            SkinCheckOutcome::BaselineAnomaly(BaselineAnomaly::OriginalRefUnresolved {
                slot: MeshSlot::Skeleton,
                ..
            })
        ));
    }
}
