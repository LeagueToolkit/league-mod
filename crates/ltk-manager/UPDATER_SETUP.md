# LTK Manager Auto-Updater Setup

This document describes how to configure the auto-updater for LTK Manager releases.

## Overview

LTK Manager uses [Tauri's updater plugin](https://v2.tauri.app/plugin/updater/) to provide automatic updates. Updates are distributed via GitHub Releases and are cryptographically signed to ensure authenticity.

## Prerequisites

- Tauri CLI installed: `cargo install tauri-cli`
- Access to the [LeagueToolkit/league-mod](https://github.com/LeagueToolkit/league-mod) repository settings

## Setup Steps

### 1. Generate Signing Keys

The updater requires a key pair for signing updates. Generate one using the Tauri CLI:

```bash
# Generate a new key pair (will prompt for a password)
cargo tauri signer generate -w ~/.tauri/ltk-manager.key
```

This creates two files:
- `~/.tauri/ltk-manager.key` - Private key (keep this secret!)
- `~/.tauri/ltk-manager.key.pub` - Public key

**Important:** Store the private key securely and never commit it to the repository.

### 2. Configure GitHub Secrets

Add the following secrets to the GitHub repository:

1. Go to **Settings** → **Secrets and variables** → **Actions**
2. Add the following repository secrets:

| Secret Name | Value |
|-------------|-------|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.tauri/ltk-manager.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you used when generating the key |

To get the private key contents:

```bash
cat ~/.tauri/ltk-manager.key
```

### 3. Update the Public Key in Configuration

Replace the placeholder public key in `src-tauri/tauri.conf.json`:

```json
{
  "plugins": {
    "updater": {
      "pubkey": "YOUR_PUBLIC_KEY_HERE",
      ...
    }
  }
}
```

To get your public key:

```bash
cat ~/.tauri/ltk-manager.key.pub
```

The public key should look something like:
```
dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6...
```

### 4. Verify Configuration

The updater configuration in `tauri.conf.json` should look like:

```json
{
  "plugins": {
    "updater": {
      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6...",
      "endpoints": [
        "https://github.com/LeagueToolkit/league-mod/releases/latest/download/latest.json"
      ],
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

## Creating a Release

### Automatic Release (Recommended)

1. Create and push a tag:
   ```bash
   git tag ltk-manager-v0.1.0
   git push origin ltk-manager-v0.1.0
   ```

2. The GitHub Actions workflow will automatically:
   - Build the Windows installer
   - Sign the update artifacts
   - Create a draft GitHub Release
   - Upload the `latest.json` manifest

3. Review the draft release and publish when ready.

### Manual Release

Use the workflow dispatch:

1. Go to **Actions** → **Release LTK Manager**
2. Click **Run workflow**
3. Enter the version number (e.g., `0.1.0`)
4. Click **Run workflow**

## Update Flow

When a user runs LTK Manager:

1. On startup (after 3 seconds), the app checks the GitHub releases endpoint
2. If a newer version is found, a notification banner appears
3. User clicks "Update Now" to download and install
4. The app automatically restarts with the new version

## Troubleshooting

### Update check fails

- Verify the endpoint URL in `tauri.conf.json` is correct
- Check if the `latest.json` file exists in the latest release
- Ensure the public key matches the key used to sign the release

### Signature verification fails

- Ensure the `TAURI_SIGNING_PRIVATE_KEY` secret contains the correct private key
- Verify the public key in `tauri.conf.json` matches the private key

### Build fails

- Check that all required secrets are configured
- Verify the Rust and Node.js versions are compatible
- Check the GitHub Actions logs for specific error messages

## Security Considerations

- Never commit the private key to the repository
- Use a strong password for the private key
- Rotate keys periodically if compromised
- Only maintainers should have access to the signing secrets

## References

- [Tauri Updater Plugin Documentation](https://v2.tauri.app/plugin/updater/)
- [Tauri GitHub Action](https://github.com/tauri-apps/tauri-action)
- [Code Signing Guide](https://v2.tauri.app/distribute/sign/)


