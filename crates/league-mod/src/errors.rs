use camino::Utf8PathBuf;
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
pub enum CliError {
    #[error("Configuration file not found")]
    #[diagnostic(
        code(config::not_found),
        help("Create a mod.config.json or mod.config.toml file in your project directory")
    )]
    ConfigNotFound { search_path: Utf8PathBuf },

    #[error("Invalid layer name: {name}")]
    #[diagnostic(
        code(layer::invalid_name),
        help("Layer names must be alphanumeric and contain no spaces or special characters")
    )]
    InvalidLayerName {
        name: String,
        #[label("invalid layer name")]
        span: Option<SourceSpan>,
    },

    #[error("Layer directory not found: {layer_name}")]
    #[diagnostic(
        code(layer::directory_missing),
        help("Create the directory content/{layer_name}/ and add your mod files there")
    )]
    LayerDirectoryMissing {
        layer_name: String,
        expected_path: Utf8PathBuf,
    },

    #[error("Invalid mod name: {name}")]
    #[diagnostic(
        code(project::invalid_name),
        help("Mod names must be alphanumeric and contain no spaces or special characters (You can set a display name later)")
    )]
    InvalidModName {
        name: String,
        #[label("invalid mod name")]
        span: Option<SourceSpan>,
    },

    #[error("Invalid version format: {version}")]
    #[diagnostic(
        code(project::invalid_version),
        help("Version must follow semantic versioning (e.g., 1.0.0, 2.1.3-beta)")
    )]
    InvalidVersion {
        version: String,
        #[label("invalid version")]
        span: Option<SourceSpan>,
    },

    #[error("Configuration file error")]
    #[diagnostic(
        code(config::parse_error),
        help("Check your mod.config.json or mod.config.toml file for syntax errors")
    )]
    ConfigParseError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
        #[label("error occurred here")]
        span: Option<SourceSpan>,
    },

    #[error("File not found: {path}")]
    #[diagnostic(
        code(file::not_found),
        help("Make sure the file exists and the path is correct")
    )]
    FileNotFound { path: Utf8PathBuf },

    #[error("Directory creation failed")]
    #[diagnostic(
        code(fs::create_dir_failed),
        help("Check file permissions and available disk space")
    )]
    DirectoryCreationFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("IO operation failed")]
    #[diagnostic(code(io::operation_failed))]
    IoError {
        #[from]
        source: std::io::Error,
    },

    #[error("Invalid base layer priority: {provided}")]
    #[diagnostic(
        code(layer::invalid_base_priority),
        help("The 'base' layer must have priority 0")
    )]
    InvalidBaseLayerPriority { provided: i32 },
}

impl CliError {
    pub fn config_not_found(search_path: Utf8PathBuf) -> Self {
        Self::ConfigNotFound { search_path }
    }

    pub fn invalid_layer_name(name: String, span: Option<SourceSpan>) -> Self {
        Self::InvalidLayerName { name, span }
    }

    pub fn layer_directory_missing(layer_name: String, expected_path: Utf8PathBuf) -> Self {
        Self::LayerDirectoryMissing {
            layer_name,
            expected_path,
        }
    }

    pub fn invalid_mod_name(name: String, span: Option<SourceSpan>) -> Self {
        Self::InvalidModName { name, span }
    }

    pub fn invalid_version(version: String, span: Option<SourceSpan>) -> Self {
        Self::InvalidVersion { version, span }
    }

    #[allow(unused)]
    pub fn config_parse_error(
        source: Box<dyn std::error::Error + Send + Sync>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self::ConfigParseError { source, span }
    }

    #[allow(unused)]
    pub fn file_not_found(path: Utf8PathBuf) -> Self {
        Self::FileNotFound { path }
    }

    #[allow(unused)]
    pub fn directory_creation_failed(path: Utf8PathBuf, source: std::io::Error) -> Self {
        Self::DirectoryCreationFailed { path, source }
    }

    #[allow(unused)]
    pub fn invalid_base_layer_priority(provided: i32) -> Self {
        Self::InvalidBaseLayerPriority { provided }
    }
}
