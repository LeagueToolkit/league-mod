# Winget Distribution Setup for League Mod

This directory contains the winget manifest files for distributing `league-mod` via Windows Package Manager.

## Prerequisites

Before submitting to winget, ensure you have:

1. ✅ **GitHub repository with proper releases** - The GitHub Actions workflow in `.github/workflows/release.yml` will handle this
2. ✅ **Proper versioning** - Update the version in `crates/league-mod/Cargo.toml` before creating releases
3. ✅ **Valid release assets** - The workflow creates ZIP files with SHA256 checksums
4. ✅ **Stable download URLs** - GitHub releases provide persistent URLs

## How to Submit to Winget Community Repository

### Step 1: Create a Release

1. Update the version in `crates/league-mod/Cargo.toml`
2. Commit your changes
3. Create and push a git tag:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
4. The GitHub Actions workflow will automatically build and create a release

### Step 2: Update Manifest Files

1. Update the version in all manifest files in `winget/manifests/LeagueToolkit/LeagueMod/0.1.0/`
2. Update the `InstallerUrl` in the installer manifest to point to your actual GitHub release
3. Update the `InstallerSha256` with the SHA256 hash from the release
4. Update URLs to point to your actual repository (already configured for LeagueToolkit organization)

### Step 3: Validate Manifests

Install winget manifest tools:
```powershell
# Install winget-create tool
winget install Microsoft.WingetCreate

# Validate your manifests
winget validate winget/manifests/LeagueMod/LeagueMod/0.1.0/
```

### Step 4: Submit to Winget Community Repository

#### Option A: Using winget-create (Recommended)
```powershell
# This will create a PR automatically
winget-create update LeagueToolkit.LeagueMod --version 0.1.0 --urls https://github.com/LeagueToolkit/league-mod/releases/download/v0.1.0/league-mod-0.1.0-windows-x64.zip
```

#### Option B: Manual Fork and PR
1. Fork the [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) repository
2. Copy your manifest files to the correct path:
   ```
   manifests/l/LeagueToolkit/LeagueMod/0.1.0/
   ```
3. Create a pull request with your changes

### Step 5: Wait for Review

- Automated validation will run on your PR
- Microsoft maintainers will review the submission
- Address any feedback and the package will be published

## Updating for New Releases

For each new release:

1. Create a new version directory: `winget/manifests/LeagueToolkit/LeagueMod/{new-version}/`
2. Copy and update the manifest files with the new version information
3. Submit using the same process above

## Manifest File Structure

- **`LeagueToolkit.LeagueMod.yaml`** - Package version manifest
- **`LeagueToolkit.LeagueMod.installer.yaml`** - Installation instructions and download URLs
- **`LeagueToolkit.LeagueMod.locale.en-US.yaml`** - Package metadata and descriptions

## Important Notes

- Keep package identifiers consistent across all versions
- Ensure download URLs are permanent (GitHub releases are perfect for this)
- Include accurate SHA256 hashes for security
- Follow winget naming conventions (Publisher.PackageName format: LeagueToolkit.LeagueMod)
- Test installation locally before submitting

## After Acceptance

Once your package is accepted:

- Users can install with: `winget install LeagueToolkit.LeagueMod`
- Or search for it: `winget search league-mod`
- Updates will be handled through the same PR process

## Troubleshooting

- **Validation errors**: Run `winget validate` locally first
- **Download issues**: Ensure GitHub release assets are publicly accessible
- **Hash mismatches**: Double-check SHA256 values from actual release files
- **Submission failures**: Check the [winget-pkgs contributing guide](https://github.com/microsoft/winget-pkgs/blob/master/CONTRIBUTING.md)

For more information, see the [official winget documentation](https://docs.microsoft.com/en-us/windows/package-manager/).
