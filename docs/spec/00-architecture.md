# Veloct — Architecture Decisions (ADR set)

> Status: **accepted for implementation**.
> Authority: `handoff.md` is the locked product contract and always wins. This file only
> chooses *technology and file layout* inside the freedom that handoff §26 explicitly grants.
> If this file ever contradicts `handoff.md`, `handoff.md` wins and this file is a bug.

## ADR-0001 — The desktop app and the managed project are two different directories

This is the single most important structural decision, and it removes a whole class of
ambiguity in the handoff:

| Thing | Lives where | Role |
|---|---|---|
| **Veloct application** | this repository (`D:\Veloct-Blog`) | The Tauri app + Astro site template + sidecar. App code, not content. |
| **Veloct project** | any user-chosen directory | Markdown, `works/`, SQLite, git repo, `.veloct/releases/`, `dist/`. One project = one site (handoff §1.1). |

The app is installed once. It can open any one project directory. "Creating a project"
means: pick an empty/other directory, and the app materialises the project scaffold in it.

Consequences:

- The app bundle ships a **site template** (`site/` in this repo). At project creation the
  template is copied into the project so the project is self-contained and reproducible.
- The project records which template it was created from in `.veloct/project.json`
  (`site_template_version`). Upgrades are explicit, never silent.
- Every Release **freezes the exact site template files** it was built with, so a historical
  Release can be rebuilt even after the app upgrades its bundled template
  (handoff §14.6, §14.13, §14.14, §23.4).

## ADR-0002 — Veloct is a Cargo workspace; the Tauri shell is a thin adapter

`handoff.md` §23/§24/§25 require automated tests over domain rules that have nothing to do
with the GUI (release fingerprints, attachment dedup, WIP routing, AI fallback, backup
exclusion). Those rules must be testable **without** a WebView, without a display, and
without Tauri.

Layout:

```text
crates/
  veloct-core        domain primitives: ids, errors, paths, secret redaction, project locks
  veloct-markdown    frontmatter, mdast, paragraph ids, directives, index building, HTML
  veloct-storage     SQLite: migrations, repositories, rebuildable index
  veloct-content     content model + source-of-truth read/write, WIP rules, atomic writer
  veloct-git         git2 wrapper: change detection, publish commits, release tags, conflicts
  veloct-process     child-process supervisor (sidecar runtime, ports, pids, restart, cleanup)
  veloct-release     release manifest, build inputs, target snapshot, fingerprint, dist roots
  veloct-deploy      logical deployment targets: cloudflare / edgeone-cn / edgeone-intl / 1panel
  veloct-ai          providers, feature→(provider,model) mapping, global fallback, SSE streaming
  veloct-attach      cache pool, 3-axis state, reference set, dedup, storage adapters
  veloct-backup      consistent snapshot, secret scan, archive, cloud targets, restore
  veloct-app         orchestration: services, DTOs, Tauri command handlers
src-tauri/           thin Tauri 2 shell (windows, capabilities, command registry)
```

Rules:

- No crate below `veloct-app` may depend on Tauri.
- `veloct-app` exposes `#[tauri::command]` functions; `src-tauri` registers them.
- All cross-boundary data is `serde` DTOs; no raw domain types leak into JS.
- Time, RNG and the filesystem are injected where behaviour must be deterministic
  (release builds must be reproducible — handoff §14.9).

## ADR-0003 — Astro is consumed as a real project; the contract is the content directory

`handoff.md` §5 forbids a fake second renderer. Therefore:

- There is exactly **one** Astro project (`site/`), used by both `astro dev` (preview) and
  `astro build` (release). Same config, same content collection, same components, same
  Tailwind, same remark pipeline.
- The app never parses Markdown for preview. It writes Markdown to disk and lets Astro render.
- Content directories are project-root `content/` and `knowledge/`, wired into Astro through
  the Content Layer `glob()` loader with a relative `base`. `content/` is the Markdown truth
  (handoff §2.3, §3.4); `works/` is the work truth (handoff §10.1).
- Preview = `astro dev` on a dynamically chosen free port, spawned by `veloct-process`.
  HMR gives the live refresh handoff §5.4 requires; a production `astro build` is never run
  just to preview.

## ADR-0004 — Local preview isolates works on a second, genuinely different origin

`handoff.md` §10.4 forbids letting work code share the main site's origin, and §10.4's last
paragraph forbids claiming strong isolation when it does not exist.

- Main site preview: `http://127.0.0.1:<main-port>`.
- Works preview: `http://127.0.0.1:<works-port>` from a second sidecar that serves the work
  bodies as plain static files. Different port on the loopback interface is a different
  origin, so this is genuine cross-origin isolation, not a simulation.
- The work shell page embeds `http://127.0.0.1:<works-port>/<slug>/` in a `sandbox`ed
  iframe **without** `allow-same-origin` (handoff §10.4.3).
- In production the same shell embeds the configured Works Origin. If a deployment target
  has no Works Origin configured, Veloct labels that target **degraded** in the UI and
  refuses to run a work as a same-origin top-level document (handoff §10.4 last paragraph).

## ADR-0005 — Node.js is a bundled sidecar, never a user prerequisite

`handoff.md` §5.2 requires that users do not install Node themselves. So:

- A pinned Node runtime is bundled as a Tauri `externalBin` sidecar together with the
  project's `node_modules` installed at project-creation time (or vendored with the template).
- The app resolves the node binary from the sidecar path, never from `PATH`.
- The managed project's `package.json` + lockfile are committed, so a historical Release can
  be rebuilt with the same resolved dependency graph (handoff §14.6: lockfile hash).

## ADR-0006 — Two static publish roots from one build

`handoff.md` §10.5/§14.1 mandates `dist/site/` + `dist/works/` from a single build.

- `astro build` writes the main site to `dist/site/` (`build.outDir`).
- An Astro integration in the `astro:build:done` hook emits the work bodies into
  `dist/works/<slug>/` and the frozen `works-manifest.json`. This runs inside the same
  build, so there is exactly one Astro build per Release.
- `dist/` holds exactly one current build at a time (handoff §14.1); historical rebuilds go
  to a temp directory and are deleted (handoff §14.13).

## ADR-0007 — Secrets only ever live in the OS credential store

`handoff.md` §12/§17.1 are absolute. Therefore:

- SQLite stores `credential_ref` strings (opaque handles), never values.
- `.veloct/**`, logs, `release.json`, deployment snapshots and backup archives are all
  produced by serializers that go through `veloct_core::redact`, and a test asserts that a
  known secret value never appears in any produced artifact.
- Backup gets a secret scan over the working tree **and** git objects before any upload
  (handoff §17.3).

## ADR-0008 — Truth-source table (one place, no exceptions)

| Data | Truth source | Derived / cache | Never |
|---|---|---|---|
| Article / page / knowledge body | `content/**/*.md`, `knowledge/**/*.md` | SQLite index, AST cache | stored in SQLite as the body |
| Work metadata | `works/<id>/work.json` | SQLite index | duplicated into SQLite as truth |
| Work body | `works/<id>/site/` | `dist/works/<slug>/` | modified by build |
| Site/app/deployment/AI config | SQLite | — | YAML |
| Release | `.veloct/releases/<id>/release.json` + tag `release/<id>` | SQLite release index | SQLite alone |
| Release build inputs | `.veloct/releases/<id>/{build-inputs,target-snapshot}.json` + `site-template/` | — | recomputed from current DB |
| Attachment identity | remote object record + Markdown AST reference set | SQLite | "is referenced" boolean |
| Credentials | OS credential store | — | SQLite, config, logs, backups |

## ADR-0009 — Deterministic release builds

`handoff.md` §14.8/§14.9 require `build_fingerprint` stability and semantic reproducibility:

- `build_inputs_hash` excludes `commit_sha` and any secret.
- `build_env_hash` hashes only an explicit allow-list of build-affecting env vars; hashing
  the whole environment is forbidden.
- All time-derived site fields (`RSS pubDate`, sitemap `lastmod`, footer build stamp) read a
  single injected `RELEASE_BUILD_TIME`; a rebuild of release *R* injects *R*'s frozen time.
- A normalisation step sorts routes, assets, sitemap and RSS entries and strips volatile
  fields so two builds of one Release compare equal (handoff §14.9, §23.6).

## ADR-0010 — Honest capability reporting over invented behaviour

The handoff repeatedly forbids silent failure and false claims. Applied to the build:

- Every deployment adapter reports `configured: true|false` and `verified: true|false`.
  An adapter whose vendor API could not be verified is implemented as a **manual/export**
  adapter and the UI says so; it never pretends to have deployed.
- Unimplemented-but-contracted behaviour is represented as an explicit `NotConfigured`
  error carrying a human explanation, never as a silent no-op or a fake success.

## ADR-0011 — Astro 7 pipeline: pin the unified processor, not the new default

*Decided after reading the live registry and docs (see `docs/spec/03-upstream-facts.md`).*

Astro's current stable is **7.3.5**, whose default Markdown processor is a new engine
("Sätteri"); `@astrojs/markdown-remark` is **no longer installed by default**. `handoff.md`
§3.3 requires custom blocks (knowledge reference cards, download cards, hyperlink cards, page
sections) to be implemented as **structured AST / directives, never as fragile string
regex**. That requirement is best served by the well-documented directive pipeline.

Therefore the site pins:

- `markdown: { processor: unified({ remarkPlugins: [remarkDirective, veloctDirectives] }) }`
  where `unified` comes from `@astrojs/markdown-remark` and `veloctDirectives` is our own
  plugin. This is the documented escape hatch for projects that depend on remark plugins.
- Custom blocks are emitted as **HTML element names** with `data.hName` (for example
  `aside` + `data-veloct-directive`), and mapped through the documented
  `<Content components={{ ... }} />` prop. We deliberately avoid relying on undocumented
  custom (capitalised) tag mapping, per the research report's §9.4 caveat.
- Tailwind is added through `@tailwindcss/vite` 4.x (CSS-first). `@astrojs/tailwind` is not
  used: its published peer range stops at Astro 5.
- `compressHTML`, `build.format` and `trailingSlash` are set **explicitly**, because their
  defaults changed in v7 and they change emitted bytes — which a reproducible Release diff
  would otherwise report as a change (handoff §14.9).
- `site/` keeps exactly one `outDir` (`dist/`); the integration hook `astro:build:done`
  (argument is `pages`, not `routes`) writes the works publish root, satisfying
  handoff §10.5/§14.1 without a second Astro build.

## ADR-0012 — EdgeOne is a manual/export adapter until its upload API is verified

`handoff.md` §14.10 requires EdgeOne · 中国大陆 and EdgeOne · 国际 as logical automatic
deployment targets. Their upload API could not be verified from an authoritative source.
Inventing endpoints would produce an adapter that reports false success, which ADR-0010
forbids and handoff §28 ("key failure paths must have explicit error states") rules out.

Decision:

- `veloct-deploy` defines one `DeployAdapter` trait and implements the full
  sub-target/status/roll-up/retry machinery against it, tested with a fake HTTP server.
- The **Cloudflare Pages** adapter is fully implemented against the documented direct-upload
  API, plus an optional Wrangler-CLI strategy.
- The **EdgeOne** logical targets are implemented as **export targets**: they are selectable,
  they produce the exact `dist/site` + `dist/works` payload the user uploads, they record a
  `DeploymentRun` with an explicit "manual upload required" status, and the UI labels them
  `manual` rather than `automatic`.
- When a verified EdgeOne API definition is supplied, only the adapter body changes; the
  snapshot, fingerprint, sub-target and retry contracts are already in place and tested.

This keeps handoff §26's locked "three logical deployment targets" intact while refusing to
fabricate a vendor integration.

