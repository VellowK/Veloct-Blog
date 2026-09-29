<#
.SYNOPSIS
  Rebuild the sidecar launcher and install it at the path Tauri resolves.

.DESCRIPTION
  `handoff.md` ADR-0005 makes Node a bundled sidecar, and Tauri's `externalBin` resolves
  `binaries/veloct-astro-sidecar` to `binaries/veloct-astro-sidecar-<target-triple>.exe` AT RUNTIME. The
  launcher is committed because it has to exist at that exact path, which creates a trap worth naming:

  **A committed binary goes stale silently.** A Rust change to `crates/veloct-sidecar` does not rebuild the
  file in `src-tauri/binaries/`, and the committed copy then lacks whatever the source gained.

  What that costs is worth being precise about, because the first version of this comment overstated it: the
  launcher is a self-contained Rust binary, so a stale one still RUNS — it does not fail with a missing symbol.
  It silently lacks the newer BEHAVIOUR. Two real changes had gone missing that way: resolving the Astro entry
  from the package's own `bin` field (before which a site with its dependencies correctly installed was reported
  as missing its template), and an extra `Clock` method. A silently older binary is the worst version of this
  problem, because everything looks like it works.

  This script closes the trap from the build side: it builds and installs the launcher, and says whether the
  result CHANGED, which is what tells a caller to commit it.

.PARAMETER Check
  Do not install. Build, compare, and exit non-zero when the committed launcher is stale, so a verification
  step can fail rather than a user discovering it.
#>
[CmdletBinding()]
param(
  [switch]$Check
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$target = Join-Path $repoRoot 'src-tauri/binaries/veloct-astro-sidecar-x86_64-pc-windows-msvc.exe'
$builtPath = Join-Path $repoRoot 'target/debug/veloct-astro-sidecar.exe'

if ($Check) {
  # A release install is what the application ships, so the CHECK builds the same profile the shipped binary
  # would come from. Debug is used here because it is what the tests use, and the point is to catch a source
  # change nobody rebuilt for.
  # Cargo writes progress, and the MSVC linker writes an informational line, to STDERR. Under
  # `$ErrorActionPreference = 'Stop'` a native command's stderr becomes a terminating PowerShell error even when
  # the command SUCCEEDED, so the preference is relaxed for this one call and the exit code decides.
  $previous = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  try {
    & (Join-Path $PSScriptRoot 'cargo.ps1') build -p veloct-sidecar 2>&1 | Out-Null
    $code = $LASTEXITCODE
  } finally {
    $ErrorActionPreference = $previous
  }
  if ($code -ne 0) {
    throw "the sidecar did not build (cargo exited $code)"
  }
  if (-not (Test-Path -LiteralPath $builtPath)) {
    throw "the build produced no launcher at $builtPath"
  }
  if (-not (Test-Path -LiteralPath $target)) {
    Write-Error "the committed launcher is MISSING at $target"
    exit 1
  }

  $built = (Get-FileHash -LiteralPath $builtPath -Algorithm SHA256).Hash
  $installed = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
  if ($built -ne $installed) {
    Write-Error @"
the committed sidecar launcher is STALE.

  committed: $installed
  built:     $built

Run `powershell -ExecutionPolicy Bypass -File .\scripts\build-sidecar.ps1` and commit the result. A stale
launcher still runs, but with OLDER behaviour, which is why nothing else would have caught it.
"@
    exit 1
  }
  Write-Host 'the committed sidecar launcher matches a fresh build'
  exit 0
}

& (Join-Path $PSScriptRoot 'cargo.ps1') build -p veloct-sidecar
if (-not (Test-Path -LiteralPath $builtPath)) {
  throw "the build produced no launcher at $builtPath"
}

$before = if (Test-Path -LiteralPath $target) {
  (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
} else {
  $null
}

Copy-Item -LiteralPath $builtPath -Destination $target -Force

$after = (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash
$sizeMb = [math]::Round((Get-Item -LiteralPath $target).Length / 1MB, 2)

if ($before -eq $after) {
  Write-Host "the sidecar launcher is already current ($sizeMb MB)"
} elseif ($null -eq $before) {
  Write-Host "installed the sidecar launcher ($sizeMb MB); COMMIT src-tauri/binaries/veloct-astro-sidecar-*.exe"
} else {
  Write-Host "REBUILT the sidecar launcher ($sizeMb MB); COMMIT src-tauri/binaries/veloct-astro-sidecar-*.exe"
}
