use std::{
    collections::HashMap,
    fs::File,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

use colored::Colorize;
use fantome::pack_to_fantome;
use league_modpkg::{
    builder::{ModpkgBuilder, ModpkgChunkBuilder, ModpkgLayerBuilder},
    utils::hash_layer_name,
    ModpkgCompression, ModpkgMetadata,
};
use mod_project::{ModProject, ModProjectLayer};

use crate::{
    errors::CliError,
    utils::{self, validate_mod_name, validate_version_format},
};
use miette::{miette, IntoDiagnostic, Result, WrapErr};

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum PackFormat {
    Modpkg,
    Fantome,
}

#[derive(Debug)]
pub struct PackModProjectArgs {
    pub config_path: Option<String>,
    pub file_name: Option<String>,
    pub output_dir: String,
    pub format: PackFormat,
}

pub fn pack_mod_project(args: PackModProjectArgs) -> Result<()> {
    let config_path = resolve_config_path(args.config_path.clone())?;
    let mod_project = load_config(&config_path)?;

    validate_mod_name(&mod_project.name)?;
    validate_version_format(&mod_project.version)?;

    match args.format {
        PackFormat::Modpkg => pack_to_modpkg(args, config_path, mod_project),
        PackFormat::Fantome => pack_to_fantome_format(args, config_path, mod_project),
    }
}

fn pack_to_modpkg(
    args: PackModProjectArgs,
    config_path: PathBuf,
    mod_project: ModProject,
) -> Result<()> {
    let content_dir = resolve_content_dir(&config_path)?;

    validate_layer_presence(&mod_project, &config_path)?;

    println!(
        "{} {}",
        "📦 Packing mod project:".bright_blue().bold(),
        mod_project.name.bright_cyan().bold()
    );

    let output_dir = resolve_output_dir(&args.output_dir, &config_path)?;

    if !output_dir.exists() {
        println!("Creating output directory: {}", output_dir.display());
        std::fs::create_dir_all(&output_dir).into_diagnostic()?;
    }

    let mut modpkg_builder = ModpkgBuilder::default().with_layer(ModpkgLayerBuilder::base());
    let mut chunk_filepaths = HashMap::new();

    modpkg_builder = build_metadata(modpkg_builder, &mod_project);
    modpkg_builder = build_layers(
        modpkg_builder,
        &content_dir,
        &mod_project,
        &mut chunk_filepaths,
    )?;

    let modpkg_file_name = create_modpkg_file_name(&mod_project, args.file_name);
    let mut writer =
        BufWriter::new(File::create(output_dir.join(&modpkg_file_name)).into_diagnostic()?);

    modpkg_builder
        .build_to_writer(&mut writer, |chunk_builder, cursor| {
            let file_path = chunk_filepaths
                .get(&(
                    chunk_builder.path_hash(),
                    hash_layer_name(chunk_builder.layer()),
                ))
                .unwrap();

            let mut file = File::open(file_path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            cursor.write_all(&buffer)?;

            Ok(())
        })
        .into_diagnostic()?;

    println!(
        "{}\n{} {}",
        "✅ Mod package created successfully!".bright_green().bold(),
        "📍 Path:".bright_green(),
        output_dir
            .join(modpkg_file_name)
            .display()
            .to_string()
            .bright_white()
            .bold()
    );

    Ok(())
}

fn pack_to_fantome_format(
    args: PackModProjectArgs,
    config_path: PathBuf,
    mod_project: ModProject,
) -> Result<()> {
    println!(
        "{} {}",
        "🎭 Packing mod project to Fantome format:"
            .bright_blue()
            .bold(),
        mod_project.name.bright_cyan().bold()
    );

    // Warn about non-base layers not being supported
    warn_about_unsupported_layers(&mod_project);

    let project_root = config_path.parent().unwrap();
    let output_dir = resolve_output_dir(&args.output_dir, &config_path)?;

    if !output_dir.exists() {
        println!(
            "{} {}",
            "📁 Creating output directory:".bright_yellow(),
            output_dir.display().to_string().bright_white().bold()
        );
        std::fs::create_dir_all(&output_dir).into_diagnostic()?;
    }

    let fantome_file_name = create_fantome_file_name(&mod_project, args.file_name);
    let output_path = output_dir.join(&fantome_file_name);

    let file = File::create(&output_path).into_diagnostic()?;
    let writer = BufWriter::new(file);

    pack_to_fantome(writer, &mod_project, project_root)
        .map_err(|e| miette!("Failed to pack to Fantome format: {}", e))?;

    println!(
        "{}\n{} {}",
        "✅ Fantome mod package created successfully!"
            .bright_green()
            .bold(),
        "📍 Path:".bright_green(),
        output_path.display().to_string().bright_white().bold()
    );

    Ok(())
}

fn warn_about_unsupported_layers(mod_project: &ModProject) {
    let non_base_layers: Vec<_> = mod_project
        .layers
        .iter()
        .filter(|layer| layer.name != "base")
        .collect();

    if !non_base_layers.is_empty() {
        println!(
            "{}",
            "⚠️  WARNING: Fantome format only supports the base layer!"
                .bright_yellow()
                .bold()
        );
        println!(
            "{}",
            "   The following layers will NOT be included in the Fantome package:"
                .bright_yellow()
                .dimmed()
        );
        for layer in non_base_layers {
            println!(
                "   {} {} {}",
                "•".bright_red(),
                layer.name.bright_red().bold(),
                format!("(priority: {})", layer.priority).dimmed()
            );
        }
        println!(
            "   {} {}",
            "💡 Tip:".bright_cyan().bold(),
            "Consider using --format modpkg to include all layers."
                .bright_yellow()
                .dimmed()
        );
        println!(); // Empty line for spacing
    }
}

// Config utils

fn resolve_config_path(config_path: Option<String>) -> Result<PathBuf> {
    match config_path {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            let cwd = std::env::current_dir().into_diagnostic()?;
            resolve_correct_config_extension(&cwd)
        }
    }
}

fn resolve_correct_config_extension(project_dir: &Path) -> Result<PathBuf> {
    // JSON first, then TOML
    let config_extensions = ["json", "toml"];

    for ext in config_extensions {
        let config_path = project_dir.join(format!("mod.config.{}", ext));
        if config_path.exists() {
            return Ok(config_path);
        }
    }

    Err(CliError::config_not_found(project_dir.to_owned()).into())
}

fn load_config(config_path: &Path) -> Result<ModProject> {
    let config_extension = config_path.extension().unwrap_or_default();

    match config_extension.to_str() {
        Some("json") => {
            let file = File::open(config_path).into_diagnostic().with_context(|| {
                format!("Failed to open config file: {}", config_path.display())
            })?;
            serde_json::from_reader(file)
                .into_diagnostic()
                .with_context(|| {
                    format!(
                        "Failed to parse JSON config file: {}",
                        config_path.display()
                    )
                })
        }
        Some("toml") => {
            let content = std::fs::read_to_string(config_path)
                .into_diagnostic()
                .with_context(|| {
                    format!("Failed to read config file: {}", config_path.display())
                })?;
            toml::from_str(&content).into_diagnostic().with_context(|| {
                format!(
                    "Failed to parse TOML config file: {}",
                    config_path.display()
                )
            })
        }
        _ => Err(miette!(
            "Invalid config file extension, expected mod.config.json or mod.config.toml"
        )),
    }
}

fn resolve_content_dir(config_path: &Path) -> Result<PathBuf> {
    Ok(config_path.parent().unwrap().join("content"))
}

fn resolve_output_dir(output_dir: &str, config_path: &Path) -> Result<PathBuf> {
    let output_dir = PathBuf::from(output_dir);
    let output_dir = match output_dir.is_absolute() {
        true => output_dir,
        false => config_path.parent().unwrap().join(output_dir),
    };
    Ok(output_dir)
}

// Layer utils

fn validate_layer_presence(mod_project: &ModProject, mod_project_dir: &Path) -> Result<()> {
    for layer in &mod_project.layers {
        if !utils::is_valid_slug(&layer.name) {
            return Err(CliError::invalid_layer_name(layer.name.clone(), None).into());
        }

        // If the user explicitly defines the base layer, ensure its priority is 0
        if layer.name == "base" && layer.priority != 0 {
            return Err(CliError::invalid_base_layer_priority(layer.priority).into());
        }

        validate_layer_dir_presence(mod_project_dir, &layer.name)?;
    }

    Ok(())
}

fn validate_layer_dir_presence(mod_project_dir: &Path, layer_name: &str) -> Result<()> {
    let layer_dir = mod_project_dir.join("content").join(layer_name);
    if !layer_dir.exists() {
        return Err(CliError::layer_directory_missing(layer_name.to_string(), layer_dir).into());
    }

    Ok(())
}

fn build_metadata(builder: ModpkgBuilder, mod_project: &ModProject) -> ModpkgBuilder {
    builder.with_metadata(ModpkgMetadata {
        name: mod_project.name.clone(),
        display_name: mod_project.display_name.clone(),
        description: Some(mod_project.description.clone()),
        version: mod_project.version.clone(),
        distributor: None,
        authors: mod_project
            .authors
            .iter()
            .map(utils::modpkg::convert_project_author)
            .collect(),
        license: utils::modpkg::convert_project_license(&mod_project.license),
    })
}

fn build_layers(
    mut modpkg_builder: ModpkgBuilder,
    content_dir: &Path,
    mod_project: &ModProject,
    chunk_filepaths: &mut HashMap<(u64, u64), PathBuf>,
) -> Result<ModpkgBuilder> {
    // Process base layer
    modpkg_builder = build_layer_from_dir(
        modpkg_builder,
        content_dir,
        &ModProjectLayer::base(),
        chunk_filepaths,
    )?;

    // Process layers
    for layer in &mod_project.layers {
        if layer.name == "base" {
            // Base layer is handled separately and must always have priority 0
            continue;
        }
        println!(
            "{} {}",
            "🏗️  Building layer:".bright_magenta(),
            layer.name.bright_cyan().bold()
        );
        modpkg_builder = modpkg_builder
            .with_layer(ModpkgLayerBuilder::new(layer.name.as_str()).with_priority(layer.priority));
        modpkg_builder = build_layer_from_dir(modpkg_builder, content_dir, layer, chunk_filepaths)?;
    }

    Ok(modpkg_builder)
}

fn build_layer_from_dir(
    mut modpkg_builder: ModpkgBuilder,
    content_dir: &Path,
    layer: &ModProjectLayer,
    chunk_filepaths: &mut HashMap<(u64, u64), PathBuf>,
) -> Result<ModpkgBuilder> {
    let layer_dir = content_dir.join(layer.name.as_str());

    for entry in glob::glob(layer_dir.join("**/*").to_str().unwrap())
        .into_diagnostic()?
        .filter_map(Result::ok)
    {
        if !entry.is_file() {
            continue;
        }

        let layer_hash = hash_layer_name(layer.name.as_str());
        let (modpkg_builder_new, path_hash) =
            build_chunk_from_file(modpkg_builder, layer, &entry, &layer_dir)?;

        chunk_filepaths
            .entry((path_hash, layer_hash))
            .or_insert(entry);

        modpkg_builder = modpkg_builder_new;
    }

    Ok(modpkg_builder)
}

fn build_chunk_from_file(
    modpkg_builder: ModpkgBuilder,
    layer: &ModProjectLayer,
    file_path: &Path,
    layer_dir: &Path,
) -> Result<(ModpkgBuilder, u64)> {
    let relative_path = file_path.strip_prefix(layer_dir).into_diagnostic()?;
    let chunk_builder = ModpkgChunkBuilder::new()
        .with_path(relative_path.to_str().unwrap())
        .into_diagnostic()?
        .with_compression(ModpkgCompression::Zstd)
        .with_layer(layer.name.as_str());

    let path_hash = chunk_builder.path_hash();
    Ok((modpkg_builder.with_chunk(chunk_builder), path_hash))
}

fn create_modpkg_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    match custom_name {
        Some(name) => {
            if name.ends_with(".modpkg") {
                name
            } else {
                format!("{}.modpkg", name)
            }
        }
        None => {
            let version = semver::Version::parse(&mod_project.version).unwrap();
            format!("{}_{}.modpkg", mod_project.name, version)
        }
    }
}

fn create_fantome_file_name(mod_project: &ModProject, custom_name: Option<String>) -> String {
    match custom_name {
        Some(name) => {
            if name.ends_with(".fantome") {
                name
            } else {
                format!("{}.fantome", name)
            }
        }
        None => {
            let version = semver::Version::parse(&mod_project.version).unwrap();
            format!("{}_{}.fantome", mod_project.name, version)
        }
    }
}
