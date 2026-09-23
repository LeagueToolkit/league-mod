//! The object phases of an edit: creation after the override files, removal after the entry
//! edits.

use std::collections::HashSet;

use indexmap::IndexMap;
use ltk_meta::{Bin, BinObject, PropertyValueEnum as V};

use crate::{BinHash, EntryName, ObjectEdit, PropertyEdit};

use super::{ObjectSkipReason, SkippedObject, coerce::Coercer, entries};

/// What the creation phase did: the objects it skipped, how many it created, and the outcome
/// of their `set` edits.
#[derive(Debug, Default)]
pub(super) struct Created {
    pub(super) skipped: Vec<SkippedObject>,
    pub(super) objects: usize,
    pub(super) sets: entries::Outcome,
}

/// Creates the cloned and constructed objects of one batch over `bin`.
///
/// Every clone reads `bin` as it is before the first object of the batch is created. The
/// objects are inserted in mapping order, then each one's `set` runs as the entry edits of
/// its name.
pub(super) fn create(
    bin: &mut Bin,
    coercer: Coercer<'_>,
    objects: &IndexMap<EntryName, ObjectEdit>,
) -> Created {
    let mut created = Created::default();
    let mut taken: HashSet<BinHash> = HashSet::new();
    let mut built = Vec::new();
    let mut sets: IndexMap<EntryName, Vec<PropertyEdit>> = IndexMap::new();
    for (name, edit) in objects {
        let skip = |reason| SkippedObject {
            name: name.clone(),
            reason,
        };
        let hash = name.object_hash();
        let object = match edit {
            ObjectEdit::Remove => continue,
            _ if bin.objects.contains_key(&hash) || taken.contains(&hash) => {
                Err(ObjectSkipReason::ObjectExists)
            }
            ObjectEdit::Clone { source, .. } => bin
                .objects
                .get(&source.object_hash())
                .map(|source_object| clone_as(source_object, source, name))
                .ok_or(ObjectSkipReason::SourceMissing),
            ObjectEdit::Construct { class, .. } => {
                let class_hash = class.class_hash();
                if coercer.schema.has_class(class_hash) {
                    Ok(BinObject::new(hash, class_hash))
                } else {
                    Err(ObjectSkipReason::UnknownClass)
                }
            }
        };
        match object {
            Ok(object) => {
                taken.insert(hash);
                built.push(object);
                if !edit.properties().is_empty() {
                    sets.insert(name.clone(), edit.properties().to_vec());
                }
            }
            Err(reason) => created.skipped.push(skip(reason)),
        }
    }
    created.objects = built.len();
    bin.objects
        .extend(built.into_iter().map(|object| (object.path_hash, object)));
    created.sets = entries::run(bin, coercer, &sets);
    created
}

/// A copy of `object`, the entry `source`, as the entry `name`.
///
/// A top-level `hash` property holding the hash of `source`, and a top-level `string`
/// property spelling the path of `source`, compared ASCII case-insensitively, name the object
/// itself. The copy holds the hash of `name` and the path of `name` there. A hash-form name
/// has no path, and a string property is left as it is where either name is hash-form.
fn clone_as(object: &BinObject, source: &EntryName, name: &EntryName) -> BinObject {
    let mut copy = object.clone();
    copy.path_hash = name.object_hash();
    let spelled = !source.is_hash() && !name.is_hash();
    for value in copy.properties.values_mut() {
        match value {
            V::Hash(hash) if hash.value == source.object_hash() => {
                hash.value = name.object_hash();
            }
            V::String(text) if spelled && text.value.eq_ignore_ascii_case(source.as_str()) => {
                name.as_str().clone_into(&mut text.value);
            }
            _ => {}
        }
    }
    copy
}

/// Removes the objects of one batch from `bin`, and returns those it did not hold with how
/// many it removed.
pub(super) fn remove(
    bin: &mut Bin,
    objects: &IndexMap<EntryName, ObjectEdit>,
) -> (Vec<SkippedObject>, usize) {
    let mut skipped = Vec::new();
    let mut removed = 0;
    for (name, edit) in objects {
        if !matches!(edit, ObjectEdit::Remove) {
            continue;
        }
        if bin.objects.shift_remove(&name.object_hash()).is_some() {
            removed += 1;
        } else {
            skipped.push(SkippedObject {
                name: name.clone(),
                reason: ObjectSkipReason::RemovalUnmatched,
            });
        }
    }
    (skipped, removed)
}
