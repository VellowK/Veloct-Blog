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
  veloct-github      the GitHub Contents API (shared by section 13.13 and section 17.4)
  veloct-s3          S3 / S3-compatible over SigV4 (shared by section 13.11 and section 17.4)
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

## ADR-0013 — Knowledge reference cards are built on the client, from a build-time payload

*Measured against the installed Astro 7.3.5, not assumed.*

`handoff.md` §3.3 requires the knowledge reference card to be a structured construct, and the
remark plugin does emit a real element for it. The natural next step would be Astro's
`<Content components={{ ... }} />` prop to substitute a Veloct component for that element.

**That prop does not work for plain Markdown on Astro 7.3.5.** Verified empirically on this
checkout: overriding `h2` with a probe component, overriding a custom `aside` element with one,
and passing a named slot into the rendered content all produced the unmodified Markdown output.
The remark plugin's emitted elements and their `data-veloct-*` attributes DO survive, so the
construct reaches the page; only the component substitution does not.

Decision:

- The remark plugin keeps emitting the structured element with its resolved target. That is the
  part §3.3 actually mandates, and it is what makes the reference machine-readable.
- The page resolves every reference at **build time**, against the current knowledge text (§9.4),
  and emits the results as a JSON payload in the document.
- One small script per page builds the card DOM from that payload. It is the only client-side
  rendering in the template, and it exists solely because the documented server-side mechanism is
  unavailable.
- Degradation is visible rather than silent: without scripting the element and the payload are
  still in the document, so a citation is never quietly lost; and a reference that did not resolve
  renders as an explicit broken card rather than being hidden (§9.3).
- The knowledge page tags each prose block with its paragraph ordinal (a rehype step, because a
  remark paragraph's `hProperties` also do not survive Astro's rendering), so clicking a card opens
  the *cited* paragraph. The match is positional, never textual: matching on text would move the
  highlight the moment the author reworded the paragraph, which is the guessing §9.3 forbids.

If a future Astro release makes `components` work for Markdown, the payload and the script can be
deleted and the cards rendered server-side with no change to the Markdown or the contracts.

## ADR-0014 — The main site is emitted to `dist/site`, the work bodies to `dist/works`

`handoff.md` §10.5 and §14.1 name those two roots. This is not merely cosmetic: the shell page route
is `/works/<slug>/` and the work body is copied to `dist/works/<slug>/`. When the main site was
emitted straight into `dist/`, those two paths collided and the work body **overwrote the shell
page**, destroying the very page that provides the sandboxed embed. Setting `outDir` to `dist/site`
and telling the build integration the absolute `dist` root removes the collision, and a check in
`scripts/verify-build.mjs` asserts the two roots stay separate.

## ADR-0015 — The GitHub Contents API is one crate, shared by two unrelated contracts

`handoff.md` names GitHub twice, for two products that share nothing but a protocol:

| Clause | What GitHub is there | What matters |
|---|---|---|
| §13.13 | an **attachment storage target** | mutable public files, dedup, a public URL prefix |
| §17.4 | a **cloud backup target** | a PRIVATE repository, listing, retention (§17.6) |

Decision: the protocol lives once, in **`crates/veloct-github`** (`GitHubContents`). It owns the
contents URL, the `Authorization`/`Accept`/`X-GitHub-Api-Version`/`User-Agent` headers, the base64
body, the two-step delete, the directory listing, and the `private` flag. It owns **no policy**: a key
format, a listing's meaning and whether a public URL is derived at all stay with the dependent.

Reasons, in the order they mattered:

- **A second copy of a protocol is a second chance for it to be wrong.** This project removed a
  duplicate §17.5 idle rule one round earlier for exactly this reason.
- The extraction is PROVEN faithful by what did not change: `veloct-attach`'s 184 tests, including the
  ones that assert exact URLs and request bodies, passed untouched because `GitHubStore` now delegates
  and the moved names are re-exported.
- The dependency is a leaf and points in one direction: `veloct-attach` and `veloct-backup` both depend
  on `veloct-github`, which depends only on `veloct-core`. Neither storage crate depends on the other.

Consequences:

- `CONTENTS_API_MAX_BYTES` (100 MB) is stated where the protocol is, because the failure it prevents is
  an out-of-memory crash rather than a clean HTTP error — the body is the object base64-encoded inside
  JSON. `veloct-backup` refuses above it, with no request, and says a large project needs a target that
  streams.
- Errors from this crate are feature-neutral (`VeloctError::Other`). Each dependent adds its own
  classification at its own boundary, because a shared client cannot name one contract's error kind
  without mislabelling the other's.
- `put_on_branch` / `delete_on_branch` exist because the two dependents know the branch differently:
  §13.13's target has a user-configured one, while §17.4's has none — asking a user to name a branch for
  a backup repository would be inventing configuration, so the repository's own default is used.

## ADR-0016 — S3/S3-compatible is one pure-Rust client; OpenSSL stays excluded (**implemented**)

`handoff.md` names S3 twice — §13.11 as an **attachment storage target** and §17.4 as a **cloud backup
target** — and "S3-compatible" is the operative word: MinIO, Cloudflare R2 and Backblaze B2 all speak this
API at a user-supplied endpoint. As with GitHub in ADR-0015, the protocol lives **once**, in
**`crates/veloct-s3`**, and the two dependents own their own policy.

Decision, taken before the code and unchanged by writing it:

- The signature is **RustCrypto `hmac` + the existing `sha2`**, and requests go through the existing
  `veloct_core::http` seam. **No `openssl-sys`, no vendor SDK.** The root `Cargo.toml` excludes git2's
  `https`/`ssh` features for exactly this reason (they pull OpenSSL onto Windows/MSVC), and a
  general-purpose S3 SDK can drag in the same stack. XML for the `LIST` response is `quick-xml`, also
  pure Rust. Both are new dependencies and both are justified against that standing reason.
- The payload hash is **computed**, not the `UNSIGNED-PAYLOAD` literal. The literal is only valid over
  TLS, and a self-hosted MinIO is usually plain `http://`. The file is streamed
  (`HttpBody::File`) and hashed in a first bounded-chunk pass, so memory does not scale with the archive.

What the code settled that the decision could not:

- **The addressing style is derived from the configuration.** No endpoint → `S3Config::aws`, virtual
  hosted (`bucket.s3.<region>.amazonaws.com`). A user-supplied endpoint → path style (`host/bucket/key`),
  because that is what a self-hosted service expects and getting it wrong is a signature failure rather
  than a 404 a user could read. An endpoint may carry a base path, and that path is signed because S3
  signs the path it is given.
- **The access key ID is configuration; only the SECRET key is a credential.** §12.2 lists "S3 Secret"
  among the values that may never enter SQLite or a config file. The access key ID authenticates nothing
  alone — AWS prints it in the console and it travels in every request's `Credential=` field — so it is a
  column/field, and the secret goes to the OS store behind `credential_ref`. Recorded here because the
  distinction is the kind of thing that gets copied wrongly in either direction.
- **The timestamp is injected.** A SigV4 signature is a function of its timestamp, so `Arc<dyn Clock>`
  reaches both adapters and a test pins the instant to reproduce a published vector (ADR-0002).

**Verification.** The tests are AWS's OWN published examples, which the documentation offers "as a test
suite to verify your code" — GET Object, PUT Object, GET Bucket Lifecycle and List Objects, whose
canonical request, `StringToSign` and final signature are all published
(<https://docs.aws.amazon.com/AmazonS3/latest/developerguide/sig-v4-header-based-auth.html>). All four
reproduce exactly, in `veloct-s3::sigv4`, along with the URI-encoding rules, the scope binding and the
signing-key derivation. Nothing here has been run against a live bucket, and the matrix says so.

Status: **implemented** for both §17.4 (backup) and §13.11 (attachment storage);
`unimplemented_reason` now answers `None` for S3 in both crates, which is asserted rather than assumed.

## ADR-0017 — SFTP is REMOVED from §13.10, at the owner's decision; §13.10 keeps the directory target

`handoff.md` is the locked contract, and this note records the change made to it at the owner's explicit
request: **the SSH/SFTP attachment target is gone.** The owner's words were that it "really is useless", and
the evidence supports that reading rather than merely permitting it:

- **It was never implementable without a new dependency — and the reason recorded for that was wrong.**
  `PROCESSING.md` §5.3 said an SSH implementation "pulls OpenSSL onto Windows/MSVC", but the comment that
  reason came from is attached to **git2** in the root `Cargo.toml` (`git2`'s `https`/`ssh` features). An
  SFTP client has its own choices: `ssh2` → `libssh2-sys` (a C library), and **`russh` 0.63 → no OpenSSL and
  no libssh2 at all** (RustCrypto throughout; `ring`/`aws-lc-rs` are OPTIONAL features, not defaults).
  Measured on 2026-10-02: this workspace already builds `rustls` + `aws-lc-rs` + `ring`, and its `Cargo.lock`
  already contains `tokio` and `tokio-util` — so neither the crypto nor the runtime would have been new.
- **It was the last kind with no adapter, and the target nobody could use.** There is no live SFTP server in
  this environment, so every SFTP claim was a `NOT_CONFIGURED` plus a paragraph of prose, and §28's H15 was
  PARTIAL for it. A protocol that can only ever be reported is a cost with no payer.
- **The 远程目录 the clause named is already covered** by a target that DOES work: `StorageKind::Local`,
  §13.10's "server storage" where the server is this machine — also the target the whole upload-and-delete
  path is proven against.

**What §13.10 is now:** the local / server DIRECTORY target — a path, not a protocol. No SSH client, no host,
no port, no credential; the directory comes from the project root and the target id, and the user configures a
path prefix and a public URL prefix. The clause keeps its number and its neighbours.

**Compatibility rule, and it is deliberate: an old record still LOADS.** The kind is removed from
`StorageKind::ALL` and from `StorageKind::parse`, so it can never be OFFERED or configured again. The enum
variant is RETAINED and marked load-only, because `read_storage_targets` parses the whole
`.veloct/storage-targets.json` as one `Vec<StorageConfig>`: dropping the variant would make ONE stale record
fail the ENTIRE list, taking a user's other targets with it. So a target saved before the removal still loads,
is reported as removed with a reason a user can act on, can be DELETED, and cannot be re-saved. Its SSH fields
(`host`, `port`, `username`) are kept so that record round-trips instead of having its configuration silently
erased.

**What was NOT removed, and why:** the "a kind this build cannot use is LISTED and REPORTED, never silently
missing" mechanism (ADR-0010). It still has exactly one user — the load-only record above — and it is the shape
Pixabay's refusal and Lsky Pro's former gap both relied on. A future kind that cannot be implemented gets that
treatment rather than a deleted row.

Status: **removed**, 2026-10-02, at the owner's request.





## ADR-0018 — An S3 endpoint says how its bucket is addressed, and SigV4's date is second precision

**Status:** accepted (2026-10-02). Both halves come from ONE live run against a real Alibaba Cloud OSS bucket,
and the second half is a DEFECT that run found rather than a design choice. It is the first time a live vendor
call in this repository produced a code change instead of a confirmation.

### The context

ADR-0016 decided that S3/S3-compatible storage is one pure-Rust client with hand-rolled SigV4, and that a
user-supplied endpoint is addressed PATH-STYLE (`https://host/bucket/key`) because "that is what a self-hosted
service almost always wants". Every S3 test asserted against `FakeHttpClient`, so no part of that decision had
ever met a real service (PROCESSING.md §5.8).

A bucket was then offered to verify against (`vellow-chengdu-archieve`, region `cn-chengdu`), and FOUR facts
were measured rather than assumed:

* the `-internal` endpoint (`oss-cn-chengdu-internal.aliyuncs.com`) is **unreachable** from outside Alibaba's
  network — a workstation must use the public one;
* a path-style bucket request is refused: **`403 SecondLevelDomainForbidden`**, "The bucket you are attempting
  to access must be addressed using OSS third level domain";
* SigV4 with the service scope **`oss`** is refused: **`400 InvalidArgument`**;
* the SAME request with the scope **`s3`** answers **200** and a real `ListBucketResult`.

So Veloct's SIGNING was already correct (it signs `s3`) and only its ADDRESSING was wrong — but the request that
reached OSS through Veloct failed for a THIRD reason, which is the defect below.

### The decision

**1. The endpoint states the addressing, with `{bucket}`.** A `{bucket}.` immediately before the host switches
to virtual-hosted addressing; the placeholder is removed at construction and `host()` puts the bucket back:

```text
https://s3.example.com                        path style:     https://s3.example.com/bucket/key
https://{bucket}.oss-cn-chengdu.aliyuncs.com  virtual-hosted: https://bucket.oss-cn-chengdu.aliyuncs.com/key
```

A placeholder anywhere else — inside the host but not at its start, in the path, or twice — is **refused by
name**. That is not decoration either: an endpoint like `https://oss-{bucket}.example.com` would otherwise be
silently rewritten into a host that does not exist, and a wrong host is a wrong signature rather than a visible
error.

**Why the user decides, and the code does not guess.** A suffix rule ("if the host ends in `aliyuncs.com`, go
virtual-hosted") would put a vendor's addressing requirement inside the client, where it cannot be corrected
without a release, and it would age badly the moment a provider changed. The endpoint is configuration the user
already writes, so the placeholder costs no new field and no new UI — and the error message can teach it.

**2. SigV4's date is formatted, never assembled by deleting characters.** `amz_timestamp` builds `x-amz-date` by
parsing the RFC 3339 instant and formatting it as `YYYYMMDDTHHMMSSZ` in UTC
(`veroct_core::clock::format_amz_timestamp`). `format_release_stamp` — which needed exactly the same shape for
release ids — now DELEGATES to it, so the two cannot drift apart.

### The defect the live run found

The first live request through this client was answered:

```
HTTP 403, AccessDenied: OSS authentication requires a valid Date.
```

Not a clock problem: a `curl` probe signed from the same machine seconds earlier was accepted. The value Veloct
sent was:

```
x-amz-date: 20261002T034316.5888216Z        <- twenty-four characters
```

`amz_timestamp` had been `rfc3339.replace(['-', ':'], "")`, which is correct for the string a FIXED test clock
produces (`2013-05-24T00:00:00Z` → `20130524T000000Z`) and wrong for every real one: a system clock carries
sub-second digits, and deleting the separators keeps them. `format_rfc3339` does end in `Z` (so the first
suspicion — a `+00:00` offset — was wrong), which is exactly why the malformed value looked plausible.

**Every test in the workspace missed it, and that is the part worth keeping:** the SigV4 unit tests inject an
already-correct `20130524T000000Z`, and the adapter tests use a `FixedClock` whose instant has zero nanoseconds,
so no test input ever contained a fraction. **No S3 request this application made could have succeeded against
any provider**, AWS included, because AWS validates the same format. It was found in the first minute of the
first live S3 call this repository ever made — §5.8's recorded gap was not merely about confidence; it was
hiding a total failure of the feature.

Two tests now stand where the blind spot was, and both are deliberate about their input: a unit test in
`veroct-core` that feeds an instant WITH nanoseconds and a non-UTC offset, and one in `veroct-s3` that reads the
SYSTEM clock — because a fixed clock is what hid the bug.

### The consequences

* A MinIO user types `https://127.0.0.1:9000` and gets exactly the behaviour ADR-0016 described. An OSS user
  types `https://{bucket}.oss-cn-chengdu.aliyuncs.com` and gets the addressing OSS demands. Neither needs a code
  change, which is the point.
* `Addressing` and `BUCKET_PLACEHOLDER` are public, so both adapters (attachment and backup) inherit the
  behaviour from the one client they share (ADR-0015) instead of choosing it separately.
* Both rules are verified against the live service in BOTH roles: `crates/veloct-attach/tests/live_s3.rs` and
  `crates/veloct-backup/tests/live_s3.rs` write, read and remove a real object under a dedicated prefix, and
  neither prints the credential.
* **A live test found a shipped defect that the whole suite could not.** That is the strongest argument this
  repository has produced for §5.8's claim that the live gap is the biggest honesty gap.