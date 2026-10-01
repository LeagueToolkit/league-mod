use std::fmt::Display;
use std::str::FromStr;

use crate::error::InvalidSlugError;

/// Lowercase words joined by hyphens or underscores.
///
/// Layer names use this shape. The constructor validates it, and a layer
/// name the packer rejects cannot enter through the builder either.
///
/// ```
/// use ltk_modpkg::Slug;
///
/// assert!(Slug::new("high-res").is_ok());
/// assert!(Slug::new("item_shop").is_ok());
/// assert!(Slug::new("High Res").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

impl Slug {
    /// Validate `value` as a slug: non-empty, ASCII lowercase letters, digits,
    /// hyphens or underscores, and not starting or ending with a hyphen or an
    /// underscore.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidSlugError> {
        let value = value.as_ref();
        let is_separator = |c: char| c == '-' || c == '_';

        let valid = !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || is_separator(c))
            && !value.starts_with(is_separator)
            && !value.ends_with(is_separator);

        match valid {
            true => Ok(Self(value.to_string())),
            false => Err(InvalidSlugError::new(value)),
        }
    }

    /// The base layer slug, which is always valid.
    pub fn base() -> Self {
        Self(crate::BASE_LAYER_NAME.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the slug, yielding the inner string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl Display for Slug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Slug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Slug {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Slug {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl TryFrom<&str> for Slug {
    type Error = InvalidSlugError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for Slug {
    type Error = InvalidSlugError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl FromStr for Slug {
    type Err = InvalidSlugError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_slugs() {
        for value in [
            "base",
            "my-layer",
            "layer123",
            "high-res",
            "item_shop",
            "high_res-textures",
        ] {
            assert!(Slug::new(value).is_ok(), "{value} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_slugs() {
        for value in [
            "",
            "-invalid",
            "invalid-",
            "_invalid",
            "invalid_",
            "UPPERCASE",
            "has spaces",
        ] {
            assert!(Slug::new(value).is_err(), "{value} should be invalid");
        }
    }

    #[test]
    fn error_names_the_offending_value() {
        let err = Slug::new("High Res").unwrap_err();

        assert!(err.to_string().contains("High Res"));
    }

    #[test]
    fn base_is_the_base_layer_name() {
        assert_eq!(Slug::base().as_str(), crate::BASE_LAYER_NAME);
        assert_eq!(Slug::base(), Slug::new(crate::BASE_LAYER_NAME).unwrap());
    }
}
