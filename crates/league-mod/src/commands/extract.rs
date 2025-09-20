use std::fs::File;
use std::path::Path;

use colored::Colorize;
use miette::IntoDiagnostic;
use league_modpkg::{Modpkg, ModpkgExtractor};

pub struct ExtractModPackageArgs {
    pub file_path: String,
    pub output_dir: String,
}

pub fn extract_mod_package(args: ExtractModPackageArgs) -> miette::Result<()> {
    let file = File::open(&args.file_path).into_diagnostic()?;
    let mut modpkg = Modpkg::mount_from_reader(file).into_diagnostic()?;

    println!(
        "{} {}",
        "📦 Extracting modpkg:".bright_blue().bold(),
        args.file_path.bright_cyan().bold()
    );

    let output_path = Path::new(&args.output_dir);
    let mut extractor = ModpkgExtractor::new(&mut modpkg);

    println!(
        "{} {}",
        "📁 Extracting to:".bright_yellow(),
        output_path.display().to_string().bright_white().bold()
    );
    extractor.extract_all(output_path).into_diagnostic()?;

    println!("{}", "✅ Extraction complete!".bright_green().bold());

    Ok(())
}
