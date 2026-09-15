use ltk_game_data::{apply, load_declarations};

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
        declarations.modules[0].steps[0].add_links[0].as_str(),
        "yes"
    );
}

#[test]
fn duplicate_base_links_keep_the_first_casing() {
    let base = b"PROP\x03\0\0\0\x02\0\0\0\x01\0A\x01\0a\0\0\0\0";
    let output = apply(base, &[]).unwrap();
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
            r#"{"version":1,"modules":[{"target":"shared","links":[],"steps":[] }]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","links":[],"+links":[]}]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":"shared","steps":[{"links":[],"+links":[]}]}]}"#,
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
        (
            "game_data.yaml",
            "version: 1\nmodules:\n- target: shared\n  links: [!unexpected shared]\n",
        ),
        ("game_data.toml", "version = 2\nmodules = []"),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = 'shared'\noverrides = ['a.ptch']",
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
        load_declarations("game_data.toml", toml, |_| unreachable!())
            .unwrap()
            .modules[0]
            .steps[0]
            .add_links[0]
            .as_str(),
        "yes"
    );
    for bytes in [
        b"PTCH\x01\0\0\0".as_slice(),
        b"PROP\x01\0\0\0\0\0\0\0",
        b"PROP\x03\0\0\0",
    ] {
        assert!(apply(bytes, &[]).is_err());
    }
}

#[test]
fn input_discovery_retains_sources_in_rejected_documents() {
    let yaml = "version: 1\nversion: 1\nmodules:\n- target: shared\n  unknown: !f32 1.0\n  source: one.json\n- source: two.json\n";
    assert!(load_declarations("game_data.yaml", yaml, |_| unreachable!()).is_err());
    assert_eq!(
        ltk_game_data::referenced_sources("game_data.yaml", yaml).unwrap(),
        ["one.json", "two.json"]
    );
    let json = r#"{"version":1,"modules":[{"source":"one.json","source":"two.json"}]}"#;
    assert_eq!(
        ltk_game_data::referenced_sources("game_data.json", json).unwrap(),
        ["one.json", "two.json"]
    );
}

#[test]
fn yaml_sources_and_steps_execute_in_order_without_rewriting_objects() {
    let declarations = load_declarations(
        "game_data.yaml",
        r#"
version: 1
modules:
  - target: shared
    source: patches/links.json
  - target: '0123456789abcdef'
    steps:
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
        declarations.modules[0].location.source.as_deref(),
        Some("patches/links.json")
    );
    assert_eq!(
        declarations.modules[1].target.chunk_hash(),
        0x0123456789abcdef
    );

    // PROP v2, dependency "shared", one zero-property object of class 2 and path 1.
    let base = b"PROP\x02\0\0\0\x01\0\0\0\x06\0shared\x01\0\0\0\x02\0\0\0\x06\0\0\0\x01\0\0\0\0\0";
    let output = apply(base, &declarations.modules[0].steps).unwrap();
    assert_eq!(output.dependencies, ["shared", "other"]);
    assert_eq!(&output.bytes[..8], &base[..8]);
    assert_eq!(&output.bytes[27..], &base[20..]);
    assert_eq!(output.diagnostics[0].path, "MISSING");
    assert_eq!(
        output.diagnostics[0].kind,
        ltk_game_data::ApplyDiagnosticKind::LinkRemovalUnmatched
    );
    assert_eq!(output.diagnostics[0].step_index, 0);
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
            let target = &declarations.modules[0].target;
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
            let document = ltk_game_data::DeclarationDocument::from(declarations.clone());
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
            "steps": [{"links": ["Added"], "-links": ["Removed"]}],
            "origin": {"manifest": "game_data.yaml", "source": "links.json", "module": 2}
        }]
    });
    let document: DeclarationDocument = serde_json::from_value(wire.clone()).unwrap();
    let declarations = document.parse().unwrap();
    assert_eq!(declarations.modules[0].location.module_index, 2);
    assert_eq!(
        declarations.modules[0].steps[0].add_links[0].as_str(),
        "Added"
    );
    assert_eq!(
        serde_json::to_value(DeclarationDocument::from(declarations.clone())).unwrap(),
        wire
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&declarations.manifest_json().unwrap()).unwrap(),
        serde_json::json!({"version":1,"modules":[{"target":"shared","steps":[{"links":["Added"],"-links":["Removed"]}]}]})
    );
    let mut unsupported = wire;
    unsupported["modules"][0]["steps"][0]["objects"] = serde_json::json!({});
    let document: DeclarationDocument = serde_json::from_value(unsupported.clone()).unwrap();
    assert_eq!(serde_json::to_value(&document).unwrap(), unsupported);
    assert!(document.parse().is_err());
}
