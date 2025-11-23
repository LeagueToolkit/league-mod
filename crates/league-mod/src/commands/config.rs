use crate::utils::config::{self, AppConfig};
use crate::utils::league_path;
use colored::Colorize;
use miette::Result;

/// Shows the current configuration
pub fn show_config() -> Result<()> {
    let cfg = config::load_config();
    let config_path = config::default_config_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    println!("\n{}", "League Mod Configuration".bright_cyan().bold());
    println!("{}", "========================".bright_cyan());
    println!();
    println!("  {} {}", "Config file:".bright_white().bold(), config_path);
    println!();
    
    // Display league_path
    match &cfg.league_path {
        Some(path) => {
            println!("  {} {}", "League Path:".bright_white().bold(), path.bright_green());
            
            // Validate the path
            if league_path::is_valid_league_path(path) {
                println!("  {} {}", "Status:".bright_white().bold(), "Valid ✓".bright_green());
            } else {
                println!("  {} {}", "Status:".bright_white().bold(), "Invalid (file not found or incorrect) ✗".bright_red());
            }
        }
        None => {
            println!("  {} {}", "League Path:".bright_white().bold(), "Not set".bright_yellow());
            println!();
            println!("  {}", "Run 'league-mod config auto-detect' to automatically find League installation".bright_yellow());
            println!("  {}", "Or use 'league-mod config set-league-path <path>' to set it manually".bright_yellow());
        }
    }
    
    println!();
    
    Ok(())
}

/// Sets the League of Legends path manually with validation
pub fn set_league_path(path: String) -> Result<()> {
    // Validate the path first
    if !league_path::is_valid_league_path(&path) {
        eprintln!("{}", "Error: Invalid League of Legends path".bright_red().bold());
        eprintln!();
        eprintln!("  {}", "The path must point to 'League of Legends.exe' in the Game folder.".bright_yellow());
        eprintln!("  {}", "Example: C:\\Riot Games\\League of Legends\\Game\\League of Legends.exe".bright_yellow());
        eprintln!();
        eprintln!("  {} The file does not exist", "•".bright_red());
        eprintln!("  {} The file is not named 'League of Legends.exe'", "•".bright_red());
        eprintln!();
        std::process::exit(1);
    }
    
    // Load existing config
    let mut cfg = config::load_config();
    
    // Update league path
    cfg.league_path = Some(path.clone());
    
    // Save config
    config::save_config(&cfg).map_err(|e| {
        miette::miette!("Failed to save config: {}", e)
    })?;
    
    println!("{}", "✓ League path set successfully!".bright_green().bold());
    println!();
    println!("  {} {}", "Path:".bright_white().bold(), path.bright_green());
    
    Ok(())
}

/// Automatically detects the League of Legends installation path
pub fn auto_detect_league_path() -> Result<()> {
    println!("{}", "Searching for League of Legends installation...".bright_cyan());
    println!();
    
    // Run auto-detection
    match league_path::auto_detect_league_path() {
        Some(detected_path) => {
            println!("{}", "✓ Found League of Legends!".bright_green().bold());
            println!();
            println!("  {} {}", "Path:".bright_white().bold(), detected_path.bright_green());
            println!();
            
            // Load existing config
            let mut cfg = config::load_config();
            
            // Update league path
            cfg.league_path = Some(detected_path.clone());
            
            // Save config
            config::save_config(&cfg).map_err(|e| {
                miette::miette!("Failed to save config: {}", e)
            })?;
            
            println!("{}", "✓ Configuration updated successfully!".bright_green().bold());
        }
        None => {
            println!("{}", "✗ Could not automatically detect League of Legends installation".bright_red().bold());
            println!();
            println!("  {}", "League of Legends may not be installed, or it's in a non-standard location.".bright_yellow());
            println!();
            println!("  {} Use 'league-mod config set-league-path <path>' to set the path manually", "•".bright_cyan());
            println!("  {} The path should point to: ...\\Game\\League of Legends.exe", "•".bright_cyan());
            println!();
            println!("  {} C:\\Riot Games\\League of Legends\\Game\\League of Legends.exe", "Example:".bright_white().bold());
        }
    }
    
    Ok(())
}

/// Resets the configuration to defaults
pub fn reset_config() -> Result<()> {
    // Get config path for display
    let config_path = config::default_config_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    
    // Create default config
    let default_cfg = AppConfig::default();
    
    // Save default config (overwrites existing)
    config::save_config(&default_cfg).map_err(|e| {
        miette::miette!("Failed to reset config: {}", e)
    })?;
    
    println!("{}", "✓ Configuration reset to defaults".bright_green().bold());
    println!();
    println!("  {} {}", "Config file:".bright_white().bold(), config_path);
    println!();
    println!("  {}", "Run 'league-mod config auto-detect' to find your League installation".bright_cyan());
    
    Ok(())
}

/// Ensures config.toml exists, creates it with defaults if not.
/// Also attempts auto-detection on first run if league_path is not set.
pub fn ensure_config_exists() -> Result<()> {
    // Try to load or create config
    let (cfg, _path) = config::load_or_create_config().map_err(|e| {
        miette::miette!("Failed to initialize config: {}", e)
    })?;
    
    // If league_path is not set, try auto-detection (first run behavior)
    if cfg.league_path.is_none() {
        if let Some(detected_path) = league_path::auto_detect_league_path() {
            // Silently save the detected path
            let mut updated_cfg = cfg;
            updated_cfg.league_path = Some(detected_path);
            let _ = config::save_config(&updated_cfg);
        }
    }
    
    Ok(())
}




