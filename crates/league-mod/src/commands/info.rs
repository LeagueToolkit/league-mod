use std::fs::File;

use crate::println_pad;
use colored::Colorize;
use league_modpkg::Modpkg;
use miette::IntoDiagnostic;

pub struct InfoModPackageArgs {
    pub file_path: String,
}

pub fn info_mod_package(args: InfoModPackageArgs) -> miette::Result<()> {
    let file = File::open(&args.file_path).into_diagnostic()?;
    let modpkg = Modpkg::mount_from_reader(file).into_diagnostic()?;

    println_pad!(
        "{} {}",
        "📦 Modpkg:".bright_blue().bold(),
        modpkg.metadata.name.bright_cyan().bold()
    );
    println_pad!(
        "{} {}",
        "🏷️  Version:".bright_green(),
        modpkg.metadata.version.bright_white().bold()
    );
    println_pad!(
        "{} {}",
        "📝 Description:".bright_yellow(),
        modpkg
            .metadata
            .description
            .unwrap_or("No description".to_string())
            .bright_white()
    );

    println_pad!("\n{}", "🏗️  Layers:".bright_magenta().bold());
    for layer in modpkg.layers.values() {
        println_pad!("   {} {}", "•".bright_cyan(), layer.name.bright_cyan());
    }

    println_pad!("\n{}", "📄 Chunks:".bright_red().bold());
    for chunk in modpkg.chunks.values() {
        println_pad!(
            "   {} {}",
            "•".bright_red().dimmed(),
            modpkg.chunk_paths[&chunk.path_hash].bright_white().dimmed()
        );
    }

    Ok(())
}
