use indexmap::IndexMap;
use ltk_game_data::{
    DeclarationDocument, Declarations, Edit, EntryEdit, EntryName, ErrorKind, Module, PropertyEdit,
    Selector, Sign, Value, load_declarations,
};

fn no_source(_: &str) -> Result<String, ltk_game_data::Error> {
    unreachable!("no source is read")
}

fn load(name: &str, text: &str) -> Declarations {
    load_declarations(name, text, no_source).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn target_edits(module: &Module) -> &[Edit] {
    match &module.selector {
        Selector::Target { edits, .. } => edits,
        other => panic!("expected a target selector, got {other:?}"),
    }
}

fn entry_edits(module: &Module) -> &IndexMap<EntryName, EntryEdit> {
    match &module.selector {
        Selector::Entries(entries) => entries,
        other => panic!("expected an entries selector, got {other:?}"),
    }
}

fn selectors(declarations: &Declarations) -> Vec<&Selector> {
    declarations.modules.iter().map(|m| &m.selector).collect()
}

fn keys(edits: &[PropertyEdit]) -> Vec<String> {
    edits.iter().map(PropertyEdit::key).collect()
}

const YAML: &str = r#"version: 1
modules:
  - target: data/characters/teemo/skins/skin0.bin
    Characters/Teemo/Skins/Skin0:
      skinMeshProperties:
        selfIllumination: 1.0
        +tagEventList: [Jade_Teemo]
      healthBarData.unitHealthBarStyle: !u8 12
      iconCircle: assets/teemo_circle.tex
      -skinAudioProperties.bankUnits: [0, 2]
    Characters/Teemo/Skins/Skin0/Resources:
      +resourceMap:
        Teemo_R_Mis: !link Characters/Jade_Teemo/Particles/R_Mis
        Teemo_R_Debuff: null
"#;

const TOML: &str = r#"version = 1

[[modules]]
target = "data/characters/teemo/skins/skin0.bin"
"Characters/Teemo/Skins/Skin0" = {
  skinMeshProperties = { selfIllumination = 1.0, "+tagEventList" = ["Jade_Teemo"] },
  "healthBarData.unitHealthBarStyle" = { u8 = 12 },
  iconCircle = "assets/teemo_circle.tex",
  "-skinAudioProperties.bankUnits" = [0, 2],
}
"Characters/Teemo/Skins/Skin0/Resources" = {
  "+resourceMap" = { Teemo_R_Mis = { link = "Characters/Jade_Teemo/Particles/R_Mis" }, Teemo_R_Debuff = "" },
}
"#;

const JSON: &str = r#"{
  "version": 1,
  "modules": [
    {
      "target": "data/characters/teemo/skins/skin0.bin",
      "Characters/Teemo/Skins/Skin0": {
        "skinMeshProperties": { "selfIllumination": 1.0, "+tagEventList": ["Jade_Teemo"] },
        "healthBarData.unitHealthBarStyle": { "u8": 12 },
        "iconCircle": "assets/teemo_circle.tex",
        "-skinAudioProperties.bankUnits": [0, 2]
      },
      "Characters/Teemo/Skins/Skin0/Resources": {
        "+resourceMap": { "Teemo_R_Mis": { "link": "Characters/Jade_Teemo/Particles/R_Mis" }, "Teemo_R_Debuff": null }
      }
    }
  ]
}"#;

#[test]
fn entry_bodies_load_from_every_format_in_key_order() {
    let yaml = load("game_data.yaml", YAML);
    let toml = load("game_data.toml", TOML);
    let json = load("game_data.json", JSON);
    let edits = target_edits(&yaml.modules[0]);
    assert_eq!(edits.len(), 1);
    let entries = &edits[0].entries;
    assert_eq!(
        entries.keys().map(EntryName::as_str).collect::<Vec<_>>(),
        [
            "Characters/Teemo/Skins/Skin0",
            "Characters/Teemo/Skins/Skin0/Resources"
        ]
    );
    let skin = &entries[&EntryName::try_from("Characters/Teemo/Skins/Skin0").unwrap()];
    assert_eq!(
        keys(skin),
        [
            "skinMeshProperties",
            "healthBarData.unitHealthBarStyle",
            "iconCircle",
            "-skinAudioProperties.bankUnits"
        ]
    );
    assert_eq!(skin[3].sign, Sign::Remove);
    assert_eq!(skin[3].path.as_str(), "skinAudioProperties.bankUnits");
    assert_eq!(
        skin[3].value,
        Value::List(vec![Value::Integer(0), Value::Integer(2)])
    );
    // A block stays a mapping; its inner signed key is a key of the mapping.
    let Value::Mapping(block) = &skin[0].value else {
        panic!("expected a block, got {:?}", skin[0].value);
    };
    assert_eq!(
        block.keys().collect::<Vec<_>>(),
        ["selfIllumination", "+tagEventList"]
    );
    assert_eq!(block["selfIllumination"], Value::Float(1.0));
    // A YAML tag and a one-key mapping are the same pin.
    let pinned = &skin[1].value;
    assert_eq!(pinned.pin(), Some("u8"));
    assert_eq!(
        *pinned,
        Value::Mapping(IndexMap::from([("u8".to_owned(), Value::Integer(12))]))
    );
    // TOML spells a null link as the empty string; the other two carry null.
    assert_eq!(target_edits(&json.modules[0]), edits);
    let toml_edits = target_edits(&toml.modules[0]);
    let resources = EntryName::try_from("Characters/Teemo/Skins/Skin0/Resources").unwrap();
    let Value::Mapping(map) = &toml_edits[0].entries[&resources][0].value else {
        panic!("expected a mapping");
    };
    assert_eq!(map["Teemo_R_Debuff"], Value::String(String::new()));
    let Value::Mapping(map) = &entries[&resources][0].value else {
        panic!("expected a mapping");
    };
    assert_eq!(map["Teemo_R_Debuff"], Value::Null);
    assert_eq!(map["Teemo_R_Mis"].pin(), Some("link"));
}

#[test]
fn block_and_dotted_forms_load_as_written() {
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a.b: 1\n      a: {c: 2, 'd[0]': {e: 3}}\n";
    let declarations = load("game_data.yaml", text);
    let edits = &target_edits(&declarations.modules[0])[0].entries;
    let body = &edits[&EntryName::try_from("Characters/A").unwrap()];
    assert_eq!(keys(body), ["a.b", "a"]);
    assert_eq!(body[0].value, Value::Integer(1));
    let Value::Mapping(block) = &body[1].value else {
        panic!("expected a block");
    };
    assert_eq!(block.keys().collect::<Vec<_>>(), ["c", "d[0]"]);
}

#[test]
fn an_entries_module_mixes_links_and_property_edits() {
    let text = "version: 1\nmodules:\n  - entries:\n      Characters/A:\n        links: [x.bin]\n        speed: 2.5\n        -links: [y.bin]\n        +tags: [a]\n";
    let declarations = load("game_data.yaml", text);
    let entries = entry_edits(&declarations.modules[0]);
    let edit = &entries[&EntryName::try_from("Characters/A").unwrap()];
    assert_eq!(keys(&edit.properties), ["speed", "+tags"]);
    assert_eq!(edit.links.add[0].as_str(), "x.bin");
    assert_eq!(edit.links.remove[0].as_str(), "y.bin");
}

#[test]
fn root_keys_that_are_not_entry_names_are_unsupported_bindings() {
    for (name, text) in [
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    objects: {}\n",
        ),
        (
            "game_data.json",
            r#"{"version": 1, "modules": [{"target": "a.bin", "speed": 1}]}"#,
        ),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = \"a.bin\"\nmodes = { a = 1 }\n",
        ),
    ] {
        let error = load_declarations(name, text, no_source).unwrap_err();
        assert!(
            matches!(&error.kind, ErrorKind::UnsupportedBinding { key } if key == "objects" || key == "speed" || key == "modes"),
            "{name}: {error}"
        );
        assert_eq!(error.location.module, Some(0), "{name}: {error}");
    }
    // A hash-form key is an entry name.
    let text = "version: 1\nmodules:\n  - target: a.bin\n    '0x6ecc5fac': {speed: 1}\n";
    let declarations = load("game_data.yaml", text);
    let edit = &target_edits(&declarations.modules[0])[0];
    assert_eq!(edit.entries.keys().next().unwrap().as_str(), "0x6ecc5fac");
    // An entry body that is not a mapping is an error naming the entry.
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/A: 5\n";
    let error = load_declarations("game_data.yaml", text, no_source).unwrap_err();
    assert_eq!(error.kind, ErrorKind::EntryBodyShape);
    assert_eq!(error.location.entry.as_deref(), Some("Characters/A"));
}

#[test]
fn structural_refusals_name_the_key() {
    type Expected = fn(&ErrorKind) -> bool;
    let cases: [(&str, &str, Expected); 8] = [
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      '+a[b': [1]\n",
            |kind| matches!(kind, ErrorKind::InvalidPropertyPath { .. }),
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: !widget 1\n",
            |kind| matches!(kind, ErrorKind::Syntax { detail } if detail.contains("!widget")),
        ),
        (
            "game_data.json",
            r#"{"version": 1, "modules": [{"target": "a.bin", "Characters/A": {"a": {"pointer": 5}}}]}"#,
            |kind| *kind == ErrorKind::StructPinShape,
        ),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = \"a.bin\"\n\"Characters/A\" = { a = { embed = { class = 1 } } }\n",
            |kind| *kind == ErrorKind::StructPinShape,
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: [{pointer: {class: X, set: {f: 1}, extra: 2}}]\n",
            |kind| *kind == ErrorKind::StructPinShape,
        ),
        // A `ref` whose value is not a string at all.
        (
            "game_data.json",
            r#"{"version": 1, "modules": [{"target": "a.bin", "Characters/A": {"a": {"ref": 5}}}]}"#,
            |kind| *kind == ErrorKind::ReferenceShape,
        ),
        // A string with no `:` to split at.
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = \"a.bin\"\n\"Characters/A\" = { a = { ref = \"nocolon\" } }\n",
            |kind| *kind == ErrorKind::ReferenceShape,
        ),
        // A reference nested in a list, whose path half does not parse.
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: [{ref: \"entry:b[\"}]\n",
            |kind| *kind == ErrorKind::ReferenceShape,
        ),
    ];
    for (name, text, expected) in cases {
        let error = load_declarations(name, text, no_source).unwrap_err();
        assert!(expected(&error.kind), "{name}: {error}");
        if !matches!(error.kind, ErrorKind::Syntax { .. }) {
            let key = if matches!(error.kind, ErrorKind::InvalidPropertyPath { .. }) {
                "+a[b"
            } else {
                "a"
            };
            assert_eq!(error.location.key.as_deref(), Some(key), "{name}: {error}");
            assert_eq!(error.location.entry.as_deref(), Some("Characters/A"));
        }
    }
    // Well-formed struct pins load.
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: !pointer null\n      b: !pointer {class: X, set: {f: 1}}\n      c: {embed: {set: {g: !pointer {class: Y}}}}\n";
    load("game_data.yaml", text);
}

#[test]
fn duplicate_signed_keys_are_errors_in_every_format() {
    for (name, text) in [
        (
            "game_data.json",
            r#"{"version": 1, "modules": [{"target": "a.bin", "Characters/A": {"a": 1, "a": 2}}]}"#,
        ),
        (
            "game_data.json",
            r#"{"version": 1, "modules": [{"target": "a.bin", "Characters/A": {"a": {"b": 1, "b": 2}}}]}"#,
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: 1\n      a: 2\n",
        ),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = \"a.bin\"\n\"Characters/A\" = { a = 1, a = 2 }\n",
        ),
    ] {
        let error = load_declarations(name, text, no_source).unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Syntax { .. }),
            "{name}: {error}"
        );
    }
}

#[test]
fn entry_bodies_round_trip_through_documents_and_manifests() {
    let declarations = load("game_data.yaml", YAML);
    let document = DeclarationDocument::try_from(declarations.clone()).unwrap();
    assert_eq!(document.parse().unwrap(), declarations);
    let json = serde_json::to_string(&document).unwrap();
    assert!(
        json.contains(r#""healthBarData.unitHealthBarStyle":{"u8":12}"#),
        "{json}"
    );
    assert!(json.contains(r#""+tagEventList":["Jade_Teemo"]"#), "{json}");
    let manifest = declarations.manifest_json().unwrap();
    assert_eq!(
        selectors(&load("game_data.json", &manifest)),
        selectors(&declarations)
    );

    let text = "version: 1\nmodules:\n  - entries:\n      Characters/A:\n        speed: 2.5\n        +tags: [a]\n";
    let declarations = load("game_data.yaml", text);
    let manifest = declarations.manifest_json().unwrap();
    assert_eq!(
        selectors(&load("game_data.json", &manifest)),
        selectors(&declarations)
    );
    let document = DeclarationDocument::try_from(declarations.clone()).unwrap();
    assert_eq!(document.parse().unwrap(), declarations);
}

/// Declarations of one `entries` module editing `Characters/A` with `properties`.
fn entries_module(properties: Vec<PropertyEdit>) -> Declarations {
    let mut edit = EntryEdit::default();
    edit.properties = properties;
    let mut entries = IndexMap::new();
    entries.insert(EntryName::try_from("Characters/A").unwrap(), edit);
    Declarations {
        version: 1,
        modules: vec![Module {
            selector: Selector::Entries(entries),
            origin: ltk_game_data::Origin {
                manifest: "game_data.json".into(),
                source: None,
                module_index: 0,
            },
        }],
    }
}

#[test]
fn a_property_path_spelling_a_binding_keyword_refuses_to_serialize() {
    for key in ["overrides", "links", "+links", "-links"] {
        let declarations =
            entries_module(vec![PropertyEdit::parse(key, Value::Integer(1)).unwrap()]);
        let error = declarations.manifest_json().unwrap_err();
        assert!(
            matches!(&error.kind, ErrorKind::ReservedBindingKey { key: named } if named == key),
            "{key}: {error:?}"
        );
        assert!(
            DeclarationDocument::try_from(declarations).is_err(),
            "{key}"
        );
    }

    // A property path that merely starts with a keyword still serializes.
    let declarations = entries_module(vec![
        PropertyEdit::parse("linksPerSecond", Value::Integer(1)).unwrap(),
    ]);
    let manifest = declarations.manifest_json().unwrap();
    assert_eq!(
        selectors(&load("game_data.json", &manifest)),
        selectors(&declarations)
    );
}

/// An entry named for a binding keyword shares a target body's mapping with the binding of
/// that name, so the serialized form drops the entry and every property edit under it. The
/// name is refused where it is built, which is the only place the consumer is still holding
/// the thing it got wrong.
///
/// A property path that spells a keyword is refused later instead, at serialization.
/// `PropertyEdit::parse` is also how block descent reads an inner key, and a struct field
/// may legitimately be named `links`. That half is
/// [`a_property_path_spelling_a_binding_keyword_refuses_to_serialize`].
#[test]
fn an_entry_name_spelling_a_binding_keyword_cannot_be_built() {
    for key in ["overrides", "links", "+links", "-links"] {
        let error = EntryName::try_from(key).unwrap_err();
        assert!(
            matches!(&error.kind, ErrorKind::ReservedBindingKey { key: k } if k == key),
            "{key}: {error:?}"
        );
    }

    // A name that merely starts with a keyword is an ordinary name.
    assert!(EntryName::try_from("links/Foo").is_ok());
    assert!(EntryName::try_from("linksPerSecond").is_ok());
}

#[test]
fn two_edits_under_one_signed_key_refuse_to_serialize() {
    let declarations = entries_module(vec![
        PropertyEdit::parse("+mFoo", Value::List(vec![Value::Integer(1)])).unwrap(),
        PropertyEdit::parse("+mFoo", Value::List(vec![Value::Integer(2)])).unwrap(),
    ]);
    let error = declarations.manifest_json().unwrap_err();
    assert!(
        matches!(&error.kind, ErrorKind::DuplicatePropertyKey { key } if key == "+mFoo"),
        "{error:?}"
    );
    assert_eq!(error.location.entry.as_deref(), Some("Characters/A"));

    // Two signs on one path are two keys, and both survive.
    let declarations = entries_module(vec![
        PropertyEdit::parse("+mFoo", Value::List(vec![Value::Integer(1)])).unwrap(),
        PropertyEdit::parse("-mFoo", Value::List(vec![Value::Integer(2)])).unwrap(),
    ]);
    let manifest = declarations.manifest_json().unwrap();
    assert_eq!(
        selectors(&load("game_data.json", &manifest)),
        selectors(&declarations)
    );
}

#[test]
fn a_document_refuses_a_duplicate_mapping_key() {
    let text = r#"{"version":1,"modules":[{"entries":{"a/b":{"x":1},"a/b":{"y":2}},"origin":{"manifest":"m","source":null,"module":0}}]}"#;
    let error = serde_json::from_str::<DeclarationDocument>(text).unwrap_err();
    assert!(error.to_string().contains("duplicate key `a/b`"), "{error}");

    // The same document without the duplicate still reads, in spelled order.
    let text = r#"{"version":1,"modules":[{"entries":{"a/b":{"x":1},"c/d":{"y":2}},"origin":{"manifest":"m","source":null,"module":0}}]}"#;
    let document: DeclarationDocument = serde_json::from_str(text).unwrap();
    let Selector::Entries(entries) = &document.parse().unwrap().modules[0].selector else {
        panic!("expected an entries selector");
    };
    assert_eq!(
        entries.keys().map(EntryName::as_str).collect::<Vec<_>>(),
        ["a/b", "c/d"]
    );
}

#[test]
fn an_empty_selector_refuses_to_load_and_to_write() {
    let empty_edits = r#"{"version":1,"modules":[{"target":"a.bin","edits":[],"origin":{"manifest":"m","source":null,"module":0}}]}"#;
    let document: DeclarationDocument = serde_json::from_str(empty_edits).unwrap();
    let error = document.parse().unwrap_err();
    assert!(matches!(error.kind, ErrorKind::EditsEmpty), "{error:?}");

    let empty_entries = r#"{"version":1,"modules":[{"entries":{},"origin":{"manifest":"m","source":null,"module":0}}]}"#;
    let document: DeclarationDocument = serde_json::from_str(empty_entries).unwrap();
    let error = document.parse().unwrap_err();
    assert!(matches!(error.kind, ErrorKind::EntriesEmpty), "{error:?}");

    let declarations = Declarations {
        version: 1,
        modules: vec![Module {
            selector: Selector::Target {
                target: ltk_game_data::Target::try_from("a.bin").unwrap(),
                edits: Vec::new(),
            },
            origin: ltk_game_data::Origin {
                manifest: "game_data.json".into(),
                source: None,
                module_index: 0,
            },
        }],
    };
    let error = declarations.manifest_json().unwrap_err();
    assert!(matches!(error.kind, ErrorKind::EditsEmpty), "{error:?}");
    assert_eq!(error.location.module, Some(0));
}

#[test]
fn an_out_of_range_integer_refuses_to_serialize() {
    let declarations = entries_module(vec![
        PropertyEdit::parse("a", Value::Integer(i128::MAX)).unwrap(),
    ]);
    let error = declarations.manifest_json().unwrap_err();
    assert!(
        matches!(error.kind, ErrorKind::Serialize { .. }),
        "{error:?}"
    );
    let error = DeclarationDocument::try_from(declarations).unwrap_err();
    assert!(
        matches!(error.kind, ErrorKind::Serialize { .. }),
        "{error:?}"
    );
}

#[test]
fn integers_span_the_i64_and_u64_ranges() {
    // A YAML integer past the ranges is the parser's float.
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: 18446744073709551616\n";
    let declarations = load("game_data.yaml", text);
    let body = &target_edits(&declarations.modules[0])[0].entries[0];
    assert!(
        matches!(body[0].value, Value::Float(_)),
        "{:?}",
        body[0].value
    );
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a: 18446744073709551615\n      b: -9223372036854775808\n";
    let declarations = load("game_data.yaml", text);
    let body = &target_edits(&declarations.modules[0])[0].entries[0];
    assert_eq!(body[0].value, Value::Integer(u64::MAX.into()));
    assert_eq!(body[1].value, Value::Integer(i64::MIN.into()));
}

/// A reference is one value however the document spells it.
///
/// The YAML tag and the one-key mapping are the same value, as a type pin's two spellings
/// are, and JSON and TOML have only the mapping.
#[test]
fn a_reference_loads_from_every_format_as_one_value() {
    let expected = Value::Mapping(IndexMap::from([(
        "ref".to_owned(),
        Value::String("Characters/B:speed".to_owned()),
    )]));
    let cases = [
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      speed: !ref Characters/B:speed\n",
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      speed: {ref: \"Characters/B:speed\"}\n",
        ),
        (
            "game_data.json",
            r#"{"version": 1, "modules": [{"target": "a.bin", "Characters/A": {"speed": {"ref": "Characters/B:speed"}}}]}"#,
        ),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = \"a.bin\"\n\"Characters/A\" = { speed = { ref = \"Characters/B:speed\" } }\n",
        ),
    ];
    for (name, text) in cases {
        let declarations = load(name, text);
        let edit = &target_edits(&declarations.modules[0])[0];
        let body = &edit.entries[&EntryName::try_from("Characters/A").unwrap()];
        assert_eq!(body[0].value, expected, "{name}");
        assert_eq!(
            body[0].value.reference(),
            Some("Characters/B:speed"),
            "{name}"
        );
        // A reference is not a pin, whatever spelling it arrived in.
        assert_eq!(body[0].value.pin(), None, "{name}");
    }
}

/// The dotted form reaches a field named `ref`, as it reaches a field named for a type.
#[test]
fn the_dotted_form_escapes_the_reference_key() {
    let text = "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      a.ref: 1\n";
    let declarations = load("game_data.yaml", text);
    let body = &target_edits(&declarations.modules[0])[0].entries
        [&EntryName::try_from("Characters/A").unwrap()];
    assert_eq!(keys(body), ["a.ref"]);
    assert_eq!(body[0].value, Value::Integer(1));
    assert_eq!(body[0].value.reference(), None);
}

/// A module reports the references its edits hold, which is how a build learns it needs the
/// object index before it starts reading chunks.
#[test]
fn a_module_reports_the_references_it_holds() {
    let text = "version: 1\nmodules:\n  - entries:\n      Characters/A:\n        speed: !ref Characters/B:speed\n        +tags: [!ref \"Characters/C:tags[0]\"]\n        name: plain\n";
    let declarations = load("game_data.yaml", text);
    let references: Vec<String> = declarations.modules[0]
        .references()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(references, ["Characters/B:speed", "Characters/C:tags[0]"]);

    let plain = load(
        "game_data.yaml",
        "version: 1\nmodules:\n  - target: a.bin\n    Characters/A:\n      speed: 1\n",
    );
    assert!(plain.modules[0].references().is_empty());
}
