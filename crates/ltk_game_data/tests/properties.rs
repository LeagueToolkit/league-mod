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
    let cases: [(&str, &str, Expected); 5] = [
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
    let document = DeclarationDocument::from(declarations.clone());
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
    let document = DeclarationDocument::from(declarations.clone());
    assert_eq!(document.parse().unwrap(), declarations);
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
