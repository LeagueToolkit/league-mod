use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};

use super::cose::{self, TokenError, claim};

/// Inputs for signing a new [`Root`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootParams<'a> {
    /// Issue time as Unix epoch seconds: the supersession value. Derive it
    /// as `max(now, predecessor iat + 1)` so a clock step can neither mint
    /// two distinct roots at the same value nor regress supersession.
    pub time_issued_at: u64,
    /// Set name -> digest of that set, strictly ascending by name. The
    /// digest is the set's Merkle tree hash (see `statement_set`), so it
    /// anchors both whole-set membership and inclusion proofs.
    pub roots: &'a [(String, [u8; 32])],
}

/// The platform's re-signed snapshot: "as of now, these are the digests of
/// my published sets."
///
/// COSE_Sign1 with `typ = "ltk/root"`, signed by the platform key. Roots
/// supersede each other by issue time (`iat`), floored by the bootstrap
/// root baked into the manager at build time. Set names are opaque
/// partition labels (per-WAD lists, a corpus, a Merkle tree) — routing,
/// never trust inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    token: cose::SealedToken,
    time_issued_at: u64,
    roots: Vec<(String, [u8; 32])>,
}

impl Root {
    pub const TYP: &'static str = "ltk/root";

    /// Build and sign a new root with the platform key.
    pub fn sign(
        params: &RootParams,
        platform_key: &ed25519_dalek::SigningKey,
    ) -> Result<Self, TokenError> {
        for (name, _) in params.roots {
            cose::validate_name(name, "roots")?;
        }
        if !params.roots.windows(2).all(|w| w[0].0 < w[1].0) {
            return Err(TokenError::ClaimInvalid("roots must be sorted"));
        }
        let roots = Value::Map(
            params
                .roots
                .iter()
                .map(|(name, digest)| (Value::Text(name.clone()), Value::Bytes(digest.to_vec())))
                .collect(),
        );
        let claims = ClaimsSetBuilder::new()
            .issued_at(cose::timestamp_from_secs(params.time_issued_at)?)
            .private_claim(claim::ROOTS, roots)
            .build();
        let raw = cose::sign_token(Self::TYP, claims, platform_key)?;
        Self::from_bytes(&raw)
    }

    /// Parse and structurally validate a root.
    /// Does NOT verify the signature; call [`Self::verify_signature`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenError> {
        let (token, claims) = cose::parse_token(Self::TYP, bytes)?;
        let time_issued_at = cose::issued_at_secs(&claims)?;
        let map = cose::private_claim(&claims, claim::ROOTS)
            .and_then(|value| value.as_map())
            .ok_or(TokenError::ClaimMissing("roots"))?;
        let mut roots = Vec::with_capacity(map.len());
        for (key, value) in map {
            let name = key.as_text().ok_or(TokenError::ClaimInvalid("roots"))?;
            cose::validate_name(name, "roots")?;
            let digest = cose::claim_bstr_exact::<32>(value, "roots")?;
            roots.push((name.to_owned(), digest));
        }
        if !roots.windows(2).all(|w| w[0].0 < w[1].0) {
            return Err(TokenError::ClaimInvalid("roots must be sorted"));
        }
        Ok(Self {
            token,
            time_issued_at,
            roots,
        })
    }

    /// Exact serialized bytes of the token (what the signature covers).
    pub fn as_bytes(&self) -> &[u8] {
        &self.token.raw
    }

    /// SHA-256 of the serialized token.
    pub fn token_hash(&self) -> [u8; 32] {
        self.token.checksum
    }

    /// Ed25519 public key of the signer (protected `kid` header). A routing
    /// hint only; trust comes from verifying against a baked platform key.
    pub fn signer_pub_key(&self) -> &[u8; 32] {
        &self.token.signer
    }

    /// Issue time as Unix epoch seconds: the supersession value. Compared
    /// only against other roots from the same platform, never against a
    /// local clock.
    pub fn time_issued_at(&self) -> u64 {
        self.time_issued_at
    }

    /// Set name -> set digest, sorted by name.
    pub fn roots(&self) -> &[(String, [u8; 32])] {
        &self.roots
    }

    /// Digest of the named set as of this root, if the platform publishes
    /// it. Names match exactly; they are opaque labels.
    pub fn root_digest(&self, name: &str) -> Option<&[u8; 32]> {
        self.roots
            .binary_search_by(|(n, _)| n.as_str().cmp(name))
            .ok()
            .map(|i| &self.roots[i].1)
    }

    /// Verify the Ed25519 signature against the given platform key.
    pub fn verify_signature(&self, platform_pub_key: &[u8; 32]) -> Result<(), TokenError> {
        self.token.verify(platform_pub_key)
    }
}
