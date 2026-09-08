use super::*;
use crate::FantomeInfo;

fn parse_version(raw_json: &str) -> String {
    let json = format!(
        r#"{{"Name":"Test Mod","Author":"Alice","Version":{raw_json},"Description":"A test mod"}}"#
    );
    serde_json::from_str::<FantomeInfo>(&json)
        .expect("a Fantome info.json with any Version value parses")
        .version
}

#[test]
fn a_valid_semver_tag_is_kept_verbatim() {
    assert_eq!(sanitize("1.2.3"), "1.2.3");
    assert_eq!(sanitize("0.0.1-beta.1+build.7"), "0.0.1-beta.1+build.7");
}

#[test]
fn surrounding_whitespace_and_a_v_prefix_are_dropped() {
    assert_eq!(sanitize("  1.2.3  "), "1.2.3");
    assert_eq!(sanitize("v1.2.3"), "1.2.3");
    assert_eq!(sanitize("V2.0"), "2.0.0");
    assert_eq!(sanitize("vv1.2.3"), DEFAULT_VERSION);
}

#[test]
fn a_short_tag_is_padded_to_three_segments() {
    assert_eq!(sanitize("1"), "1.0.0");
    assert_eq!(sanitize("1.1"), "1.1.0");
}

#[test]
fn a_tag_with_more_than_three_segments_keeps_the_first_three() {
    assert_eq!(sanitize("1.2.3.1615.2"), "1.2.3");
}

#[test]
fn leading_zeros_are_stripped_from_every_segment() {
    assert_eq!(sanitize("01.02.03"), "1.2.3");
    assert_eq!(sanitize("1.00.0"), "1.0.0");
}

#[test]
fn a_segment_keeps_its_leading_digits_and_drops_the_rest() {
    assert_eq!(sanitize("1.0b"), "1.0.0");
    assert_eq!(sanitize("1.2.3a"), "1.2.3");
}

#[test]
fn a_segment_after_one_without_digits_is_dropped() {
    assert_eq!(sanitize("1.beta.3"), "1.0.0");
}

#[test]
fn pre_release_and_build_metadata_survive_a_rebuilt_core() {
    assert_eq!(sanitize("1.2.3.4-beta"), "1.2.3-beta");
    assert_eq!(sanitize("1.0-rc.1+exp"), "1.0.0-rc.1+exp");
}

#[test]
fn an_unparsable_suffix_is_dropped_and_the_core_survives() {
    assert_eq!(sanitize("1.2.3.4-"), "1.2.3");
    assert_eq!(sanitize("1.2-+"), "1.2.0");
}

#[test]
fn a_tag_without_a_leading_digit_becomes_the_default() {
    assert_eq!(sanitize(""), DEFAULT_VERSION);
    assert_eq!(sanitize("   "), DEFAULT_VERSION);
    assert_eq!(sanitize("no idea"), DEFAULT_VERSION);
    assert_eq!(sanitize("beta.1"), DEFAULT_VERSION);
}

#[test]
fn a_segment_wider_than_a_u64_becomes_the_default() {
    assert_eq!(sanitize("99999999999999999999999.1.0"), DEFAULT_VERSION);
}

#[test]
fn a_floating_point_version_field_parses() {
    assert_eq!(parse_version("1.1"), "1.1.0");
    assert_eq!(parse_version("1.0"), "1.0.0");
}

#[test]
fn a_floating_point_version_field_keeps_one_fractional_digit() {
    assert_eq!(parse_version("1.3333333"), "1.3.0");
    assert_eq!(parse_version("1.25"), "1.2.0");
}

#[test]
fn a_version_string_keeps_every_digit_of_its_minor_segment() {
    assert_eq!(parse_version(r#""1.3333333""#), "1.3333333.0");
    assert_eq!(parse_version(r#""1.25""#), "1.25.0");
}

#[test]
fn an_integer_version_field_parses() {
    assert_eq!(parse_version("2"), "2.0.0");
    assert_eq!(parse_version("-3"), DEFAULT_VERSION);
}

#[test]
fn a_string_version_field_parses() {
    assert_eq!(parse_version(r#""1.2.3.1615.2""#), "1.2.3");
}

#[test]
fn a_version_field_of_any_other_type_becomes_the_default() {
    assert_eq!(parse_version("null"), DEFAULT_VERSION);
    assert_eq!(parse_version("true"), DEFAULT_VERSION);
    assert_eq!(parse_version(r#"["1.0.0"]"#), DEFAULT_VERSION);
    assert_eq!(parse_version(r#"{"Major":1}"#), DEFAULT_VERSION);
}

#[test]
fn an_absent_version_field_becomes_the_default() {
    let json = r#"{"Name":"Test Mod","Author":"Alice","Description":"A test mod"}"#;
    let info = serde_json::from_str::<FantomeInfo>(json)
        .expect("a Fantome info.json without a Version field parses");
    assert_eq!(info.version, DEFAULT_VERSION);
}

#[test]
fn a_sanitized_version_serializes_as_a_string() {
    let info = FantomeInfo {
        version: parse_version("1.1"),
        ..Default::default()
    };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["Version"], serde_json::json!("1.1.0"));
}
