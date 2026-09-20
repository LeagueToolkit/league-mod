//! The class schema of the installed patch.
//!
//! A [`Schema`] answers the shape of a field on a class. The consumer implements it over the
//! schema it holds; [`NoSchema`] says nothing, every property is typed from the base, and an
//! authored class name is refused.

use std::sync::Arc;

use ltk_hash::BinHash;
use ltk_meta::{PropertyKind, PropertyValueEnum, path::ValueShape};

/// The class schema of the installed patch.
///
/// `expected` returning `None` is "the schema says nothing", never a mismatch; the base
/// value's shape is the fallback.
pub trait Schema {
    /// The shape of `field` on `class`.
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape>;

    /// Whether the schema knows `class`.
    fn has_class(&self, class: BinHash) -> bool;
}

impl<S: Schema + ?Sized> Schema for &S {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        (**self).expected(class, field)
    }

    fn has_class(&self, class: BinHash) -> bool {
        (**self).has_class(class)
    }
}

impl<S: Schema + ?Sized> Schema for Box<S> {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        (**self).expected(class, field)
    }

    fn has_class(&self, class: BinHash) -> bool {
        (**self).has_class(class)
    }
}

impl<S: Schema + ?Sized> Schema for Arc<S> {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        (**self).expected(class, field)
    }

    fn has_class(&self, class: BinHash) -> bool {
        (**self).has_class(class)
    }
}

/// A property type: a kind, and the kinds a container carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shape {
    /// The property's kind.
    pub kind: PropertyKind,
    /// A map's key kind.
    pub key: Option<PropertyKind>,
    /// A container's or an option's item kind; a map's value kind.
    pub item: Option<PropertyKind>,
}

impl Shape {
    /// The shape of a kind with no key and no item.
    #[must_use]
    pub const fn bare(kind: PropertyKind) -> Self {
        Self {
            kind,
            key: None,
            item: None,
        }
    }

    /// The shape of a value in a base tree.
    #[must_use]
    pub fn of(value: &PropertyValueEnum) -> Self {
        Self::from(ValueShape::of(value))
    }
}

impl From<ValueShape> for Shape {
    fn from(shape: ValueShape) -> Self {
        Self {
            kind: shape.kind,
            key: shape.key_kind,
            item: shape.item_kind,
        }
    }
}

/// The schema that says nothing. Every property is typed from the base, and no class is known.
///
/// `has_class` answers `false` for every class. A struct pin naming its own class is refused
/// with `UnknownClass`; one whose class comes from the base tree applies, the base being the
/// attestation ([ADR-0022]).
///
/// [ADR-0022]: https://github.com/LeagueToolkit/league-mod/blob/main/docs/adr/0022-unattested-class-refusal.md
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NoSchema;

impl Schema for NoSchema {
    fn expected(&self, _: BinHash, _: BinHash) -> Option<Shape> {
        None
    }

    fn has_class(&self, _: BinHash) -> bool {
        false
    }
}
