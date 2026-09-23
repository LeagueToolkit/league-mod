//! A declaration property path as an address into a bin object.
//!
//! A declaration path is Riot's property path with one escape: a segment name spelled `0x` and
//! 8 hexadecimal digits is the field hash itself. The client hashes that spelling as text, and
//! `ltk_meta` resolves a `PropertyPath` the way the client does. A declaration path resolves
//! as a `ValuePath` instead, with each field by its hash and each key converted to the map's
//! key kind.

use ltk_meta::{
    BinObject, PropertyValueEnum as V,
    path::{MapKey, PropertyPath, ResolveErrorKind, Segment, Subscript, ValuePath, ValueSegment},
};

use crate::BinHash;

use super::hash32_of;

/// The hash of the field a segment names: the hash a hash-form name spells, else the hash of
/// the name.
pub(crate) fn field(segment: &Segment<'_>) -> BinHash {
    hash32_of(segment.name)
}

/// The `ValuePath` of `path` inside `object`.
///
/// A `{key}` literal converts to the key kind of the map the path reaches in `object`.
///
/// # Errors
///
/// The resolution failure of the prefix a `{key}` subscripts, `NotIndexable` where that prefix
/// is not a map, and `InvalidKey` where the literal does not convert.
pub(crate) fn value_path(
    object: &BinObject,
    path: &PropertyPath,
) -> Result<ValuePath, ResolveErrorKind> {
    let mut at = ValuePath::new();
    for segment in path.segments() {
        at.push(ValueSegment::Field(field(&segment)));
        match &segment.subscript {
            None => {}
            Some(Subscript::Index(index)) => {
                at.push_index(usize::try_from(*index).unwrap_or(usize::MAX));
            }
            Some(Subscript::Key(literal)) => {
                let key = match object.resolve_at(&at).map_err(|error| error.kind())? {
                    V::Map(map) => MapKey::from_literal(literal, map.key_kind())
                        .ok_or(ResolveErrorKind::InvalidKey(map.key_kind()))?,
                    other => return Err(ResolveErrorKind::NotIndexable(other.kind())),
                };
                at.push_key(key);
            }
        }
    }
    Ok(at)
}

/// The value at `path` inside `object`.
///
/// # Errors
///
/// The [`ResolveErrorKind`] of the segment that does not apply.
pub(crate) fn resolve<'a>(
    object: &'a BinObject,
    path: &PropertyPath,
) -> Result<&'a V, ResolveErrorKind> {
    let at = value_path(object, path)?;
    object.resolve_at(&at).map_err(|error| error.kind())
}
