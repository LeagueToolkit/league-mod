use crate::error::IpcResult;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Get basic app information.
#[tauri::command]
pub fn get_app_info() -> IpcResult<AppInfo> {
    IpcResult::ok(AppInfo {
        name: "LTK Manager".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

