//! A `PTCH` target: every edit lowered to the patch's own deletions, objects and records.

use std::io::Cursor;

use ltk_game_data::{
    ApplyDiagnosticKind, ApplyResult, BinHash, Declarations, Edit, EntryName, ErrorKind, NoSchema,
    OverridePath, PropertySkipReason, Selector, apply, load_declarations,
};
use ltk_meta::{
    Bin, BinObject, BinOverride, PropertyValueEnum as V, path::PropertyPath, property::values,
};

fn h(name: &str) -> BinHash {
    BinHash::from(name)
}

fn path(text: &str) -> PropertyPath {
    PropertyPath::new(text).unwrap()
}

fn no_override(path: &OverridePath) -> Result<Vec<u8>, ltk_game_data::Error> {
    unreachable!("no override is read: {path}")
}

/// The base scene: `UI/Icon` with a `speed`, a `Rect` embed of `x` and `y`, and a `tags` list,
/// and `UI/Other` with a `speed`.
fn scene() -> Bin {
    let rect = values::Embedded(values::Struct {
        class_hash: h("Rect"),
        properties: [
            (h("x"), values::F32::new(1.0).into()),
            (h("y"), values::F32::new(2.0).into()),
        ]
        .into_iter()
        .collect(),
    });
    let tags = values::Container::new(
        ltk_meta::PropertyKind::String,
        vec![values::String::new("a".into()).into()],
    )
    .unwrap();
    let icon = BinObject::builder(h("UI/Icon"), h("Icon"))
        .property(h("speed"), values::F32::new(1.0))
        .property(h("Rect"), rect)
        .property(h("tags"), tags)
        .build();
    let other = BinObject::builder(h("UI/Other"), h("Icon"))
        .property(h("speed"), values::F32::new(4.0))
        .build();
    Bin::builder().object(icon).object(other).build()
}

/// A variant that moves `UI/Icon` to `x = 10` and adds `UI/Added`.
fn variant() -> BinOverride {
    BinOverride::builder()
        .set(h("UI/Icon"), path("Rect.x"), values::F32::new(10.0))
        .object(
            BinObject::builder(h("UI/Added"), h("Icon"))
                .property(h("speed"), values::F32::new(7.0))
                .build(),
        )
        .build()
}

fn bytes_of(patch: &BinOverride) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    patch.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

fn edits(body: &str) -> Declarations {
    let body = body
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!("version: 1\nmodules:\n  - target: ui/uirtl\n{body}\n");
    load_declarations("game_data.yaml", &text, |_| unreachable!())
        .unwrap_or_else(|error| panic!("{error}"))
}

fn edits_of(declarations: &Declarations) -> &[Edit] {
    match &declarations.modules[0].selector {
        Selector::Target { edits, .. } => edits,
        _ => panic!("expected a target module"),
    }
}

/// The game's copy of an entry: the base scene's object.
fn game_entry(name: &EntryName) -> Result<Option<BinObject>, ltk_game_data::Error> {
    Ok(scene().objects.get(&name.object_hash()).cloned())
}

fn run(body: &str) -> ApplyResult {
    let declarations = edits(body);
    apply(
        &bytes_of(&variant()),
        edits_of(&declarations),
        no_override,
        game_entry,
        &NoSchema,
    )
    .unwrap()
}

fn patch_of(output: &ApplyResult) -> BinOverride {
    BinOverride::from_reader(&mut Cursor::new(&output.bytes)).unwrap()
}

/// The base scene with the output laid over it, as the client lays a switched-on variant.
fn laid(output: &ApplyResult) -> Bin {
    let mut bin = scene();
    let report = patch_of(output).apply(&mut bin);
    assert!(report.is_clean(), "{report}");
    bin
}

fn at(bin: &Bin, object: &str, property: &str) -> V {
    bin.objects[&h(object)]
        .resolve(&path(property))
        .unwrap_or_else(|error| panic!("{property}: {error}"))
        .clone()
}

fn records(patch: &BinOverride) -> Vec<(BinHash, &str)> {
    patch
        .patches
        .iter()
        .map(|record| (record.object_hash, record.path.as_str()))
        .collect()
}

fn skips(output: &ApplyResult) -> Vec<(ApplyDiagnosticKind, &str)> {
    output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind != ApplyDiagnosticKind::SchemaFallback)
        .map(|diagnostic| (diagnostic.kind, diagnostic.path.as_str()))
        .collect()
}

#[test]
fn an_edit_of_a_base_object_is_a_record_the_client_lays_over_the_base() {
    let output = run("UI/Other:\n  speed: !f32 9");

    assert_eq!(skips(&output), []);
    assert!(output.dependencies.is_empty());
    let patch = patch_of(&output);
    assert_eq!(
        records(&patch),
        [(h("UI/Icon"), "Rect.x"), (h("UI/Other"), "speed")]
    );
    assert_eq!(
        at(&laid(&output), "UI/Other", "speed"),
        values::F32::new(9.0).into()
    );
}

#[test]
fn a_record_at_a_held_path_or_above_it_replaces_the_held_records() {
    let output = run("UI/Icon:\n  Rect:\n    y: !f32 5");

    assert_eq!(
        records(&patch_of(&output)),
        [(h("UI/Icon"), "Rect.x"), (h("UI/Icon"), "Rect.y")]
    );

    let replaced = run("UI/Icon:\n  Rect.x: !f32 3");
    assert_eq!(records(&patch_of(&replaced)), [(h("UI/Icon"), "Rect.x")]);
    assert_eq!(
        at(&laid(&replaced), "UI/Icon", "Rect.x"),
        values::F32::new(3.0).into()
    );
}

#[test]
fn a_container_edit_reads_the_base_the_variant_shows() {
    let output = run("UI/Icon:\n  +tags: [b]");

    let bin = laid(&output);
    let V::Container(tags) = at(&bin, "UI/Icon", "tags") else {
        panic!("tags is a list");
    };
    assert_eq!(tags.into_items().len(), 2);
    assert_eq!(at(&bin, "UI/Icon", "Rect.x"), values::F32::new(10.0).into());
}

#[test]
fn an_object_the_variant_holds_takes_an_edit_in_place() {
    let output = run("UI/Added:\n  speed: !f32 1");

    let patch = patch_of(&output);
    assert_eq!(records(&patch), [(h("UI/Icon"), "Rect.x")]);
    assert_eq!(
        patch.objects[&h("UI/Added")].properties[&h("speed")],
        values::F32::new(1.0).into()
    );
}

#[test]
fn a_removal_deletes_a_base_object_and_drops_one_the_variant_holds() {
    let output = run("objects:\n  UI/Icon:\n    remove: true\n  UI/Added:\n    remove: true");

    assert_eq!(skips(&output), []);
    let patch = patch_of(&output);
    assert_eq!(patch.deleted, [h("UI/Icon")]);
    assert!(patch.objects.is_empty());
    assert!(patch.patches.is_empty());
}

#[test]
fn a_created_object_is_one_the_variant_holds() {
    let output = run("objects:\n  UI/Copy:\n    clone: UI/Icon\n  UI/Other:\n    clone: UI/Icon");

    assert_eq!(
        skips(&output),
        [(ApplyDiagnosticKind::ObjectSkipped, "UI/Other")]
    );
    let patch = patch_of(&output);
    let copy = &patch.objects[&h("UI/Copy")];
    assert_eq!(
        copy.resolve(&path("Rect.x")).unwrap(),
        &V::from(values::F32::new(10.0))
    );
}

#[test]
fn a_hash_form_path_on_a_base_object_is_skipped() {
    let speed = format!("\"0x{:08x}\"", h("speed").0);
    let output = run(&format!(
        "UI/Other:\n  {speed}: !f32 1\nUI/Added:\n  {speed}: !f32 1"
    ));

    let skipped: Vec<_> = output
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.property.as_ref())
        .map(|property| (property.entry.as_str(), property.reason))
        .collect();
    assert_eq!(skipped, [("UI/Other", PropertySkipReason::HashFormPath)]);
}

#[test]
fn a_link_edit_is_unsupported_and_the_rest_applies() {
    let output = run("links: [data/a.bin]\nUI/Other:\n  speed: !f32 9");

    assert_eq!(
        skips(&output),
        [(ApplyDiagnosticKind::LinkUnsupported, "data/a.bin")]
    );
    assert!(output.changed());
}

#[test]
fn an_entry_the_game_lacks_or_cannot_read_is_reported() {
    let declarations = edits("UI/Gone:\n  speed: !f32 1\nUI/Broken:\n  speed: !f32 1");
    let output = apply(
        &bytes_of(&variant()),
        edits_of(&declarations),
        no_override,
        |name: &EntryName| {
            if name.as_str() == "UI/Broken" {
                return Err(ltk_game_data::Error::new(ErrorKind::UnsupportedBase));
            }
            Ok(None)
        },
        &NoSchema,
    )
    .unwrap();

    let kinds: Vec<_> = skips(&output).into_iter().map(|(kind, _)| kind).collect();
    assert_eq!(
        kinds,
        [
            ApplyDiagnosticKind::EntryUnreadable,
            ApplyDiagnosticKind::PropertyEditSkipped,
            ApplyDiagnosticKind::PropertyEditSkipped,
        ]
    );
    assert!(!output.changed());
    assert_eq!(output.bytes, bytes_of(&variant()));
}

#[test]
fn an_override_file_merges_into_the_variant() {
    let file = BinOverride::builder()
        .set(h("UI/Other"), path("speed"), values::F32::new(2.0))
        .delete(h("UI/Gone"))
        .build();
    let declarations = edits("overrides: [ui/extra.ptch]");
    let output = apply(
        &bytes_of(&variant()),
        edits_of(&declarations),
        |_: &OverridePath| Ok(bytes_of(&file)),
        game_entry,
        &NoSchema,
    )
    .unwrap();

    let patch = patch_of(&output);
    assert_eq!(
        records(&patch),
        [(h("UI/Icon"), "Rect.x"), (h("UI/Other"), "speed")]
    );
    assert_eq!(patch.deleted, [h("UI/Gone")]);
    assert_eq!(output.applied.records, 1);
}
