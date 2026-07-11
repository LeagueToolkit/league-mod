//! Small shared helpers: hex, key loading, time, statement-dir loading.

use camino::{Utf8Path, Utf8PathBuf};
use miette::{IntoDiagnostic, WrapErr, miette};

use ltk_sig::io::statement::Statement;

pub fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode_hex32(hex: &str) -> miette::Result<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return Err(miette!("expected 64 hex chars, got {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    for (i, pair) in bytes.chunks_exact(2).enumerate() {
        let hi = (pair[0] as char)
            .to_digit(16)
            .ok_or_else(|| miette!("invalid hex char"))?;
        let lo = (pair[1] as char)
            .to_digit(16)
            .ok_or_else(|| miette!("invalid hex char"))?;
        out[i] = (hi * 16 + lo) as u8;
    }
    Ok(out)
}

/// Load an Ed25519 signing key from a 32-byte seed file.
pub fn load_signing_key(path: &Utf8Path) -> miette::Result<ed25519_dalek::SigningKey> {
    let bytes = std::fs::read(path.as_std_path())
        .into_diagnostic()
        .wrap_err_with(|| format!("reading key file {path}"))?;
    let seed: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| miette!("key file {path} must be exactly 32 bytes"))?;
    Ok(ed25519_dalek::SigningKey::from_bytes(&seed))
}

/// Current Unix time in seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs()
}

/// Parse a `name=path` set specification.
pub fn parse_set_spec(spec: &str) -> miette::Result<(String, Utf8PathBuf)> {
    let (name, path) = spec
        .split_once('=')
        .ok_or_else(|| miette!("set spec '{spec}' must be name=path"))?;
    if name.is_empty() {
        return Err(miette!("set spec '{spec}' has an empty name"));
    }
    Ok((name.to_owned(), Utf8PathBuf::from(path)))
}

/// Load and parse every `.token` statement in a directory, sorted by file
/// name for deterministic output.
pub fn load_statements(dir: &Utf8Path) -> miette::Result<Vec<(Utf8PathBuf, Statement)>> {
    let mut paths: Vec<Utf8PathBuf> = Vec::new();
    for entry in std::fs::read_dir(dir.as_std_path())
        .into_diagnostic()
        .wrap_err_with(|| format!("reading statement directory {dir}"))?
    {
        let entry = entry.into_diagnostic()?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|p| miette!("non-UTF-8 path: {}", p.display()))?;
        if path.extension() == Some("token") {
            paths.push(path);
        }
    }
    paths.sort();

    let mut statements = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = std::fs::read(path.as_std_path())
            .into_diagnostic()
            .wrap_err_with(|| format!("reading {path}"))?;
        let statement = Statement::from_bytes(&bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("parsing statement {path}"))?;
        statements.push((path, statement));
    }
    Ok(statements)
}
