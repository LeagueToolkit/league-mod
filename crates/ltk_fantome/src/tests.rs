use super::*;

fn info_json(license: Option<FantomeLicense>) -> serde_json::Value {
    let info = FantomeInfo {
        name: "Test Mod".to_string(),
        author: "Alice".to_string(),
        version: "1.0.0".to_string(),
        description: "A test mod".to_string(),
        license,
        tags: vec![],
        champions: vec![],
        maps: vec![],
        layers: HashMap::new(),
    };
    serde_json::to_value(&info).unwrap()
}

#[test]
fn info_json_emits_spdx_license() {
    let json = info_json(Some(FantomeLicense::Spdx("MIT".to_string())));
    assert_eq!(json["License"], serde_json::json!("MIT"));
}

#[test]
fn info_json_emits_custom_license() {
    let json = info_json(Some(FantomeLicense::Custom {
        name: "My License".to_string(),
        url: Some("https://example.com/terms".to_string()),
    }));
    assert_eq!(
        json["License"],
        serde_json::json!({ "Name": "My License", "Url": "https://example.com/terms" })
    );

    let json = info_json(Some(FantomeLicense::Custom {
        name: "My License".to_string(),
        url: None,
    }));
    assert_eq!(json["License"], serde_json::json!({ "Name": "My License" }));
}

#[test]
fn info_json_omits_absent_license() {
    let json = info_json(None);
    assert!(
        json.get("License").is_none(),
        "License key must be omitted entirely, got: {json}"
    );
}

#[test]
fn legacy_info_json_without_license_still_parses() {
    let legacy = r#"{
            "Name": "Old Mod",
            "Author": "Someone",
            "Version": "1.0.0",
            "Description": "Packed before licenses existed"
        }"#;

    let info: FantomeInfo = serde_json::from_str(legacy).unwrap();

    assert_eq!(info.name, "Old Mod");
    assert_eq!(info.license, None);
}

#[test]
fn custom_license_rejects_unknown_field() {
    let typoed = r#"{
            "Name": "Test",
            "Author": "Test",
            "Version": "1.0.0",
            "Description": "Test",
            "License": { "Name": "My License", "Ur1": "https://example.com/terms" }
        }"#;

    assert!(
        serde_json::from_str::<FantomeInfo>(typoed).is_err(),
        "a misspelled license key must not parse as a URL-less license"
    );
}
