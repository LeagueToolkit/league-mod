use crate::errors::CliError;
use miette::Result;
use regex::Regex;

pub mod config;
pub mod league_path;
pub mod modpkg;
pub mod update;

#[macro_export]
macro_rules! println_pad {
    ($($arg:tt)*) => {{
        let __s = format!($($arg)*);
        for __line in __s.lines() {
            println!("    {}", __line);
        }
    }};
}

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

/// Prints the provided lines inside an ASCII box
pub fn print_ansi_boxed_lines(lines: &[String]) {
    let ansi = Regex::new("\x1b\\[[0-9;]*m").unwrap();
    let visible_len = |s: &str| ansi.replace_all(s, "").chars().count();

    let width = lines
        .iter()
        .map(|s| visible_len(s.as_str()))
        .max()
        .unwrap_or(0);

    let border = "-".repeat(width + 4);
    println_pad!("{}", border);
    for line in lines {
        let pad = width - visible_len(line.as_str());
        println_pad!("| {}{} |", line, " ".repeat(pad));
    }
    println_pad!("{}", border);
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
