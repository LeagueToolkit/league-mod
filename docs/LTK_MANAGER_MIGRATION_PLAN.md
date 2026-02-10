# ltk-manager Repository Migration Plan

This document outlines the plan to move `crates/ltk-manager` from the `league-mod` monorepo into its own dedicated repository.

## Motivation

- ltk-manager accounts for ~97% of repo size (mostly `node_modules` and frontend tooling)
- It has completely independent frontend tooling (pnpm, Vite, React, Tailwind, ESLint, Husky)
- The CI already conditionally includes/excludes it with path-based change detection
- It already has a dedicated release workflow (`release-ltk-manager.yml`)
- Contributors working on library crates or the CLI don't need 300MB+ of frontend dependencies

## Current State

### Workspace dependencies used by ltk-manager

| Crate | Version | Published to crates.io | Used by ltk-manager |
|-------|---------|----------------------|---------------------|
| `ltk_modpkg` | 0.1.5 | Yes | Yes |
| `ltk_mod_project` | 0.1.4 | Yes | Yes |
| `ltk_mod_core` | 0.1.0 | **No** | Yes |
| `ltk_fantome` | 0.1.4 | Yes | Yes |
| `ltk_overlay` | 0.1.0 | **No** | Yes |
| `ltk_pki` | — | Yes | No |

### CI/CD files affected

| File | Action needed |
|------|--------------|
| `.github/workflows/ci.yml` | Remove all ltk-manager conditional logic |
| `.github/workflows/release-ltk-manager.yml` | Move to new repo |
| `.github/workflows/release-plz.yml` | Remove ltk-manager `[[package]]` entry |
| `.github/workflows/publish-winget.yml` | Move to new repo if ltk-manager-specific |
| `release-plz.toml` | Remove ltk-manager entry |
| `deny.toml` | No change (stays in monorepo) |

---

## Prerequisites (Do Before Migration)

### 1. Publish `ltk_mod_core` to crates.io

Currently at v0.1.0 and **not published**. ltk-manager depends on it.

- [ ] Add `ltk_mod_core` to `release-plz.toml` with `publish = true`
- [ ] Review its public API for crates.io readiness (docs, metadata in `Cargo.toml`)
- [ ] Add `description`, `license`, `repository`, `keywords` fields if missing
- [ ] Publish initial version

### 2. Publish `ltk_overlay` to crates.io

Currently at v0.1.0 and **not published**. ltk-manager depends on it.

- [ ] Add `ltk_overlay` to `release-plz.toml` with `publish = true`
- [ ] Ensure its workspace dependencies (`ltk_mod_project`, `ltk_modpkg`) use crates.io versions, not path deps
- [ ] Review public API for crates.io readiness
- [ ] Publish initial version

### 3. Stabilize library crate APIs

The migration creates a multi-repo coordination cost for breaking changes. Before splitting:

- [ ] Audit the API surface of all 5 library crates that ltk-manager consumes
- [ ] Identify any planned breaking changes and land them first
- [ ] Consider whether a `0.2.0` milestone makes sense before the split

---

## Migration Steps

### Phase 1: Create the new repository

1. **Create `LeagueToolkit/ltk-manager`** repository on GitHub
2. **Initialize with the contents of `crates/ltk-manager/`** as the repo root:
   ```
   ltk-manager/          (new repo root)
   ├── src-tauri/        (Rust backend, becomes a standalone crate)
   ├── src/              (React frontend)
   ├── docs/
   ├── package.json
   ├── pnpm-lock.yaml
   ├── tsconfig.json
   ├── vite.config.ts
   ├── .husky/
   └── ...
   ```
3. **Preserve git history** using `git filter-repo` or `git subtree split`:
   ```bash
   # From league-mod repo
   git subtree split --prefix=crates/ltk-manager -b ltk-manager-split

   # In new repo
   git pull ../league-mod ltk-manager-split
   ```

### Phase 2: Update Rust dependencies

Replace path dependencies in `src-tauri/Cargo.toml` with crates.io versions:

```toml
# Before (path deps)
ltk_modpkg = { path = "../../ltk_modpkg", features = ["project"] }
ltk_mod_project = { path = "../../ltk_mod_project" }
ltk_mod_core = { path = "../../ltk_mod_core" }
ltk_fantome = { path = "../../ltk_fantome" }
ltk_overlay = { path = "../../ltk_overlay" }

# After (crates.io)
ltk_modpkg = { version = "0.1", features = ["project"] }
ltk_mod_project = { version = "0.1" }
ltk_mod_core = { version = "0.1" }
ltk_fantome = { version = "0.1" }
ltk_overlay = { version = "0.1" }
```

Update `repository` field:
```toml
repository = "https://github.com/LeagueToolkit/ltk-manager"
```

### Phase 3: Move CI/CD

**Move to new repo:**
- [ ] `release-ltk-manager.yml` — update `projectPath` from `crates/ltk-manager` to `.`
- [ ] `publish-winget.yml` — if ltk-manager-specific

**Create new CI workflow** for the standalone repo (simplified, no change detection needed):
```yaml
name: CI
on:
  pull_request:
    branches: [main]
  push:
    branches: [main]

jobs:
  check-frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/setup-node@v4
        with: { node-version: "22" }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - run: pnpm install --frozen-lockfile
      - run: pnpm check   # typecheck + lint + format

  check-rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/setup-node@v4
        with: { node-version: "22" }
      - uses: pnpm/action-setup@v4
        with: { version: 9 }
      - run: pnpm install --frozen-lockfile && pnpm build
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy, rustfmt }
      - uses: Swatinem/rust-cache@v2
      - run: cargo check --manifest-path src-tauri/Cargo.toml
      - run: cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
      - run: cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
      - run: cargo test --manifest-path src-tauri/Cargo.toml
```

### Phase 4: Clean up the monorepo

In `league-mod`:

- [ ] Remove `crates/ltk-manager/` directory
- [ ] Remove `"crates/ltk-manager/src-tauri"` from workspace `members` in root `Cargo.toml`
- [ ] Remove `[[package]] name = "ltk-manager"` from `release-plz.toml`
- [ ] Delete `.github/workflows/release-ltk-manager.yml`
- [ ] Simplify `.github/workflows/ci.yml`:
  - Remove `detect-changes` job
  - Remove all `if: needs.detect-changes.outputs.ltk-manager == 'true'` conditionals
  - Remove Node.js/pnpm setup steps
  - Remove `--exclude ltk-manager` fallbacks
  - Remove Tauri-specific Linux deps (`libwebkit2gtk`, `libgtk-3`, etc.)
- [ ] Remove `.husky/` directory from workspace root if it was only for ltk-manager
- [ ] Update root `CLAUDE.md` to remove ltk-manager references (or link to new repo)

### Phase 5: Update documentation and links

- [ ] Update new repo's `CLAUDE.md` to be self-contained (no `../../CLAUDE.md` reference)
- [ ] Update new repo's `Cargo.toml` metadata (repository URL, homepage)
- [ ] Update new repo's `tauri.conf.json` if it references the old repo
- [ ] Add a note in the monorepo README pointing to the new repo
- [ ] Update any GitHub issue templates, labels, or project boards
- [ ] Transfer open ltk-manager issues to the new repo (via GitHub issue transfer)

---

## Post-Migration Workflow

### Coordinating library crate updates

When a library crate (e.g., `ltk_overlay`) releases a new version:

1. New version is published to crates.io via `release-plz` in the monorepo
2. Dependabot or Renovate in `ltk-manager` repo picks up the update
3. PR is created automatically to bump the dependency version
4. CI validates the update, team reviews and merges

**Recommended:** Enable Renovate or Dependabot in the new repo for automated dependency updates.

### Local development with unreleased library changes

For developing against unpublished changes in a library crate, use Cargo's path override:

```toml
# .cargo/config.toml (gitignored in ltk-manager repo)
[patch.crates-io]
ltk_overlay = { path = "../league-mod/crates/ltk_overlay" }
ltk_modpkg = { path = "../league-mod/crates/ltk_modpkg" }
```

This lets you iterate locally without publishing, then remove the override before committing.

---

## Rollback Plan

If issues arise during migration:

1. The monorepo still has the full history — nothing is deleted until Phase 4
2. Phase 4 cleanup can be reverted with a single `git revert`
3. The new repo can switch back to git path dependencies temporarily if crates.io versions are problematic

---

## Decision: When to Pull the Trigger

Migrate when **all** of these are true:

- [ ] `ltk_mod_core` and `ltk_overlay` are published to crates.io
- [ ] No breaking changes to library crate APIs are planned in the near term
- [ ] The team agrees on the coordination workflow (Renovate/Dependabot + patch overrides)
