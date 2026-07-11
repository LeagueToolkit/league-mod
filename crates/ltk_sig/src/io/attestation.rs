use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};

use super::cose::{self, TokenError, claim};

/// Salted binding of an attestation to one account:
/// `hash = SHA-256(salt || account_id)`. The salt keeps attestations from
/// doubling as a rainbow table of account identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBinding {
    salt: Vec<u8>,
    hash: [u8; 32],
}

impl AccountBinding {
    pub const SALT_MIN: usize = 8;
    pub const SALT_MAX: usize = 64;

    /// Bind an account identifier with the given salt.
    pub fn new(salt: Vec<u8>, account_id: &str) -> Result<Self, TokenError> {
        let hash = Self::compute(&salt, account_id)?;
        Ok(Self { salt, hash })
    }

    /// Reassemble a binding from its wire claims.
    pub fn from_parts(salt: Vec<u8>, hash: [u8; 32]) -> Result<Self, TokenError> {
        validate_salt(&salt)?;
        Ok(Self { salt, hash })
    }

    /// Check whether this binding matches an account identifier.
    pub fn matches(&self, account_id: &str) -> bool {
        Self::compute(&self.salt, account_id).is_ok_and(|hash| hash == self.hash)
    }

    pub fn salt(&self) -> &[u8] {
        &self.salt
    }

    pub fn hash(&self) -> &[u8; 32] {
        &self.hash
    }

    fn compute(salt: &[u8], account_id: &str) -> Result<[u8; 32], TokenError> {
        validate_salt(salt)?;
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(salt);
        hasher.update(account_id.as_bytes());
        Ok(hasher.finalize().into())
    }
}

fn validate_salt(salt: &[u8]) -> Result<(), TokenError> {
    if salt.len() < AccountBinding::SALT_MIN || salt.len() > AccountBinding::SALT_MAX {
        return Err(TokenError::ClaimInvalid("account_id_salt"));
    }
    Ok(())
}

/// Inputs for signing a new [`Attestation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttestationParams {
    /// Issue time as Unix epoch seconds (informational on attestations).
    pub time_issued_at: u64,
    /// Expiry as Unix epoch seconds, strictly after the issue time.
    /// Mandatory: a signed attestation cannot be revoked short of rotating
    /// the platform key, so an attestation without an expiry would be
    /// irrevocable. The platform's control point is declining to re-issue.
    pub time_expires_at: u64,
    /// SHA-256 of the statement token being endorsed.
    pub statement_hash: [u8; 32],
    /// Optional binding to one account; the attested mod then only works on
    /// that account.
    pub account_binding: Option<AccountBinding>,
}

/// The platform's direct endorsement of one statement: "I validated the
/// exact statement whose hash this is."
///
/// COSE_Sign1 with `typ = "ltk/attestation"`, signed by the platform key
/// (protected `kid` header is a routing hint; trust comes from verifying
/// against a baked platform key in `trust::PlatformTrust::attest`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attestation {
    token: cose::SealedToken,
    time_issued_at: u64,
    time_expires_at: u64,
    statement_hash: [u8; 32],
    account_binding: Option<AccountBinding>,
}

impl Attestation {
    pub const TYP: &'static str = "ltk/attestation";

    /// Build and sign a new attestation with the platform key.
    pub fn sign(
        params: &AttestationParams,
        platform_key: &ed25519_dalek::SigningKey,
    ) -> Result<Self, TokenError> {
        if params.time_expires_at <= params.time_issued_at {
            return Err(TokenError::ClaimInvalid("exp not after iat"));
        }
        let mut builder = ClaimsSetBuilder::new()
            .issued_at(cose::timestamp_from_secs(params.time_issued_at)?)
            .expiration_time(cose::timestamp_from_secs(params.time_expires_at)?)
            .private_claim(
                claim::STATEMENT_HASH,
                Value::Bytes(params.statement_hash.to_vec()),
            );
        if let Some(binding) = &params.account_binding {
            builder = builder
                .private_claim(
                    claim::ACCOUNT_ID_SALT,
                    Value::Bytes(binding.salt().to_vec()),
                )
                .private_claim(
                    claim::ACCOUNT_ID_HASH,
                    Value::Bytes(binding.hash().to_vec()),
                );
        }
        let raw = cose::sign_token(Self::TYP, builder.build(), platform_key)?;
        Self::from_bytes(&raw)
    }

    /// Parse and structurally validate an attestation.
    /// Does NOT verify the signature; call [`Self::verify_signature`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TokenError> {
        let (token, claims) = cose::parse_token(Self::TYP, bytes)?;
        let time_issued_at = cose::issued_at_secs(&claims)?;
        let time_expires_at =
            cose::expires_at_secs(&claims)?.ok_or(TokenError::ClaimMissing("exp"))?;
        if time_expires_at <= time_issued_at {
            return Err(TokenError::ClaimInvalid("exp not after iat"));
        }
        let statement_hash = cose::private_claim(&claims, claim::STATEMENT_HASH)
            .ok_or(TokenError::ClaimMissing("statement_hash"))
            .and_then(|value| cose::claim_bstr_exact::<32>(value, "statement_hash"))?;

        let salt = cose::private_claim(&claims, claim::ACCOUNT_ID_SALT)
            .map(|value| {
                value
                    .as_bytes()
                    .cloned()
                    .ok_or(TokenError::ClaimInvalid("account_id_salt"))
            })
            .transpose()?;
        let hash = cose::private_claim(&claims, claim::ACCOUNT_ID_HASH)
            .map(|value| cose::claim_bstr_exact::<32>(value, "account_id_hash"))
            .transpose()?;
        let account_binding = match (salt, hash) {
            (Some(salt), Some(hash)) => Some(AccountBinding::from_parts(salt, hash)?),
            (None, None) => None,
            _ => return Err(TokenError::ClaimInvalid("account binding incomplete")),
        };

        Ok(Self {
            token,
            time_issued_at,
            time_expires_at,
            statement_hash,
            account_binding,
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

    /// Issue time as Unix epoch seconds (informational).
    pub fn time_issued_at(&self) -> u64 {
        self.time_issued_at
    }

    /// Expiry as Unix epoch seconds.
    pub fn time_expires_at(&self) -> u64 {
        self.time_expires_at
    }

    /// SHA-256 of the statement token this attestation endorses.
    pub fn statement_hash(&self) -> &[u8; 32] {
        &self.statement_hash
    }

    /// Account binding, if the attestation is bound to one account.
    pub fn account_binding(&self) -> Option<&AccountBinding> {
        self.account_binding.as_ref()
    }

    /// Whether the attestation is expired at the given time. Expiry is the
    /// only revocation an issued attestation has short of rotating the
    /// platform key.
    pub fn is_expired_at(&self, now: u64) -> bool {
        now >= self.time_expires_at
    }

    /// Verify the Ed25519 signature against the given platform key.
    pub fn verify_signature(&self, platform_pub_key: &[u8; 32]) -> Result<(), TokenError> {
        self.token.verify(platform_pub_key)
    }
}
