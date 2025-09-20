# PowerShell script to help update winget manifest files for new releases
param(
    [Parameter(Mandatory=$true)]
    [string]$Version,
    
    [Parameter(Mandatory=$true)]
    [string]$GitHubUsername,
    
    [Parameter(Mandatory=$true)]
    [string]$Sha256Hash,
    
    [string]$ReleaseDate = (Get-Date -Format "yyyy-MM-dd")
)

Write-Host "Updating winget manifests for version $Version" -ForegroundColor Green

# Create new version directory
$versionDir = "manifests/LeagueToolkit/LeagueMod/$Version"
if (!(Test-Path $versionDir)) {
    New-Item -ItemType Directory -Path $versionDir -Force
    Write-Host "Created directory: $versionDir" -ForegroundColor Yellow
}

# Update package manifest
$packageManifest = @"
# Package manifest for league-mod
PackageIdentifier: LeagueToolkit.LeagueMod
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
"@

$packageManifest | Out-File "$versionDir/LeagueToolkit.LeagueMod.yaml" -Encoding UTF8
Write-Host "Updated package manifest" -ForegroundColor Yellow

# Update installer manifest
$installerManifest = @"
# Installer manifest for league-mod
PackageIdentifier: LeagueToolkit.LeagueMod
PackageVersion: $Version
Platform:
- Windows.Desktop
MinimumOSVersion: 10.0.0.0
InstallerType: zip
Scope: user
InstallModes:
- interactive
- silent
UpgradeBehavior: install
ReleaseDate: $ReleaseDate
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/$GitHubUsername/league-mod/releases/download/v$Version/league-mod-$Version-windows-x64.zip
  InstallerSha256: $Sha256Hash
  NestedInstallerType: portable
  NestedInstallerFiles:
  - RelativeFilePath: league-mod.exe
    PortableCommandAlias: league-mod
ManifestType: installer
ManifestVersion: 1.6.0
"@

$installerManifest | Out-File "$versionDir/LeagueToolkit.LeagueMod.installer.yaml" -Encoding UTF8
Write-Host "Updated installer manifest" -ForegroundColor Yellow

# Update locale manifest
$localeManifest = @"
# Locale manifest for league-mod
PackageIdentifier: LeagueToolkit.LeagueMod
PackageVersion: $Version
PackageLocale: en-US
Publisher: LeagueToolkit
PublisherUrl: https://github.com/$GitHubUsername/league-mod
PublisherSupportUrl: https://github.com/$GitHubUsername/league-mod/issues
Author: $GitHubUsername
PackageName: League Mod
PackageUrl: https://github.com/$GitHubUsername/league-mod
License: AGPL-3.0
LicenseUrl: https://github.com/$GitHubUsername/league-mod/blob/main/LICENSE
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
  
  Designed for mod creators who want to package and distribute their League of Legends mods easily.
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
  Release $Version of League Mod toolkit.
  
  See full changelog at: https://github.com/$GitHubUsername/league-mod/releases/tag/v$Version
ReleaseNotesUrl: https://github.com/$GitHubUsername/league-mod/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: 1.6.0
"@

$localeManifest | Out-File "$versionDir/LeagueToolkit.LeagueMod.locale.en-US.yaml" -Encoding UTF8
Write-Host "Updated locale manifest" -ForegroundColor Yellow

Write-Host "✅ Manifest files updated for version $Version" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "1. Validate manifests: winget validate $versionDir" -ForegroundColor White
Write-Host "2. Test locally: winget install --manifest $versionDir" -ForegroundColor White
Write-Host "3. Submit to winget-pkgs repository" -ForegroundColor White
