//! Verifier-side verification context: attest statements once up front,
//! answer per-file queries from a flat lookup table.
//!
//! [`VerifyContext`] bridges the trust layer and file-level verification of
//! a merged overlay: the overlay build records every statement that shipped
//! content into the overlay together with its evidence; the verifier replays
//! them into this context, then asks per referenced file whether some
//! attested statement vouches for the bytes it observed.
//!
//! The approach is deliberate: **attest everything up front, then query a
//! LUT**. Each offered statement runs the attestation rule once
//! ([`PlatformTrust::attest`], any-of across all platforms); the file
//! entries of every attested statement are flattened into a multimap keyed
//! by file name hash. Queries ([`VerifyContext::attested`]) are then plain
//! hash probes — no signature or membership work is ever repeated per file.
//! Cost scales linearly with statement count: the up-front pass is dominated
//! by at most one Ed25519 verification per direct-attested statement (the
//! `kid` routing hint puts the matching platform key first) and one digest
//! comparison plus binary search per membership-attested one, so hundreds of
//! statements cost milliseconds and an order of magnitude more is still
//! unremarkable. Callers that need a plain bool lookup (e.g. the base-skin
//! diagnostic) adapt it with a closure over [`VerifyContext::attested`].
//!
//! Two properties to keep in mind:
//!
//! - The context is a **snapshot of trust**: offer cached and fetched roots
//!   to the [`PlatformTrust`] ratchets before building it. Attestation
//!   happens at [`VerifyContext::add_statement`] time, so advancing a root
//!   afterwards would not re-judge statements already admitted. Build a
//!   fresh context per verification run — and per logged-in account.
//! - Statement WAD bindings (`wad_name`, `wad_toc_digest`) are **advisory
//!   scoping, not enforced here**: what binds an attested statement to
//!   content is the file identities it lists. Both are surfaced on the
//!   [`Endorsement`] for callers that want to scope or report.

use std::collections::HashMap;

use thiserror::Error;

use crate::io::bundle::Bundle;
use crate::io::statement::{FileEntry, Statement};
use crate::io::statement_set::StatementSet;
use crate::trust::{AttestContext, AttestError, Attested, Evidence, PlatformTrust};

/// One platform's refusal of one piece of evidence, kept for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// Name of the refusing platform (UI attribution and log routing).
    pub platform: String,
    /// Index into the evidence slice offered to
    /// [`VerifyContext::add_statement`].
    pub evidence: usize,
    /// Why this platform refused this evidence.
    pub error: AttestError,
}

/// No platform accepted any of the offered evidence. Carries every
/// per-platform refusal; empty when no evidence (or no platform) was
/// available to try.
#[derive(Debug, Error, miette::Diagnostic, PartialEq, Eq, Clone)]
#[error("Statement is not attested by any trusted platform ({} rejections)", .rejections.len())]
pub struct NotAttested {
    /// One entry per (evidence, platform) pair tried before giving up.
    pub rejections: Vec<Rejection>,
}

/// The record of one attested statement: who vouched, over which lane, and
/// the statement's advisory WAD bindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endorsement {
    /// SHA-256 identity of the attested statement token.
    pub statement_hash: [u8; 32],
    /// Name of the WAD the statement's files belong to, if bound. Advisory
    /// scoping — not enforced by the context.
    pub wad_name: Option<String>,
    /// SHA-256 of the TOC of the WAD build the statement targets, if bound.
    /// Advisory scoping — not enforced by the context.
    pub wad_toc_digest: Option<[u8; 32]>,
    /// Name of the attesting platform (UI attribution and log routing).
    pub platform: String,
    /// How the statement was attested.
    pub attested: Attested,
}

/// The identity under which a file is offered for attestation — one of the
/// three bindings a statement's [`FileEntry`] carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCheck {
    /// xxh3 of the compressed chunk data as stored in the merged WAD (the
    /// WAD TOC checksum, what the runtime merged-WAD check observes). Fast,
    /// NOT collision resistant.
    ChecksumCompressed(u64),
    /// xxh3 of the uncompressed data. Fast, NOT collision resistant.
    ChecksumUncompressed(u64),
    /// SHA-256 of the decompressed chunk data as stored in the WAD: the
    /// cryptographic binding, for install/overlay-build-time verification.
    DigestDecompressed([u8; 32]),
}

impl FileCheck {
    fn matches(&self, entry: &FileEntry) -> bool {
        match *self {
            Self::ChecksumCompressed(checksum) => entry.checksum_compressed == checksum,
            Self::ChecksumUncompressed(checksum) => entry.checksum_uncompressed == checksum,
            Self::DigestDecompressed(digest) => entry.digest_decompressed == digest,
        }
    }
}

/// Which section of a bundle a defective item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleItemKind {
    Root,
    Set,
    Statement,
}

/// A bundle item that failed to parse or apply. Diagnostics only — the
/// bundle is untrusted input and bad items are skipped, never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleDefect {
    pub kind: BundleItemKind,
    /// Index within the item's bundle section.
    pub index: usize,
    pub error: String,
}

/// Result of [`VerifyContext::from_bundle`]: the assembled context plus
/// everything that did not make it in.
#[derive(Debug)]
pub struct BundleAssembly {
    pub context: VerifyContext,
    /// Statements no platform vouched for: (index in the bundle's
    /// statements section, the collected rejections).
    pub rejected_statements: Vec<(usize, NotAttested)>,
    /// Items that failed to parse or apply.
    pub defects: Vec<BundleDefect>,
}

/// Verification context over multiple platforms and many statements:
/// statements are attested as they are added, and the entries of every
/// attested statement feed a per-file lookup table.
#[derive(Debug, Clone)]
pub struct VerifyContext {
    platforms: Vec<PlatformTrust>,
    /// One record per attested statement, in insertion order.
    endorsements: Vec<Endorsement>,
    /// Statement token hash -> index into `endorsements` (dedup).
    by_statement: HashMap<[u8; 32], usize>,
    /// File name hash -> attested entries under that name, each tagged with
    /// the endorsement (index) that vouches for it.
    files: HashMap<u64, Vec<(FileEntry, usize)>>,
}

impl VerifyContext {
    /// Build an empty context over the given platform trust states. Advance
    /// the root ratchets (baked, cached, fetched) *before* this; the context
    /// judges statements against the trust state as-is.
    pub fn new(platforms: Vec<PlatformTrust>) -> Self {
        Self {
            platforms,
            endorsements: Vec::new(),
            by_statement: HashMap::new(),
            files: HashMap::new(),
        }
    }

    /// The platform trust states this context judges against.
    pub fn platforms(&self) -> &[PlatformTrust] {
        &self.platforms
    }

    /// Offer a statement with its evidence. Runs the attestation rule
    /// (any-of): each evidence item in order, against every platform, until
    /// one accepts. On acceptance the statement's file entries are folded
    /// into the lookup table and its [`Endorsement`] is returned; on refusal
    /// nothing is recorded and every rejection is reported.
    ///
    /// A statement that is already attested is returned as-is without
    /// re-judging — offering the same statement from several mods (or with
    /// different evidence, or under a changed [`AttestContext`]) is an
    /// idempotent no-op. The context never re-judges an admitted statement;
    /// to re-check under a new account or time, build a fresh context.
    pub fn add_statement(
        &mut self,
        statement: &Statement,
        evidence: &[Evidence<'_>],
        context: &AttestContext<'_>,
    ) -> Result<&Endorsement, NotAttested> {
        if let Some(&index) = self.by_statement.get(&statement.token_hash()) {
            return Ok(&self.endorsements[index]);
        }

        let mut rejections = Vec::new();
        let mut accepted = None;
        'evidence: for (evidence_index, item) in evidence.iter().enumerate() {
            for platform_index in self.platform_order(item) {
                let trust = &self.platforms[platform_index];
                match trust.attest(statement, *item, context) {
                    Ok(attested) => {
                        accepted = Some((platform_index, attested));
                        break 'evidence;
                    }
                    Err(error) => rejections.push(Rejection {
                        platform: trust.platform().name.clone(),
                        evidence: evidence_index,
                        error,
                    }),
                }
            }
        }
        let Some((platform_index, attested)) = accepted else {
            return Err(NotAttested { rejections });
        };

        let index = self.endorsements.len();
        self.endorsements.push(Endorsement {
            statement_hash: statement.token_hash(),
            wad_name: statement.wad_name().map(str::to_owned),
            wad_toc_digest: statement.wad_toc_digest().copied(),
            platform: self.platforms[platform_index].platform().name.clone(),
            attested,
        });
        self.by_statement.insert(statement.token_hash(), index);
        for entry in statement.entries() {
            self.files
                .entry(entry.name_hash)
                .or_default()
                .push((*entry, index));
        }
        Ok(&self.endorsements[index])
    }

    /// Platform indices in trial order. The `kid` on a direct attestation is
    /// a routing hint: platforms whose baked key matches it are tried first,
    /// so the honest path costs one signature check. The hint orders trials,
    /// it never filters them — a wrong hint costs wasted checks, not a
    /// refusal.
    fn platform_order(&self, evidence: &Evidence) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.platforms.len()).collect();
        if let Evidence::Direct { attestation } = evidence {
            let kid = *attestation.signer_pub_key();
            order.sort_by_key(|&index| self.platforms[index].platform().pub_key != kid);
        }
        order
    }

    /// All attested statements, in the order they were added — the record of
    /// which mod signatures were actually used.
    pub fn endorsements(&self) -> &[Endorsement] {
        &self.endorsements
    }

    /// The endorsement of a statement by its token hash, if attested.
    pub fn endorsement(&self, statement_hash: &[u8; 32]) -> Option<&Endorsement> {
        self.by_statement
            .get(statement_hash)
            .map(|&index| &self.endorsements[index])
    }

    /// Attest one file: does some attested statement list an entry with this
    /// name hash whose identity matches the check? Returns the endorsement
    /// that vouches for it (the first matching one, when several do).
    ///
    /// The xxh3 checksum lanes are what the runtime merged-WAD check
    /// observes and are not collision resistant; [`FileCheck::DigestDecompressed`]
    /// is the cryptographic binding for install/build-time verification.
    pub fn attested(&self, name_hash: u64, check: FileCheck) -> Option<&Endorsement> {
        self.files
            .get(&name_hash)?
            .iter()
            .find(|(entry, _)| check.matches(entry))
            .map(|&(_, index)| &self.endorsements[index])
    }

    /// Assemble a context from baked platform trust states and an untrusted
    /// [`Bundle`] — the one load path shared by the game-side verifier, the
    /// manager, and tooling:
    ///
    /// 1. every bundled root is offered to every platform's ratchet;
    /// 2. bundled sets are named by matching their digest against each
    ///    platform's accepted root (set names are routing, carried by roots);
    /// 3. every bundled statement is attested through the membership lane.
    ///
    /// The bundle is untrusted input: malformed, foreign, stale, or
    /// unvouched items become [`BundleAssembly`] diagnostics, never errors —
    /// tampering with a bundle can only remove trust.
    pub fn from_bundle(
        platforms: Vec<PlatformTrust>,
        bundle: &Bundle,
        context: &AttestContext,
    ) -> BundleAssembly {
        let mut this = Self::new(platforms);
        let mut defects = Vec::new();

        for (index, bytes) in bundle.roots().iter().enumerate() {
            let mut accepted = false;
            let mut errors: Vec<String> = Vec::new();
            for trust in &mut this.platforms {
                if !trust.platform().accepts_roots {
                    continue;
                }
                match trust.advance_root(bytes) {
                    // Ok(false) (stale or already current) is still a valid
                    // root under some platform key — not a defect.
                    Ok(_) => accepted = true,
                    Err(error) => errors.push(format!("{}: {error}", trust.platform().name)),
                }
            }
            if !accepted {
                defects.push(BundleDefect {
                    kind: BundleItemKind::Root,
                    index,
                    error: if errors.is_empty() {
                        "no platform accepts roots".to_owned()
                    } else {
                        errors.join("; ")
                    },
                });
            }
        }

        let mut named_sets: Vec<(String, StatementSet)> = Vec::new();
        for (index, bytes) in bundle.sets().iter().enumerate() {
            let set = match StatementSet::from_bytes(bytes) {
                Ok(set) => set,
                Err(error) => {
                    defects.push(BundleDefect {
                        kind: BundleItemKind::Set,
                        index,
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            let mut named = false;
            for trust in &this.platforms {
                let Some(root) = trust.root() else { continue };
                for (name, digest) in root.roots() {
                    // Dedup by (name, digest), not name alone: set names are
                    // per-platform labels, so two platforms may publish
                    // *different* sets under the same conventional name.
                    if *digest == set.digest()
                        && !named_sets.iter().any(|(existing_name, existing_set)| {
                            existing_name == name && existing_set.digest() == set.digest()
                        })
                    {
                        named_sets.push((name.clone(), set.clone()));
                        named = true;
                    }
                }
            }
            if !named {
                defects.push(BundleDefect {
                    kind: BundleItemKind::Set,
                    index,
                    error: "no accepted root references this set".to_owned(),
                });
            }
        }

        let mut rejected_statements = Vec::new();
        for (index, bytes) in bundle.statements().iter().enumerate() {
            let statement = match Statement::from_bytes(bytes) {
                Ok(statement) => statement,
                Err(error) => {
                    defects.push(BundleDefect {
                        kind: BundleItemKind::Statement,
                        index,
                        error: error.to_string(),
                    });
                    continue;
                }
            };
            let evidence: Vec<Evidence> = named_sets
                .iter()
                .map(|(name, set)| Evidence::Membership {
                    set_name: name,
                    set,
                })
                .collect();
            if let Err(rejection) = this.add_statement(&statement, &evidence, context) {
                rejected_statements.push((index, rejection));
            }
        }

        BundleAssembly {
            context: this,
            rejected_statements,
            defects,
        }
    }

    /// Every attested entry under a file name hash, with the endorsement
    /// vouching for it — e.g. to report "an attested version of this file
    /// exists, but not the one observed".
    pub fn attested_entries(
        &self,
        name_hash: u64,
    ) -> impl Iterator<Item = (&FileEntry, &Endorsement)> {
        self.files
            .get(&name_hash)
            .into_iter()
            .flatten()
            .map(|(entry, index)| (entry, &self.endorsements[*index]))
    }
}
