use crate::println_pad;
use crate::{
    errors::CliError,
    utils::{validate_mod_name, validate_version_format},
};
use camino::{Utf8Path, Utf8PathBuf};
use colored::Colorize;
use ltk_fantome::{get_unsupported_layers, pack_to_fantome};
use ltk_mod_project::ModProject;
use ltk_modpkg::project::{self as modpkg_project, PackError};
use miette::{miette, IntoDiagnostic, Result, WrapErr};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

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
    #[allow(dead_code)]
    pub sign: bool,
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
    let config_path = Utf8PathBuf::try_from(config_path)
        .into_diagnostic()
        .wrap_err("Config path is not valid UTF-8")?;
    let project_root = config_path.parent().unwrap();

    println_pad!(
        "{} {}",
        "📦 Packing mod project:".bright_blue().bold(),
        mod_project.name.bright_cyan().bold()
    );

    let output_dir = resolve_output_dir(&args.output_dir, &config_path)?;

    if !output_dir.exists() {
        println_pad!("Creating output directory: {}", output_dir);
        std::fs::create_dir_all(&output_dir).into_diagnostic()?;
    }

    // Print layer info
    for layer in &mod_project.layers {
        if layer.name != "base" {
            println_pad!(
                "{} {}",
                "🏗️  Building layer:".bright_yellow(),
                layer.name.bright_cyan().bold()
            );
        }
    }

    let modpkg_file_name = modpkg_project::create_file_name(&mod_project, args.file_name);
    let output_path = output_dir.join(&modpkg_file_name);

    // Use the shared packing logic from ltk_modpkg
    modpkg_project::pack_from_project(project_root, &output_path, &mod_project)
        .map_err(|e| convert_pack_error(e, project_root))?;

    println_pad!(
        "{}\n{} {}",
        "Mod package created successfully!".bright_green().bold(),
        "Path:".bright_green(),
        output_path.as_str().bright_white().bold()
    );

    Ok(())
}

/// Convert PackError to a miette diagnostic with CLI-friendly error messages.
fn convert_pack_error(err: PackError, project_root: &Utf8Path) -> miette::Report {
    match err {
        PackError::LayerDirMissing { layer, path } => {
            CliError::layer_directory_missing(layer, path.into_std_path_buf()).into()
        }
        PackError::InvalidLayerName(name) => CliError::invalid_layer_name(name, None).into(),
        PackError::InvalidBaseLayerPriority(priority) => {
            CliError::invalid_base_layer_priority(priority).into()
        }
        PackError::ConfigNotFound(_) => {
            CliError::config_not_found(project_root.as_std_path().to_owned()).into()
        }
        other => miette!("Failed to pack mod: {}", other),
    }
}

fn pack_to_fantome_format(
    args: PackModProjectArgs,
    config_path: PathBuf,
    mod_project: ModProject,
) -> Result<()> {
    let config_path = Utf8PathBuf::try_from(config_path)
        .into_diagnostic()
        .wrap_err("Config path is not valid UTF-8")?;

    println_pad!(
        "{} {}",
        "Packing mod project to Fantome format:"
            .bright_blue()
            .bold(),
        mod_project.name.bright_cyan().bold()
    );

    warn_about_unsupported_layers(&mod_project);

    let project_root = config_path.parent().unwrap();
    let output_dir = resolve_output_dir(&args.output_dir, &config_path)?;

    if !output_dir.exists() {
        println_pad!(
            "{} {}",
            "📁 Creating output directory:".bright_yellow(),
            output_dir.as_str().bright_white().bold()
        );
        std::fs::create_dir_all(&output_dir).into_diagnostic()?;
    }

    let fantome_file_name = ltk_fantome::create_file_name(&mod_project, args.file_name);
    let output_path = output_dir.join(&fantome_file_name);

    let file = File::create(&output_path).into_diagnostic()?;
    let writer = BufWriter::new(file);

    pack_to_fantome(writer, &mod_project, project_root.as_std_path())
        .map_err(|e| miette!("Failed to pack to Fantome format: {}", e))?;

    println_pad!(
        "{}\n{} {}",
        "Fantome mod package created successfully!"
            .bright_green()
            .bold(),
        "Path:".bright_green(),
        output_path.as_str().bright_white().bold()
    );

    Ok(())
}

fn warn_about_unsupported_layers(mod_project: &ModProject) {
    let non_base_layers = get_unsupported_layers(mod_project);

    if !non_base_layers.is_empty() {
        println_pad!(
            "{}",
            "⚠️  WARNING: Fantome format only supports the base layer!"
                .bright_yellow()
                .bold()
        );
        println_pad!(
            "{}",
            "   The following layers will NOT be included in the Fantome package:"
                .bright_yellow()
                .dimmed()
        );
        for layer in non_base_layers {
            println_pad!(
                "   {} {} {}",
                "•".bright_red(),
                layer.name.bright_red().bold(),
                format!("(priority: {})", layer.priority).dimmed()
            );
        }
        println_pad!(
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

fn resolve_output_dir(output_dir: &str, config_path: &Utf8Path) -> Result<Utf8PathBuf> {
    let output_dir = Utf8PathBuf::from(output_dir);
    let output_dir = match output_dir.is_absolute() {
        true => output_dir,
        false => config_path.parent().unwrap().join(&output_dir),
    };
    Ok(output_dir)
}
