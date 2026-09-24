//! The entry-edit phase: property edits lowered to one `Bin::patch` per property key.
//!
//! A block on a struct is flattened to leaf edits; leaves are grouped by path; each key's
//! set, removals, and additions produce one value ([ADR-0017]). Every skip is a report.
//!
//! [ADR-0017]: https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0017-per-key-patch-lowering.md

use indexmap::IndexMap;
use ltk_meta::{
    Bin, PropertyKind as K, PropertyValueEnum as V,
    path::{PropertyPath, Segment},
    property::values,
};

use crate::{BinHash, EntryName, PropertyEdit, Schema, Shape, Sign, Value};

use super::{
    ApplyDiagnosticKind, PropertySkipReason as Reason, SkippedProperty, address, coerce::Coercer,
};

/// One diagnostic of the phase, before its edit index is known.
#[derive(Debug)]
pub(super) struct Report {
    pub(super) kind: ApplyDiagnosticKind,
    pub(super) path: String,
    pub(super) property: Option<SkippedProperty>,
    pub(super) detail: Option<String>,
}

/// The phase's reports and how many property keys it set.
#[derive(Debug, Default)]
pub(super) struct Outcome {
    pub(super) reports: Vec<Report>,
    pub(super) properties: usize,
}

/// Runs the entry edits of one batch over `bin`.
///
/// The coercer carries the schema, so this phase reads it from there rather than taking it
/// twice. The resolved references reach coercion and nothing else, so they never appear here.
pub(super) fn run(
    bin: &mut Bin,
    coercer: Coercer<'_>,
    entries: &IndexMap<EntryName, Vec<PropertyEdit>>,
) -> Outcome {
    let mut phase = Phase {
        bin,
        schema: coercer.schema,
        coercer,
        outcome: Outcome::default(),
    };
    for (name, edits) in entries {
        phase.entry(name, edits);
    }
    phase.outcome
}

/// The property a path names, independent of how the path spells it.
///
/// A bin property name hashes ASCII case-insensitively, so `mFoo` and `MFoo` name one
/// property. Two spellings of one property share an identity and one [`Group`], whose single
/// `Bin::patch` carries every edit of that property.
///
/// A subscript is compared as written. An index is already canonical, a `u32` however it was
/// spelled. A `{key}` is the literal text: `{"Key"}` and `{"key"}` select one entry of a map
/// whose key kind is `hash` and two entries of one whose key kind is `string`, and the
/// property's kinds come from the schema, which this has no access to. Two spellings of one
/// map key are two groups, and the second `Bin::patch` of the pair wins.
fn identity(path: &PropertyPath) -> String {
    path.segments()
        .map(|segment| match &segment.subscript {
            Some(subscript) => format!("{:08x}{subscript}", address::field(&segment).0),
            None => format!("{:08x}", address::field(&segment).0),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// A property edit with block descent applied.
#[derive(Debug)]
struct Leaf {
    path: PropertyPath,
    sign: Sign,
    value: Value,
}

/// The leaf edits of one property key.
#[derive(Debug)]
struct Group {
    path: PropertyPath,
    set: Option<Value>,
    removals: Vec<Value>,
    additions: Vec<Value>,
}

impl Group {
    fn new(path: PropertyPath) -> Self {
        Self {
            path,
            set: None,
            removals: Vec::new(),
            additions: Vec::new(),
        }
    }

    fn push(&mut self, leaf: Leaf) {
        match leaf.sign {
            Sign::Set => self.set = Some(leaf.value),
            Sign::Add => self.additions.push(leaf.value),
            Sign::Remove => self.removals.push(leaf.value),
        }
    }

    /// The sign of the first operation, the one a resolution report names.
    fn first_sign(&self) -> Sign {
        if self.set.is_some() {
            Sign::Set
        } else if self.removals.is_empty() {
            Sign::Add
        } else {
            Sign::Remove
        }
    }
}

/// Where a property's shape comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Typing {
    /// The schema's answer for the field.
    Schema,
    /// The base value's shape, the schema saying nothing.
    Base,
    /// The schema's fallback shape, the schema saying nothing and the base omitting the field.
    Fallback,
}

/// Where a leaf edit lands: the property's shape and its base value.
struct Site {
    shape: Shape,
    base: Option<V>,
    typing: Typing,
}

struct Phase<'a> {
    bin: &'a mut Bin,
    schema: &'a dyn Schema,
    coercer: Coercer<'a>,
    outcome: Outcome,
}

impl Phase<'_> {
    fn skip(&mut self, entry: &EntryName, path: String, reason: Reason) {
        self.outcome.reports.push(Report {
            kind: ApplyDiagnosticKind::PropertyEditSkipped,
            path,
            property: Some(SkippedProperty {
                entry: entry.clone(),
                reason,
            }),
            detail: None,
        });
    }

    fn entry(&mut self, name: &EntryName, edits: &[PropertyEdit]) {
        let hash = name.object_hash();
        if !self.bin.objects.contains_key(&hash) {
            for edit in edits {
                self.skip(name, edit.key(), Reason::MissingObject);
            }
            return;
        }
        let mut leaves = Vec::new();
        for edit in edits {
            self.flatten(name, hash, None, edit, &mut leaves);
        }
        let mut groups: IndexMap<String, Group> = IndexMap::new();
        for leaf in leaves {
            groups
                .entry(identity(&leaf.path))
                .or_insert_with(|| Group::new(leaf.path.clone()))
                .push(leaf);
        }
        for group in groups.into_values() {
            match self.settle(hash, &group) {
                Ok(()) => self.outcome.properties += 1,
                Err((sign, reason)) => {
                    self.skip(name, format!("{}{}", sign.as_str(), group.path), reason);
                }
            }
        }
    }

    /// Appends the leaf edits of `edit` under `prefix`: the edit itself, or the edits of its
    /// block when the base at its path is a struct.
    fn flatten(
        &mut self,
        name: &EntryName,
        hash: BinHash,
        prefix: Option<&PropertyPath>,
        edit: &PropertyEdit,
        leaves: &mut Vec<Leaf>,
    ) {
        let path = match prefix {
            Some(prefix) => {
                PropertyPath::new(format!("{}.{}", prefix.as_str(), edit.path.as_str()))
                    .expect("two property paths join to a property path")
            }
            None => edit.path.clone(),
        };
        let key = || format!("{}{}", edit.sign.as_str(), path.as_str());
        // A reference is a one-key mapping, so it would descend as a block and have `ref`
        // read as a field name. It names a value instead, and coercion resolves it.
        if let Value::Mapping(block) = &edit.value
            && edit.value.pin().is_none()
            && edit.value.reference().is_none()
        {
            let base = address::resolve(&self.bin.objects[&hash], &path).ok();
            match base {
                Some(V::Struct(pointer)) if *pointer.class_hash == 0 => {
                    self.skip(name, key(), Reason::NullPointer);
                    return;
                }
                Some(V::Struct(_) | V::Embedded(_)) => {
                    if edit.sign != Sign::Set {
                        self.skip(name, key(), Reason::SignOnScalar);
                        return;
                    }
                    for (inner_key, value) in block {
                        match PropertyEdit::parse(inner_key, value.clone()) {
                            Ok(inner) => self.flatten(name, hash, Some(&path), &inner, leaves),
                            Err(_) => {
                                let joined = format!("{}.{inner_key}", path.as_str());
                                self.skip(name, joined, Reason::InvalidPath);
                            }
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        leaves.push(Leaf {
            path,
            sign: edit.sign,
            value: edit.value.clone(),
        });
    }

    /// Types the property at `path` of object `hash`.
    fn locate(&mut self, hash: BinHash, path: &PropertyPath) -> Result<Site, Reason> {
        let object = &self.bin.objects[&hash];
        let segments: Vec<Segment<'_>> = path.segments().collect();
        let (last, parents) = segments
            .split_last()
            .expect("a property path has a segment");
        let (class, properties) = if parents.is_empty() {
            (object.class_hash, &object.properties)
        } else {
            let parent = parents
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(".");
            let parent = PropertyPath::new(parent).expect("a prefix of a path is a path");
            match address::resolve(object, &parent) {
                Ok(V::Struct(pointer)) if *pointer.class_hash == 0 => {
                    return Err(Reason::NullPointer);
                }
                Ok(V::Struct(pointer)) => (pointer.class_hash, &pointer.properties),
                Ok(V::Embedded(values::Embedded(embed))) => (embed.class_hash, &embed.properties),
                Ok(_) => return Err(Reason::CannotDescend),
                Err(kind) => return Err(kind.into()),
            }
        };
        let field = address::field(last);
        if last.subscript.is_some() {
            let element = address::resolve(object, path)?;
            return Ok(Site {
                shape: Shape::of(element),
                base: Some(element.clone()),
                typing: Typing::Schema,
            });
        }
        let base = properties.get(&field);
        match self.schema.expected(class, field) {
            Some(shape) => Ok(Site {
                shape,
                base: base.cloned(),
                typing: Typing::Schema,
            }),
            None => match base {
                Some(value) => Ok(Site {
                    shape: Shape::of(value),
                    base: Some(value.clone()),
                    typing: Typing::Base,
                }),
                None => {
                    let shape = self
                        .schema
                        .fallback(class, field)
                        .ok_or(Reason::Untypable)?;
                    Ok(Site {
                        shape,
                        base: None,
                        typing: Typing::Fallback,
                    })
                }
            },
        }
    }

    /// Computes and sets the value of one property key.
    fn settle(&mut self, hash: BinHash, group: &Group) -> Result<(), (Sign, Reason)> {
        let site = self
            .locate(hash, &group.path)
            .map_err(|reason| (group.first_sign(), reason))?;
        if site.typing != Typing::Schema {
            self.report_fallback(&group.path);
        }

        self.coercer.fell_back.set(false);
        let written = self.write(hash, group, &site);
        if site.typing == Typing::Schema && self.coercer.fell_back.get() {
            self.report_fallback(&group.path);
        }
        written
    }

    /// Reports the property key at `path` as typed without the schema's answer.
    fn report_fallback(&mut self, path: &PropertyPath) {
        self.outcome.reports.push(Report {
            kind: ApplyDiagnosticKind::SchemaFallback,
            path: path.as_str().to_owned(),
            property: None,
            detail: None,
        });
    }

    /// Computes the value of one located property key and patches it into object `hash`.
    fn write(&mut self, hash: BinHash, group: &Group, site: &Site) -> Result<(), (Sign, Reason)> {
        let mut current = site.base.clone();
        if let Some(set) = &group.set {
            let value = self
                .coercer
                .coerce(set, site.shape, site.base.as_ref())
                .map_err(|reason| (Sign::Set, reason))?;
            current = Some(value);
        }
        if !group.removals.is_empty() || !group.additions.is_empty() {
            let mut contained = match Contained::of(current, site.shape) {
                Ok(Some(contained)) if contained.shape() != site.shape => {
                    return Err((group.first_sign(), Reason::TypeMismatch));
                }
                Ok(Some(contained)) => contained,
                Ok(None) if group.removals.is_empty() => {
                    Contained::empty(site.shape).map_err(|reason| (Sign::Add, reason))?
                }
                Ok(None) => return Err((Sign::Remove, Reason::ContainerAbsent)),
                Err(reason) => return Err((group.first_sign(), reason)),
            };
            for removal in &group.removals {
                contained
                    .remove(removal, self.coercer, site.shape)
                    .map_err(|reason| (Sign::Remove, reason))?;
            }
            for addition in &group.additions {
                contained
                    .add(addition, self.coercer, site.shape)
                    .map_err(|reason| (Sign::Add, reason))?;
            }
            current = Some(contained.into_value());
        }
        let value = current.expect("a group has a set, a removal, or an addition");
        let at = address::value_path(&self.bin.objects[&hash], &group.path)
            .map_err(|kind| (group.first_sign(), kind.into()))?;
        self.bin
            .patch_at(hash, &at, value)
            .map_err(|error| (group.first_sign(), Reason::from(&error)))?;
        Ok(())
    }
}

/// The elements of a list or the entries of a map, open for removals and additions.
enum Contained {
    List {
        unordered: bool,
        item: K,
        items: Vec<V>,
    },
    Map {
        key: K,
        item: K,
        entries: Vec<(V, V)>,
    },
}

impl Contained {
    /// The container `current` holds, or `None` for an absent property of container shape.
    fn of(current: Option<V>, shape: Shape) -> Result<Option<Self>, Reason> {
        if !matches!(shape.kind, K::Container | K::UnorderedContainer | K::Map) {
            return Err(Reason::SignOnScalar);
        }
        Ok(Some(match current {
            None => return Ok(None),
            Some(V::Container(list)) => Self::List {
                unordered: false,
                item: list.item_kind(),
                items: list.into_items(),
            },
            Some(V::UnorderedContainer(values::UnorderedContainer(list))) => Self::List {
                unordered: true,
                item: list.item_kind(),
                items: list.into_items(),
            },
            Some(V::Map(map)) => Self::Map {
                key: map.key_kind(),
                item: map.value_kind(),
                entries: map.into_entries(),
            },
            Some(_) => return Err(Reason::SignOnScalar),
        }))
    }

    /// The shape the container holds.
    fn shape(&self) -> Shape {
        match self {
            Self::List {
                unordered, item, ..
            } => Shape {
                kind: if *unordered {
                    K::UnorderedContainer
                } else {
                    K::Container
                },
                key: None,
                item: Some(*item),
            },
            Self::Map { key, item, .. } => Shape {
                kind: K::Map,
                key: Some(*key),
                item: Some(*item),
            },
        }
    }

    /// The empty container of `shape`, for `+` on a property the base omits.
    fn empty(shape: Shape) -> Result<Self, Reason> {
        let item = shape.item.ok_or(Reason::Untypable)?;
        Ok(match shape.kind {
            K::Map => Self::Map {
                key: shape.key.ok_or(Reason::Untypable)?,
                item,
                entries: Vec::new(),
            },
            kind => Self::List {
                unordered: kind == K::UnorderedContainer,
                item,
                items: Vec::new(),
            },
        })
    }

    /// Removes what `removal` names: elements by value or by index, entries by key.
    fn remove(
        &mut self,
        removal: &Value,
        coercer: Coercer<'_>,
        shape: Shape,
    ) -> Result<(), Reason> {
        match self {
            Self::List {
                item: K::Struct | K::Embedded,
                items,
                ..
            } => {
                let Value::List(indices) = removal else {
                    return Err(Reason::KindMismatch);
                };
                let mut indices = indices
                    .iter()
                    .map(|index| match index {
                        Value::Integer(index) => usize::try_from(*index)
                            .ok()
                            .filter(|index| *index < items.len())
                            .ok_or(Reason::RemovalUnmatched),
                        _ => Err(Reason::KindMismatch),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                indices.sort_unstable();
                indices.dedup();
                for index in indices.into_iter().rev() {
                    items.remove(index);
                }
                Ok(())
            }
            Self::List { items, .. } => {
                let removed = list_items(coercer.coerce(removal, shape, None)?);
                for element in removed {
                    let before = items.len();
                    items.retain(|candidate| *candidate != element);
                    if items.len() == before {
                        return Err(Reason::RemovalUnmatched);
                    }
                }
                Ok(())
            }
            Self::Map { key, entries, .. } => {
                let Value::List(keys) = removal else {
                    return Err(Reason::KindMismatch);
                };
                for spelled in keys {
                    let text = match spelled {
                        Value::String(text) => text.clone(),
                        Value::Integer(integer) => integer.to_string(),
                        Value::Bool(flag) => flag.to_string(),
                        _ => return Err(Reason::KindMismatch),
                    };
                    let wanted = coercer.key(&text, *key)?;
                    let before = entries.len();
                    entries.retain(|(candidate, _)| *candidate != wanted);
                    if entries.len() == before {
                        return Err(Reason::RemovalUnmatched);
                    }
                }
                Ok(())
            }
        }
    }

    /// Appends the elements of `addition`, or adds or replaces its entries by key.
    fn add(&mut self, addition: &Value, coercer: Coercer<'_>, shape: Shape) -> Result<(), Reason> {
        let added = coercer.coerce(addition, shape, None)?;
        match self {
            Self::List { items, .. } => {
                items.extend(list_items(added));
                Ok(())
            }
            Self::Map { entries, .. } => {
                let V::Map(added) = added else {
                    return Err(Reason::KindMismatch);
                };
                for (key, value) in added.into_entries() {
                    match entries.iter_mut().find(|(candidate, _)| *candidate == key) {
                        Some(entry) => entry.1 = value,
                        None => entries.push((key, value)),
                    }
                }
                Ok(())
            }
        }
    }

    fn into_value(self) -> V {
        match self {
            Self::List {
                unordered,
                item,
                items,
            } => {
                let list = values::Container::new(item, items)
                    .expect("every element was coerced to the item kind");
                if unordered {
                    values::UnorderedContainer(list).into()
                } else {
                    list.into()
                }
            }
            Self::Map { key, item, entries } => values::Map::new(key, item, entries)
                .expect("every key and value was coerced to its kind")
                .into(),
        }
    }
}

/// The elements of a coerced list value.
fn list_items(value: V) -> Vec<V> {
    match value {
        V::Container(list) => list.into_items(),
        V::UnorderedContainer(values::UnorderedContainer(list)) => list.into_items(),
        _ => Vec::new(),
    }
}
