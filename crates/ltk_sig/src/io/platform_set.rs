//! The platform set: the verifier's baked trust roots.
//!
//! A platform set is authored as a JSON template, converted to CBOR by a
//! build script ([`PlatformSet::from_json`], behind the `json` feature), and
//! baked into the manager with `include_bytes!`. On the wire it is a bare
//! token (`ltk/platform-set`), which gives it the same canonical-encoding
//! and hash-identity properties as every other token — and the same
//! structure, signed, becomes the future platform registry token if
//! onboarding ever needs to outpace releases.
//!
//! Each entry carries an **enforced lane policy**: which authority-bearing
//! tokens the verifier honors from that platform's key (roots, direct
//! attestations, account-bound attestations only). Statements need no such
//! gating — they are unsigned content and confer nothing.

use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};

use super::cose::{self, TokenError, claim};

/// Entry map keys inside the `platforms` claim. Unknown keys are ignored
/// (forward compatibility); never reuse a key for a different meaning.
mod entry_key {
    pub const NAME: i128 = 1;
    pub const PUB_KEY: i128 = 2;
    pub const URL: i128 = 3;
    pub const MIN_ISSUED_AT: i128 = 4;
    pub const ACCEPTS_ROOTS: i128 = 5;
    pub const ATTESTATIONS: i128 = 6;
}

/// Which direct attestations a platform's key may vouch with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttestationPolicy {
    /// The platform never issues direct attestations; any offered one is
    /// refused (e.g. a corpus platform that only re-signs roots).
    None,
    /// Only account-bound attestations are accepted (the developer lane): a
    /// stolen key can mint nothing that works beyond a single account.
    AccountBound,
    /// Any direct attestation is accepted (e.g. an upload-validation lane).
    Any,
}

impl AttestationPolicy {
    /// Wire representation of the policy.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AccountBound => "account-bound",
            Self::Any => "any",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "none" => Some(Self::None),
            "account-bound" => Some(Self::AccountBound),
            "any" => Some(Self::Any),
            _ => None,
        }
    }
}

/// One baked platform entry: a trust root with an enforced lane policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Platform {
    /// UI attribution and log routing; never a trust input. Also the sort
    /// and lookup key within the set (non-empty ASCII, < 128 bytes).
    pub name: String,
    /// The platform's Ed25519 public key — the only key the platform has.
    pub pub_key: [u8; 32],
    /// URL prefix for fetching the current root and set files (non-empty,
    /// <= 255 bytes).
    pub url: String,
    /// Freshness floor: roots issued before this time are never accepted.
    /// Refreshed at each manager release.
    pub min_issued_at: u64,
    /// Whether roots — and therefore the membership lane — are accepted.
    pub accepts_roots: bool,
    /// Which direct attestations are accepted.
    pub attestations: AttestationPolicy,
}

impl Platform {
    fn validate(&self) -> Result<(), TokenError> {
        cose::validate_name(&self.name, "platforms")?;
        if self.url.is_empty() || self.url.len() > 255 {
            return Err(TokenError::ClaimInvalid("platforms: url"));
        }
        Ok(())
    }
}

/// The baked platform set: every trust decision starts here.
///
/// Bare token with `typ = "ltk/platform-set"`; the `platforms` claim is an
/// array of entry maps, strictly ascending by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformSet {
    token: cose::BareToken,
    platforms: Vec<Platform>,
}

impl PlatformSet {
    pub const TYP: &'static str = "ltk/platform-set";

    /// Build and seal a platform set from entries strictly sorted by name.
    pub fn seal(platforms: &[Platform]) -> Result<Self, TokenError> {
        for platform in platforms {
            platform.validate()?;
        }
        if !platforms.windows(2).all(|w| w[0].name < w[1].name) {
            return Err(TokenError::ClaimInvalid("platforms must be sorted"));
        }
        let entries = Value::Array(platforms.iter().map(encode_entry).collect());
        let claims = ClaimsSetBuilder::new()
            .private_claim(claim::PLATFORMS, entries)
            .build();
        let raw = cose::seal_bare(Self::TYP, claims)?;
        Self::from_bytes(&raw)
    }

    /// Parse and structurally validate a platform set.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenError> {
        let (token, claims) = cose::parse_bare(Self::TYP, bytes)?;
        let entries = cose::private_claim(&claims, claim::PLATFORMS)
            .and_then(|value| value.as_array())
            .ok_or(TokenError::ClaimMissing("platforms"))?;
        let platforms: Vec<Platform> =
            entries.iter().map(decode_entry).collect::<Result<_, _>>()?;
        for platform in &platforms {
            platform.validate()?;
        }
        if !platforms.windows(2).all(|w| w[0].name < w[1].name) {
            return Err(TokenError::ClaimInvalid("platforms must be sorted"));
        }
        Ok(Self { token, platforms })
    }

    /// Exact serialized bytes of the token (what the identity hash covers).
    pub fn as_bytes(&self) -> &[u8] {
        &self.token.raw
    }

    /// SHA-256 of the serialized token.
    pub fn token_hash(&self) -> [u8; 32] {
        self.token.checksum
    }

    /// Platform entries, sorted by name.
    pub fn platforms(&self) -> &[Platform] {
        &self.platforms
    }

    /// Find a platform entry by name.
    pub fn get(&self, name: &str) -> Option<&Platform> {
        self.platforms
            .binary_search_by(|p| p.name.as_str().cmp(name))
            .ok()
            .map(|i| &self.platforms[i])
    }
}

fn encode_entry(platform: &Platform) -> Value {
    Value::Map(vec![
        (
            Value::Integer(entry_key::NAME.try_into().unwrap()),
            Value::Text(platform.name.clone()),
        ),
        (
            Value::Integer(entry_key::PUB_KEY.try_into().unwrap()),
            Value::Bytes(platform.pub_key.to_vec()),
        ),
        (
            Value::Integer(entry_key::URL.try_into().unwrap()),
            Value::Text(platform.url.clone()),
        ),
        (
            Value::Integer(entry_key::MIN_ISSUED_AT.try_into().unwrap()),
            Value::from(platform.min_issued_at),
        ),
        (
            Value::Integer(entry_key::ACCEPTS_ROOTS.try_into().unwrap()),
            Value::Bool(platform.accepts_roots),
        ),
        (
            Value::Integer(entry_key::ATTESTATIONS.try_into().unwrap()),
            Value::Text(platform.attestations.as_str().to_owned()),
        ),
    ])
}

fn decode_entry(value: &Value) -> Result<Platform, TokenError> {
    let map = value
        .as_map()
        .ok_or(TokenError::ClaimInvalid("platforms"))?;
    let field = |key: i128| {
        map.iter()
            .find(|(k, _)| k.as_integer().is_some_and(|i| i128::from(i) == key))
            .map(|(_, v)| v)
    };

    let name = field(entry_key::NAME)
        .and_then(|v| v.as_text())
        .ok_or(TokenError::ClaimInvalid("platforms: name"))?
        .to_owned();
    let pub_key = field(entry_key::PUB_KEY)
        .ok_or(TokenError::ClaimInvalid("platforms: pub_key"))
        .and_then(|v| cose::claim_bstr_exact::<32>(v, "platforms: pub_key"))?;
    let url = field(entry_key::URL)
        .and_then(|v| v.as_text())
        .ok_or(TokenError::ClaimInvalid("platforms: url"))?
        .to_owned();
    let min_issued_at = field(entry_key::MIN_ISSUED_AT)
        .and_then(|v| v.as_integer())
        .and_then(|i| u64::try_from(i).ok())
        .ok_or(TokenError::ClaimInvalid("platforms: min_issued_at"))?;
    let accepts_roots = field(entry_key::ACCEPTS_ROOTS)
        .and_then(|v| v.as_bool())
        .ok_or(TokenError::ClaimInvalid("platforms: accepts_roots"))?;
    let attestations = field(entry_key::ATTESTATIONS)
        .and_then(|v| v.as_text())
        .and_then(AttestationPolicy::from_str)
        .ok_or(TokenError::ClaimInvalid("platforms: attestations"))?;

    Ok(Platform {
        name,
        pub_key,
        url,
        min_issued_at,
        accepts_roots,
        attestations,
    })
}

#[cfg(feature = "json")]
pub use template::TemplateError;

/// JSON template support for build scripts (feature `json`).
#[cfg(feature = "json")]
mod template {
    use thiserror::Error;

    use super::super::cose::TokenError;
    use super::{AttestationPolicy, Platform, PlatformSet};

    #[derive(Debug, Error, miette::Diagnostic, PartialEq, Eq, Clone)]
    pub enum TemplateError {
        #[error("Failed to parse platform template JSON: {0}")]
        Json(String),

        #[error("Platform template is invalid: {0}")]
        Invalid(&'static str),

        #[error(transparent)]
        Token(#[from] TokenError),
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TemplateFile {
        platforms: Vec<TemplateEntry>,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TemplateEntry {
        name: String,
        /// Ed25519 public key as 64 hex characters.
        pub_key: String,
        url: String,
        min_issued_at: u64,
        accepts: TemplateAccepts,
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TemplateAccepts {
        roots: bool,
        attestations: TemplateAttestations,
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "kebab-case")]
    enum TemplateAttestations {
        None,
        AccountBound,
        Any,
    }

    impl PlatformSet {
        /// Parse a JSON template and seal it as the baked platform set.
        /// Entries are sorted by name; everything else is validated by
        /// [`PlatformSet::seal`]. Intended for build scripts:
        ///
        /// ```json
        /// {
        ///   "platforms": [
        ///     {
        ///       "name": "corpus",
        ///       "pub_key": "<64 hex chars>",
        ///       "url": "https://example.com/ltk",
        ///       "min_issued_at": 1767225600,
        ///       "accepts": { "roots": true, "attestations": "none" }
        ///     }
        ///   ]
        /// }
        /// ```
        pub fn from_json(json: &str) -> Result<Self, TemplateError> {
            let file: TemplateFile =
                serde_json::from_str(json).map_err(|err| TemplateError::Json(err.to_string()))?;
            let mut platforms = file
                .platforms
                .into_iter()
                .map(|entry| {
                    Ok(Platform {
                        name: entry.name,
                        pub_key: decode_hex_key(&entry.pub_key)
                            .ok_or(TemplateError::Invalid("pub_key must be 64 hex chars"))?,
                        url: entry.url,
                        min_issued_at: entry.min_issued_at,
                        accepts_roots: entry.accepts.roots,
                        attestations: match entry.accepts.attestations {
                            TemplateAttestations::None => AttestationPolicy::None,
                            TemplateAttestations::AccountBound => AttestationPolicy::AccountBound,
                            TemplateAttestations::Any => AttestationPolicy::Any,
                        },
                    })
                })
                .collect::<Result<Vec<_>, TemplateError>>()?;
            platforms.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(Self::seal(&platforms)?)
        }
    }

    fn decode_hex_key(hex: &str) -> Option<[u8; 32]> {
        let bytes = hex.as_bytes();
        if bytes.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, pair) in bytes.chunks_exact(2).enumerate() {
            let hi = (pair[0] as char).to_digit(16)?;
            let lo = (pair[1] as char).to_digit(16)?;
            out[i] = (hi * 16 + lo) as u8;
        }
        Some(out)
    }
}
