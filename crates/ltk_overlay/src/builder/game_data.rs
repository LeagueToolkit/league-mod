//! Declaration selector resolution, target application, and diagnostics.
//!
//! A `target` module binds to one chunk by hash. An `entries` module binds each entry to every
//! chunk declaring it, through the object index (`docs/design/game-data.md` section 6).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use camino::Utf8PathBuf;
use ltk_game_data::{
    ApplyDiagnosticKind, Edit, EntryEdit, EntryName, IndexMap, Module, Origin, OverridePath,
    Selector, SkippedProperty, SkippedRecord,
};
use ltk_game_index::{ArchiveId, BuildOptions, GameIndex, ObjectBuildError, ObjectIndex};
use ltk_mod_project::ModProjectLayer;
use ltk_wad::WadHash;
use serde::{Deserialize, Serialize};

use super::{OverlayBuilder, OverlayProgress, OverlayStage, OverrideMeta, OverrideSource};
use crate::{error::Result, game::GameIndexExt, utils::ContentHash};

/// The category of a declaration diagnostic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "camelCase")]
pub enum GameDataDiagnosticKind {
    DeclarationsRejected,
    TargetSkipped,
    /// An entry no game bin declares. Its edits are skipped.
    EntryUnresolved,
    /// An entry several game bins declare. Every one is edited. Informational.
    EntryFanOut,
    /// The object index did not load or build. Every `entries` module is skipped.
    IndexUnavailable,
    /// An override file the provider cannot supply. The file is skipped.
    OverrideUnreadable,
    /// An override file that is not a `PTCH`. The file is skipped.
    OverrideInvalid,
    /// One override record that does not apply. The remaining records apply.
    OverrideRecordSkipped,
    LinkRemovalUnmatched,
    /// One property key whose edit does not apply. The remaining keys apply.
    PropertyEditSkipped,
    /// A property typed from the base, the schema saying nothing. Informational.
    SchemaFallback,
    /// A missing or unrecognized serialized category.
    #[default]
    #[serde(other)]
    Unknown,
}

/// A declaration diagnostic. The edit index is zero-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDataDiagnostic {
    #[serde(default)]
    pub kind: GameDataDiagnosticKind,
    pub mod_id: String,
    pub layer: String,
    /// The authored target or entry name.
    pub target: Option<String>,
    /// The chunk the diagnostic is about.
    #[serde(default)]
    pub chunk: Option<WadHash>,
    pub origin: Option<Origin>,
    #[serde(rename = "edit")]
    pub edit_index: Option<usize>,
    /// The record of an `OverrideRecordSkipped` diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record: Option<SkippedRecord>,
    /// The property of a `PropertyEditSkipped` diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<SkippedProperty>,
    pub message: String,
}

impl From<ApplyDiagnosticKind> for GameDataDiagnosticKind {
    fn from(kind: ApplyDiagnosticKind) -> Self {
        match kind {
            ApplyDiagnosticKind::LinkRemovalUnmatched => Self::LinkRemovalUnmatched,
            ApplyDiagnosticKind::OverrideUnreadable => Self::OverrideUnreadable,
            ApplyDiagnosticKind::OverrideInvalid => Self::OverrideInvalid,
            ApplyDiagnosticKind::OverrideRecordSkipped => Self::OverrideRecordSkipped,
            ApplyDiagnosticKind::PropertyEditSkipped => Self::PropertyEditSkipped,
            ApplyDiagnosticKind::SchemaFallback => Self::SchemaFallback,
            _ => Self::Unknown,
        }
    }
}

impl GameDataDiagnosticKind {
    /// The human-readable statement of an application diagnostic about `path`.
    fn apply_message(self, path: &str) -> String {
        match self {
            Self::LinkRemovalUnmatched => format!("Link removal is absent: {path}"),
            Self::OverrideUnreadable => format!("Override file cannot be read: {path}"),
            Self::OverrideInvalid => format!("Override file is not a PTCH: {path}"),
            Self::OverrideRecordSkipped => format!("Override record is skipped: {path}"),
            Self::PropertyEditSkipped => format!("Property edit is skipped: {path}"),
            Self::SchemaFallback => format!("Property is typed from the base: {path}"),
            _ => format!("Application diagnostic: {path}"),
        }
    }
}

/// One module's edits bound to one chunk.
struct Application {
    mod_id: String,
    layer: String,
    /// The authored target or entry name, as diagnostics report it.
    target: String,
    /// The chunk as the override source names it: the authored target, or the hex hash of a
    /// declaring chunk.
    chunk_path: String,
    chunk: WadHash,
    edits: Vec<Edit>,
    origin: Origin,
}

/// A module of one enabled layer, in precedence order.
struct Pending {
    mod_id: String,
    layer: String,
    module: Module,
}

impl Pending {
    fn diagnostic(
        &self,
        kind: GameDataDiagnosticKind,
        target: Option<&EntryName>,
        message: impl ToString,
    ) -> GameDataDiagnostic {
        GameDataDiagnostic {
            kind,
            mod_id: self.mod_id.clone(),
            layer: self.layer.clone(),
            target: target.map(|name| name.as_str().to_owned()),
            chunk: None,
            origin: Some(self.module.origin.clone()),
            edit_index: None,
            record: None,
            property: None,
            message: message.to_string(),
        }
    }
}

/// The diagnostic of an `entries` module skipped for want of an object index.
fn index_unavailable(pending: &Pending, error: &ObjectBuildError) -> GameDataDiagnostic {
    pending.diagnostic(
        GameDataDiagnosticKind::IndexUnavailable,
        None,
        format!("Object index is unavailable: {error}; entries are skipped"),
    )
}

/// Lowers the entries of `pending` to one application per declaring chunk, in mapping order.
fn lower_entries(
    pending: &Pending,
    entries: &IndexMap<EntryName, EntryEdit>,
    index: &ObjectIndex,
    game: &GameIndex,
    targets: &mut BTreeMap<WadHash, Vec<Application>>,
    diagnostics: &mut Vec<GameDataDiagnostic>,
) {
    for (name, edit) in entries {
        // The index reports declarations in storage order and names one chunk more than once
        // for an entry several of its archives declare. Each distinct chunk is edited once; a
        // second application over the accumulating bytes lands every `+` edit twice.
        let chunks: IndexMap<WadHash, ArchiveId> = index
            .declarations(name.object_hash())
            .iter()
            .map(|declaration| (declaration.chunk, declaration.archive))
            .collect();
        match chunks.len() {
            0 => diagnostics.push(pending.diagnostic(
                GameDataDiagnosticKind::EntryUnresolved,
                Some(name),
                "No game bin declares the entry; edits are skipped",
            )),
            1 => {}
            count => {
                let named: Vec<String> = chunks
                    .iter()
                    .map(|(chunk, archive)| {
                        format!("{:016x} ({})", chunk.0, game.archive(*archive).name)
                    })
                    .collect();
                diagnostics.push(pending.diagnostic(
                    GameDataDiagnosticKind::EntryFanOut,
                    Some(name),
                    format!(
                        "Entry is declared in {count} chunks, each edited: {}",
                        named.join(", ")
                    ),
                ));
            }
        }
        for chunk in chunks.into_keys() {
            let mut chunk_edit = Edit::default();
            chunk_edit
                .entries
                .insert(name.clone(), edit.properties.clone());
            chunk_edit.links = edit.links.clone();
            targets.entry(chunk).or_default().push(Application {
                mod_id: pending.mod_id.clone(),
                layer: pending.layer.clone(),
                target: name.as_str().to_owned(),
                chunk_path: format!("{:016x}", chunk.0),
                chunk,
                edits: vec![chunk_edit],
                origin: pending.module.origin.clone(),
            });
        }
    }
}

impl Application {
    fn diagnostic(
        &self,
        kind: GameDataDiagnosticKind,
        edit_index: Option<usize>,
        message: impl ToString,
    ) -> GameDataDiagnostic {
        GameDataDiagnostic {
            kind,
            mod_id: self.mod_id.clone(),
            layer: self.layer.clone(),
            target: Some(self.target.clone()),
            chunk: Some(self.chunk),
            origin: Some(self.origin.clone()),
            edit_index,
            record: None,
            property: None,
            message: message.to_string(),
        }
    }

    /// The overlay diagnostic of one application diagnostic of this application.
    fn lower(&self, diagnostic: ltk_game_data::ApplyDiagnostic) -> GameDataDiagnostic {
        let kind = GameDataDiagnosticKind::from(diagnostic.kind);
        let mut lowered = self.diagnostic(
            kind,
            Some(diagnostic.edit_index),
            kind.apply_message(&diagnostic.path),
        );
        lowered.record = diagnostic.record;
        lowered.property = diagnostic.property;
        lowered
    }
}

/// The override files read during one build, shared across the applications naming them.
#[derive(Default)]
struct ResourceCache {
    bytes: HashMap<(String, String, String), Arc<[u8]>>,
}

impl ResourceCache {
    /// The bytes of `path` in the layer of `application`, read once per build.
    fn read(
        &mut self,
        enabled_mods: &mut [super::EnabledMod],
        application: &Application,
        path: &OverridePath,
    ) -> std::result::Result<Arc<[u8]>, ltk_game_data::Error> {
        let key = (
            application.mod_id.clone(),
            application.layer.clone(),
            path.as_str().to_owned(),
        );
        if let Some(bytes) = self.bytes.get(&key) {
            return Ok(Arc::clone(bytes));
        }
        let enabled = enabled_mods
            .iter_mut()
            .find(|enabled| enabled.id == application.mod_id)
            .ok_or_else(|| ltk_game_data::Error::io(path.as_str(), &"mod is not enabled"))?;
        let bytes: Arc<[u8]> = enabled
            .content
            .read_game_data_resource(&application.layer, path.as_str())
            .map_err(|error| {
                let kind = match &error {
                    crate::Error::ModContent(crate::ModContentError::GameDataResourceMissing {
                        ..
                    }) => ltk_game_data::ErrorKind::InputMissing,
                    other => ltk_game_data::ErrorKind::Io {
                        detail: other.to_string(),
                    },
                };
                ltk_game_data::Error::in_document(kind, path.as_str())
            })?
            .into();
        self.bytes.insert(key, Arc::clone(&bytes));
        Ok(bytes)
    }
}

impl OverlayBuilder {
    pub(super) fn apply_game_data(
        &mut self,
        game: &GameIndex,
        metadata: &mut HashMap<WadHash, OverrideMeta>,
    ) -> Result<()> {
        let mut pending: Vec<Pending> = Vec::new();
        for enabled in self.enabled_mods.iter_mut().rev() {
            let mut layers = enabled.content.mod_project()?.layers;
            if !layers.iter().any(|layer| layer.is_base()) {
                layers.push(ModProjectLayer::base());
            }
            layers.sort_by(ModProjectLayer::apply_order);
            for layer in layers {
                if !enabled.is_layer_active(&layer.name) {
                    continue;
                }
                let result = enabled
                    .content
                    .game_data_declarations(&layer.name)
                    .and_then(|declarations| {
                        if let Some(declarations) = &declarations {
                            declarations.validate()?;
                        }
                        Ok(declarations)
                    });
                match result {
                    Ok(Some(declarations)) => {
                        pending.extend(declarations.modules.into_iter().map(|module| Pending {
                            mod_id: enabled.id.clone(),
                            layer: layer.name.clone(),
                            module,
                        }));
                    }
                    Ok(None) => {}
                    Err(error) => self.last_game_data_diagnostics.push(GameDataDiagnostic {
                        kind: GameDataDiagnosticKind::DeclarationsRejected,
                        mod_id: enabled.id.clone(),
                        layer: layer.name,
                        target: None,
                        chunk: None,
                        origin: None,
                        edit_index: None,
                        record: None,
                        property: None,
                        message: format!(
                            "Layer declarations refused: {error}; update the consumer for unsupported bindings"
                        ),
                    }),
                }
            }
        }

        let object_index = pending
            .iter()
            .any(|pending| matches!(pending.module.selector, Selector::Entries(_)))
            .then(|| self.load_object_index(game));
        if matches!(object_index, Some(Err(_))) {
            self.check_called_off()?;
        }
        let mut targets: BTreeMap<WadHash, Vec<Application>> = BTreeMap::new();
        for pending in pending {
            match &pending.module.selector {
                Selector::Target { target, edits } => {
                    let hash = WadHash::from(target.chunk_hash());
                    targets.entry(hash).or_default().push(Application {
                        mod_id: pending.mod_id,
                        layer: pending.layer,
                        target: target.as_str().to_owned(),
                        chunk_path: target.as_str().to_owned(),
                        chunk: hash,
                        edits: edits.clone(),
                        origin: pending.module.origin,
                    });
                }
                Selector::Entries(entries) => match &object_index {
                    Some(Ok(index)) => lower_entries(
                        &pending,
                        entries,
                        index,
                        game,
                        &mut targets,
                        &mut self.last_game_data_diagnostics,
                    ),
                    Some(Err(error)) => self
                        .last_game_data_diagnostics
                        .push(index_unavailable(&pending, error)),
                    None => unreachable!("an entries module loads the object index"),
                },
                _ => unreachable!("the overlay lowers every selector of its `ltk_game_data`"),
            }
        }

        let mut bases = HashMap::new();
        if !targets.is_empty() {
            for enabled in self.enabled_mods.iter_mut().rev() {
                for (hash, meta) in super::metadata::collect_unfiltered_mod_metadata(enabled, game)?
                {
                    if targets.contains_key(&hash) {
                        bases.insert(hash, meta);
                    }
                }
            }
        }
        let subchunktoc_blocked = game.subchunktoc_blocked();
        let mut resources = ResourceCache::default();
        for (hash, applications) in targets {
            let original = bases.remove(&hash).or_else(|| metadata.get(&hash).cloned());
            let game_wad = game
                .row(hash)
                .map(|row| game.wad_rel_path(row.first_holder()));
            let base = self.read_declaration_base(hash, original.as_ref(), game_wad.as_ref());
            let mut bytes = match base {
                Ok(bytes) => bytes,
                Err(error) => {
                    for application in &applications {
                        self.last_game_data_diagnostics.push(application.diagnostic(
                            GameDataDiagnosticKind::TargetSkipped,
                            None,
                            &error,
                        ));
                    }
                    continue;
                }
            };
            if subchunktoc_blocked.contains(&hash) {
                for application in &applications {
                    self.last_game_data_diagnostics.push(application.diagnostic(
                        GameDataDiagnosticKind::TargetSkipped,
                        None,
                        "target is a blocked game chunk",
                    ));
                }
                continue;
            }
            let mut dependencies = Vec::new();
            let mut applied = false;
            for application in &applications {
                let enabled_mods = &mut self.enabled_mods;
                let read_override =
                    |path: &OverridePath| resources.read(enabled_mods, application, path);
                let schema = &self.game_data_schema;
                match ltk_game_data::apply(&bytes, &application.edits, read_override, schema) {
                    Ok(output) => {
                        self.last_game_data_diagnostics.extend(
                            output
                                .diagnostics
                                .into_iter()
                                .map(|diagnostic| application.lower(diagnostic)),
                        );
                        bytes = output.bytes;
                        dependencies = output.dependencies;
                        applied = true;
                    }
                    Err(error) => self.last_game_data_diagnostics.push(application.diagnostic(
                        GameDataDiagnosticKind::TargetSkipped,
                        None,
                        error,
                    )),
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
                        chunk_path: Utf8PathBuf::from(owner.chunk_path.as_str()),
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

    /// Loads the object index cached in the state directory, or builds it and writes the cache.
    ///
    /// Reported as [`OverlayStage::IndexingObjects`]. A failure is logged at warn.
    fn load_object_index(
        &self,
        game: &GameIndex,
    ) -> std::result::Result<ObjectIndex, ObjectBuildError> {
        self.emit_progress(OverlayProgress::stage(OverlayStage::IndexingObjects));
        let path = self.state_dir.object_index_cache();
        let called_off = || self.is_called_off();
        let mut options = BuildOptions::default();
        options.called_off = Some(&called_off);
        match ObjectIndex::load_or_build_with(game, &path, &options) {
            Ok(index) => {
                tracing::info!(
                    "Object index holds {} objects from {} bins",
                    index.len(),
                    index.stats().bins
                );
                Ok(index)
            }
            Err(error) => {
                tracing::warn!("Object index is unavailable: {error}; entries modules are skipped");
                Err(error)
            }
        }
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
        self.game_dir
            .read_chunk(wad, hash)
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::game_index_with_hashes;

    fn pending(entries: &[&str]) -> (Pending, IndexMap<EntryName, EntryEdit>) {
        let entries: IndexMap<EntryName, EntryEdit> = entries
            .iter()
            .map(|name| (EntryName::try_from(*name).unwrap(), EntryEdit::default()))
            .collect();
        let pending = Pending {
            mod_id: "mod".into(),
            layer: "base".into(),
            module: Module {
                selector: Selector::Entries(entries.clone()),
                origin: Origin {
                    manifest: "game_data.yaml".into(),
                    source: None,
                    module_index: 3,
                },
            },
        };
        (pending, entries)
    }

    #[test]
    fn an_unavailable_object_index_is_one_diagnostic_per_entries_module() {
        let (pending, _entries) = pending(&["Characters/Teemo/Skins/Skin0", "Characters/Ahri"]);
        let diagnostic = index_unavailable(&pending, &ObjectBuildError::CalledOff);
        assert_eq!(diagnostic.kind, GameDataDiagnosticKind::IndexUnavailable);
        assert_eq!(diagnostic.target, None);
        assert_eq!(diagnostic.mod_id, "mod");
        assert_eq!(diagnostic.origin.as_ref().unwrap().module_index, 3);
        assert_eq!(diagnostic.chunk, None);
        assert!(diagnostic.message.contains("called off"));
    }

    #[test]
    fn an_empty_object_index_leaves_every_entry_unresolved_in_mapping_order() {
        let (_fixture, game) =
            game_index_with_hashes(&[("DATA/FINAL/Champions/Aatrox.wad.client", &[WadHash(1)])]);
        let index = ObjectIndex::build(&game).unwrap();
        let (pending, entries) = pending(&["Characters/Zed", "Characters/Ahri"]);
        let mut targets = BTreeMap::new();
        let mut diagnostics = Vec::new();
        lower_entries(
            &pending,
            &entries,
            &index,
            &game,
            &mut targets,
            &mut diagnostics,
        );
        assert!(targets.is_empty());
        let targets: Vec<Option<&str>> = diagnostics.iter().map(|d| d.target.as_deref()).collect();
        assert_eq!(targets, [Some("Characters/Zed"), Some("Characters/Ahri")]);
        assert!(
            diagnostics
                .iter()
                .all(|d| d.kind == GameDataDiagnosticKind::EntryUnresolved)
        );
    }
}
