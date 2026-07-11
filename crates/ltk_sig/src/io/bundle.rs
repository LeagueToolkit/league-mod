//! The bundle: one serialized blob carrying every token a verifier needs —
//! the CMS / PEM-chain analogue for this model.
//!
//! A bundle is a dumb container: arrays of raw serialized roots, set files,
//! and statements, moved between processes as a single file (out-of-band
//! distribution today; the overlay build emitting only the statements it
//! used tomorrow). It is a bare token and carries **no authority**: a
//! verifier authenticates each item internally — roots against baked
//! platform keys, sets against the accepted root's digests, statements by
//! set membership (`verify::VerifyContext::from_bundle`). Editing a bundle
//! on disk can therefore only remove trust, never add it.

use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};

use super::cose::{self, TokenError, claim};

/// Inputs for sealing a new [`Bundle`]. Items are raw serialized bytes;
/// the container does not validate them — verifiers judge each item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleParams<'a> {
    /// Serialized `Root` tokens.
    pub roots: &'a [Vec<u8>],
    /// Serialized set files.
    pub sets: &'a [Vec<u8>],
    /// Serialized `Statement` tokens.
    pub statements: &'a [Vec<u8>],
}

/// The token container: roots, set files, and statements as one blob.
///
/// Bare token with `typ = "ltk/bundle"`. Empty sections are omitted from
/// the encoding; unknown sections from newer producers are ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    token: cose::BareToken,
    roots: Vec<Vec<u8>>,
    sets: Vec<Vec<u8>>,
    statements: Vec<Vec<u8>>,
}

impl Bundle {
    pub const TYP: &'static str = "ltk/bundle";

    /// Build and seal a new bundle.
    pub fn seal(params: &BundleParams) -> Result<Self, TokenError> {
        let mut builder = ClaimsSetBuilder::new();
        for (label, items) in [
            (claim::BUNDLE_ROOTS, params.roots),
            (claim::BUNDLE_SETS, params.sets),
            (claim::BUNDLE_STATEMENTS, params.statements),
        ] {
            if !items.is_empty() {
                let array = items.iter().map(|b| Value::Bytes(b.clone())).collect();
                builder = builder.private_claim(label, Value::Array(array));
            }
        }
        let raw = cose::seal_bare(Self::TYP, builder.build())?;
        Self::from_bytes(&raw)
    }

    /// Parse and structurally validate a bundle (shape only; the items
    /// inside are untrusted and judged individually by the verifier).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenError> {
        let (token, claims) = cose::parse_bare(Self::TYP, bytes)?;
        let items = |label: i64, name: &'static str| -> Result<Vec<Vec<u8>>, TokenError> {
            match cose::private_claim(&claims, label) {
                None => Ok(Vec::new()),
                Some(value) => value
                    .as_array()
                    .ok_or(TokenError::ClaimInvalid(name))?
                    .iter()
                    .map(|item| {
                        item.as_bytes()
                            .cloned()
                            .ok_or(TokenError::ClaimInvalid(name))
                    })
                    .collect(),
            }
        };
        Ok(Self {
            roots: items(claim::BUNDLE_ROOTS, "bundle_roots")?,
            sets: items(claim::BUNDLE_SETS, "bundle_sets")?,
            statements: items(claim::BUNDLE_STATEMENTS, "bundle_statements")?,
            token,
        })
    }

    /// Exact serialized bytes of the token.
    pub fn as_bytes(&self) -> &[u8] {
        &self.token.raw
    }

    /// SHA-256 of the serialized token (identification only — a bundle
    /// carries no authority).
    pub fn token_hash(&self) -> [u8; 32] {
        self.token.checksum
    }

    /// Serialized `Root` tokens, as carried.
    pub fn roots(&self) -> &[Vec<u8>] {
        &self.roots
    }

    /// Serialized set files, as carried.
    pub fn sets(&self) -> &[Vec<u8>] {
        &self.sets
    }

    /// Serialized `Statement` tokens, as carried.
    pub fn statements(&self) -> &[Vec<u8>] {
        &self.statements
    }
}
