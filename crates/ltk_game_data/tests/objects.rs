//! `objects` bindings: loading, the document round trip, and the creation and removal phases.

use std::{collections::HashSet, io::Cursor};

use ltk_game_data::{
    ApplyDiagnosticKind, ApplyResult, BinHash, DeclarationDocument, Declarations, Edit, EntryName,
    ErrorKind, NoSchema, ObjectEdit, ObjectSkipReason, OverridePath, PropertyKind as K,
    PropertySkipReason, Schema, Selector, Shape, apply, load_declarations,
};
use ltk_meta::{Bin, BinObject, PropertyValueEnum as V, path::PropertyPath, property::values};

fn h(name: &str) -> BinHash {
    BinHash::from(name)
}

fn no_override(path: &OverridePath) -> Result<Vec<u8>, ltk_game_data::Error> {
    unreachable!("no override is read: {path}")
}

fn no_entry(_: &EntryName) -> Result<Option<BinObject>, ltk_game_data::Error> {
    Ok(None)
}

/// A schema of class `C`: `speed`, an `f32`; `label`, a `string`; `objectPath`, a `hash`.
struct TestSchema;

impl Schema for TestSchema {
    fn expected(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        if class != h("C") {
            return None;
        }
        [
            ("speed", K::F32),
            ("label", K::String),
            ("particlePath", K::String),
            ("objectPath", K::Hash),
        ]
        .into_iter()
        .find(|(name, _)| h(name) == field)
        .map(|(_, kind)| Shape::bare(kind))
    }

    fn has_class(&self, class: BinHash) -> bool {
        HashSet::from([h("C")]).contains(&class)
    }
}

/// A PROP v3 with `Characters/A` of class `C`, whose `objectPath` and `particlePath` name the
/// object itself and whose `label` names another object.
fn base_bin() -> Vec<u8> {
    let object = BinObject::builder(h("Characters/A"), h("C"))
        .property(h("speed"), values::F32::new(1.0))
        .property(h("objectPath"), values::Hash::new(h("Characters/A")))
        .property(
            h("particlePath"),
            values::String::new("characters/a".into()),
        )
        .property(h("label"), values::String::new("Characters/B".into()))
        .build();
    let mut cursor = Cursor::new(Vec::new());
    Bin::builder()
        .object(object)
        .build()
        .to_writer(&mut cursor)
        .unwrap();
    cursor.into_inner()
}

fn load(name: &str, text: &str) -> Declarations {
    load_declarations(name, text, |_| unreachable!()).unwrap_or_else(|error| panic!("{error}"))
}

fn edits(declarations: &Declarations) -> &[Edit] {
    match &declarations.modules[0].selector {
        Selector::Target { edits, .. } => edits,
        _ => panic!("expected a target module"),
    }
}

/// A target manifest over `a.bin` holding `body`, one edit, indented under the module.
fn manifest(body: &str) -> String {
    let body = body
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("version: 1\nmodules:\n  - target: a.bin\n{body}\n")
}

fn run(text: &str, schema: &dyn Schema) -> ApplyResult {
    let declarations = load("game_data.yaml", text);
    apply(
        &base_bin(),
        edits(&declarations),
        no_override,
        no_entry,
        schema,
    )
    .unwrap()
}

fn object<'a>(bin: &'a Bin, name: &str) -> &'a BinObject {
    bin.objects
        .get(&h(name))
        .unwrap_or_else(|| panic!("no object {name}"))
}

fn at(object: &BinObject, path: &str) -> V {
    object
        .resolve(&PropertyPath::new(path).unwrap())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
        .clone()
}

fn skipped_objects(output: &ApplyResult) -> Vec<(&str, ObjectSkipReason)> {
    output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == ApplyDiagnosticKind::ObjectSkipped)
        .map(|diagnostic| {
            let object = diagnostic.object.as_ref().unwrap();
            (object.name.as_str(), object.reason)
        })
        .collect()
}

const YAML: &str = "version: 1
modules:
  - target: a.bin
    objects:
      Characters/Copy:
        clone: Characters/A
        set:
          speed: !f32 2.5
      Characters/New:
        class: C
        set:
          label: fresh
      Characters/Old:
        remove: true
";

const TOML: &str = r#"version = 1
[[modules]]
target = "a.bin"
[modules.objects."Characters/Copy"]
clone = "Characters/A"
set = { speed = { f32 = 2.5 } }
[modules.objects."Characters/New"]
class = "C"
set = { label = "fresh" }
[modules.objects."Characters/Old"]
remove = true
"#;

const JSON: &str = r#"{"version": 1, "modules": [{"target": "a.bin", "objects": {
  "Characters/Copy": {"clone": "Characters/A", "set": {"speed": {"f32": 2.5}}},
  "Characters/New": {"class": "C", "set": {"label": "fresh"}},
  "Characters/Old": {"remove": true}
}}]}"#;

#[test]
fn objects_load_from_every_format_as_one_model() {
    let yaml = load("game_data.yaml", YAML);
    let edit = &edits(&yaml)[0];
    let names: Vec<&str> = edit.objects.keys().map(EntryName::as_str).collect();
    assert_eq!(
        names,
        ["Characters/Copy", "Characters/New", "Characters/Old"]
    );
    assert!(matches!(
        &edit.objects[0],
        ObjectEdit::Clone { source, properties }
            if source.as_str() == "Characters/A" && properties.len() == 1
    ));
    assert!(matches!(
        &edit.objects[1],
        ObjectEdit::Construct { class, properties }
            if class.as_str() == "C" && class.class_hash() == h("C") && properties.len() == 1
    ));
    assert_eq!(edit.objects[2], ObjectEdit::Remove);
    for (name, text) in [("game_data.toml", TOML), ("game_data.json", JSON)] {
        assert_eq!(edits(&load(name, text)), edits(&yaml), "{name}");
    }
}

#[test]
fn objects_round_trip_through_documents_and_manifests() {
    let declarations = load("game_data.yaml", YAML);
    let document = DeclarationDocument::try_from(declarations.clone()).unwrap();
    assert_eq!(document.parse().unwrap(), declarations);
    let manifest = declarations.manifest_json().unwrap();
    assert_eq!(
        edits(&load("game_data.json", &manifest)),
        edits(&declarations)
    );
}

#[test]
fn an_object_body_takes_clone_or_class_with_set_or_remove_alone() {
    for body in [
        "Characters/X: {clone: Characters/A, class: C}",
        "Characters/X: {set: {speed: 1}}",
        "Characters/X: {remove: false}",
        "Characters/X: {remove: true, set: {speed: 1}}",
        "Characters/X: {remove: true, clone: Characters/A}",
        "Characters/X: {clone: Characters/A, extra: 1}",
        "Characters/X: {clone: Characters/A, set: [1]}",
        "Characters/X: {clone: 5}",
        "Characters/X: 5",
    ] {
        let text = manifest(&format!("objects:\n  {body}"));
        let error = load_declarations("game_data.yaml", &text, |_| unreachable!()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::ObjectBodyShape, "{body}: {error}");
        assert_eq!(
            error.location.entry.as_deref(),
            Some("Characters/X"),
            "{body}"
        );
    }
    let error = load_declarations(
        "game_data.yaml",
        &manifest("objects: [Characters/X]"),
        |_| unreachable!(),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::ObjectBodyShape);
    let error = load_declarations(
        "game_data.yaml",
        &manifest("objects:\n  Characters/X: {class: ''}"),
        |_| unreachable!(),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::EmptyClassName);
}

#[test]
fn objects_inside_an_entries_module_are_errors() {
    let text = "version: 1\nmodules:\n  - entries:\n      Characters/A:\n        objects: {}\n";
    let error = load_declarations("game_data.yaml", text, |_| unreachable!()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::ObjectsInEntry, "{error}");
    // The field Riot spells `Objects` is a property, as `Overrides` is.
    load(
        "game_data.yaml",
        "version: 1\nmodules:\n  - entries:\n      Characters/A:\n        Objects: null\n",
    );
}

#[test]
fn a_clone_copies_its_source_applies_set_and_names_itself() {
    let output = run(
        &manifest("objects:\n  Characters/Copy:\n    clone: Characters/A\n    set: {speed: 2.5}"),
        &TestSchema,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(output.applied.objects, 1);
    assert_eq!(output.applied.properties, 1);
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    let copy = object(&bin, "Characters/Copy");
    assert_eq!(copy.class_hash, h("C"));
    assert_eq!(at(copy, "speed"), values::F32::new(2.5).into());
    // The properties naming the source name the copy; every other one is copied as it is.
    assert_eq!(
        at(copy, "objectPath"),
        values::Hash::new(h("Characters/Copy")).into()
    );
    assert_eq!(
        at(copy, "particlePath"),
        values::String::new("Characters/Copy".into()).into()
    );
    assert_eq!(
        at(copy, "label"),
        values::String::new("Characters/B".into()).into()
    );
    // The source is untouched.
    let source = object(&bin, "Characters/A");
    assert_eq!(at(source, "speed"), values::F32::new(1.0).into());
    assert_eq!(
        at(source, "objectPath"),
        values::Hash::new(h("Characters/A")).into()
    );
}

#[test]
fn a_construction_holds_its_class_and_set_only() {
    let text = manifest("objects:\n  Characters/New:\n    class: C\n    set: {label: fresh}");
    let output = run(&text, &TestSchema);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    let new = object(&bin, "Characters/New");
    assert_eq!(new.class_hash, h("C"));
    assert_eq!(new.properties.len(), 1);
    assert_eq!(at(new, "label"), values::String::new("fresh".into()).into());

    // A class the schema does not know is not constructed.
    let output = run(&text, &NoSchema);
    assert_eq!(
        skipped_objects(&output),
        [("Characters/New", ObjectSkipReason::UnknownClass)]
    );
    assert!(!output.changed());
}

/// [`TestSchema`] for a build it does not describe: its shapes answer only as fallbacks.
struct FallbackOnly;

impl Schema for FallbackOnly {
    fn expected(&self, _: BinHash, _: BinHash) -> Option<Shape> {
        None
    }

    fn fallback(&self, class: BinHash, field: BinHash) -> Option<Shape> {
        TestSchema.expected(class, field)
    }

    fn has_class(&self, class: BinHash) -> bool {
        TestSchema.has_class(class)
    }
}

#[test]
fn a_construction_types_its_set_through_the_fallback_on_an_undescribed_build() {
    let text = manifest("objects:\n  Characters/New:\n    class: C\n    set: {label: fresh}");
    let output = run(&text, &FallbackOnly);
    let fallbacks: Vec<_> = output
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.kind, diagnostic.path.as_str()))
        .collect();
    assert_eq!(fallbacks, [(ApplyDiagnosticKind::SchemaFallback, "label")]);
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    let new = object(&bin, "Characters/New");
    assert_eq!(at(new, "label"), values::String::new("fresh".into()).into());
}

#[test]
fn a_set_edit_that_does_not_apply_is_reported_under_the_new_object() {
    let output = run(
        &manifest("objects:\n  Characters/Copy:\n    clone: Characters/A\n    set: {speed: text}"),
        &TestSchema,
    );
    let skipped: Vec<_> = output
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == ApplyDiagnosticKind::PropertyEditSkipped)
        .map(|diagnostic| {
            let property = diagnostic.property.as_ref().unwrap();
            (
                property.entry.as_str(),
                diagnostic.path.as_str(),
                property.reason,
            )
        })
        .collect();
    assert_eq!(
        skipped,
        [("Characters/Copy", "speed", PropertySkipReason::KindMismatch)]
    );
    // The object is created whatever its `set` does.
    assert_eq!(output.applied.objects, 1);
}

#[test]
fn a_creation_needs_a_free_name_and_a_source_held_at_the_start() {
    // `Characters/C` spelled a second time, by its hash.
    let hashed = format!("0x{:08x}", *h("Characters/C"));
    let output = run(
        &manifest(&format!(
            "objects:
  Characters/A: {{class: C}}
  Characters/B: {{clone: Characters/Nope}}
  Characters/C: {{clone: Characters/A}}
  '{hashed}': {{class: C}}
  Characters/D: {{clone: Characters/C}}"
        )),
        &TestSchema,
    );
    assert_eq!(
        skipped_objects(&output),
        [
            ("Characters/A", ObjectSkipReason::ObjectExists),
            ("Characters/B", ObjectSkipReason::SourceMissing),
            (hashed.as_str(), ObjectSkipReason::ObjectExists),
            // A clone reads the target before the batch creates anything.
            ("Characters/D", ObjectSkipReason::SourceMissing),
        ]
    );
    assert_eq!(output.applied.objects, 1);
}

#[test]
fn a_later_edit_clones_what_an_earlier_one_created() {
    let output = run(
        &manifest(
            "edits:
  - objects:
      Characters/C: {clone: Characters/A}
  - objects:
      Characters/D: {clone: Characters/C}",
        ),
        &TestSchema,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    assert_eq!(
        at(object(&bin, "Characters/D"), "objectPath"),
        values::Hash::new(h("Characters/D")).into()
    );
}

#[test]
fn entry_edits_run_between_creation_and_removal() {
    let output = run(
        &manifest(
            "objects:
  Characters/New: {class: C}
  Characters/A: {remove: true}
  Characters/Gone: {remove: true}
Characters/New:
  speed: 4
Characters/A:
  speed: 3",
        ),
        &TestSchema,
    );
    assert_eq!(
        skipped_objects(&output),
        [("Characters/Gone", ObjectSkipReason::RemovalUnmatched)]
    );
    // The new object is edited as an entry; the removed one is edited, then removed.
    assert_eq!(output.applied.properties, 2);
    assert_eq!(output.applied.objects, 2);
    let bin = Bin::from_reader(&mut Cursor::new(&output.bytes)).unwrap();
    assert_eq!(
        at(object(&bin, "Characters/New"), "speed"),
        values::F32::new(4.0).into()
    );
    assert!(!bin.objects.contains_key(&h("Characters/A")));
}

#[test]
fn a_reference_in_a_set_is_a_reference_of_the_edit() {
    let declarations = load(
        "game_data.yaml",
        &manifest(
            "objects:\n  Characters/New:\n    class: C\n    set: {speed: !ref Characters/B:speed}",
        ),
    );
    let references = declarations.modules[0].references();
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].to_string(), "Characters/B:speed");
}
