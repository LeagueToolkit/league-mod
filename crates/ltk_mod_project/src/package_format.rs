//! The archive formats a mod project can be packed into.

use camino::Utf8Path;
use std::fmt;

/// A format a mod project can be packed into for distribution.
///
/// The counterpart to [`ConfigFormat`](crate::ConfigFormat), which is about the
/// project's own config file rather than the package built from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageFormat {
    /// `.modpkg`, the League Toolkit format.
    Modpkg,
    /// `.fantome`, the legacy format. Carries only the base layer.
    Fantome,
}

impl PackageFormat {
    /// Every supported package format.
    pub const ALL: [PackageFormat; 2] = [PackageFormat::Modpkg, PackageFormat::Fantome];

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
    pub fn from_path(path: &Utf8Path) -> Option<Self> {
        Self::from_extension(path.extension()?)
    }

    /// The file extension for this format, without the leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            PackageFormat::Modpkg => "modpkg",
            PackageFormat::Fantome => "fantome",
        }
    }
}

impl fmt::Display for PackageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.extension())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_extension_ignores_case() {
        assert_eq!(
            PackageFormat::from_extension("modpkg"),
            Some(PackageFormat::Modpkg)
        );
        assert_eq!(
            PackageFormat::from_extension("Fantome"),
            Some(PackageFormat::Fantome)
        );
        assert_eq!(PackageFormat::from_extension("zip"), None);
    }

    #[test]
    fn from_path_reads_the_extension() {
        assert_eq!(
            PackageFormat::from_path(Utf8Path::new("out/my-mod_1.0.0.fantome")),
            Some(PackageFormat::Fantome)
        );
        assert_eq!(PackageFormat::from_path(Utf8Path::new("LICENSE")), None);
    }
}
