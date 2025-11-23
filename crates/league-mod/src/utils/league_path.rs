use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Validates if a path points to a valid League of Legends executable
/// The path must exist and point to "League of Legends.exe"
pub fn is_valid_league_path(path: &str) -> bool {
    let p = Path::new(path);
    
    // Check if file exists
    if !p.exists() {
        return false;
    }
    
    // Check if it's the correct executable name
    if let Some(file_name) = p.file_name() {
        if let Some(name_str) = file_name.to_str() {
            return name_str == "League of Legends.exe";
        }
    }
    
    false
}

/// Get all available drive letters on Windows using WMIC
/// Returns common drives if detection fails
fn get_available_drives() -> Vec<String> {
    // Only works on Windows
    if cfg!(not(target_os = "windows")) {
        return Vec::new();
    }

    // Try to detect drives using WMIC
    if let Ok(output) = Command::new("wmic")
        .args(&["logicaldisk", "get", "caption"])
        .output()
    {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            let mut drives = Vec::new();
            
            for line in stdout.lines() {
                // Look for drive letters (e.g., "C:")
                if let Some(first_char) = line.trim().chars().next() {
                    if first_char.is_ascii_uppercase() && line.contains(':') {
                        drives.push(first_char.to_string());
                    }
                }
            }
            
            if !drives.is_empty() {
                return drives;
            }
        }
    }
    
    // Fallback to common drives if WMIC fails
    vec!["C", "D", "E", "F", "G", "H"]
        .into_iter()
        .map(String::from)
        .collect()
}

/// Detect League installation from RiotClientInstalls.json
/// This is the most reliable method when the file exists
/// File location: C:\ProgramData\Riot Games\RiotClientInstalls.json
fn detect_from_riot_client_installs() -> Option<String> {
    // Get system drive (usually C:)
    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
    
    // Build path to RiotClientInstalls.json
    let riot_installs_path = PathBuf::from(&system_drive)
        .join("ProgramData")
        .join("Riot Games")
        .join("RiotClientInstalls.json");
    
    // Check if file exists
    if !riot_installs_path.exists() {
        return None;
    }
    
    // Try to read and parse the JSON file
    let contents = fs::read_to_string(&riot_installs_path).ok()?;
    let data: serde_json::Value = serde_json::from_str(&contents).ok()?;
    
    // Look for associated_client field
    let associated_client = data.get("associated_client")?.as_object()?;
    
    // Find League of Legends installation
    for (install_path, _) in associated_client {
        if install_path.to_lowercase().contains("league of legends") {
            // Normalize path (convert forward slashes to backslashes on Windows)
            let normalized = install_path.replace('/', "\\");
            
            // Build path to the executable
            let exe_path = PathBuf::from(&normalized)
                .join("Game")
                .join("League of Legends.exe");
            
            // Validate the path
            if let Some(path_str) = exe_path.to_str() {
                if is_valid_league_path(path_str) {
                    return Some(path_str.to_string());
                }
            }
        }
    }
    
    None
}

/// Detect League installation from running process
/// Tries to find LeagueClientUx.exe or LeagueClient.exe
fn detect_from_running_process() -> Option<String> {
    // Only works on Windows
    if cfg!(not(target_os = "windows")) {
        return None;
    }

    // Try LeagueClientUx.exe first (more reliable)
    if let Some(path) = detect_process("LeagueClientUx.exe") {
        return Some(path);
    }
    
    // Try LeagueClient.exe
    if let Some(path) = detect_process("LeagueClient.exe") {
        return Some(path);
    }
    
    // Try League of Legends.exe directly
    detect_process("League of Legends.exe")
}

/// Helper function to detect a specific process and extract its path
fn detect_process(process_name: &str) -> Option<String> {
    let output = Command::new("wmic")
        .args(&[
            "process",
            "where",
            &format!("name='{}'", process_name),
            "get",
            "ExecutablePath",
            "/value",
        ])
        .output()
        .ok()?;
    
    let stdout = String::from_utf8(output.stdout).ok()?;
    
    // Parse output to find ExecutablePath
    for line in stdout.lines() {
        if line.starts_with("ExecutablePath=") {
            let path = line
                .trim_start_matches("ExecutablePath=")
                .trim()
                .replace('\r', "");
            
            if !path.is_empty() {
                // If we found LeagueClient.exe or LeagueClientUx.exe, 
                // we need to find the Game folder
                if process_name == "LeagueClient.exe" || process_name == "LeagueClientUx.exe" {
                    let root_path = Path::new(&path).parent()?;
                    let game_exe = root_path.join("Game").join("League of Legends.exe");
                    
                    if let Some(game_path) = game_exe.to_str() {
                        if is_valid_league_path(game_path) {
                            return Some(game_path.to_string());
                        }
                    }
                } else {
                    // For "League of Legends.exe", the path is already correct
                    if is_valid_league_path(&path) {
                        return Some(path);
                    }
                }
            }
        }
    }
    
    None
}

/// Check common installation paths on all available drives
fn detect_from_common_paths() -> Option<String> {
    let drives = get_available_drives();
    let mut paths_to_check = Vec::new();
    
    // Build list of common installation paths for each drive
    for drive in drives {
        paths_to_check.push(format!(
            "{}:\\Riot Games\\League of Legends\\Game\\League of Legends.exe",
            drive
        ));
        paths_to_check.push(format!(
            "{}:\\Program Files\\Riot Games\\League of Legends\\Game\\League of Legends.exe",
            drive
        ));
        paths_to_check.push(format!(
            "{}:\\Program Files (x86)\\Riot Games\\League of Legends\\Game\\League of Legends.exe",
            drive
        ));
    }
    
    // Check each path
    for path in paths_to_check {
        if is_valid_league_path(&path) {
            return Some(path);
        }
    }
    
    None
}

/// Detect League installation from Windows Registry
fn detect_from_registry() -> Option<String> {
    // Only works on Windows
    if cfg!(not(target_os = "windows")) {
        return None;
    }

    // Try to query the registry for League installation path
    let output = Command::new("reg")
        .args(&[
            "query",
            "HKLM\\SOFTWARE\\WOW6432Node\\Riot Games, Inc\\League of Legends",
            "/v",
            "Location",
        ])
        .output()
        .ok()?;
    
    let stdout = String::from_utf8(output.stdout).ok()?;
    
    // Parse the output to find the Location value
    for line in stdout.lines() {
        if line.contains("Location") && line.contains("REG_SZ") {
            // Extract the path (format: "Location    REG_SZ    C:\Path\To\League")
            let parts: Vec<&str> = line.split("REG_SZ").collect();
            if parts.len() >= 2 {
                let root_path = parts[1].trim();
                let game_exe = PathBuf::from(root_path)
                    .join("Game")
                    .join("League of Legends.exe");
                
                if let Some(path_str) = game_exe.to_str() {
                    if is_valid_league_path(path_str) {
                        return Some(path_str.to_string());
                    }
                }
            }
        }
    }
    
    None
}

/// Comprehensive auto-detection of League of Legends installation
/// Uses multiple detection methods in order of reliability:
/// 1. RiotClientInstalls.json (most reliable)
/// 2. Running League processes
/// 3. Common installation paths on all available drives
/// 4. Windows Registry
///
/// Returns the path to "League of Legends.exe" if found, None otherwise
pub fn auto_detect_league_path() -> Option<String> {
    // Only supported on Windows
    if cfg!(not(target_os = "windows")) {
        return None;
    }
    
    // Method 1: Try RiotClientInstalls.json first (most reliable)
    if let Some(path) = detect_from_riot_client_installs() {
        return Some(path);
    }
    
    // Method 2: Try to detect from running process
    if let Some(path) = detect_from_running_process() {
        return Some(path);
    }
    
    // Method 3: Check common installation paths
    if let Some(path) = detect_from_common_paths() {
        return Some(path);
    }
    
    // Method 4: Try Windows Registry as last resort
    if let Some(path) = detect_from_registry() {
        return Some(path);
    }
    
    // No League installation found
    None
}




