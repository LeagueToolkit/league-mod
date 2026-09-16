use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
};

use ltk_game_data::{
    ApplyDiagnostic, ApplyDiagnosticKind, ApplyResult, BinHash, NoSchema, OverridePath,
    PropertyKind as K, PropertySkipReason as Reason, Schema, Selector, Shape, apply,
    load_declarations,
};
use ltk_meta::{
    PropertyValueEnum as V,
    concrete::{Bin, BinObject, values},
    path::PropertyPath,
    property::NoMeta,
    property::values::Embedded,
};

fn no_override(path: &OverridePath) -> Result<Vec<u8>, ltk_game_data::Error> {
    unreachable!("no override is read: {path}")
}

fn h(name: &str) -> BinHash {
    BinHash::from(name)
}

/// A field on a class, as the hand-written schema keys it.
fn field(class: &str, name: &str) -> (BinHash, BinHash) {
    (h(class), h(name))
}

/// A schema of two classes: `C`, the object's, and `E`, the embedded struct's.
struct TestSchema {
    fields: HashMap<(BinHash, BinHash), Shape>,
    classes: HashSet<BinHash>,
}

impl TestSchema {
    fn new() -> Self {
        let list = |item| Shape {
            kind: K::Container,
            key: None,
            item: Some(item),
        };
        let fields = HashMap::from([
            (field("C", "speed"), Shape::bare(K::F32)),
            (field("C", "count"), Shape::bare(K::U8)),
            (field("C", "name"), Shape::bare(K::String)),
            (field("C", "tags"), list(K::Hash)),
            (field("C", "units"), list(K::Embedded)),
            (
                field("C", "resources"),
                Shape {
                    kind: K::Map,
                    key: Some(K::Hash),
                    item: Some(K::ObjectLink),
                },
            ),
            (field("C", "mesh"), Shape::bare(K::Embedded)),
            (field("C", "ptr"), Shape::bare(K::Struct)),
            (
                field("C", "opt"),
                Shape {
                    kind: K::Optional,
                    key: None,
                    item: Some(K::U32),
                },
            ),
            (field("C", "color"), Shape::bare(K::Color)),
            (field("C", "pos"), Shape::bare(K::Vector3)),
            (field("C", "mat"), Shape::bare(K::Matrix44)),
            (field("C", "flag"), Shape::bare(K::BitBool)),
            (field("C", "chunk"), Shape::bare(K::WadChunkLink)),
            (field("C", "iconAvatar"), Shape::bare(K::WadChunkLink)),
            (field("C", "extras"), list(K::U16)),
            (field("C", "wide"), Shape::bare(K::I64)),
            (field("E", "texture"), Shape::bare(K::String)),
            (field("E", "scale"), Shape::bare(K::F32)),
        ]);
        Self {
            fields,
            classes: HashSet::from([h("C"), h("E")]),
        }
    }
}

impl Schema for TestSchema {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        self.fields.get(&(class, field)).copied()
    }

    fn has_class(&self, class: BinHash) -> bool {
        self.classes.contains(&class)
    }
}

fn embed(texture: &str) -> Embedded {
    Embedded(values::Struct {
        class_hash: h("E"),
        properties: [(h("texture"), V::from(values::String::new(texture.into())))].into(),
        meta: NoMeta,
    })
}

/// A PROP v3 with one object `Characters/A` of class `C`.
fn base_bin() -> Vec<u8> {
    let tags = values::Container::new(
        K::Hash,
        vec![
            values::Hash::new(h("a")).into(),
            values::Hash::new(h("b")).into(),
        ],
    )
    .unwrap();
    let units = values::Container::new(
        K::Embedded,
        vec![embed("u0").into(), embed("u1").into(), embed("u2").into()],
    )
    .unwrap();
    let resources = values::Map::new(
        K::Hash,
        K::ObjectLink,
        vec![(
            values::Hash::new(h("x")).into(),
            values::ObjectLink::new(h("X")).into(),
        )],
    )
    .unwrap();
    let object = BinObject::builder(h("Characters/A"), h("C"))
        .property(h("speed"), values::F32::new(1.0))
        .property(h("count"), values::U8::new(3))
        .property(h("name"), values::String::new("n".into()))
        .property(h("tags"), tags)
        .property(h("units"), units)
        .property(h("resources"), resources)
        .property(h("mesh"), embed("t"))
        .property(h("ptr"), values::Struct::default())
        .property(h("opt"), values::Optional::empty(K::U32).unwrap())
        .property(h("flag"), values::BitBool::new(false))
        .build();
    let bin = Bin::builder().object(object).build();
    let mut cursor = Cursor::new(Vec::new());
    bin.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

fn run(manifest: &str, schema: &dyn Schema) -> ApplyResult {
    let declarations = load_declarations("game_data.yaml", manifest, |_| unreachable!())
        .unwrap_or_else(|e| panic!("{e}"));
    let Selector::Target { edits, .. } = &declarations.modules[0].selector else {
        panic!("expected a target");
    };
    apply(&base_bin(), edits, no_override, schema).unwrap()
}

fn manifest(body: &str) -> String {
    let body = body
        .lines()
        .map(|line| format!("      {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n{body}\n")
}

fn value_at(output: &ApplyResult, path: &str) -> V {
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    bin.objects[&h("Characters/A")]
        .resolve(&PropertyPath::new(path).unwrap())
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .clone()
}

fn skips(output: &ApplyResult) -> Vec<(&str, Reason)> {
    output
        .diagnostics
        .iter()
        .filter(|d| d.kind == ApplyDiagnosticKind::PropertyEditSkipped)
        .map(|d| (d.path.as_str(), d.property.as_ref().unwrap().reason))
        .collect()
}

fn fallbacks(output: &ApplyResult) -> Vec<&str> {
    output
        .diagnostics
        .iter()
        .filter(|d| d.kind == ApplyDiagnosticKind::SchemaFallback)
        .map(|d| d.path.as_str())
        .collect()
}

#[test]
fn the_schema_types_absent_properties_and_the_base_types_the_rest() {
    let body = "iconAvatar: assets/a.tex\nspeed: 2.5\n";
    let output = run(&manifest(body), &TestSchema::new());
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        value_at(&output, "iconAvatar"),
        values::WadChunkLink::new(ltk_game_data::path_hash("assets/a.tex")).into()
    );
    assert_eq!(value_at(&output, "speed"), values::F32::new(2.5).into());
    assert_eq!(&output.bytes[..8], b"PROP\x03\0\0\0");

    let output = run(&manifest(body), &NoSchema);
    assert_eq!(skips(&output), [("iconAvatar", Reason::Untypable)]);
    assert_eq!(fallbacks(&output), ["speed"]);
    assert_eq!(value_at(&output, "speed"), values::F32::new(2.5).into());
    assert_eq!(output.diagnostics[0].edit_index, 0);
    assert_eq!(
        output.diagnostics[0]
            .property
            .as_ref()
            .unwrap()
            .entry
            .as_str(),
        "Characters/A"
    );
}

#[test]
fn every_coercion_row_passes_and_every_reason_fails() {
    let schema = TestSchema::new();
    let passing: &[(&str, &str, V)] = &[
        ("count: 12", "count", values::U8::new(12).into()),
        ("count: !u8 12", "count", values::U8::new(12).into()),
        ("wide: -5", "wide", values::I64::new(-5).into()),
        ("speed: 3", "speed", values::F32::new(3.0).into()),
        ("speed: 0.5", "speed", values::F32::new(0.5).into()),
        ("flag: true", "flag", values::BitBool::new(true).into()),
        ("flag: 1", "flag", values::BitBool::new(true).into()),
        (
            "name: hello",
            "name",
            values::String::new("hello".into()).into(),
        ),
        (
            "name: !string ''",
            "name",
            values::String::new(String::new()).into(),
        ),
        (
            "chunk: '0x00000000deadbeef'",
            "chunk",
            values::WadChunkLink::new(0xdead_beef_u64).into(),
        ),
        ("chunk: ''", "chunk", values::WadChunkLink::new(0u64).into()),
        (
            "chunk: null",
            "chunk",
            values::WadChunkLink::new(0u64).into(),
        ),
        (
            "pos: [1, 2.5, 3]",
            "pos",
            values::Vector3::new(glam::Vec3::new(1.0, 2.5, 3.0)).into(),
        ),
        (
            "mat: [1,0,0,0, 0,1,0,0, 0,0,1,0, 5,6,7,1]",
            "mat",
            values::Matrix44::new(
                glam::Mat4::from_cols_array(&[
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 6.0, 7.0, 1.0,
                ])
                .transpose(),
            )
            .into(),
        ),
        (
            "color: [255, 200, 60, 255]",
            "color",
            values::Color::new(ltk_primitives::Color::new(255, 200, 60, 255)).into(),
        ),
        (
            "tags: [c, '0x00000001']",
            "tags",
            values::Container::new(
                K::Hash,
                vec![
                    values::Hash::new(h("c")).into(),
                    values::Hash::new(BinHash(1)).into(),
                ],
            )
            .unwrap()
            .into(),
        ),
        (
            "opt: [7]",
            "opt",
            values::Optional::new(K::U32, Some(values::U32::new(7).into()))
                .unwrap()
                .into(),
        ),
        (
            "opt: !option 7",
            "opt",
            values::Optional::new(K::U32, Some(values::U32::new(7).into()))
                .unwrap()
                .into(),
        ),
        (
            "opt: null",
            "opt",
            values::Optional::empty(K::U32).unwrap().into(),
        ),
        (
            "resources: {y: Y, z: null}",
            "resources",
            values::Map::new(
                K::Hash,
                K::ObjectLink,
                vec![
                    (
                        values::Hash::new(h("y")).into(),
                        values::ObjectLink::new(h("Y")).into(),
                    ),
                    (
                        values::Hash::new(h("z")).into(),
                        values::ObjectLink::new(BinHash(0)).into(),
                    ),
                ],
            )
            .unwrap()
            .into(),
        ),
        ("ptr: null", "ptr", values::Struct::default().into()),
        (
            "ptr: !pointer {class: E, set: {texture: v, scale: 2}}",
            "ptr",
            values::Struct {
                class_hash: h("E"),
                properties: [
                    (h("texture"), V::from(values::String::new("v".into()))),
                    (h("scale"), V::from(values::F32::new(2.0))),
                ]
                .into(),
                meta: NoMeta,
            }
            .into(),
        ),
        (
            "mesh: !embed {set: {texture: w}}",
            "mesh",
            embed("w").into(),
        ),
        ("mesh: {texture: w}", "mesh", embed("w").into()),
        ("mesh.texture: w", "mesh", embed("w").into()),
        ("units[1].texture: w", "units[1]", embed("w").into()),
    ];
    for (body, path, expected) in passing {
        let output = run(&manifest(body), &schema);
        assert!(
            output.diagnostics.is_empty(),
            "{body}: {:?}",
            output.diagnostics
        );
        assert_eq!(&value_at(&output, path), expected, "{body}");
    }
    let failing: &[(&str, &str, Reason)] = &[
        ("count: 300", "count", Reason::OutOfRange),
        ("count: -1", "count", Reason::OutOfRange),
        ("count: 1.0", "count", Reason::KindMismatch),
        ("speed: 16777217", "speed", Reason::PrecisionLoss),
        ("speed: !u8 1", "speed", Reason::PinMismatch),
        ("speed: text", "speed", Reason::KindMismatch),
        ("flag: 2", "flag", Reason::OutOfRange),
        ("pos: [1, 2]", "pos", Reason::ArityMismatch),
        ("color: [1, 2, 3, 256]", "color", Reason::OutOfRange),
        ("color: '#fff'", "color", Reason::KindMismatch),
        ("tags: [1]", "tags", Reason::KindMismatch),
        ("tags: !string [a]", "tags", Reason::PinMismatch),
        ("opt: [1, 2]", "opt", Reason::ArityMismatch),
        ("+speed: [1]", "+speed", Reason::SignOnScalar),
        ("-tags: [zzz]", "-tags", Reason::RemovalUnmatched),
        ("-extras: [1]", "-extras", Reason::ContainerAbsent),
        ("ptr: {texture: v}", "ptr", Reason::NullPointer),
        ("ptr: !pointer {class: Nope}", "ptr", Reason::UnknownClass),
        (
            "ptr: !pointer {class: E, set: {nope: 1}}",
            "ptr",
            Reason::Untypable,
        ),
        (
            "ptr: !pointer {class: E, set: {'a.b': 1}}",
            "ptr",
            Reason::InvalidPath,
        ),
        ("mesh: !embed {class: C}", "mesh", Reason::PinMismatch),
        ("mesh: !pointer {class: E}", "mesh", Reason::PinMismatch),
        ("count.x: 1", "count.x", Reason::CannotDescend),
        ("tags[5]: a", "tags[5]", Reason::IndexOutOfRange),
        ("nope: 1", "nope", Reason::Untypable),
        ("mesh: {texture: [1]}", "mesh.texture", Reason::KindMismatch),
    ];
    for (body, path, reason) in failing {
        let output = run(&manifest(body), &schema);
        assert_eq!(skips(&output), [(*path, *reason)], "{body}");
    }
}

#[test]
fn signed_edits_replace_the_whole_container_from_the_base() {
    let schema = TestSchema::new();
    let hashes = |names: &[&str]| -> V {
        values::Container::new(
            K::Hash,
            names
                .iter()
                .map(|n| values::Hash::new(h(n)).into())
                .collect(),
        )
        .unwrap()
        .into()
    };
    let output = run(&manifest("+tags: [c]\n-tags: [a]\n"), &schema);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "tags"), hashes(&["b", "c"]));

    // The set applies first, then removals, then additions, whatever the key order.
    let output = run(
        &manifest("+tags: !hash [d]\n-tags: [x]\ntags: [x, y]\n"),
        &schema,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "tags"), hashes(&["y", "d"]));

    // An unmatched removal skips the whole key.
    let output = run(&manifest("+tags: [c]\n-tags: [zzz]\n"), &schema);
    assert_eq!(skips(&output), [("-tags", Reason::RemovalUnmatched)]);
    assert_eq!(value_at(&output, "tags"), hashes(&["a", "b"]));

    // `+` on a list the base omits creates it with the schema's item kind.
    let output = run(&manifest("+extras: [1, 2]\n"), &schema);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        value_at(&output, "extras"),
        values::Container::new(
            K::U16,
            vec![values::U16::new(1).into(), values::U16::new(2).into()]
        )
        .unwrap()
        .into()
    );
    let output = run(&manifest("+extras: [1, 2]\n"), &NoSchema);
    assert_eq!(skips(&output), [("+extras", Reason::Untypable)]);

    // Embeds are removed by index.
    let output = run(&manifest("-units: [2, 0]\n"), &schema);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        value_at(&output, "units"),
        values::Container::new(K::Embedded, vec![embed("u1").into()])
            .unwrap()
            .into()
    );
    let output = run(&manifest("-units: [3]\n"), &schema);
    assert_eq!(skips(&output), [("-units", Reason::RemovalUnmatched)]);

    // A map removes by key, then adds or replaces by key: `x` is removed and re-added.
    let output = run(
        &manifest("+resources: {x: X2, y: !link Y}\n-resources: [x]\n"),
        &schema,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        value_at(&output, "resources"),
        values::Map::new(
            K::Hash,
            K::ObjectLink,
            vec![
                (
                    values::Hash::new(h("x")).into(),
                    values::ObjectLink::new(h("X2")).into()
                ),
                (
                    values::Hash::new(h("y")).into(),
                    values::ObjectLink::new(h("Y")).into()
                ),
            ]
        )
        .unwrap()
        .into()
    );
    let output = run(&manifest("+resources: {x: X2}\n"), &schema);
    assert_eq!(
        value_at(&output, "resources"),
        values::Map::new(
            K::Hash,
            K::ObjectLink,
            vec![(
                values::Hash::new(h("x")).into(),
                values::ObjectLink::new(h("X2")).into()
            )]
        )
        .unwrap()
        .into()
    );
    let output = run(&manifest("-resources: [nope]\n"), &schema);
    assert_eq!(skips(&output), [("-resources", Reason::RemovalUnmatched)]);
}

#[test]
fn blocks_merge_field_by_field_and_report_joined_paths() {
    let schema = TestSchema::new();
    let output = run(
        &manifest("mesh:\n  texture: w\n  scale: 2\nunits[0]:\n  texture: z\n"),
        &schema,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let V::Embedded(Embedded(mesh)) = value_at(&output, "mesh") else {
        panic!("expected an embed");
    };
    assert_eq!(
        mesh.properties.keys().copied().collect::<Vec<_>>(),
        [h("texture"), h("scale")]
    );
    assert_eq!(
        value_at(&output, "units[0].texture"),
        values::String::new("z".into()).into()
    );

    // A sign on a block, an inner key that is not a path, and a scalar inside a block.
    let output = run(
        &manifest("+mesh: {texture: w}\nmesh: {'a[': 1, scale: x}\n"),
        &schema,
    );
    assert_eq!(
        skips(&output),
        [
            ("+mesh", Reason::SignOnScalar),
            ("mesh.a[", Reason::InvalidPath),
            ("mesh.scale", Reason::KindMismatch),
        ]
    );

    // A one-key mapping on a struct with a type name that is not `pointer` or `embed` descends.
    let output = run(&manifest("mesh: {string: 1}\n"), &schema);
    assert_eq!(skips(&output), [("mesh.string", Reason::Untypable)]);
}

#[test]
fn missing_objects_skip_every_edit_and_skipped_targets_keep_their_bytes() {
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/Missing:\n      a: 1\n      +b: [1]\n";
    let output = run(text, &TestSchema::new());
    assert_eq!(
        skips(&output),
        [("a", Reason::MissingObject), ("+b", Reason::MissingObject)]
    );
    assert_eq!(output.bytes, base_bin());

    // An `entries` module body applies the same way.
    let text = "version: 1\nmodules:\n  - entries:\n      Characters/A:\n        speed: 4\n        links: [x.bin]\n";
    let declarations = load_declarations("game_data.yaml", text, |_| unreachable!()).unwrap();
    let Selector::Entries(entries) = &declarations.modules[0].selector else {
        panic!("expected entries");
    };
    let entry = &entries[0];
    let mut edit = ltk_game_data::Edit::default();
    edit.entries.insert(
        entries.keys().next().unwrap().clone(),
        entry.properties.clone(),
    );
    edit.links = entry.links.clone();
    let output = apply(&base_bin(), &[edit], no_override, &TestSchema::new()).unwrap();
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "speed"), values::F32::new(4.0).into());
    assert_eq!(output.dependencies, ["x.bin"]);
}

#[test]
fn property_diagnostics_serialize_with_codes() {
    let output = run(&manifest("count: 300\n"), &TestSchema::new());
    let json = serde_json::to_string(&output.diagnostics).unwrap();
    assert!(
        json.contains(r#""kind":"propertyEditSkipped""#)
            && json.contains(r#""reason":"outOfRange""#),
        "{json}"
    );
    let back: Vec<ApplyDiagnostic> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, output.diagnostics);
    let unknown: ApplyDiagnostic = serde_json::from_str(
        r#"{"kind":"propertyEditSkipped","edit":0,"path":"x","property":{"entry":"Characters/A","reason":"later"}}"#,
    )
    .unwrap();
    assert_eq!(unknown.property.unwrap().reason, Reason::Unknown);
}
