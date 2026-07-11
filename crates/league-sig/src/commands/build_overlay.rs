use std::fs::File;
use std::io::BufReader;

use camino::{Utf8Path, Utf8PathBuf};
use miette::{IntoDiagnostic, WrapErr, miette};

use ltk_overlay::{EnabledMod, FantomeContent, OverlayBuilder};

pub fn run(
    game_dir: &Utf8Path,
    output: &Utf8Path,
    state_dir: Option<&Utf8Path>,
    mods: &[Utf8PathBuf],
) -> miette::Result<()> {
    let state_dir = match state_dir {
        Some(dir) => dir.to_path_buf(),
        None => Utf8PathBuf::from(format!("{output}.state")),
    };

    let mut enabled = Vec::with_capacity(mods.len());
    for mod_path in mods {
        let file = File::open(mod_path.as_std_path())
            .into_diagnostic()
            .wrap_err_with(|| format!("opening {mod_path}"))?;
        let content = FantomeContent::new(BufReader::new(file))
            .into_diagnostic()
            .wrap_err_with(|| format!("opening fantome archive {mod_path}"))?
            .with_archive_path(mod_path.clone());
        enabled.push(EnabledMod {
            id: mod_path
                .file_name()
                .ok_or_else(|| miette!("mod path {mod_path} has no file name"))?
                .to_owned(),
            content: Box::new(content),
            enabled_layers: None,
        });
    }

    let mut builder = OverlayBuilder::new(
        game_dir.to_path_buf(),
        output.to_path_buf(),
        state_dir.clone(),
    );
    builder.set_enabled_mods(enabled);
    let result = builder
        .build()
        .into_diagnostic()
        .wrap_err("overlay build")?;

    println!(
        "overlay at {}: {} WAD(s) built, {} reused, in {:?}",
        result.overlay_root,
        result.wads_built.len(),
        result.wads_reused.len(),
        result.build_time
    );
    for wad in &result.wads_built {
        println!("  built  {wad}");
    }
    for wad in &result.wads_reused {
        println!("  reused {wad}");
    }
    Ok(())
}
