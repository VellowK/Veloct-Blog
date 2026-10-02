<#
.SYNOPSIS
  Sign Veloct's build artefacts with a LOCAL, self-signed code-signing certificate.

.DESCRIPTION
  PROCESSING.md section 5.10.4 records the gap this closes: the installer is the only artefact a user RUNS, and
  it was unsigned. Run this after `scripts/build-installer.ps1` (and after signing the packaged binaries, if the
  installer is to carry signed ones — see the ORDER note below).

  WHAT A SELF-SIGNED CERTIFICATE DOES AND DOES NOT DO. Both halves matter, and only the first is usually said:

    * It DOES prove the bytes were signed by the holder of this key, and it gives Windows a publisher name to
      show once the certificate is trusted on that machine (Trusted Publishers).
    * It does NOT remove SmartScreen's warning on a machine that does not have the certificate, and it is NOT a
      substitute for a CA-issued one. A self-signed signature is exactly as trustworthy as the route the public
      certificate took to reach the machine reading it. Nothing here claims otherwise.

  WHERE THE KEY LIVES, and why this script signs from the STORE rather than a .pfx: the certificate and its
  private key stay in the current user's certificate store (`Cert:\CurrentUser\My`). Nothing here writes a
  password to disk, and NO private key material may ever enter this repository. The PUBLIC half is exported to
  `src-tauri/signing/` so that a machine which wants to trust these builds can install it.

  WHAT CAN AND CANNOT BE SIGNED THIS WAY — MEASURED on 2026-10-02, because the obvious assumption is wrong:

    * The setup `.exe` CAN be signed, and it is the artefact a user runs: `-Installer` signs it and it verifies
      `Valid`.
    * The APP binary's signature does NOT survive packaging. Signing `target/release/veloct-desktop.exe` and then
      rebuilding the installer leaves the packaged app **NotSigned** — measured, and the reason is likely that
      `target/release/<name>.exe` is a hard link to `target/release/deps/<name>-<hash>.exe`, so writing the
      signature breaks the link and the next build copies the unsigned file back over it. **A signature placed
      here is therefore a signature on THIS FILE, not on what a user installs.**
    * The SUPPORTED way to sign the packaged binaries is Tauri's own `bundle.windows.certificateThumbprint`
      (with `digestAlgorithm` and `timestampUrl`) in `src-tauri/tauri.conf.json`, which signs during the build.
      It is not configured here because the thumbprint is machine-specific and would make the committed manifest
      unusable on any other machine; the honest state is recorded in PROCESSING.md section 5.10.4 instead.

  THREE BINARIES ARE DELIBERATELY NOT SIGNED, each for a different reason:

    * `veloct-node.exe` is a third-party runtime this project redistributes, and **it already carries its own
      vendor signature** — the installed copy verifies `Valid`, `CN=OpenJS Foundation`. Signing it with this
      certificate would be a claim about someone else's binary.
    * `src-tauri/binaries/veloct-astro-sidecar-<triple>.exe` is the COMMITTED launcher, and
      `scripts/build-sidecar.ps1 -Check` compares it BYTE FOR BYTE against a fresh `cargo build -p veloct-sidecar`
      (PROCESSING.md section 2.2). A signed file can never match a fresh unsigned build, so signing it would
      break that check permanently. The launcher is an internal helper the app starts, not something a user
      runs, so it stays unsigned on purpose.

.PARAMETER Files
  Files to sign. Defaults to the repo-built release binary a user launches
  (`target/release/veloct-desktop.exe`).

.PARAMETER Installer
  Also sign `target/release/bundle/nsis/Veloct_*_x64-setup.exe`.

.PARAMETER Check
  Report each file's signature status without signing anything, and exit non-zero when one is not `Valid`.
#>
[CmdletBinding()]
param(
    [string[]]$Files = @(),
    [switch]$Installer,
    [switch]$Check,
    [string]$Subject = 'CN=VellowChire Code Signing, O=Veloct',
    [string]$TimestampServer = 'http://timestamp.digicert.com'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$signingDir = Join-Path $repoRoot 'src-tauri/signing'
$publicCert = Join-Path $signingDir 'veloct-codesign.cer'

function Get-SigningCertificate {
    $existing = Get-ChildItem Cert:\CurrentUser\My |
        Where-Object { $_.Subject -eq $Subject -and $_.HasPrivateKey } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1
    if ($existing) { return $existing }

    Write-Host "creating a self-signed code-signing certificate: $Subject"
    # `-Type CodeSigningCert` sets the EKU (1.3.6.1.5.5.7.3.3), which is what makes Windows accept the
    # certificate for Authenticode. A certificate without it signs a file that no verifier will accept.
    return New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject $Subject `
        -FriendlyName 'Veloct code signing (self-signed, local)' `
        -KeyUsage DigitalSignature `
        -KeyAlgorithm RSA -KeyLength 3072 `
        -NotAfter (Get-Date).AddYears(3) `
        -CertStoreLocation Cert:\CurrentUser\My
}

function Export-PublicCertificate($cert) {
    New-Item -ItemType Directory -Force -Path $signingDir | Out-Null
    # Cert, Base64: the PUBLIC half only. This is safe to commit and is what a user installs to trust a build.
    Export-Certificate -Cert $cert -FilePath $publicCert -Type CERT -Force | Out-Null
    Write-Host "public certificate written to $publicCert (private key stays in the certificate store)"
}

function Test-TrustedPublisher($cert) {
    $trusted = Get-ChildItem Cert:\CurrentUser\TrustedPublisher -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
    return [bool]$trusted
}

function Add-TrustedPublisher($cert) {
    # A SELF-SIGNED certificate is its own ROOT, so the chain only validates when the certificate is trusted as a
    # root as well as as a publisher. TrustedPublisher alone produces a signed file whose status reads
    # `UnknownError`: "A certificate chain processed, but terminated in a root certificate which is not trusted by
    # the trust provider" — which is what the first run of this script reported, and the reason both stores are
    # written below.
    #
    # Both are CURRENTUSER scope: no elevation, no machine-wide trust, and reversible with the two commands in
    # this script's own docs (`Remove-Item Cert:\CurrentUser\Root\<thumbprint>`).
    if (-not (Test-Path $publicCert)) { return }

    $publisher = Get-ChildItem Cert:\CurrentUser\TrustedPublisher -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
    if (-not $publisher) {
        Import-Certificate -FilePath $publicCert -CertStoreLocation Cert:\CurrentUser\TrustedPublisher | Out-Null
        Write-Host 'certificate added to Cert:\CurrentUser\TrustedPublisher (this user)'
    }

    $root = Get-ChildItem Cert:\CurrentUser\Root -ErrorAction SilentlyContinue |
        Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
    if (-not $root) {
        Import-Certificate -FilePath $publicCert -CertStoreLocation Cert:\CurrentUser\Root | Out-Null
        Write-Host 'certificate added to Cert:\CurrentUser\Root as its own root (this user; remove with'
        Write-Host "  Remove-Item Cert:\CurrentUser\Root\$($cert.Thumbprint) and the TrustedPublisher copy)"
    }
}

function Sign-One($path, $cert) {
    Write-Host "signing $path"
    try {
        $result = Set-AuthenticodeSignature -FilePath $path -Certificate $cert `
            -HashAlgorithm SHA256 -TimestampServer $TimestampServer
    } catch {
        # A timestamp is what keeps a signature valid after the certificate expires. It needs the network, and
        # a failure here must be SAID rather than silently degraded: a signature without one is still valid,
        # but it stops being valid when the certificate does.
        Write-Warning "timestamping failed ($($_.Exception.Message)); signing WITHOUT a timestamp"
        $result = Set-AuthenticodeSignature -FilePath $path -Certificate $cert -HashAlgorithm SHA256
    }
    $status = $result.Status
    Write-Host "  status: $status"
    if ($status -ne 'Valid') {
        Write-Warning "  $path reports $status — see the signature's own message below"
        Write-Host "  $($result.StatusMessage)"
    }
    return $status
}

# -----------------------------------------------------------------------------------------------------------
$targets = @()
if ($Files.Count -gt 0) {
    $targets = $Files
} else {
    foreach ($candidate in @(
        'target/release/veloct-desktop.exe'
    )) {
        $full = Join-Path $repoRoot $candidate
        if (Test-Path $full) { $targets += $full }
    }
}
if ($Installer) {
    $targets += Get-ChildItem (Join-Path $repoRoot 'target/release/bundle/nsis') -Filter 'Veloct_*_x64-setup.exe' -ErrorAction SilentlyContinue |
        Select-Object -ExpandProperty FullName
}
if ($targets.Count -eq 0) {
    throw 'nothing to sign: build first, or pass -Files'
}

if ($Check) {
    $bad = 0
    foreach ($file in $targets) {
        $signature = Get-AuthenticodeSignature -FilePath $file
        $signer = if ($signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { '(none)' }
        "{0,-58} {1,-12} {2}" -f (Split-Path -Leaf $file), $signature.Status, $signer
        if ($signature.Status -ne 'Valid') { $bad++ }
    }
    if ($bad -gt 0) { throw "$bad artefact(s) are not validly signed" }
    Write-Host 'every artefact is validly signed'
    exit 0
}

$cert = Get-SigningCertificate
Export-PublicCertificate $cert
Add-TrustedPublisher $cert
Write-Host "certificate thumbprint: $($cert.Thumbprint)"
Write-Host "valid until:            $($cert.NotAfter)"

$failed = 0
foreach ($file in $targets) {
    if (-not (Test-Path $file)) { throw "no such file: $file" }
    if ((Sign-One $file $cert) -ne 'Valid') { $failed++ }
}

if ($failed -gt 0) {
    throw "$failed artefact(s) did not verify as Valid"
}
Write-Host "signed $($targets.Count) artefact(s), each verified Valid"
