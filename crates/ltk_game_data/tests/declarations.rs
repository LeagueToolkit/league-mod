use ltk_game_data::{compile, materialise};

#[test]
fn yaml_requires_strings_for_lookup_paths_and_hashes() {
    for target in ["{ path: 123 }", "{ hash: 0123456789012345 }"] {
        let text = format!("version: 1\nmodules:\n  - target: {target}\n    links: [shared]\n");
        assert!(
            compile("game_data.yaml", &text, |_| unreachable!()).is_err(),
            "{target}"
        );
    }
    let program = compile(
        "game_data.yaml",
        "version: 1\nmodules:\n  - target: { path: on }\n    links: [yes]\n",
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
            r#"{"version":1,"modules":[{"target":{"path":"shared"},"links":[],"steps":[] }]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":{"path":"shared"},"source":"a.json","links":[] }]}"#,
        ),
        (
            "game_data.json",
            r#"{"version":1,"modules":[{"target":{"path":"shared","hash":"0123456789abcdef"},"links":[] }]}"#,
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n- target: {path: shared}\n  links: []\n  links: []\n",
        ),
        (
            "game_data.yaml",
            "version: 1\nmodules:\n- target: {path: shared}\n  links: [!unexpected shared]\n",
        ),
        ("game_data.toml", "version = 2\nmodules = []"),
        (
            "game_data.toml",
            "version = 1\n[[modules]]\ntarget = {path = 'shared'}\noverrides = ['a.ptch']",
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
    let toml = "version = 1\n[[modules]]\ntarget = {\n path = 'shared',\n}\nlinks = ['yes']";
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
    let yaml = "version: 1\nversion: 1\nmodules:\n- target: {path: shared}\n  unknown: !f32 1.0\n  source: one.json\n- source: two.json\n";
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
  - target: { path: shared }
    source: patches/links.json
  - target: { hash: '0123456789abcdef' }
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
