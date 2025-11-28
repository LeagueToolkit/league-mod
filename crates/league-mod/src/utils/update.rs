use std::io;
use std::time::Duration;

use colored::Colorize;
use semver::Version;

#[derive(Debug, serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Default)]
struct UpdateConfig {
    last_update_check_epoch_secs: u64,
}

fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

#[allow(dead_code)]
pub fn check_for_update_nonblocking() {
    std::thread::spawn(|| {
        if let Err(_e) = try_check_and_log_update() {
            // Silently ignore update check failures.
        }
    });
}

pub fn check_for_update_blocking() {
    let _ = try_check_and_log_update();
}

fn try_check_and_log_update() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Throttle: only check once every 5 minutes
    if is_throttled(5 * 60).unwrap_or(false) {
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "league-mod/{} (+https://github.com/LeagueToolkit/league-mod)",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_millis(800))
        .build()?;

    let resp = client
        .get("https://api.github.com/repos/LeagueToolkit/league-mod/releases/latest")
        .send()?;

    if !resp.status().is_success() {
        return Ok(());
    }

    // Record a check attempt regardless of network outcome to avoid stampede
    let _ = write_last_check_now();

    let release: GithubRelease = resp.json()?;

    let tag = release.tag_name.as_str();
    let stripped = match tag.strip_prefix("league-mod-") {
        Some(s) => s,
        None => return Ok(()),
    };
    let latest_version_str = stripped.trim_start_matches('v');
    let latest_version = match Version::parse(latest_version_str) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let current = current_version();
    if latest_version > current {
        let line1 = format!(
            "{} {} → {}",
            "ℹ Update available:".bright_yellow().bold(),
            current.to_string().bright_white(),
            latest_version.to_string().bright_green().bold()
        );
        let line2 = format!(
            "{} {}",
            "Get it here:".bright_cyan(),
            release.html_url.bright_blue().underline()
        );

        crate::utils::print_ansi_boxed_lines(&[line1, line2]);
    }

    Ok(())
}

fn read_update_config() -> io::Result<UpdateConfig> {
    if let Some(path) = crate::utils::config::config_path("league-mod.config.json") {
        // Convert Utf8PathBuf to std::path::Path for read_json
        let std_path = std::path::Path::new(path.as_str());
        if let Some(cfg) = crate::utils::config::read_json::<UpdateConfig>(std_path)? {
            return Ok(cfg);
        }
    }
    Ok(UpdateConfig::default())
}

fn write_update_config(cfg: &UpdateConfig) -> io::Result<()> {
    if let Some(path) = crate::utils::config::config_path("league-mod.config.json") {
        // Convert Utf8PathBuf to std::path::Path for write_json_pretty
        let std_path = std::path::Path::new(path.as_str());
        // Best-effort write; create or overwrite
        let _ = crate::utils::config::write_json_pretty(std_path, cfg);
    }
    Ok(())
}

fn now_epoch_secs() -> u64 {
    crate::utils::config::now_epoch_secs()
}

fn is_throttled(min_interval_secs: u64) -> io::Result<bool> {
    let cfg = read_update_config()?;
    let now = now_epoch_secs();
    Ok(cfg.last_update_check_epoch_secs != 0
        && now.saturating_sub(cfg.last_update_check_epoch_secs) < min_interval_secs)
}

fn write_last_check_now() -> io::Result<()> {
    let mut cfg = read_update_config()?;
    cfg.last_update_check_epoch_secs = now_epoch_secs();
    write_update_config(&cfg)
}
