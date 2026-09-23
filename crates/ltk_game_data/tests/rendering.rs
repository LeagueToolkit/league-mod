//! Rendering is the inverse of coercion: a rendered value, written as YAML and loaded back,
//! coerces to the value it came from.

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::Cursor,
};

use glam::{Mat4, Vec2, Vec3, Vec4};
use ltk_game_data::{
    ApplyDiagnosticKind, BinHash, ErrorKind, FieldNames, IndexMap, Names, NoSchema, OverridePath,
    PropertyKind as K, Schema, Selector, Shape, Value, apply, load_declarations, path_hash,
};
use ltk_hash::WadHash;
use ltk_meta::{Bin, BinObject, PropertyValueEnum as V, property::values};

const ENTRY: &str = "Characters/A";

fn h(name: &str) -> BinHash {
    BinHash::from(name)
}

/// The names of a test: fields by class, and every other hash by its text.
#[derive(Default)]
struct Table {
    fields: HashMap<(BinHash, BinHash), &'static str>,
    hashes: HashMap<BinHash, &'static str>,
    files: HashMap<u64, &'static str>,
}

impl Table {
    fn new() -> Self {
        let mut table = Self::default();
        for (class, field) in [
            ("E", "texture"),
            ("E", "scale"),
            ("E", "inner"),
            ("P", "count"),
        ] {
            table.fields.insert((h(class), h(field)), field);
        }
        for name in ["E", "P", "Characters/B", "tag"] {
            table.hashes.insert(h(name), name);
        }
        table
            .files
            .insert(path_hash("assets/a.tex"), "assets/a.tex");
        table
    }
}

impl FieldNames for Table {
    fn field(&self, field: BinHash, class: Option<BinHash>) -> Option<Cow<'_, str>> {
        self.fields.get(&(class?, field)).map(|name| (*name).into())
    }

    fn hash(&self, hash: BinHash) -> Option<Cow<'_, str>> {
        self.hashes.get(&hash).map(|name| (*name).into())
    }
}

impl Names for Table {
    fn class(&self, class: BinHash) -> Option<Cow<'_, str>> {
        self.hash(class)
    }

    fn entry(&self, entry: BinHash) -> Option<Cow<'_, str>> {
        self.hash(entry)
    }

    fn file(&self, chunk: u64) -> Option<Cow<'_, str>> {
        self.files.get(&chunk).map(|name| (*name).into())
    }
}

/// The schema of the two struct classes the tests render.
struct StructSchema;

impl Schema for StructSchema {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        let bare = |kind| Some(Shape::bare(kind));
        match (class, field) {
            (class, field) if class == h("E") && field == h("texture") => bare(K::String),
            (class, field) if class == h("E") && field == h("scale") => bare(K::F32),
            (class, field) if class == h("E") && field == h("inner") => bare(K::Struct),
            (class, field) if class == h("P") && field == h("count") => bare(K::U8),
            _ => None,
        }
    }

    fn has_class(&self, class: BinHash) -> bool {
        HashSet::from([h("E"), h("P")]).contains(&class)
    }
}

fn no_override(path: &OverridePath) -> Result<Vec<u8>, ltk_game_data::Error> {
    unreachable!("no override is read: {path}")
}

/// A manifest setting property `p` of the entry to `value`, written through `to_yaml`.
fn manifest(value: &Value) -> String {
    let yaml = value.to_yaml().unwrap();
    let inline = match value {
        Value::Mapping(_) => false,
        Value::List(items) => items
            .iter()
            .all(|item| !matches!(item, Value::List(_) | Value::Mapping(_))),
        _ => true,
    };
    // Every line after the first stands deeper than the key, a block scalar's included.
    let block = yaml.replace('\n', "\n          ");
    let set = if inline {
        format!("p: {block}")
    } else {
        format!("p:\n          {block}")
    };
    format!("version: 1\nmodules:\n  - entries:\n      {ENTRY}:\n        {set}\n")
}

/// `rendered`, written as YAML and loaded back.
fn reloaded(rendered: &Value) -> Value {
    let text = manifest(rendered);
    let declarations = load_declarations("game_data.yaml", &text, |_| unreachable!())
        .unwrap_or_else(|error| panic!("{error}\n{text}"));
    let Selector::Entries(entries) = &declarations.modules[0].selector else {
        panic!("expected an entries module");
    };
    entries.values().next().unwrap().properties[0].value.clone()
}

/// Renders `value`, writes it as YAML, loads it, and applies it over a base holding `seed`.
fn round_trip(value: &V, seed: &V, schema: &dyn Schema) -> V {
    let rendered = Value::render(value, &Table::new()).unwrap();
    let text = manifest(&rendered);
    let declarations = load_declarations("game_data.yaml", &text, |_| unreachable!())
        .unwrap_or_else(|error| panic!("{error}\n{text}"));
    let Selector::Entries(entries) = &declarations.modules[0].selector else {
        panic!("expected an entries module");
    };
    let mut edit = ltk_game_data::Edit::default();
    edit.entries.insert(
        entries.keys().next().unwrap().clone(),
        entries.values().next().unwrap().properties.clone(),
    );

    let object = BinObject::builder(h(ENTRY), h("C"))
        .property(h("p"), seed.clone())
        .build();
    let mut base = Cursor::new(Vec::new());
    Bin::builder()
        .object(object)
        .build()
        .to_writer(&mut base)
        .unwrap();

    let output = apply(
        &base.into_inner(),
        &[edit],
        no_override,
        |_| Ok(None),
        schema,
    )
    .unwrap();
    let skipped: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == ApplyDiagnosticKind::PropertyEditSkipped)
        .collect();
    assert!(skipped.is_empty(), "{skipped:?}\n{text}");
    assert_eq!(output.applied.properties, 1, "{text}");
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    bin.objects[&h(ENTRY)].properties[&h("p")].clone()
}

fn list(kind: K, items: Vec<V>) -> V {
    values::Container::new(kind, items).unwrap().into()
}

fn embed(texture: &str, scale: f32) -> values::Embedded {
    values::Embedded(values::Struct {
        class_hash: h("E"),
        properties: [
            (h("texture"), V::from(values::String::new(texture.into()))),
            (h("scale"), V::from(values::F32::new(scale))),
        ]
        .into(),
    })
}

/// A value of every kind the rendering table reads that needs no schema, each beside a seed
/// of the same shape and another value.
fn schemaless() -> Vec<(V, V)> {
    let matrix = Mat4::from_cols_array(&std::array::from_fn(|at| at as f32 * 0.1));
    vec![
        (
            values::Bool::new(true).into(),
            values::Bool::new(false).into(),
        ),
        (
            values::BitBool::new(true).into(),
            values::BitBool::new(false).into(),
        ),
        (values::I8::new(i8::MIN).into(), values::I8::new(0).into()),
        (values::U8::new(u8::MAX).into(), values::U8::new(0).into()),
        (
            values::I16::new(i16::MIN).into(),
            values::I16::new(0).into(),
        ),
        (
            values::U16::new(u16::MAX).into(),
            values::U16::new(0).into(),
        ),
        (
            values::I32::new(i32::MIN).into(),
            values::I32::new(0).into(),
        ),
        (
            values::U32::new(u32::MAX).into(),
            values::U32::new(0).into(),
        ),
        (
            values::I64::new(i64::MIN).into(),
            values::I64::new(0).into(),
        ),
        (
            values::U64::new(u64::MAX).into(),
            values::U64::new(0).into(),
        ),
        (values::F32::new(0.1).into(), values::F32::new(0.0).into()),
        (values::F32::new(2.0).into(), values::F32::new(0.0).into()),
        (
            values::F32::new(f32::MAX).into(),
            values::F32::new(0.0).into(),
        ),
        (
            values::F32::new(f32::MIN_POSITIVE).into(),
            values::F32::new(0.0).into(),
        ),
        (
            values::Vector2::new(Vec2::new(0.1, -2.5)).into(),
            values::Vector2::new(Vec2::ZERO).into(),
        ),
        (
            values::Vector3::new(Vec3::new(0.1, 0.2, 0.3)).into(),
            values::Vector3::new(Vec3::ZERO).into(),
        ),
        (
            values::Vector4::new(Vec4::new(1.0, 0.2, 0.3, 4.0)).into(),
            values::Vector4::new(Vec4::ZERO).into(),
        ),
        (
            values::Matrix44::new(matrix).into(),
            values::Matrix44::new(Mat4::IDENTITY).into(),
        ),
        (
            values::Color::new(ltk_primitives::Color::new(1, 2, 3, 255)).into(),
            values::Color::new(ltk_primitives::Color::new(0, 0, 0, 0)).into(),
        ),
        (
            values::String::new("true".into()).into(),
            values::String::new(String::new()).into(),
        ),
        (
            values::Hash::new(h("tag")).into(),
            values::Hash::new(h("x")).into(),
        ),
        (
            values::Hash::new(BinHash(0x71ad_094e)).into(),
            values::Hash::new(h("x")).into(),
        ),
        (
            values::ObjectLink::new(h("Characters/B")).into(),
            values::ObjectLink::new(h("x")).into(),
        ),
        (
            values::WadChunkLink::new(WadHash(path_hash("assets/a.tex"))).into(),
            values::WadChunkLink::new(WadHash(1)).into(),
        ),
        (
            values::WadChunkLink::new(WadHash(0x0123_4567_89ab_cdef)).into(),
            values::WadChunkLink::new(WadHash(1)).into(),
        ),
        (
            list(
                K::F32,
                vec![values::F32::new(0.1).into(), values::F32::new(3.0).into()],
            ),
            list(K::F32, vec![]),
        ),
        (
            list(
                K::Vector3,
                vec![values::Vector3::new(Vec3::new(1.0, 2.0, 3.0)).into()],
            ),
            list(K::Vector3, vec![]),
        ),
        (
            values::UnorderedContainer(
                values::Container::new(K::Hash, vec![values::Hash::new(h("tag")).into()]).unwrap(),
            )
            .into(),
            values::UnorderedContainer(values::Container::new(K::Hash, vec![]).unwrap()).into(),
        ),
        (
            values::Optional::new(K::U32, Some(values::U32::new(7).into()))
                .unwrap()
                .into(),
            values::Optional::empty(K::U32).unwrap().into(),
        ),
        (
            values::Optional::new(
                K::Vector3,
                Some(values::Vector3::new(Vec3::new(1.0, 2.0, 3.0)).into()),
            )
            .unwrap()
            .into(),
            values::Optional::empty(K::Vector3).unwrap().into(),
        ),
        (
            values::Optional::empty(K::F32).unwrap().into(),
            values::Optional::new(K::F32, Some(values::F32::new(1.0).into()))
                .unwrap()
                .into(),
        ),
        (
            values::Optional::new(K::Struct, Some(values::Struct::default().into()))
                .unwrap()
                .into(),
            values::Optional::empty(K::Struct).unwrap().into(),
        ),
        (
            values::Map::new(
                K::Hash,
                K::ObjectLink,
                vec![
                    (
                        values::Hash::new(h("tag")).into(),
                        values::ObjectLink::new(h("Characters/B")).into(),
                    ),
                    (
                        values::Hash::new(BinHash(1)).into(),
                        values::ObjectLink::new(BinHash(2)).into(),
                    ),
                ],
            )
            .unwrap()
            .into(),
            values::Map::new(K::Hash, K::ObjectLink, vec![])
                .unwrap()
                .into(),
        ),
        (
            values::Map::new(
                K::F32,
                K::String,
                vec![(
                    values::F32::new(0.1).into(),
                    values::String::new("1".into()).into(),
                )],
            )
            .unwrap()
            .into(),
            values::Map::new(K::F32, K::String, vec![]).unwrap().into(),
        ),
        (
            values::Map::new(
                K::Bool,
                K::U8,
                vec![(values::Bool::new(true).into(), values::U8::new(1).into())],
            )
            .unwrap()
            .into(),
            values::Map::new(K::Bool, K::U8, vec![]).unwrap().into(),
        ),
        (values::Struct::default().into(), {
            V::from(values::Struct {
                class_hash: h("P"),
                properties: IndexMap::new(),
            })
        }),
    ]
}

/// The struct values, which a schema types when they are read back.
fn structs() -> Vec<(V, V)> {
    let pointer = values::Struct {
        class_hash: h("P"),
        properties: [(h("count"), V::from(values::U8::new(3)))].into(),
    };
    let mut nested = embed("outer", 0.1);
    nested
        .0
        .properties
        .insert(h("inner"), pointer.clone().into());
    vec![
        (embed("t", 0.1).into(), embed("seed", 1.0).into()),
        (nested.into(), embed("seed", 1.0).into()),
        (pointer.clone().into(), values::Struct::default().into()),
        (
            list(
                K::Embedded,
                vec![embed("a", 1.0).into(), embed("b", 2.0).into()],
            ),
            list(K::Embedded, vec![]),
        ),
        (
            values::Optional::new(K::Struct, Some(pointer.into()))
                .unwrap()
                .into(),
            values::Optional::empty(K::Struct).unwrap().into(),
        ),
    ]
}

#[test]
fn a_rendered_value_coerces_back_under_no_schema() {
    for (value, seed) in schemaless() {
        assert_eq!(round_trip(&value, &seed, &NoSchema), value);
    }
}

#[test]
fn a_rendered_value_coerces_back_under_a_schema() {
    for (value, seed) in schemaless().into_iter().chain(structs()) {
        assert_eq!(round_trip(&value, &seed, &StructSchema), value);
    }
}

#[test]
fn a_single_renders_with_its_shortest_spelling() {
    let rendered = Value::render(&values::F32::new(0.1).into(), &()).unwrap();
    assert_eq!(rendered, Value::Float(0.1));
    assert_eq!(rendered.to_yaml().unwrap(), "0.1");
}

#[test]
fn an_option_renders_bare_or_as_a_one_element_list() {
    let render = |kind, content: Option<V>| {
        Value::render(&values::Optional::new(kind, content).unwrap().into(), &()).unwrap()
    };
    assert_eq!(render(K::F32, None), Value::Null);
    assert_eq!(
        render(K::U32, Some(values::U32::new(7).into())),
        Value::Integer(7)
    );
    assert_eq!(
        render(
            K::Vector2,
            Some(values::Vector2::new(Vec2::new(1.0, 2.0)).into())
        ),
        Value::List(vec![Value::List(vec![
            Value::Float(1.0),
            Value::Float(2.0)
        ])])
    );
}

#[test]
fn a_struct_renders_as_a_struct_pin_with_named_fields() {
    let rendered = Value::render(&embed("t", 0.5).into(), &Table::new()).unwrap();
    assert_eq!(
        rendered.to_yaml().unwrap(),
        "!embed(E)\ntexture: t\nscale: 0.5"
    );

    let empty = values::Struct {
        class_hash: BinHash(0xdead_beef),
        properties: IndexMap::new(),
    };
    assert_eq!(
        Value::render(&empty.into(), &Table::new())
            .unwrap()
            .to_yaml()
            .unwrap(),
        "!pointer(0xdeadbeef) {}"
    );
}

#[test]
fn a_class_a_tag_cannot_carry_stays_in_the_document_form() {
    let pin: Value =
        serde_json::from_str(r#"{"pointer": {"class": "A, B", "set": {"f": 1}}}"#).unwrap();
    assert_eq!(
        pin.to_yaml().unwrap(),
        "pointer:\n  class: A, B\n  set:\n    f: 1"
    );
    assert_eq!(reloaded(&pin), pin);
    let null: Value = serde_json::from_str(r#"{"pointer": null}"#).unwrap();
    assert_eq!(null.to_yaml().unwrap(), "!pointer null");
    assert_eq!(reloaded(&null), null);
}

#[test]
fn a_nameless_field_is_an_error_naming_its_path() {
    let mut value = embed("t", 0.5);
    value.0.properties.insert(
        h("inner"),
        values::Struct {
            class_hash: h("P"),
            properties: [(BinHash(0x1234_5678), V::from(values::U8::new(1)))].into(),
        }
        .into(),
    );
    let items = list(K::Embedded, vec![embed("a", 1.0).into(), value.into()]);

    let error = Value::render(&items, &Table::new()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NamelessField);
    assert_eq!(error.location.key.as_deref(), Some("[1].inner.0x12345678"));
}

#[test]
fn a_hash_renders_as_its_name_or_its_spelling() {
    let render = |value: V| Value::render(&value, &Table::new()).unwrap();
    let text = |text: &str| Value::String(text.to_owned());
    assert_eq!(render(values::Hash::new(h("tag")).into()), text("tag"));
    assert_eq!(
        render(values::Hash::new(BinHash(7)).into()),
        text("0x00000007")
    );
    assert_eq!(
        render(values::ObjectLink::new(h("Characters/B")).into()),
        text("Characters/B")
    );
    assert_eq!(
        render(values::ObjectLink::new(BinHash(7)).into()),
        text("0x00000007")
    );
    assert_eq!(
        render(values::WadChunkLink::new(WadHash(path_hash("assets/a.tex"))).into()),
        text("assets/a.tex")
    );
    assert_eq!(
        render(values::WadChunkLink::new(WadHash(7)).into()),
        text("0x0000000000000007")
    );
}

#[test]
fn a_name_that_does_not_hash_back_is_ignored() {
    let mut table = Table::new();
    table.hashes.insert(BinHash(7), "wrong");
    table.files.insert(7, "wrong.tex");
    table.fields.insert((h("P"), BinHash(9)), "wrong");

    let render = |value: V| Value::render(&value, &table);
    assert_eq!(
        render(values::Hash::new(BinHash(7)).into()).unwrap(),
        Value::String("0x00000007".into())
    );
    assert_eq!(
        render(values::WadChunkLink::new(WadHash(7)).into()).unwrap(),
        Value::String("0x0000000000000007".into())
    );
    let pointer = values::Struct {
        class_hash: h("P"),
        properties: [(BinHash(9), V::from(values::U8::new(1)))].into(),
    };
    assert_eq!(
        render(pointer.into()).unwrap_err().kind,
        ErrorKind::NamelessField
    );
}

#[test]
fn a_map_key_with_no_spelling_is_an_error_naming_its_path() {
    let vectors = values::Map::new(
        K::Vector2,
        K::U8,
        vec![(
            values::Vector2::new(Vec2::ZERO).into(),
            values::U8::new(1).into(),
        )],
    )
    .unwrap();
    let error = Value::render(&vectors.into(), &()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnrenderableKey);
    assert_eq!(error.location.key.as_deref(), Some("{vec2}"));

    let pin = values::Map::new(
        K::String,
        K::U8,
        vec![(
            values::String::new("u8".into()).into(),
            values::U8::new(1).into(),
        )],
    )
    .unwrap();
    let error = Value::render(&pin.into(), &()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnrenderableKey);
    assert_eq!(error.location.key.as_deref(), Some("{u8}"));
}

#[test]
fn yaml_text_reloads_to_the_same_value() {
    let text = |text: &str| Value::String(text.to_owned());
    let values = [
        text("0x71ad094e"),
        text("true"),
        text("1"),
        text("null"),
        text("1.5"),
        text(""),
        text("~"),
        text("a: b"),
        text("- a"),
        text("# not a comment"),
        text("line\nbreak"),
        text(" padded "),
        Value::Null,
        Value::Bool(false),
        Value::Integer(i128::from(u64::MAX)),
        Value::Integer(i128::from(i64::MIN)),
        Value::Float(1.0),
        Value::Float(1e30),
        Value::Float(-0.000_001),
        Value::List(vec![]),
        Value::List(vec![text("1"), Value::Integer(1), Value::Null]),
        Value::List(vec![
            Value::List(vec![Value::Float(1.0), Value::Float(2.0)]),
            Value::List(vec![]),
        ]),
        Value::Mapping(IndexMap::from([
            (
                "a".to_owned(),
                Value::Mapping(IndexMap::from([("b".to_owned(), text("no"))])),
            ),
            ("0x00000001".to_owned(), Value::List(vec![text("x")])),
            ("1".to_owned(), Value::Integer(2)),
        ])),
        Value::List(vec![
            Value::Mapping(IndexMap::from([
                ("a".to_owned(), Value::Integer(1)),
                ("b".to_owned(), Value::List(vec![Value::Integer(1)])),
            ])),
            Value::Mapping(IndexMap::from([("c".to_owned(), Value::Null)])),
        ]),
    ];
    for value in values {
        assert_eq!(reloaded(&value), value, "{}", value.to_yaml().unwrap());
    }
}

#[test]
fn a_list_of_scalars_writes_in_flow_style() {
    let value = Value::Mapping(IndexMap::from([
        (
            "pos".to_owned(),
            Value::List(vec![
                Value::Float(1.0),
                Value::Float(0.5),
                Value::Integer(2),
            ]),
        ),
        (
            "rows".to_owned(),
            Value::List(vec![Value::List(vec![Value::Integer(1)])]),
        ),
    ]));
    assert_eq!(value.to_yaml().unwrap(), "pos: [1.0, 0.5, 2]\nrows:\n- [1]");
}

#[test]
fn an_integer_past_the_ranges_does_not_write() {
    let error = Value::Integer(i128::MAX).to_yaml().unwrap_err();
    assert!(matches!(error.kind, ErrorKind::Serialize { .. }));
}
