use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// Get the application data directory for storing settings using Tauri's path resolver.
pub fn get_app_data_dir(app_handle: &AppHandle) -> Option<PathBuf> {
    app_handle.path().app_data_dir().ok()
}

/// Get the path to the settings file.
pub fn get_settings_file_path(app_handle: &AppHandle) -> Option<PathBuf> {
    get_app_data_dir(app_handle).map(|p| p.join("settings.json"))
}

/// Load settings from disk, returning defaults if file doesn't exist.
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

/// Save settings to disk.
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

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    fs::write(&settings_path, contents)?;
    tracing::info!("Saved settings to {:?}", settings_path);

    Ok(())
}

/// Application settings state.
pub struct SettingsState(pub Mutex<Settings>);

impl SettingsState {
    pub fn new(app_handle: &AppHandle) -> Self {
        Self(Mutex::new(load_settings(app_handle)))
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self(Mutex::new(Settings::default()))
    }
}

/// Theme selection for the application.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

/// Accent color configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccentColor {
    /// Preset color name: "blue", "purple", "green", "orange", "pink", "red", "teal"
    pub preset: Option<String>,
    /// Custom hue value (0-360) for custom colors
    pub custom_hue: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub league_path: Option<PathBuf>,
    pub mod_storage_path: Option<PathBuf>,
    /// Directory where mod projects are stored (for Creator Workshop).
    pub workshop_path: Option<PathBuf>,
    pub first_run_complete: bool,
    /// Application theme (system, dark, or light).
    pub theme: Theme,
    /// Accent color configuration.
    pub accent_color: AccentColor,
}
