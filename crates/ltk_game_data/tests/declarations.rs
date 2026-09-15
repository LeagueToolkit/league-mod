use ltk_game_data::{compile, materialise};

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
            compile("game_data.yaml", &text, |_| unreachable!()).is_err(),
            "{target}"
        );
    }
    let program = compile(
        "game_data.yaml",
        "version: 1\nmodules:\n  - target: on\n    links: [yes]\n",
        |_| unreachable!(),
    )
    .unwrap();
    assert_eq!(program.modules[0].steps[0].links, ["yes"]);
}

#[test]
fn duplicate_base_links_keep_the_first_casing() {
    let base = b"PROP\x03\0\0\0\x02\0\0\0\x01\0A\x01\0a\0\0\0\0";
    let output = materialise(base, &[]).unwrap();
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
            compile(name, text, |_| panic!(
                "invalid body must not read a source"
            ))
            .is_err(),
            "{text}"
        );
    }
    let toml = "version = 1\n[[modules]]\ntarget = 'shared'\nlinks = ['yes']";
    assert_eq!(
        compile("game_data.toml", toml, |_| unreachable!())
            .unwrap()
            .modules[0]
            .steps[0]
            .links,
        ["yes"]
    );
    for bytes in [
        b"PTCH\x01\0\0\0".as_slice(),
        b"PROP\x01\0\0\0\0\0\0\0",
        b"PROP\x03\0\0\0",
    ] {
        assert!(materialise(bytes, &[]).is_err());
    }
}

#[test]
fn input_discovery_retains_sources_in_rejected_documents() {
    let yaml = "version: 1\nversion: 1\nmodules:\n- target: shared\n  unknown: !f32 1.0\n  source: one.json\n- source: two.json\n";
    assert!(compile("game_data.yaml", yaml, |_| unreachable!()).is_err());
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
    let program = compile(
        "game_data.yaml",
        r#"
version: 1
modules:
  - target: shared
    source: patches/links.json
  - target: '0123456789abcdef'
    steps:
      - links: [After]
      - '-links': [after]
"#,
        |source| {
            assert_eq!(source, "patches/links.json");
            Ok(r#"{"version":1,"links":["Shared","other"],"-links":["MISSING"]}"#.to_owned())
        },
    )
    .unwrap();
    assert_eq!(program.modules.len(), 2);
    assert_eq!(
        program.modules[0].origin.source.as_deref(),
        Some("patches/links.json")
    );
    assert_eq!(
        program.modules[1].target.hash().unwrap(),
        0x0123456789abcdef
    );

    // PROP v2, dependency "shared", one zero-property object of class 2 and path 1.
    let base = b"PROP\x02\0\0\0\x01\0\0\0\x06\0shared\x01\0\0\0\x02\0\0\0\x06\0\0\0\x01\0\0\0\0\0";
    let output = materialise(base, &program.modules[0].steps).unwrap();
    assert_eq!(output.dependencies, ["shared", "other"]);
    assert_eq!(&output.bytes[..8], &base[..8]);
    assert_eq!(&output.bytes[27..], &base[20..]);
    assert_eq!(output.reports[0].path, "MISSING");
    assert_eq!(output.reports[0].step, 0);
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
            let program = compile(name, &text, |_| unreachable!()).unwrap();
            let target = &program.modules[0].target;
            assert_eq!(target.display_name(), value);
            assert_eq!(*target, ltk_game_data::Target::from(value.to_owned()));
            if let Some(hash) = hash {
                assert_eq!(target.hash().unwrap(), hash);
            }
            let manifest = program.manifest_json().unwrap();
            let json: serde_json::Value = serde_json::from_str(&manifest).unwrap();
            assert_eq!(json["modules"][0]["target"], value);
            let document = ltk_game_data::Document::from(program.clone());
            assert_eq!(document.program().unwrap(), program);
        }
    }
}
