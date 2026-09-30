# PROCESSING.md — state of Veloct, and what the next agent should do

A **status and orientation document** for whoever picks this up next, written after three consecutive
rounds of work on the cloud targets and the AI settings. It is not a contract: `handoff.md` remains the
locked product contract and always wins, and `docs/spec/*` records the decisions `handoff.md` leaves open.

**Read in this order:**
1. `handoff.md` §28 — the completion definition. It is the only authority on what "done" means.
2. This file — §1 for where the work stands, **§5 for what is genuinely missing**, §7 for what to do next.
3. `README.md` — how to build and test on this machine.
4. `docs/spec/00-architecture.md` — the ADRs, including the two added by these rounds (ADR-0015, ADR-0016).

**Before changing anything**, run §3.1 and make the six numbers match. The working tree is DIRTY (§1.1),
and deciding what to do about that is the first thing this file asks of you, because every number in it
describes uncommitted work.

**The single most useful habit for this repository:** when a contract clause says a feature exists,
`grep` for a caller before believing it. Two of the three rounds here found their work that way — a whole
model (`CloudTarget`, `Provider::list_models`) implemented, tested, and called by nothing.

---

## 1. Where the work stands

| Fact | Value |
|---|---|
| Last commit | `f5c372a Add PROCESSING.md: the state of Veloct for the next agent` |
| Working tree | **DIRTY** — three rounds of work are uncommitted. See §1.1, and decide about it FIRST |
| Rust tests | **1295 passed, 0 failed** (17 workspace crates + `src-tauri`) |
| Frontend tests | **24 passed** (vitest) |
| `clippy --workspace --all-targets` | **0 warnings** |
| Site build verifier | **46/46 checks** |
| Test matrix | **198 rows at `I`, 0 at `P`** (171 + 12 for GitHub + 9 for S3 + 6 for §11.2/§11.5) |
| Shell commands registered | **70** (60 + 6 cloud-backup + 4 AI-model commands) |

Every command above exits **0** on success. That matters and is itself a recent fix — see §2.

**These numbers were re-run at the end of the last round, and NOTHING in the code has changed since** —
the only edit afterwards was to this file. So they describe the current source exactly, and §3.1 should
reproduce them; if it does not, something outside this repository changed and that is the first thing to
find out.

### 1.1 What these rounds changed

Three increments. The first two close §17.4; the third wires §11.2 and §11.5.

1. **GitHub** — the pre-existing `CloudTarget` / `BackupTargetAdapter` model got a real adapter, the
   targets moved into SQLite's `backup_targets`, each run into `backups` (§22.7), and §17.2 is now
   VERIFIED against the repository on every upload rather than only declared by the user.
2. **S3 / S3-compatible** — a new `veloct-s3` crate implementing AWS Signature Version 4, plus adapters
   for BOTH §17.4 (cloud backup) and §13.11 (attachment storage). Both of §17.4's targets now exist.
3. **AI model discovery and the connection test** — `Provider::list_models`, `merge_models`,
   `test_connection` and `veloct_storage::replace_models`/`read_models` went from **zero call sites
   above `veloct-ai`** to four commands and a settings panel. Wiring it up **exposed a real §18.1 leak**
   (§2.8): a provider's 401 body echoes the key, and that body was becoming the error message.

**What is still NOT done is in §5**, and the honest summary is: SFTP and Lsky Pro have no adapter, no
remote operation has ever touched a live service, and the attachment panel still has no target FORM
(§5.5). Nothing above should be read as a claim that Veloct is complete.

**Uncommitted on purpose.** 104 new tests pass (1191 → 1295) and clippy is clean, but a commit is the
user's call, so `git status` is the truth and this file must not claim a commit that does not exist.
`git status --short` shows **34 entries** — 28 modified files and 6 new paths. The many `*.log` files in
the working directory are this session's captured verification output, and `.gitignore` line 25 ignores
`*.log`, so they are NOT part of the change set; they are safe to read (they are the raw evidence for the
numbers in §1) and safe to delete.

**The files these rounds added**, so the next agent knows what is new without reading the whole diff:

```text
crates/veloct-github/                 NEW  the GitHub Contents API, shared by §13.13 and §17.4
crates/veloct-s3/                     NEW  SigV4 + an S3 object client, shared by §13.11 and §17.4
crates/veloct-backup/src/github.rs    NEW  the §17.4 GitHub cloud backup target
crates/veloct-backup/src/s3.rs        NEW  the §17.4 S3 cloud backup target
crates/veloct-attach/src/s3.rs        NEW  the §13.11 S3 attachment target
crates/veloct-app/src/cloud_backup.rs NEW  cloud targets in SQLite + the §22.7 run record
```

Everything else is edits to existing files; `git diff --stat` lists them, and the two docs that record
the decisions are `docs/spec/00-architecture.md` (ADR-0015, ADR-0016) and `docs/spec/02-test-matrix.md`
(rows `C11`, `D104`–`D129`, each naming the test AND the command that was run).

**The matrix at 198 `I` / 0 `P` is a claim about recorded evidence, not proof that §28 is met.**
It has been wrong in that direction before: at one point it read 93 `I` while the desktop
workbench did not exist at all. When you find a gap, the matrix row is what is wrong.

---

## 2. Things that were fixed and must not regress

These are the failure modes this session actually hit. Each one cost real time.

### 2.1 `scripts/cargo.ps1` reported success as failure

The script sets `$ErrorActionPreference = 'Stop'` for its own guards. Under that preference a
native command's **stderr becomes a terminating PowerShell error**, and cargo writes its progress
(plus an MSVC linker line) to stderr. So `build -p veloct-sidecar` **succeeded and exited 1**,
while a genuine failure exited 101.

For most of this project every verification **parsed output instead of checking exit codes**,
which is why it went unnoticed. The preference is now relaxed for the cargo call alone and
`$LASTEXITCODE` decides. **Check exit codes now — they are trustworthy.**

### 2.2 The committed sidecar launcher goes stale silently

`src-tauri/tauri.conf.json` declares `externalBin: ["binaries/veloct-astro-sidecar"]`, and Tauri
resolves that at runtime to `binaries/veloct-astro-sidecar-<target-triple>.exe`. Because the
launcher must exist at that exact path it is **committed**, and therefore a Rust change to
`crates/veloct-sidecar` does **not** rebuild it.

It was stale (2.60 MB against a 2.66 MB build), silently missing the package-`bin` resolution
that fixed "a site with its dependencies installed is reported as missing its template".

- Rebuild and install: `scripts/build-sidecar.ps1`
- Check without installing: `scripts/build-sidecar.ps1 -Check` (exits non-zero when stale)

**Being accurate about the cost:** a stale launcher still RUNS — it is a self-contained Rust
binary, so it does not fail with a missing symbol. It silently has older behaviour, which is
worse. Run `-Check` after touching `crates/veloct-sidecar`.

### 2.3 `VeloctError`'s hint is not in its `Display`

`error.to_string()` gives the message only. The actionable guidance is
`error.hint() -> Option<&str>`. Several tests in this session asserted on `to_string()` and were
wrong. Use `.hint()`.

### 2.4 The attach transaction once uploaded all zeros

`AttachmentTransaction::upload` sent `vec![0u8; size_bytes]` — the right number of bytes, all
zero — because it had no path to the cache. The upload succeeded, the size matched, and the
picture was blank. It now reads the cache through `with_cache_root`, and a missing or
size-mismatched cache file leaves the attachment `FAILED` and retryable rather than sending zeros.

**Lesson worth generalising:** a test double that records only sizes (like `MemoryStore`) cannot
catch a wrong-bytes bug. When you touch an upload path, assert the BYTES.

### 2.5 `FakeHttpClient` CONSUMES a queued response, so a loop of calls needs one client per call

Found this round. `take_matching` **removes** the matched response from the queue, so this:

```rust
let client = Arc::new(FakeHttpClient::new().always(401, "{\"token\":\"ghp_example_token\"}"));
for error in [api.put("a", b"x"), api.get("a"), api.list("d"), api.repository_info()] { ... }
```

passes for the WRONG reason: the first call gets the 401 and the rest get the fake's built-in
`404 FakeHttpClient has no response queued for …`. The test was written that way first, and the
failure it produced was the useful part. **Give each call its own client**, or queue one response
per call.

### 2.6 Two of §22.7's tables were DEAD SCHEMA

`backup_targets` and `backups` were created by the first migration and read by nothing —
`rebuild.rs` lists them as configuration rather than derived data, so they were never even
rebuilt. A schema that nothing writes looks like an implemented feature in every review. When a
contract clause names a table, `grep` for a reader before believing it. The GitHub cloud-target
round is what made them real (§5.1).

### 2.7 A self-inflicted bug a test caught before it shipped

`archive_created_at` recovered an archive's timestamp from its name — and its first version split
on `-` and took only the FIRST field, so `20260101-120000` yielded the date and threw the time
away, returning `None` for every real archive. Retention then had no sort key. Two tests failed
immediately. **This is why the row exists:** the parser has a case for a name that merely looks
like an archive, because a guess there would be a wrong timestamp rather than a missing one.

### 2.8 A REAL secret leak, found by wiring a feature rather than by review

The §11.5 connection test asserts that its message cannot contain the API key. It FAILED:

```text
AI provider error CLIENT_ERROR (p1): provider returned HTTP 401:
  {"error":{"message":"Incorrect API key provided: sk-example-secret"}}
```

`parse_chat_response` and `parse_models_response` in `veloct-ai` were passing a provider's RAW error
body into the message. Providers echo the key back — that is the documented example
`veloct_core::secret::redact_with_known` exists for — so the §18.1 guarantee held only at the LOG SINK,
not at the source, and the window was shown the key too. Both builders now go through
`Provider::failure()`, which redacts with the credential the provider holds. **The lesson:** the
crate-level rules were all tested and passing; what the wiring test found was a leak on the path nothing
had ever driven.

Two things to keep:
- Redact where the secret is KNOWN, not where it is displayed. A sink that must remember is a sink that
  will forget.
- That test now states the two layers separately, because confusing them is easy: the exact value the
  caller HOLDS (only the caller can catch it) and a realistic key SHAPE it has never seen (only the
  pattern layer can). The OpenAI pattern needs 20+ characters after `sk-`, so a shorter string is
  deliberately left alone and a test value must be realistic.

---

## 3. Environment facts (non-obvious, all necessary)

- **`pwsh` is not installed.** Use `powershell -ExecutionPolicy Bypass -File <script>`; `.ps1`
  execution is disabled.
- **All Cargo goes through `scripts/cargo.ps1`.** It sets an isolated `CARGO_HOME=.cargo-home/`
  (the machine's user-level `config.toml` points crates.io at an unreachable mirror) and passes
  the machine HTTP proxy via `CARGO_HTTP_PROXY`. Cargo is at
  `%USERPROFILE%\.cargo\bin\cargo.exe`, not on `PATH`.
- **Use `npm.cmd`, never `npm`.** PowerShell script execution is blocked, so `npm.ps1` fails.
- **Two npm roots:** the repository root (`package.json`) is the **desktop workbench**
  (React/TypeScript/Vite). `site/` is the **Astro template** and has its own `package.json`.
- **The frontend has no dev server running here.** `dist-ui/` is built by `npm run build:ui` and
  `tauri.conf.json` points `frontendDist` at it. `npm run dev:ui` is only for HMR work.
- Node 24, npm 11, `rustc 1.97.1`, MSVC BuildTools, WebView2. Only
  `x86_64-pc-windows-msvc` is installed, so the sidecar must stay suffixed
  `-x86_64-pc-windows-msvc.exe`.
- **The pinned Node runtime is fetched, not committed** (`scripts/fetch-node.ps1`, pin in
  `scripts/node-runtime.toml`, 89 MB, hash-verified against Node's own `SHASUMS256.txt`). The
  2.66 MB launcher IS committed. If `src-tauri/binaries/veloct-node-*.exe` is missing, run the
  fetch script.
- **`git show <blob>` mangles binary output** through line-ending conversion and reports a wrong
  size. Use `git cat-file -s` / `git hash-object` to compare a committed binary with disk.
- **Never use PowerShell text replacement on source files.** `Set-Content -NoNewline` corrupted a
  file with cp1252 once, and it happened AGAIN this session: `(Get-Content) -replace ... | Set-Content
  -Encoding UTF8` wrote a BOM and turned `§13.11` into `搂13.11` in `crates/veloct-attach/src/s3.rs`.
  Use the `write`/`edit` tools, or `[System.IO.File]::WriteAllText($p, $t, (New-Object
  System.Text.UTF8Encoding($false)))`, or Python `pathlib` with explicit UTF-8.
- **`Get-Content` lies about a BOM-less UTF-8 file.** PowerShell 5.1 decodes it with the system code
  page, so correct `§` prints as `搂` and a repaired file looks corrupted. Verify encoding with the
  `read` tool, or by checking the first three bytes for `EF BB BF`, never by eyeballing `Get-Content`.

### 3.1 The commands to run before claiming anything

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 test --workspace     # exit 0, 1295 passed
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 clippy --workspace --all-targets
powershell -ExecutionPolicy Bypass -File .\scripts\build-sidecar.ps1 -Check
cmd /c "npm.cmd run test:ui"                                                       # 24 passed
cmd /c "npm.cmd run build:ui"                                                      # tsc clean
# in site/:
$env:VELOCT_BUILD_TIME="2026-01-01T12:00:00.000Z"; npm.cmd run build; node scripts/verify-build.mjs
```

Three integration tests are `#[ignore]`d because they spawn real processes. Run them explicitly
when you touch the areas they cover:

```powershell
.\scripts\cargo.ps1 test -p veloct-release  --test end_to_end_release   -- --ignored   # real Astro build
.\scripts\cargo.ps1 test -p veloct-desktop  --test publish_integration  -- --ignored   # real publish
.\scripts\cargo.ps1 test -p veloct-desktop  --test backup_integration   -- --ignored   # real backup/restore
```

---

## 4. What IS implemented

All numbers are test counts. The matrix holds **218 rows across tables A-G** (A 23, B 14,
C 12, D 131, E 21, F 6, G 11) plus a separate **20-row H table** which IS the `handoff.md` §28
acceptance checklist, one row per headline requirement. **198 of the A-G rows carry a status and
all 198 are `I`**; the remaining 20 are table headers or rows whose status lives in a linked row.

**The H table is the one to read when judging completion.** It is short, it mirrors §28 clause
by clause, and each row names the evidence it requires.

| Crate | What it owns |
|---|---|
| `veloct-core` | Errors (with `hint()`), secrets/redaction, atomic writes, `ProjectRoot`, `FileRevision`, HTTP client trait + fakes, clock, ids |
| `veloct-markdown` | Frontmatter, directives, paragraph ids `<!--p:ULID-->`, references, rewrite, diagnostics, parse |
| `veloct-storage` | SQLite schema + migrations, rebuildable index, `ai_settings` (providers, models, features, fallback), **and the re-exported `Connection` its own API takes** |
| `veloct-content` | Content model, WIP lifecycle, paragraph ids, `create_from_source`, works import/metadata |
| `veloct-git` | Change sets, publish commits, release tags, worktrees |
| `veloct-logging` | `[info]/[warning]/[error]`, mandatory redaction AT CONSTRUCTION, rotation, log page |
| `veloct-process` | Preview supervisor, port reservation, readiness probes |
| `veloct-release` | `release.json` (no commit sha), frozen inputs, deterministic fingerprint, one build → two roots, **`publish()`** = freeze → build → commit → tag |
| `veloct-sidecar` | The Astro launcher: resolves the bundled Node from the sidecar path, NEVER `PATH` (ADR-0005) |
| `veloct-deploy` | Three logical targets × main/works sub-targets, Cloudflare adapter, manual exports, roll-up (`partial` vs `failed`), re-send, **SEO push lifecycle** |
| `veloct-ai` | Providers, per-feature model mapping, CANCELLED-safe fallback, context, apply plans, **the page creator**, **model discovery + a connection test whose failure message is redacted at the source** (§2.8) |
| `veloct-attach` | Three independent state axes, reference sets, dedup, cache pool, upload transaction, **GitHub and S3 attachment targets (both delegating to their protocol crate)**, and the canonical MIME map |
| `veloct-github` | The GitHub Contents API, ONCE: `Repository`, `GitHubContents` (put/get/list/delete/repository_info), `base64_encode`, the 100 MB ceiling. No policy — that stays with the dependent (ADR-0015) |
| `veloct-s3` | S3 / S3-compatible, ONCE: SigV4 (`CanonicalRequest`, `uri_encode`, the signing-key chain), and `S3Client` (streamed put, HEAD, paginated LIST, DELETE, a signed connection test). No policy either (ADR-0016) |
| `veloct-backup` | Selection rules, gzip archive + manifest, secret scan, staging restore, retention, target orchestration, **the GitHub and S3 cloud backup targets — §17.2 verified against the repository, and S3's streamed upload plus the bucket's own timestamps** |
| `veloct-app` | Orchestration services: attachments, backup/restore, deploy, autobackup, **cloud backup targets in SQLite + the run record**, plus `CommandError` |
| `src-tauri` | The Tauri shell: **70 registered commands**, `AppState`, preview wiring, credential store (keyring), sidecar resolution |
| `src/` | The workbench: 3-pane layout, **Tiptap WYSIWYG** editor with a Markdown view, draggable split with a REAL Astro iframe preview, autosave, conflict UI, log page, **AI settings with model discovery, a connection test and a model list**, page creator, attachments, publish, backups (including cloud targets), deploy |

**Verified end-to-end against real processes**, not only unit tests:

- A real desktop binary builds and launches (`target/debug/veloct-desktop.exe`, ~37 MB).
- The sidecar starts the real Astro dev server through the real bundled Node: `GET /` → HTTP 200,
  an article route → 200, and `taskkill /T` frees the port.
- `publish` runs a real Astro build, emits `dist/site` + `dist/works`, writes `release.json`,
  commits, tags, and the tag resolves to the commit the publish record names.
- Backup/restore against the repository's own content: 56 files archived; full restore wrote 56;
  **content-only restore wrote 11 and preserved 45**; an unconfirmed restore changed nothing.

### 4.1 The 20-row acceptance table, judged honestly

`docs/spec/02-test-matrix.md`'s H table IS `handoff.md` §28 clause by clause. Read it before
believing any summary, including this one. Judged against the evidence that exists today:

| # | Requirement | Honest verdict |
|---|---|---|
| H1 | Create and manage a project | **done** — `ProjectService`, 70 shell commands |
| H2 | Markdown edit, autosave 2 s / 30 s / save-on-close | **done** — `src/lib/autosave.ts`, 24 frontend tests |
| H3 | SQLite index rebuilds | **done** — `veloct-storage::rebuild` |
| H4 | Real Astro live preview, same project as production | **done** — sidecar smoke test: HTTP 200, article route 200 |
| H5 | WIP control, stable URL, finished redirect | **done** |
| H6 | Knowledge paragraph references + repair | **done** |
| H7 | Page creator | **done** — `veloct-ai::page` + `src/PageCreator.tsx` |
| H8 | AI per-feature provider/model + fallback | **done** |
| H9 | AI confirm / apply-all / apply-partial | **done** (apply lives in `veloct-ai::apply`; the UI exposes confirm) |
| H10 | Work import, screenshot, iframe preview, isolation | **PARTIAL — see §5.9** |
| H11 | Publish commit + Release tag | **done** — real publish integration run |
| H12 | One build per publish, all targets share it | **done** |
| H13 | Failed target re-publish by original Release | **done** — `resendable_sub_targets` |
| H14 | 1Panel manual upload of current `dist` | **done** as a manual export |
| H15 | Attachments: cache pool, **sftp, s3, lsky**, github, remote image | **PARTIAL — sftp and lsky still have NO adapter.** GitHub, **S3** and the local directory work; the pool, dedup and references work; remote image search (`network.rs`) exists but has no command. §13.11's S3 target is verified against AWS's own published SigV4 vectors |
| H16 | Dedup safe, multi-reference safe, no auto remote delete | **done** |
| H17 | Auto backup, manual backup, local export, full restore, content-only restore | **PARTIAL — all five work, and BOTH of §17.4's cloud targets are now real: GitHub (§17.2 verified against the repository on every upload) and S3 (§13.11's client, a streamed upload, the bucket's own timestamps). Targets live in SQLite's `backup_targets`, each run is recorded per target in `backups` (§22.7), and §17.3's finding blocks the upload while the local archive survives. What is still missing is NOT the target model — see §5.8: no live service has ever been contacted** |
| H18 | No secret in config/log/backup; scan before backup | **done** — scan runs BEFORE the archive is written; and see §2.8, where wiring a feature found a leak on a path review had missed |
| H19 | SEO, comments, logging operational | **done** — SEO push lifecycle, giscus comments (off by default), log page |
| H20 | Key failure paths explicit, never silent | **done** — `hint()`, `anchor_error`, `unimplemented_reason()`, `capabilities().verified` |

**So: 17 of 20 are done, and H10, H15 and H17 are partial.** The three partials are the honest
answer to "is §28 met" today. Nothing above should be read as a claim that Veloct is complete.

---

## 5. What is NOT implemented

Ordered by how much of §28 they block. **§5.1, §5.2 and §5.6 are CLOSED** — they are kept, clearly
marked, because earlier versions of this file called them the biggest gaps and a reader looking for those
sentences must find out what happened to them. The OPEN items are §5.3, §5.4, §5.5, §5.7, §5.8, §5.9.

**Judge §28 against the H table in `docs/spec/02-test-matrix.md`, not against this list.** This list is
where the gaps are argued; the H table is what says whether they matter.

### 5.1 CLOSED: the GitHub half of §17.4, §17.2 and §17.7

Kept here rather than deleted, because what it says is what a reader of the old text would look for.
**It is no longer a gap.** `veloct-backup`'s `CloudTarget` / `BackupTargetAdapter` / `run_targets` /
`prune` model now HAS a real implementation behind it:

- `veloct_github::GitHubContents` — the Contents API once, shared with §13.13 (ADR-0015), including
  the directory `list` the old note called "the missing piece".
- `veloct_backup::GitHubBackupTarget` — `upload` / `list` / `remove` / `test_connection`. `upload`
  re-checks the repository's privacy **on every run** (§17.2 verified, not asserted) and refuses an
  archive above the Contents API's 100 MB ceiling **with no request at all**.
- `veloct_app::cloud_backup` — the targets in SQLite's `backup_targets`, the run in `backups`
  (§22.7's rows; both tables were dead schema until now), `UploadOutcome`s that keep SKIPPED apart
  from FAILED, and a `ReportingTarget` so an unusable target fails on its own without abandoning the
  others.
- `src-tauri` — 6 commands, and `run_auto_backup_if_due` no longer reports `targets: Vec::new()`,
  which was the honest comment quoted below:

```rust
// Section 17.7's cloud targets are not wired yet, and reporting an empty list is honest
targets: Vec::new(),   // REMOVED
```

### 5.2 CLOSED: S3 — §17.4's second cloud target and §13.11's storage target

The old text here said there was **no SigV4 code anywhere** and that a configured S3 target became a
`ReportingTarget` that stored nothing. Both halves are now implemented:

- **`crates/veloct-s3`** — `sigv4` (`CanonicalRequest`, `uri_encode`, the four-HMAC signing-key chain,
  the scope) and `client` (`S3Client`: streamed PUT, HEAD, paginated LIST over a real XML parser,
  DELETE, a signed connection test). ADR-0016 records the dependency decision the old text asked for:
  RustCrypto `hmac` + the existing `sha2`, `quick-xml`, **no OpenSSL and no vendor SDK**.
- **`veloct_backup::S3BackupTarget`** (§17.4) and **`veloct_attach::S3Store`** (§13.11) — one client,
  two policies, exactly as ADR-0015 did for GitHub. `unimplemented_reason` now answers `None` for S3 in
  BOTH crates, and that is asserted rather than assumed.
- **The evidence is AWS's own published test suite.** All four documented SigV4 examples — GET Object,
  PUT Object, GET Bucket Lifecycle, List Objects — have their canonical request, its hash, the
  `StringToSign` and the final signature reproduced exactly. The documentation offers them "as a test
  suite to verify your code", which is the closest thing to a live-endpoint check that this environment
  can produce.

The access key ID is configuration and only the SECRET key is a credential — recorded in ADR-0016
because §12.2 lists "S3 Secret" and the distinction is easy to copy wrongly in either direction.

### 5.3 SFTP storage target (§13.10)

Declared unimplemented and reported. Needs an SSH implementation; no `ssh` feature exists
**on purpose** (see above). Either accept a dependency that brings OpenSSL and verify it builds on
MSVC, or keep reporting the gap. **Do not add a guessed protocol.**

### 5.4 Lsky Pro storage target (§13.12)

Declared unimplemented and reported. Needs Lsky's own upload-token protocol, which is not verified
in `docs/spec/03-upstream-facts.md`. Same rule: verify the protocol or keep reporting.

### 5.5 The attachment panel has no target FORM — the most visible gap in §13

Verified by search rather than remembered, and stated precisely because the imprecise version is
misleading:

| What | State |
|---|---|
| `api.uploadAttachment` | **called** by `src/AttachmentPanel.tsx` — uploading TO a target works |
| `api.storageTargets` | **called** — the panel lists them, with their kind and whether a credential exists |
| `api.saveStorageTarget` | exists in `api.ts`, **called by no component** |
| `api.deleteStorageTarget` | exists in `api.ts`, **called by no component** |
| `api.uploadEligibility` | exists in `api.ts`, **called by no component** |

So every §13.1 kind can be configured through the command surface and **none of them through the
window**: a user cannot create a target, cannot remove one, and is not told before an attempt whether an
upload is even eligible (§13.2.2's reason is available and unshown). §13.11's S3 fields have no way in at
all.

**§13.8's explicit remote cleanup is missing at BOTH layers.** `StorageAdapter::delete_remote` is
implemented by the GitHub and S3 stores and asserted by the matrix (`D16`), but **no Tauri command exposes
it** — verified by search, not by memory — so the only path from the window is `remove_attachment`, which
removes the record and the CACHED bytes and deliberately leaves the remote object alone. That is exactly
right for §13.8's rule, and it means a user who wants to tidy a remote object has no way to ask. Both a
command and a button are missing.

The commands, the DTOs and the `api.ts` calls all already carry the fields (`endpoint`, `region`,
`bucket`, `access_key`, `credential`, `path_prefix`, `public_url_prefix`). What is missing is the form.
Small, self-contained, and the reason §28's H15 stays PARTIAL for the *user-facing* half even after S3
landed.

**Definition of done for this item:** a form that creates or updates a target, a delete button, and the
eligibility reason shown before an upload is attempted. The delete button needs a decision rather than an
oversight: `delete_storage_target` also removes that target's credential from the OS store, so the button
should say so, and §13.8's separate remote-cleanup command (above) is a second, smaller piece. Neither is
large — but each needs its own matrix row and recorded command, like every other `I` in this repository.

### 5.6 CLOSED: AI model discovery (§11.2) and the connection test (§11.5)

The old text here said `build_models_request()` and `replace_models` existed and **no command or button
called them**, and that the panel asked the user to type a model id. Both halves are wired:

- `src-tauri`: `refresh_ai_models`, `add_ai_model`, `delete_ai_model`, `test_ai_provider` — four commands.
- `page_creator::refresh_models` fetches the provider's list, merges it with the stored one through
  `Provider::merge_models`, and writes it with `replace_models`. The merge is the crate's, not a second
  copy: a hand-added model survives a refresh that does not return it (§11.2).
- `page_creator::provider_from_row` is now the ONE way a provider is built, used by the page creator and
  by the refresh alike — a discrepancy there would mean a provider that works for generation but not for
  a refresh.
- `src/AiSettingsPanel` (in `src/PageCreator.tsx`) has 测试连接, 刷新模型, the model list with its source,
  手动添加 and 删除.
- **The wiring found a real §18.1 leak** (§2.8): a provider that echoes the key back in a 401 body had
  that body become the error message shown in the window. `Provider::failure()` now redacts at the
  source with the credential it holds.

What is still NOT here: a per-provider `context_window` is stored but never shown, and a model list is
fetched only when the user asks — there is no background refresh, which §11.2 does not require.

### 5.7 The NSIS installer

`src-tauri/tauri.conf.json` sets `bundle.targets = ["nsis"]`, but **`tauri build` has never run**:
`tauri-cli` is not installed, and installing it needs the network (crates.io IS reachable — that is how
`veloct-s3`'s two new dependencies arrived — so `cargo install tauri-cli` or an `npx @tauri-apps/cli`
devDependency would work). What is verified is `cargo build -p veloct-desktop` plus a launch smoke test,
NOT a packaged installer.

### 5.8 Smaller, stated gaps

- **A real remote operation has never run.** Every GitHub upload, S3 request, deploy and cloud push is
  asserted against `FakeHttpClient`. No live token has been used, and nothing has been sent to
  Cloudflare, EdgeOne, github.com or any S3 endpoint. Nothing in the matrix claims otherwise — and this
  now includes the whole cloud-backup path. **ADR-0016's SigV4 tests are AWS's published vectors rather
  than a live call, which is the strongest claim this environment can support and is labelled as such.**
- **One DeployPanel distinction is incomplete:** a target's `kind` is filled from configuration
  after a run, and the crate's own `needs_attention` is recomputed in the view rather than read
  from `LogicalTargetRun`.
- `resend_backup_to_cloud` re-derives the archive's SHA-256 by reading the file, because
  `BackupManifest` does not carry it (it is the digest of the FINISHED archive, which exists only after
  the manifest is written). The value is identical for unchanged bytes — `write_archive` computes it
  the same way — so a re-send names the same remote object rather than creating a second one, but an
  `archive_sha256` field in the sidecar record would be cheaper than a full second read.

### 5.9 H10 is partial: work import, screenshots and remote images have no command

Verified by search, not memory: `src-tauri/src/*.rs` contains **zero** references to
`network_image`/`image_search`, to `screenshot`, and to `import_work`/`work_import`. The model
exists in the crates:

- `veloct-attach::network` has `ImageSource`, `ImageResult`, `ImageSearch`, `NetworkImageChoice`,
  `is_network_image`, `network_image_notice` — §13.14's search → choose → write the remote URL.
- `veloct-content::works` has `metadata` and `import` — §10's work import.
- The poster-wall **screenshot route** (`/work-previews/<slug>/screenshot.webp`) is not emitted by
  the site either.

So §28's "the works system" is reachable in that works can be imported BY THE LIBRARY and their
pages build into `dist/works` on a separate origin, but a user cannot import a work, generate a
screenshot, or search for a remote image from the window. Each is a command plus a button, and
each is small.

---

## 6. Things that have been true and are worth preserving

These are the habits that produced the found bugs, and they are easy to lose.

1. **When a test fails, decide which side is wrong before "fixing" it.** In this session the
   TEST was wrong more often than the code: `dist/` and `.attachments-cache/` ARE included in a
   backup (§17.1), appending bytes to a gzip stream does NOT corrupt it, a failed upload is
   `FAILED` and retryable rather than `PENDING`, and a well-formed `knowledgeref` to a missing
   target is not a parse error. Each of those was a wrong premise, and "fixing" the code would
   have broken correct behaviour.

2. **Assert the behaviour, not the constant.** A test asserting `MAX_ATTACHMENT_BYTES >= 1 MiB`
   proves nothing and clippy says so. It became a sparse file above the ceiling, asserting the
   refusal AND that nothing was cached.

3. **A thing that cannot be seen from the filesystem needs reporting.** A release tag that no
   longer resolves, an unreadable tracker, a stale launcher: each is invisible until something
   says so.

4. **State a gap rather than papering over it.** `unimplemented_reason()`, `capabilities().verified`,
   `anchor_error`, and the `ReportingTarget` that stores nothing while saying why all exist so a user
   or the next agent is told the truth. Do not replace one of those with a guessed request or a
   silent no-op — and note that the one that USED to be here (an empty cloud target list) was closed
   by implementing the target rather than by softening the message.

5. **Prefer a real command over a claim.** Every matrix row at `I` names the test that proves it,
   and several name `#[ignore]`d runs against real processes. If you add a row, add the command.

---

## 7. Suggested order for the next agent

**Step 0, before any of the below:** run §3.1 and make the six results match §1. If they do not, find out
why before building on them. Then decide what to do about the dirty working tree (§1.1) — every number in
this file describes uncommitted work, and a `git checkout` would erase three rounds of it.

1. **Read `handoff.md` §28 again** and diff it against §4 above and the H table in the matrix.
   `handoff.md` wins over this file, and over any summary of it, including this one.
2. **Build the storage-target FORM (§5.5).** It is the most visible gap: §13's kinds can be configured
   only through commands, §13.11's S3 fields have no way in, and the file is `src/AttachmentPanel.tsx`.
   `api.ts` already has `saveStorageTarget`, `deleteStorageTarget` and `uploadEligibility` — the work is
   the form itself, plus deciding how loudly the delete button should warn that it also removes the
   stored credential. Add a matrix row and a command when you finish.
3. **Attempt the NSIS installer (§5.7)** now that the network is confirmed reachable.
4. Keep SFTP and Lsky Pro (§5.3, §5.4) either implemented properly or **explicitly reported**. Never
   guess a protocol — and note that **Lsky Pro is the last attachment kind with no adapter**, so it is the
   one that would move §28's H15, provided its upload-token protocol can be verified from a real source.
5. **A live smoke test of ONE remote operation is the remaining honesty gap** (§5.8): every path is
   asserted against `FakeHttpClient`, and ADR-0016's SigV4 evidence is AWS's published vectors rather than
   a real request. If the user can supply a throwaway private repository, a private bucket and a token,
   that is worth more than another unit test — and it is the only thing that would let anyone write
   "verified against a live service" anywhere in this repository.
6. **When §5 changes, the matrix changes with it.** Every row at `I` must name a test AND the command you
   ran; a row that cannot is a row that is wrong. `docs/spec/02-test-matrix.md` §J is where the recorded
   runs go, and this file's §1 table is where the totals go.

**Two habits worth keeping, both learned the hard way in these rounds:**

- **Prefer a delegation over a second implementation.** `veloct-github` and `veloct-s3` exist because two
  contracts needed one protocol; the extraction was proven faithful by the tests that did NOT change. When
  you find yourself writing a map, a rule or a protocol twice, that is the signal.
- **Redact where the secret is known, not where it is displayed** (§2.8). The §18.1 guarantee held at the
  log sink and failed at the source, and only a test that drove the path end to end found it.
