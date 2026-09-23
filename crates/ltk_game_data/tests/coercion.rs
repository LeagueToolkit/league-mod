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
    Bin, BinObject, PropertyValueEnum as V,
    path::PropertyPath,
    property::values,
    property::values::{Embedded, UnorderedContainer},
};

fn no_override(path: &OverridePath) -> Result<Vec<u8>, ltk_game_data::Error> {
    unreachable!("no override is read: {path}")
}

/// The caller with no game: every reference reports `ReferenceMissingEntry`.
fn no_entry(
    _: &ltk_game_data::EntryName,
) -> Result<Option<ltk_meta::BinObject>, ltk_game_data::Error> {
    Ok(None)
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
            (
                field("C", "optptr"),
                Shape {
                    kind: K::Optional,
                    key: None,
                    item: Some(K::Struct),
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
            (field("C", "id"), Shape::bare(K::Hash)),
            (field("C", "mesh2"), Shape::bare(K::Embedded)),
            (
                field("C", "tags2"),
                Shape {
                    kind: K::UnorderedContainer,
                    key: None,
                    item: Some(K::Hash),
                },
            ),
            (field("C", "names"), list(K::Hash)),
            (field("E", "texture"), Shape::bare(K::String)),
            (field("E", "scale"), Shape::bare(K::F32)),
            // A Riot field named `ref`. The dotted form is the only way to reach it.
            (field("E", "ref"), Shape::bare(K::U32)),
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
        .property(
            h("tags2"),
            UnorderedContainer(
                values::Container::new(K::Hash, vec![values::Hash::new(h("a")).into()]).unwrap(),
            ),
        )
        .property(
            h("names"),
            values::Container::new(K::String, vec![values::String::new("n".into()).into()])
                .unwrap(),
        )
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
    apply(&base_bin(), edits, no_override, no_entry, schema).unwrap()
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

/// The hash of a container of `K::Hash` items.
fn hashes(names: &[&str]) -> V {
    values::Container::new(
        K::Hash,
        names
            .iter()
            .map(|name| values::Hash::new(h(name)).into())
            .collect(),
    )
    .unwrap()
    .into()
}

#[test]
fn two_spellings_of_one_property_reach_the_same_group() {
    // A bin property name hashes without case, so both keys name `tags`.
    let output = run(&manifest("+tags: [c]\n+TAGS: [d]\n"), &TestSchema::new());
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "tags"), hashes(&["a", "b", "c", "d"]));

    // A set under one spelling and an addition under another settle together.
    let output = run(&manifest("TAGS: [x]\n+tags: [y]\n"), &TestSchema::new());
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "tags"), hashes(&["x", "y"]));
}

/// A schema that types every field of [`TestSchema`] and knows no class name.
struct ShapesOnly(TestSchema);

impl Schema for ShapesOnly {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        self.0.expected(class, field)
    }

    fn has_class(&self, _: BinHash) -> bool {
        false
    }
}

#[test]
fn a_class_the_base_carries_needs_no_schema_however_the_pin_spells_it() {
    // `mesh` is an embedded `E` in the base. Naming `E` and leaving it out are one pin.
    for body in [
        "mesh: !embed { texture: z }\n",
        "mesh: !embed(E) { texture: z }\n",
    ] {
        let output = run(&manifest(body), &ShapesOnly(TestSchema::new()));
        assert!(
            output.diagnostics.is_empty(),
            "{body}: {:?}",
            output.diagnostics
        );
        assert_eq!(
            value_at(&output, "mesh.texture"),
            values::String::new("z".into()).into(),
            "{body}"
        );
    }
}

#[test]
fn an_authored_class_needs_the_schema_and_a_base_class_does_not() {
    // `ptr` is a null pointer in the base, so its class can only be the authored one.
    let typo = manifest("ptr: !pointer(Typo)\n");
    assert_eq!(
        skips(&run(&typo, &TestSchema::new())),
        [("ptr", Reason::UnknownClass)]
    );
    assert_eq!(
        skips(&run(&typo, &NoSchema)),
        [("ptr", Reason::UnknownClass)]
    );

    // The same schema refuses a class no base value carries.
    assert_eq!(
        skips(&run(
            &manifest("ptr: !pointer(E)\n"),
            &ShapesOnly(TestSchema::new())
        )),
        [("ptr", Reason::UnknownClass)]
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
        (
            "optptr: !pointer(E) {texture: t}",
            "optptr",
            values::Optional::new(K::Struct, Some(embed("t").0.into()))
                .unwrap()
                .into(),
        ),
        ("ptr: null", "ptr", values::Struct::default().into()),
        (
            "ptr: !pointer null",
            "ptr",
            values::Struct::default().into(),
        ),
        (
            "opt: !option {}",
            "opt",
            values::Optional::empty(K::U32).unwrap().into(),
        ),
        ("id: Foo", "id", values::Hash::new(h("Foo")).into()),
        (
            "tags2: [b]",
            "tags2",
            UnorderedContainer(
                values::Container::new(K::Hash, vec![values::Hash::new(h("b")).into()]).unwrap(),
            )
            .into(),
        ),
        (
            "resources: {q: !link null}",
            "resources",
            values::Map::new(
                K::Hash,
                K::ObjectLink,
                vec![(
                    values::Hash::new(h("q")).into(),
                    values::ObjectLink::new(BinHash(0)).into(),
                )],
            )
            .unwrap()
            .into(),
        ),
        (
            "ptr: !pointer(E) {texture: v, scale: 2}",
            "ptr",
            values::Struct {
                class_hash: h("E"),
                properties: [
                    (h("texture"), V::from(values::String::new("v".into()))),
                    (h("scale"), V::from(values::F32::new(2.0))),
                ]
                .into(),
            }
            .into(),
        ),
        ("mesh: !embed {texture: w}", "mesh", embed("w").into()),
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
        ("ptr: {}", "ptr", Reason::NullPointer),
        ("opt: {}", "opt", Reason::KindMismatch),
        ("mesh2: {texture: q}", "mesh2", Reason::KindMismatch),
        ("mesh2: !string q", "mesh2", Reason::PinMismatch),
        ("+names: [x]", "+names", Reason::TypeMismatch),
        ("-names: [n]", "-names", Reason::TypeMismatch),
        ("ptr: !pointer(Nope)", "ptr", Reason::UnknownClass),
        ("ptr: !pointer(E) {nope: 1}", "ptr", Reason::Untypable),
        ("ptr: !pointer(E) {'a.b': 1}", "ptr", Reason::InvalidPath),
        ("mesh: !embed(C)", "mesh", Reason::PinMismatch),
        ("mesh: !hash x", "mesh", Reason::PinMismatch),
        ("mesh: {hash: x}", "mesh", Reason::PinMismatch),
        ("mesh: {string: x}", "mesh", Reason::PinMismatch),
        (
            "ptr: !pointer(E) {texture: {hash: x}}",
            "ptr",
            Reason::PinMismatch,
        ),
        ("mesh: !pointer(E)", "mesh", Reason::PinMismatch),
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

    // A one-key mapping keyed by a type name is a pin on a struct too; the dotted path reaches
    // a field with a type's name.
    let output = run(&manifest("mesh: {string: 1}\n"), &schema);
    assert_eq!(skips(&output), [("mesh", Reason::PinMismatch)]);
    let output = run(&manifest("mesh.string: 1\n"), &schema);
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
    let output = apply(
        &base_bin(),
        &[edit],
        no_override,
        no_entry,
        &TestSchema::new(),
    )
    .unwrap();
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

/// A list of skips does not say whether anything landed, so the result counts what did. A
/// caller that writes a result with nothing counted writes the base.
#[test]
fn the_result_says_whether_any_edit_landed() {
    // The object is not in the base, so every edit skips.
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/Missing:\n      a: 1\n";
    let output = run(text, &TestSchema::new());
    assert!(!output.changed(), "{:?}", output.applied);
    assert_eq!(output.applied, ltk_game_data::Applied::default());
    assert_eq!(output.bytes, base_bin());

    // One property set.
    let output = run(&manifest("speed: 4\n"), &TestSchema::new());
    assert!(output.changed());
    assert_eq!(output.applied.properties, 1);
    assert_eq!(output.applied.records, 0);

    // A link edit is a change, though it never reaches the object tree.
    let text = "version: 1\nmodules:\n  - target: a.bin\n    links: [x.bin]\n";
    let output = run(text, &TestSchema::new());
    assert!(output.changed());
    assert_eq!(output.applied.links_added, 1);
    assert_eq!(output.applied.properties, 0);

    // An edit list with no edits at all is not a change.
    let output = apply(&base_bin(), &[], no_override, no_entry, &TestSchema::new()).unwrap();
    assert!(!output.changed());
}

/// An override file whose every record skipped reached nothing, so it does not cost the
/// re-encode that writes PROP v3 over a v2 base.
#[test]
fn an_override_that_applies_nothing_leaves_the_bytes_alone() {
    let base = base_bin();
    let mut patch = ltk_meta::BinOverride::default();
    patch.patches.push(ltk_meta::PropertyPatch {
        object_hash: h("Characters/Absent"),
        path: PropertyPath::new("speed").unwrap(),
        value: values::F32::new(9.0).into(),
    });
    let mut written = Cursor::new(Vec::new());
    patch.to_writer(&mut written).unwrap();
    let bytes = written.into_inner();

    let mut edit = ltk_game_data::Edit::default();
    edit.overrides
        .push(OverridePath::try_from("a.ptch").unwrap());
    let output = apply(
        &base,
        &[edit],
        |_| Ok(bytes.clone()),
        no_entry,
        &TestSchema::new(),
    )
    .unwrap();

    assert!(!output.changed(), "{:?}", output.applied);
    assert_eq!(output.bytes, base);
    assert_eq!(
        output
            .diagnostics
            .iter()
            .map(|d| d.kind)
            .collect::<Vec<_>>(),
        [ApplyDiagnosticKind::OverrideRecordSkipped]
    );
    // The skip carries what the tree said, which no code of this crate holds.
    assert!(output.diagnostics[0].detail.is_some());
}

/// The game's copy of `Characters/B`, the entry every reference test names.
///
/// Its values are deliberately the shapes `Characters/A` declares, so a reference lands
/// wherever the coercion table would take a literal of the same shape.
fn referenced_entry() -> BinObject {
    BinObject::builder(h("Characters/B"), h("C"))
        .property(h("speed"), values::F32::new(9.0))
        .property(h("name"), values::String::new("copied".into()))
        .property(
            h("tags"),
            values::Container::new(K::Hash, vec![values::Hash::new(h("a")).into()]).unwrap(),
        )
        .property(h("mesh"), embed("copied"))
        .property(h("count"), values::U8::new(7))
        .property(h("link"), values::ObjectLink::new(h("copied")))
        .build()
}

/// A reader of one entry, the hand-written stand-in for an installed game.
fn one_entry(name: &ltk_game_data::EntryName) -> Result<Option<BinObject>, ltk_game_data::Error> {
    Ok((name.as_str() == "Characters/B").then(referenced_entry))
}

fn run_referenced(manifest: &str) -> ApplyResult {
    let declarations = load_declarations("game_data.yaml", manifest, |_| unreachable!())
        .unwrap_or_else(|e| panic!("{e}"));
    let Selector::Target { edits, .. } = &declarations.modules[0].selector else {
        panic!("expected a target");
    };
    apply(
        &base_bin(),
        edits,
        no_override,
        one_entry,
        &TestSchema::new(),
    )
    .unwrap()
}

#[test]
fn a_reference_reads_the_games_copy_wherever_a_value_goes() {
    // A set, a list item, a map value, and a `set` field of a struct pin.
    let output = run_referenced(&manifest(
        "speed: {ref: \"Characters/B:speed\"}\n\
         name: !ref Characters/B:name\n\
         +tags: [!ref \"Characters/B:tags[0]\"]\n\
         +resources:\n  y: !ref Characters/B:link\n\
         mesh: {embed: {set: {texture: !ref Characters/B:name}}}\n",
    ));
    assert_eq!(skips(&output), [], "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "speed"), values::F32::new(9.0).into());
    assert_eq!(
        value_at(&output, "name"),
        values::String::new("copied".into()).into()
    );
    assert_eq!(
        value_at(&output, "tags[2]"),
        values::Hash::new(h("a")).into()
    );
    assert_eq!(
        value_at(&output, "resources{\"y\"}"),
        values::ObjectLink::new(h("copied")).into()
    );
    assert_eq!(
        value_at(&output, "mesh.texture"),
        values::String::new("copied".into()).into()
    );
}

#[test]
fn a_reference_is_an_operand_of_an_addition_and_a_removal() {
    let output = run_referenced(&manifest("-tags: [!ref \"Characters/B:tags[0]\"]\n"));
    assert_eq!(skips(&output), [], "{:?}", output.diagnostics);
    // `a` was the referenced value, so the base's remaining tag is `b`.
    assert_eq!(
        value_at(&output, "tags[0]"),
        values::Hash::new(h("b")).into()
    );
}

#[test]
fn both_reference_reasons_and_a_shape_mismatch_are_reported() {
    // The entry the reader does not supply.
    let output = run_referenced(&manifest("speed: !ref Characters/Missing:speed\n"));
    assert_eq!(skips(&output), [("speed", Reason::ReferenceMissingEntry)]);

    // The entry resolves, the path inside it does not.
    let output = run_referenced(&manifest("speed: !ref Characters/B:absent\n"));
    assert_eq!(skips(&output), [("speed", Reason::ReferenceUnresolved)]);

    // The reference resolves to an `f32` where the property is a string.
    let output = run_referenced(&manifest("name: !ref Characters/B:speed\n"));
    assert_eq!(skips(&output), [("name", Reason::KindMismatch)]);

    // A caller with no game resolves nothing.
    let output = run(
        &manifest("speed: !ref Characters/B:speed\n"),
        &TestSchema::new(),
    );
    assert_eq!(skips(&output), [("speed", Reason::ReferenceMissingEntry)]);
}

#[test]
fn the_dotted_form_reaches_a_field_named_ref() {
    // `a.ref` is a path, not a reference, so it descends and the schema types it. The schema
    // gives `E` a `ref` field, so the value has to land on it rather than merely not resolve.
    let output = run_referenced(&manifest("mesh.ref: 1\n"));
    assert_eq!(skips(&output), [], "{:?}", output.diagnostics);
    assert_eq!(value_at(&output, "mesh.ref"), values::U32::new(1).into());
}

#[test]
fn a_type_pin_over_a_reference_is_refused() {
    // A pin fixes the kind a literal reads as. A reference has no literal spelling, so a pin
    // over one asks for nothing. Every position answers the same way.
    for (key, spelling) in [
        ("speed", "speed: !f32 {ref: \"Characters/B:speed\"}\n"),
        ("tags", "tags: !hash {ref: \"Characters/B:tags\"}\n"),
        ("opt", "opt: !u32 {ref: \"Characters/B:count\"}\n"),
        (
            "mesh.scale",
            "mesh: {embed: {set: {scale: !f32 {ref: \"Characters/B:speed\"}}}}\n",
        ),
    ] {
        let output = run_referenced(&manifest(spelling));
        let reasons: Vec<Reason> = skips(&output).into_iter().map(|(_, r)| r).collect();
        assert_eq!(reasons, [Reason::PinMismatch], "{key}: {spelling}");
    }
}

#[test]
fn a_reference_is_a_whole_container_operand() {
    // The referenced `tags` is a one-item container, and the base's is `[a, b]`.
    let set = run_referenced(&manifest("tags: !ref Characters/B:tags\n"));
    assert_eq!(skips(&set), [], "{:?}", set.diagnostics);
    assert_eq!(value_at(&set, "tags[0]"), values::Hash::new(h("a")).into());

    let added = run_referenced(&manifest("+tags: !ref Characters/B:tags\n"));
    assert_eq!(skips(&added), [], "{:?}", added.diagnostics);
    assert_eq!(
        value_at(&added, "tags[2]"),
        values::Hash::new(h("a")).into()
    );

    let removed = run_referenced(&manifest("-tags: !ref Characters/B:tags\n"));
    assert_eq!(skips(&removed), [], "{:?}", removed.diagnostics);
    assert_eq!(
        value_at(&removed, "tags[0]"),
        values::Hash::new(h("b")).into()
    );
}

#[test]
fn an_entry_the_caller_cannot_read_is_reported_apart_from_one_the_game_lacks() {
    let declarations = load_declarations(
        "game_data.yaml",
        &manifest("speed: !ref Characters/B:speed\n"),
        |_| unreachable!(),
    )
    .unwrap();
    let Selector::Target { edits, .. } = &declarations.modules[0].selector else {
        panic!("expected a target");
    };
    let output = apply(
        &base_bin(),
        edits,
        no_override,
        |name: &ltk_game_data::EntryName| {
            Err(ltk_game_data::Error::io(name.as_str(), &"wad is corrupt"))
        },
        &TestSchema::new(),
    )
    .unwrap();

    // The key still skips, because the value never arrived.
    assert_eq!(skips(&output), [("speed", Reason::ReferenceMissingEntry)]);
    // The read failure is its own diagnostic, naming the reference and what the reader said.
    // Without it the author reads only `ReferenceMissingEntry`, which blames the declaration
    // for an installation the build could not read.
    let unreadable: Vec<&ApplyDiagnostic> = output
        .diagnostics
        .iter()
        .filter(|d| d.kind == ApplyDiagnosticKind::ReferenceUnreadable)
        .collect();
    assert_eq!(unreadable.len(), 1, "{:?}", output.diagnostics);
    assert_eq!(unreadable[0].path, "Characters/B:speed");
    assert_eq!(unreadable[0].edit_index, 0);
    assert!(
        unreadable[0]
            .detail
            .as_ref()
            .is_some_and(|detail| detail.contains("wad is corrupt")),
        "{:?}",
        unreadable[0].detail
    );
}

#[test]
fn the_two_spellings_of_one_entry_are_read_once() {
    // An entry written once by path and once by hash is one entry, so it is read once and
    // both references resolve from that reading.
    let hash_form = format!("{:#010x}", h("Characters/B").0);
    let declarations = load_declarations(
        "game_data.yaml",
        &manifest(&format!(
            "speed: !ref Characters/B:speed\nname: !ref {hash_form}:name\n"
        )),
        |_| unreachable!(),
    )
    .unwrap();
    let Selector::Target { edits, .. } = &declarations.modules[0].selector else {
        panic!("expected a target");
    };
    let mut reads = 0;
    let output = apply(
        &base_bin(),
        edits,
        no_override,
        |name: &ltk_game_data::EntryName| {
            reads += 1;
            Ok((name.object_hash() == h("Characters/B")).then(referenced_entry))
        },
        &TestSchema::new(),
    )
    .unwrap();
    assert_eq!(skips(&output), [], "{:?}", output.diagnostics);
    assert_eq!(reads, 1);
    assert_eq!(value_at(&output, "speed"), values::F32::new(9.0).into());
    assert_eq!(
        value_at(&output, "name"),
        values::String::new("copied".into()).into()
    );
}

#[test]
fn an_entry_is_read_once_however_many_references_name_it() {
    let declarations = load_declarations(
        "game_data.yaml",
        &manifest("speed: !ref Characters/B:speed\nname: !ref Characters/B:name\n"),
        |_| unreachable!(),
    )
    .unwrap();
    let Selector::Target { edits, .. } = &declarations.modules[0].selector else {
        panic!("expected a target");
    };
    let mut reads = Vec::new();
    let output = apply(
        &base_bin(),
        edits,
        no_override,
        |name: &ltk_game_data::EntryName| {
            reads.push(name.as_str().to_owned());
            one_entry(name)
        },
        &TestSchema::new(),
    )
    .unwrap();
    assert_eq!(skips(&output), []);
    assert_eq!(reads, ["Characters/B"]);
}
