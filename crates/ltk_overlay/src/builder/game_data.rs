//! Declaration target selection, materialisation, and diagnostics.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use camino::Utf8PathBuf;
use ltk_game_data::{Module, Origin};
use ltk_wad::WadHash;
use serde::{Deserialize, Serialize};

use super::{OverlayBuilder, OverrideMeta, OverrideSource};
use crate::{error::Result, game_index::GameIndex, strings::read_game_chunk, utils::ContentHash};

/// A declaration diagnostic. The step index is zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDataReport {
    pub mod_id: String,
    pub layer: String,
    pub target: Option<String>,
    pub origin: Option<Origin>,
    pub step: Option<usize>,
    pub message: String,
}

struct Application {
    mod_id: String,
    layer: String,
    module: Module,
}

impl Application {
    fn report(&self, step: Option<usize>, message: impl ToString) -> GameDataReport {
        GameDataReport {
            mod_id: self.mod_id.clone(),
            layer: self.layer.clone(),
            target: Some(self.module.target.display_name().to_owned()),
            origin: Some(self.module.origin.clone()),
            step,
            message: message.to_string(),
        }
    }
}

impl OverlayBuilder {
    /// Declaration reports from the most recent build, including cached builds.
    pub fn game_data_reports(&self) -> &[GameDataReport] {
        &self.last_game_data_reports
    }

    pub(super) fn materialise_game_data(
        &mut self,
        game: &GameIndex,
        metadata: &mut HashMap<WadHash, OverrideMeta>,
    ) -> Result<()> {
        let mut targets: BTreeMap<WadHash, Vec<Application>> = BTreeMap::new();
        for enabled in self.enabled_mods.iter_mut().rev() {
            let mut layers = enabled.content.mod_project()?.layers;
            if !layers.iter().any(|layer| layer.is_base()) {
                layers.push(ltk_mod_project::ModProjectLayer::base());
            }
            layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));
            for layer in layers {
                if !enabled.is_layer_active(&layer.name) {
                    continue;
                }
                let result = enabled.content.game_data(&layer.name).and_then(|program| {
                    if let Some(program) = &program {
                        program.validate()?;
                    }
                    Ok(program)
                });
                match result {
                    Ok(Some(program)) => for module in program.modules {
                        let hash = WadHash::from(module.target.hash().expect("validated program"));
                        targets.entry(hash).or_default().push(Application { mod_id: enabled.id.clone(), layer: layer.name.clone(), module });
                    },
                    Ok(None) => {},
                    Err(error) => self.last_game_data_reports.push(GameDataReport {
                        mod_id: enabled.id.clone(), layer: layer.name, target: None, origin: None, step: None,
                        message: format!("Layer declarations refused: {error}; update the consumer for unsupported bindings"),
                    }),
                }
            }
        }

        let mut bases = HashMap::new();
        if !targets.is_empty() {
            for enabled in self.enabled_mods.iter_mut().rev() {
                for (hash, meta) in
                    super::metadata::collect_unfiltered_mod_metadata(enabled, game, &self.game_dir)?
                {
                    if targets.contains_key(&hash) {
                        bases.insert(hash, meta);
                    }
                }
            }
        }
        for (hash, applications) in targets {
            let original = bases.remove(&hash).or_else(|| metadata.get(&hash).cloned());
            let game_wad = game
                .find_wads_with_hash(hash)
                .and_then(|paths| paths.iter().min())
                .cloned();
            let base = self.read_declaration_base(hash, original.as_ref(), game_wad.as_ref());
            let mut bytes = match base {
                Ok(bytes) => bytes,
                Err(error) => {
                    for application in &applications {
                        self.last_game_data_reports
                            .push(application.report(None, &error));
                    }
                    continue;
                }
            };
            if game.subchunktoc_blocked().contains(&hash) {
                for application in &applications {
                    self.last_game_data_reports
                        .push(application.report(None, "target is a blocked game chunk"));
                }
                continue;
            }
            let mut dependencies = Vec::new();
            let mut applied = false;
            for application in &applications {
                match ltk_game_data::materialise(&bytes, &application.module.steps) {
                    Ok(output) => {
                        for report in output.reports {
                            self.last_game_data_reports.push(application.report(
                                Some(report.step),
                                format!("Link removal is absent: {}", report.path),
                            ));
                        }
                        bytes = output.bytes;
                        dependencies = output.dependencies;
                        applied = true;
                    }
                    Err(error) => self
                        .last_game_data_reports
                        .push(application.report(None, error)),
                }
            }
            if !applied {
                continue;
            }
            let owner = applications.last().expect("target has an application");
            metadata.insert(
                hash,
                OverrideMeta {
                    content_hash: ContentHash::of(&bytes),
                    uncompressed_size: bytes.len(),
                    source: OverrideSource::GameData {
                        mod_id: owner.mod_id.clone(),
                        chunk_path: Utf8PathBuf::from(owner.module.target.display_name()),
                        bytes: Arc::from(bytes),
                    },
                    fallback_wad: original
                        .as_ref()
                        .and_then(|value| value.fallback_wad.clone())
                        .or(game_wad),
                    unlocalized_wad: original.and_then(|value| value.unlocalized_wad),
                    linked_bins: dependencies,
                },
            );
        }
        Ok(())
    }

    fn read_declaration_base(
        &mut self,
        hash: WadHash,
        original: Option<&OverrideMeta>,
        game_wad: Option<&Utf8PathBuf>,
    ) -> std::result::Result<Vec<u8>, String> {
        if let Some(original) = original {
            let mod_id = original.source.mod_id();
            let enabled = self
                .enabled_mods
                .iter_mut()
                .find(|value| value.id == mod_id)
                .ok_or_else(|| "target has no ordinary mod base".to_owned())?;
            return match &original.source {
                OverrideSource::LayerWad {
                    layer,
                    wad_name,
                    rel_path,
                    ..
                } => enabled
                    .content
                    .read_wad_override_file(layer, wad_name, rel_path)
                    .map_err(|e| e.to_string()),
                OverrideSource::Raw { rel_path, .. } => enabled
                    .content
                    .read_raw_override_file(rel_path)
                    .map_err(|e| e.to_string()),
                _ => Err("target is reserved for another build operation".to_owned()),
            };
        }
        let wad = game_wad
            .ok_or_else(|| "target is absent from enabled content and the game index".to_owned())?;
        read_game_chunk(&self.game_dir, wad, hash).map_err(|e| e.to_string())
    }
}
