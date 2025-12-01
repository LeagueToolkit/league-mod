use std::fs::File;

use crate::errors::CliError;
use crate::println_pad;
use camino::Utf8Path;
use colored::Colorize;
use ltk_fantome::{FantomeExtractError, FantomeExtractor};
use ltk_modpkg::{Modpkg, ModpkgExtractor};
use miette::{IntoDiagnostic, Result};

pub struct ExtractModPackageArgs {
    pub file_path: String,
    pub output_dir: String,
}

pub fn extract_mod_package(args: ExtractModPackageArgs) -> Result<()> {
    let file_path = Utf8Path::new(&args.file_path);

    // Check if it is a fantome file (ends with .fantome)
    if let Some(extension) = file_path.extension() {
        if extension == "fantome" {
            return extract_fantome_package(args);
        }
    }

    let file = File::open(&args.file_path).into_diagnostic()?;
    let mut modpkg = Modpkg::mount_from_reader(file).into_diagnostic()?;

    println_pad!(
        "{} {}",
        "📦 Extracting modpkg:".bright_blue().bold(),
        args.file_path.bright_cyan().bold()
    );

    let output_path = Utf8Path::new(&args.output_dir);
    let mut extractor = ModpkgExtractor::new(&mut modpkg);

    println_pad!(
        "{} {}",
        "📁 Extracting to:".bright_yellow(),
        output_path.as_str().bright_white().bold()
    );
    extractor.extract_all(output_path).into_diagnostic()?;

    println_pad!("{}", "✅ Extraction complete!".bright_green().bold());

    Ok(())
}

fn extract_fantome_package(args: ExtractModPackageArgs) -> Result<()> {
    let file = File::open(&args.file_path).into_diagnostic()?;

    println_pad!(
        "{} {}",
        "👻 Extracting Fantome package:".bright_blue().bold(),
        args.file_path.bright_cyan().bold()
    );

    let output_path = Utf8Path::new(&args.output_dir);
    let mut extractor = FantomeExtractor::new(file).map_err(map_fantome_error)?;

    extractor
        .extract_to(output_path.as_std_path())
        .map_err(map_fantome_error)?;

    println_pad!(
        "{} {}",
        "📁 Extracted and converted to mod project at:".bright_yellow(),
        output_path.as_str().bright_white().bold()
    );

    println_pad!("{}", "✅ Extraction complete!".bright_green().bold());

    Ok(())
}

/// Map FantomeExtractError to CliError for user-friendly error messages.
fn map_fantome_error(err: FantomeExtractError) -> CliError {
    match err {
        FantomeExtractError::RawUnsupported => CliError::FantomeRawUnsupported,
        FantomeExtractError::PackedWadUnsupported { wad_name } => {
            CliError::FantomePackedWadUnsupported { wad_name }
        }
        FantomeExtractError::Io(e) => CliError::IoError { source: e },
        FantomeExtractError::Zip(e) => CliError::IoError {
            source: std::io::Error::other(e),
        },
        FantomeExtractError::Json(e) => CliError::IoError {
            source: std::io::Error::other(e),
        },
        FantomeExtractError::MissingMetadata => CliError::IoError {
            source: std::io::Error::other("Missing info.json metadata file"),
        },
    }
}
