# Installs the latest League Mod CLI release for Windows (user scope)
param(
    [string]$Owner = "LeagueToolkit",
    [string]$Repo  = "league-mod",
    [string]$Channel = "windows-x64",
    [string]$InstallDir = "$env:LOCALAPPDATA\LeagueToolkit\league-mod"
)

$ErrorActionPreference = 'Stop'

Write-Host "Installing League Mod..." -ForegroundColor Cyan

if (!(Test-Path -LiteralPath $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# Get releases and find the latest league-mod CLI release (not library releases)
$releaseApi = "https://api.github.com/repos/$Owner/$Repo/releases"
try {
    $releases = Invoke-RestMethod -Uri $releaseApi -Headers @{ 'User-Agent' = 'league-mod-installer' }
} catch {
    throw "Failed to query GitHub releases: $($_.Exception.Message)"
}

# Find the first release with tag starting with "league-mod-v" (the CLI binary release)
$release = $releases | Where-Object { $_.tag_name -match '^league-mod-v' } | Select-Object -First 1
if (-not $release) {
    throw "Could not find a league-mod CLI release (tag matching 'league-mod-v*')."
}

$tag = $release.tag_name
# Extract the semantic version from the tag (e.g., "league-mod-v0.2.1" -> "0.2.1")
$match = [regex]::Match($tag, '\d+\.\d+\.\d+([\-\+][A-Za-z0-9\.-]+)?')
$version = if ($match.Success) { $match.Value } else { $tag.TrimStart('v') }

$assetName = "league-mod-$version-$Channel.zip"
$asset = $release.assets | Where-Object { $_.name -eq $assetName } | Select-Object -First 1
if (-not $asset) {
    # Fallback: find any league-mod asset matching the channel
    $pattern = "^league-mod-.*-" + [regex]::Escape($Channel) + "\.zip$"
    $asset = $release.assets | Where-Object { $_.name -match $pattern } | Select-Object -First 1
}
if (-not $asset) {
    throw "Could not find asset matching '$assetName' (channel $Channel) in the latest release."
}
if ($asset.name -ne $assetName) { $assetName = $asset.name }

$zipPath = Join-Path $env:TEMP $assetName
Write-Host "Downloading $assetName ($version)..." -ForegroundColor Yellow
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing

Write-Host "Extracting to $InstallDir" -ForegroundColor Yellow
Expand-Archive -Path $zipPath -DestinationPath $InstallDir -Force

# Create a shim directory so PATH is simple and stable
$binDir = Join-Path $InstallDir 'bin'
if (!(Test-Path -LiteralPath $binDir)) { New-Item -ItemType Directory -Path $binDir | Out-Null }

# Ensure the executable exists
$exePath = Join-Path $InstallDir 'league-mod.exe'
if (!(Test-Path -LiteralPath $exePath)) {
    throw "league-mod.exe not found after extraction: $exePath"
}

# Place a thin cmd shim in bin to avoid spaces in paths and simplify PATH updates
$shimCmd = @"
@echo off
""$exePath"" %*
"@
Set-Content -LiteralPath (Join-Path $binDir 'league-mod.cmd') -Value $shimCmd -Encoding Ascii -Force

# Add to user PATH if missing
$currentPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not ($currentPath -split ';' | Where-Object { $_ -eq $binDir })) {
    $newPath = if ([string]::IsNullOrEmpty($currentPath)) { $binDir } else { "$currentPath;$binDir" }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "Added to PATH (User): $binDir" -ForegroundColor Green
} else {
    Write-Host "PATH already contains: $binDir" -ForegroundColor Green
}

Write-Host "Installed league-mod $version to $InstallDir" -ForegroundColor Green
Write-Host "Open a new terminal and run: league-mod --help" -ForegroundColor Cyan

