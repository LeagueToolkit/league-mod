use std::fs::File;

use camino::{Utf8Path, Utf8PathBuf};
use colored::Colorize;
use ltk_modpkg::Modpkg;
use ltk_overlay::skin_integrity::check_single_mod;
use ltk_overlay::{EnabledMod, FantomeContent, FsModContent, ModContentProvider, ModpkgContent};
use miette::{miette, IntoDiagnostic};

use crate::println_pad;
use crate::utils::config;

pub struct SanitizeModArgs {
    pub file_path: String,
    pub game_dir: Option<String>,
}

/// Verify a mod's base-skin integrity against the game files, straight from
/// the packaged archive — nothing is installed or extracted to disk.
///
/// This is the same closed-world check the in-game verifier enforces: the
/// base skin's mesh references must resolve inside the champion WAD the game
/// loads. Violations mean the mod is broken (missing assets), outdated
/// (references assets removed from the game), or mis-packaged (assets shipped
/// to the wrong WAD).
pub fn sanitize_mod(args: SanitizeModArgs) -> miette::Result<()> {
    // Violations and baseline anomalies inside the overlay/sanitize stack are
    // reported via `tracing`; surface warnings and errors on stderr.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let mod_path = Utf8PathBuf::from(&args.file_path);
    let game_dir = resolve_game_dir(args.game_dir.map(Utf8PathBuf::from))?;

    let mut enabled_mod = EnabledMod {
        id: mod_path.file_stem().unwrap_or("mod").to_string(),
        content: open_content(&mod_path)?,
        enabled_layers: None,
    };

    let index_cache = config::config_path("game_index.bin").unwrap_or_else(|| {
        Utf8PathBuf::from_path_buf(std::env::temp_dir().join("league-mod-game-index.bin"))
            .unwrap_or_else(|_| Utf8PathBuf::from("league-mod-game-index.bin"))
    });

    let offenders = check_single_mod(&game_dir, &index_cache, &mut enabled_mod)
        .map_err(|err| miette!("{err}"))?;

    if offenders.is_empty() {
        println_pad!(
            "{} {}",
            "✅".bright_green(),
            "Base-skin integrity OK".bright_green().bold()
        );
        return Ok(());
    }

    for offender in &offenders {
        println_pad!(
            "{} {} ({})",
            "❌ Broken base skin:".bright_red().bold(),
            offender.champion.bright_cyan().bold(),
            offender.wad.bright_white()
        );
        for violation in &offender.violations {
            println_pad!("   {} {}", "-".bright_red(), violation);
        }
    }
    Err(miette!(
        "{} champion WAD(s) violate base-skin integrity — this mod would be rejected in-game",
        offenders.len()
    ))
}

/// Open the mod content by path kind: a `.fantome`/`.modpkg` archive (read
/// in-memory, never extracted) or a mod project directory.
fn open_content(path: &Utf8Path) -> miette::Result<Box<dyn ModContentProvider>> {
    if path.is_dir() {
        return Ok(Box::new(FsModContent::new(path.to_owned())));
    }
    match path
        .extension()
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("fantome") => {
            let file = File::open(path.as_std_path()).into_diagnostic()?;
            Ok(Box::new(
                FantomeContent::new(file).map_err(|err| miette!("{err}"))?,
            ))
        }
        Some("modpkg") => {
            let file = File::open(path.as_std_path()).into_diagnostic()?;
            let modpkg = Modpkg::mount_from_reader(file).into_diagnostic()?;
            Ok(Box::new(ModpkgContent::new(modpkg)))
        }
        _ => Err(miette!(
            "unsupported mod format: '{path}' (expected a .fantome or .modpkg file, or a mod project directory)"
        )),
    }
}

/// Resolve the game directory (the one containing `DATA/FINAL`) from the
/// explicit argument or the configured League path, accepting the game dir
/// itself, the install root, or the game executable path.
fn resolve_game_dir(arg: Option<Utf8PathBuf>) -> miette::Result<Utf8PathBuf> {
    let base = match arg {
        Some(dir) => dir,
        None => config::load_config().league_path.ok_or_else(|| {
            miette!(
                "no game directory: pass --game-dir or set the League path with `league-mod config set-league-path`"
            )
        })?,
    };

    let base = if base.is_file() {
        base.parent().unwrap_or(&base).to_owned()
    } else {
        base
    };
    for candidate in [base.clone(), base.join("Game")] {
        if candidate.join("DATA").join("FINAL").as_std_path().exists() {
            return Ok(candidate);
        }
    }
    Err(miette!(
        "'{base}' does not look like a League game directory (no DATA/FINAL found)"
    ))
}
