use ltk_sig::io::{
    attestation::{AccountBinding, Attestation, AttestationParams},
    cose::TokenError,
    root::{Root, RootParams},
    statement::{FileEntry, Statement, StatementParams},
    statement_set::{StatementSet, StatementSetError},
};
use ltk_sig::trust::{
    AttestContext, AttestError, AttestationPolicy, Attested, Evidence, Platform, PlatformSet,
    PlatformTrust, TrustError,
};

// Key seeds: 1 = corpus platform, 2 = developer platform, 9 = untrusted.
fn key(seed: u8) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
}

fn pub_key(seed: u8) -> [u8; 32] {
    key(seed).verifying_key().to_bytes()
}

fn entries() -> Vec<FileEntry> {
    vec![
        FileEntry {
            name_hash: 1,
            checksum_compressed: 11,
            checksum_uncompressed: 21,
            digest_decompressed: [1; 32],
        },
        FileEntry {
            name_hash: 2,
            checksum_compressed: 12,
            checksum_uncompressed: 22,
            digest_decompressed: [2; 32],
        },
    ]
}

fn statement(wad_name: Option<&str>) -> Statement {
    let entries = entries();
    Statement::seal(&StatementParams {
        wad_name,
        wad_toc_digest: Some([7; 32]),
        entries: &entries,
    })
    .unwrap()
}

fn attestation(
    platform_key: &ed25519_dalek::SigningKey,
    statement_hash: [u8; 32],
    time_expires_at: u64,
    account_binding: Option<AccountBinding>,
) -> Attestation {
    Attestation::sign(
        &AttestationParams {
            time_issued_at: 1_000,
            time_expires_at,
            statement_hash,
            account_binding,
        },
        platform_key,
    )
    .unwrap()
}

fn root(
    platform_key: &ed25519_dalek::SigningKey,
    time_issued_at: u64,
    roots: Vec<(String, [u8; 32])>,
) -> Root {
    Root::sign(
        &RootParams {
            time_issued_at,
            roots: &roots,
        },
        platform_key,
    )
    .unwrap()
}

/// A fully permissive platform entry; tests that exercise lane policy or
/// the floor construct restricted ones explicitly.
fn platform(seed: u8) -> Platform {
    Platform {
        name: format!("platform-{seed}"),
        pub_key: pub_key(seed),
        url: "https://example.com/ltk".to_owned(),
        min_issued_at: 0,
        accepts_roots: true,
        attestations: AttestationPolicy::Any,
    }
}

/// Platform key = key(1), bootstrap root at iat 1.
fn trust(roots: Vec<(String, [u8; 32])>) -> PlatformTrust {
    let bootstrap = root(&key(1), 1, roots);
    PlatformTrust::new(platform(1), Some(bootstrap.as_bytes())).unwrap()
}

fn context(now: u64) -> AttestContext<'static> {
    AttestContext {
        now,
        account_id: None,
    }
}

// ----------------------------------------------------------------- statements

#[test]
fn statement_round_trip_and_lookup() {
    let statement = statement(Some("Aatrox.wad.client"));

    assert_eq!(statement.wad_name(), Some("Aatrox.wad.client"));
    assert_eq!(statement.wad_toc_digest(), Some(&[7; 32]));
    assert_eq!(statement.entries(), entries().as_slice());
    assert_eq!(statement.get_by_name_hash(2), Some(&entries()[1]));
    assert_eq!(statement.get_by_name_hash(3), None);

    let reparsed = Statement::from_bytes(statement.as_bytes()).unwrap();
    assert_eq!(reparsed, statement);
    assert_eq!(reparsed.token_hash(), statement.token_hash());
}

#[test]
fn statement_wad_bindings_are_optional() {
    let entries = entries();
    let statement = Statement::seal(&StatementParams {
        wad_name: None,
        wad_toc_digest: None,
        entries: &entries,
    })
    .unwrap();
    assert_eq!(statement.wad_name(), None);
    assert_eq!(statement.wad_toc_digest(), None);

    let reparsed = Statement::from_bytes(statement.as_bytes()).unwrap();
    assert_eq!(reparsed, statement);
}

#[test]
fn statement_rejects_unsorted_entries() {
    let mut unsorted = entries();
    unsorted.reverse();
    let result = Statement::seal(&StatementParams {
        wad_name: None,
        wad_toc_digest: None,
        entries: &unsorted,
    });
    assert_eq!(
        result,
        Err(TokenError::ClaimInvalid("entries must be sorted"))
    );
}

#[test]
fn statement_rejects_invalid_wad_name() {
    for bad in ["", "Åatrox.wad.client", &"x".repeat(128)] {
        let result = Statement::seal(&StatementParams {
            wad_name: Some(bad),
            wad_toc_digest: None,
            entries: &[],
        });
        assert_eq!(result, Err(TokenError::ClaimInvalid("wad_name")));
    }
}

#[test]
fn statement_identity_is_content() {
    // Two identical statements seal to identical bytes and hashes; changing
    // one claim changes the hash.
    let a = statement(Some("Aatrox.wad.client"));
    let b = statement(Some("Aatrox.wad.client"));
    assert_eq!(a.as_bytes(), b.as_bytes());
    assert_eq!(a.token_hash(), b.token_hash());

    let c = statement(Some("Zed.wad.client"));
    assert_ne!(a.token_hash(), c.token_hash());
}

#[test]
fn bare_token_is_domain_separated() {
    use ltk_sig::io::cose::{claim, seal_bare};

    // A bare token with the wrong typ must not parse as a statement.
    let claims = coset::cwt::ClaimsSetBuilder::new()
        .private_claim(
            claim::FILE_ENTRIES,
            coset::cbor::value::Value::Bytes(Vec::new()),
        )
        .build();
    let raw = seal_bare("ltk/other", claims).unwrap();
    assert_eq!(
        Statement::from_bytes(&raw),
        Err(TokenError::TypMismatch {
            expected: Statement::TYP
        })
    );

    // A statement is not a COSE structure at all.
    let statement = statement(None);
    assert!(Attestation::from_bytes(statement.as_bytes()).is_err());
}

#[test]
fn bare_token_rejects_non_canonical_bytes() {
    let statement = statement(None);

    // Trailing garbage changes the identity without changing the parsed
    // content — must be rejected, not silently accepted under a new hash.
    let mut trailing = statement.as_bytes().to_vec();
    trailing.push(0);
    assert_eq!(
        Statement::from_bytes(&trailing),
        Err(TokenError::NonCanonical)
    );
}

// --------------------------------------------------------------- attestations

#[test]
fn attestation_round_trip() {
    let statement = statement(None);
    let attestation = attestation(&key(1), statement.token_hash(), 2_000, None);

    assert_eq!(attestation.time_issued_at(), 1_000);
    assert_eq!(attestation.time_expires_at(), 2_000);
    assert_eq!(attestation.statement_hash(), &statement.token_hash());
    assert_eq!(attestation.account_binding(), None);
    assert!(!attestation.is_expired_at(1_999));
    assert!(attestation.is_expired_at(2_000));
    assert_eq!(attestation.signer_pub_key(), &pub_key(1));
    attestation.verify_signature(&pub_key(1)).unwrap();

    let reparsed = Attestation::from_bytes(attestation.as_bytes()).unwrap();
    assert_eq!(reparsed, attestation);
}

#[test]
fn attestation_requires_exp() {
    use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};
    use ltk_sig::io::cose::{claim, sign_token, timestamp_from_secs};

    // An attestation without an expiry would be irrevocable short of a
    // platform-key rotation — refused at parse.
    let claims = ClaimsSetBuilder::new()
        .issued_at(timestamp_from_secs(1_000).unwrap())
        .private_claim(claim::STATEMENT_HASH, Value::Bytes(vec![3; 32]))
        .build();
    let raw = sign_token(Attestation::TYP, claims, &key(1)).unwrap();
    assert_eq!(
        Attestation::from_bytes(&raw),
        Err(TokenError::ClaimMissing("exp"))
    );
}

#[test]
fn attestation_rejects_exp_not_after_iat() {
    let result = Attestation::sign(
        &AttestationParams {
            time_issued_at: 2_000,
            time_expires_at: 2_000,
            statement_hash: [3; 32],
            account_binding: None,
        },
        &key(1),
    );
    assert_eq!(result, Err(TokenError::ClaimInvalid("exp not after iat")));
}

#[test]
fn account_binding_validates_salt() {
    assert_eq!(
        AccountBinding::new(vec![9; 4], "account-123"),
        Err(TokenError::ClaimInvalid("account_id_salt"))
    );
    assert_eq!(
        AccountBinding::new(vec![9; 65], "account-123"),
        Err(TokenError::ClaimInvalid("account_id_salt"))
    );
    // Different salts produce different hashes for the same account.
    let a = AccountBinding::new(vec![1; 16], "account-123").unwrap();
    let b = AccountBinding::new(vec![2; 16], "account-123").unwrap();
    assert_ne!(a.hash(), b.hash());
}

#[test]
fn attestation_rejects_incomplete_account_binding() {
    use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};
    use ltk_sig::io::cose::{claim, sign_token, timestamp_from_secs};

    // An attestation carrying a salt without the hash must not parse.
    let claims = ClaimsSetBuilder::new()
        .issued_at(timestamp_from_secs(1_000).unwrap())
        .expiration_time(timestamp_from_secs(2_000).unwrap())
        .private_claim(claim::STATEMENT_HASH, Value::Bytes(vec![3; 32]))
        .private_claim(claim::ACCOUNT_ID_SALT, Value::Bytes(vec![9; 16]))
        .build();
    let raw = sign_token(Attestation::TYP, claims, &key(1)).unwrap();
    assert_eq!(
        Attestation::from_bytes(&raw),
        Err(TokenError::ClaimInvalid("account binding incomplete"))
    );
}

#[test]
fn signed_token_type_is_domain_separated() {
    let attestation = attestation(&key(1), [3; 32], 2_000, None);
    assert_eq!(
        Root::from_bytes(attestation.as_bytes()),
        Err(TokenError::TypMismatch {
            expected: Root::TYP
        })
    );
}

#[test]
fn tampered_token_fails_verification() {
    let attestation = attestation(&key(1), [3; 32], 2_000, None);
    let mut bytes = attestation.as_bytes().to_vec();
    // Trailing bytes are the Ed25519 signature; flip one bit.
    *bytes.last_mut().unwrap() ^= 1;
    let tampered = Attestation::from_bytes(&bytes).unwrap();
    assert_eq!(
        tampered.verify_signature(&pub_key(1)),
        Err(TokenError::SignatureInvalid)
    );
}

// ---------------------------------------------------------------------- roots

#[test]
fn root_round_trip() {
    let sets = vec![
        ("aatrox.wad.client".to_owned(), [3; 32]),
        ("zed.wad.client".to_owned(), [4; 32]),
    ];
    let root = root(&key(1), 42, sets.clone());

    assert_eq!(root.time_issued_at(), 42);
    assert_eq!(root.roots(), sets.as_slice());
    assert_eq!(root.root_digest("aatrox.wad.client"), Some(&[3; 32]));
    assert_eq!(root.root_digest("annie.wad.client"), None);
    root.verify_signature(&pub_key(1)).unwrap();

    let reparsed = Root::from_bytes(root.as_bytes()).unwrap();
    assert_eq!(reparsed, root);
}

#[test]
fn root_may_publish_nothing() {
    let root = root(&key(1), 1, Vec::new());
    assert!(root.roots().is_empty());
    let reparsed = Root::from_bytes(root.as_bytes()).unwrap();
    assert!(reparsed.roots().is_empty());
}

#[test]
fn root_rejects_unsorted_or_invalid_names() {
    let unsorted = vec![("b".to_owned(), [4; 32]), ("a".to_owned(), [3; 32])];
    let result = Root::sign(
        &RootParams {
            time_issued_at: 1,
            roots: &unsorted,
        },
        &key(1),
    );
    assert_eq!(
        result,
        Err(TokenError::ClaimInvalid("roots must be sorted"))
    );

    let duplicate = vec![("a".to_owned(), [3; 32]), ("a".to_owned(), [4; 32])];
    let result = Root::sign(
        &RootParams {
            time_issued_at: 1,
            roots: &duplicate,
        },
        &key(1),
    );
    assert_eq!(
        result,
        Err(TokenError::ClaimInvalid("roots must be sorted"))
    );

    for bad in ["", "nöt-ascii", &"x".repeat(128)] {
        let invalid = vec![(bad.to_owned(), [3; 32])];
        let result = Root::sign(
            &RootParams {
                time_issued_at: 1,
                roots: &invalid,
            },
            &key(1),
        );
        assert_eq!(result, Err(TokenError::ClaimInvalid("roots")));
    }
}

// ----------------------------------------------------------------------- sets

#[test]
fn statement_set_round_trip() {
    let set = StatementSet::from_hashes(vec![[1; 32], [2; 32]]).unwrap();
    assert_eq!(set.len(), 2);
    assert!(set.contains(&[1; 32]));
    assert!(!set.contains(&[3; 32]));

    let reparsed = StatementSet::from_bytes(&set.to_bytes()).unwrap();
    assert_eq!(reparsed, set);
    assert_eq!(reparsed.digest(), set.digest());
}

#[test]
fn statement_set_rejects_malformed_input() {
    assert_eq!(
        StatementSet::from_hashes(vec![[2; 32], [1; 32]]),
        Err(StatementSetError::Malformed("hashes must be sorted"))
    );
    assert_eq!(
        StatementSet::from_hashes(vec![[1; 32], [1; 32]]),
        Err(StatementSetError::Malformed("hashes must be sorted"))
    );
    assert_eq!(
        StatementSet::from_bytes(&[0; 33]),
        Err(StatementSetError::Malformed("length not a multiple of 32"))
    );
}

// --------------------------------------------------------------- platform set

#[test]
fn platform_set_round_trip() {
    let platforms = vec![
        Platform {
            name: "corpus".to_owned(),
            pub_key: pub_key(1),
            url: "https://corpus.example.com/ltk".to_owned(),
            min_issued_at: 1_000,
            accepts_roots: true,
            attestations: AttestationPolicy::None,
        },
        Platform {
            name: "developer".to_owned(),
            pub_key: pub_key(2),
            url: "https://dev.example.com/ltk".to_owned(),
            min_issued_at: 0,
            accepts_roots: false,
            attestations: AttestationPolicy::AccountBound,
        },
    ];
    let set = PlatformSet::seal(&platforms).unwrap();

    assert_eq!(set.platforms(), platforms.as_slice());
    assert_eq!(set.get("corpus"), Some(&platforms[0]));
    assert_eq!(set.get("unknown"), None);

    let reparsed = PlatformSet::from_bytes(set.as_bytes()).unwrap();
    assert_eq!(reparsed, set);
    assert_eq!(reparsed.token_hash(), set.token_hash());
}

#[test]
fn platform_set_rejects_unsorted_or_invalid_entries() {
    let entry = |name: &str| Platform {
        name: name.to_owned(),
        ..platform(1)
    };

    let result = PlatformSet::seal(&[entry("b"), entry("a")]);
    assert_eq!(
        result,
        Err(TokenError::ClaimInvalid("platforms must be sorted"))
    );

    let result = PlatformSet::seal(&[entry("a"), entry("a")]);
    assert_eq!(
        result,
        Err(TokenError::ClaimInvalid("platforms must be sorted"))
    );

    let result = PlatformSet::seal(&[entry("")]);
    assert_eq!(result, Err(TokenError::ClaimInvalid("platforms")));

    let result = PlatformSet::seal(&[Platform {
        url: String::new(),
        ..platform(1)
    }]);
    assert_eq!(result, Err(TokenError::ClaimInvalid("platforms: url")));
}

#[test]
fn platform_set_rejects_unknown_attestation_policy() {
    use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};
    use ltk_sig::io::cose::{claim, seal_bare};

    // Entry map with an attestation policy string this verifier does not
    // know: refused rather than misread (baked config, not a wire format
    // that needs forward compatibility).
    let entry = Value::Map(vec![
        (Value::from(1), Value::Text("corpus".to_owned())),
        (Value::from(2), Value::Bytes(pub_key(1).to_vec())),
        (
            Value::from(3),
            Value::Text("https://example.com".to_owned()),
        ),
        (Value::from(4), Value::from(0u64)),
        (Value::from(5), Value::Bool(true)),
        (Value::from(6), Value::Text("future-policy".to_owned())),
    ]);
    let claims = ClaimsSetBuilder::new()
        .private_claim(claim::PLATFORMS, Value::Array(vec![entry]))
        .build();
    let raw = seal_bare(PlatformSet::TYP, claims).unwrap();
    assert_eq!(
        PlatformSet::from_bytes(&raw),
        Err(TokenError::ClaimInvalid("platforms: attestations"))
    );
}

#[cfg(feature = "json")]
#[test]
fn platform_set_from_json_template() {
    use ltk_sig::io::platform_set::TemplateError;

    let hex_key: String = pub_key(1).iter().map(|b| format!("{b:02x}")).collect();
    let json = format!(
        r#"{{
            "platforms": [
                {{
                    "name": "developer",
                    "pub_key": "{hex_key}",
                    "url": "https://dev.example.com/ltk",
                    "min_issued_at": 500,
                    "accepts": {{ "roots": false, "attestations": "account-bound" }}
                }},
                {{
                    "name": "corpus",
                    "pub_key": "{hex_key}",
                    "url": "https://corpus.example.com/ltk",
                    "min_issued_at": 1000,
                    "accepts": {{ "roots": true, "attestations": "none" }}
                }}
            ]
        }}"#
    );
    let set = PlatformSet::from_json(&json).unwrap();

    // Entries are sorted by name regardless of template order.
    let names: Vec<&str> = set.platforms().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["corpus", "developer"]);

    let corpus = set.get("corpus").unwrap();
    assert_eq!(corpus.pub_key, pub_key(1));
    assert_eq!(corpus.min_issued_at, 1_000);
    assert!(corpus.accepts_roots);
    assert_eq!(corpus.attestations, AttestationPolicy::None);

    let developer = set.get("developer").unwrap();
    assert!(!developer.accepts_roots);
    assert_eq!(developer.attestations, AttestationPolicy::AccountBound);

    // The template round-trips through the wire format.
    let reparsed = PlatformSet::from_bytes(set.as_bytes()).unwrap();
    assert_eq!(reparsed, set);

    // Malformed keys and unknown fields are loud errors.
    let bad_key = json.replacen(&hex_key, "not-hex", 1);
    assert_eq!(
        PlatformSet::from_json(&bad_key),
        Err(TemplateError::Invalid("pub_key must be 64 hex chars"))
    );
    let typo = json.replace("min_issued_at", "min_isued_at");
    assert!(matches!(
        PlatformSet::from_json(&typo),
        Err(TemplateError::Json(_))
    ));
}

// ------------------------------------------------------------- platform trust

#[test]
fn platform_trust_bootstrap_verifies_root() {
    let trust = trust(Vec::new());
    assert_eq!(trust.root().unwrap().time_issued_at(), 1);
    assert_eq!(trust.platform().pub_key, pub_key(1));

    // A root signed by a foreign key must not bootstrap.
    let bootstrap = root(&key(9), 1, Vec::new());
    assert_eq!(
        PlatformTrust::new(platform(1), Some(bootstrap.as_bytes())),
        Err(TrustError::Token(TokenError::SignatureInvalid))
    );

    // Direct-only platforms carry no root.
    let direct_only = PlatformTrust::new(platform(2), None).unwrap();
    assert_eq!(direct_only.root(), None);
}

#[test]
fn root_ratchet_moves_forward_only() {
    let mut trust = trust(Vec::new());

    // Newer root advances.
    let newer = root(&key(1), 5, Vec::new());
    assert!(trust.advance_root(newer.as_bytes()).unwrap());
    assert_eq!(trust.root().unwrap().time_issued_at(), 5);

    // Stale root is ignored, not an error (max of baked/cached/fetched).
    let stale = root(&key(1), 3, Vec::new());
    assert!(!trust.advance_root(stale.as_bytes()).unwrap());
    assert_eq!(trust.root().unwrap().time_issued_at(), 5);

    // Re-offering the current root is an idempotent no-op.
    assert!(!trust.advance_root(newer.as_bytes()).unwrap());

    // A *different* root at the same issue time is an equivocation error.
    let conflicting = root(&key(1), 5, vec![("a".to_owned(), [3; 32])]);
    assert_eq!(
        trust.advance_root(conflicting.as_bytes()),
        Err(TrustError::Conflict)
    );
    assert_eq!(trust.root().unwrap().as_bytes(), newer.as_bytes());

    // A root signed by a foreign key never advances the ratchet.
    let foreign = root(&key(9), 9, Vec::new());
    assert_eq!(
        trust.advance_root(foreign.as_bytes()),
        Err(TrustError::Token(TokenError::SignatureInvalid))
    );
}

#[test]
fn root_floor_is_enforced() {
    let floored = Platform {
        min_issued_at: 100,
        ..platform(1)
    };

    // A bootstrap root below the floor is a build-time configuration error.
    let stale_bootstrap = root(&key(1), 50, Vec::new());
    assert_eq!(
        PlatformTrust::new(floored.clone(), Some(stale_bootstrap.as_bytes())),
        Err(TrustError::RootBelowFloor)
    );

    // Fetched roots below the floor are silently ignored; at or above it,
    // the ratchet advances normally.
    let mut trust = PlatformTrust::new(floored, None).unwrap();
    assert!(!trust.advance_root(stale_bootstrap.as_bytes()).unwrap());
    assert_eq!(trust.root(), None);
    let fresh = root(&key(1), 150, Vec::new());
    assert!(trust.advance_root(fresh.as_bytes()).unwrap());
    assert_eq!(trust.root().unwrap().time_issued_at(), 150);
}

#[test]
fn lane_policy_refuses_roots() {
    let direct_only = Platform {
        accepts_roots: false,
        ..platform(1)
    };
    let root = root(&key(1), 5, Vec::new());

    // Roots from a direct-only platform are refused outright — a stolen key
    // for such a platform cannot mint durable membership.
    assert_eq!(
        PlatformTrust::new(direct_only.clone(), Some(root.as_bytes())),
        Err(TrustError::RootsNotAccepted)
    );
    let mut trust = PlatformTrust::new(direct_only, None).unwrap();
    assert_eq!(
        trust.advance_root(root.as_bytes()),
        Err(TrustError::RootsNotAccepted)
    );
}

#[test]
fn lane_policy_refuses_direct_attestations() {
    let statement = statement(None);
    // A corpus-style platform: roots only.
    let roots_only = Platform {
        attestations: AttestationPolicy::None,
        ..platform(1)
    };
    let trust = PlatformTrust::new(roots_only, None).unwrap();
    let attestation = attestation(&key(1), statement.token_hash(), 2_000, None);

    // Even a validly-signed attestation is refused: the lane is closed.
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Direct {
                attestation: &attestation,
            },
            &context(1_500),
        ),
        Err(AttestError::AttestationsNotAccepted)
    );
}

#[test]
fn lane_policy_requires_account_binding() {
    let statement = statement(None);
    // A developer-style platform: account-bound attestations only.
    let developer = Platform {
        attestations: AttestationPolicy::AccountBound,
        ..platform(2)
    };
    let trust = PlatformTrust::new(developer, None).unwrap();

    // An unbound attestation is refused even though the signature is valid —
    // a stolen developer-platform key cannot mint account-free authority.
    let unbound = attestation(&key(2), statement.token_hash(), 2_000, None);
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Direct {
                attestation: &unbound,
            },
            &context(1_500),
        ),
        Err(AttestError::AccountBindingRequired)
    );

    // A bound one attests normally.
    let binding = AccountBinding::new(vec![9; 16], "account-123").unwrap();
    let bound = attestation(&key(2), statement.token_hash(), 2_000, Some(binding));
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Direct {
                attestation: &bound,
            },
            &AttestContext {
                now: 1_500,
                account_id: Some("account-123"),
            },
        ),
        Ok(Attested::Direct {
            account_bound: true
        })
    );
}

// --------------------------------------------------------- attest: membership

#[test]
fn attest_membership_lane() {
    let statement = statement(Some("Aatrox.wad.client"));
    let set = StatementSet::from_hashes(vec![statement.token_hash()]).unwrap();
    let trust = trust(vec![("aatrox.wad.client".to_owned(), set.digest())]);
    let evidence = Evidence::Membership {
        set_name: "aatrox.wad.client",
        set: &set,
    };

    assert_eq!(
        trust.attest(&statement, evidence, &context(1_500)),
        Ok(Attested::Membership)
    );

    // A member set whose digest the root does not cover is rejected.
    let forged = StatementSet::from_hashes(vec![[0; 32], statement.token_hash()]).unwrap();
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Membership {
                set_name: "aatrox.wad.client",
                set: &forged,
            },
            &context(1_500),
        ),
        Err(AttestError::SetDigestMismatch)
    );

    // A set name the root does not publish cannot vouch.
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Membership {
                set_name: "zed.wad.client",
                set: &set,
            },
            &context(1_500),
        ),
        Err(AttestError::RootUnknownName)
    );

    // Not in the set: nothing vouches for it.
    let unlisted = other_statement();
    assert_eq!(
        trust.attest(&unlisted, evidence, &context(1_500)),
        Err(AttestError::NotVouched)
    );

    // A direct-only platform has no membership lane at all.
    let direct_only = PlatformTrust::new(platform(2), None).unwrap();
    assert_eq!(
        direct_only.attest(&statement, evidence, &context(1_500)),
        Err(AttestError::RootMissing)
    );
}

#[test]
fn attest_inclusion_lane() {
    let statement = statement(Some("Aatrox.wad.client"));
    let listed_other = other_statement();
    let mut hashes = vec![statement.token_hash(), listed_other.token_hash()];
    hashes.sort();
    let set = StatementSet::from_hashes(hashes).unwrap();
    let trust = trust(vec![("aatrox.wad.client".to_owned(), set.digest())]);
    let proof = set.prove(&statement.token_hash()).unwrap();
    let evidence = Evidence::Inclusion {
        set_name: "aatrox.wad.client",
        proof: &proof,
    };

    assert_eq!(
        trust.attest(&statement, evidence, &context(1_500)),
        Ok(Attested::Membership)
    );

    // The proof binds the exact statement hash: it cannot vouch for another
    // statement, even one the same set also lists.
    assert_eq!(
        trust.attest(&listed_other, evidence, &context(1_500)),
        Err(AttestError::NotVouched)
    );

    // A tampered path proves nothing.
    let mut tampered = proof.clone();
    tampered.path[0][0] ^= 1;
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Inclusion {
                set_name: "aatrox.wad.client",
                proof: &tampered,
            },
            &context(1_500),
        ),
        Err(AttestError::NotVouched)
    );

    // A set name the root does not publish cannot vouch.
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Inclusion {
                set_name: "zed.wad.client",
                proof: &proof,
            },
            &context(1_500),
        ),
        Err(AttestError::RootUnknownName)
    );

    // A direct-only platform has no membership lane at all.
    let direct_only = PlatformTrust::new(platform(2), None).unwrap();
    assert_eq!(
        direct_only.attest(&statement, evidence, &context(1_500)),
        Err(AttestError::RootMissing)
    );
}

fn other_statement() -> Statement {
    Statement::seal(&StatementParams {
        wad_name: Some("Zed.wad.client"),
        wad_toc_digest: None,
        entries: &[],
    })
    .unwrap()
}

// ------------------------------------------------------------- attest: direct

#[test]
fn attest_direct_lane() {
    let statement = statement(Some("Aatrox.wad.client"));
    let trust = trust(Vec::new());
    let attestation = attestation(&key(1), statement.token_hash(), 2_000, None);
    let evidence = Evidence::Direct {
        attestation: &attestation,
    };

    assert_eq!(
        trust.attest(&statement, evidence, &context(1_500)),
        Ok(Attested::Direct {
            account_bound: false
        })
    );

    // Expired attestation is a dead attestation.
    assert_eq!(
        trust.attest(&statement, evidence, &context(2_000)),
        Err(AttestError::Expired)
    );

    // Attestation endorsing a different statement.
    let other = other_statement();
    assert_eq!(
        trust.attest(&other, evidence, &context(1_500)),
        Err(AttestError::StatementMismatch)
    );

    // Attestation signed by a key the platform does not own.
    let foreign = self::attestation(&key(9), statement.token_hash(), 2_000, None);
    assert_eq!(
        trust.attest(
            &statement,
            Evidence::Direct {
                attestation: &foreign,
            },
            &context(1_500),
        ),
        Err(AttestError::Signature(TokenError::SignatureInvalid))
    );
}

#[test]
fn attest_direct_lane_account_binding() {
    let statement = statement(None);
    // The developer platform issues account-bound attestations and has no
    // membership lane.
    let trust = PlatformTrust::new(platform(2), None).unwrap();
    let binding = AccountBinding::new(vec![9; 16], "account-123").unwrap();
    let attestation = attestation(&key(2), statement.token_hash(), 2_000, Some(binding));
    let evidence = Evidence::Direct {
        attestation: &attestation,
    };
    let account_context = |account_id| AttestContext {
        now: 1_500,
        account_id: Some(account_id),
    };

    assert_eq!(
        trust.attest(&statement, evidence, &account_context("account-123")),
        Ok(Attested::Direct {
            account_bound: true
        })
    );

    // Bound to a different account: sharing the mod does nothing.
    assert_eq!(
        trust.attest(&statement, evidence, &account_context("account-456")),
        Err(AttestError::AccountIdMismatch)
    );

    // No account identity available at all.
    assert_eq!(
        trust.attest(&statement, evidence, &context(1_500)),
        Err(AttestError::AccountIdMissing)
    );

    // Expiry is forced renewal.
    assert_eq!(
        trust.attest(
            &statement,
            evidence,
            &AttestContext {
                now: 2_000,
                account_id: Some("account-123"),
            },
        ),
        Err(AttestError::Expired)
    );
}

#[test]
fn attest_is_any_of_across_platforms() {
    let statement = statement(None);
    let attestation = attestation(&key(2), statement.token_hash(), 2_000, None);
    let evidence = Evidence::Direct {
        attestation: &attestation,
    };

    // Platform 1 does not vouch; platform 2 does. Any-of accepts.
    let platforms = [
        trust(Vec::new()),
        PlatformTrust::new(platform(2), None).unwrap(),
    ];
    let attested = platforms
        .iter()
        .find_map(|p| p.attest(&statement, evidence, &context(1_500)).ok());
    assert_eq!(
        attested,
        Some(Attested::Direct {
            account_bound: false
        })
    );
}

// ------------------------------------------------------------------ robustness

#[test]
fn unknown_claims_are_ignored() {
    use coset::{cbor::value::Value, cwt::ClaimsSetBuilder};
    use ltk_sig::io::cose::{claim, seal_bare};

    // A token from a newer producer carrying a claim we do not know yet
    // still parses (forward compatibility).
    let claims = ClaimsSetBuilder::new()
        .private_claim(claim::FILE_ENTRIES, Value::Bytes(Vec::new()))
        .private_claim(-79_999, Value::Text("from the future".to_owned()))
        .build();
    let raw = seal_bare(Statement::TYP, claims).unwrap();
    let parsed = Statement::from_bytes(&raw).unwrap();
    assert!(parsed.entries().is_empty());
}

#[test]
fn negative_timestamps_are_rejected() {
    use coset::{cbor::value::Value, cwt::ClaimsSetBuilder, cwt::Timestamp};
    use ltk_sig::io::cose::{claim, sign_token};

    let claims = ClaimsSetBuilder::new()
        .issued_at(Timestamp::WholeSeconds(-1))
        .private_claim(claim::STATEMENT_HASH, Value::Bytes(vec![3; 32]))
        .build();
    let raw = sign_token(Attestation::TYP, claims, &key(1)).unwrap();
    assert_eq!(
        Attestation::from_bytes(&raw),
        Err(TokenError::ClaimInvalid("timestamp"))
    );
}

#[test]
fn duplicate_statement_entries_are_rejected() {
    // Duplicate name hashes in a statement ("strictly ascending" = unique).
    let duplicate = vec![entries()[0], entries()[0]];
    let result = Statement::seal(&StatementParams {
        wad_name: None,
        wad_toc_digest: None,
        entries: &duplicate,
    });
    assert_eq!(
        result,
        Err(TokenError::ClaimInvalid("entries must be sorted"))
    );
}

#[test]
fn oversized_inputs_are_rejected() {
    use ltk_sig::io::cose::MAX_TOKEN_LEN;
    use ltk_sig::io::statement_set::MAX_SET_LEN;

    let oversized = vec![0u8; MAX_TOKEN_LEN + 1];
    assert_eq!(
        Statement::from_bytes(&oversized),
        Err(TokenError::TooLarge(MAX_TOKEN_LEN + 1))
    );
    assert_eq!(
        Root::from_bytes(&oversized),
        Err(TokenError::TooLarge(MAX_TOKEN_LEN + 1))
    );

    let oversized = vec![0u8; MAX_SET_LEN + 32];
    assert_eq!(
        StatementSet::from_bytes(&oversized),
        Err(StatementSetError::TooLarge(MAX_SET_LEN + 32))
    );
}

// --------------------------------------------------------------- malleability

#[test]
fn non_canonical_cose_encoding_is_rejected() {
    use coset::{CoseSign1, Label, TaggedCborSerializable, cbor::value::Value};

    // Take a validly-signed root and inject junk into the *unprotected*
    // header. The Ed25519 signature covers only the protected header and
    // payload, so it still verifies — but the token bytes, and thus the
    // token hash, now differ. This must be rejected at parse so that token
    // identity stays equal to signed content (no forged equivocation).
    let root = root(&key(1), 5, Vec::new());
    let mut sign1 = CoseSign1::from_tagged_slice(root.as_bytes()).unwrap();
    sign1
        .unprotected
        .rest
        .push((Label::Int(0x4242), Value::Bool(true)));
    let malleated = sign1.to_tagged_vec().unwrap();
    assert_ne!(malleated, root.as_bytes());

    assert_eq!(Root::from_bytes(&malleated), Err(TokenError::NonCanonical));

    // The genuine, canonical root still parses and advances the ratchet, so
    // an attacker serving a re-skin cannot displace or conflict with it.
    let mut trust = trust(Vec::new());
    assert_eq!(
        trust.advance_root(&malleated),
        Err(TrustError::Token(TokenError::NonCanonical))
    );
    assert!(trust.advance_root(root.as_bytes()).unwrap());
    assert_eq!(trust.root().unwrap().time_issued_at(), 5);
}
