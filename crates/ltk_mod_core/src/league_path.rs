//! League of Legends path detection and validation utilities.

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;
use sysinfo::{Disks, System};

/// Validates if a path points to a valid League of Legends executable.
pub fn is_valid_league_path(path: &Utf8Path) -> bool {
    if !path.exists() {
        return false;
    }
    if let Some(file_name) = path.file_name() {
        return file_name == "League of Legends.exe";
    }
    false
}

/// Get all available drives using sysinfo (cross-platform).
fn get_available_drives() -> Vec<String> {
    let disks = Disks::new_with_refreshed_list();

    let mut drives: Vec<String> = disks
        .iter()
        .filter_map(|disk| disk.mount_point().to_str().map(|s| s.to_string()))
        .collect();

    // Fallback to common Windows drives if detection fails
    if drives.is_empty() && cfg!(target_os = "windows") {
        drives = vec!["C:", "D:", "E:", "F:", "G:", "H:"]
            .into_iter()
            .map(String::from)
            .collect();
    }

    drives
}

/// Detect League installation from RiotClientInstalls.json.
fn detect_from_riot_client_installs() -> Option<Utf8PathBuf> {
    // Build path: C:\ProgramData\Riot Games\RiotClientInstalls.json
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    let system_root = format!("{}\\", system_drive);

    let riot_installs_path = Utf8PathBuf::from(&system_root)
        .join("ProgramData")
        .join("Riot Games")
        .join("RiotClientInstalls.json");

    if !riot_installs_path.exists() {
        return None;
    }

    let contents = fs::read_to_string(riot_installs_path.as_str()).ok()?;
    let data: serde_json::Value = serde_json::from_str(&contents).ok()?;
    let associated_client = data.get("associated_client")?.as_object()?;

    for (install_path, _) in associated_client {
        // Remove trailing separator (file_name() returns None for paths ending in separator)
        let cleaned_path = install_path.trim_end_matches(['/', '\\']);
        let normalized_path = Utf8PathBuf::from(cleaned_path);

        // Check for exact "League of Legends" folder (excludes PBE)
        if let Some(folder_name) = normalized_path.file_name() {
            if folder_name == "League of Legends" {
                let exe_path = normalized_path.join("Game").join("League of Legends.exe");
                if is_valid_league_path(&exe_path) {
                    return Some(exe_path);
                }
            }
        }
    }

    None
}

/// Detect League installation from running process using sysinfo.
fn detect_from_running_process() -> Option<Utf8PathBuf> {
    let system = System::new_all();

    let check_process = |name: &str| -> Option<Utf8PathBuf> {
        for process in system.processes_by_name(name.as_ref()) {
            let path = process
                .exe()
                .and_then(|p| Utf8PathBuf::from_path_buf(p.to_path_buf()).ok())?;

            // For client processes, navigate to Game folder
            if name == "LeagueClientUx.exe" || name == "LeagueClient.exe" {
                let root_path = path.parent()?;
                let game_exe = root_path.join("Game").join("League of Legends.exe");

                if is_valid_league_path(&game_exe) {
                    return Some(game_exe);
                }
                continue;
            }

            if is_valid_league_path(&path) {
                return Some(path);
            }
        }
        None
    };

    // Try processes in order of reliability
    check_process("LeagueClientUx.exe")
        .or_else(|| check_process("LeagueClient.exe"))
        .or_else(|| check_process("League of Legends.exe"))
}

/// Check common installation paths on all available drives.
fn detect_from_common_paths() -> Option<Utf8PathBuf> {
    let drives = get_available_drives();
    let mut paths_to_check = Vec::new();

    for drive in &drives {
        let drive_root = drive.trim_end_matches(['\\', '/']);

        paths_to_check.push(
            Utf8PathBuf::from(drive_root)
                .join("Riot Games")
                .join("League of Legends")
                .join("Game")
                .join("League of Legends.exe"),
        );
        paths_to_check.push(
            Utf8PathBuf::from(drive_root)
                .join("Program Files")
                .join("Riot Games")
                .join("League of Legends")
                .join("Game")
                .join("League of Legends.exe"),
        );
        paths_to_check.push(
            Utf8PathBuf::from(drive_root)
                .join("Program Files (x86)")
                .join("Riot Games")
                .join("League of Legends")
                .join("Game")
                .join("League of Legends.exe"),
        );
    }

    paths_to_check
        .into_iter()
        .find(|path| is_valid_league_path(path))
}

/// Detect League installation from Windows Registry.
fn detect_from_registry() -> Option<Utf8PathBuf> {
    if cfg!(not(target_os = "windows")) {
        return None;
    }

    let output = std::process::Command::new("reg")
        .args([
            "query",
            "HKLM\\SOFTWARE\\WOW6432Node\\Riot Games, Inc\\League of Legends",
            "/v",
            "Location",
        ])
        .output()
        .ok()?;

    let stdout = String::from_utf8(output.stdout).ok()?;

    for line in stdout.lines() {
        if line.contains("Location") && line.contains("REG_SZ") {
            let parts: Vec<&str> = line.split("REG_SZ").collect();
            if parts.len() >= 2 {
                let root_path = parts[1].trim();
                let game_exe = Utf8PathBuf::from(root_path)
                    .join("Game")
                    .join("League of Legends.exe");

                if is_valid_league_path(&game_exe) {
                    return Some(game_exe);
                }
            }
        }
    }

    None
}

/// Auto-detect League of Legends installation.
///
/// Detection methods (in order of reliability):
/// 1. RiotClientInstalls.json
/// 2. Running League processes
/// 3. Common installation paths
/// 4. Windows Registry
pub fn auto_detect_league_path() -> Option<Utf8PathBuf> {
    detect_from_riot_client_installs()
        .or_else(detect_from_running_process)
        .or_else(detect_from_common_paths)
        .or_else(detect_from_registry)
}
