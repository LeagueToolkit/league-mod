#!/bin/bash

# Shell script to help update winget manifest files for new releases

usage() {
    echo "Usage: $0 <version> <github-username> <sha256-hash> [release-date]"
    echo "Example: $0 0.1.0 myusername abc123... 2024-01-01"
    exit 1
}

if [ $# -lt 3 ]; then
    usage
fi

VERSION="$1"
GITHUB_USERNAME="$2"
SHA256_HASH="$3"
RELEASE_DATE="${4:-$(date +%Y-%m-%d)}"

echo "Updating winget manifests for version $VERSION"

# Create new version directory
VERSION_DIR="manifests/LeagueToolkit/LeagueMod/$VERSION"
mkdir -p "$VERSION_DIR"
echo "Created directory: $VERSION_DIR"

# Update package manifest
cat > "$VERSION_DIR/LeagueToolkit.LeagueMod.yaml" << EOF
# Package manifest for league-mod
PackageIdentifier: LeagueToolkit.LeagueMod
PackageVersion: $VERSION
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
EOF
echo "Updated package manifest"

# Update installer manifest
cat > "$VERSION_DIR/LeagueToolkit.LeagueMod.installer.yaml" << EOF
# Installer manifest for league-mod
PackageIdentifier: LeagueToolkit.LeagueMod
PackageVersion: $VERSION
Platform:
- Windows.Desktop
MinimumOSVersion: 10.0.0.0
InstallerType: zip
Scope: user
InstallModes:
- interactive
- silent
UpgradeBehavior: install
ReleaseDate: $RELEASE_DATE
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/$GITHUB_USERNAME/league-mod/releases/download/v$VERSION/league-mod-$VERSION-windows-x64.zip
  InstallerSha256: $SHA256_HASH
  NestedInstallerType: portable
  NestedInstallerFiles:
  - RelativeFilePath: league-mod.exe
    PortableCommandAlias: league-mod
ManifestType: installer
ManifestVersion: 1.6.0
EOF
echo "Updated installer manifest"

# Update locale manifest
cat > "$VERSION_DIR/LeagueToolkit.LeagueMod.locale.en-US.yaml" << EOF
# Locale manifest for league-mod
PackageIdentifier: LeagueToolkit.LeagueMod
PackageVersion: $VERSION
PackageLocale: en-US
Publisher: LeagueToolkit
PublisherUrl: https://github.com/$GITHUB_USERNAME/league-mod
PublisherSupportUrl: https://github.com/$GITHUB_USERNAME/league-mod/issues
Author: $GITHUB_USERNAME
PackageName: League Mod
PackageUrl: https://github.com/$GITHUB_USERNAME/league-mod
License: AGPL-3.0
LicenseUrl: https://github.com/$GITHUB_USERNAME/league-mod/blob/main/LICENSE
ShortDescription: A comprehensive toolkit for creating, managing, and distributing League of Legends mods
Description: |-
  League Mod is a powerful command-line toolkit for League of Legends mod developers and users. 
  It provides a complete solution for creating, managing, and distributing mods using the modpkg format.
  
  Key features:
  • Initialize new mod projects with interactive prompts
  • Pack mod projects into distributable .modpkg files
  • Extract existing .modpkg files for inspection or modification
  • Display detailed information about mod packages
  • Layer-based mod organization with priority system
  • Support for file transformers and preprocessing
  • Cross-format configuration (JSON and TOML)
  
  Perfect for both mod creators who want to package and distribute their work, and end users who want to manage and install League of Legends mods easily.
Tags:
- league-of-legends
- modding
- gaming
- cli
- toolkit
- lol
- mods
- package-manager
ReleaseNotes: |-
  Release $VERSION of League Mod toolkit.
  
  See full changelog at: https://github.com/$GITHUB_USERNAME/league-mod/releases/tag/v$VERSION
ReleaseNotesUrl: https://github.com/$GITHUB_USERNAME/league-mod/releases/tag/v$VERSION
ManifestType: defaultLocale
ManifestVersion: 1.6.0
EOF
echo "Updated locale manifest"

echo "✅ Manifest files updated for version $VERSION"
echo ""
echo "Next steps:"
echo "1. Validate manifests: winget validate $VERSION_DIR"
echo "2. Test locally: winget install --manifest $VERSION_DIR"
echo "3. Submit to winget-pkgs repository"
