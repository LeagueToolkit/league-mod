//! The terms one bin object mentions.

use std::fmt;

use ltk_hash::BinHash;
use ltk_meta::BinObject;
use ltk_meta::property::Kind;
use ltk_meta::walk::{ChildSegment, Leaf, Node, TreeValue, Visit, Visitor};
use serde::{Deserialize, Serialize};

use super::NameTable;

/// What a hash term is, and the one kind a string term has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
#[repr(u8)]
pub enum TermKind {
    /// The declaration's own path hash.
    Object = 0,
    /// The class the declaration is declared as.
    Class = 1,
    /// The class of a struct or embedded value inside the declaration.
    Embedded = 2,
    /// A property name, at any depth.
    Field = 3,
    /// A `hash` value.
    Hash = 4,
    /// An object link value.
    Link = 5,
    /// A chunk link value.
    Wad = 6,
    /// A string value, ASCII-lowercased.
    String = 7,
}

impl TermKind {
    /// Every kind, in declaration order.
    pub const ALL: [TermKind; 8] = [
        TermKind::Object,
        TermKind::Class,
        TermKind::Embedded,
        TermKind::Field,
        TermKind::Hash,
        TermKind::Link,
        TermKind::Wad,
        TermKind::String,
    ];

    /// Every kind but [`TermKind::String`].
    pub const HASH_KINDS: [TermKind; 7] = [
        TermKind::Object,
        TermKind::Class,
        TermKind::Embedded,
        TermKind::Field,
        TermKind::Hash,
        TermKind::Link,
        TermKind::Wad,
    ];

    /// The name table a hash kind resolves through. `None` for [`TermKind::String`].
    #[must_use]
    pub const fn table(self) -> Option<NameTable> {
        match self {
            TermKind::Object | TermKind::Link => Some(NameTable::Entries),
            TermKind::Class | TermKind::Embedded => Some(NameTable::Types),
            TermKind::Field => Some(NameTable::Fields),
            TermKind::Hash => Some(NameTable::Hashes),
            TermKind::Wad => Some(NameTable::Chunks),
            TermKind::String => None,
        }
    }

    /// The lowercase name of the kind.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            TermKind::Object => "object",
            TermKind::Class => "class",
            TermKind::Embedded => "embedded",
            TermKind::Field => "field",
            TermKind::Hash => "hash",
            TermKind::Link => "link",
            TermKind::Wad => "wad",
            TermKind::String => "string",
        }
    }

    /// The position of the kind in [`TermKind::ALL`].
    pub(crate) const fn ordinal(self) -> usize {
        self as usize
    }
}

impl fmt::Display for TermKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of term kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TermKinds(u8);

impl TermKinds {
    /// Every kind.
    pub const ALL: TermKinds = TermKinds(u8::MAX);
    /// No kind.
    pub const NONE: TermKinds = TermKinds(0);

    /// The set holding exactly `kinds`.
    pub fn of(kinds: impl IntoIterator<Item = TermKind>) -> Self {
        kinds.into_iter().collect()
    }

    /// Whether the set holds `kind`.
    #[must_use]
    pub const fn contains(self, kind: TermKind) -> bool {
        self.0 & (1 << kind as u8) != 0
    }

    /// The set with `kind` added.
    #[must_use]
    pub const fn with(self, kind: TermKind) -> Self {
        Self(self.0 | (1 << kind as u8))
    }

    /// The set with `kind` removed.
    #[must_use]
    pub const fn without(self, kind: TermKind) -> Self {
        Self(self.0 & !(1 << kind as u8))
    }

    /// Every kind in the set, in [`TermKind::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = TermKind> {
        TermKind::ALL
            .into_iter()
            .filter(move |&kind| self.contains(kind))
    }

    /// Whether the set holds no kind.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// One bit per kind, at the kind's position in [`TermKind::ALL`].
    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    /// The set of the kinds whose bits are set.
    pub(crate) const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }
}

impl FromIterator<TermKind> for TermKinds {
    fn from_iter<I: IntoIterator<Item = TermKind>>(kinds: I) -> Self {
        kinds.into_iter().fold(Self::NONE, Self::with)
    }
}

impl From<TermKind> for TermKinds {
    fn from(kind: TermKind) -> Self {
        Self::NONE.with(kind)
    }
}

impl fmt::Debug for TermKinds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

/// Receives the terms of one object.
pub(crate) trait TermSink {
    /// A hash term.
    fn hash(&mut self, kind: TermKind, hash: u64);
    /// A string value as written.
    fn string(&mut self, value: &str);
}

/// Emits every term of each object a walk crosses into a [`TermSink`].
///
/// The visitor runs over the streaming view and over the owned tree alike. Over the view it
/// decodes a leaf only where the leaf can be a term: a container, optional or map is opened
/// only when its item or key kind is one of [`is_term_leaf`]'s, and a number is never read.
/// A node's properties are reached by the walk itself.
pub(crate) struct TermVisitor<'s, S> {
    pub sink: &'s mut S,
}

/// Whether a value of `kind` is a term leaf.
fn is_term_leaf(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::String | Kind::Hash | Kind::WadChunkLink | Kind::ObjectLink
    )
}

impl<S: TermSink> TermVisitor<'_, S> {
    /// Emits `value` when it is a term leaf.
    fn leaf<'a, V: TreeValue<'a>>(&mut self, value: V) -> Result<(), ltk_meta::Error> {
        match value.as_leaf()? {
            Some(Leaf::String(text)) => self.sink.string(text),
            Some(Leaf::Hash(hash)) => self.sink.hash(TermKind::Hash, hash.as_u64()),
            Some(Leaf::File(chunk)) => self.sink.hash(TermKind::Wad, chunk.0),
            Some(Leaf::Link(object)) => self.sink.hash(TermKind::Link, u64::from(object.0)),
            _ => {}
        }
        Ok(())
    }

    /// Emits every term leaf of `value` outside its nodes.
    fn leaves<'a, V: TreeValue<'a>>(&mut self, value: V) -> Result<(), ltk_meta::Error> {
        let kind = value.kind();
        if is_term_leaf(kind) {
            return self.leaf(value);
        }
        if !matches!(
            kind,
            Kind::Container | Kind::UnorderedContainer | Kind::Optional | Kind::Map
        ) {
            return Ok(());
        }
        let declared = value.declaration()?;
        let items = declared.item_kind.is_some_and(is_term_leaf);
        let keys = declared.key_kind.is_some_and(is_term_leaf);
        if !items && !keys {
            return Ok(());
        }
        for child in value.children()? {
            let (segment, item) = child?;
            if keys && let ChildSegment::Key(key) = segment {
                self.leaf(key)?;
            }
            if items {
                self.leaf(item)?;
            }
        }
        Ok(())
    }
}

impl<'a, V: TreeValue<'a>, S: TermSink> Visitor<'a, V> for TermVisitor<'_, S> {
    type Error = ltk_meta::Error;

    fn enter_node(&mut self, node: &Node<'_, 'a, V>) -> Result<Visit, Self::Error> {
        let class = u64::from(node.class_hash().0);
        if node.is_root() {
            self.sink
                .hash(TermKind::Object, u64::from(node.object_hash().0));
            self.sink.hash(TermKind::Class, class);
        } else {
            self.sink.hash(TermKind::Embedded, class);
        }
        Ok(Visit::Continue)
    }

    fn enter_property(
        &mut self,
        field: BinHash,
        value: V,
        _node: &Node<'_, 'a, V>,
    ) -> Result<Visit, Self::Error> {
        self.sink.hash(TermKind::Field, u64::from(field.0));
        self.leaves(value)?;
        Ok(Visit::Continue)
    }
}

/// Emits every term `object` holds, its own path and class included.
///
/// # Errors
///
/// None over the owned tree. The signature carries the walk's.
pub(crate) fn collect_terms(
    object: &BinObject,
    sink: &mut impl TermSink,
) -> Result<(), ltk_meta::Error> {
    object.walk(&mut TermVisitor { sink })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use ltk_hash::{HashValue, WadHash};
    use ltk_meta::PropertyValueEnum;
    use ltk_meta::property::values;

    use super::*;

    #[derive(Default)]
    struct Collected {
        hashes: Vec<(TermKind, u64)>,
        strings: Vec<String>,
    }

    impl TermSink for Collected {
        fn hash(&mut self, kind: TermKind, hash: u64) {
            self.hashes.push((kind, hash));
        }
        fn string(&mut self, value: &str) {
            self.strings.push(value.to_owned());
        }
    }

    #[test]
    fn every_leaf_kind_inside_every_container_is_a_term() {
        let inner = values::Struct {
            class_hash: BinHash(0xC1A5_0002),
            properties: [(
                BinHash(0xF1E1_0002),
                PropertyValueEnum::String(values::String {
                    value: "ASSETS/inner.skn".into(),
                }),
            )]
            .into_iter()
            .collect(),
        };
        let embedded = values::Embedded(values::Struct {
            class_hash: BinHash(0xC1A5_0003),
            properties: [(
                BinHash(0xF1E1_0008),
                PropertyValueEnum::U32(values::U32::new(7)),
            )]
            .into_iter()
            .collect(),
        });
        let mut list = values::Container::empty(Kind::Hash).unwrap();
        list.push(PropertyValueEnum::Hash(values::Hash {
            value: HashValue::narrow(0x4A5E_0001),
        }))
        .unwrap();
        let mut unordered = values::Container::empty(Kind::String).unwrap();
        unordered
            .push(PropertyValueEnum::String(values::String {
                value: "set".into(),
            }))
            .unwrap();
        let mut map = values::Map::empty(Kind::String, Kind::ObjectLink).unwrap();
        map.push(
            PropertyValueEnum::String(values::String {
                value: "key".into(),
            }),
            PropertyValueEnum::ObjectLink(values::ObjectLink {
                value: BinHash(0x0100_0002),
            }),
        )
        .unwrap();
        let mut option = values::Optional::empty(Kind::WadChunkLink).unwrap();
        option
            .set(Some(PropertyValueEnum::WadChunkLink(
                values::WadChunkLink {
                    value: WadHash(0xDEAD_BEEF_0000_0001),
                },
            )))
            .unwrap();

        let object = BinObject::builder(0x0100_0001u32, 0xC1A5_0001u32)
            .property(0xF1E1_0001u32, inner)
            .property(0xF1E1_0003u32, list)
            .property(0xF1E1_0004u32, map)
            .property(0xF1E1_0005u32, option)
            .property(0xF1E1_0006u32, values::F32::new(1.5))
            .property(0xF1E1_0007u32, embedded)
            .property(0xF1E1_0009u32, values::UnorderedContainer(unordered))
            .build();

        let mut got = Collected::default();
        collect_terms(&object, &mut got).unwrap();

        assert_eq!(got.strings, ["ASSETS/inner.skn", "key", "set"]);
        assert_eq!(
            got.hashes,
            [
                (TermKind::Object, 0x0100_0001),
                (TermKind::Class, 0xC1A5_0001),
                (TermKind::Field, 0xF1E1_0001),
                (TermKind::Embedded, 0xC1A5_0002),
                (TermKind::Field, 0xF1E1_0002),
                (TermKind::Field, 0xF1E1_0003),
                (TermKind::Hash, 0x4A5E_0001),
                (TermKind::Field, 0xF1E1_0004),
                (TermKind::Link, 0x0100_0002),
                (TermKind::Field, 0xF1E1_0005),
                (TermKind::Wad, 0xDEAD_BEEF_0000_0001),
                (TermKind::Field, 0xF1E1_0006),
                (TermKind::Field, 0xF1E1_0007),
                (TermKind::Embedded, 0xC1A5_0003),
                (TermKind::Field, 0xF1E1_0008),
                (TermKind::Field, 0xF1E1_0009),
            ]
        );
    }

    #[test]
    fn a_kind_set_holds_what_it_was_given() {
        let set = TermKinds::of([TermKind::Object, TermKind::String]);
        assert!(set.contains(TermKind::Object));
        assert!(set.contains(TermKind::String));
        assert!(!set.contains(TermKind::Class));
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [TermKind::Object, TermKind::String]
        );
        assert_eq!(
            set.without(TermKind::Object).without(TermKind::String),
            TermKinds::NONE
        );
        assert!(TermKinds::NONE.is_empty());
        assert_eq!(TermKinds::ALL.iter().count(), 8);
        assert_eq!(TermKinds::of(TermKind::ALL), TermKinds::ALL);
    }

    #[test]
    fn a_kind_displays_and_serializes_lowercase() {
        assert_eq!(TermKind::Embedded.to_string(), "embedded");
        for (index, kind) in TermKind::ALL.into_iter().enumerate() {
            assert_eq!(kind.ordinal(), index);
        }
        assert_eq!(TermKind::Wad.table(), Some(NameTable::Chunks));
        assert_eq!(TermKind::String.table(), None);
    }
}
