//! Per-platform verifier state and the attestation rule.
//!
//! A [`Platform`] is a trust root: one Ed25519 public key, an update URL,
//! a freshness floor, and an enforced lane policy — one entry of the baked
//! [`PlatformSet`]. [`PlatformTrust`] holds one platform's verifier state:
//! the platform entry plus the freshest accepted [`Root`]. It is
//! bootstrapped from the root baked into the manager at build time and
//! advanced by offering cached and freshly fetched candidates; stale
//! candidates are ignored, so feeding it `baked, cached, fetched` in any
//! order converges on the max.
//!
//! Root freshness is supersession by issue time (`iat`): newest issued wins,
//! floored by the platform's baked `min_issued_at` — an accepted root is
//! never older than the floor embedded in the app. Issue times are ratchet
//! values only: they are never compared to a local clock. Expiry (`exp`) is
//! the deliberate exception, as a direct-lane backstop (see
//! [`PlatformTrust::attest`]).
//!
//! **Attestation** binds platform authority to content: a statement is
//! attested when the platform directly endorses its hash
//! ([`Evidence::Direct`]) or when the hash is a member of a set covered by
//! the platform's current root — shown either by the whole set file
//! ([`Evidence::Membership`]) or by an inclusion proof against the same
//! digest ([`Evidence::Inclusion`]). New validation schemes are new
//! platform entries or new proof formats — the binding rule stays the same.

use thiserror::Error;

use crate::io::attestation::Attestation;
use crate::io::cose::TokenError;
use crate::io::root::Root;
use crate::io::statement::Statement;
use crate::io::statement_set::{InclusionProof, StatementSet, verify_inclusion};

pub use crate::io::platform_set::{AttestationPolicy, Platform, PlatformSet};

#[derive(Debug, Error, miette::Diagnostic, PartialEq, Eq, Clone)]
pub enum TrustError {
    #[error(transparent)]
    Token(#[from] TokenError),

    #[error("Conflicting root tokens at the same issue time")]
    Conflict,

    #[error("Platform does not accept roots")]
    RootsNotAccepted,

    #[error("Root is older than the platform's minimum issue time")]
    RootBelowFloor,
}

#[derive(Debug, Error, miette::Diagnostic, PartialEq, Eq, Clone)]
pub enum AttestError {
    #[error("Platform does not accept direct attestations")]
    AttestationsNotAccepted,

    #[error("Platform only accepts account-bound attestations")]
    AccountBindingRequired,

    #[error("Attestation signature does not verify under the platform key: {0}")]
    Signature(TokenError),

    #[error("Attestation endorses a different statement")]
    StatementMismatch,

    #[error("Attestation is expired")]
    Expired,

    #[error("Attestation is account-bound and no account identity is available")]
    AccountIdMissing,

    #[error("Attestation is bound to a different account")]
    AccountIdMismatch,

    #[error("Platform has no accepted root")]
    RootMissing,

    #[error("Current root has no set with the offered name")]
    RootUnknownName,

    #[error("Set file does not match the digest in the current root")]
    SetDigestMismatch,

    #[error("Statement is not vouched for by this platform")]
    NotVouched,
}

/// Evidence offered to bind a statement to platform authority. New proof
/// formats extend this enum.
#[derive(Debug, Clone, Copy)]
pub enum Evidence<'a> {
    /// A platform-signed attestation naming the statement's hash.
    Direct { attestation: &'a Attestation },
    /// The statement's hash is a member of the named set, whose file
    /// (fetched by content address) matches the digest in the current root.
    Membership {
        set_name: &'a str,
        set: &'a StatementSet,
    },
    /// The statement's hash is a member of the named set, shown by an
    /// inclusion proof against the digest in the current root — the compact
    /// form for verifiers that hold no set files (extracted at overlay
    /// build time, checked game-side).
    Inclusion {
        set_name: &'a str,
        proof: &'a InclusionProof,
    },
}

/// Verifier-side context for attestation checks.
#[derive(Debug, Clone, Copy)]
pub struct AttestContext<'a> {
    /// Current time as Unix epoch seconds, used ONLY against `exp` (the
    /// mandatory direct-lane backstop; days-granularity is sufficient).
    /// Never compared to `iat`.
    ///
    /// **Caller contract:** this must be a real, roughly-correct, non-rewound
    /// clock. `exp` is the only revocation an issued attestation has short
    /// of a platform-key rotation, so a caller that passes a rewound or zero
    /// `now` disables attestation expiry entirely. A machine playing online
    /// League has a good-enough clock (TLS to the game servers fails
    /// otherwise); the manager should use it, not a value an attacker can
    /// influence.
    pub now: u64,
    /// Identifier of the currently logged-in account, required by
    /// account-bound attestations.
    pub account_id: Option<&'a str>,
}

/// How a statement was attested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attested {
    /// Directly endorsed by a platform-signed attestation.
    Direct {
        /// Whether the attestation was bound to (and matched) the current
        /// account.
        account_bound: bool,
    },
    /// Endorsed by membership in a set covered by the platform's current
    /// root.
    Membership,
}

/// One trusted platform's verifier state: the baked platform entry plus the
/// freshest accepted root (absent for platforms that only issue direct
/// attestations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformTrust {
    platform: Platform,
    root: Option<Root>,
}

impl PlatformTrust {
    /// Build verifier state from a baked platform entry and its optional
    /// bootstrap root, verifying the root against the platform key and the
    /// platform's floor (`min_issued_at`) and lane policy. A bootstrap root
    /// that violates either is a build-time configuration error, reported
    /// loudly rather than ignored.
    pub fn new(platform: Platform, bootstrap_root: Option<&[u8]>) -> Result<Self, TrustError> {
        let root = match bootstrap_root {
            Some(bytes) => {
                if !platform.accepts_roots {
                    return Err(TrustError::RootsNotAccepted);
                }
                let root = Root::from_bytes(bytes)?;
                root.verify_signature(&platform.pub_key)?;
                if root.time_issued_at() < platform.min_issued_at {
                    return Err(TrustError::RootBelowFloor);
                }
                Some(root)
            }
            None => None,
        };
        Ok(Self { platform, root })
    }

    /// Offer a candidate root (cached or freshly fetched) to the ratchet.
    ///
    /// A root verified against the platform key with a **newer issue time**
    /// supersedes the current one; returns whether it did. Stale candidates
    /// (older issue times, or issue times below the platform's baked
    /// `min_issued_at` floor) are ignored, not errors, so callers can offer
    /// everything they have. Invalid signatures and distinct roots at an
    /// equal issue time are errors (honest platforms never mint two, by the
    /// `max(now, predecessor + 1)` rule).
    pub fn advance_root(&mut self, bytes: &[u8]) -> Result<bool, TrustError> {
        if !self.platform.accepts_roots {
            return Err(TrustError::RootsNotAccepted);
        }
        let candidate = Root::from_bytes(bytes)?;
        candidate.verify_signature(&self.platform.pub_key)?;
        if candidate.time_issued_at() < self.platform.min_issued_at {
            return Ok(false);
        }
        match &self.root {
            None => {
                self.root = Some(candidate);
                Ok(true)
            }
            Some(current) => match candidate.time_issued_at().cmp(&current.time_issued_at()) {
                std::cmp::Ordering::Greater => {
                    self.root = Some(candidate);
                    Ok(true)
                }
                std::cmp::Ordering::Equal => {
                    if candidate.as_bytes() != current.as_bytes() {
                        Err(TrustError::Conflict)
                    } else {
                        Ok(false)
                    }
                }
                std::cmp::Ordering::Less => Ok(false),
            },
        }
    }

    /// The baked platform entry this state trusts.
    pub fn platform(&self) -> &Platform {
        &self.platform
    }

    /// The freshest accepted root, if any.
    pub fn root(&self) -> Option<&Root> {
        self.root.as_ref()
    }

    /// Apply the attestation rule: bind a statement to this platform's
    /// authority with the offered evidence.
    ///
    /// - [`Evidence::Direct`]: the platform's lane policy must allow the
    ///   attestation (any, account-bound only, or none); it must verify
    ///   under the platform key, endorse exactly this statement's hash, be
    ///   unexpired, and its account binding (when present) must match the
    ///   current account.
    /// - [`Evidence::Membership`]: the current root must map the offered set
    ///   name to a digest, the offered set file must match that digest, and
    ///   the set must contain the statement's hash. No signature exists on a
    ///   statement to check — membership endorses the exact bytes.
    /// - [`Evidence::Inclusion`]: as membership, but shown by an audit path
    ///   connecting the statement's hash to the digest instead of by the
    ///   whole set file.
    ///
    /// Across platforms the rule is any-of: run this per [`PlatformTrust`]
    /// and accept if any returns an [`Attested`].
    pub fn attest(
        &self,
        statement: &Statement,
        evidence: Evidence,
        context: &AttestContext,
    ) -> Result<Attested, AttestError> {
        match evidence {
            Evidence::Direct { attestation } => {
                match self.platform.attestations {
                    AttestationPolicy::None => return Err(AttestError::AttestationsNotAccepted),
                    AttestationPolicy::AccountBound if attestation.account_binding().is_none() => {
                        return Err(AttestError::AccountBindingRequired);
                    }
                    _ => {}
                }
                attestation
                    .verify_signature(&self.platform.pub_key)
                    .map_err(AttestError::Signature)?;
                if *attestation.statement_hash() != statement.token_hash() {
                    return Err(AttestError::StatementMismatch);
                }
                if attestation.is_expired_at(context.now) {
                    return Err(AttestError::Expired);
                }
                let account_bound = match attestation.account_binding() {
                    Some(binding) => {
                        let account_id = context.account_id.ok_or(AttestError::AccountIdMissing)?;
                        if !binding.matches(account_id) {
                            return Err(AttestError::AccountIdMismatch);
                        }
                        true
                    }
                    None => false,
                };
                Ok(Attested::Direct { account_bound })
            }
            Evidence::Membership { set_name, set } => {
                let root = self.root.as_ref().ok_or(AttestError::RootMissing)?;
                let digest = root
                    .root_digest(set_name)
                    .ok_or(AttestError::RootUnknownName)?;
                if set.digest() != *digest {
                    return Err(AttestError::SetDigestMismatch);
                }
                if set.contains(&statement.token_hash()) {
                    Ok(Attested::Membership)
                } else {
                    Err(AttestError::NotVouched)
                }
            }
            Evidence::Inclusion { set_name, proof } => {
                let root = self.root.as_ref().ok_or(AttestError::RootMissing)?;
                let digest = root
                    .root_digest(set_name)
                    .ok_or(AttestError::RootUnknownName)?;
                if verify_inclusion(digest, &statement.token_hash(), proof) {
                    Ok(Attested::Membership)
                } else {
                    Err(AttestError::NotVouched)
                }
            }
        }
    }
}
