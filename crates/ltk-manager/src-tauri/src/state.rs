use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Get the application data directory for storing settings using Tauri's path resolver
pub fn get_app_data_dir(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle.path().app_data_dir().ok()
}

/// Get the path to the settings file
pub fn get_settings_file_path(app_handle: &AppHandle) -> Option<PathBuf> {
    get_app_data_dir(app_handle).map(|p| p.join("settings.json"))
}

/// Load settings from disk, returning defaults if file doesn't exist
pub fn load_settings(app_handle: &AppHandle) -> Settings {
    let Some(settings_path) = get_settings_file_path(app_handle) else {
        tracing::warn!("Could not determine settings file path, using defaults");
        return Settings::default();
    };

    if !settings_path.exists() {
        tracing::info!("Settings file not found, using defaults");
        return Settings::default();
    }

    match fs::read_to_string(&settings_path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(settings) => {
                tracing::info!("Loaded settings from {:?}", settings_path);
                settings
            }
            Err(e) => {
                tracing::error!("Failed to parse settings file: {}", e);
                Settings::default()
            }
        },
        Err(e) => {
            tracing::error!("Failed to read settings file: {}", e);
            Settings::default()
        }
    }
}

/// Save settings to disk
pub fn save_settings_to_disk(
    app_handle: &AppHandle,
    settings: &Settings,
) -> Result<(), std::io::Error> {
    let Some(settings_path) = get_settings_file_path(app_handle) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine settings file path",
        ));
    };

    // Ensure the directory exists
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    fs::write(&settings_path, contents)?;
    tracing::info!("Saved settings to {:?}", settings_path);

    Ok(())
}

#[derive(Debug)]
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub installed_mods: Mutex<Vec<InstalledMod>>,
}

impl AppState {
    /// Create AppState with settings loaded from disk
    pub fn new(app_handle: &AppHandle) -> Self {
        Self {
            settings: Mutex::new(load_settings(app_handle)),
            installed_mods: Mutex::new(Vec::new()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            installed_mods: Mutex::new(Vec::new()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub league_path: Option<PathBuf>,
    pub mod_storage_path: Option<PathBuf>,
    /// Directory where mod projects are stored (for Creator Workshop)
    pub workshop_path: Option<PathBuf>,
    pub theme: Theme,
    pub first_run_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub authors: Vec<String>,
    pub enabled: bool,
    pub installed_at: String,
    pub file_path: PathBuf,
    pub layers: Vec<ModLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModLayer {
    pub name: String,
    pub priority: i32,
    pub enabled: bool,
}
