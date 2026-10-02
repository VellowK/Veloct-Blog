# Veloct 1.0.0

**Artefact:** `Veloct_1.0.0_x64-setup.exe` — 29,374,472 bytes (28.01 MiB), SHA-256 `CDB98F4409247BC2F54CEA3E84B822DB05FEA42960AE29331989FBADBAE63773`.
**Signature:** `Valid` under `CN=VellowChire Code Signing, O=Veloct` — a LOCAL, SELF-SIGNED certificate
(`src-tauri/signing/veloct-codesign.cer`). **What that does and does not buy:** it proves the bytes were signed by
the holder of that key and gives Windows a publisher name **once the certificate is installed into that machine's
Trusted Publishers**; it does **not** remove SmartScreen's warning on a machine that has not done that, and it is not
a substitute for a CA-issued certificate. Every binary INSIDE the package is signed too — `veloct-desktop.exe`,
`veloct-astro-sidecar.exe` and `uninstall.exe` with this certificate, and `veloct-node.exe` with its own
vendor signature (`CN=OpenJS Foundation`).

**Verified by installing it, not by building it:** all four installed binaries verify `Valid`; the installed
launcher serves `GET http://localhost:4399/` with **200**; a tree-kill frees the port; `uninstall.exe` removes
the directory. The recorded run is `git log`'s `1.0.0` commit and PROCESSING.md §1.

## What is verified against real services

| Service | Evidence |
|---|---|
| GitHub | attachment round trip, §17.2's privacy refusal against a real PUBLIC repository, and the §17.4 backup round trip on a private one (D170–D172) |
| S3 / Alibaba Cloud OSS | both roles — the §13.11 attachment target and the §17.4 backup target (D175–D176); the first live call found a defect that had made EVERY S3 request fail (D174) |
| Unsplash | search, attribution, the mandatory download report (D164), and the browse mode's topic endpoints (D182) |

Coverage at this release: **1458 Rust tests passed / 0 failed / 12 ignored**, **165 frontend tests**, **52/52 site
checks**, clippy **0 warnings**, and a test matrix of **273 rows, 253 at I, 0 at P**.

## What this release does NOT do — stated rather than implied

- **§5.9 / H10:** the work poster is **a file the user chooses**, not a capture. The contract's §10.3 asks for
  "a screenshot **or a placeholder**", which is met; automatic capture needs a headless browser and that
  dependency decision was declined by the owner.
- **Pexels** is implemented and tested but **no new user can obtain a key** (the vendor paused issuance, §5.12).
  Existing keys work. Do not "fix" this by inventing one.
- **Lsky Pro has never contacted a live instance** (§5.8): its adapter is built from the vendor's own product page
  and is unit-tested, but no `live_lsky.rs` exists.
- **Cloudflare and EdgeOne** deploy targets have never been contacted (§5.8) — no accounts.
- **Language gaps:** the S3 failure sentence a user sees when an upload fails (`veloct-s3::refused()`) and a
  handful of other validators still return English inside an otherwise Chinese UI; the list is in the `1.0.0`
  commit message and in D184's neighbours (§5.10.1).
- **Signing:** see above. For distribution to strangers, buy a certificate; no code change substitutes for it.

## Reproducing this build

PROCESSING.md §2.15 is the procedure, in order, including the two steps that bite: a signed build signs the
committed sidecar launcher **in place** (the script now restores it, and `build-sidecar.ps1 -Check` is the
assertion), and signing `target/release/<app>.exe` on its own does nothing durable because the next build copies
the unsigned file back.