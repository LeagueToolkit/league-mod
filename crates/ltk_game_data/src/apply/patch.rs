//! Application of edits over a `PTCH`: every edit lowered to the patch's own deletions, objects
//! and records.
//!
//! The edits run over a view: the objects the patch holds, and the game's copy of each other
//! object an edit names with the patch's records laid over it. An object the patch holds takes an
//! edit in place. Every other object takes one record per settled property key.

use std::{cell::Cell, collections::HashSet, io::Cursor};

use ltk_meta::{Bin, BinObject, BinOverride, PropertyPatch};

use crate::{BinHash, Edit, EntryName, Error, ObjectEdit, OverridePath, Schema};

use super::{
    Applied, ApplyDiagnostic, ApplyDiagnosticKind, ApplyResult, bin_error, coerce, entries,
    objects, read_override_file, report_objects, report_properties, resolve_references,
};

/// Applies ordered edits to a `PTCH`, per `docs/design/game-data.md` section 6.
pub(super) fn apply<B: AsRef<[u8]>>(
    base: &[u8],
    edits: &[Edit],
    mut read_override: impl FnMut(&OverridePath) -> Result<B, Error>,
    mut read_entry: impl FnMut(&EntryName) -> Result<Option<BinObject>, Error>,
    schema: &dyn Schema,
) -> Result<ApplyResult, Error> {
    let patch = BinOverride::from_reader(&mut Cursor::new(base)).map_err(|e| bin_error(&e))?;
    let mut diagnostics = Vec::new();
    let references = resolve_references(edits, &mut read_entry, &mut diagnostics);
    let fell_back = Cell::new(false);
    let coercer = coerce::Coercer {
        schema,
        references: &references,
        fell_back: &fell_back,
    };

    let mut view = View::new(patch);
    let mut applied = Applied::default();
    for (index, edit) in edits.iter().enumerate() {
        for path in &edit.overrides {
            if let Some(file) =
                read_override_file(path, &mut read_override, index, &mut diagnostics)
            {
                applied.records += file.patches.len();
                applied.objects += file.objects.len() + file.deleted.len();
                view.merge(file);
            }
        }

        view.load(edit, &mut read_entry, index, &mut diagnostics);

        let before = view.keys();
        let created = objects::create(&mut view.bin, coercer, &edit.objects);
        applied.objects += created.objects;
        report_objects(&mut diagnostics, index, created.skipped);
        view.own_new(&before);

        let (outcome, records) =
            entries::run_recording(&mut view.bin, coercer, &edit.entries, &view.owned);
        view.record(records);
        for outcome in [created.sets, outcome] {
            applied.properties += report_properties(&mut diagnostics, index, outcome);
        }

        let before = view.keys();
        let (skipped, removed) = objects::remove(&mut view.bin, &edit.objects);
        applied.objects += removed;
        report_objects(&mut diagnostics, index, skipped);
        view.drop_missing(&before);

        for path in edit.links.add.iter().chain(&edit.links.remove) {
            diagnostics.push(ApplyDiagnostic {
                kind: ApplyDiagnosticKind::LinkUnsupported,
                edit_index: index,
                path: path.as_str().to_owned(),
                record: None,
                property: None,
                object: None,
                detail: None,
            });
        }
    }

    if !(applied.records > 0 || applied.objects > 0 || applied.properties > 0) {
        return Ok(ApplyResult {
            bytes: base.to_vec(),
            dependencies: Vec::new(),
            applied,
            diagnostics,
        });
    }

    let mut cursor = Cursor::new(Vec::new());
    view.into_patch()
        .to_writer(&mut cursor)
        .map_err(|e| bin_error(&e))?;
    Ok(ApplyResult {
        bytes: cursor.into_inner(),
        dependencies: Vec::new(),
        applied,
        diagnostics,
    })
}

/// The objects a batch of edits reads over a `PTCH`, and the patch they lower to.
struct View {
    patch: BinOverride,
    /// The objects the patch holds, then the game's copy of each other object an edit names,
    /// with the patch's records laid over it.
    bin: Bin,
    /// The objects the patch holds.
    owned: HashSet<BinHash>,
    /// Every object read from the game or refused a read, which is never read again.
    asked: HashSet<BinHash>,
}

impl View {
    fn new(patch: BinOverride) -> Self {
        let mut bin = Bin::builder().build();
        bin.objects = patch.objects.clone();
        let owned = patch.objects.keys().copied().collect();
        Self {
            patch,
            bin,
            owned,
            asked: HashSet::new(),
        }
    }

    fn keys(&self) -> HashSet<BinHash> {
        self.bin.objects.keys().copied().collect()
    }

    /// Takes an override file's deletions, objects and records into the patch, and lays them
    /// over the view. A record over an object the view has not read applies when it is read.
    fn merge(&mut self, file: BinOverride) {
        self.owned.extend(file.objects.keys().copied());
        self.patch.deleted.extend(file.deleted.iter().copied());
        self.patch.patches.extend(file.patches.iter().cloned());
        file.apply(&mut self.bin);
    }

    /// Reads the game's copy of every object `edit` names that the view lacks, with the
    /// patch's records laid over it. A record the copy does not fit is left out, as the
    /// client leaves it out.
    fn load(
        &mut self,
        edit: &Edit,
        read_entry: &mut impl FnMut(&EntryName) -> Result<Option<BinObject>, Error>,
        index: usize,
        diagnostics: &mut Vec<ApplyDiagnostic>,
    ) {
        let sources = edit.objects.values().filter_map(|object| match object {
            ObjectEdit::Clone { source, .. } => Some(source),
            _ => None,
        });
        for name in edit
            .entries
            .keys()
            .chain(edit.objects.keys())
            .chain(sources)
        {
            let hash = name.object_hash();
            if self.bin.objects.contains_key(&hash)
                || self.patch.deleted.contains(&hash)
                || !self.asked.insert(hash)
            {
                continue;
            }

            match read_entry(name) {
                Ok(Some(mut object)) => {
                    for record in self.patch.patches.iter().filter(|r| r.object_hash == hash) {
                        let _ = object.patch(&record.path, record.value.clone());
                    }
                    self.bin.objects.insert(hash, object);
                }
                Ok(None) => {}
                Err(error) => diagnostics.push(ApplyDiagnostic {
                    kind: ApplyDiagnosticKind::EntryUnreadable,
                    edit_index: index,
                    path: name.as_str().to_owned(),
                    record: None,
                    property: None,
                    object: None,
                    detail: Some(error.to_string()),
                }),
            }
        }
    }

    /// Takes every object the view gained since `before` as the patch's own. A created object
    /// the patch deletes is no longer deleted.
    fn own_new(&mut self, before: &HashSet<BinHash>) {
        let created: Vec<BinHash> = self
            .bin
            .objects
            .keys()
            .filter(|hash| !before.contains(hash))
            .copied()
            .collect();
        for hash in created {
            self.owned.insert(hash);
            self.patch.deleted.retain(|deleted| *deleted != hash);
        }
    }

    /// Drops every object the view lost since `before`: from the patch's objects where the
    /// patch holds it, into its deletions where the game does. Its records go with it.
    fn drop_missing(&mut self, before: &HashSet<BinHash>) {
        for &hash in before {
            if self.bin.objects.contains_key(&hash) {
                continue;
            }
            if !self.owned.remove(&hash) {
                self.patch.deleted.push(hash);
            }
            self.patch
                .patches
                .retain(|record| record.object_hash != hash);
        }
    }

    /// Appends each record, dropping every held record of its object at its path or under it.
    /// The new value was settled over the view, which those records had already shaped.
    fn record(&mut self, records: Vec<PropertyPatch>) {
        for record in records {
            let identity = entries::identity(&record.path);
            self.patch.patches.retain(|held| {
                held.object_hash != record.object_hash
                    || !covers(&identity, &entries::identity(&held.path))
            });
            self.patch.patches.push(record);
        }
    }

    fn into_patch(self) -> BinOverride {
        let Self {
            mut patch,
            bin,
            owned,
            ..
        } = self;
        patch.objects = bin
            .objects
            .into_iter()
            .filter(|(hash, _)| owned.contains(hash))
            .collect();
        let mut seen = HashSet::new();
        patch.deleted.retain(|hash| seen.insert(*hash));
        patch
    }
}

/// Whether the property at identity `held` is the one at `path` or lies under it.
fn covers(path: &str, held: &str) -> bool {
    held.strip_prefix(path)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '[', '{']))
}
