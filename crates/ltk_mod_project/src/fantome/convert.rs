//! Conversions between mod project types and the Fantome metadata shapes.

use std::collections::HashMap;

use ltk_fantome::{FantomeInfo, FantomeLayerInfo, FantomeLicense};

use crate::{default_layers, ModMap, ModProject, ModProjectAuthor, ModProjectLicense, ModTag};

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
pub fn fantome_info_from_project(mod_project: &ModProject) -> FantomeInfo {
    FantomeInfo {
        name: mod_project.display_name.clone(),
        author: format_authors(&mod_project.authors),
        version: mod_project.version.clone(),
        description: mod_project.description.clone(),
        license: mod_project.license.as_ref().map(FantomeLicense::from),
        tags: mod_project.tags.iter().map(|t| t.to_string()).collect(),
        champions: mod_project.champions.clone(),
        maps: mod_project.maps.iter().map(|m| m.to_string()).collect(),
        layers: build_fantome_layers(mod_project),
    }
}

/// The project an imported archive's metadata describes.
///
/// The project name is the display name slugified, since Fantome carries no
/// separate machine name; layers reset to the default base layer, the only
/// one the format stores content for.
pub fn project_from_fantome_info(info: FantomeInfo) -> ModProject {
    ModProject {
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
        layers: default_layers(),
        thumbnail: None,
    }
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
