use camino::Utf8Path;
use miette::{IntoDiagnostic, WrapErr, miette};

use ltk_sig::io::root::{Root, RootParams};
use ltk_sig::io::statement_set::StatementSet;

use crate::util::{encode_hex, load_signing_key, parse_set_spec, unix_now};

pub fn run(
    key_path: &Utf8Path,
    set_specs: &[String],
    predecessor: Option<&Utf8Path>,
    output: &Utf8Path,
) -> miette::Result<()> {
    let key = load_signing_key(key_path)?;

    let mut roots: Vec<(String, [u8; 32])> = Vec::with_capacity(set_specs.len());
    for spec in set_specs {
        let (name, path) = parse_set_spec(spec)?;
        let bytes = std::fs::read(path.as_std_path())
            .into_diagnostic()
            .wrap_err_with(|| format!("reading set file {path}"))?;
        let set = StatementSet::from_bytes(&bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("parsing set file {path}"))?;
        roots.push((name, set.digest()));
    }
    roots.sort_by(|a, b| a.0.cmp(&b.0));
    if roots.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(miette!("duplicate set name"));
    }

    // Supersession rule: iat = max(now, predecessor + 1), so a clock step
    // can neither mint two distinct roots at one issue time nor regress
    // supersession. The predecessor must verify under the signing key —
    // flooring on a foreign or forged token could poison the ratchet.
    let mut time_issued_at = unix_now();
    if let Some(pred_path) = predecessor {
        let bytes = std::fs::read(pred_path.as_std_path())
            .into_diagnostic()
            .wrap_err_with(|| format!("reading predecessor root {pred_path}"))?;
        let pred = Root::from_bytes(&bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("parsing predecessor root {pred_path}"))?;
        pred.verify_signature(&key.verifying_key().to_bytes())
            .into_diagnostic()
            .wrap_err("predecessor root is not signed by this key")?;
        if pred.time_issued_at() >= time_issued_at {
            println!(
                "predecessor iat {} is not in the past; deriving iat as predecessor + 1",
                pred.time_issued_at()
            );
            time_issued_at = pred.time_issued_at() + 1;
        }
    }

    let root = Root::sign(
        &RootParams {
            time_issued_at,
            roots: &roots,
        },
        &key,
    )
    .into_diagnostic()?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent.as_std_path()).into_diagnostic()?;
    }
    std::fs::write(output.as_std_path(), root.as_bytes())
        .into_diagnostic()
        .wrap_err_with(|| format!("writing root token {output}"))?;

    println!(
        "root signed at iat={} covering {} set(s):",
        root.time_issued_at(),
        root.roots().len()
    );
    for (name, digest) in root.roots() {
        println!("  {name} = {}", encode_hex(digest));
    }
    println!("written to {output}");
    Ok(())
}
