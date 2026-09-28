# Veloct repository-local Cargo runner.
#
# Two environment problems exist on this machine, and both are handled here so nobody has to
# rediscover them:
#
#  1. `%USERPROFILE%\.cargo\config.toml` replaces crates.io with a mirror that is unreachable.
#     We therefore use an isolated CARGO_HOME (`.cargo-home/`) that pins the canonical sparse
#     registry. A project-local `source.crates-io.replace-with` cannot do this, because Cargo
#     rejects a replacement source that also defines the registry name `crates-io`.
#  2. Windows has a system HTTP proxy (an xray client on 127.0.0.1) that Cargo's libcurl picks
#     up. We read the configured proxy from the registry and pass it explicitly.
#
# Usage (note the -ExecutionPolicy Bypass; .ps1 execution is disabled on this machine):
#   powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace
#   powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 check -p veloct-core

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

# --- seed the crate cache so a first build does not re-download everything ---------------
$registrySrc = Join-Path $env:USERPROFILE '.cargo\registry'
$registryDst = Join-Path $cargoHome 'registry'
if ((Test-Path $registrySrc) -and -not (Test-Path $registryDst)) {
    Write-Host 'seeding crate cache from the user Cargo home ...' -ForegroundColor DarkGray
    try {
        New-Item -ItemType Junction -Path $registryDst -Target $registrySrc -ErrorAction Stop | Out-Null
    }
    catch {
        Copy-Item $registrySrc $registryDst -Recurse -Force
    }
}

$env:CARGO_HOME = $cargoHome
$env:Path = "$cargoBin;$env:Path"

# --- detect and apply the machine HTTP proxy ---------------------------------------------
# An explicit VELOCT_CARGO_PROXY always wins, so a machine with a different proxy (or none)
# can override without editing this file.
$proxyUrl = $env:VELOCT_CARGO_PROXY
if (-not $proxyUrl) {
    $inetKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $inet = Get-ItemProperty -Path $inetKey -ErrorAction SilentlyContinue
    if ($null -ne $inet -and $inet.ProxyEnable -eq 1 -and $inet.ProxyServer) {
        $raw = [string] $inet.ProxyServer
        # Formats seen in the wild: "127.0.0.1:10818" or "http=127.0.0.1:10818;https=127.0.0.1:10818".
        $candidates = $raw -split ';'
        $server = $null
        foreach ($candidate in $candidates) {
            $text = $candidate.Trim()
            if ($text -match '\=') {
                $parts = $text.Split('=', 2)
                $text = $parts[1].Trim()
            }
            if ($text.Length -gt 0) { $server = $text; break }
        }
        if ($server) {
            if ($server -match '^https?://') {
                $proxyUrl = $server
            }
            else {
                $proxyUrl = 'http://' + $server
            }
        }
    }
}

if ($proxyUrl) {
    $env:CARGO_HTTP_PROXY = $proxyUrl
    Write-Host "cargo: using registry proxy $proxyUrl" -ForegroundColor DarkGray
}

Write-Host "cargo: CARGO_HOME=$cargoHome" -ForegroundColor DarkGray

Push-Location $repoRoot
try {
    & $cargoExe @args
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
