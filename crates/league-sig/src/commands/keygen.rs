use camino::Utf8Path;
use miette::{IntoDiagnostic, WrapErr};

use crate::util::encode_hex;

pub fn run(output: &Utf8Path) -> miette::Result<()> {
    let key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent.as_std_path()).into_diagnostic()?;
    }
    std::fs::write(output.as_std_path(), key.to_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("writing key file {output}"))?;

    println!("private key seed: {output}");
    println!(
        "public key:       {}",
        encode_hex(&key.verifying_key().to_bytes())
    );
    Ok(())
}
