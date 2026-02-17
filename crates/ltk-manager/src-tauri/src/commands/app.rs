use crate::error::IpcResult;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub log_file_path: Option<String>,
}

/// Get basic app information.
#[tauri::command]
pub fn get_app_info() -> IpcResult<AppInfo> {
    let log_file_path = crate::default_log_dir().map(|p| p.to_string_lossy().into_owned());

    IpcResult::ok(AppInfo {
        name: "LTK Manager".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        log_file_path,
    })
}
