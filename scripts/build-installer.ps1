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
    [string]$Bundles = 'nsis',
    # Signing. The thumbprint is NOT in `tauri.conf.json` on purpose: it is machine-specific, and a committed
    # thumbprint would make the manifest unusable on every other machine (PROCESSING.md section 5.10.4). It is
    # discovered here instead - from this parameter, from `VELOCT_SIGN_THUMBPRINT`, or from the certificate
    # store by subject - and passed to the bundler as a MERGED config, so the committed manifest stays neutral.
    [string]$SignThumbprint = '',
    [string]$SignSubject = 'CN=VellowChire Code Signing, O=Veloct',
    [string]$TimestampUrl = 'http://timestamp.digicert.com',
    [switch]$NoSign
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

# --- signing ---------------------------------------------------------------------------------
#
# WHY THIS IS DONE BY THE BUNDLER RATHER THAN AS A POST-BUILD STEP, measured on 2026-10-02: signing
# `target/release/veloct-desktop.exe` and THEN building the package leaves the INSTALLED app `NotSigned`,
# because the build copies the unsigned file back over it (`target/release/<name>.exe` is a hard link to
# `target/release/deps/<name>-<hash>.exe`). Tauri signs during the bundle step, which is the supported order.
function Resolve-SigningThumbprint {
    if ($NoSign) { return $null }
    $candidate = if ($SignThumbprint) { $SignThumbprint } else { $env:VELOCT_SIGN_THUMBPRINT }
    if ($candidate) {
        $found = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Thumbprint -eq $candidate }
        if (-not $found) { throw "no certificate with thumbprint $candidate in Cert:\CurrentUser\My" }
        return $found[0].Thumbprint
    }
    $bySubject = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq $SignSubject -and $_.HasPrivateKey } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
    if ($bySubject) { return $bySubject.Thumbprint }
    return $null
}

$signThumbprint = Resolve-SigningThumbprint
$signConfig = $null
# Tauri signs the COMMITTED external binaries IN PLACE (`src-tauri/binaries/veloct-astro-sidecar-<triple>.exe`),
# which was measured the hard way on 2026-10-02: after one signed build, `scripts/build-sidecar.ps1 -Check` fails
# permanently, because that check compares the committed launcher BYTE FOR BYTE against a fresh `cargo build`
# (PROCESSING.md section 2.2) and a signed file can never match an unsigned one. The package SHOULD carry the
# signed copy, so the fix is not to stop signing: it is to put the committed source file back afterwards, which
# this script does automatically rather than trusting anyone to remember.
$externalBinaries = @(Get-ChildItem (Join-Path $repoRoot 'src-tauri/binaries') -Filter '*.exe' -ErrorAction SilentlyContinue)
$binaryBackup = @{}
foreach ($binary in $externalBinaries) {
    $binaryBackup[$binary.FullName] = [System.IO.File]::ReadAllBytes($binary.FullName)
}

if ($signThumbprint) {
    $cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Thumbprint -eq $signThumbprint }
    Write-Host "tauri: SIGNING with $($cert.Subject) (thumbprint $signThumbprint, until $($cert.NotAfter))" -ForegroundColor Green
    # A temporary MERGED config: `tauri build --config <path>` merges it over tauri.conf.json, so nothing in the
    # repository records a machine's certificate.
    $signConfig = Join-Path $env:TEMP 'veloct-sign-config.json'
    $json = [ordered]@{
        bundle = [ordered]@{
            windows = [ordered]@{
                certificateThumbprint = $signThumbprint
                digestAlgorithm       = 'sha256'
                timestampUrl          = $TimestampUrl
            }
        }
    } | ConvertTo-Json -Depth 6
    [System.IO.File]::WriteAllText($signConfig, $json, (New-Object System.Text.UTF8Encoding($false)))
}
else {
    # Stated rather than silent: an unsigned build is a fact about the artefact, and section 5.10.4 records
    # exactly what a signature does and does not buy even when there is one.
    Write-Warning 'tauri: NO signing certificate was found, so this package will be UNSIGNED (run scripts/sign-artifacts.ps1 once to create one, or pass -SignThumbprint)'
}

Push-Location $repoRoot
try {
    # A native command's stderr becomes a TERMINATING error under this script's own preference, and both
    # cargo and the bundler write progress there. The preference is relaxed for the call alone and the
    # exit code decides — the same defect `scripts/cargo.ps1` documents in PROCESSING.md section 2.1.
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        if ($signConfig) {
            & node $tauriCli build --bundles $Bundles --config $signConfig
        }
        else {
            & node $tauriCli build --bundles $Bundles
        }
        $code = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previous
    }
    exit $code
}
finally {
    Pop-Location
    if ($signConfig -and (Test-Path $signConfig)) { Remove-Item $signConfig -Force -ErrorAction SilentlyContinue }
    # Restore the committed external binaries the bundler signed in place, so the tree stays clean and
    # `build-sidecar.ps1 -Check` keeps meaning what it says. Reported rather than silent: a restore that
    # happened is a fact about the build.
    foreach ($path in $binaryBackup.Keys) {
        $current = [System.IO.File]::ReadAllBytes($path)
        if (-not [System.Linq.Enumerable]::SequenceEqual([byte[]]$current, [byte[]]$binaryBackup[$path])) {
            [System.IO.File]::WriteAllBytes($path, [byte[]]$binaryBackup[$path])
            Write-Host "tauri: restored $(Split-Path -Leaf $path) (the bundler signed the committed copy in place; the PACKAGE keeps the signed one)" -ForegroundColor Yellow
        }
    }
}
