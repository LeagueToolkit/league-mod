//! Conversions between mod project types and the Fantome metadata shapes.

use std::collections::HashMap;

use camino::Utf8Path;

use crate::hashtable_routes::{
    file_name_of, is_plain_tail, HashtableRoute, NameClaims, PlannedRoute,
};
use ltk_fantome::{FantomeHashtable, FantomeInfo, FantomeLayerInfo, FantomeLicense};

use crate::{
    ModMap, ModProject, ModProjectAuthor, ModProjectHashtable, ModProjectLayer, ModProjectLicense,
    ModTag, HASHES_DIR_NAME,
};

impl From<&ModProjectLicense> for FantomeLicense {
    fn from(license: &ModProjectLicense) -> Self {
        match license {
            ModProjectLicense::Spdx(id) => FantomeLicense::Spdx(id.clone()),
            ModProjectLicense::Custom { name, url } => FantomeLicense::Custom {
                name: name.clone(),
                url: url.clone(),
            },
        }
    }
}

impl From<FantomeLicense> for ModProjectLicense {
    fn from(license: FantomeLicense) -> Self {
        match license {
            FantomeLicense::Spdx(id) => ModProjectLicense::Spdx(id),
            FantomeLicense::Custom { name, url } => ModProjectLicense::Custom { name, url },
        }
    }
}

/// The `META/info.json` metadata a project packs to.
impl From<&ModProject> for FantomeInfo {
    fn from(mod_project: &ModProject) -> Self {
        Self {
            name: mod_project.display_name.clone(),
            author: format_authors(&mod_project.authors),
            version: mod_project.version.clone(),
            description: mod_project.description.clone(),
            license: mod_project.license.as_ref().map(FantomeLicense::from),
            tags: mod_project.tags.iter().map(|t| t.to_string()).collect(),
            champions: mod_project.champions.clone(),
            maps: mod_project.maps.iter().map(|m| m.to_string()).collect(),
            layers: build_fantome_layers(mod_project),
            // The pack owns the hashtable manifest: it computes the routes
            // once (fallibly - colliding file names are refused) and writes
            // what they declare, so the entries and the manifest cannot
            // disagree.
            hashtables: vec![],
            extra: Default::default(),
        }
    }
}

/// The project an imported archive's metadata describes.
///
/// The project name is the display name slugified, since Fantome carries no
/// separate machine name.
impl From<FantomeInfo> for ModProject {
    fn from(info: FantomeInfo) -> Self {
        Self {
            name: slug::slugify(&info.name),
            display_name: info.name,
            version: info.version,
            description: info.description,
            authors: vec![ModProjectAuthor::Name(info.author)],
            license: info.license.map(ModProjectLicense::from),
            tags: info.tags.into_iter().map(ModTag::from).collect(),
            champions: info.champions,
            maps: info.maps.into_iter().map(ModMap::from).collect(),
            transformers: vec![],
            layers: layers_from_fantome(info.layers),
            thumbnail: None,
            // The import owns the hashtable manifest: it computes the routes
            // once (fallibly - colliding file names are refused) and writes
            // the files where they declare, so the files and the manifest
            // cannot disagree.
            hashtables: vec![],
        }
    }
}

/// Where each of an archive's `Hashtables` declarations lands in the
/// project.
///
/// Each path is rewritten to where the importer writes the file: an archive
/// path under `META/hashes/` keeps its tail beneath `hashes/`, and a table
/// declared anywhere else lands under `hashes/` by its file name. Two
/// entries declaring one archive file keep declaring one project file.
///
/// An entry whose declared width no key can have is dropped rather than
/// carried: its table cannot be read out of the archive (see
/// `FantomeReader::read_hashtables`), so keeping the declaration would
/// declare a file the import never writes.
///
/// # Errors
///
/// [`DuplicateHashtableName`](crate::DuplicateHashtableName) when two
/// different declared files land on one project file name - writing both
/// would clobber one with the other, and an ambiguous archive must not be
/// guessed at.
pub(crate) fn project_routes(
    declared: &[FantomeHashtable],
) -> Result<Vec<HashtableRoute<ModProjectHashtable>>, crate::DuplicateHashtableName> {
    let mut claims = NameClaims::default();
    declared
        .iter()
        .filter(|manifest| manifest.to_entry().is_some())
        .map(|manifest| {
            let mapped = ModProjectHashtable {
                path: claims.claim(
                    HASHES_DIR_NAME,
                    meta_hashes_tail(&manifest.path)
                        .filter(|tail| is_plain_tail(tail))
                        .unwrap_or_else(|| file_name_of(&manifest.path)),
                    &manifest.path,
                )?,
                category: manifest.category.clone(),
                algorithm: manifest.algorithm.clone(),
                bits: manifest.bits,
            };
            Ok(HashtableRoute {
                source: manifest.path.clone(),
                manifest: mapped,
            })
        })
        .collect()
}

/// Where each planned table lands in the archive, paired with its table.
///
/// The mirror of [`project_routes`]: every planned table maps, verbatim
/// except for the path. A path under `hashes/` keeps its tail beneath
/// `META/hashes/`, so pack, import, pack again keeps every table where it
/// was; a table declared elsewhere in the project lands under `META/hashes/`
/// by its file name.
///
/// # Errors
///
/// [`DuplicateHashtableName`](crate::DuplicateHashtableName) when two
/// different declared files land on one archive name - refused rather than
/// renamed, since the author can rename a file and a silently renamed table
/// would ship under a name nobody chose.
pub(crate) fn fantome_routes(
    planned: &[crate::pack::PlannedHashtable],
) -> Result<Vec<PlannedRoute<'_, FantomeHashtable>>, crate::DuplicateHashtableName> {
    let mut claims = NameClaims::default();
    planned
        .iter()
        .map(|planned| {
            let source = planned.entry().path();
            let mapped = FantomeHashtable {
                path: claims.claim(
                    "META/hashes",
                    source
                        .strip_prefix(HASHES_DIR_NAME)
                        .ok()
                        .map(Utf8Path::as_str)
                        .filter(|tail| is_plain_tail(tail))
                        .unwrap_or_else(|| file_name_of(source.as_str())),
                    source.as_str(),
                )?,
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

/// The tail of an archive path under `META/hashes/`, in any casing, or `None`
/// for a path declared elsewhere in the archive.
fn meta_hashes_tail(archive_path: &str) -> Option<&str> {
    const PREFIX: &str = "META/hashes/";
    archive_path
        .get(..PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
        .map(|_| &archive_path[PREFIX.len()..])
        .filter(|tail| !tail.is_empty())
}

/// The layer table an archive's `META/info.json` declares.
///
/// Fantome stores content for the base layer alone, but it does carry the
/// other layers' names, priorities and string overrides, and nothing
/// downstream can recover an override the import dropped.
///
/// `info.layers` is a map, so [`ModProjectLayer::normalize_table`] orders the result rather
/// than leaving it to however the map iterated, and adds the base layer when
/// the archive names none.
fn layers_from_fantome(layers: HashMap<String, FantomeLayerInfo>) -> Vec<ModProjectLayer> {
    let mut layers: Vec<ModProjectLayer> = layers
        .into_values()
        .map(|layer| ModProjectLayer {
            name: layer.name,
            display_name: layer.display_name,
            priority: layer.priority,
            description: None,
            string_overrides: layer.string_overrides,
        })
        .collect();

    ModProjectLayer::normalize_table(&mut layers);

    layers
}

fn build_fantome_layers(mod_project: &ModProject) -> HashMap<String, FantomeLayerInfo> {
    let mut layers = HashMap::new();
    for layer in &mod_project.layers {
        // Only include layers that have string overrides
        if !layer.string_overrides.is_empty() {
            layers.insert(
                layer.name.clone(),
                FantomeLayerInfo {
                    name: layer.name.clone(),
                    display_name: layer.display_name.clone(),
                    priority: layer.priority,
                    string_overrides: layer.string_overrides.clone(),
                },
            );
        }
    }
    layers
}

fn format_authors(authors: &[ModProjectAuthor]) -> String {
    if authors.is_empty() {
        return "Unknown".to_string();
    }

    let author_names: Vec<String> = authors
        .iter()
        .map(|author| match author {
            ModProjectAuthor::Name(name) => name.clone(),
            ModProjectAuthor::Role { name, role: _ } => name.clone(),
        })
        .collect();

    author_names.join(", ")
}
