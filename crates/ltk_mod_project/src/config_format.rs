//! The config file formats a mod project can be stored in.

use camino::Utf8Path;
use std::fmt;

/// A format a mod project configuration can be read from and written to.
///
/// Exists so that loading and saving dispatch on one value instead of matching
/// on file extensions in two places that can drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigFormat {
    /// `mod.config.json`.
    Json,
    /// `mod.config.toml`.
    Toml,
}

impl ConfigFormat {
    /// Every supported format, in the order a project directory is searched.
    pub const ALL: [ConfigFormat; 2] = [ConfigFormat::Json, ConfigFormat::Toml];

    /// The format an extension names, matched case-insensitively.
    ///
    /// The extension is given without a leading dot, as
    /// [`Utf8Path::extension`] returns it.
    pub fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.extension().eq_ignore_ascii_case(extension))
    }

    /// The format a path's extension names.
    ///
    /// Returns `None` for a path with no extension as well as for one this
    /// crate cannot read.
    pub fn from_path(path: &Utf8Path) -> Option<Self> {
        Self::from_extension(path.extension()?)
    }

    /// The file extension for this format, without the leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            ConfigFormat::Json => "json",
            ConfigFormat::Toml => "toml",
        }
    }

    /// The config file name a project directory is searched for.
    pub fn file_name(self) -> &'static str {
        match self {
            ConfigFormat::Json => "mod.config.json",
            ConfigFormat::Toml => "mod.config.toml",
        }
    }
}

impl fmt::Display for ConfigFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ConfigFormat::Json => "JSON",
            ConfigFormat::Toml => "TOML",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_extension_ignores_case() {
        assert_eq!(
            ConfigFormat::from_extension("json"),
            Some(ConfigFormat::Json)
        );
        assert_eq!(
            ConfigFormat::from_extension("JSON"),
            Some(ConfigFormat::Json)
        );
        assert_eq!(
            ConfigFormat::from_extension("Toml"),
            Some(ConfigFormat::Toml)
        );
        assert_eq!(ConfigFormat::from_extension("yaml"), None);
    }

    #[test]
    fn from_path_reads_the_extension() {
        assert_eq!(
            ConfigFormat::from_path(Utf8Path::new("a/mod.config.toml")),
            Some(ConfigFormat::Toml)
        );
        assert_eq!(ConfigFormat::from_path(Utf8Path::new("LICENSE")), None);
    }

    /// The searched file name and the extension must agree, since one is used
    /// to find a config and the other to decide how to parse it.
    #[test]
    fn file_name_ends_with_extension() {
        for format in ConfigFormat::ALL {
            assert_eq!(
                ConfigFormat::from_path(Utf8Path::new(format.file_name())),
                Some(format)
            );
        }
    }
}
