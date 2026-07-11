use camino::Utf8Path;

use crate::util::{encode_hex, load_signing_key};

pub fn run(key: &Utf8Path) -> miette::Result<()> {
    let key = load_signing_key(key)?;
    println!("{}", encode_hex(&key.verifying_key().to_bytes()));
    Ok(())
}
