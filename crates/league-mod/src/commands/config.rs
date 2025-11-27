use crate::utils::config::{self, AppConfig};
use crate::utils::league_path;
use colored::Colorize;
use miette::Result;

fn update_league_path_in_config(path: String) -> Result<()> {
    let mut cfg = config::load_config();
    cfg.league_path = Some(path);
    config::save_config(&cfg).map_err(|e| miette::miette!("Failed to save config: {}", e))
}

pub fn show_config() -> Result<()> {
    let cfg = config::load_config();
    let config_path = config::default_config_path()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    println!("\n{}", "League Mod Configuration".bright_cyan().bold());
    println!("{}", "========================".bright_cyan());
    println!();
    println!("  {} {}", "Config file:".bright_white().bold(), config_path);
    println!();

    match &cfg.league_path {
        Some(path) => {
            println!(
                "  {} {}",
                "League Path:".bright_white().bold(),
                path.bright_green()
            );

            if league_path::is_valid_league_path(path) {
                println!(
                    "  {} {}",
                    "Status:".bright_white().bold(),
                    "Valid ✓".bright_green()
                );
            } else {
                println!(
                    "  {} {}",
                    "Status:".bright_white().bold(),
                    "Invalid (file not found or incorrect) ✗".bright_red()
                );
            }
        }
        None => {
            println!(
                "  {} {}",
                "League Path:".bright_white().bold(),
                "Not set".bright_yellow()
            );
            println!();
            println!(
                "  {}",
                "Run 'league-mod config auto-detect' to automatically find League installation"
                    .bright_yellow()
            );
            println!(
                "  {}",
                "Or use 'league-mod config set-league-path <path>' to set it manually"
                    .bright_yellow()
            );
        }
    }

    println!();
    Ok(())
}

pub fn set_league_path(path: String) -> Result<()> {
    if !league_path::is_valid_league_path(&path) {
        eprintln!(
            "  {}",
            "The path must point to 'League of Legends.exe' in the Game folder.".bright_yellow()
        );
        eprintln!(
            "  {}",
            "Example: C:\\Riot Games\\League of Legends\\Game\\League of Legends.exe"
                .bright_yellow()
        );
        eprintln!();
        eprintln!("  {} The file does not exist", "•".bright_red());
        eprintln!(
            "  {} The file is not named 'League of Legends.exe'",
            "•".bright_red()
        );

        return Err(miette::miette!("Invalid League of Legends path"));
    }

    update_league_path_in_config(path.clone())?;

    println!(
        "{}",
        "✓ League path set successfully!".bright_green().bold()
    );
    println!();
    println!(
        "  {} {}",
        "Path:".bright_white().bold(),
        path.bright_green()
    );

    Ok(())
}

pub fn auto_detect_league_path() -> Result<()> {
    println!(
        "{}",
        "Searching for League of Legends installation...".bright_cyan()
    );
    println!();

    match league_path::auto_detect_league_path() {
        Some(detected_path) => {
            println!("{}", "✓ Found League of Legends!".bright_green().bold());
            println!();
            println!(
                "  {} {}",
                "Path:".bright_white().bold(),
                detected_path.bright_green()
            );
            println!();

            update_league_path_in_config(detected_path)?;

            println!(
                "{}",
                "✓ Configuration updated successfully!"
                    .bright_green()
                    .bold()
            );
        }
        None => {
            println!(
                "{}",
                "✗ Could not automatically detect League of Legends installation"
                    .bright_red()
                    .bold()
            );
            println!();
            println!(
                "  {}",
                "League of Legends may not be installed, or it's in a non-standard location."
                    .bright_yellow()
            );
            println!();
            println!(
                "  {} Use 'league-mod config set-league-path <path>' to set the path manually",
                "•".bright_cyan()
            );
            println!(
                "  {} The path should point to: ...\\Game\\League of Legends.exe",
                "•".bright_cyan()
            );
            println!();
            println!(
                "  {} C:\\Riot Games\\League of Legends\\Game\\League of Legends.exe",
                "Example:".bright_white().bold()
            );
        }
    }

    Ok(())
}

pub fn reset_config() -> Result<()> {
    let config_path = config::default_config_path()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let default_cfg = AppConfig::default();
    config::save_config(&default_cfg)
        .map_err(|e| miette::miette!("Failed to reset config: {}", e))?;

    println!(
        "{}",
        "✓ Configuration reset to defaults".bright_green().bold()
    );
    println!();
    println!("  {} {}", "Config file:".bright_white().bold(), config_path);
    println!();
    println!(
        "  {}",
        "Run 'league-mod config auto-detect' to find your League installation".bright_cyan()
    );

    Ok(())
}

/// Ensures config.toml exists and attempts auto-detection if league_path is not set.
pub fn ensure_config_exists() -> Result<()> {
    let (cfg, _path) = config::load_or_create_config()
        .map_err(|e| miette::miette!("Failed to initialize config: {}", e))?;

    if cfg.league_path.is_none() {
        if let Some(detected_path) = league_path::auto_detect_league_path() {
            let mut updated_cfg = cfg;
            updated_cfg.league_path = Some(detected_path);
            let _ = config::save_config(&updated_cfg);
        }
    }

    Ok(())
}
