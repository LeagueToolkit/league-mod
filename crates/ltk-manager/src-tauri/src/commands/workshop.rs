use crate::error::{AppError, AppResult, IpcResult, MutexResultExt};
use crate::state::SettingsState;
use camino::Utf8PathBuf;
use chrono::{DateTime, Utc};
use ltk_mod_project::{default_layers, ModProject, ModProjectAuthor, ModProjectLayer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::State;

/// A workshop project displayed in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopProject {
    /// Absolute path to the project directory
    pub path: String,
    /// Project slug name (directory name)
    pub name: String,
    /// Human-readable display name
    pub display_name: String,
    /// Semantic version string
    pub version: String,
    /// Project description
    pub description: String,
    /// List of authors
    pub authors: Vec<WorkshopAuthor>,
    /// Project layers
    pub layers: Vec<WorkshopLayer>,
    /// Path to thumbnail image if exists
    pub thumbnail_path: Option<String>,
    /// Last modification time
    pub last_modified: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopAuthor {
    pub name: String,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkshopLayer {
    pub name: String,
    pub priority: i32,
    pub description: Option<String>,
    #[serde(default)]
    pub string_overrides: HashMap<String, String>,
}

/// Arguments for creating a new project.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProjectArgs {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub authors: Vec<String>,
}

/// Arguments for packing a project.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackProjectArgs {
    pub project_path: String,
    pub output_dir: Option<String>,
    pub format: PackFormat,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackFormat {
    Modpkg,
    Fantome,
}

/// Result of a successful pack operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
    pub output_path: String,
    pub file_name: String,
    pub format: String,
}

/// Validation result for a project.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// ============================================================================
// Commands
// ============================================================================

/// Get all workshop projects from the configured workshop directory.
#[tauri::command]
pub fn get_workshop_projects(settings: State<SettingsState>) -> IpcResult<Vec<WorkshopProject>> {
    get_workshop_projects_inner(&settings).into()
}

fn get_workshop_projects_inner(settings: &State<SettingsState>) -> AppResult<Vec<WorkshopProject>> {
    let settings = settings.0.lock().mutex_err()?.clone();
    let workshop_path = settings
        .workshop_path
        .ok_or(AppError::WorkshopNotConfigured)?;

    if !workshop_path.exists() {
        return Ok(Vec::new());
    }

    let mut projects = Vec::new();

    for entry in fs::read_dir(&workshop_path)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        // Check for mod.config.json or mod.config.toml
        let config_path = find_config_file(&path);
        if config_path.is_none() {
            continue;
        }

        match load_workshop_project(&path) {
            Ok(project) => projects.push(project),
            Err(e) => {
                tracing::warn!("Skipping invalid project at {}: {}", path.display(), e);
            }
        }
    }

    // Sort by last modified (newest first)
    projects.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(projects)
}

/// Create a new workshop project.
#[tauri::command]
pub fn create_workshop_project(
    args: CreateProjectArgs,
    settings: State<SettingsState>,
) -> IpcResult<WorkshopProject> {
    create_workshop_project_inner(args, &settings).into()
}

fn create_workshop_project_inner(
    args: CreateProjectArgs,
    settings: &State<SettingsState>,
) -> AppResult<WorkshopProject> {
    let settings = settings.0.lock().mutex_err()?.clone();
    let workshop_path = settings
        .workshop_path
        .ok_or(AppError::WorkshopNotConfigured)?;

    // Validate project name
    if !is_valid_project_name(&args.name) {
        return Err(AppError::ValidationFailed(
            "Project name must be lowercase alphanumeric with hyphens only".to_string(),
        ));
    }

    let project_dir = workshop_path.join(&args.name);

    if project_dir.exists() {
        return Err(AppError::ProjectAlreadyExists(args.name));
    }

    // Create project structure
    fs::create_dir_all(&project_dir)?;
    fs::create_dir_all(project_dir.join("content").join("base"))?;

    // Create mod.config.json
    let authors: Vec<ModProjectAuthor> = args
        .authors
        .into_iter()
        .map(ModProjectAuthor::Name)
        .collect();

    let mod_project = ModProject {
        name: args.name.clone(),
        display_name: args.display_name,
        version: "1.0.0".to_string(),
        description: args.description,
        authors,
        license: None,
        transformers: Vec::new(),
        layers: default_layers(),
        thumbnail: None,
    };

    let config_path = project_dir.join("mod.config.json");
    let config_content = serde_json::to_string_pretty(&mod_project)?;
    fs::write(&config_path, config_content)?;

    // Create README.md
    let readme_content = format!(
        "# {}\n\n{}\n",
        mod_project.display_name, mod_project.description
    );
    fs::write(project_dir.join("README.md"), readme_content)?;

    load_workshop_project(&project_dir)
}

/// Get a single workshop project by path.
#[tauri::command]
pub fn get_workshop_project(project_path: String) -> IpcResult<WorkshopProject> {
    get_workshop_project_inner(&project_path).into()
}

fn get_workshop_project_inner(project_path: &str) -> AppResult<WorkshopProject> {
    let path = PathBuf::from(project_path);
    if !path.exists() {
        return Err(AppError::ProjectNotFound(project_path.to_string()));
    }
    load_workshop_project(&path)
}

/// Save project configuration changes.
#[tauri::command]
pub fn save_project_config(
    project_path: String,
    display_name: String,
    version: String,
    description: String,
    authors: Vec<WorkshopAuthor>,
) -> IpcResult<WorkshopProject> {
    save_project_config_inner(project_path, display_name, version, description, authors).into()
}

fn save_project_config_inner(
    project_path: String,
    display_name: String,
    version: String,
    description: String,
    authors: Vec<WorkshopAuthor>,
) -> AppResult<WorkshopProject> {
    let path = PathBuf::from(&project_path);
    if !path.exists() {
        return Err(AppError::ProjectNotFound(project_path));
    }

    let config_path =
        find_config_file(&path).ok_or_else(|| AppError::ProjectNotFound(project_path.clone()))?;

    // Load existing config
    let contents = fs::read_to_string(&config_path)?;
    let mut mod_project: ModProject = if config_path
        .extension()
        .map(|e| e == "toml")
        .unwrap_or(false)
    {
        toml::from_str(&contents).map_err(|e| AppError::Other(e.to_string()))?
    } else {
        serde_json::from_str(&contents)?
    };

    // Update fields
    mod_project.display_name = display_name;
    mod_project.version = version;
    mod_project.description = description;
    mod_project.authors = authors
        .into_iter()
        .map(|a| match a.role {
            Some(role) => ModProjectAuthor::Role { name: a.name, role },
            None => ModProjectAuthor::Name(a.name),
        })
        .collect();

    // Save as JSON (always save as JSON for consistency)
    let json_config_path = path.join("mod.config.json");
    let config_content = serde_json::to_string_pretty(&mod_project)?;
    fs::write(&json_config_path, config_content)?;

    load_workshop_project(&path)
}

/// Delete a workshop project.
#[tauri::command]
pub fn delete_workshop_project(project_path: String) -> IpcResult<()> {
    delete_workshop_project_inner(&project_path).into()
}

fn delete_workshop_project_inner(project_path: &str) -> AppResult<()> {
    let path = PathBuf::from(project_path);
    if !path.exists() {
        return Err(AppError::ProjectNotFound(project_path.to_string()));
    }

    // Safety check: ensure it contains a mod.config file
    if find_config_file(&path).is_none() {
        return Err(AppError::ValidationFailed(
            "Directory does not appear to be a mod project".to_string(),
        ));
    }

    fs::remove_dir_all(&path)?;
    Ok(())
}

/// Pack a workshop project to .modpkg or .fantome format.
#[tauri::command]
pub fn pack_workshop_project(args: PackProjectArgs) -> IpcResult<PackResult> {
    pack_workshop_project_inner(args).into()
}

fn pack_workshop_project_inner(args: PackProjectArgs) -> AppResult<PackResult> {
    let project_path = PathBuf::from(&args.project_path);
    if !project_path.exists() {
        return Err(AppError::ProjectNotFound(args.project_path));
    }

    let project_path_utf8 = Utf8PathBuf::try_from(project_path.clone())
        .map_err(|_| AppError::InvalidPath("Project path is not valid UTF-8".to_string()))?;

    // Load and validate project
    let config_path = find_config_file(&project_path)
        .ok_or_else(|| AppError::ProjectNotFound(args.project_path.clone()))?;
    let mod_project = load_mod_project(&config_path)?;

    // Determine output directory
    let output_dir = args
        .output_dir
        .map(Utf8PathBuf::from)
        .unwrap_or_else(|| project_path_utf8.join("build"));

    // Create output directory if needed
    fs::create_dir_all(output_dir.as_std_path())?;

    match args.format {
        PackFormat::Modpkg => {
            let file_name = ltk_modpkg::project::create_file_name(&mod_project, None);
            let output_path = output_dir.join(&file_name);

            ltk_modpkg::project::pack_from_project(&project_path_utf8, &output_path, &mod_project)
                .map_err(|e| AppError::PackFailed(e.to_string()))?;

            Ok(PackResult {
                output_path: output_path.to_string(),
                file_name,
                format: "modpkg".to_string(),
            })
        }
        PackFormat::Fantome => {
            let file_name = ltk_fantome::create_file_name(&mod_project, None);
            let output_path = output_dir.join(&file_name);

            let file = fs::File::create(output_path.as_std_path())?;
            let writer = std::io::BufWriter::new(file);

            ltk_fantome::pack_to_fantome(writer, &mod_project, project_path_utf8.as_std_path())
                .map_err(|e| AppError::PackFailed(e.to_string()))?;

            Ok(PackResult {
                output_path: output_path.to_string(),
                file_name,
                format: "fantome".to_string(),
            })
        }
    }
}

/// Import a .modpkg file as a new workshop project.
#[tauri::command]
pub fn import_from_modpkg(
    file_path: String,
    settings: State<SettingsState>,
) -> IpcResult<WorkshopProject> {
    import_from_modpkg_inner(file_path, &settings).into()
}

fn import_from_modpkg_inner(
    file_path: String,
    settings: &State<SettingsState>,
) -> AppResult<WorkshopProject> {
    let settings = settings.0.lock().mutex_err()?.clone();
    let workshop_path = settings
        .workshop_path
        .ok_or(AppError::WorkshopNotConfigured)?;

    let file = fs::File::open(&file_path)?;
    let mut modpkg =
        ltk_modpkg::Modpkg::mount_from_reader(file).map_err(|e| AppError::Modpkg(e.to_string()))?;

    let metadata = modpkg
        .load_metadata()
        .map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Check if project already exists
    let project_dir = workshop_path.join(&metadata.name);
    if project_dir.exists() {
        return Err(AppError::ProjectAlreadyExists(metadata.name));
    }

    // Create project directory
    fs::create_dir_all(&project_dir)?;

    // Extract content
    let content_dir = project_dir.join("content");
    fs::create_dir_all(&content_dir)?;

    let mut extractor = ltk_modpkg::ModpkgExtractor::new(&mut modpkg);
    extractor
        .extract_all(&content_dir)
        .map_err(|e| AppError::Modpkg(e.to_string()))?;

    // Build layers from header, preserving string overrides from metadata
    let mut layers: Vec<ModProjectLayer> = modpkg
        .layers
        .values()
        .map(|l| {
            let meta_layer = metadata.layers.iter().find(|ml| ml.name == l.name);
            ModProjectLayer {
                name: l.name.clone(),
                priority: l.priority,
                description: meta_layer.and_then(|ml| ml.description.clone()),
                string_overrides: meta_layer
                    .map(|ml| ml.string_overrides.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();
    layers.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)));

    if !layers.iter().any(|l| l.name == "base") {
        layers.insert(0, ModProjectLayer::base());
    }

    // Create mod project config
    let mod_project = ModProject {
        name: metadata.name,
        display_name: metadata.display_name,
        version: metadata.version.to_string(),
        description: metadata.description.unwrap_or_default(),
        authors: metadata
            .authors
            .into_iter()
            .map(|a| ModProjectAuthor::Name(a.name))
            .collect(),
        license: None,
        transformers: Vec::new(),
        layers,
        thumbnail: None,
    };

    let config_path = project_dir.join("mod.config.json");
    fs::write(&config_path, serde_json::to_string_pretty(&mod_project)?)?;

    // Extract README and thumbnail
    if let Ok(readme_bytes) = modpkg.load_readme() {
        let _ = fs::write(project_dir.join("README.md"), readme_bytes);
    }
    if let Ok(thumbnail_bytes) = modpkg.load_thumbnail() {
        let _ = fs::write(project_dir.join("thumbnail.webp"), thumbnail_bytes);
    }

    load_workshop_project(&project_dir)
}

/// Set a project's thumbnail image.
#[tauri::command]
pub fn set_project_thumbnail(
    project_path: String,
    image_path: String,
) -> IpcResult<WorkshopProject> {
    set_project_thumbnail_inner(&project_path, &image_path).into()
}

fn set_project_thumbnail_inner(project_path: &str, image_path: &str) -> AppResult<WorkshopProject> {
    let project_dir = PathBuf::from(project_path);
    if !project_dir.exists() {
        return Err(AppError::ProjectNotFound(project_path.to_string()));
    }

    // Verify it's a valid project
    if find_config_file(&project_dir).is_none() {
        return Err(AppError::ProjectNotFound(project_path.to_string()));
    }

    let source_path = PathBuf::from(image_path);
    if !source_path.exists() {
        return Err(AppError::InvalidPath(image_path.to_string()));
    }

    let extension = source_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    let supported_formats = [
        "webp", "png", "jpg", "jpeg", "gif", "bmp", "tiff", "tif", "ico",
    ];
    if !supported_formats.contains(&extension.as_str()) {
        return Err(AppError::ValidationFailed(format!(
            "Unsupported image format: {}. Supported formats: {}",
            extension,
            supported_formats.join(", ")
        )));
    }

    let source_utf8_path = camino::Utf8Path::from_path(&source_path)
        .ok_or_else(|| AppError::ValidationFailed("Invalid UTF-8 path".to_string()))?;

    let webp_data = ltk_modpkg::project::load_thumbnail(source_utf8_path)
        .map_err(|e| AppError::ValidationFailed(format!("Thumbnail processing failed: {}", e)))?;

    // Remove existing thumbnails for backwards compatibility
    let _ = fs::remove_file(project_dir.join("thumbnail.webp"));
    let _ = fs::remove_file(project_dir.join("thumbnail.png"));

    let target_path = project_dir.join("thumbnail.webp");
    fs::write(&target_path, webp_data)?;

    load_workshop_project(&project_dir)
}

/// Get a project's thumbnail path (validates it exists).
#[tauri::command]
pub fn get_project_thumbnail(thumbnail_path: String) -> IpcResult<String> {
    get_project_thumbnail_inner(&thumbnail_path).into()
}

fn get_project_thumbnail_inner(thumbnail_path: &str) -> AppResult<String> {
    let path = PathBuf::from(thumbnail_path);
    if !path.exists() {
        return Err(AppError::InvalidPath(thumbnail_path.to_string()));
    }
    Ok(thumbnail_path.to_string())
}

/// Save string overrides for a specific layer in a workshop project.
#[tauri::command]
pub fn save_layer_string_overrides(
    project_path: String,
    layer_name: String,
    string_overrides: HashMap<String, String>,
) -> IpcResult<WorkshopProject> {
    save_layer_string_overrides_inner(project_path, layer_name, string_overrides).into()
}

fn save_layer_string_overrides_inner(
    project_path: String,
    layer_name: String,
    string_overrides: HashMap<String, String>,
) -> AppResult<WorkshopProject> {
    let path = PathBuf::from(&project_path);
    if !path.exists() {
        return Err(AppError::ProjectNotFound(project_path));
    }

    let config_path =
        find_config_file(&path).ok_or_else(|| AppError::ProjectNotFound(project_path.clone()))?;

    // Load existing config
    let mut mod_project = load_mod_project(&config_path)?;

    // Find the target layer
    let layer = mod_project
        .layers
        .iter_mut()
        .find(|l| l.name == layer_name)
        .ok_or_else(|| {
            AppError::ValidationFailed(format!("Layer '{}' not found in project", layer_name))
        })?;

    // Update string overrides
    layer.string_overrides = string_overrides;

    // Save as JSON
    let json_config_path = path.join("mod.config.json");
    let config_content = serde_json::to_string_pretty(&mod_project)?;
    fs::write(&json_config_path, config_content)?;

    load_workshop_project(&path)
}

/// Validate a project before packing.
#[tauri::command]
pub fn validate_project(project_path: String) -> IpcResult<ValidationResult> {
    validate_project_inner(&project_path).into()
}

fn validate_project_inner(project_path: &str) -> AppResult<ValidationResult> {
    let path = PathBuf::from(project_path);
    if !path.exists() {
        return Err(AppError::ProjectNotFound(project_path.to_string()));
    }

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check for config file
    let config_path = match find_config_file(&path) {
        Some(p) => p,
        None => {
            errors.push("No mod.config.json or mod.config.toml found".to_string());
            return Ok(ValidationResult {
                valid: false,
                errors,
                warnings,
            });
        }
    };

    // Try to load config
    let mod_project = match load_mod_project(&config_path) {
        Ok(p) => p,
        Err(e) => {
            errors.push(format!("Failed to parse config: {}", e));
            return Ok(ValidationResult {
                valid: false,
                errors,
                warnings,
            });
        }
    };

    // Validate name
    if !is_valid_project_name(&mod_project.name) {
        errors.push("Project name must be lowercase alphanumeric with hyphens only".to_string());
    }

    // Validate version
    if semver::Version::parse(&mod_project.version).is_err() {
        errors.push(format!(
            "Invalid version format: {} (expected semver like 1.0.0)",
            mod_project.version
        ));
    }

    // Check content directory exists
    let content_dir = path.join("content");
    if !content_dir.exists() {
        errors.push("content/ directory not found".to_string());
    } else {
        // Check layer directories
        for layer in &mod_project.layers {
            let layer_dir = content_dir.join(&layer.name);
            if !layer_dir.exists() {
                errors.push(format!("Layer directory content/{} not found", layer.name));
            } else if layer_dir.read_dir().map(|d| d.count() == 0).unwrap_or(true) {
                warnings.push(format!("Layer content/{} is empty", layer.name));
            }
        }
    }

    // Check for base layer
    if !mod_project.layers.iter().any(|l| l.name == "base") {
        warnings.push("No 'base' layer defined".to_string());
    }

    // Check thumbnail
    if path.join("thumbnail.webp").exists() || path.join("thumbnail.png").exists() {
        // Thumbnail exists, good
    } else {
        warnings.push("No thumbnail found (thumbnail.webp or thumbnail.png)".to_string());
    }

    Ok(ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    })
}

// ============================================================================
// Helpers
// ============================================================================

fn find_config_file(project_dir: &std::path::Path) -> Option<PathBuf> {
    let json_path = project_dir.join("mod.config.json");
    if json_path.exists() {
        return Some(json_path);
    }

    let toml_path = project_dir.join("mod.config.toml");
    if toml_path.exists() {
        return Some(toml_path);
    }

    None
}

fn load_mod_project(config_path: &std::path::Path) -> AppResult<ModProject> {
    let contents = fs::read_to_string(config_path)?;

    let is_toml = config_path
        .extension()
        .map(|e| e == "toml")
        .unwrap_or(false);

    if is_toml {
        toml::from_str(&contents).map_err(|e| AppError::Other(e.to_string()))
    } else {
        serde_json::from_str(&contents).map_err(AppError::from)
    }
}

fn load_workshop_project(project_dir: &std::path::Path) -> AppResult<WorkshopProject> {
    let config_path = find_config_file(project_dir)
        .ok_or_else(|| AppError::ProjectNotFound(project_dir.display().to_string()))?;

    let mod_project = load_mod_project(&config_path)?;

    // Get last modified time
    let metadata = fs::metadata(&config_path)?;
    let last_modified = metadata
        .modified()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());

    // Check for thumbnail
    let thumbnail_path = if project_dir.join("thumbnail.webp").exists() {
        Some(project_dir.join("thumbnail.webp").display().to_string())
    } else if project_dir.join("thumbnail.png").exists() {
        Some(project_dir.join("thumbnail.png").display().to_string())
    } else {
        None
    };

    let authors = mod_project
        .authors
        .iter()
        .map(|a| match a {
            ModProjectAuthor::Name(name) => WorkshopAuthor {
                name: name.clone(),
                role: None,
            },
            ModProjectAuthor::Role { name, role } => WorkshopAuthor {
                name: name.clone(),
                role: Some(role.clone()),
            },
        })
        .collect();

    let layers = mod_project
        .layers
        .iter()
        .map(|l| WorkshopLayer {
            name: l.name.clone(),
            priority: l.priority,
            description: l.description.clone(),
            string_overrides: l.string_overrides.clone(),
        })
        .collect();

    Ok(WorkshopProject {
        path: project_dir.display().to_string(),
        name: mod_project.name,
        display_name: mod_project.display_name,
        version: mod_project.version,
        description: mod_project.description,
        authors,
        layers,
        thumbnail_path,
        last_modified,
    })
}

fn is_valid_project_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}
