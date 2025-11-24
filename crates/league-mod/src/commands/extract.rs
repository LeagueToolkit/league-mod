use std::fs::File;
use std::io::{Read, Write};

use crate::println_pad;
use camino::Utf8Path;
use colored::Colorize;
use ltk_fantome::FantomeInfo;
use ltk_mod_project::{ModProject, ModProjectAuthor};
use ltk_modpkg::{Modpkg, ModpkgExtractor};
use miette::{IntoDiagnostic, Result};
use zip::ZipArchive;

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
    let mut archive = ZipArchive::new(file).into_diagnostic()?;

    println_pad!(
        "{} {}",
        "👻 Extracting Fantome package:".bright_blue().bold(),
        args.file_path.bright_cyan().bold()
    );

    let output_path = Utf8Path::new(&args.output_dir);

    // Read metadata
    let mut info_file = archive.by_name("META/info.json").into_diagnostic()?;
    let mut info_content = String::new();
    info_file
        .read_to_string(&mut info_content)
        .into_diagnostic()?;
    drop(info_file);

    let info: FantomeInfo = serde_json::from_str(&info_content).into_diagnostic()?;

    // Create mod project
    let mod_project = ModProject {
        name: slug::slugify(&info.name),
        display_name: info.name,
        version: info.version,
        description: info.description,
        authors: vec![ModProjectAuthor::Name(info.author)],
        license: None,
        transformers: vec![],
        layers: ltk_mod_project::default_layers(),
        thumbnail: None, // Will set if image exists
    };

    // Create output directory
    if !output_path.exists() {
        std::fs::create_dir_all(output_path).into_diagnostic()?;
    }

    // Extract files
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).into_diagnostic()?;
        let file_name = file.name().to_string();

        if file_name.starts_with("WAD/") {
            // Extract WAD content to content/base/
            let relative_path = file_name.strip_prefix("WAD/").unwrap();
            let output_file_path = output_path.join("content").join("base").join(relative_path);

            if file.is_dir() {
                std::fs::create_dir_all(&output_file_path).into_diagnostic()?;
            } else {
                if let Some(parent) = output_file_path.parent() {
                    std::fs::create_dir_all(parent).into_diagnostic()?;
                }
                let mut outfile = File::create(&output_file_path).into_diagnostic()?;
                std::io::copy(&mut file, &mut outfile).into_diagnostic()?;
            }
        } else if file_name == "META/README.md" {
            // Extract README
            let output_file_path = output_path.join("README.md");
            let mut outfile = File::create(&output_file_path).into_diagnostic()?;
            std::io::copy(&mut file, &mut outfile).into_diagnostic()?;
        } else if file_name == "META/image.png" {
            // Extract thumbnail
            let output_file_path = output_path.join("thumbnail.png");
            let mut outfile = File::create(&output_file_path).into_diagnostic()?;
            std::io::copy(&mut file, &mut outfile).into_diagnostic()?;
        }
    }

    // Update thumbnail in mod project if it was extracted
    let mut final_mod_project = mod_project;
    if output_path.join("thumbnail.png").exists() {
        final_mod_project.thumbnail = Some("thumbnail.png".to_string());
    }

    // Write mod.config.json
    let config_path = output_path.join("mod.config.json");
    let config_content = serde_json::to_string_pretty(&final_mod_project).into_diagnostic()?;
    let mut config_file = File::create(config_path).into_diagnostic()?;
    config_file
        .write_all(config_content.as_bytes())
        .into_diagnostic()?;

    println_pad!(
        "{} {}",
        "📁 Extracted and converted to mod project at:".bright_yellow(),
        output_path.as_str().bright_white().bold()
    );

    println_pad!("{}", "✅ Extraction complete!".bright_green().bold());

    Ok(())
}
