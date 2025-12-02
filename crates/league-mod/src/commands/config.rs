use crate::utils::config::{self, AppConfig};
use crate::utils::league_path;
use camino::Utf8PathBuf;
use colored::Colorize;
use miette::Result;

fn update_league_path_in_config(path: Utf8PathBuf) -> Result<()> {
    let mut cfg = config::load_config();
    cfg.league_path = Some(path);
    config::save_config(&cfg).map_err(|e| miette::miette!("Failed to save config: {}", e))
}

/// Print a config path entry with status indicator
fn print_path_config(
    name: &str,
    path: Option<&Utf8PathBuf>,
    validator: impl Fn(&Utf8PathBuf) -> bool,
) {
    match path {
        Some(p) => {
            let status = if validator(p) {
                "✓".bright_green()
            } else {
                "✗".bright_red()
            };
            println!("  {} {} {}", format!("{}:", name).bright_white(), p, status);
        }
        None => {
            println!(
                "  {} {}",
                format!("{}:", name).bright_white(),
                "(not set)".bright_yellow()
            );
        }
    }
}

pub fn show_config() -> Result<()> {
    let cfg = config::load_config();
    let config_path = config::default_config_path()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    println!();
    println!("  {} {}", "config_file:".bright_white(), config_path);

    print_path_config("league_path", cfg.league_path.as_ref(), |p| {
        league_path::is_valid_league_path(p.as_path())
    });

    print_path_config("hashtable_dir", cfg.hashtable_dir.as_ref(), |p| p.exists());

    println!();
    Ok(())
}

pub fn set_league_path(path: String) -> Result<()> {
    let path = Utf8PathBuf::from(&path);
    if !league_path::is_valid_league_path(path.as_path()) {
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
        path.as_str().bright_green()
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
                detected_path.as_str().bright_green()
            );
            println!();

            update_league_path_in_config(detected_path.clone())?;

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
