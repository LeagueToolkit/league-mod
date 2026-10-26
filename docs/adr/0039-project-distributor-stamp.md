# ADR-0039: Project distributor stamp

- **Status:** Proposed
- **Date:** 2026-10-26
- **Crates:** `ltk_mod_project`, `ltk_fantome`
- **Related:** ADR-0040, and the spec sections that state the rule: `docs/design/modpkg.md`
  [section 11.6](../design/modpkg.md#s11.6) and [section 15](../design/modpkg.md#s15)

## Context and problem statement

RuneForge (`runeforge.dev`) inspects an uploaded `.modpkg` or `.fantome` and prefills the mod
post from its metadata. The site matches an upload to an existing post by
`distributor.site_id` and `distributor.mod_id` in the `.modpkg` metadata.

LTK Manager publishes a project to RuneForge with an API token. The first publish creates the
post and returns its ID. Every later upload of the same project carries that ID, or the site asks
the author which post the upload updates.

`ltk_modpkg` defines `DistributorInfo` and the `distributor` metadata key. `ltk_mod_project` packs
every `.modpkg` with `distributor: nil`. A `.fantome` has no distributor key. The `.modpkg` spec
states that a distribution site stamps the distributor when it accepts an upload.

## Decision drivers

- A pack from the CLI and a pack from LTK Manager produce the same distributor.
- An uploaded archive reaches the site unchanged. Players download the bytes the author packed.
- The ID survives a pack, an extraction and a repack.

## Considered options

1. **The mod project carries the stamp.** `mod.config.json` holds `distributor`, and every pack
   writes it into the archive.
2. **The site stamps the archive on upload.** The site rewrites the metadata of each accepted
   archive.
3. **The packer takes the stamp as an option.** LTK Manager stores the ID outside the project and
   passes it to each pack.

## Decision

**A mod project carries an optional `distributor` with the four keys of the `.modpkg`
`distributor`. A pack writes it into the `.modpkg` metadata and into the `Distributor` key of a
`.fantome` `info.json`. An extraction writes it back into the project.** The rules are in
`docs/design/modpkg.md` [section 11.6](../design/modpkg.md#s11.6) and
[section 15](../design/modpkg.md#s15).

An unstamped project serializes without the key, and a config without the key loads with no
stamp.

## Consequences

- **Positive:** the stamp travels with the project source. Any tool that packs the project
  produces an archive the site matches to its post.
- **Positive:** the site never rewrites an upload.
- **Negative:** a fork of a stamped project carries the original post's ID until the author
  removes it, and an upload of the fork names the original post.
- **Negative:** `ModProject` gains a public field, a breaking change for a struct literal
  outside the crate (ADR-0040).
- **Negative:** `Distributor` is a LeagueToolkit extension of `info.json`. A tool that writes a
  `Distributor` key of another shape makes the `info.json` fail to parse.
- **Revisit when:** a second distribution site needs a stamp of its own on one project.

## Pros and cons of the options

### The mod project carries the stamp

- Good: one source of truth, readable and editable in `mod.config.json`.
- Good: the CLI, LTK Manager and any other packer agree without coordination.
- Bad: the project holds one distributor. A mod published on two sites keeps one stamp.

### The site stamps the archive on upload

- Good: no change to the mod project format.
- Bad: the site rewrites a `.modpkg` metadata chunk or a `.fantome` `info.json`, and the
  archive players download differs from the archive the author packed.
- Bad: the author's project never learns the ID. The next upload carries no stamp and the site
  asks which post it updates.

### The packer takes the stamp as an option

- Good: no change to the mod project format.
- Bad: LTK Manager keeps the ID in its own store. A pack from the CLI omits the stamp.
- Bad: the ID lives in two places, the manager store and each packed archive, and an
  extraction has nowhere to write it.
