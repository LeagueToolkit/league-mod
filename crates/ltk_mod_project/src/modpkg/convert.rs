//! Conversions between mod project types and the `.modpkg` metadata shapes.
//!
//! The counterpart of the Fantome module's `convert`, with one difference that
//! shapes the whole module: a package stores its layer table twice. The header
//! carries each layer's name and priority and is what the loader reads; the
//! metadata chunk carries the display name, description and string overrides a
//! header layer has no room for. Reading a project back out of a package is the
//! join of the two, which is why [`read_project`] takes the package rather than
//! its metadata alone.

use std::io::{Read, Seek};

use ltk_modpkg::{
    Modpkg, ModpkgAuthor, ModpkgError, ModpkgLayer, ModpkgLayerMetadata, ModpkgLicense,
    ModpkgMetadata,
};

use camino::Utf8Path;

use crate::hashtable_routes::{file_name_of, is_plain_tail, NameClaims, PlannedRoute};
use crate::{
    ModMap, ModProject, ModProjectAuthor, ModProjectHashtable, ModProjectLayer, ModProjectLicense,
    ModTag, HASHES_DIR_NAME,
};

/// Read the project a mounted `.modpkg` describes.
///
/// The layer table is the join of the package's header against its metadata
/// chunk: the header decides which layers exist and what their priorities are,
/// the metadata fills in the display name, description and string overrides.
/// A layer the metadata names but the header does not is not a layer the
/// package holds content for, and is dropped.
///
/// Only the metadata chunk is decompressed; the content chunks are left where
/// they are. That is what makes this cheap enough for a caller that wants a
/// package's config without unpacking it.
///
/// # Errors
///
/// Fails with [`ModpkgError`] when the package ships no metadata chunk, or the
/// chunk cannot be decompressed or decoded.
pub fn read_project<R: Read + Seek>(modpkg: &mut Modpkg<R>) -> Result<ModProject, ModpkgError> {
    let metadata = modpkg.load_metadata()?;

    Ok(ModProject {
        layers: layers_from_modpkg(modpkg.layers().values(), metadata.layers()),
        ..ModProject::from(&metadata)
    })
}

/// The project an archive's metadata chunk describes.
///
/// The layer table comes from the metadata chunk alone, where the priorities
/// are informational. A caller holding the package itself should prefer
/// [`read_project`], which reads them from the header the loader reads.
impl From<&ModpkgMetadata> for ModProject {
    fn from(metadata: &ModpkgMetadata) -> Self {
        let mut layers: Vec<ModProjectLayer> = metadata
            .layers()
            .iter()
            .map(ModProjectLayer::from)
            .collect();
        ModProjectLayer::normalize_table(&mut layers);

        Self {
            name: metadata.name().to_owned(),
            display_name: metadata.display_name().to_owned(),
            version: metadata.version().to_string(),
            description: metadata.description().unwrap_or_default().to_owned(),
            authors: metadata
                .authors()
                .iter()
                .map(ModProjectAuthor::from)
                .collect(),
            license: license_from_modpkg(metadata.license()),
            tags: metadata.tags().iter().cloned().map(ModTag::from).collect(),
            champions: metadata.champions().to_vec(),
            maps: metadata.maps().iter().cloned().map(ModMap::from).collect(),
            transformers: vec![],
            layers,
            thumbnail: None,
            hashtables: project_hashtables(metadata.hashtables()),
        }
    }
}

/// The project manifest a package's `hashtables` declarations become.
///
/// Each path is rewritten to where the extractor writes the file, by the
/// package format's own placement rule ([`ltk_modpkg::hashtable_file_name`]),
/// so the written files and the written manifest cannot drift. An entry
/// whose chunk the extractor plans nothing for - one declared outside
/// `_meta_/hashes/` - is dropped with it: keeping the declaration would
/// declare a file the import never writes.
///
/// An entry whose declared width no key can have is dropped too, mirroring
/// the fantome side: `PackPlan::hashtables()` refuses an impossible width,
/// so carrying it would import a project that cannot pack.
pub(crate) fn project_hashtables(
    declared: &[ltk_modpkg::ModpkgHashtable],
) -> Vec<ModProjectHashtable> {
    declared
        .iter()
        .filter(|manifest| manifest.to_entry().is_some())
        .filter_map(|manifest| {
            let file_name = ltk_modpkg::hashtable_file_name(&manifest.path)?;
            Some(ModProjectHashtable {
                path: format!("{HASHES_DIR_NAME}/{file_name}"),
                category: manifest.category.clone(),
                algorithm: manifest.algorithm.clone(),
                bits: manifest.bits,
            })
        })
        .collect()
}

/// Where each planned table lands in the package, paired with its table.
///
/// Every planned table maps, verbatim except for the path. A path directly
/// under `hashes/` keeps its file name beneath `_meta_/hashes/`, so pack,
/// import, pack again keeps every table where it was; a table declared
/// elsewhere (or nested) lands under `_meta_/hashes/` by its file name -
/// `ModpkgBuilder::with_hashtable` takes nothing deeper.
///
/// # Errors
///
/// [`DuplicateHashtableName`](crate::DuplicateHashtableName) when two
/// different declared files land on one chunk name - refused rather than
/// renamed, since the author can rename a file and a silently renamed table
/// would ship under a name nobody chose.
pub(crate) fn modpkg_routes(
    planned: &[crate::pack::PlannedHashtable],
) -> Result<Vec<PlannedRoute<'_, ltk_modpkg::ModpkgHashtable>>, crate::DuplicateHashtableName> {
    let mut claims = NameClaims::default();
    planned
        .iter()
        .map(|planned| {
            let source = planned.entry().path();
            let file_name = source
                .strip_prefix(HASHES_DIR_NAME)
                .ok()
                .map(Utf8Path::as_str)
                .filter(|tail| is_plain_tail(tail) && !tail.contains('/'))
                .unwrap_or_else(|| file_name_of(source.as_str()));
            let mapped = ltk_modpkg::ModpkgHashtable {
                path: claims.claim(ltk_modpkg::HASHTABLES_CHUNK_DIR, file_name, source.as_str())?,
                category: planned.entry().category().clone(),
                algorithm: planned.entry().algorithm().clone(),
                bits: planned.entry().width().bits(),
            };
            Ok(PlannedRoute {
                manifest: mapped,
                planned,
            })
        })
        .collect()
}

/// The layer table a package declares, joining its header against its metadata.
///
/// `header` decides the table: it is what the loader reads, so a priority the
/// two disagree about is the header's. `metadata` is matched into it by name.
/// [`ModProjectLayer::normalize_table`] orders the result, since the header's
/// table is hashed.
fn layers_from_modpkg<'a>(
    header: impl IntoIterator<Item = &'a ModpkgLayer>,
    metadata: &[ModpkgLayerMetadata],
) -> Vec<ModProjectLayer> {
    let mut layers: Vec<ModProjectLayer> = header
        .into_iter()
        .map(|layer| {
            let described = metadata.iter().find(|entry| entry.name == layer.name);

            ModProjectLayer {
                name: layer.name.clone(),
                display_name: described.and_then(|entry| entry.display_name.clone()),
                priority: layer.priority,
                description: described.and_then(|entry| entry.description.clone()),
                string_overrides: described
                    .map(|entry| entry.string_overrides.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();

    ModProjectLayer::normalize_table(&mut layers);

    layers
}

/// A layer as the metadata chunk alone describes it, priority included.
impl From<&ModpkgLayerMetadata> for ModProjectLayer {
    fn from(layer: &ModpkgLayerMetadata) -> Self {
        Self {
            name: layer.name.clone(),
            display_name: layer.display_name.clone(),
            priority: layer.priority,
            description: layer.description.clone(),
            string_overrides: layer.string_overrides.clone(),
        }
    }
}

impl From<&ModpkgAuthor> for ModProjectAuthor {
    fn from(author: &ModpkgAuthor) -> Self {
        match author.role() {
            Some(role) => Self::Role {
                name: author.name().to_owned(),
                role: role.to_owned(),
            },
            None => Self::Name(author.name().to_owned()),
        }
    }
}

/// The project's license field, absent for a package declaring none.
///
/// A free function rather than a `From` impl, because the absent case makes the
/// result an `Option` and there is no conversion to write for the inner type
/// alone.
fn license_from_modpkg(license: &ModpkgLicense) -> Option<ModProjectLicense> {
    match license {
        ModpkgLicense::None => None,
        ModpkgLicense::Spdx { spdx_id } => Some(ModProjectLicense::Spdx(spdx_id.clone())),
        ModpkgLicense::Custom { name, url } => Some(ModProjectLicense::Custom {
            name: name.clone(),
            url: url.clone(),
        }),
    }
}
