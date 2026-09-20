use ltk_game_data::{
    ApplyDiagnosticKind, BinHash, DeclarationDocument, Edit, ErrorKind, Module, NoSchema,
    OverridePath, RecordSkipReason, ReferencedInputs, Selector, SkippedRecord, Target, apply,
    load_declarations,
};
use ltk_meta::{
    BinOverride,
    concrete::{Bin, BinObject, values},
    path::PropertyPath,
};
use std::io::Cursor;

/// An override reader for edits without overrides.
fn no_override(path: &OverridePath) -> Result<Vec<u8>, ltk_game_data::Error> {
    unreachable!("no override is read: {path}")
}

/// The caller with no game: every reference reports `ReferenceMissingEntry`.
fn no_entry(_: &ltk_game_data::EntryName) -> Option<ltk_meta::BinObject> {
    None
}

/// A PROP v3 with one dependency and one object `1` of class `2` whose `speed` is 1.0.
fn base_bin() -> Vec<u8> {
    let bin = Bin::builder()
        .dependency("shared")
        .object(
            BinObject::builder(1u32, 2u32)
                .property(BinHash::from("speed"), values::F32::new(1.0))
                .build(),
        )
        .build();
    let mut cursor = Cursor::new(Vec::new());
    bin.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

/// A PTCH setting `speed` on `object` to `speed`.
fn ptch(object: u32, speed: f32) -> Vec<u8> {
    let patch = BinOverride::builder()
        .set(
            object,
            PropertyPath::new("speed").unwrap(),
            values::F32::new(speed),
        )
        .build();
    let mut cursor = Cursor::new(Vec::new());
    patch.to_writer(&mut cursor).unwrap();
    cursor.into_inner()
}

fn speed_of(bytes: &[u8]) -> f32 {
    let bin = Bin::from_reader(&mut Cursor::new(bytes)).unwrap();
    match bin.objects[&BinHash(1)].resolve(&PropertyPath::new("speed").unwrap()) {
        Ok(ltk_meta::PropertyValueEnum::F32(value)) => value.value,
        other => panic!("unexpected speed: {other:?}"),
    }
}

/// The entry names of an `entries` module, in mapping order.
fn entry_names(module: &Module) -> Vec<&str> {
    match &module.selector {
        Selector::Entries(entries) => entries.keys().map(|name| name.as_str()).collect(),
        _ => panic!("expected an entries selector"),
    }
}

/// The target and edits of a `target` module.
fn target_of(module: &Module) -> (&Target, &[Edit]) {
    match &module.selector {
        Selector::Target { target, edits } => (target, edits),
        _ => panic!("expected a target selector"),
    }
}

#[test]
fn yaml_requires_strings_for_lookup_paths_and_hashes() {
    for target in [
        "123",
        "0123456789012345",
        "true",
        "null",
        "[]",
        "{}",
        "{path: shared}",
        "\"\"",
    ] {
        let text = format!("version: 1\nmodules:\n  - target: {target}\n    links: [shared]\n");
        assert!(
            load_declarations("game_data.yaml", &text, |_| unreachable!()).is_err(),
            "{target}"
        );
    }
    let declarations = load_declarations(
        "game_data.yaml",
        "version: 1\nmodules:\n  - target: on\n    links: [yes]\n",
        |_| unreachable!(),
    )
    .unwrap();
    assert_eq!(
        target_of(&declarations.modules[0]).1[0].links.add[0].as_str(),
        "yes"
    );
}

#[test]
fn duplicate_base_links_keep_the_first_casing() {
    let base = b"PROP\x03\0\0\0\x02\0\0\0\x01\0A\x01\0a\0\0\0\0";
    let output = apply(base, &[], no_override, no_entry, &NoSchema).unwrap();
    assert_eq!(output.dependencies, ["A"]);
}

#[test]
fn formats_reject_duplicate_keys_mixed_bodies_and_unsupported_bindings() {
    let invalid = [
        (
            "game_data.json",
            r#"{"version":1,"version":1,"modules":[]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","links":[],"edits":[] }]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","links":[],"+links":[]}]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","edits":[{"links":[],"+links":[]}]}]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","source":"a.json","links":[] }]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":{"path":"shared","hash":"0123456789abcdef"},"links":[] }]}"#,
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n- target: shared\n  links: []\n  links: []\n",
        ),
        ("game_data.toml", "version = 2\nmodules = []"),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = 'shared'\nmodes = ['Map11/ARAM']",
        ),
    ];
    for (name, text) in invalid {
        assert!(
            load_declarations(name, text, |_| panic!(
                "invalid body must not read a source"
            ))
            .is_err(),
            "{text}"
        );
    }
    let toml = "version = 1\n[[modules]]\ntarget = 'shared'\nlinks = ['yes']";
    assert_eq!(
        target_of(
            &load_declarations("game_data.toml", toml, |_| unreachable!())
                .unwrap()
                .modules[0]
        )
        .1[0]
            .links
            .add[0]
            .as_str(),
        "yes"
    );
    for bytes in [
        b"PTCH\x01\0\0\0".as_slice(),
        b"PROP\x01\0\0\0\0\0\0\0",
        b"PROP\x03\0\0\0",
    ] {
        assert!(apply(bytes, &[], no_override, no_entry, &NoSchema).is_err());
    }
}

#[test]
fn input_discovery_retains_sources_in_rejected_documents() {
    let yaml = "version: 1\nversion: 1\nmodules:\n- target: shared\n  unknown: !f32 1.0\n  source: one.json\n- source: two.json\n";
    assert!(load_declarations("game_data.yaml", yaml, |_| unreachable!()).is_err());
    assert_eq!(
        ReferencedInputs::discover("game_data.yaml", yaml)
            .unwrap()
            .sources,
        ["one.json", "two.json"]
    );
    let json = r#"{"version":1,"modules":[{"source":"one.json","source":"two.json"}]}"#;
    assert_eq!(
        ReferencedInputs::discover("game_data.json", json)
            .unwrap()
            .sources,
        ["one.json", "two.json"]
    );
}

#[test]
fn yaml_sources_and_edits_execute_in_order_without_rewriting_objects() {
    let declarations = load_declarations(
        "game_data.yaml",
        r#"
version: 1
modules:
  - target: shared
    source: patches/links.json
  - target: '0123456789abcdef'
    edits:
      - '+links': [After]
      - '-links': [after]
"#,
        |source| {
            assert_eq!(source, "patches/links.json");
            Ok(r#"{"version":1,"+links":["Shared","other"],"-links":["MISSING"]}"#.to_owned())
        },
    )
    .unwrap();
    assert_eq!(declarations.modules.len(), 2);
    assert_eq!(
        declarations.modules[0].origin.source.as_deref(),
        Some("patches/links.json")
    );
    assert_eq!(
        target_of(&declarations.modules[1]).0.chunk_hash(),
        0x0123456789abcdef
    );

    // PROP v2, dependency "shared", one zero-property object of class 2 and path 1.
    let base = b"PROP\x02\0\0\0\x01\0\0\0\x06\0shared\x01\0\0\0\x02\0\0\0\x06\0\0\0\x01\0\0\0\0\0";
    let output = apply(
        base,
        target_of(&declarations.modules[0]).1,
        no_override,
        no_entry,
        &NoSchema,
    )
    .unwrap();
    assert_eq!(output.dependencies, ["shared", "other"]);
    assert_eq!(&output.bytes[..8], &base[..8]);
    assert_eq!(&output.bytes[27..], &base[20..]);
    assert_eq!(output.diagnostics[0].path, "MISSING");
    assert_eq!(
        output.diagnostics[0].kind,
        ltk_game_data::ApplyDiagnosticKind::LinkRemovalUnmatched
    );
    assert_eq!(output.diagnostics[0].edit_index, 0);
}

#[test]
fn scalar_targets_preserve_identifier_kind_and_spelling() {
    for (value, hash) in [
        ("0123456789ABCDEF", Some(0x0123456789abcdef)),
        ("0123456789012345", Some(0x0123456789012345)),
        ("Data/Characters/Teemo/skin0.bin", None),
        ("shared", None),
        ("literal.ltk.bin", None),
        ("0x0123456789abcdef", None),
        ("0123456789abcdeg", None),
        ("0123456789abcde", None),
    ] {
        for (name, text) in [
            (
                "game_data.json",
                format!(r#"{{"version":1,"modules":[{{"target":"{value}","links":[]}}]}}"#),
            ),
            (
                "game_data.yaml",
                format!("version: 1\nmodules:\n- target: '{value}'\n  links: []\n"),
            ),
            (
                "game_data.toml",
                format!("version = 1\n[[modules]]\ntarget = '{value}'\nlinks = []\n"),
            ),
        ] {
            let declarations = load_declarations(name, &text, |_| unreachable!()).unwrap();
            let target = target_of(&declarations.modules[0]).0;
            assert_eq!(target.as_str(), value);
            assert_eq!(
                *target,
                ltk_game_data::Target::try_from(value.to_owned()).unwrap()
            );
            if let Some(hash) = hash {
                assert_eq!(target.chunk_hash(), hash);
            }
            let manifest = declarations.manifest_json().unwrap();
            let json: serde_json::Value = serde_json::from_str(&manifest).unwrap();
            assert_eq!(json["modules"][0]["target"], value);
            let document =
                ltk_game_data::DeclarationDocument::try_from(declarations.clone()).unwrap();
            assert_eq!(document.parse().unwrap(), declarations);
        }
    }
}

#[test]
fn authored_identifiers_enforce_validity_before_application() {
    use ltk_game_data::{LinkPath, Target};
    assert!(Target::try_from("").is_err());
    assert!(serde_json::from_str::<Target>(r#""""#).is_err());
    let target = Target::try_from("0123456789ABCDEF").unwrap();
    assert_eq!(target.chunk_hash(), 0x0123456789abcdef);
    assert_eq!(target.as_str(), "0123456789ABCDEF");
    assert!(LinkPath::try_from("").is_err());
    for value in [
        serde_json::json!(""),
        serde_json::json!(42),
        serde_json::json!("a".repeat(65536)),
    ] {
        assert!(serde_json::from_value::<LinkPath>(value).is_err());
    }
    assert!(LinkPath::try_from("é".repeat(32768)).is_err());
    let limit = LinkPath::try_from("a".repeat(65535)).unwrap();
    assert_eq!(limit.as_str().len(), 65535);
    let literal = LinkPath::try_from("Data/Literal.ltk.bin").unwrap();
    assert_eq!(literal.as_str(), "Data/Literal.ltk.bin");
    assert_eq!(
        serde_json::to_string(&literal).unwrap(),
        r#""Data/Literal.ltk.bin""#
    );
}

#[test]
fn declaration_documents_preserve_wire_fields_and_refuse_unsupported_bindings() {
    use ltk_game_data::DeclarationDocument;
    let wire = serde_json::json!({
        "version": 1,
        "modules": [{
            "target": "shared",
            "edits": [{"links": ["Added"], "-links": ["Removed"]}],
            "origin": {"manifest": "game_data.yaml", "source": "links.json", "module": 2}
        }]
    });
    let document: DeclarationDocument = serde_json::from_value(wire.clone()).unwrap();
    let declarations = document.parse().unwrap();
    assert_eq!(declarations.modules[0].origin.module_index, 2);
    assert_eq!(
        target_of(&declarations.modules[0]).1[0].links.add[0].as_str(),
        "Added"
    );
    assert_eq!(
        serde_json::to_value(DeclarationDocument::try_from(declarations.clone()).unwrap()).unwrap(),
        wire
    );
    let mut empty = wire.clone();
    empty["modules"][0]["edits"][0] = serde_json::json!({});
    let document: DeclarationDocument = serde_json::from_value(empty).unwrap();
    assert!(
        target_of(&document.parse().unwrap().modules[0]).1[0]
            .links
            .is_empty()
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&declarations.manifest_json().unwrap()).unwrap(),
        serde_json::json!({"version":1,"modules":[{"target":"shared","edits":[{"links":["Added"],"-links":["Removed"]}]}]})
    );
    let mut unsupported = wire;
    unsupported["modules"][0]["edits"][0]["objects"] = serde_json::json!({});
    let document: DeclarationDocument = serde_json::from_value(unsupported.clone()).unwrap();
    assert_eq!(serde_json::to_value(&document).unwrap(), unsupported);
    assert!(document.parse().is_err());
}

#[test]
fn entries_modules_load_in_mapping_order_from_every_format() {
    let yaml = r#"
version: 1
modules:
  - entries:
      Characters/Teemo/Skins/Skin0:
        links: [Shared]
      Characters/Teemo/Skins/Skin0/Resources:
        '-links': [Old]
        '+links': [New]
      Characters/Ahri/Skins/Skin0:
        '-links': [Gone]
"#;
    let toml = r#"
version = 1
[[modules]]
[modules.entries."Characters/Teemo/Skins/Skin0"]
links = ["Shared"]
[modules.entries."Characters/Teemo/Skins/Skin0/Resources"]
"-links" = ["Old"]
"+links" = ["New"]
[modules.entries."Characters/Ahri/Skins/Skin0"]
"-links" = ["Gone"]
"#;
    let json = r#"{"version":1,"modules":[{"entries":{
        "Characters/Teemo/Skins/Skin0":{"links":["Shared"]},
        "Characters/Teemo/Skins/Skin0/Resources":{"-links":["Old"],"+links":["New"]},
        "Characters/Ahri/Skins/Skin0":{"-links":["Gone"]}}}]}"#;
    for (name, text) in [
        ("game_data.yaml", yaml),
        ("game_data.toml", toml),
        ("game_data.json", json),
    ] {
        let declarations = load_declarations(name, text, |_| unreachable!()).unwrap();
        assert_eq!(declarations.modules.len(), 1, "{name}");
        assert_eq!(declarations.modules[0].origin.module_index, 0);
        let Selector::Entries(entries) = &declarations.modules[0].selector else {
            panic!("{name}: expected an entries selector");
        };
        let names: Vec<&str> = entries.keys().map(|name| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Characters/Teemo/Skins/Skin0",
                "Characters/Teemo/Skins/Skin0/Resources",
                "Characters/Ahri/Skins/Skin0",
            ],
            "{name}"
        );
        let edits: Vec<&ltk_game_data::EntryEdit> = entries.values().collect();
        assert_eq!(edits[0].links.add[0].as_str(), "Shared");
        assert!(edits[0].links.remove.is_empty());
        assert_eq!(edits[1].links.remove[0].as_str(), "Old");
        assert_eq!(edits[1].links.add[0].as_str(), "New");
        assert_eq!(edits[2].links.remove[0].as_str(), "Gone");

        let document = ltk_game_data::DeclarationDocument::try_from(declarations.clone()).unwrap();
        let parsed = document.parse().unwrap();
        assert_eq!(parsed, declarations, "{name}");
        assert_eq!(entry_names(&parsed.modules[0]), names, "{name}");
        let manifest = declarations.manifest_json().unwrap();
        let again = load_declarations("game_data.json", &manifest, |_| unreachable!()).unwrap();
        assert_eq!(
            again.modules[0].selector, declarations.modules[0].selector,
            "{name}"
        );
        assert_eq!(entry_names(&again.modules[0]), names, "{name}");
    }
}

#[test]
fn a_module_requires_exactly_one_selector() {
    for text in [
        r#"{"version":1,"modules":[{"target":"a","links":["a"]},{"target":"shared","entries":{"x":{"links":["a"]}}}]}"#,
        r#"{"version":1,"modules":[{"target":"a","links":["a"]},{"edits":[]}]}"#,
        r#"{"version":1,"modules":[{"target":"a","links":["a"]},{"entries":{"x":{"links":["a"]}},"links":["a"]}]}"#,
        r#"{"version":1,"modules":[{"target":"a","links":["a"]},{"entries":{"x":{"links":["a"]}},"source":"a.json"}]}"#,
    ] {
        let error = load_declarations("game_data.json", text, |_| unreachable!()).unwrap_err();
        assert!(error.to_string().contains("module 1"), "{text}: {error}");
    }
    let document: ltk_game_data::DeclarationDocument = serde_json::from_value(serde_json::json!({
        "version": 1,
        "modules": [{
            "target": "shared",
            "entries": {"x": {"links": ["a"]}},
            "origin": {"manifest": "game_data.yaml", "source": null, "module": 4}
        }]
    }))
    .unwrap();
    let error = document.parse().unwrap_err().to_string();
    assert!(error.contains("module 4"), "{error}");
    for module in [
        serde_json::json!({"origin": {"manifest": "game_data.yaml", "source": null, "module": 0}}),
        serde_json::json!({"target": "shared", "origin": {"manifest": "game_data.yaml", "source": null, "module": 0}}),
        serde_json::json!({"entries": {"x": {"links": ["a"]}}, "edits": [], "origin": {"manifest": "game_data.yaml", "source": null, "module": 0}}),
    ] {
        let document: ltk_game_data::DeclarationDocument =
            serde_json::from_value(serde_json::json!({"version": 1, "modules": [module]})).unwrap();
        assert!(document.parse().is_err(), "{module}");
    }
}

#[test]
fn entry_bodies_refuse_sources_unknown_keys_and_empty_names() {
    for text in [
        r#"{"version":1,"modules":[{"entries":{"x":{"source":"a.json"}}}]}"#,
        r#"{"version":1,"modules":[{"target":"a","edits":[]}]}"#,
        r#"{"version":1,"modules":[{"target":"a","edits":[{}]}]}"#,
        r#"{"version":1,"modules":[{"entries":{"x":{}}}]}"#,
        r#"{"version":1,"modules":[{"entries":{"x":{"overrides":["a"]}}}]}"#,
        r#"{"version":1,"modules":[{"entries":{"":{"links":["a"]}}}]}"#,
        r#"{"version":1,"modules":[{"entries":{"x":{"links":["a"]},"x":{"links":["b"]}}}]}"#,
        r#"{"version":1,"modules":[{"entries":{}}]}"#,
    ] {
        assert!(
            load_declarations("game_data.json", text, |_| panic!(
                "an entry body must not read a source"
            ))
            .is_err(),
            "{text}"
        );
    }
    assert!(
        load_declarations(
            "game_data.yaml",
            "version: 1\nmodules:\n- entries:\n    x: {links: [a]}\n    x: {links: [b]}\n",
            |_| unreachable!()
        )
        .is_err()
    );
}

#[test]
fn entry_names_hash_like_bin_objects_and_refuse_the_empty_string() {
    use ltk_game_data::EntryName;
    let name = EntryName::try_from("Characters/TEEMO/Skins/Skin0").unwrap();
    assert_eq!(name.as_str(), "Characters/TEEMO/Skins/Skin0");
    assert_eq!(name.object_hash(), ltk_game_data::BinHash(0x591cbdbd));
    assert_eq!(
        EntryName::try_from("characters/teemo/skins/skin0".to_owned())
            .unwrap()
            .object_hash(),
        name.object_hash()
    );
    assert_eq!(String::from(name.clone()), "Characters/TEEMO/Skins/Skin0");
    assert_eq!(
        serde_json::to_string(&name).unwrap(),
        r#""Characters/TEEMO/Skins/Skin0""#
    );
    let hashed = EntryName::try_from("0x591CBDBD").unwrap();
    assert_eq!(hashed.object_hash(), name.object_hash());
    assert_eq!(hashed.as_str(), "0x591CBDBD");
    assert_eq!(hashed.to_string(), "0x591CBDBD");
    for path in [
        "0x591cbdb",
        "0x591cbdbd1",
        "591cbdbd",
        "0X591CBDBD",
        "0x591cbdbg",
    ] {
        assert_ne!(
            EntryName::try_from(path).unwrap().object_hash(),
            ltk_game_data::BinHash(0x591cbdbd),
            "{path}"
        );
    }
    assert!(EntryName::try_from("").is_err());
    assert!(EntryName::try_from(String::new()).is_err());
    assert!(serde_json::from_str::<EntryName>(r#""""#).is_err());
    assert!(serde_json::from_str::<EntryName>("1").is_err());
}

#[test]
fn override_paths_enforce_spelling_and_extension() {
    for value in ["a/b.ptch", "X.PTCH", "Test.wad.client/patch.ptch"] {
        assert_eq!(OverridePath::try_from(value).unwrap().as_str(), value);
    }
    for value in [
        "",
        "a\\b.ptch",
        "/a.ptch",
        "../a.ptch",
        "a/../b.ptch",
        "a/./b.ptch",
        "a//b.ptch",
        ".ptch",
        "a.bin",
        "a.ptch/",
    ] {
        assert!(OverridePath::try_from(value).is_err(), "{value:?}");
    }
    let rito = OverridePath::try_from("a.rito").unwrap_err();
    assert!(rito.to_string().contains(".rito"), "{rito}");
}

#[test]
fn override_paths_resolve_against_their_source_and_stay_within_the_layer() {
    let declarations = load_declarations(
        "game_data.yaml",
        "version: 1\nmodules:\n  - target: shared\n    overrides: [patches/./a.ptch]\n  - target: other\n    source: Test.wad.client/links.yaml\n  - target: third\n    source: ./patches/links.yaml\n",
        |source| {
            Ok(match source {
                "./patches/links.yaml" => "version: 1\noverrides: [../dotted.ptch]\n".to_owned(),
                _ => "version: 1\noverrides: [../patch.ptch, sub/../local.ptch]\nlinks: [x]\n"
                    .to_owned(),
            })
        },
    )
    .unwrap();
    let paths = |module: &Module| -> Vec<String> {
        target_of(module).1[0]
            .overrides
            .iter()
            .map(|path| path.as_str().to_owned())
            .collect()
    };
    assert_eq!(paths(&declarations.modules[0]), ["patches/a.ptch"]);
    assert_eq!(
        paths(&declarations.modules[1]),
        ["patch.ptch", "Test.wad.client/local.ptch"]
    );
    assert_eq!(paths(&declarations.modules[2]), ["dotted.ptch"]);
    for authored in ["../../patch.ptch", "/patch.ptch", "a\\b.ptch", "a.rito", ""] {
        let error = load_declarations(
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: shared\n    source: Test.wad.client/links.yaml\n",
            |_| Ok(format!("version: 1\noverrides: [{authored:?}]\n")),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("module 0"),
            "{authored}: {error}"
        );
    }
    assert!(
        load_declarations(
            "game_data.yaml",
            "version: 1\nmodules:\n  - target: shared\n    overrides: []\n",
            |_| unreachable!(),
        )
        .is_ok()
    );
}

#[test]
fn overrides_inside_entries_are_errors() {
    for (name, text) in [
        (
            "game_data.yaml",
            "version: 1\nmodules:\n  - entries:\n      Characters/Teemo:\n        overrides: [a.ptch]\n".to_owned(),
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"entries":{"Characters/Teemo":{"overrides":["a.ptch"]}}}]}"#.to_owned(),
        ),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\n[modules.entries.\"Characters/Teemo\"]\noverrides = [\"a.ptch\"]\n".to_owned(),
        ),
    ] {
        let error = load_declarations(name, &text, |_| unreachable!()).unwrap_err();
        assert!(error.to_string().contains("overrides"), "{name}: {error}");
    }
    let document: DeclarationDocument = serde_json::from_str(
        r#"{"version":1,"modules":[{"entries":{"Characters/Teemo":{"overrides":["a.ptch"]}},"origin":{"manifest":"game_data.json","source":null,"module":0}}]}"#,
    )
    .unwrap();
    assert!(document.parse().is_err());
}

#[test]
fn overrides_rewrite_objects_and_report_skipped_records() {
    let base = base_bin();
    let mut v2 = base.clone();
    v2[4] = 2;
    let declarations = load_declarations(
        "game_data.json",
        r#"{"version":1,"modules":[{"target":"shared","edits":[
            {"overrides":["first.ptch","second.ptch"],"links":["Added"]},
            {"overrides":["third.ptch"]}
        ]}]}"#,
        |_| unreachable!(),
    )
    .unwrap();
    let mut reads = Vec::new();
    let output = apply(
        &v2,
        target_of(&declarations.modules[0]).1,
        |path| {
            reads.push(path.as_str().to_owned());
            Ok(match path.as_str() {
                "first.ptch" => ptch(1, 2.0),
                "second.ptch" => ptch(9, 3.0),
                "third.ptch" => ptch(1, 4.0),
                other => panic!("{other}"),
            })
        },
        no_entry,
        &NoSchema,
    )
    .unwrap();
    assert_eq!(reads, ["first.ptch", "second.ptch", "third.ptch"]);
    assert_eq!(&output.bytes[..8], b"PROP\x03\0\0\0");
    assert_eq!(speed_of(&output.bytes), 4.0);
    assert_eq!(output.dependencies, ["shared", "Added"]);
    assert_eq!(output.diagnostics.len(), 1, "{:?}", output.diagnostics);
    let skipped = &output.diagnostics[0];
    assert_eq!(skipped.kind, ApplyDiagnosticKind::OverrideRecordSkipped);
    assert_eq!(skipped.edit_index, 0);
    assert_eq!(skipped.path, "second.ptch");
    assert_eq!(
        skipped.record,
        Some(SkippedRecord {
            index: 0,
            object: BinHash(9),
            property: "speed".into(),
            reason: RecordSkipReason::MissingObject,
        })
    );
    let json = serde_json::to_value(skipped).unwrap();
    assert_eq!(json["kind"], "overrideRecordSkipped");
    assert_eq!(json["record"]["reason"], "missingObject");
}

#[test]
fn unavailable_overrides_are_reported_and_links_still_apply() {
    let base = base_bin();
    let declarations = load_declarations(
        "game_data.json",
        r#"{"version":1,"modules":[{"target":"shared","overrides":["missing.ptch","prop.ptch"],"links":["Added"]}]}"#,
        |_| unreachable!(),
    )
    .unwrap();
    let output = apply(
        &base,
        target_of(&declarations.modules[0]).1,
        |path| match path.as_str() {
            "missing.ptch" => Err(ltk_game_data::Error::in_document(
                ltk_game_data::ErrorKind::InputMissing,
                path.as_str(),
            )),
            _ => Ok(base_bin()),
        },
        no_entry,
        &NoSchema,
    )
    .unwrap();
    assert_eq!(output.dependencies, ["shared", "Added"]);
    assert_eq!(
        &output.bytes[output.bytes.len() - 27..],
        &base[base.len() - 27..]
    );
    let kinds: Vec<_> = output
        .diagnostics
        .iter()
        .map(|d| (d.kind, d.path.as_str(), d.edit_index))
        .collect();
    assert_eq!(
        kinds,
        [
            (ApplyDiagnosticKind::OverrideUnreadable, "missing.ptch", 0),
            (ApplyDiagnosticKind::OverrideInvalid, "prop.ptch", 0),
        ]
    );
    assert!(output.diagnostics.iter().all(|d| d.record.is_none()));
}

#[test]
fn documents_round_trip_overrides_and_manifests_write_them() {
    let declarations = load_declarations(
        "game_data.json",
        r#"{"version":1,"modules":[{"target":"shared","edits":[{"overrides":["a.ptch"]},{"links":["x"]}]}]}"#,
        |_| unreachable!(),
    )
    .unwrap();
    let document = DeclarationDocument::try_from(declarations.clone()).unwrap();
    let json = serde_json::to_string(&document).unwrap();
    assert!(json.contains(r#""overrides":["a.ptch"]"#), "{json}");
    assert_eq!(document.parse().unwrap(), declarations);
    let manifest = declarations.manifest_json().unwrap();
    assert!(manifest.contains(r#""overrides": ["#), "{manifest}");
    let reloaded = load_declarations("game_data.json", &manifest, |_| unreachable!()).unwrap();
    assert_eq!(
        target_of(&reloaded.modules[0]).1,
        target_of(&declarations.modules[0]).1
    );
    assert!(
        serde_json::from_str::<DeclarationDocument>(
            r#"{"version":1,"modules":[{"target":"shared","edits":[{"overrides":["../a.ptch"]}],"origin":{"manifest":"m","source":null,"module":0}}]}"#
        )
        .unwrap()
        .parse()
        .is_err()
    );
}

#[test]
fn override_discovery_retains_paths_in_rejected_documents() {
    let yaml = "version: 1\nversion: 1\nmodules:\n- target: shared\n  overrides: [a.ptch]\n  unknown: 1\n- target: other\n  edits:\n    - overrides: [b.ptch, c.ptch]\n";
    assert!(load_declarations("game_data.yaml", yaml, |_| unreachable!()).is_err());
    assert_eq!(
        ReferencedInputs::discover("game_data.yaml", yaml)
            .unwrap()
            .overrides,
        ["a.ptch", "b.ptch", "c.ptch"]
    );
    let source = r#"{"version":1,"overrides":["d.ptch"],"edits":[{"overrides":["e.ptch"]}]}"#;
    assert_eq!(
        ReferencedInputs::discover("source.json", source).unwrap(),
        ReferencedInputs {
            sources: vec![],
            overrides: vec!["d.ptch".into(), "e.ptch".into()],
        }
    );
}

#[test]
fn override_discovery_reaches_an_overrides_list_inside_an_entries_entry() {
    let yaml = "version: 1\nmodules:\n- entries:\n    'a/b':\n      overrides: [inside.ptch]\n";
    let error = load_declarations("game_data.yaml", yaml, |_| unreachable!()).unwrap_err();
    assert!(matches!(error.kind, ErrorKind::OverridesInEntry));
    assert_eq!(
        ReferencedInputs::discover("game_data.yaml", yaml)
            .unwrap()
            .overrides,
        ["inside.ptch"]
    );
}

#[test]
fn a_document_extension_is_compared_without_case() {
    let yaml = "version: 1\nmodules:\n- target: shared\n  links: [a]\n";
    assert!(load_declarations("game_data.YAML", yaml, |_| unreachable!()).is_ok());
    assert!(load_declarations("game_data.YML", yaml, |_| unreachable!()).is_ok());
    let json = r#"{"version":1,"modules":[{"target":"shared","links":["a"]}]}"#;
    assert!(load_declarations("game_data.JSON", json, |_| unreachable!()).is_ok());
    let manifest = "version: 1\nmodules:\n- target: shared\n  source: Shared.YAML\n";
    assert!(
        load_declarations("game_data.yaml", manifest, |name| {
            assert_eq!(name, "Shared.YAML");
            Ok("version: 1\nlinks: [a]\n".to_string())
        })
        .is_ok()
    );
    let error = load_declarations("game_data.rito", yaml, |_| unreachable!()).unwrap_err();
    assert!(matches!(error.kind, ErrorKind::UnknownFormat));
}

#[test]
fn a_json_error_span_points_at_the_offending_byte_of_a_multibyte_line() {
    let text = "{\n  \"version\": 1,\n  \"modules\": [{\"target\": \"\u{e9}\u{e9}\u{e9}\u{e9}\", \"links\": }]\n}";
    let error = load_declarations("game_data.json", text, |_| unreachable!()).unwrap_err();
    let span = error.location.span.expect("a syntax error carries a span");
    assert_eq!(&text[span.start..span.end], "}");
    assert!(span.end <= text.len());
}

#[test]
fn an_override_path_refuses_a_drive_prefix() {
    for spelled in ["C:/evil.ptch", "C:evil.ptch", "c:\u{5c}evil.ptch", "C:"] {
        assert!(
            OverridePath::try_from(spelled).is_err(),
            "accepted `{spelled}`"
        );
    }
    assert!(OverridePath::try_from("a/b.ptch").is_ok());
}

#[test]
fn a_written_manifest_loads_to_the_same_declarations() {
    let declarations = load_declarations(
        "game_data.json",
        r#"{"version":1,"modules":[
            {"target":"shared","edits":[{"overrides":["a.ptch"],"+links":["x"],"-links":["y"]},{"links":[]}]},
            {"entries":{"Characters/A":{"links":["s"]},"0x0000abcd":{"-links":["t"]}}},
            {"target":"0123456789ABCDEF","overrides":["b.ptch"]}
        ]}"#,
        |_| unreachable!(),
    )
    .unwrap();
    let manifest = declarations.manifest_json().unwrap();
    assert_eq!(
        manifest,
        r#"{
  "version": 1,
  "modules": [
    {
      "target": "shared",
      "edits": [
        {
          "overrides": [
            "a.ptch"
          ],
          "links": [
            "x"
          ],
          "-links": [
            "y"
          ]
        },
        {
          "links": []
        }
      ]
    },
    {
      "entries": {
        "Characters/A": {
          "links": [
            "s"
          ]
        },
        "0x0000abcd": {
          "links": [],
          "-links": [
            "t"
          ]
        }
      }
    },
    {
      "target": "0123456789ABCDEF",
      "edits": [
        {
          "overrides": [
            "b.ptch"
          ],
          "links": []
        }
      ]
    }
  ]
}"#
    );
    let reloaded = load_declarations("game_data.json", &manifest, |_| unreachable!()).unwrap();
    assert_eq!(reloaded, declarations);
}

#[test]
fn errors_carry_codes_and_typed_locations() {
    use ltk_game_data::{ErrorKind, Location, Span};

    let text = "version: 1\nmodules:\n  - target: a.bin\n    entries: {}\n";
    let error = load_declarations("game_data.yaml", text, |_| unreachable!()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::SelectorConflict);
    assert_eq!(
        *error.location,
        Location {
            document: Some("game_data.yaml".into()),
            module: Some(0),
            ..Location::default()
        }
    );

    let text = "version: 1\nmodules:\n  - target: a.bin\n    source: shared.yaml\n";
    let error = load_declarations("game_data.yaml", text, |source| {
        Ok(match source {
            "shared.yaml" => "version: 1\nedits:\n  - links: [x]\n  - {}\n".to_owned(),
            _ => unreachable!(),
        })
    })
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::EditWithoutBindings);
    assert_eq!(error.location.document.as_deref(), Some("shared.yaml"));
    assert_eq!(error.location.module, Some(0));
    assert_eq!(error.location.edit, Some(1));
    assert_eq!(
        error.to_string(),
        "shared.yaml: module 0: edit 1: edit requires at least one binding"
    );

    let error = load_declarations(
        "game_data.yaml",
        "version: 1\nmodules: [",
        |_| unreachable!(),
    )
    .unwrap_err();
    assert!(matches!(error.kind, ErrorKind::Syntax { .. }), "{error}");
    assert!(error.location.span.is_some(), "{error}");
    let error = load_declarations(
        "game_data.json",
        r#"{"version": 1, "modules": [}"#,
        |_| unreachable!(),
    )
    .unwrap_err();
    assert_eq!(error.location.span, Some(Span { start: 27, end: 28 }));
    let error = load_declarations(
        "game_data.toml",
        "version = 1\nmodules = [",
        |_| unreachable!(),
    )
    .unwrap_err();
    assert!(error.location.span.is_some(), "{error}");

    let error = Target::try_from("").unwrap_err();
    assert_eq!(error.kind, ErrorKind::EmptyTarget);
    assert_eq!(error.location.key.as_deref(), Some("target"));
}
