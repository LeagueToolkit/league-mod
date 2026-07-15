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
use thiserror::Error;

use crate::resolve::{CorruptBin, ResolveError, resolve_bin_entry_with};
use crate::skin::{
    MeshSlot, skin_character_data_class, skin_mesh_refs, skin0_bin_path, skin0_entry_hash,
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
}

/// How a referenced asset relates to the merged and original WADs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefStatus {
    /// Same TOC checksum as the original chunk.
    Unmodified,
    /// Present with different content (or a chunk the original lacks).
    /// Not a correctness violation. Note this is checksum-based: an asset
    /// byte-identical to the original but compressed differently also
    /// reads as modified.
    Modified,
    /// Not present in the merged WAD — a closed-world violation.
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
        matches!(self.status, RefStatus::Missing(_))
            && !(policy.allow_dangling_texture && self.slot == MeshSlot::Texture)
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
    /// property is unaffected.
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
                RefStatus::Unmodified | RefStatus::Modified => {}
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
    /// The merged `skin0.bin` chunk is byte-identical to the original — a
    /// vanilla base skin, nothing to check. (Assumption shared with the
    /// in-game verifier: an unmodified root bin means an unmodified base
    /// skin; a mod would have to modify only a *linked* bin to sidestep
    /// it, which in practice does not happen for skin swaps.)
    SkippedUnmodified,
    /// The base skin was checked; see [`SkinIntegrity::is_broken`].
    Report(SkinIntegrity),
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

    let Some(original_root) = original.checksum(root_hash) else {
        return anomaly(BaselineAnomaly::OriginalRootMissing {
            bin_path: root_bin_path,
        });
    };
    let Some(merged_root) = merged.checksum(root_hash) else {
        return anomaly(BaselineAnomaly::MergedRootMissing {
            bin_path: root_bin_path,
        });
    };
    if merged_root == original_root {
        tracing::debug!("'{root_bin_path}' is unmodified; base-skin check skipped");
        return SkinCheckOutcome::SkippedUnmodified;
    }

    // The original WAD must satisfy every assumption this check enforces
    // before the mod can be judged against it.
    if let Err(baseline) = validate_baseline(original, &root_bin_path, entry_hash, skin_class) {
        return anomaly(baseline);
    }

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
        let status = match merged.checksum(name_hash) {
            Some(checksum) if original.checksum(name_hash) == Some(checksum) => {
                RefStatus::Unmodified
            }
            Some(_) => RefStatus::Modified,
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
            missing_required: mesh_refs.missing_required_slots(),
            refs,
            corrupt: outcome.corrupt,
        },
        policy,
    )
}

/// Resolve and extract the original WAD's base skin, mapping every failure
/// to the [`BaselineAnomaly`] it evidences.
fn validate_baseline(
    original: &mut dyn ChunkSource,
    root_bin_path: &str,
    entry_hash: BinHash,
    skin_class: BinHash,
) -> Result<(), BaselineAnomaly> {
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
    Ok(())
}

fn anomaly(baseline: BaselineAnomaly) -> SkinCheckOutcome {
    tracing::error!("base-skin baseline anomaly: {baseline}");
    SkinCheckOutcome::BaselineAnomaly(baseline)
}

fn report(integrity: SkinIntegrity, policy: SkinPolicy) -> SkinCheckOutcome {
    for line in integrity.violations(policy) {
        tracing::warn!("Base-skin violation for {}: {line}", integrity.champion);
    }
    SkinCheckOutcome::Report(integrity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::WadChunkSource;
    use indexmap::IndexMap;
    use ltk_meta::property::{NoMeta, values};
    use ltk_meta::{Bin, BinObject};
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
                (MeshSlot::Texture, RefStatus::Modified),
            ]
        );
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
