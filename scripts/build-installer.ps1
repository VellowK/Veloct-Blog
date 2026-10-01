# Veloct repository-local Tauri bundle build.
#
# `tauri build` shells out to `cargo metadata` and to `cargo build` itself, so it needs the same three
# environment fixes `scripts/cargo.ps1` documents, plus one of its own:
#
#  1. `%USERPROFILE%\.cargo\config.toml` points crates.io at an unreachable mirror, so CARGO_HOME is
#     the repository's isolated `.cargo-home/`.
#  2. The machine HTTP proxy is passed to Cargo explicitly.
#  3. `cargo` and `rustup` are NOT on PATH; they live in `%USERPROFILE%\.cargo\bin`, which is prepended
#     here. Without this, `tauri build` fails with
#     `failed to run 'cargo metadata' command ... program not found` — measured, not assumed.
#
# The Tauri CLI is a devDependency (`@tauri-apps/cli`), so the command is resolved from
# `node_modules/.bin` rather than requiring a global install.
#
# Usage (note the -ExecutionPolicy Bypass; .ps1 execution is disabled on this machine):
#   powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1
#   powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Bundles nsis
#
# The artefact lands in `target/release/bundle/nsis/`.

param(
    [string]$Bundles = 'nsis'
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoHome = Join-Path $repoRoot '.cargo-home'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$cargoExe = Join-Path $cargoBin 'cargo.exe'

if (-not (Test-Path $cargoExe)) {
    throw "cargo was not found at $cargoExe (rustup installs it there; it is not on PATH)"
}
if (-not (Test-Path (Join-Path $cargoHome 'config.toml'))) {
    throw "missing .cargo-home/config.toml - the isolated Cargo home is required on this machine"
}

$tauriCli = Join-Path $repoRoot 'node_modules\@tauri-apps\cli\tauri.js'
if (-not (Test-Path $tauriCli)) {
    throw "the Tauri CLI was not found at $tauriCli; run 'npm.cmd install' first"
}

$env:CARGO_HOME = $cargoHome
$env:Path = "$cargoBin;$env:Path"

# --- detect and apply the machine HTTP proxy (same rule as cargo.ps1) --------------------
$proxyUrl = $env:VELOCT_CARGO_PROXY
if (-not $proxyUrl) {
    $inetKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $inet = Get-ItemProperty -Path $inetKey -ErrorAction SilentlyContinue
    if ($null -ne $inet -and $inet.ProxyEnable -eq 1 -and $inet.ProxyServer) {
        $raw = [string] $inet.ProxyServer
        foreach ($candidate in ($raw -split ';')) {
            $text = $candidate.Trim()
            if ($text -match '\=') {
                $text = $text.Split('=', 2)[1].Trim()
            }
            if ($text.Length -gt 0) {
                $proxyUrl = if ($text -match '^https?://') { $text } else { 'http://' + $text }
                break
            }
        }
    }
}

if ($proxyUrl) {
    $env:CARGO_HTTP_PROXY = $proxyUrl
    Write-Host "tauri: using registry proxy $proxyUrl" -ForegroundColor DarkGray
}

Write-Host "tauri: CARGO_HOME=$cargoHome" -ForegroundColor DarkGray

Push-Location $repoRoot
try {
    # A native command's stderr becomes a TERMINATING error under this script's own preference, and both
    # cargo and the bundler write progress there. The preference is relaxed for the call alone and the
    # exit code decides — the same defect `scripts/cargo.ps1` documents in PROCESSING.md section 2.1.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & node $tauriCli build --bundles $Bundles
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previous
    }
    exit $code
}
finally {
    Pop-Location
}
