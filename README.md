# Veloct

A local-first, single-user, single-site desktop workbench for building and publishing one static
personal website.

`handoff.md` is the product contract and it is locked. This README explains how the repository is
arranged, how to build and test it on this machine, and where the decisions that `handoff.md`
leaves open are recorded.

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
| Site, deployment, AI configuration | SQLite |
| A release | `.veloct/releases/<id>/release.json` + the git tag `release/<id>` + its commit |
| Attachment identity | the remote object record plus the Markdown AST reference set |
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
  veloct-process     sidecar supervision (ports, pids, restart, tree-kill)
  veloct-release     release manifest, build inputs, target snapshot, fingerprint, publish
  veloct-deploy      logical targets with main/works sub-targets and adapters
  veloct-ai          providers, per-feature model mapping, global fallback
  veloct-attach      cache pool, reference set, dedup, storage adapters
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
# Rust
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop
powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 check -p veloct-desktop

# Frontend and site (npm.ps1 is blocked here, so use npm.cmd)
npm.cmd install
npm.cmd run build
```

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
| `handoff.md` | The locked product contract. Always wins. |
| `docs/spec/00-architecture.md` | ADRs: the technology and layout choices `handoff.md` leaves open. |
| `docs/spec/01-data-model.md` | On-disk layout, frontmatter contract, SQLite schema, status machines. |
| `docs/spec/02-test-matrix.md` | Every locked behaviour mapped to the test that proves it. |
| `docs/spec/03-upstream-facts.md` | Live-verified library versions and API shapes, plus what is unverified. |

`docs/spec/02-test-matrix.md` is the completion checklist. A clause stays `P` (planned) until a
test exists **and** passes in a recorded command; it is never marked done from intention alone.

## Honesty rules this project holds itself to

* No silent failure. A condition the app cannot satisfy becomes an explicit, explained error.
* No invented vendor APIs. An integration that could not be verified is a manual/export target
  labelled as such in the UI, not a fake success.
* No secret in SQLite, a config file, a log, or a backup.
* A user cancelling an AI request is `CANCELLED`, not an error, and never triggers the fallback.
* A release is rebuilt from its own frozen inputs, never from the current working tree.
