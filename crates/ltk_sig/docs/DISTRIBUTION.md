# Distribution

How tokens and set files move between a platform and verifiers: the URL
layout, caching rules, transport trust properties, and what travels inside
mod packages. The token formats and the trust model itself are specified in
[DESIGN.md](DESIGN.md).

## URL layout

Each platform serves everything under its baked `url` prefix. Three paths:

| Path | Object | Mutability |
| ---- | ------ | ---------- |
| `{url}/root` | The current `Root` token | **Mutable** — the platform's only mutable object |
| `{url}/sets/{digest}` | The set file whose Merkle tree hash is `{digest}` | Immutable |
| `{url}/attestations/{sha256}` | The current `Attestation` for the statement whose hash is `{sha256}`, if the platform has one | Mutable — re-issued to renew `exp` |

Conventions:

- `{digest}` and `{sha256}` are 64 lowercase hex characters.
- Response bodies are the raw token / set-file bytes (`application/octet-stream`).
- Reads are anonymous — no authentication, no listing endpoints.
- `404` means "does not exist / nothing fresh", never "revoked": revocation is
  omission from the next root, or an attestation reaching its `exp`.
- `{url}/attestations/…` is only served by platforms with a direct lane
  (`attestations` policy other than `none`); a roots-only platform has no such
  endpoint.

## Transport is not a trust input

Every object is authenticated end-to-end after fetching — roots and
attestations by the platform signature, set files by their content address —
and anything that fails its check is discarded as if the fetch had failed.
Consequences:

- Mirrors, third-party CDNs, and even plain HTTP are acceptable transports.
  TLS is recommended for privacy and availability, but integrity never
  depends on it.
- Serving stale data is harmless by construction: an old root loses to the
  verifier's ratchet (and can never dip below the baked `min_issued_at`
  floor), wrong set bytes fail the digest check, and a missing or stale
  attestation just means the client keeps using what it already holds,
  within its `exp` bound. A hostile or lazy CDN can delay freshness; it can
  never forge or roll back state.

## Caching

Server / CDN:

- `{url}/root`: short TTL (on the order of a minute). Staleness is safe, so
  aggressive CDN caching costs only freshness.
- `{url}/sets/…`: `Cache-Control: public, max-age=31536000, immutable` —
  content-addressed objects never change.
- `{url}/attestations/…`: short TTL, like the root.

Manager (client) disk cache, per platform:

- The freshest accepted root (exact token bytes — the ratchet persists
  across runs and only ever moves forward).
- Set files, keyed by digest hex. Only digests missing from the cache are
  fetched after a root refresh; a client with no new mods fetches the root
  and nothing else.
- Fetched attestations, keyed by statement hash. The manager may re-fetch
  `{url}/attestations/{statement hash}` to renew an expiring direct
  attestation without user action.

Garbage collection mirrors on both sides: the platform deletes set files no
recent root references; the manager prunes cached sets its current roots no
longer name.

## Publishing flow (platform side)

Publishing a new state is a two-step upload with a natural atomic commit
point:

1. Upload any new set files first. They are content-addressed, so uploading
   them before anything references them is invisible to clients.
2. Upload the new `Root` token to `{url}/root` last. The root swap is the
   commit: the moment it lands, clients that fetch it can resolve every
   digest it names.

Roots derive their issue time as `max(now, predecessor + 1)`, so a wrong
publisher clock can never mint two distinct roots at one issue time or
regress supersession. Partitions with no changes keep byte-identical set
files at unchanged URLs, so a publish uploads one small token plus only the
sets that actually changed.

## Tokens in packages

A mod package embeds its **statement**; the pre-verified corpus case
(membership via the baked root and cached sets) therefore works fully
offline. Packages may also embed **attestations** granted at validation
time as a bridge until the platform sweeps the statement into its sets; the
manager can renew these via the attestations endpoint when online.

Where exactly the tokens live inside the `.modpkg` container is part of the
`ltk_modpkg` format specification (pending).
