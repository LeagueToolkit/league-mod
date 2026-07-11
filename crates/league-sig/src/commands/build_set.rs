use camino::Utf8Path;
use miette::{IntoDiagnostic, WrapErr};

use ltk_sig::io::statement_set::StatementSet;

use crate::util::{encode_hex, load_statements};

pub fn run(statements_dir: &Utf8Path, output_dir: &Utf8Path) -> miette::Result<()> {
    let statements = load_statements(statements_dir)?;
    let mut hashes: Vec<[u8; 32]> = statements.iter().map(|(_, s)| s.token_hash()).collect();
    hashes.sort();
    hashes.dedup();

    let set = StatementSet::from_hashes(hashes).into_diagnostic()?;
    let digest_hex = encode_hex(&set.digest());

    std::fs::create_dir_all(output_dir.as_std_path()).into_diagnostic()?;
    let out_path = output_dir.join(&digest_hex);
    std::fs::write(out_path.as_std_path(), set.to_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("writing set file {out_path}"))?;

    println!(
        "set {digest_hex}: {} statement(s) from {} token file(s)",
        set.len(),
        statements.len()
    );
    println!("written to {out_path}");
    Ok(())
}
