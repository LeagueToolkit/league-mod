use std::fs::File;

use crate::println_pad;
use colored::Colorize;
use ltk_modpkg::Modpkg;
use miette::IntoDiagnostic;
use serde_json::to_string_pretty;

pub struct InfoModPackageArgs {
    pub file_path: String,
}

pub fn info_mod_package(args: InfoModPackageArgs) -> miette::Result<()> {
    let file = File::open(&args.file_path).into_diagnostic()?;
    let mut modpkg = Modpkg::mount_from_reader(file).into_diagnostic()?;
    let metadata = modpkg.load_metadata().into_diagnostic()?;
    let pretty_metadata = to_string_pretty(&metadata).into_diagnostic()?;

    println_pad!(
        "{} {}",
        "📦 Modpkg:".bright_blue().bold(),
        metadata.name.bright_cyan().bold()
    );
    println_pad!(
        "{} {}",
        "🏷️ Version:".bright_green(),
        metadata.version.bright_white().bold()
    );
    println_pad!(
        "{} {}",
        "📝 Description:".bright_yellow(),
        metadata
            .description
            .unwrap_or("No description".to_string())
            .bright_white()
    );

    println_pad!("\n{}", "🏗️  Layers:".bright_magenta().bold());
    for layer in modpkg.layers.values() {
        // Try to find a matching layer metadata entry (to show description, if present).
        let layer_description = metadata
            .layers
            .iter()
            .find(|lm| lm.name == layer.name)
            .and_then(|lm| lm.description.as_deref());

        match layer_description {
            Some(desc) => println_pad!(
                "   {} {} {} - {}",
                "•".bright_cyan(),
                layer.name.bright_cyan().bold(),
                format!("(priority: {})", layer.priority).dimmed(),
                desc.bright_white()
            ),
            None => println_pad!(
                "   {} {} {}",
                "•".bright_cyan(),
                layer.name.bright_cyan().bold(),
                format!("(priority: {})", layer.priority).dimmed()
            ),
        }
    }

    println_pad!("\n{}", "🧾 Full metadata (JSON):".bright_magenta().bold());
    println_pad!("{}", pretty_metadata);

    Ok(())
}
