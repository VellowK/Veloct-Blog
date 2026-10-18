# Veloct

A local-first, single-user, single-site desktop workbench for building and publishing one static
personal website.

**Next AI: start with [NEXT_AI.md](<NEXT_AI.md>)**, then [PROCESSING.md](<PROCESSING.md>) for current requirements and [UPDATE.md](<UPDATE.md>) for the final tested artifacts. The application repository contains substantial uncommitted/untracked work; the Git HEAD alone is not the handover.

[handoff.md](<handoff.md>) is the locked product contract; explicit later owner requirements take precedence without silently rewriting it. Older [process archives](<docs/archive/>) and earlier verification sections are historical evidence, not current status. This README explains the repository structure and build commands; it does not claim the latest test package was installed or published.

**2026-10-18 handover:** all requested product work is complete; wait for the owner's next task rather than automatically fixing old issues. This handover round edits documentation only, with no new product test/build. The latest functional round is homepage avatar/cover → attachment pool → preferred cloud target. Full-workspace verification has one known Windows install-cancellation test failure; it is not all green. Exact results, boundaries and signed local test packages are recorded in [NEXT_AI.md](<NEXT_AI.md>).

## Two directories, not one

The single most important structural idea (see `docs/spec/00-architecture.md` ADR-0001):

| | Where | What it is |
|---|---|---|
| **The Veloct application** | this repository | Tauri app, Rust domain crates, the Astro site template, the Node sidecar. Application code. |
| **A Veloct project** | any directory the user picks | Markdown content, `works/`, the SQLite database, the git repository, `.veloct/releases/`, `dist/`. Exactly one site. |

Creating a project materialises the scaffold and copies the bundled Astro template into it, so the
project is self-contained and its releases stay reproducible.

## Truth sources (never ambiguous)

| Data | Truth source |
|---|---|
| Article / page / knowledge body | Markdown files under `content/` and `knowledge/` |
| Work metadata and body | `works/<stable-id>/work.json` and `works/<stable-id>/site/` |
| Site appearance / profile-home settings | The user's project `data/site.json`; managed by `SiteSettingsService`, preserving unknown keys |
| GitHub activity | Explicitly synced `data/github-activity.json` snapshot; static builds do not fetch it |
| Deployment targets | User project `.veloct/deploy-targets.json`; credentials are OS-store handles, not secrets |
| Attachment storage targets / sync | User project `.veloct/storage-targets.json` / `.veloct/attachment-sync.json`; credentials stay in the OS store |
| Removed AI page creation | No desktop UI/API/commands remain; the shared crate and historical data are retained, not an active feature |
| A release | `.veloct/releases/<id>/release.json` + the git tag `release/<id>` + its commit |
| Attachment identity / references | Pool/object records, Markdown AST references and saved homepage avatar/cover references; changing a site image must not erase article references |
| Credentials | the operating system credential store — never SQLite, config, logs or backups |

SQLite's content tables are a **derived index**. Dropping them and rebuilding from Markdown must
produce identical rows, and a test enforces that.

## Repository layout

```text
crates/
  veloct-core        ids, errors, project-relative paths, secret redaction, project locks,
                     time abstraction, and the single HTTP seam
  veloct-markdown    frontmatter contract, mdast, directives, stable paragraph ids, references
  veloct-storage     SQLite migrations, schema, repositories, rebuildable index
  veloct-content     content model, truth-source I/O, WIP rules, work import
  veloct-git         change detection, publish commits, release tags, conflict detection
  veloct-process     sidecar supervision (ports, pids, restart, tree-kill), child liveness and exit codes
  veloct-sidecar     the Astro launcher the app spawns: `dev` / `preview` hand over to Node,
                     `serve <dir>` serves a built directory itself (the works preview)
  veloct-logging     structured events, redaction at the source, rotation, the log page's backend
  veloct-release     release manifest, build inputs, target snapshot, fingerprint, publish
  veloct-deploy      logical targets with main/works sub-targets and adapters
  veloct-ai          retained shared/legacy library; AI page-creation desktop feature was removed
  veloct-attach      cache pool, reference set, dedup, storage adapters
  veloct-github      the GitHub Contents API, once: §13.13's attachment target and §17.4's backup
                     target are one protocol in two contracts (ADR-0015)
  veloct-s3          S3 / S3-compatible over AWS Signature Version 4, once, for the same two
                     contracts (ADR-0016). Pure Rust: no OpenSSL and no vendor SDK.
  veloct-backup      consistent snapshots, secret scan, archive, cloud targets, restore
  veloct-app         orchestration services, DTOs, Tauri commands
src-tauri/           thin Tauri 2 shell (windows, capabilities, command registry)
src/                 React workbench UI (added by the ui-editor workstream)
site/                the Astro project template (added by the site-astro workstream)
docs/spec/           the engineering contracts this repository is built against
scripts/cargo.ps1    the required way to run Cargo on this machine
handoff.md           the locked product contract
```

## Building and testing on this machine

Three environment quirks are handled by `scripts/cargo.ps1`. Use it; a bare `cargo` will fail.

```powershell
# Rust — run the full workspace checks when Rust code changes. This handover does not claim a fresh result.
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --no-fail-fast
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 clippy --workspace --all-targets

# Frontend and site (npm.ps1 is blocked here, so use npm.cmd).
# The root package builds the desktop workbench with `build:ui`; the Astro template builds in site/.
npm.cmd install
npm.cmd run test:ui -- --run
npm.cmd run build:ui
cd site; npm.cmd install; npm.cmd run build
```

The desktop package, in the order that matters:

```powershell
# A green build is not installation evidence: report install/run results only for the exact tested package.
powershell -ExecutionPolicy Bypass -File scripts/build-sidecar.ps1 -Check   # the committed launcher is current
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1        # builds, packages, and lets the BUNDLER sign
# The standalone EXE can be NotSigned after bundling: if so, sign it, then independently check both final files.
# powershell -ExecutionPolicy Bypass -File scripts/sign-artifacts.ps1
powershell -ExecutionPolicy Bypass -File scripts/sign-artifacts.ps1 -Installer -Check
# A signed nst*.tmp is NOT the final installer: require bundler exit 0 and the final file's timestamp/hash.
```

**What the signature does and does not buy** (see PROCESSING.md): the certificate is LOCAL and
SELF-SIGNED, so it proves integrity and gives a publisher name once `src-tauri/signing/veloct-codesign.cer` is
installed into a machine's Trusted Publishers — and it does **not** remove SmartScreen's warning on a machine
that has not done that. That is what a purchased certificate is for, not code.

**The package is signed, and so is what is inside it.** This used to read "it currently covers the SETUP
only": signing `target/release/veloct-desktop.exe` on its own did not survive packaging, so the app inside
came out `NotSigned`. The supported fix is now IN PLACE rather than merely named —
`scripts/build-installer.ps1` discovers the certificate (its `-SignThumbprint`, then `VELOCT_SIGN_THUMBPRINT`,
then the store by subject) and passes Tauri's own `bundle.windows.certificateThumbprint` (plus
`digestAlgorithm` and a timestamp server) to the BUNDLER, so the signature is applied during the build.
Historical installation checks found `veloct-desktop.exe`, `veloct-astro-sidecar.exe` and `uninstall.exe`
`Valid` under `CN=VellowChire Code Signing, O=Veloct`; `veloct-node.exe` keeps its vendor signature
`CN=OpenJS Foundation`. Those are NOT installation results for every newer package: the latest handover
only records final-file signature checks, not a new install/uninstall. The COMMITTED launcher stays
unsigned on purpose because `build-sidecar.ps1 -Check` compares it byte for byte against a fresh unsigned
build. Do not kill a running user application to release locked build files; ask the user to close it.

Why the wrapper exists:

1. `%USERPROFILE%\.cargo\config.toml` replaces crates.io with a mirror that is unreachable from
   this environment. The wrapper uses an isolated `CARGO_HOME` (`.cargo-home/`) pinned to the
   canonical sparse registry. A project-local `replace-with` cannot fix this, because Cargo
   rejects a replacement source that also defines the registry name `crates-io`.
2. Windows has a system HTTP proxy (an xray client) that Cargo's libcurl picks up. The wrapper
   reads it from the registry and passes it explicitly.
3. `cargo` and `rustup` are installed under `%USERPROFILE%\.cargo\bin` and are **not** on `PATH`.

`.ps1` execution is disabled on this machine, hence `-ExecutionPolicy Bypass`.

## Documentation map

| File | Purpose |
|---|---|
| [NEXT_AI.md](<NEXT_AI.md>) | Read first: current completed state, source map, safety boundaries, exact final artifacts, and next-agent steps. |
| [handoff.md](<handoff.md>) | Locked product contract; explicit later owner requirements take precedence. |
| [PROCESSING.md](<PROCESSING.md>) | Current product rules, data-safety boundaries and working discipline; old process details are archived. |
| [ISSUE.md](<ISSUE.md>) | Confirmed failures and evidence gaps first; historical priorities are not an automatic backlog. |
| [UPDATE.md](<UPDATE.md>) | Per-round changes, failed runs and tested artifacts; not every historical count describes the current tree. |
| [Architecture](<docs/spec/00-architecture.md>) | ADRs: technology/layout decisions and later requirement amendments. |
| [Data model](<docs/spec/01-data-model.md>) | On-disk layout, frontmatter contract, SQLite schema, status machines. |
| [Test matrix](<docs/spec/02-test-matrix.md>) | Behaviour-to-evidence map; latest homepage-image rows are D246–D250. |
| [Upstream facts](<docs/spec/03-upstream-facts.md>) | Versioned library/API evidence and explicit integration limits. |
| [Archives](<docs/archive/>) | Historical process snapshots; never treat them as current state or open instructions. |

`docs/spec/02-test-matrix.md` is the completion checklist. A clause stays `P` (planned) until a
test exists **and** passes in a recorded command; it is never marked done from intention alone.

## Honesty rules this project holds itself to

* No silent failure. A condition the app cannot satisfy becomes an explicit, explained error.
* No invented vendor APIs. An integration that could not be verified is a manual/export target
  labelled as such in the UI, not a fake success.
* No secret in SQLite, a config file, a log, or a backup.
* Removed AI page-creation functionality stays removed; a retained shared crate or historical contract is not permission to restore desktop entry points.
* A release is rebuilt from its own frozen inputs, never from the current working tree.
