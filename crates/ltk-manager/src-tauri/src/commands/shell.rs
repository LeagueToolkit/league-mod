use std::path::PathBuf;

use crate::error::{AppError, AppResult, IpcResult};

/// Opens a file location in the system file explorer.
#[tauri::command]
pub async fn reveal_in_explorer(path: String) -> IpcResult<()> {
    reveal_in_explorer_inner(&path).into()
}

fn reveal_in_explorer_inner(path: &str) -> AppResult<()> {
    let path = PathBuf::from(path);

    // Get the parent directory if it's a file
    let dir = if path.is_file() {
        path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
    } else {
        path
    };

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Other(format!("Failed to open explorer: {}", e)))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Other(format!("Failed to open Finder: {}", e)))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| AppError::Other(format!("Failed to open file manager: {}", e)))?;
    }

    Ok(())
}
