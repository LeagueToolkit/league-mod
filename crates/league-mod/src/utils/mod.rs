use crate::errors::CliError;
use miette::Result;
use regex::Regex;

pub mod modpkg;

pub fn is_valid_slug(name: impl AsRef<str>) -> bool {
    Regex::new(r"^[[:word:]-]+$")
        .unwrap()
        .is_match(name.as_ref())
}

pub fn validate_mod_name(name: impl AsRef<str>) -> Result<()> {
    let name_str = name.as_ref();
    if !is_valid_slug(name_str) {
        return Err(CliError::invalid_mod_name(name_str.to_string(), None).into());
    }

    Ok(())
}

pub fn validate_version_format(version: impl AsRef<str>) -> Result<()> {
    let version_str = version.as_ref();
    if semver::Version::parse(version_str).is_err() {
        return Err(CliError::invalid_version(version_str.to_string(), None).into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_slug_valid() {
        assert!(is_valid_slug("test"));
        assert!(is_valid_slug("test-123"));
        assert!(!is_valid_slug("test 123"));
        assert!(!is_valid_slug("test!123"));
        assert!(!is_valid_slug("Nice mod: ([test])@"));
    }
}
