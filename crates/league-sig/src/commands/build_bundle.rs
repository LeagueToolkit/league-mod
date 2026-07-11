use camino::{Utf8Path, Utf8PathBuf};
use miette::{IntoDiagnostic, WrapErr};

use ltk_sig::io::bundle::{Bundle, BundleParams};

use crate::util::{encode_hex, load_statements};

pub fn run(
    roots: &[Utf8PathBuf],
    sets: &[Utf8PathBuf],
    statements_dir: Option<&Utf8Path>,
    output: &Utf8Path,
) -> miette::Result<()> {
    let read_all = |paths: &[Utf8PathBuf]| -> miette::Result<Vec<Vec<u8>>> {
        paths
            .iter()
            .map(|path| {
                std::fs::read(path.as_std_path())
                    .into_diagnostic()
                    .wrap_err_with(|| format!("reading {path}"))
            })
            .collect()
    };
    let root_bytes = read_all(roots)?;
    let set_bytes = read_all(sets)?;
    let statement_bytes: Vec<Vec<u8>> = match statements_dir {
        Some(dir) => load_statements(dir)?
            .into_iter()
            .map(|(_, statement)| statement.as_bytes().to_vec())
            .collect(),
        None => Vec::new(),
    };

    let bundle = Bundle::seal(&BundleParams {
        roots: &root_bytes,
        sets: &set_bytes,
        statements: &statement_bytes,
    })
    .into_diagnostic()?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent.as_std_path()).into_diagnostic()?;
    }
    std::fs::write(output.as_std_path(), bundle.as_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("writing bundle {output}"))?;

    println!(
        "bundle {}: {} root(s), {} set(s), {} statement(s), {} bytes",
        encode_hex(&bundle.token_hash()),
        bundle.roots().len(),
        bundle.sets().len(),
        bundle.statements().len(),
        bundle.as_bytes().len()
    );
    println!("written to {output}");
    Ok(())
}
