# Signature Format Design

## Purpose

This document describes the token formats used by `ltk_sig`, how statements are attested and verified, and the trust model behind them.

The model is **platform-attested and default-deny**: a mod's files are accepted because a trusted platform currently vouches for them, not because nobody has denounced them. It rests on three concepts:

- **Platform** — a trust root: one Ed25519 public key plus an update URL and an enforced lane policy, baked into the manager. A platform vouches for content by signing with that single key. One platform, one key; new roles (developer signing, upload validation, cross-signing) are new platform entries, not key hierarchies.
- **Statement** — the content: which files a mod provides, with checksums and digests, optionally bound to a WAD name and TOC. Statements are **unsigned** and identified purely by the SHA-256 of their bytes; a statement says nothing about whether it is trusted.
- **Attestation** — the platform's proof that it validated a statement. Either **direct** (a platform-signed token naming the statement's hash, optionally bound to one account) or **membership** (the statement's hash is included in a set covered by the platform's currently signed root).

Every trust decision reduces to one question: *does a baked platform key vouch for this statement hash, right now?* There are no certificates, no key chains, and no delegation. Recency comes from the platform periodically re-signing its root, floored by the minimum issue time baked into the app at build time.

The overarching design goal is to **never get painted into a corner**: every axis that can grow — attestation methods, proof formats, platforms, subjects, claims — has a named extension point (see [Extensibility](#extensibility)).

## Scope

The design covers:

- The two token envelopes: signed (COSE/CWT) and bare (unsigned, hash-identified)
- `Statement` (`ltk/statement`) — the content token
- `Attestation` (`ltk/attestation`) — the platform's direct endorsement of one statement
- `Root` (`ltk/root`) — the platform's re-signed snapshot of its published sets
- `PlatformSet` (`ltk/platform-set`) — the baked trust configuration: keys, floors, and lane policies
- Set files and their content-addressed distribution
- The attestation rule, root freshness, multi-platform trust, and rollout phases

The URL layout, caching, and transport properties of token distribution are specified separately in [DISTRIBUTION.md](DISTRIBUTION.md). Riot WAD reading and RSA TOC signature verification are handled entirely by the [`ltk_wad`](https://github.com/LeagueToolkit/league-toolkit) crate and are out of scope here.

Developer certificates and cross-signing are **deliberately out of scope** for now; the extension points that leave room for them are noted where relevant.

## Platforms

The verifier ships with a baked **platform set**: authored as a JSON template, converted to CBOR by the manager's build script, and embedded with `include_bytes!` (see the `PlatformSet` token below). Each entry is:

| Field | Use |
| ----- | --- |
| `name` | UI attribution, log routing, and the lookup key within the set; never a trust input |
| `pub_key` | The platform's Ed25519 public key — the only key the platform has |
| `url` | URL prefix for fetching the current root and set files |
| `min_issued_at` | Freshness floor: roots issued before this time are never accepted; refreshed at each release (see Root freshness) |
| `accepts` | **Enforced lane policy**: whether roots are accepted, and whether direct attestations are (`none`, `account-bound`, or `any`) |

The manager may additionally bake the newest known `Root` per platform for offline first-run membership; any root, baked or fetched, must satisfy the floor.

The lane policy is what scopes each key, and verifiers enforce it — authority a platform was never granted is refused even under a valid signature:

- The **corpus platform** (`roots: true, attestations: none`) key can live offline: it only re-signs roots (a batch operation at whatever cadence its operator chooses), so its exposure is a ceremony, not a service.
- The **developer platform** (`roots: false, attestations: account-bound`) key is warm — it signs on demand after login — but a stolen copy can mint nothing that works beyond a single account: verifiers refuse roots and unbound attestations from it outright.
- A future **upload-validation platform** (`roots: false, attestations: any`) key is likewise warm and should keep its attestations short-lived (`exp`), with validated statements swept into a root by the corpus platform.

Adding, removing, or **rotating the key of** a platform is a manager release. The manager releases monthly, so this cadence is the recovery bound for a platform-key compromise. That is the deliberate trade for having no certificate machinery — and the lane policy keeps the blast radius of each key to exactly the lane it serves, while the trust rule stays a single signature check.

If platform onboarding or rotation ever needs to outpace releases, the same indirection recurses: the platform set, signed, becomes a *platform registry token* (future work, purely additive).

## Token Envelopes

### Signed tokens

`Attestation` and `Root` are tagged `COSE_Sign1` structures (RFC 9052, CBOR tag 18) carrying a CWT claims set (RFC 8392) as payload:

- **Protected header**:
  - `alg` (1) = Ed25519 (`-19`, fully-specified EdDSA over edwards25519)
  - `typ` (16, RFC 9596) = token type string, used for domain separation — a token parsed as the wrong type is rejected before any claim is read
  - `kid` (4) = the signing platform's raw Ed25519 public key (32 bytes); a routing hint only, never a trust input — acceptance always requires signature verification against a baked platform key
- **Payload**: CWT claims map (see claim registry below)
- **Signature**: Ed25519 over the COSE `Sig_structure`, covering the protected header and payload bytes exactly as serialized. Verification uses `verify_strict` (rejects small-order/mixed-order keys)

### Bare tokens

`Statement` is a **bare token**: a CBOR array `[typ (tstr), claims (bstr)]` where the claims are a CWT claims map. Bare tokens carry no signature — a statement is pure content, and a signature would confer nothing (trust only ever comes from a platform attesting the statement's hash). This also means mod packing needs no keys at all.

### Identity and canonical form

Both envelopes share the identity rule: tokens are **immutable** once parsed or built — they retain their exact wire bytes and re-serialize byte-for-byte. Token identity is `SHA-256(serialized token bytes)`, which therefore endorses the *exact bytes*.

For this identity to be sound, identity must equal content, so parsing rejects any non-canonical token: signed tokens must have an empty unprotected header, and every token must round-trip byte-for-byte through a deterministic re-encoding. This closes the malleability gap left by the signature covering only the `Sig_structure` — without it, an attacker could re-encode a validly-signed token to a different hash (same claims, same signature) and, for example, forge equivocation `Conflict`s (see Root freshness) by serving a re-skinned copy of the current root. Every distinct accepted byte string is a distinct token.

**Forward compatibility rule:** each token type reads only its own claims and ignores everything else, unknown or otherwise. This lets old verifiers coexist with newer token producers.

### Claim registry

Standard CWT claims:

| Claim | Label | Use |
| ----- | ----- | --- |
| `exp` | 4 | Expiry of an `Attestation` (Unix epoch seconds), strictly after `iat`; mandatory on attestations. The only claim compared to a local clock. (See Expiry and clocks.) |
| `iat` | 6 | Issue time (Unix epoch seconds). On a `Root` this is the **supersession value** — newest issued wins, derived by the signer as `max(now, predecessor + 1)`, never compared to a verifier clock. Informational on attestations. |

LTK private-use claims (RFC 8392 reserves labels below -65536; never reuse a label for a different meaning, even across token types — allocate the next free one):

| Claim | Label | Type | Use |
| ----- | ----- | ---- | --- |
| `file_entries` | -70001 | bstr(n×56) | Packed file entries sorted by name hash |
| `wad_name` | -70002 | tstr | Optional: name of the WAD the statement's files belong to (e.g. `Aatrox.wad.client`; non-empty ASCII, < 128 bytes) |
| `wad_toc_digest` | -70003 | bstr(32) | Optional: SHA-256 of the TOC of the WAD build the statement targets — pins the validation to one exact game build. Canonical derivation: for every TOC chunk in order, `path_hash` (u64 LE) ‖ `checksum` (u64 LE) |
| `statement_hash` | -70004 | bstr(32) | SHA-256 of the statement token an attestation endorses |
| `account_id_salt` | -70005 | bstr(8..=64) | Salt for the account binding of an attestation; present iff `account_id_hash` is |
| `account_id_hash` | -70006 | bstr(32) | `SHA-256(salt ‖ account_id)` binding an attestation to one account; present iff `account_id_salt` is |
| `roots` | -70007 | map | `set name (tstr) → digest (bstr(32))` of the platform's currently published sets, strictly ascending by name |
| `platforms` | -70008 | array | Platform entries of the baked platform set, strictly ascending by name (see `PlatformSet`) |
| `bundle_roots` | -70009 | array of bstr | Serialized `Root` tokens carried by a `Bundle` |
| `bundle_sets` | -70010 | array of bstr | Serialized set files carried by a `Bundle` |
| `bundle_statements` | -70011 | array of bstr | Serialized `Statement` tokens carried by a `Bundle` (-70012 and -70013 are reserved for bundled attestations and inclusion proofs) |

Bulk data is carried as packed byte strings rather than CBOR arrays: fixed stride, no per-item CBOR overhead, and sortedness (strict ascending, enforced at signing and parse time) makes encodings unambiguous. The `roots` map is likewise required to have strictly ascending keys at both ends. Serialized tokens and set files are capped at 16 MiB.

## Token Types

### `Statement` (`ltk/statement`)

The content token: a set of files, with two-tier checksums, optionally bound to the WAD they belong to.

- Envelope: bare (unsigned); identity is the SHA-256 of the exact bytes
- Claims: `file_entries`, optional `wad_name`, optional `wad_toc_digest`

Entry layout (little-endian, 56 bytes):

| Offset | Type | Field |
| ------ | ---- | ----- |
| 0 | u64 | `name_hash` (xxh64 of file name) |
| 8 | u64 | `checksum_compressed` (xxh3 of compressed data as stored in the WAD) |
| 16 | u64 | `checksum_uncompressed` (xxh3 of uncompressed data) |
| 24 | [u8; 32] | `digest_decompressed` (SHA-256 of the decompressed file content) |

**Two-tier content verification.** The xxh3 checksums mirror the WAD TOC and are what the runtime check compares against; they are fast but **not collision resistant**. `digest_decompressed` is the cryptographic binding: at install/overlay-build time the manager must hash each decompressed chunk and match it against the listed digest. After that, the runtime check only needs to detect accidental divergence, which xxh3 is fit for.

The optional WAD bindings scope the statement: `wad_name` says which WAD the files patch, and `wad_toc_digest` pins the exact original WAD build the files were validated against, letting platforms version validations and tooling detect game-update drift. Both are advisory to the attestation rule itself — attestation binds the statement bytes wholesale.

Statements are distributed embedded in the mod package, so the simple case (pre-verified mod, offline machine) is fully self-contained.

### `Attestation` (`ltk/attestation`)

The platform's direct endorsement of one statement: "I validated the exact statement whose hash this is."

- Signer (`kid`): the platform key
- Claims: `iat`, `exp`, `statement_hash`, optional `account_id_salt` + `account_id_hash`

Direct attestations make a validation usable *immediately* — a fresh upload, a developer's own build — with zero format changes as new validation schemes appear. Two containment claims govern them:

- **`exp`** — a signed attestation cannot be revoked short of rotating the platform key (a release), so the expiry is mandatory: an attestation without one would be irrevocable. The platform's control point is declining to re-issue; durable validity is the membership lane's job.
- **Account binding** — `account_id_hash = SHA-256(salt ‖ account_id)` with a per-attestation random salt (so attestations don't become a rainbow table of account identifiers). The manager knows the logged-in account and recomputes the hash; a mismatch fails verification. An account-bound mod therefore only works on that account — sharing it does nothing, which is the real abuse containment for developer-signed mods.

### `Root` (`ltk/root`)

The platform's re-signed snapshot: "as of now, these are the digests of my published sets."

- Signer (`kid`): the platform key
- Claims: `iat` (supersession value), `roots` (set name → digest)

The root carries the **only signature in the membership lane**: one root signature endorses every statement in every set it covers. Inclusion evidence — a whole set file or a Merkle audit path — is unsigned data, authenticated purely by connecting a statement hash to a digest the root signs. Adding a thousand statements to the corpus costs one re-signed root, and the verifier checks one signature per root refresh, not one per mod.

Set names are **opaque partition labels** chosen by the platform — per-WAD lists, one big `corpus` set. They carry no trust semantics; partitioning exists so caches and fetches stay small. A platform may publish any number of sets under one root.

A digest is the **Merkle tree hash** of the set (see Set Files), so the one value anchors both membership forms: the manager checks the whole set file it fetched, and a verifier holding only a compact inclusion proof checks the audit path — no format change, no second digest.

Roots supersede each other by issue time and are re-signed at whatever cadence the platform chooses — publishing is cheap by construction (sign one small token, upload changed set files). Nothing forces a schedule; the floor baked into each manager release (below) is the only recency obligation.

### `PlatformSet` (`ltk/platform-set`)

The baked trust configuration itself: every trust decision starts here.

- Envelope: bare (unsigned) — the set is embedded in the binary, not transported, so signing it would be circular. Its signed twin is the future platform registry token.
- Claims: `platforms` — an array of entry maps, strictly ascending by name

Entry map layout (integer keys; unknown keys are ignored):

| Key | Type | Field |
| --- | ---- | ----- |
| 1 | tstr | `name` |
| 2 | bstr(32) | `pub_key` |
| 3 | tstr | `url` |
| 4 | uint | `min_issued_at` |
| 5 | bool | `accepts_roots` |
| 6 | tstr | `attestations` — `none`, `account-bound`, or `any`. Unknown values are rejected: this is baked configuration consumed by same-version code, not a wire format that needs forward compatibility |

The set is authored as a JSON template, kept reviewable in the repository:

```json
{
  "platforms": [
    {
      "name": "corpus",
      "pub_key": "<64 hex chars>",
      "url": "https://example.com/ltk",
      "min_issued_at": 1767225600,
      "accepts": { "roots": true, "attestations": "none" }
    }
  ]
}
```

and converted to canonical CBOR at build time (`PlatformSet::from_json`, behind the `json` feature — intended for build scripts, which then write the sealed bytes for `include_bytes!`).

## Set Files

A set file is the sorted, unique 32-byte SHA-256 hashes of every statement the platform currently endorses in that partition (`n×32` bytes, no header). Its **digest** — the value the root's `roots` map carries — is the RFC 6962 Merkle tree hash over those hashes as ordered leaf inputs:

- `leaf = SHA-256(0x00 ‖ statement hash)`, `node = SHA-256(0x01 ‖ left ‖ right)` (domain-separated so leaves and nodes can never be confused)
- subtrees split at the largest power of two smaller than the leaf count; the empty set hashes to `SHA-256("")`

The encoding is canonical (packed, sorted, unique), so the digest pins the exact file bytes just as a flat hash would — and it additionally anchors **inclusion proofs**: the RFC 6962 audit path from one statement hash to the digest (`leaf_index (u64 LE) ‖ tree_size (u64 LE) ‖ path (n×32)`, verified per RFC 9162 §2.1.3.2). A proof is `32·⌈log₂ n⌉` bytes regardless of set size, so the overlay builder can extract one per used statement and embed it for a verifier that holds no set files (see Runtime). Proofs are pinned to the root snapshot they were extracted under and are re-extracted on rebuild; distribution itself always ships whole set files — the manager needs them anyway, both to know what is endorsed and because proofs can only be extracted from a full set.

Distribution is **content-addressed**: a set file lives at `{url}/sets/{hex digest}` — the digest from the root's `roots` map is both the authenticator (parse, recompute the tree hash, compare) and the URL. The full URL layout, caching rules, and transport trust properties are specified in [DISTRIBUTION.md](DISTRIBUTION.md). Consequences:

- Every published object is immutable: no cache invalidation anywhere, CDN- and mirror-friendly.
- Frequent root re-signing costs almost nothing: partitions with no changes keep byte-identical files at unchanged URLs, so each root uploads one small token plus only the sets that actually changed.
- Clients skip fetches for every digest they already have cached; a client with no new mods fetches the root and nothing else.
- Housekeeping is garbage collection of set files no recent root references.

## Attestation Rule

A statement is **attested** with respect to platform `P` when either lane binds it:

- **Direct**: an `Attestation` verifies under `P`'s key, its `statement_hash` equals the statement's hash, it is unexpired, and its account binding (when present) matches the currently logged-in account.
- **Membership**: `P`'s current accepted root maps some offered set name to a digest, and the offered evidence connects the statement's hash to that digest — either the whole set file (it matches the digest and contains the hash) or an inclusion proof (the audit path recomputes to the digest). No per-statement signature exists or is checked — the root's signature covers the whole set, and hash membership endorses the exact statement bytes.

Across platforms the rule is **any-of**: one trusted platform vouching suffices. Ecosystem trust is bounded by the laxest trusted platform; the remedies are curation of the baked set and fast removal.

The lanes complement each other in time: the direct lane makes a validation usable immediately and is bounded by expiry and binding; the membership lane is the durable destination, carrying statements clock-free once the platform sweeps them into its sets.

### Root freshness

The manager tracks one ratchet per platform: the newest accepted root, ordered by issue time (`iat`). A candidate root is accepted only if its signature verifies under the platform key and it does not lower the ratchet — `max(baked bootstrap, cached, freshly fetched)`. Stale candidates are ignored rather than rejected, so offering tokens in any order converges on the max, while a *distinct* root at an equal issue time is an equivocation error (honest signers never mint two, by the `max(now, predecessor + 1)` rule — which also means a wrong signing clock can delay debuggability but never regress supersession).

The platform's baked `min_issued_at` is the floor the user-visible guarantee comes from: **an accepted root is never older than the floor embedded in the app** — a root below it is ignored no matter how validly signed. Each manager release refreshes the floors (e.g. to the newest root observed at build time), so they advance with the release cadence with no extra operational machinery; a baked bootstrap root additionally makes a fresh install work offline, and no install can be rolled back below its build. A platform that stops re-signing simply stops progressing — its last accepted sets keep working.

Issue times are supersession values that happen to be human-readable (any root's ratchet position is a date you can correlate with logs), not clock inputs: verifiers only compare them to the best already seen, never to their own clock.

Direct attestations are deliberately **not** subject to the floor — they are durable endorsements, not snapshots, and flooring them would silently void every issued attestation each release. Their containment is `exp`, the account binding, and ultimately key rotation.

The residual exposure is a verifier that has not been online since before a key compromise: it will honor the compromised platform's tokens until a release rotates the key. The manager refreshing roots whenever it is online, plus `exp` on hot lanes, is the practical mitigation.

### Expiry and clocks

Durable validity comes from membership and the root ratchet, never from clocks. `exp` exists for one purpose: bounding direct-attestation authority on verifiers that cannot see a key rotation yet. It is compared against the verifier's local clock, which is acceptable here where it was not acceptable for durable validity: the check needs days-granularity, not minutes; a machine that plays online has a roughly-correct clock (TLS to the game's servers fails otherwise); and the lane it guards is already contained by the account binding. Verifiers must not check `iat` against the clock (skew would cause false rejections and it adds nothing).

### Manager (install / overlay build)

On each overlay build the manager curates runtime state:

1. Refresh each platform's root (if online) and any set files needed for the installed mods.
2. For each installed mod's statement: apply the attestation rule with whatever evidence the package and platform provide (a direct attestation, or a set name plus set file), and verify `digest_decompressed` against the actual decompressed chunks being written into the overlay.
3. Emit a local **curated table** per WAD: the flattened, sorted file entries from every statement that attested, regardless of lane.

The curated tables are local manager-produced state, trusted exactly as much as the overlay files themselves (same producer, same directory).

### Runtime

The runtime verifier consumes only the curated tables: for each overlaid file selected by the property-bins in play, an exact `(name_hash, checksum, digest)` membership check — one mmap and a binary search. No COSE, CBOR, or signature code runs game-side.

Curated tables are the floor, not the ceiling: because set digests anchor inclusion proofs, the overlay build can instead embed the root token, the used statements, and one proof per statement — evidence whose size scales with installed mods, not corpus size. A runtime that verifies that chain end-to-end from its own baked platform set trusts nothing the manager wrote: one Ed25519 verification per root plus `⌈log₂ n⌉` SHA-256s per statement, once per launch. Such a runtime carries its own `min_issued_at` floor (refreshed per release, like the manager's), which bounds the one move a local user has — replaying an older root and proofs to resurrect a since-revoked statement.

To make local development easier, the runtime verifier is expected to skip checking when any of these hold:

1. Playing replays: first process argument ends with `.rofl`
2. Playing spectator streams: first process argument starts with `spectator`
3. Playing on PBE: arguments contain `-PlatformID=PBE1` **and** `-Region=PBE`

> **Note:** PBE arguments may be spoofed through launcher modifications (e.g., Pengu Loader). It is expected that Riot would introduce game-side checks to ensure the correct region is passed to the game if this becomes an issue.

## Revocation and Compromise Recovery

| Event | Action | Takes effect |
| ----- | ------ | ------------ |
| Bad statement discovered | Omit its hash from the sets under the next root | Next root + client refresh |
| Broken checker/scanner version | Omit everything it approved (platform-side bookkeeping); stop accepting its evidence | Next root |
| Bad direct attestation issued | Contained by `exp` and any account binding; platform declines re-issue | ≤ `exp` |
| Developer account abused | Its attestations only work on that account; platform declines renewal | Bounded by construction |
| Platform key compromised | Rotate the key in a manager release (which also re-bakes the floor) | Next release |
| Platform compromised/distrusted | Remove its entry from the baked set | Next release |

A stolen platform key holds until a release ships — but only the authority its lane policy grants, because verifiers refuse the rest: the key that can mint durable membership (the corpus platform) can be kept offline and only touched to re-sign roots, while keys that must be warm can only mint expiring, account-bound authority no matter what an attacker signs with them.

## Rollout Phases

1. **Corpus launch.** The pre-verified corpus (~3.3k mods) is sealed as statements; set files and the first root are signed in an offline ceremony with the corpus platform key; the bootstrap root is baked into the manager. Zero online attack surface — the key returns to storage until the next re-signing.
2. **Upload validation.** A warm-keyed platform entry validates uploads and issues short-`exp` direct attestations immediately; the corpus platform periodically sweeps validated statement hashes into its sets. Formats unchanged.
3. **Developer platform.** A dedicated platform entry issues account-bound, expiring attestations to logged-in developers for their own builds. Their mods attest on their own account with one platform round-trip per build. Developer *certificates* (offline-capable developer signing) and cross-signing between platforms remain future work — see Extensibility.

## Extensibility

The corners we refuse to paint ourselves into, and the escape hatch for each:

- **New validation scheme or role** → a new platform entry with its own key and lane policy; the attestation rule is unchanged.
- **New membership proof format** → a new digest interpretation under the same `Root` token; verifier evidence grows a variant, existing formats coexist. (Exercised once already: inclusion proofs verify against the same Merkle digest the whole-set check uses.)
- **Developer certificates / cross-signing** → future token types under new `typ` strings that feed the same rule (more ways to bind a statement hash to a platform); deliberately skipped for now.
- **New subject kinds** → statements already carry their own scoping (`wad_name`, `wad_toc_digest`, both optional) and set names are opaque, so other subjects reuse everything.
- **Scoping a platform to certain content** → an additive per-entry constraint (e.g. an allowlist of `wad_name`s the platform may vouch for); the platform entry grows a key, formats unchanged.
- **New token type** → pick a new `typ` under `ltk/`, allocate claims, reuse an envelope (`cose::sign_token` / `cose::seal_bare`).
- **Platform onboarding faster than releases** → the platform set, signed, becomes a registry token (recurses the same trust structure).
- **Claims** → unknown claims are ignored; labels are never reused; the next free label in the -70000 block is allocated in `io/cose.rs::claim`.

## Security and Validation Notes

- Statements are unsigned by design: never trust one except through the attestation rule — the attested hash covers the exact bytes. For signed tokens, always verify the signature before trusting claims; parsing alone (`from_bytes`) validates structure, not authenticity.
- Ed25519 verification is strict (`verify_strict`); `kid` is a routing hint, never a trust input — verification is always against a baked platform key.
- Sorted arrays and uniqueness constraints are enforced at both signing and parse time to avoid ambiguous encodings; tokens round-trip byte-for-byte; hashes are computed over exact received bytes, never over a re-serialization.
- Root supersession only moves forward, floored by the baked bootstrap; issue times are never compared to a local clock; `exp` is, deliberately, as a direct-lane backstop only.
- At install/overlay-build time, verify `digest_decompressed` against actual content; xxh3 checksums are not a security boundary.
- The only trust inputs are the baked platform set (keys, floors, lane policies), optional baked roots, token signatures, and set-file digests. Names — platform names, set names, WAD names — are routing and attribution, never trust inputs.
