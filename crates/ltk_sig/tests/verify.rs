use ltk_sig::io::{
    attestation::{Attestation, AttestationParams},
    bundle::{Bundle, BundleParams},
    root::{Root, RootParams},
    statement::{FileEntry, Statement, StatementParams},
    statement_set::StatementSet,
};
use ltk_sig::trust::{
    AttestContext, AttestError, AttestationPolicy, Attested, Evidence, Platform, PlatformTrust,
};
use ltk_sig::verify::{FileCheck, VerifyContext};

// Key seeds: 1 = corpus platform, 2 = developer platform, 3 = partner
// platform, 9 = untrusted.
fn key(seed: u8) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
}

fn pub_key(seed: u8) -> [u8; 32] {
    key(seed).verifying_key().to_bytes()
}

/// A file entry whose checksums and digest are all derived from `seed`.
fn entry(name_hash: u64, seed: u8) -> FileEntry {
    FileEntry {
        name_hash,
        checksum_compressed: 1_000 + seed as u64,
        checksum_uncompressed: 2_000 + seed as u64,
        digest_decompressed: [seed; 32],
    }
}

fn statement(wad_name: Option<&str>, entries: &[FileEntry]) -> Statement {
    Statement::seal(&StatementParams {
        wad_name,
        wad_toc_digest: Some([7; 32]),
        entries,
    })
    .unwrap()
}

fn attestation(seed: u8, statement_hash: [u8; 32]) -> Attestation {
    Attestation::sign(
        &AttestationParams {
            time_issued_at: 1_000,
            time_expires_at: 10_000,
            statement_hash,
            account_binding: None,
        },
        &key(seed),
    )
    .unwrap()
}

fn platform(
    seed: u8,
    name: &str,
    accepts_roots: bool,
    attestations: AttestationPolicy,
) -> Platform {
    Platform {
        name: name.to_owned(),
        pub_key: pub_key(seed),
        url: "https://example.com/ltk".to_owned(),
        min_issued_at: 0,
        accepts_roots,
        attestations,
    }
}

/// A corpus-style platform (key 1): membership lane only, bootstrapped with
/// a root covering the given sets.
fn corpus_trust(sets: Vec<(String, [u8; 32])>) -> PlatformTrust {
    let root = Root::sign(
        &RootParams {
            time_issued_at: 1,
            roots: &sets,
        },
        &key(1),
    )
    .unwrap();
    PlatformTrust::new(
        platform(1, "corpus", true, AttestationPolicy::None),
        Some(root.as_bytes()),
    )
    .unwrap()
}

/// A developer-style platform (key 2): direct lane only.
fn developer_trust() -> PlatformTrust {
    PlatformTrust::new(
        platform(2, "developer", false, AttestationPolicy::Any),
        None,
    )
    .unwrap()
}

fn now(now: u64) -> AttestContext<'static> {
    AttestContext {
        now,
        account_id: None,
    }
}

#[test]
fn context_attests_and_answers_file_queries() {
    let entry_a = entry(10, 1);
    let entry_b = entry(20, 2);
    let stmt_a = statement(Some("Aatrox.wad.client"), &[entry_a]);
    let stmt_b = statement(Some("Zed.wad.client"), &[entry_b]);

    let set = StatementSet::from_hashes(vec![stmt_a.token_hash()]).unwrap();
    let mut context = VerifyContext::new(vec![
        corpus_trust(vec![("corpus".to_owned(), set.digest())]),
        developer_trust(),
    ]);

    // Statement A via membership, statement B via a direct attestation.
    let endorsement = context
        .add_statement(
            &stmt_a,
            &[Evidence::Membership {
                set_name: "corpus",
                set: &set,
            }],
            &now(1_500),
        )
        .unwrap();
    assert_eq!(endorsement.platform, "corpus");
    assert_eq!(endorsement.attested, Attested::Membership);
    assert_eq!(endorsement.wad_name.as_deref(), Some("Aatrox.wad.client"));
    assert_eq!(endorsement.wad_toc_digest, Some([7; 32]));
    assert_eq!(endorsement.statement_hash, stmt_a.token_hash());

    let att_b = attestation(2, stmt_b.token_hash());
    let endorsement = context
        .add_statement(
            &stmt_b,
            &[Evidence::Direct {
                attestation: &att_b,
            }],
            &now(1_500),
        )
        .unwrap();
    assert_eq!(endorsement.platform, "developer");
    assert_eq!(
        endorsement.attested,
        Attested::Direct {
            account_bound: false
        }
    );

    // Files answer under all three identities.
    let vouched = context
        .attested(10, FileCheck::ChecksumCompressed(1_001))
        .unwrap();
    assert_eq!(vouched.platform, "corpus");
    let vouched = context
        .attested(10, FileCheck::ChecksumUncompressed(2_001))
        .unwrap();
    assert_eq!(vouched.platform, "corpus");
    let vouched = context
        .attested(20, FileCheck::DigestDecompressed([2; 32]))
        .unwrap();
    assert_eq!(vouched.platform, "developer");

    // Wrong checksum or unknown name hash: nothing vouches.
    assert!(
        context
            .attested(10, FileCheck::ChecksumCompressed(1_002))
            .is_none()
    );
    assert!(
        context
            .attested(99, FileCheck::ChecksumCompressed(1_001))
            .is_none()
    );

    // The endorsement record doubles as the "used signatures" report.
    assert_eq!(context.endorsements().len(), 2);
    assert!(context.endorsement(&stmt_a.token_hash()).is_some());
    assert!(context.endorsement(&[0; 32]).is_none());
}

#[test]
fn unattested_statement_reports_all_rejections_and_stays_out() {
    let stmt = statement(None, &[entry(10, 1)]);
    let mut context = VerifyContext::new(vec![corpus_trust(Vec::new()), developer_trust()]);

    // Evidence 0: membership in a set no root covers. Evidence 1: an
    // attestation signed by an untrusted key.
    let set = StatementSet::from_hashes(vec![stmt.token_hash()]).unwrap();
    let foreign = attestation(9, stmt.token_hash());
    let err = context
        .add_statement(
            &stmt,
            &[
                Evidence::Membership {
                    set_name: "corpus",
                    set: &set,
                },
                Evidence::Direct {
                    attestation: &foreign,
                },
            ],
            &now(1_500),
        )
        .unwrap_err();

    // Every (evidence, platform) pair is reported with its own reason.
    let summary: Vec<(&str, usize)> = err
        .rejections
        .iter()
        .map(|r| (r.platform.as_str(), r.evidence))
        .collect();
    assert_eq!(
        summary,
        [
            ("corpus", 0),
            ("developer", 0),
            ("corpus", 1),
            ("developer", 1)
        ]
    );
    assert_eq!(err.rejections[0].error, AttestError::RootUnknownName);
    assert_eq!(err.rejections[1].error, AttestError::RootMissing);
    assert_eq!(
        err.rejections[2].error,
        AttestError::AttestationsNotAccepted
    );
    assert!(matches!(err.rejections[3].error, AttestError::Signature(_)));

    // A refused statement contributes nothing to the table.
    assert!(context.endorsements().is_empty());
    assert!(context.endorsement(&stmt.token_hash()).is_none());
    assert!(
        context
            .attested(10, FileCheck::ChecksumCompressed(1_001))
            .is_none()
    );

    // No evidence at all: refused with zero rejections.
    let err = context.add_statement(&stmt, &[], &now(1_500)).unwrap_err();
    assert!(err.rejections.is_empty());
}

#[test]
fn duplicate_statements_are_deduplicated() {
    let stmt = statement(None, &[entry(10, 1)]);
    let att = attestation(2, stmt.token_hash());
    let mut context = VerifyContext::new(vec![developer_trust()]);

    context
        .add_statement(
            &stmt,
            &[Evidence::Direct { attestation: &att }],
            &now(1_500),
        )
        .unwrap();

    // Re-offering the same statement — even with no evidence — returns the
    // existing endorsement and does not duplicate file entries.
    let endorsement = context.add_statement(&stmt, &[], &now(1_500)).unwrap();
    assert_eq!(endorsement.platform, "developer");
    assert_eq!(context.endorsements().len(), 1);
    assert_eq!(context.attested_entries(10).count(), 1);
}

#[test]
fn any_of_attributes_to_the_accepting_platform() {
    let stmt = statement(None, &[entry(10, 1)]);
    let att = attestation(3, stmt.token_hash());
    // Two platforms hold the kid-matched key, so the hint tries both before
    // the developer platform: the first refuses by lane policy, the second
    // accepts — acceptance must not require the first trial to succeed.
    let mut context = VerifyContext::new(vec![
        developer_trust(),
        PlatformTrust::new(
            platform(3, "partner-roots", false, AttestationPolicy::None),
            None,
        )
        .unwrap(),
        PlatformTrust::new(platform(3, "partner", false, AttestationPolicy::Any), None).unwrap(),
    ]);

    let endorsement = context
        .add_statement(
            &stmt,
            &[Evidence::Direct { attestation: &att }],
            &now(1_500),
        )
        .unwrap();
    assert_eq!(endorsement.platform, "partner");
}

#[test]
fn later_evidence_can_attest_after_earlier_fails() {
    let stmt = statement(None, &[entry(10, 1)]);
    // Membership set that no root covers, then a valid direct attestation.
    let set = StatementSet::from_hashes(vec![stmt.token_hash()]).unwrap();
    let att = attestation(2, stmt.token_hash());
    let mut context = VerifyContext::new(vec![corpus_trust(Vec::new()), developer_trust()]);

    let endorsement = context
        .add_statement(
            &stmt,
            &[
                Evidence::Membership {
                    set_name: "corpus",
                    set: &set,
                },
                Evidence::Direct { attestation: &att },
            ],
            &now(1_500),
        )
        .unwrap();
    assert_eq!(
        endorsement.attested,
        Attested::Direct {
            account_bound: false
        }
    );
}

#[test]
fn inclusion_proof_attests_without_the_set() {
    let stmt = statement(None, &[entry(10, 1)]);
    let set = StatementSet::from_hashes(vec![stmt.token_hash()]).unwrap();
    let proof = set.prove(&stmt.token_hash()).unwrap();

    // The context only ever sees the proof — the shape of a game-side
    // verifier that holds no set files.
    let mut context = VerifyContext::new(vec![corpus_trust(vec![(
        "corpus".to_owned(),
        set.digest(),
    )])]);
    let endorsement = context
        .add_statement(
            &stmt,
            &[Evidence::Inclusion {
                set_name: "corpus",
                proof: &proof,
            }],
            &now(1_500),
        )
        .unwrap();
    assert_eq!(endorsement.attested, Attested::Membership);
    assert!(
        context
            .attested(10, FileCheck::ChecksumCompressed(1_001))
            .is_some()
    );
}

#[test]
fn account_bound_endorsement_and_snapshot_dedup() {
    use ltk_sig::io::attestation::AccountBinding;

    let stmt = statement(None, &[entry(10, 1)]);
    let binding = AccountBinding::new(vec![9; 16], "account-123").unwrap();
    let att = Attestation::sign(
        &AttestationParams {
            time_issued_at: 1_000,
            time_expires_at: 10_000,
            statement_hash: stmt.token_hash(),
            account_binding: Some(binding),
        },
        &key(2),
    )
    .unwrap();
    let mut context = VerifyContext::new(vec![developer_trust()]);
    let account = AttestContext {
        now: 1_500,
        account_id: Some("account-123"),
    };

    let endorsement = context
        .add_statement(&stmt, &[Evidence::Direct { attestation: &att }], &account)
        .unwrap();
    assert_eq!(
        endorsement.attested,
        Attested::Direct {
            account_bound: true
        }
    );

    // The context is a snapshot: re-offering an admitted statement under a
    // changed context (different account, time past expiry) returns the
    // existing endorsement without re-judging. Re-checking requires a fresh
    // context — hence one per verification run and per account.
    let other_account = AttestContext {
        now: 20_000,
        account_id: Some("account-456"),
    };
    let endorsement = context
        .add_statement(
            &stmt,
            &[Evidence::Direct { attestation: &att }],
            &other_account,
        )
        .unwrap();
    assert_eq!(
        endorsement.attested,
        Attested::Direct {
            account_bound: true
        }
    );
}

#[test]
fn same_file_from_two_statements_resolves_by_checksum() {
    // Two mod versions ship different content for the same file name.
    let stmt_v1 = statement(None, &[entry(10, 1)]);
    let stmt_v2 = statement(None, &[entry(10, 3)]);
    let att_v1 = attestation(2, stmt_v1.token_hash());
    let att_v2 = attestation(2, stmt_v2.token_hash());
    let mut context = VerifyContext::new(vec![developer_trust()]);

    context
        .add_statement(
            &stmt_v1,
            &[Evidence::Direct {
                attestation: &att_v1,
            }],
            &now(1_500),
        )
        .unwrap();
    context
        .add_statement(
            &stmt_v2,
            &[Evidence::Direct {
                attestation: &att_v2,
            }],
            &now(1_500),
        )
        .unwrap();

    // Each observed checksum resolves to the statement that listed it.
    let vouched = context
        .attested(10, FileCheck::ChecksumCompressed(1_001))
        .unwrap();
    assert_eq!(vouched.statement_hash, stmt_v1.token_hash());
    let vouched = context
        .attested(10, FileCheck::ChecksumCompressed(1_003))
        .unwrap();
    assert_eq!(vouched.statement_hash, stmt_v2.token_hash());
    assert_eq!(context.attested_entries(10).count(), 2);
}

#[test]
fn bundle_assembles_into_context() {
    use ltk_sig::verify::BundleItemKind;

    let stmt = statement(Some("Aatrox.wad.client"), &[entry(10, 1)]);
    let unvouched = statement(None, &[entry(20, 2)]);
    let set = StatementSet::from_hashes(vec![stmt.token_hash()]).unwrap();
    let root = Root::sign(
        &RootParams {
            time_issued_at: 5,
            roots: &[("corpus".to_owned(), set.digest())],
        },
        &key(1),
    )
    .unwrap();

    // A bundle with one junk item per section: junk root (unverifiable),
    // valid-but-unreferenced set, junk statement — all tolerated.
    let orphan_set = StatementSet::from_hashes(vec![[9; 32]]).unwrap();
    let bundle = Bundle::seal(&BundleParams {
        roots: &[root.as_bytes().to_vec(), b"junk".to_vec()],
        sets: &[set.to_bytes(), orphan_set.to_bytes()],
        statements: &[
            stmt.as_bytes().to_vec(),
            unvouched.as_bytes().to_vec(),
            b"junk".to_vec(),
        ],
    })
    .unwrap();
    let bundle = Bundle::from_bytes(bundle.as_bytes()).unwrap();
    assert_eq!(bundle.roots().len(), 2);
    assert_eq!(bundle.statements().len(), 3);

    // Only the platform key is trusted; the bundle provides the rest.
    let trust =
        PlatformTrust::new(platform(1, "corpus", true, AttestationPolicy::None), None).unwrap();
    let assembly = VerifyContext::from_bundle(vec![trust], &bundle, &now(1_500));

    assert_eq!(assembly.context.endorsements().len(), 1);
    let vouched = assembly
        .context
        .attested(10, FileCheck::ChecksumCompressed(1_001))
        .unwrap();
    assert_eq!(vouched.wad_name.as_deref(), Some("Aatrox.wad.client"));
    assert_eq!(
        assembly.context.platforms()[0]
            .root()
            .unwrap()
            .time_issued_at(),
        5
    );

    // The unvouched statement is rejected, the junk items are defects.
    assert_eq!(assembly.rejected_statements.len(), 1);
    assert_eq!(assembly.rejected_statements[0].0, 1);
    let kinds: Vec<(BundleItemKind, usize)> =
        assembly.defects.iter().map(|d| (d.kind, d.index)).collect();
    assert_eq!(
        kinds,
        [
            (BundleItemKind::Root, 1),
            (BundleItemKind::Set, 1),
            (BundleItemKind::Statement, 2)
        ]
    );
}

#[test]
fn bundle_names_same_named_sets_from_two_platforms() {
    // Set names are per-platform labels, so two platforms publishing
    // *different* sets under the same conventional name ("corpus") must not
    // shadow each other: a statement endorsed only by the second platform's
    // set still attests, and neither set is a defect.
    let stmt_a = statement(None, &[entry(10, 1)]);
    let stmt_b = statement(None, &[entry(20, 2)]);
    let set_a = StatementSet::from_hashes(vec![stmt_a.token_hash()]).unwrap();
    let set_b = StatementSet::from_hashes(vec![stmt_b.token_hash()]).unwrap();
    let sign_root = |seed: u8, digest: [u8; 32]| {
        Root::sign(
            &RootParams {
                time_issued_at: 5,
                roots: &[("corpus".to_owned(), digest)],
            },
            &key(seed),
        )
        .unwrap()
    };
    let root_a = sign_root(1, set_a.digest());
    let root_b = sign_root(3, set_b.digest());

    let bundle = Bundle::seal(&BundleParams {
        roots: &[root_a.as_bytes().to_vec(), root_b.as_bytes().to_vec()],
        sets: &[set_a.to_bytes(), set_b.to_bytes()],
        statements: &[stmt_a.as_bytes().to_vec(), stmt_b.as_bytes().to_vec()],
    })
    .unwrap();

    let platforms = vec![
        PlatformTrust::new(platform(1, "corpus", true, AttestationPolicy::None), None).unwrap(),
        PlatformTrust::new(platform(3, "partner", true, AttestationPolicy::None), None).unwrap(),
    ];
    let assembly = VerifyContext::from_bundle(platforms, &bundle, &now(1_500));

    assert_eq!(assembly.defects, []);
    assert_eq!(assembly.rejected_statements, []);
    assert_eq!(assembly.context.endorsements().len(), 2);
    let vouched = assembly.context.endorsement(&stmt_a.token_hash()).unwrap();
    assert_eq!(vouched.platform, "corpus");
    let vouched = assembly.context.endorsement(&stmt_b.token_hash()).unwrap();
    assert_eq!(vouched.platform, "partner");
}

#[test]
fn membership_set_covers_many_statements() {
    // The corpus bulk path: one set file endorsing many statements at once.
    let statements: Vec<Statement> = (0..200u64)
        .map(|i| statement(None, &[entry(i, i as u8)]))
        .collect();
    let mut hashes: Vec<[u8; 32]> = statements.iter().map(|s| s.token_hash()).collect();
    hashes.sort();
    let set = StatementSet::from_hashes(hashes).unwrap();

    let mut context = VerifyContext::new(vec![corpus_trust(vec![(
        "corpus".to_owned(),
        set.digest(),
    )])]);
    for stmt in &statements {
        context
            .add_statement(
                stmt,
                &[Evidence::Membership {
                    set_name: "corpus",
                    set: &set,
                }],
                &now(1_500),
            )
            .unwrap();
    }

    assert_eq!(context.endorsements().len(), 200);
    for i in 0..200u64 {
        assert!(
            context
                .attested(i, FileCheck::ChecksumCompressed(1_000 + (i as u8) as u64))
                .is_some()
        );
    }
}
