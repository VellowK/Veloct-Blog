<#
.SYNOPSIS
  Fetch and verify the pinned Node runtime the Astro sidecar launches.

.DESCRIPTION
  ADR-0005 requires a BUNDLED Node, never the user's. The runtime is 89 MB, so it is fetched from the
  official distribution and verified against the checksum Node publishes rather than committed: the pin
  lives in `scripts/node-runtime.toml` and this script refuses to install anything whose hash does not
  match, which is what makes "the bundled runtime" a reproducible statement instead of "whatever was on
  the build machine".

  Run once after cloning, and again whenever the pin changes:

    powershell -ExecutionPolicy Bypass -File .\scripts\fetch-node.ps1

  Re-running is cheap when the runtime is already correct: the hash is checked first and the download is
  skipped.

.PARAMETER Force
  Re-download even when the installed runtime already matches the pin.
#>
[CmdletBinding()]
param(
  [switch]$Force
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$pinPath = Join-Path $PSScriptRoot 'node-runtime.toml'

if (-not (Test-Path -LiteralPath $pinPath)) {
  throw "the pin is missing: $pinPath"
}

# A tiny TOML reader for the flat shape this file uses. A general parser would be a dependency; this reads
# `key = "value"` inside a named section and refuses anything it does not understand, so a malformed pin is
# reported rather than silently producing an empty value.
function Read-Pin {
  param([string]$Path)

  $sections = @{}
  $current = ''
  foreach ($rawLine in Get-Content -LiteralPath $Path) {
    $line = $rawLine.Trim()
    if ($line -eq '' -or $line.StartsWith('#')) { continue }

    if ($line.StartsWith('[') -and $line.EndsWith(']')) {
      $current = $line.Substring(1, $line.Length - 2).Trim()
      if (-not $sections.ContainsKey($current)) { $sections[$current] = @{} }
      continue
    }

    $parts = $line -split '=', 2
    if ($parts.Count -ne 2) { throw "cannot read the pin line: $rawLine" }
    $key = $parts[0].Trim()
    $value = $parts[1].Trim().Trim('"')
    if ($current -eq '') { $sections[$key] = $value; continue }
    $sections[$current][$key] = $value
  }
  return $sections
}

$pin = Read-Pin -Path $pinPath
$version = $pin['version']
if (-not $version) { throw 'the pin has no version' }

$section = $pin['windows-x64']
if (-not $section) { throw 'the pin has no [windows-x64] section' }

$archive = $section['archive']
$archiveSha = $section['archive_sha256']
$member = $section['member']
$memberSha = $section['member_sha256']
$targetRelative = $section['target']

foreach ($required in @('archive', 'archive_sha256', 'member', 'member_sha256', 'target')) {
  if (-not $section[$required]) { throw "the [windows-x64] section has no $required" }
}

$target = Join-Path $repoRoot ($targetRelative -replace '/', [IO.Path]::DirectorySeparatorChar)
$targetDirectory = Split-Path -Parent $target
if (-not (Test-Path -LiteralPath $targetDirectory)) {
  New-Item -ItemType Directory -Path $targetDirectory -Force | Out-Null
}

function Get-Sha256 {
  param([string]$Path)
  return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

if (-not $Force -and (Test-Path -LiteralPath $target)) {
  $installed = Get-Sha256 -Path $target
  if ($installed -eq $memberSha) {
    Write-Host "node $version is already installed and matches the pin: $targetRelative"
    exit 0
  }
  Write-Host "the installed node does not match the pin (have $installed, want $memberSha); re-fetching"
}

$url = "https://nodejs.org/dist/$version/$archive"
$tempZip = Join-Path ([IO.Path]::GetTempPath()) $archive

Write-Host "downloading $url"
$progress = $ProgressPreference
# The progress bar makes a 36 MB download several times slower, and this is a script a human waits on once.
$ProgressPreference = 'SilentlyContinue'
try {
  Invoke-WebRequest -Uri $url -OutFile $tempZip -UseBasicParsing
} finally {
  $ProgressPreference = $progress
}

$archiveHash = Get-Sha256 -Path $tempZip
if ($archiveHash -ne $archiveSha) {
  Remove-Item -LiteralPath $tempZip -Force -ErrorAction SilentlyContinue
  throw "the download does not match the published checksum: have $archiveHash, want $archiveSha"
}
Write-Host "the archive matches the published checksum"

Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($tempZip)
try {
  $entry = $zip.Entries | Where-Object { $_.FullName -eq $member }
  if (-not $entry) { throw "$archive has no member $member" }

  # Extract to a temporary name and verify BEFORE replacing the installed runtime, so a failed fetch cannot
  # leave a half-written binary where the launcher would find it.
  $staged = "$target.staged"
  [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $staged, $true)
} finally {
  $zip.Dispose()
  Remove-Item -LiteralPath $tempZip -Force -ErrorAction SilentlyContinue
}

$stagedHash = Get-Sha256 -Path $staged
if ($stagedHash -ne $memberSha) {
  Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
  throw "the extracted node.exe does not match the pin: have $stagedHash, want $memberSha"
}

Move-Item -LiteralPath $staged -Destination $target -Force
$size = [math]::Round((Get-Item -LiteralPath $target).Length / 1MB, 2)
Write-Host "installed node $version ($size MB) at $targetRelative"
