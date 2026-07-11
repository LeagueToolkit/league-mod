//! Shared token plumbing: the signed COSE_Sign1/CWT envelope and the bare
//! (unsigned) envelope.
//!
//! Signed tokens (`Attestation`, `Root`) are tagged `COSE_Sign1` (RFC 9052)
//! whose payload is a CWT claims set (RFC 8392):
//!
//! - Protected header: `alg` = Ed25519 (-19), `typ` (16, RFC 9596) = token
//!   type string for domain separation, `kid` (4) = the signing platform's
//!   raw Ed25519 public key (32 bytes; a routing hint, never a trust input).
//! - Payload: CWT claims map. Standard claims (`iat`, `exp`) are used where
//!   they fit; LTK-specific claims live in the CWT private-use range (labels
//!   below -65536), see [`claim`].
//! - Signature: Ed25519 over the COSE `Sig_structure`, which covers the
//!   protected header and payload. There is no hand-rolled hashing: the
//!   signature always covers the exact serialized bytes.
//!
//! Bare tokens (`Statement`) are a CBOR array `[typ (tstr), claims (bstr)]`
//! with no signature: a statement is pure content, trusted only when a
//! platform attests its hash, so a signature would confer nothing.
//!
//! Tokens of both envelopes are immutable once built: they keep their exact
//! wire bytes (`as_bytes`) and re-serialize byte-for-byte. Token identity is
//! `SHA-256(raw bytes)`; attestations and set files reference tokens by this
//! hash, which therefore endorses the exact bytes.
//!
//! To add a new token type: pick a new `typ` string, allocate private claim
//! labels in [`claim`], build a `ClaimsSet`, and call [`sign_token`] /
//! [`parse_token`] (signed) or [`seal_bare`] / [`parse_bare`] (bare).

use coset::{
    Algorithm, CborSerializable, CoseSign1, CoseSign1Builder, HeaderBuilder, Label,
    TaggedCborSerializable,
    cbor::value::Value,
    cwt::{ClaimName, ClaimsSet, Timestamp},
    iana,
};
use thiserror::Error;

/// `typ` protected-header label (RFC 9596).
pub const HEADER_LABEL_TYP: i64 = 16;

/// Maximum accepted size of a serialized token.
pub const MAX_TOKEN_LEN: usize = 16 * 1024 * 1024;

/// CWT private-use claim labels (RFC 8392 reserves values below -65536 for
/// private use). All LTK claims live in the -70000 block; never reuse a label
/// for a different meaning, even across token types — allocate the next free
/// one.
pub mod claim {
    /// bstr(n*56): packed file entries sorted by name hash, see `FileEntry`.
    pub const FILE_ENTRIES: i64 = -70001;
    /// tstr: optional name of the WAD a statement's files belong to
    /// (non-empty ASCII, < 128 bytes).
    pub const WAD_NAME: i64 = -70002;
    /// bstr(32): optional SHA-256 of the TOC of the WAD build a statement
    /// targets.
    pub const WAD_TOC_DIGEST: i64 = -70003;
    /// bstr(32): SHA-256 of the statement token an attestation endorses.
    pub const STATEMENT_HASH: i64 = -70004;
    /// bstr(8..=64): salt for the account binding of an attestation.
    pub const ACCOUNT_ID_SALT: i64 = -70005;
    /// bstr(32): SHA-256(salt || account_id) binding an attestation to one
    /// account.
    pub const ACCOUNT_ID_HASH: i64 = -70006;
    /// map: set name (tstr) -> digest (bstr(32)) of the platform's currently
    /// published sets, strictly ascending by name.
    pub const ROOTS: i64 = -70007;
    /// array of platform entry maps, strictly ascending by name; see
    /// `platform_set`.
    pub const PLATFORMS: i64 = -70008;
    /// array of bstr: serialized `Root` tokens carried by a bundle.
    pub const BUNDLE_ROOTS: i64 = -70009;
    /// array of bstr: serialized set files carried by a bundle.
    pub const BUNDLE_SETS: i64 = -70010;
    /// array of bstr: serialized `Statement` tokens carried by a bundle.
    pub const BUNDLE_STATEMENTS: i64 = -70011;
    // -70012 is reserved for direct attestations carried by a bundle.
    // -70013 is reserved for inclusion proofs carried by a bundle.
}

#[derive(Debug, Error, miette::Diagnostic, PartialEq, Eq, Clone)]
pub enum TokenError {
    #[error("Token is too large: {0} bytes")]
    TooLarge(usize),

    #[error("Failed to decode COSE/CBOR structure: {0}")]
    Cbor(String),

    #[error(
        "Token is not in canonical form (non-empty unprotected header or non-deterministic encoding)"
    )]
    NonCanonical,

    #[error("Token type mismatch: expected {expected:?}")]
    TypMismatch { expected: &'static str },

    #[error("Unsupported signature algorithm (expected Ed25519)")]
    AlgorithmUnsupported,

    #[error("Token payload is missing")]
    PayloadMissing,

    #[error("Signer key id is missing or not a 32-byte Ed25519 key")]
    SignerKeyMissing,

    #[error("Required claim is missing: {0}")]
    ClaimMissing(&'static str),

    #[error("Claim has invalid contents: {0}")]
    ClaimInvalid(&'static str),

    #[error("Signature verification failed")]
    SignatureInvalid,

    #[error("Public key is invalid")]
    PublicKeyInvalid,
}

/// A parsed and structurally validated (but not yet signature-verified)
/// signed token. Holds everything needed to re-serialize byte-for-byte and
/// to verify the signature; decoded claims are returned separately by
/// [`parse_token`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedToken {
    /// Exact serialized bytes of the tagged COSE_Sign1.
    pub raw: Vec<u8>,
    /// SHA-256 over `raw`; token identity.
    pub checksum: [u8; 32],
    /// Signer's Ed25519 public key from the protected `kid` header.
    pub signer: [u8; 32],
    /// COSE `Sig_structure` bytes the signature covers.
    pub tbs: Vec<u8>,
    /// Ed25519 signature bytes.
    pub signature: [u8; 64],
}

/// A parsed and structurally validated bare (unsigned) token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BareToken {
    /// Exact serialized bytes of the `[typ, claims]` array.
    pub raw: Vec<u8>,
    /// SHA-256 over `raw`; token identity (what attestations endorse).
    pub checksum: [u8; 32],
}

/// Build and sign a tagged COSE_Sign1 token over the given claims.
pub fn sign_token(
    typ: &'static str,
    claims: ClaimsSet,
    key: &ed25519_dalek::SigningKey,
) -> Result<Vec<u8>, TokenError> {
    use ed25519_dalek::Signer;
    let protected = HeaderBuilder::new()
        .algorithm(iana::Algorithm::Ed25519)
        .key_id(key.verifying_key().to_bytes().to_vec())
        .value(HEADER_LABEL_TYP, Value::Text(typ.to_owned()))
        .build();
    let payload = claims
        .to_vec()
        .map_err(|err| TokenError::Cbor(err.to_string()))?;
    let sign1 = CoseSign1Builder::new()
        .protected(protected)
        .payload(payload)
        .create_signature(b"", |data| key.sign(data).to_bytes().to_vec())
        .build();
    sign1
        .to_tagged_vec()
        .map_err(|err| TokenError::Cbor(err.to_string()))
}

/// Parse a tagged COSE_Sign1 token, checking `typ`, `alg`, `kid` and payload
/// shape. Does NOT verify the signature; call [`SealedToken::verify`].
pub fn parse_token(
    typ: &'static str,
    bytes: &[u8],
) -> Result<(SealedToken, ClaimsSet), TokenError> {
    if bytes.len() > MAX_TOKEN_LEN {
        return Err(TokenError::TooLarge(bytes.len()));
    }
    let sign1 =
        CoseSign1::from_tagged_slice(bytes).map_err(|err| TokenError::Cbor(err.to_string()))?;

    // Token identity is SHA-256(raw bytes) and drives the equivocation
    // detector and set membership — so identity must equal content. The
    // Ed25519 signature covers only the protected header and payload (the
    // COSE `Sig_structure`), leaving the outer CBOR framing and the
    // unprotected header malleable: an attacker could re-encode a
    // validly-signed token to a different hash. Close that by requiring
    // canonical form — an empty unprotected header and a byte-exact
    // deterministic round-trip — so every distinct byte string is a distinct
    // signed token, not a re-skin of one.
    if !sign1.unprotected.is_empty() {
        return Err(TokenError::NonCanonical);
    }
    let canonical = sign1
        .clone()
        .to_tagged_vec()
        .map_err(|err| TokenError::Cbor(err.to_string()))?;
    if canonical != bytes {
        return Err(TokenError::NonCanonical);
    }

    if sign1.protected.header.alg != Some(Algorithm::Assigned(iana::Algorithm::Ed25519)) {
        return Err(TokenError::AlgorithmUnsupported);
    }

    let typ_ok = sign1.protected.header.rest.iter().any(|(label, value)| {
        *label == Label::Int(HEADER_LABEL_TYP) && value.as_text() == Some(typ)
    });
    if !typ_ok {
        return Err(TokenError::TypMismatch { expected: typ });
    }

    let signer: [u8; 32] = sign1
        .protected
        .header
        .key_id
        .as_slice()
        .try_into()
        .map_err(|_| TokenError::SignerKeyMissing)?;

    let payload = sign1.payload.as_deref().ok_or(TokenError::PayloadMissing)?;
    let claims = ClaimsSet::from_slice(payload).map_err(|err| TokenError::Cbor(err.to_string()))?;

    let signature: [u8; 64] = sign1
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| TokenError::SignatureInvalid)?;
    let tbs = sign1.tbs_data(b"");

    use sha2::Digest;
    let checksum = sha2::Sha256::digest(bytes).into();

    Ok((
        SealedToken {
            raw: bytes.to_vec(),
            checksum,
            signer,
            tbs,
            signature,
        },
        claims,
    ))
}

/// Serialize a bare (unsigned) token: the CBOR array `[typ, claims]`.
pub fn seal_bare(typ: &'static str, claims: ClaimsSet) -> Result<Vec<u8>, TokenError> {
    let payload = claims
        .to_vec()
        .map_err(|err| TokenError::Cbor(err.to_string()))?;
    let value = Value::Array(vec![Value::Text(typ.to_owned()), Value::Bytes(payload)]);
    let mut raw = Vec::new();
    coset::cbor::ser::into_writer(&value, &mut raw)
        .map_err(|err| TokenError::Cbor(err.to_string()))?;
    Ok(raw)
}

/// Parse a bare token, checking `typ` and payload shape. Bare tokens have no
/// signature: identity is the hash, and trust only ever comes from a
/// platform attesting that hash.
pub fn parse_bare(typ: &'static str, bytes: &[u8]) -> Result<(BareToken, ClaimsSet), TokenError> {
    if bytes.len() > MAX_TOKEN_LEN {
        return Err(TokenError::TooLarge(bytes.len()));
    }
    let value: Value =
        coset::cbor::de::from_reader(bytes).map_err(|err| TokenError::Cbor(err.to_string()))?;

    // Same canonical-form rule as signed tokens: identity must equal
    // content, so a re-encoding (or trailing garbage) must not parse.
    let mut canonical = Vec::new();
    coset::cbor::ser::into_writer(&value, &mut canonical)
        .map_err(|err| TokenError::Cbor(err.to_string()))?;
    if canonical != bytes {
        return Err(TokenError::NonCanonical);
    }

    let items = value
        .as_array()
        .ok_or_else(|| TokenError::Cbor("bare token must be a CBOR array".to_owned()))?;
    let [typ_value, payload_value] = items.as_slice() else {
        return Err(TokenError::Cbor(
            "bare token must be a two-item array".to_owned(),
        ));
    };
    if typ_value.as_text() != Some(typ) {
        return Err(TokenError::TypMismatch { expected: typ });
    }
    let payload = payload_value.as_bytes().ok_or(TokenError::PayloadMissing)?;
    let claims = ClaimsSet::from_slice(payload).map_err(|err| TokenError::Cbor(err.to_string()))?;

    use sha2::Digest;
    let checksum = sha2::Sha256::digest(bytes).into();

    Ok((
        BareToken {
            raw: bytes.to_vec(),
            checksum,
        },
        claims,
    ))
}

impl SealedToken {
    /// Verify the Ed25519 signature against the given public key.
    pub fn verify(&self, pub_key: &[u8; 32]) -> Result<(), TokenError> {
        let key = ed25519_dalek::VerifyingKey::from_bytes(pub_key)
            .map_err(|_| TokenError::PublicKeyInvalid)?;
        let signature = ed25519_dalek::Signature::from_bytes(&self.signature);
        key.verify_strict(&self.tbs, &signature)
            .map_err(|_| TokenError::SignatureInvalid)
    }
}

/// Look up a private-use claim by label.
pub fn private_claim(claims: &ClaimsSet, id: i64) -> Option<&Value> {
    claims
        .rest
        .iter()
        .find(|(name, _)| *name == ClaimName::PrivateUse(id))
        .map(|(_, value)| value)
}

/// Decode the `iat` claim as Unix epoch seconds. For roots this is the
/// supersession value (newest issued wins); verifiers only compare it to the
/// best already seen, never to a local clock.
pub fn issued_at_secs(claims: &ClaimsSet) -> Result<u64, TokenError> {
    timestamp_secs(
        claims
            .issued_at
            .as_ref()
            .ok_or(TokenError::ClaimMissing("iat"))?,
    )
}

/// Decode the optional `exp` claim as Unix epoch seconds. Expiry exists only
/// as a direct-lane backstop; durable validity never comes from clocks.
pub fn expires_at_secs(claims: &ClaimsSet) -> Result<Option<u64>, TokenError> {
    claims
        .expiration_time
        .as_ref()
        .map(timestamp_secs)
        .transpose()
}

/// Validate a short name (WAD names, set names): non-empty ASCII shorter
/// than 128 bytes.
pub fn validate_name(name: &str, claim: &'static str) -> Result<(), TokenError> {
    if name.is_empty() || name.len() >= 128 || !name.is_ascii() {
        return Err(TokenError::ClaimInvalid(claim));
    }
    Ok(())
}

/// Convert a CWT timestamp to non-negative whole seconds.
pub fn timestamp_secs(ts: &Timestamp) -> Result<u64, TokenError> {
    match ts {
        Timestamp::WholeSeconds(s) if *s >= 0 => Ok(*s as u64),
        _ => Err(TokenError::ClaimInvalid("timestamp")),
    }
}

/// Convert whole seconds to a CWT timestamp.
pub fn timestamp_from_secs(secs: u64) -> Result<Timestamp, TokenError> {
    i64::try_from(secs)
        .map(Timestamp::WholeSeconds)
        .map_err(|_| TokenError::ClaimInvalid("timestamp"))
}

/// Decode a claim value holding a fixed-size byte string.
pub fn claim_bstr_exact<const N: usize>(
    value: &Value,
    name: &'static str,
) -> Result<[u8; N], TokenError> {
    value
        .as_bytes()
        .and_then(|bytes| <[u8; N]>::try_from(bytes.as_slice()).ok())
        .ok_or(TokenError::ClaimInvalid(name))
}
