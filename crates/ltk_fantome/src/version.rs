//! Sanitizing the `Version` field of a Fantome info.json into semver.
//!
//! The field is hand written across a decade of tools. It holds a JSON number
//! (`"Version": 1.1`), a string with more than three segments
//! (`"Version": "1.2.3.1615.2"`), a `v` prefix, leading zeros, or prose.
//! Deserializing any one of them yields a valid semver string. A number's
//! fractional part is a fraction, not a segment: `1.3333333` names the version
//! between `1` and `2`, and reads as `1.3.0`.

use serde::de::{Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;

/// The version an unreadable field sanitizes to.
const DEFAULT_VERSION: &str = "1.0.0";

/// The number of segments in a semver core: `major.minor.patch`.
const CORE_SEGMENTS: usize = 3;

/// The version a Fantome info.json without a `Version` field carries.
pub(crate) fn default_version() -> String {
    DEFAULT_VERSION.to_string()
}

/// Reads the `Version` field of a Fantome info.json as a semver string.
///
/// A string, a number, `null`, and a value of any other JSON type all
/// deserialize. `sanitize` shapes the value. A value carrying no version at
/// all becomes `1.0.0`.
pub(crate) fn deserialize_version<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(VersionVisitor)
}

struct VersionVisitor;

impl<'de> Visitor<'de> for VersionVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a version string or number")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(sanitize(value))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(sanitize(&value.to_string()))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(sanitize(&value.to_string()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E> {
        Ok(sanitize(&truncate_fraction(&value.to_string())))
    }

    fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(DEFAULT_VERSION.to_string())
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(DEFAULT_VERSION.to_string())
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(DEFAULT_VERSION.to_string())
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(DEFAULT_VERSION.to_string())
    }
}

/// Sanitizes one hand written version tag into a valid semver string.
///
/// A tag that already parses as semver keeps the spelling it is written with.
/// Any other tag is rebuilt. Surrounding whitespace and a leading `v` are
/// dropped. The core keeps its first three dot separated segments, padded to
/// three; a segment keeps its leading digits with leading zeros stripped. The
/// pre-release and build metadata are kept when they parse alongside that
/// core. A tag whose first segment holds no leading digit, and a segment
/// wider than a `u64`, yield `1.0.0`.
fn sanitize(raw: &str) -> String {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_prefix(['v', 'V']).unwrap_or(trimmed);
    if semver::Version::parse(trimmed).is_ok() {
        return trimmed.to_string();
    }

    let suffix_start = trimmed.find(['-', '+']).unwrap_or(trimmed.len());
    let (core, suffix) = trimmed.split_at(suffix_start);
    let Some(core) = sanitize_core(core) else {
        return DEFAULT_VERSION.to_string();
    };

    let with_suffix = format!("{core}{suffix}");
    if semver::Version::parse(&with_suffix).is_ok() {
        return with_suffix;
    }
    if semver::Version::parse(&core).is_ok() {
        return core;
    }
    DEFAULT_VERSION.to_string()
}

/// Cuts the fractional part of a decimal number to one digit.
///
/// A number-typed `Version` field carries a fraction, not a minor version:
/// `1.3333333` names the version between `1` and `2`, and its first
/// fractional digit is the minor version it names. Text holding no `.` is
/// returned as it is.
fn truncate_fraction(text: &str) -> String {
    let Some((integer, fraction)) = text.split_once('.') else {
        return text.to_string();
    };
    match fraction.chars().next() {
        Some(digit) => format!("{integer}.{digit}"),
        None => integer.to_string(),
    }
}

/// Rebuilds the `major.minor.patch` core of a version tag.
///
/// `None` for a core whose first segment holds no leading digit.
fn sanitize_core(core: &str) -> Option<String> {
    let mut segments = Vec::with_capacity(CORE_SEGMENTS);
    for segment in core.split('.').take(CORE_SEGMENTS) {
        let digits: String = segment
            .trim()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.is_empty() {
            break;
        }
        let digits = digits.trim_start_matches('0');
        segments.push(if digits.is_empty() { "0" } else { digits }.to_string());
    }

    if segments.is_empty() {
        return None;
    }
    while segments.len() < CORE_SEGMENTS {
        segments.push("0".to_string());
    }
    Some(segments.join("."))
}

#[cfg(test)]
mod tests;
