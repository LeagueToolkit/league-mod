# ltk_sig

Signing and verification of League Toolkit mod packages.

The trust model is **platform-attested and default-deny**, built from three
concepts:

- **Platforms** — trust roots: one Ed25519 public key plus an update URL,
  baked into the manager. One platform, one key; new roles (developer
  signing, upload validation, cross-signing) are new platform entries, not
  key hierarchies.
- **Statements** — the content: which files a mod provides, with fast
  checksums and binding digests, optionally bound to a WAD name and TOC.
  Unsigned, identified purely by the SHA-256 of their bytes, and
  authority-neutral on their own.
- **Attestations** — the platform's proof that it validated a statement:
  **direct** (a platform-signed token naming the statement's hash,
  optionally bound to one account and expiring) or **membership** (the
  statement's hash is included in a set covered by the platform's currently
  signed root).

Every trust decision reduces to "does a baked platform key vouch for this
statement hash, right now?" — no certificates, no chains, no delegation.

Signed tokens are `COSE_Sign1`/CWT structures signed with Ed25519 — faster
than RSA in practice and far more compact (64-byte signatures, 32-byte keys).
Riot WAD TOCs keep their native RSA PKCS#1 v1.5 verification, handled by the
[`ltk_wad`](https://crates.io/crates/ltk_wad) crate.

## Anti-rollback

Durable validity never comes from clocks: roots supersede each other by
issue time, compared only against the best already seen, and the bootstrap
root baked into the manager at build time floors the ratchet — an accepted
root is never older than the one embedded in the app, and each release
advances the floor. Attestation expiry exists solely as a direct-lane
backstop (and as forced renewal for account-bound attestations), never as
the source of durable validity.

## Contents

- `io::cose` — shared token envelopes: signed COSE_Sign1/CWT (sign, parse,
  verify) and bare unsigned tokens (seal, parse)
- `io::platform_set` — the baked platform set: keys, URLs, freshness floors,
  and enforced lane policies (JSON template → CBOR via the `json` feature)
- `io::statement` — the content token, with two-tier checksums (fast xxh3 +
  binding SHA-256) and optional WAD bindings
- `io::attestation` — the platform's direct endorsement of one statement,
  with mandatory expiry and optional account binding
- `io::root` — the platform's re-signed snapshot of its published set digests
- `io::statement_set` — content-addressed membership set files, identified
  by their RFC 6962 Merkle tree hash, with compact inclusion proofs for
  verifiers that hold no set files
- `trust` — per-platform verifier state (root ratchet) and the attestation
  rule
- `verify` — the verification context: attest statements once, answer
  per-file queries from a lookup table
- `base_skin` (feature `base-skin`) — overlay diagnostics: verify a
  champion's base skin in a merged WAD against the original, classifying
  modified references as attested or not

See [docs/DESIGN.md](docs/DESIGN.md) for the full wire formats, claim
registry, and trust model, and [docs/DISTRIBUTION.md](docs/DISTRIBUTION.md)
for the URL layout, caching, and transport properties.
