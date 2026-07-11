//! Verification diagnostics for League of Legends mod overlays.
//!
//! Verifies the base skin (skin0 — the free skin every player has, and the
//! one mods replace) of a champion in a final merged overlay WAD against the
//! original game WAD: resolve the `Characters/{Champ}/Skins/Skin0` entry the
//! way the game does — starting from
//! `data/characters/{champ}/skins/skin0.bin` and following `linked` bins
//! until the entry is found — then extract the mesh assets its
//! `SkinMeshProperties` references (`Skeleton`, `SimpleSkin`, `Texture`) and
//! report every reference that differs from the original WAD:
//!
//! - **Attested** — modified, but the `(name hash, checksum)` pair is vouched for
//!   (curated from attested statements by the caller).
//! - **Unattested** — modified and nothing vouches for it: logged.
//! - **Missing** — the referenced path is absent from the merged WAD: logged.
//!
//! Chunks byte-identical to the original WAD are omitted from the report,
//! and the whole check is skipped when `skin0.bin` itself is unmodified.
//!
//! The game-side patcher is the enforcement point; the manager runs this as a
//! diagnostic, so violations are logged (`tracing::warn`) and reported, never fatal.
//!
//! The attestation input is deliberately a plain `(name_hash, checksum) -> bool`
//! lookup rather than a token type: the caller typically adapts a
//! [`VerifyContext`](crate::verify::VerifyContext) with a closure —
//! `|hash, checksum| context.attested(hash, FileCheck::ChecksumCompressed(checksum)).is_some()`
//! — keeping WAD diagnostics decoupled from the token layer.
//!
//! Property bins are parsed with [`ltk_meta`]; WADs with [`ltk_wad`]. This
//! module is gated behind the `base-skin` feature because it pulls in that
//! WAD/bin parsing stack, which pure token consumers do not need.

use std::collections::{HashSet, VecDeque};
use std::io::{Cursor, Read, Seek};

use ltk_hash::Hash as _;
use ltk_meta::{Bin, BinObject, PropertyValueEnum};
use ltk_wad::Wad;
use thiserror::Error;

pub use ltk_hash::{BinHash, WadHash};

/// Upper bound on the number of bins visited while following `linked` bins,
/// guarding against absurd or cyclic dependency graphs.
const MAX_LINKED_BINS: usize = 64;

/// Why a bin entry could not be resolved from a root bin and its linked bins.
///
/// Distinct outcomes so callers can report precisely: a missing root bin is
/// "nothing to verify here" (non-champion WAD), while a champion WAD whose
/// skin entry cannot be found is itself diagnostic-worthy.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ResolveError {
    #[error("bin '{bin_path}' is not in the WAD")]
    RootBinMissing { bin_path: String },

    #[error("entry {entry:08x} not found in '{root}' or its linked bins")]
    EntryNotFound { root: String, entry: BinHash },

    #[error("entry {entry:08x} in '{bin_path}' has class {class:08x}, expected {expected:08x}")]
    WrongClass {
        bin_path: String,
        entry: BinHash,
        class: BinHash,
        expected: BinHash,
    },

    #[error("gave up resolving entry from '{root}': more than {limit} linked bins")]
    TooManyLinkedBins { root: String, limit: usize },
}

/// A bin entry resolved by walking a root bin and its linked bins.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBinEntry {
    /// The bin file that defines the entry.
    pub bin_path: String,
    /// xxh64 chunk hash of `bin_path`.
    pub bin_name_hash: u64,
    /// WAD TOC checksum of the bin chunk.
    pub bin_checksum: u64,
    /// The entry object.
    pub object: BinObject,
}

/// Mesh asset references extracted from one `SkinCharacterDataProperties` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinMeshRefs {
    /// Path hash of the entry (e.g. `Characters/Nautilus/Skins/Skin0`).
    pub entry_hash: BinHash,
    /// `SkinMeshProperties.Skeleton` (`.skl`).
    pub skeleton: Option<String>,
    /// `SkinMeshProperties.SimpleSkin` (`.skn`).
    pub simple_skin: Option<String>,
    /// `SkinMeshProperties.Texture` (`.tex`/`.dds`).
    pub texture: Option<String>,
}

impl SkinMeshRefs {
    /// The referenced asset paths that are present, in a fixed order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        [
            self.skeleton.as_deref(),
            self.simple_skin.as_deref(),
            self.texture.as_deref(),
        ]
        .into_iter()
        .flatten()
    }
}

/// Extract the mesh references from one `SkinCharacterDataProperties` object
/// (its `SkinMeshProperties` embed).
pub fn skin_mesh_refs(object: &BinObject) -> SkinMeshRefs {
    let mesh_prop = BinHash::hash_str("SkinMeshProperties");
    let skeleton_prop = BinHash::hash_str("Skeleton");
    let simple_skin_prop = BinHash::hash_str("SimpleSkin");
    let texture_prop = BinHash::hash_str("Texture");

    let mut refs = SkinMeshRefs {
        entry_hash: object.path_hash,
        skeleton: None,
        simple_skin: None,
        texture: None,
    };
    let mesh_properties = match object.properties.get(&mesh_prop) {
        Some(PropertyValueEnum::Embedded(embedded)) => Some(&embedded.0.properties),
        Some(PropertyValueEnum::Struct(inner)) => Some(&inner.properties),
        _ => None,
    };
    if let Some(props) = mesh_properties {
        let string_of = |key: BinHash| match props.get(&key) {
            Some(PropertyValueEnum::String(s)) => Some(s.value.clone()),
            _ => None,
        };
        refs.skeleton = string_of(skeleton_prop);
        refs.simple_skin = string_of(simple_skin_prop);
        refs.texture = string_of(texture_prop);
    }
    refs
}

/// How a chunk referenced by a skin bin relates to the trust inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinRefStatus {
    /// Byte-identical to the original game chunk — nothing to verify.
    /// Never listed in [`SkinBinReport::refs`]; can still appear as
    /// [`SkinBinReport::bin_status`] when the entry was found in an
    /// untouched linked bin.
    Unmodified,
    /// Modified, and the `(name hash, checksum)` pair is attested.
    Attested,
    /// Modified, and nothing vouches for it.
    Unattested,
    /// The referenced path has no chunk in the merged WAD at all.
    Missing,
}

/// One referenced asset and its classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinRefCheck {
    /// The path exactly as referenced by the bin.
    pub path: String,
    /// xxh64 chunk hash of the (lowercased) path.
    pub name_hash: u64,
    pub status: SkinRefStatus,
}

/// Verification report for a champion's base skin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinBinReport {
    /// The bin file in which the skin entry was found — `skin0.bin` itself
    /// or one of its linked bins.
    pub bin_path: String,
    /// xxh64 chunk hash of the bin path.
    pub bin_name_hash: u64,
    /// Status of the bin chunk itself (informational; the attestation rule
    /// the runtime enforces is about the referenced mesh assets).
    pub bin_status: SkinRefStatus,
    /// Classification of every referenced mesh asset that differs from the
    /// original WAD; references byte-identical to the original are omitted.
    pub refs: Vec<SkinRefCheck>,
}

impl SkinBinReport {
    /// Whether every reported reference is attested (references identical
    /// to the original WAD are never reported).
    pub fn is_clean(&self) -> bool {
        self.refs.iter().all(|r| {
            matches!(
                r.status,
                SkinRefStatus::Unmodified | SkinRefStatus::Attested
            )
        })
    }
}

/// Classify one chunk against the trust inputs.
///
/// `merged_checksum`/`original_checksum` are the WAD TOC checksums (xxh3 of
/// the compressed chunk data as stored); `attested` decides whether a
/// `(name hash, checksum)` pair is vouched for.
pub fn classify_chunk(
    name_hash: u64,
    merged_checksum: Option<u64>,
    original_checksum: Option<u64>,
    attested: &dyn Fn(u64, u64) -> bool,
) -> SkinRefStatus {
    match merged_checksum {
        None => SkinRefStatus::Missing,
        Some(checksum) => {
            if original_checksum == Some(checksum) {
                SkinRefStatus::Unmodified
            } else if attested(name_hash, checksum) {
                SkinRefStatus::Attested
            } else {
                SkinRefStatus::Unattested
            }
        }
    }
}

/// Chunk access for [`resolve_bin_entry_with`]: the walk only asks whether
/// a chunk exists and loads its decompressed bytes. [`resolve_bin_entry`]
/// implements it over a mounted WAD; statement extraction implements it
/// over a *virtual merge* (mod overrides on top of the original WAD) so
/// producers resolve the entry exactly as the verifier will.
pub trait BinChunkSource {
    /// Whether a chunk with this name hash exists.
    fn contains(&mut self, name_hash: u64) -> bool;
    /// Decompressed chunk bytes; `None` for chunks that exist but cannot be
    /// read (implementations log the reason — the walk skips and continues).
    fn load(&mut self, name_hash: u64) -> Option<Vec<u8>>;
}

/// A bin entry resolved over an abstract chunk source: [`ResolvedBinEntry`]
/// without the WAD TOC checksum, which such a source does not have.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedBinObject {
    /// The bin file that defines the entry.
    pub bin_path: String,
    /// xxh64 chunk hash of `bin_path`.
    pub bin_name_hash: u64,
    /// The entry object.
    pub object: BinObject,
}

/// Resolve a bin entry the way the game does: walk the root bin and its
/// `linked` bins (breadth-first, within this chunk source) and return as
/// soon as a bin defines the entry.
///
/// Linked bins absent from the source (e.g. references into Global) are
/// skipped with a debug log; bins that fail to load or parse are skipped
/// with a warning — neither aborts the walk. When `expected_class` is
/// given, a found entry with a different class is a definitive
/// [`ResolveError::WrongClass`] (entry path hashes are unique across the
/// merged bin graph, so there is nothing further to search).
pub fn resolve_bin_entry_with(
    source: &mut dyn BinChunkSource,
    root_bin_path: &str,
    entry_hash: BinHash,
    expected_class: Option<BinHash>,
) -> std::result::Result<ResolvedBinObject, ResolveError> {
    if !source.contains(*WadHash::hash_str(root_bin_path)) {
        return Err(ResolveError::RootBinMissing {
            bin_path: root_bin_path.to_owned(),
        });
    }

    let mut queue = VecDeque::from([root_bin_path.to_owned()]);
    let mut visited: HashSet<u64> = HashSet::new();

    while let Some(bin_path) = queue.pop_front() {
        let bin_name_hash = *WadHash::hash_str(&bin_path);
        if !visited.insert(bin_name_hash) {
            continue;
        }
        if visited.len() > MAX_LINKED_BINS {
            return Err(ResolveError::TooManyLinkedBins {
                root: root_bin_path.to_owned(),
                limit: MAX_LINKED_BINS,
            });
        }

        if !source.contains(bin_name_hash) {
            tracing::debug!("Linked bin '{bin_path}' is not in this WAD, skipping");
            continue;
        }
        let Some(data) = source.load(bin_name_hash) else {
            continue;
        };
        let bin = match Bin::from_reader(&mut Cursor::new(&data[..])) {
            Ok(bin) => bin,
            Err(e) => {
                tracing::warn!("Failed to parse bin '{bin_path}': {e}");
                continue;
            }
        };

        if let Some(object) = bin.get_object(entry_hash) {
            if let Some(expected) = expected_class
                && object.class_hash != expected
            {
                return Err(ResolveError::WrongClass {
                    bin_path,
                    entry: entry_hash,
                    class: object.class_hash,
                    expected,
                });
            }
            return Ok(ResolvedBinObject {
                bin_path,
                bin_name_hash,
                object: object.clone(),
            });
        }

        queue.extend(bin.dependencies.iter().cloned());
    }

    Err(ResolveError::EntryNotFound {
        root: root_bin_path.to_owned(),
        entry: entry_hash,
    })
}

/// Resolve a bin entry from a mounted WAD (see [`resolve_bin_entry_with`]
/// for the walk semantics).
pub fn resolve_bin_entry<TSource: Read + Seek>(
    wad: &mut Wad<TSource>,
    root_bin_path: &str,
    entry_hash: BinHash,
    expected_class: Option<BinHash>,
) -> std::result::Result<ResolvedBinEntry, ResolveError> {
    struct WadSource<'a, T: Read + Seek>(&'a mut Wad<T>);
    impl<T: Read + Seek> BinChunkSource for WadSource<'_, T> {
        fn contains(&mut self, name_hash: u64) -> bool {
            self.0.chunks().contains(name_hash)
        }
        fn load(&mut self, name_hash: u64) -> Option<Vec<u8>> {
            let chunk = self.0.chunks().get(name_hash).copied()?;
            match self.0.load_chunk_decompressed(&chunk) {
                Ok(data) => Some(data.into_vec()),
                Err(e) => {
                    tracing::warn!("Failed to decompress bin chunk {name_hash:016x}: {e}");
                    None
                }
            }
        }
    }

    let resolved = resolve_bin_entry_with(
        &mut WadSource(wad),
        root_bin_path,
        entry_hash,
        expected_class,
    )?;
    let bin_checksum = wad
        .chunks()
        .get(resolved.bin_name_hash)
        .expect("resolved bin was loaded from this WAD")
        .checksum;
    Ok(ResolvedBinEntry {
        bin_path: resolved.bin_path,
        bin_name_hash: resolved.bin_name_hash,
        bin_checksum,
        object: resolved.object,
    })
}

/// Verify the mesh assets referenced by a champion's base skin (skin0 — the
/// free skin, the one mods replace) in a mounted merged overlay WAD against
/// the mounted original game WAD.
///
/// Returns `Ok(None)` when the merged WAD's
/// `data/characters/{champion}/skins/skin0.bin` chunk is byte-identical to
/// the original's — a vanilla skin bin, nothing to check. Otherwise the
/// `Characters/{Champ}/Skins/Skin0` entry is resolved via
/// [`resolve_bin_entry`] and every referenced mesh asset that differs from
/// the original WAD is classified in the report; references byte-identical
/// to the original are omitted.
///
/// * `original` — the unpatched game WAD; only its TOC checksums are read.
/// * `merged` — the final overlay WAD (original plus patches) the game loads.
/// * `champion` — lowercase champion directory name (see
///   [`champion_from_wad_filename`]).
/// * `attested` — whether a `(name hash, checksum)` pair is attested.
///
/// [`ResolveError::RootBinMissing`] means "nothing to verify" (a non-champion
/// WAD); the other resolve errors are diagnostic-worthy. Reference violations
/// are logged via `tracing::warn` and returned in the report; this is a
/// diagnostic, never fatal (enforcement belongs to the game-side patcher).
pub fn verify_base_skin<TOriginal: Read + Seek, TMerged: Read + Seek>(
    original: &Wad<TOriginal>,
    merged: &mut Wad<TMerged>,
    champion: &str,
    attested: &dyn Fn(u64, u64) -> bool,
) -> std::result::Result<Option<SkinBinReport>, ResolveError> {
    let entry_hash = BinHash::hash_str(format!("characters/{champion}/skins/skin0"));
    let skin_class = BinHash::hash_str("SkinCharacterDataProperties");
    let root_bin_path = format!("data/characters/{champion}/skins/skin0.bin");
    let root_name_hash = *WadHash::hash_str(&root_bin_path);

    let original_checksum = |hash: u64| original.chunks().get(hash).map(|c| c.checksum);

    if let Some(merged_root) = merged.chunks().get(root_name_hash)
        && original_checksum(root_name_hash) == Some(merged_root.checksum)
    {
        tracing::debug!("'{root_bin_path}' is unmodified; base-skin check skipped");
        return Ok(None);
    }

    let resolved = resolve_bin_entry(merged, &root_bin_path, entry_hash, Some(skin_class))?;
    Ok(Some(build_report(
        merged,
        resolved,
        &original_checksum,
        attested,
    )))
}

/// Classify the bin containing the skin entry and every mesh asset it
/// references, dropping references byte-identical to the original WAD.
fn build_report<TSource: Read + Seek>(
    merged: &mut Wad<TSource>,
    resolved: ResolvedBinEntry,
    original_checksum: &dyn Fn(u64) -> Option<u64>,
    attested: &dyn Fn(u64, u64) -> bool,
) -> SkinBinReport {
    let ResolvedBinEntry {
        bin_path,
        bin_name_hash,
        bin_checksum,
        object,
    } = resolved;

    let bin_status = classify_chunk(
        bin_name_hash,
        Some(bin_checksum),
        original_checksum(bin_name_hash),
        attested,
    );

    let mesh_refs = skin_mesh_refs(&object);
    let mut refs = Vec::new();
    for path in mesh_refs.paths() {
        let name_hash = *WadHash::hash_str(path);
        let merged_checksum = merged.chunks().get(name_hash).map(|c| c.checksum);
        let status = classify_chunk(
            name_hash,
            merged_checksum,
            original_checksum(name_hash),
            attested,
        );
        match status {
            SkinRefStatus::Unmodified => continue,
            SkinRefStatus::Unattested => tracing::warn!(
                "Skin bin '{bin_path}' references modified, unattested \
                 file '{path}' ({name_hash:016x})"
            ),
            SkinRefStatus::Missing => tracing::warn!(
                "Skin bin '{bin_path}' references '{path}' \
                 ({name_hash:016x}), which is missing from the merged WAD"
            ),
            SkinRefStatus::Attested => {}
        }
        refs.push(SkinRefCheck {
            path: path.to_owned(),
            name_hash,
            status,
        });
    }

    let report = SkinBinReport {
        bin_path,
        bin_name_hash,
        bin_status,
        refs,
    };
    tracing::debug!(
        "Base-skin verification via '{}': {} modified references, clean: {}",
        report.bin_path,
        report.refs.len(),
        report.is_clean()
    );
    report
}

/// SHA-256 digest identifying a WAD build by its TOC: for every chunk in TOC
/// order, `path_hash` (u64 LE) followed by `checksum` (u64 LE).
///
/// This is the canonical derivation of the `wad_toc_digest` statement claim:
/// producers hash the original game WAD they validated against, verifiers
/// hash the WAD they are patching, and equality pins the statement to that
/// exact game build.
pub fn wad_toc_digest<TSource: Read + Seek>(wad: &Wad<TSource>) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    for chunk in wad.chunks().iter() {
        hasher.update(chunk.path_hash.to_le_bytes());
        hasher.update(chunk.checksum.to_le_bytes());
    }
    hasher.finalize().into()
}

/// Derive the lowercase champion directory name from a WAD filename
/// (`Nautilus.wad.client` → `nautilus`).
pub fn champion_from_wad_filename(file_name: &str) -> Option<String> {
    let stem = file_name.split('.').next()?;
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use ltk_meta::property::{NoMeta, values};
    use ltk_meta::{Bin, BinObject};
    use std::collections::{BTreeMap, HashSet};
    use std::io::Write;

    const SKL: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skl";
    const SKN: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body.skn";
    const TEX: &str = "ASSETS/Characters/Testchamp/Skins/Skin01/body_TX_CM.tex";

    fn h(name: &str) -> BinHash {
        BinHash::hash_str(name)
    }

    /// A skin entry with unrelated properties around the mesh embed, at the
    /// given entry path. `scale` varies the bytes so tests can produce a
    /// "modded" variant of an otherwise identical bin.
    fn skin_entry_scaled(entry_path: &str, scale: f32) -> BinObject {
        let mesh = values::Embedded(values::Struct {
            class_hash: h("SkinMeshDataProperties"),
            properties: IndexMap::from([
                (h("Skeleton"), values::String::from(SKL).into()),
                (h("SimpleSkin"), values::String::from(SKN).into()),
                (h("Texture"), values::String::from(TEX).into()),
                (h("SkinScale"), values::F32::new(scale).into()),
            ]),
            meta: NoMeta,
        });

        BinObject::<NoMeta>::builder(h(entry_path), h("SkinCharacterDataProperties"))
            .property(h("SkinClassification"), values::U32::new(1))
            .property(h("ChampionSkinName"), values::String::from("TestchampBase"))
            .property(h("SkinMeshProperties"), mesh)
            .build()
    }

    fn skin_entry(entry_path: &str) -> BinObject {
        skin_entry_scaled(entry_path, 1.1)
    }

    /// A realistic skin0 bin built with the official `ltk_meta` types: a
    /// resource-resolver entry alongside the skin entry at the path the game
    /// resolves.
    fn skin_bin_scaled(scale: f32) -> Bin {
        let resolver_entry = BinObject::<NoMeta>::builder(
            h("Characters/Testchamp/Skins/Skin0/Resources"),
            h("ResourceResolver"),
        )
        .build();

        Bin::builder()
            .dependency("DATA/Characters/Testchamp/Testchamp.bin")
            .object(resolver_entry)
            .object(skin_entry_scaled("Characters/Testchamp/Skins/Skin0", scale))
            .build()
    }

    fn skin_bin() -> Bin {
        skin_bin_scaled(1.1)
    }

    fn bin_bytes(bin: &Bin) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        bin.to_writer(&mut cursor).unwrap();
        cursor.into_inner()
    }

    // ---------------------------------------------------------- extraction

    #[test]
    fn extracts_mesh_refs_from_skin_entry() {
        // Round-trip through the real wire format before extracting.
        let bytes = bin_bytes(&skin_bin());
        let bin = Bin::from_reader(&mut Cursor::new(&bytes[..])).unwrap();

        let object = bin
            .get_object(h("Characters/Testchamp/Skins/Skin0"))
            .unwrap();
        let skin = skin_mesh_refs(object);
        assert_eq!(skin.entry_hash, h("Characters/Testchamp/Skins/Skin0"));
        assert_eq!(skin.skeleton.as_deref(), Some(SKL));
        assert_eq!(skin.simple_skin.as_deref(), Some(SKN));
        assert_eq!(skin.texture.as_deref(), Some(TEX));
        assert_eq!(skin.paths().count(), 3);
    }

    #[test]
    fn missing_mesh_properties_yield_empty_refs() {
        let object = BinObject::<NoMeta>::builder(
            h("Characters/Testchamp/Skins/Skin0"),
            h("SkinCharacterDataProperties"),
        )
        .property(h("SkinClassification"), values::U32::new(1))
        .build();
        assert_eq!(skin_mesh_refs(&object).paths().count(), 0);
    }

    // ------------------------------------------------------- classification

    #[test]
    fn classify_chunk_statuses() {
        let attested: HashSet<(u64, u64)> = HashSet::from([(7, 100)]);
        let lookup = |hash: u64, checksum: u64| attested.contains(&(hash, checksum));

        assert_eq!(
            classify_chunk(7, None, None, &lookup),
            SkinRefStatus::Missing
        );
        assert_eq!(
            classify_chunk(7, Some(5), Some(5), &lookup),
            SkinRefStatus::Unmodified
        );
        assert_eq!(
            classify_chunk(7, Some(100), Some(5), &lookup),
            SkinRefStatus::Attested
        );
        assert_eq!(
            classify_chunk(7, Some(100), None, &lookup),
            SkinRefStatus::Attested
        );
        assert_eq!(
            classify_chunk(7, Some(101), Some(5), &lookup),
            SkinRefStatus::Unattested
        );
    }

    #[test]
    fn champion_derivation() {
        assert_eq!(
            champion_from_wad_filename("Nautilus.wad.client").as_deref(),
            Some("nautilus")
        );
        assert_eq!(
            champion_from_wad_filename("Map11.wad.client").as_deref(),
            Some("map11")
        );
        assert_eq!(champion_from_wad_filename(""), None);
    }

    // ------------------------------------------------- end-to-end over WADs

    fn chunk_hash(path: &str) -> u64 {
        *WadHash::hash_str(path)
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

    /// An original WAD with no chunks: everything in the merged WAD counts
    /// as modified.
    fn empty_wad() -> Wad<Cursor<Vec<u8>>> {
        build_wad(&BTreeMap::new())
    }

    #[test]
    fn unmodified_skin0_bin_skips_check() {
        let bin_hash = chunk_hash("data/characters/testchamp/skins/skin0.bin");
        let (skl, skn, tex) = (chunk_hash(SKL), chunk_hash(SKN), chunk_hash(TEX));

        let original = build_wad(&BTreeMap::from([
            (bin_hash, bin_bytes(&skin_bin())),
            (skl, b"skeleton-data".to_vec()),
            (skn, b"mesh-data".to_vec()),
            (tex, b"texture-data".to_vec()),
        ]));
        // The texture is modified, but skin0.bin itself is vanilla: the
        // check does not apply.
        let mut merged = build_wad(&BTreeMap::from([
            (bin_hash, bin_bytes(&skin_bin())),
            (skl, b"skeleton-data".to_vec()),
            (skn, b"mesh-data".to_vec()),
            (tex, b"MODDED-texture-data".to_vec()),
        ]));

        let report = verify_base_skin(&original, &mut merged, "testchamp", &|_, _| false).unwrap();
        assert_eq!(report, None);
    }

    #[test]
    fn verifies_skin_refs_across_original_and_merged() {
        let bin_hash = chunk_hash("data/characters/testchamp/skins/skin0.bin");
        let (skl, skn, tex) = (chunk_hash(SKL), chunk_hash(SKN), chunk_hash(TEX));

        let original = build_wad(&BTreeMap::from([
            (bin_hash, bin_bytes(&skin_bin())),
            (skl, b"skeleton-data".to_vec()),
            (skn, b"mesh-data".to_vec()),
            (tex, b"texture-data".to_vec()),
        ]));
        // Merged: the mod rewrites the skin bin and replaces the texture;
        // skeleton and mesh pass through untouched.
        let mut merged = build_wad(&BTreeMap::from([
            (bin_hash, bin_bytes(&skin_bin_scaled(2.0))),
            (skl, b"skeleton-data".to_vec()),
            (skn, b"mesh-data".to_vec()),
            (tex, b"MODDED-texture-data".to_vec()),
        ]));
        let modded_tex_checksum = merged.chunks().get(tex).unwrap().checksum;

        // Nothing attested: only the modified texture is reported —
        // untouched references are omitted entirely.
        let report = verify_base_skin(&original, &mut merged, "testchamp", &|_, _| false)
            .unwrap()
            .unwrap();
        assert_eq!(report.bin_path, "data/characters/testchamp/skins/skin0.bin");
        assert_eq!(report.bin_status, SkinRefStatus::Unattested);
        assert_eq!(report.refs.len(), 1);
        assert_eq!(report.refs[0].path, TEX);
        assert_eq!(report.refs[0].status, SkinRefStatus::Unattested);
        assert!(!report.is_clean());

        // With the texture attested, the skin is clean.
        let attested = HashSet::from([(tex, modded_tex_checksum)]);
        let report = verify_base_skin(&original, &mut merged, "testchamp", &|h, c| {
            attested.contains(&(h, c))
        })
        .unwrap()
        .unwrap();
        assert!(report.is_clean());
        assert_eq!(report.refs.len(), 1);
        assert_eq!(report.refs[0].status, SkinRefStatus::Attested);
    }

    #[test]
    fn missing_reference_is_flagged() {
        let bin_hash = chunk_hash("data/characters/testchamp/skins/skin0.bin");
        // The bin references SKL/SKN/TEX but the merged WAD only contains
        // the bin and the skeleton; nothing exists in the original.
        let original = empty_wad();
        let contents = BTreeMap::from([
            (bin_hash, bin_bytes(&skin_bin())),
            (chunk_hash(SKL), b"skeleton-data".to_vec()),
        ]);
        let mut merged = build_wad(&contents);

        let report = verify_base_skin(&original, &mut merged, "testchamp", &|_, _| false)
            .unwrap()
            .unwrap();
        let missing: Vec<&str> = report
            .refs
            .iter()
            .filter(|r| r.status == SkinRefStatus::Missing)
            .map(|r| r.path.as_str())
            .collect();
        assert_eq!(missing, vec![SKN, TEX]);
        // The skeleton exists only in the merged WAD: modified, unattested.
        assert_eq!(report.refs.len(), 3);
        assert_eq!(report.refs[0].path, SKL);
        assert_eq!(report.refs[0].status, SkinRefStatus::Unattested);
        assert!(!report.is_clean());
    }

    #[test]
    fn garbage_bin_is_skipped_not_fatal() {
        let bin_hash = chunk_hash("data/characters/testchamp/skins/skin0.bin");
        let contents = BTreeMap::from([(bin_hash, b"not a property bin".to_vec())]);
        let mut merged = build_wad(&contents);

        let result = verify_base_skin(&empty_wad(), &mut merged, "testchamp", &|_, _| false);
        assert!(matches!(result, Err(ResolveError::EntryNotFound { .. })));
    }

    #[test]
    fn wad_without_base_skin_is_root_bin_missing() {
        // A WAD with no skin0.bin at all (e.g. a non-champion WAD): the
        // benign "nothing to verify here" outcome.
        let contents = BTreeMap::from([(chunk_hash("data/other/file.bin"), b"data".to_vec())]);
        let mut merged = build_wad(&contents);

        let result = verify_base_skin(&empty_wad(), &mut merged, "testchamp", &|_, _| false);
        assert!(matches!(result, Err(ResolveError::RootBinMissing { .. })));
    }

    #[test]
    fn entry_at_wrong_path_is_not_used() {
        // The bin contains a SkinCharacterDataProperties entry, but at a
        // different champion's path — the game would not resolve it for
        // testchamp, so neither do we.
        let bin = Bin::builder()
            .object(skin_entry("Characters/Otherchamp/Skins/Skin0"))
            .build();
        let contents = BTreeMap::from([(
            chunk_hash("data/characters/testchamp/skins/skin0.bin"),
            bin_bytes(&bin),
        )]);
        let mut merged = build_wad(&contents);

        let result = verify_base_skin(&empty_wad(), &mut merged, "testchamp", &|_, _| false);
        assert!(matches!(result, Err(ResolveError::EntryNotFound { .. })));
    }

    #[test]
    fn entry_with_wrong_class_is_a_distinct_error() {
        // The entry path resolves, but the object is not a
        // SkinCharacterDataProperties — definitive, reported as such.
        let bin = Bin::builder()
            .object(
                BinObject::<NoMeta>::builder(
                    h("Characters/Testchamp/Skins/Skin0"),
                    h("ResourceResolver"),
                )
                .build(),
            )
            .build();
        let contents = BTreeMap::from([(
            chunk_hash("data/characters/testchamp/skins/skin0.bin"),
            bin_bytes(&bin),
        )]);
        let mut merged = build_wad(&contents);

        let result = verify_base_skin(&empty_wad(), &mut merged, "testchamp", &|_, _| false);
        assert!(matches!(
            result,
            Err(ResolveError::WrongClass { class, expected, .. })
                if class == h("ResourceResolver")
                    && expected == h("SkinCharacterDataProperties")
        ));
    }

    #[test]
    fn entry_found_via_linked_bin() {
        // skin0.bin defines nothing itself but links a concat bin (plus a
        // dependency living outside this WAD, which must be skipped); the
        // skin entry lives in the concat bin.
        let root = Bin::builder()
            .dependency("DATA/Characters/Testchamp/Testchamp.bin") // not in WAD
            .dependency("data/testchamp_skin0_concat.bin")
            .build();
        let concat = Bin::builder()
            .object(skin_entry("Characters/Testchamp/Skins/Skin0"))
            .build();

        let contents = BTreeMap::from([
            (
                chunk_hash("data/characters/testchamp/skins/skin0.bin"),
                bin_bytes(&root),
            ),
            (
                chunk_hash("data/testchamp_skin0_concat.bin"),
                bin_bytes(&concat),
            ),
            (chunk_hash(SKL), b"skeleton-data".to_vec()),
            (chunk_hash(SKN), b"mesh-data".to_vec()),
            (chunk_hash(TEX), b"texture-data".to_vec()),
        ]);
        let mut merged = build_wad(&contents);

        // Empty original WAD: everything is "modified" and unattested.
        let report = verify_base_skin(&empty_wad(), &mut merged, "testchamp", &|_, _| false)
            .unwrap()
            .unwrap();
        assert_eq!(report.bin_path, "data/testchamp_skin0_concat.bin");
        assert_eq!(report.refs.len(), 3);
        assert!(
            report
                .refs
                .iter()
                .all(|r| r.status == SkinRefStatus::Unattested)
        );
    }

    #[test]
    fn linked_bin_cycles_terminate() {
        // skin0.bin and the concat bin link each other; neither defines the
        // skin entry. The walk must terminate with no report.
        let root = Bin::builder()
            .dependency("data/testchamp_skin0_concat.bin")
            .build();
        let concat = Bin::builder()
            .dependency("data/characters/testchamp/skins/skin0.bin")
            .build();

        let contents = BTreeMap::from([
            (
                chunk_hash("data/characters/testchamp/skins/skin0.bin"),
                bin_bytes(&root),
            ),
            (
                chunk_hash("data/testchamp_skin0_concat.bin"),
                bin_bytes(&concat),
            ),
        ]);
        let mut merged = build_wad(&contents);

        let result = verify_base_skin(&empty_wad(), &mut merged, "testchamp", &|_, _| false);
        assert!(matches!(result, Err(ResolveError::EntryNotFound { .. })));
    }
}
