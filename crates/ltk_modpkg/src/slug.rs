use std::fmt::Display;
use std::str::FromStr;

use crate::error::InvalidSlugError;

/// A lowercase, hyphen-separated identifier.
///
/// Layer names use this shape. Validating in the constructor means a layer
/// name that the packer would reject cannot be smuggled in through the
/// builder instead.
///
/// ```
/// use ltk_modpkg::Slug;
///
/// assert!(Slug::new("high-res").is_ok());
/// assert!(Slug::new("High Res").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Slug(String);

impl Slug {
    /// Validate `value` as a slug: non-empty, ASCII lowercase alphanumeric or
    /// hyphens, and not starting or ending with a hyphen.
    pub fn new(value: impl AsRef<str>) -> Result<Self, InvalidSlugError> {
        let value = value.as_ref();

        let valid = !value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !value.starts_with('-')
            && !value.ends_with('-');

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
        for value in ["base", "my-layer", "layer123", "high-res"] {
            assert!(Slug::new(value).is_ok(), "{value} should be valid");
        }
    }

    #[test]
    fn rejects_invalid_slugs() {
        for value in ["", "-invalid", "invalid-", "UPPERCASE", "has spaces"] {
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
