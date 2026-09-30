# PROCESSING.md — state of Veloct, and what the next agent should do

A **status and orientation document** for whoever picks this up next, written after three consecutive
rounds of work on the cloud targets and the AI settings. It is not a contract: `handoff.md` remains the
locked product contract and always wins, and `docs/spec/*` records the decisions `handoff.md` leaves open.

**Read in this order:**
1. `handoff.md` §28 — the completion definition. It is the only authority on what "done" means.
2. This file — §1 for where the work stands, **§5 for what is genuinely missing**, §7 for what to do next.
3. `README.md` — how to build and test on this machine.
4. `docs/spec/00-architecture.md` — the ADRs, including the two added by these rounds (ADR-0015, ADR-0016).

**Before changing anything**, run §3.1 and make the six numbers match. They were measured on commit
`2496604`, and the working tree is **CLEAN** (§1.1) — every round is committed, so nothing here is a
claim about work that no checkout can reproduce.

**The single most useful habit for this repository:** when a contract clause says a feature exists,
`grep` for a caller before believing it. Two of the three rounds here found their work that way — a whole
model (`CloudTarget`, `Provider::list_models`) implemented, tested, and called by nothing — and the §13
round found a rule in the same position (§5.10.1: a validation arm whose only reader is a test).

---

## 1. Where the work stands

| Fact | Value |
|---|---|
| Last commit these numbers describe | `2496604 Make the section 13 storage targets configurable, and expose section 13.8's cleanup` — **every commit after it changes documentation only**, so it is still the last commit that touches code |
| Working tree | **CLEAN** — the §13 round and the storage-kind follow-up are committed as `2496604`; the only commit after it carries this recording (§1.1) |
| Rust tests | **1324 passed, 0 failed, 5 ignored** (+29 over `bf75265`'s 1295; 17 workspace crates + `src-tauri`) |
| Frontend tests | **71 passed** (vitest: 24 `frontmatter` + 47 `storageTargetForm`) |
| `clippy --workspace --all-targets` | **0 warnings** |
| Site build verifier | **46/46 checks** — **NOT re-run this round**: `site/` is untouched by it, and §J of the matrix names the command behind every number here |
| Test matrix | **209 rows at `I`, 0 at `P`** (198 + 11 for §13's form, its cleanup and the storage-kind report), out of **229** rows across tables A–G |
| Shell commands registered | **72** (70 + `delete_remote_object` + `storage_kinds`) |

Every command above exits **0** on success. That matters and is itself a recent fix — see §2.

**These numbers were measured on commit `2496604` and are reproducible from it**, which is the point
of the commit: before it, four rounds of work had been verified in a working tree and never in a
checkout, and a number that is true of one directory on one machine is a weaker claim than it looks.
`docs/spec/02-test-matrix.md` §J holds the run, the per-crate rows that sum to 1324 exactly, and the
earlier snapshot it supersedes. The commit that carries the paragraph you are reading came
immediately after `2496604` and changes only this file and the matrix, which no verify command reads —
so if §3.1 does not reproduce these numbers from a checkout of `2496604`, something outside this
repository changed and that is the first thing to find out.

### 1.1 What these rounds changed

**COMMITTED, and therefore reproducible from a checkout.** Four commits carry the cloud-target and AI
rounds — `050a3c5` (the GitHub/S3 half of §17.4 and §13.11), `57003e5` (the §18.1 redaction),
`61ffde7` (§11.2's model discovery and §11.5's connection test) and `7df0625` (the tree the previous
version of this file described) — and `bf75265` then recorded them and corrected this file. Together
they were 104 new tests (1191 → 1295) and a clean clippy, and §J of the matrix holds the run whose
tree hash those commits match. They contain three increments. The first two close §17.4; the third
wires §11.2 and §11.5.

1. **GitHub** — the pre-existing `CloudTarget` / `BackupTargetAdapter` model got a real adapter, the
   targets moved into SQLite's `backup_targets`, each run into `backups` (§22.7), and §17.2 is now
   VERIFIED against the repository on every upload rather than only declared by the user.
2. **S3 / S3-compatible** — a new `veloct-s3` crate implementing AWS Signature Version 4, plus adapters
   for BOTH §17.4 (cloud backup) and §13.11 (attachment storage). Both of §17.4's targets now exist.
3. **AI model discovery and the connection test** — `Provider::list_models`, `merge_models`,
   `test_connection` and `veloct_storage::replace_models`/`read_models` went from **zero call sites
   above `veloct-ai`** to four commands and a settings panel. Wiring it up **exposed a real §18.1 leak**
   (§2.8): a provider's 401 body echoes the key, and that body was becoming the error message.

The files those rounds ADDED, so the next agent knows what was new without reading the whole diff:

```text
crates/veloct-github/                 NEW  the GitHub Contents API, shared by §13.13 and §17.4
crates/veloct-s3/                     NEW  SigV4 + an S3 object client, shared by §13.11 and §17.4
crates/veloct-backup/src/github.rs    NEW  the §17.4 GitHub cloud backup target
crates/veloct-backup/src/s3.rs        NEW  the §17.4 S3 cloud backup target
crates/veloct-attach/src/s3.rs        NEW  the §13.11 S3 attachment target
crates/veloct-app/src/cloud_backup.rs NEW  cloud targets in SQLite + the §22.7 run record
```

Everything else there was edits to existing files, recorded in `docs/spec/00-architecture.md`
(ADR-0015, ADR-0016) and as matrix rows `C11` and `D104`–`D129`.

4. **§13's storage targets, and §13.8's explicit remote cleanup** — the local directory became a kind
   of its own (`StorageKind::Local`) instead of the thing an unconfigured SFTP target SILENTLY degraded
   into; `upload_attachment` and the new `delete_remote_object` build their target through ONE routing
   helper, so the two can never disagree; the object record now names the target the bytes actually
   reached rather than the one the attachment was added for; §13.8's cleanup exists at both layers
   (transaction → service → Tauri command → button) with three refusals that each remove nothing; and
   the window gained the storage-target FORM plus §13.2.2's eligibility reason, read BEFORE an upload
   is offered. **+22 Rust tests (1295 → 1317) and +41 frontend tests (24 → 65)**, clippy clean,
   `build:ui` clean. §5.5 is what this closed; §5.10 is what it left.
5. **The storage-kind report (§5.5's follow-up)** — `StorageKind::ALL` became the ONE canonical kind
   list, guarded by a test whose expected array is written out beside an exhaustive match with no
   wildcard arm, so a kind added to the enum does not compile until it is named there;
   `is_implemented` was derived from `unimplemented_reason` so that split is written down once; and the
   `storage_kinds` command reports each kind's `implemented`/`unimplemented_reason` from those crate
   functions, taking NO project state, so the window can say "SFTP has no adapter in this build" BEFORE
   a target of that kind exists. **+7 Rust tests (1317 → 1324) and +6 frontend tests (65 → 71)**, clippy
   clean, `build:ui` clean. See §5.5 and matrix rows D139–D140.

**ALL OF IT IS COMMITTED: `2496604`** — "Make the section 13 storage targets configurable, and expose
section 13.8's cleanup" — so the numbers in §1 are reproducible from a checkout rather than from one
working directory. `git status --short` is **empty**, and the commit after it carries this recording.

The paths that commit changed, so the next agent knows what is new without reading the whole diff:

```text
crates/veloct-attach/src/lib.rs,storage.rs,targets.rs,transaction.rs   the Local kind, StorageKind::ALL, the target routing model, §13.8's transaction
crates/veloct-app/src/attachments.rs,lib.rs                            the service half: delete_remote_object, RemoteDeleteView, ObjectView.storage_target_id
src-tauri/src/attachments.rs,lib.rs                                    storage_target_for, the three commands, StorageTargetView's S3 fields
src/AttachmentPanel.tsx,lib/api.ts,styles.css                          the form, the cleanup confirm step, the eligibility reason, the kind-gap report
src/lib/storageTargetForm.ts                                           NEW  the form's decisions, as a pure module (47 tests)
tests/storageTargetForm.test.ts                                        NEW  those tests
```

The many `*.log` files in the working directory are this session's captured verification output, and
`.gitignore` line 25 ignores `*.log`, so they are NOT part of the change set; they are safe to read
(they are the raw evidence for the numbers in §1) and safe to delete.

**The matrix at 209 `I` / 0 `P` is a claim about recorded evidence, not proof that §28 is met.**
It has been wrong in that direction before: at one point it read 93 `I` while the desktop
workbench did not exist at all. When you find a gap, the matrix row is what is wrong.
The rows this round added (D130–D140) each name the test AND the crate, and where a row cannot
assert something (an untested refusal, a command body no test drives) it says so in the row as a
**stated limit** rather than implying coverage — `docs/spec/02-test-matrix.md` §I makes that the rule.

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
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 test --workspace     # exit 0, 1324 passed, 5 ignored
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 clippy --workspace --all-targets
powershell -ExecutionPolicy Bypass -File .\scripts\build-sidecar.ps1 -Check
cmd /c "npm.cmd run test:ui"                                                       # 71 passed
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

All numbers are test counts. The matrix holds **229 rows across tables A-G** (A 23, B 14,
C 12, D 142, E 21, F 6, G 11) plus a separate **20-row H table** which IS the `handoff.md` §28
acceptance checklist, one row per headline requirement. **209 of the A-G rows carry a status and
all 209 are `I`**; the remaining 20 are table headers or rows whose status lives in a linked row.
The eleven new ones are `D130`–`D140`: §13's form, the local kind, the end of the
unconfigured-SFTP degradation, §13.8's explicit cleanup, §13.2.2's reason, and the storage-kind
report the window reads before a target exists.

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
| `veloct-attach` | Three independent state axes, reference sets, dedup, cache pool, upload transaction, **GitHub, S3 and the local-directory attachment targets (`StorageKind::Local`, §13.10 where the server is this machine)**, **§13.8's explicit `delete_remote_object` — the only path that removes a remote object**, and the canonical MIME map |
| `veloct-github` | The GitHub Contents API, ONCE: `Repository`, `GitHubContents` (put/get/list/delete/repository_info), `base64_encode`, the 100 MB ceiling. No policy — that stays with the dependent (ADR-0015) |
| `veloct-s3` | S3 / S3-compatible, ONCE: SigV4 (`CanonicalRequest`, `uri_encode`, the signing-key chain), and `S3Client` (streamed put, HEAD, paginated LIST, DELETE, a signed connection test). No policy either (ADR-0016) |
| `veloct-backup` | Selection rules, gzip archive + manifest, secret scan, staging restore, retention, target orchestration, **the GitHub and S3 cloud backup targets — §17.2 verified against the repository, and S3's streamed upload plus the bucket's own timestamps** |
| `veloct-app` | Orchestration services: attachments, backup/restore, deploy, autobackup, **cloud backup targets in SQLite + the run record**, plus `CommandError` |
| `src-tauri` | The Tauri shell: **72 registered commands**, `AppState`, preview wiring, credential store (keyring), sidecar resolution, **the storage-target commands including §13.8's `delete_remote_object` and `storage_kinds`, which reports each kind's adapter status from the crate without touching project state** |
| `src/` | The workbench: 3-pane layout, **Tiptap WYSIWYG** editor with a Markdown view, draggable split with a REAL Astro iframe preview, autosave, conflict UI, log page, **AI settings with model discovery, a connection test and a model list**, page creator, attachments **with the storage-target FORM, §13.8's remote cleanup, §13.2.2's eligibility reason and the kind-gap report read before the first save**, publish, backups (including cloud targets), deploy |

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
| H2 | Markdown edit, autosave 2 s / 30 s / save-on-close | **done** — `src/lib/autosave.ts`, 71 frontend tests |
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
| H15 | Attachments: cache pool, **sftp, s3, lsky**, github, remote image | **PARTIAL, and this round moved it: the local directory is a kind of its own, the unconfigured-SFTP-to-local degradation is gone, §13.8's explicit remote cleanup exists at both layers, and the window can report a kind's gap BEFORE a target of that kind is saved.** What is STILL PARTIAL and exactly why: **(1) `sftp` has NO adapter** — configurable, and it reports the gap instead of guessing a protocol (§5.3, a dependency decision); **(2) `lsky` has NO adapter** for the same reason — its upload-token protocol is not verified in `docs/spec/03-upstream-facts.md` (§5.4); **(3) remote image** search exists in `veloct-attach::network` but has no command and no button, so §13.14 is unusable from the window (§5.9, shared with H10); **(4) nothing has ever contacted a live service** — GitHub and S3 are asserted against a fake HTTP client and the local kind writes to the filesystem (§5.8). The pool, the three axes, dedup, multi-reference, §13.8's no-auto-delete rule and the kind report are done (B1–B14, D16, D91–D93, D116–D123, D130–D140). **§28 as a whole is not met either, and not only here: §5.7's installer has never been built — `tauri build` has never run and no NSIS package exists** |
| H16 | Dedup safe, multi-reference safe, no auto remote delete | **done** |
| H17 | Auto backup, manual backup, local export, full restore, content-only restore | **PARTIAL — all five work, and BOTH of §17.4's cloud targets are now real: GitHub (§17.2 verified against the repository on every upload) and S3 (§13.11's client, a streamed upload, the bucket's own timestamps). Targets live in SQLite's `backup_targets`, each run is recorded per target in `backups` (§22.7), and §17.3's finding blocks the upload while the local archive survives. What is still missing is NOT the target model — see §5.8: no live service has ever been contacted** |
| H18 | No secret in config/log/backup; scan before backup | **done** — scan runs BEFORE the archive is written; and see §2.8, where wiring a feature found a leak on a path review had missed |
| H19 | SEO, comments, logging operational | **done** — SEO push lifecycle, giscus comments (off by default), log page |
| H20 | Key failure paths explicit, never silent | **done** — `hint()`, `anchor_error`, `unimplemented_reason()`, `capabilities().verified` |

**So: 17 of 20 are done, and H10, H15 and H17 are partial.** The three partials are the honest
answer to "is §28 met" today. Nothing above should be read as a claim that Veloct is complete.

---

## 5. What is NOT implemented

Ordered by how much of §28 they block. **§5.1, §5.2, §5.5 and §5.6 are CLOSED** — they are kept, clearly
marked, because earlier versions of this file called them the biggest gaps and a reader looking for those
sentences must find out what happened to them. The OPEN items are §5.3, §5.4, §5.7, §5.8, §5.9 and §5.10.

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

### 5.5 CLOSED: the attachment panel's target FORM, and §13.8's explicit remote cleanup

Kept here rather than deleted, because the old text called this **the most visible gap in §13** and a
reader looking for that sentence must find out what happened to it. **It is no longer a gap.** The old
table said `saveStorageTarget`, `deleteStorageTarget` and `uploadEligibility` "exist in `api.ts`, called
by no component", and that §13.8's cleanup was "missing at BOTH layers". Re-verified the same way, by
search:

| What | State now |
|---|---|
| `api.saveStorageTarget` | **called** — `src/AttachmentPanel.tsx`'s 创建目标 / 保存修改, through `storageTargetForm.ts::buildSaveRequest` |
| `api.deleteStorageTarget` | **called** — a two-step 删除 confirmation that says the credential goes too |
| `api.uploadEligibility` | **called** — for every attachment on every reload, and the reason is rendered beside the button |
| `api.deleteRemoteObject` | **NEW, and called** — §13.8's cleanup, per attachment, refused unless the target really holds the object |

What closed it, and where to look:

- **The form** is `src/AttachmentPanel.tsx` + `src/lib/storageTargetForm.ts` (NEW, 47 vitest tests in
  `tests/storageTargetForm.test.ts`). Which fields a kind asks for, what a blank credential means, why a
  draft cannot be saved, which filled values a kind switch will NOT submit, and why the upload button is
  disabled are all decided in that pure module, because this repository has no React test renderer and a
  rule that lives in JSX is a rule nothing checks. §13.11's S3 fields (`endpoint`, `region`, `bucket`,
  `accessKey`, the secret) finally have a way in — and `StorageTargetView` had to GAIN
  `endpoint`/`region`/`bucket` first, because a form that cannot see what it saved would silently rewrite
  the target on the next save.
- **§13.8's cleanup at both layers**: `AttachmentTransaction::delete_remote_object` →→
  `AttachmentService::delete_remote_object` →→ the registered Tauri command `delete_remote_object` →→
  `api.deleteRemoteObject` →→ 仅删除远程对象. It refuses, removing NOTHING, on a target whose
  `capabilities().can_delete_remotely` is false (SFTP and Lsky report their own §13.10/§13.12 gap), on a
  target other than the one the object's RECORD names, and on an attachment with no recorded object. On
  success the remote object and its record go, `remote_url`/`remote_key` are cleared, the upload axis
  returns to `PENDING`, and **the cached bytes and the reference records are untouched** — §13.8 removes
  the remote object, not the user's record of the file. That the local target's delete removes the real
  BYTES from disk is asserted at two layers (§2.4's lesson).
- **The defect that made the local path invisible is fixed**: an `sftp` target with no host and no
  credential used to be handed a LOCAL directory adapter, so an unconfigured SFTP target "uploaded" to
  the machine it was running on and reported a URL no server serves. `StorageKind::Local` now exists as
  its own kind and `storage_target_for` routes every upload and every delete through one decision.
- **The delete button's decision** the old text asked for was taken: `delete_storage_target` also
  removes the target's credential from the OS store, so the confirmation says so, and says the remote
  files are NOT deleted.

**What is still not here, stated rather than papered over:** SFTP and Lsky Pro remain adapterless
(§5.3, §5.4) — the window now SAYS so before a target of either kind is saved, which is all a command
can do about a missing protocol; and the smaller limits this round left are §5.10.
`docs/spec/02-test-matrix.md` rows `D130`–`D140` are the recorded evidence, each naming the test AND
the crate, and the follow-up that made the gap report reachable is part of the same commit (`2496604`)
and the same run, not a later increment:

- **`StorageKind::ALL`** is the ONE canonical kind list, compared against a written-out expected array
  and guarded by an exhaustive match with no wildcard arm, so a kind added to the enum does not
  compile until it is named there.
- **`is_implemented(kind)` is `unimplemented_reason(kind).is_none()`** — the split is written down
  once, so a kind cannot be reported as implemented while also carrying a §5.3/§5.4 reason.
- **The `storage_kinds` command** reports each kind's `implemented`/`unimplemented_reason` from those
  crate functions and takes NO project state, so the answer exists before any target does. The window
  reads it in `reload()` under its own try/catch (a failed read leaves every kind unknown, never
  "works") and shows the gaps above the form.

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

### 5.10 Smaller gaps THIS round left, stated rather than papered over

Four, each found by search or by running the thing, and each recorded as a **stated limit** in the
matrix row that covers it — see `docs/spec/02-test-matrix.md` §I for what that means.

1. **`StorageConfig::validate` has NO production caller.** `grep '\.validate()'` finds it in tests
   only — **re-checked on the final tree, commit `2496604`**, after the `storage_kinds` follow-up and
   the `is_implemented` repair had both touched this area: still no caller. That matters because this
   round ADDED a `Local` arm to it (refusing a prefix that names a drive letter or a UNC root) and a
   test for that arm, so the rule looks enforced and is not: the window enforces the same thing in
   `storageTargetForm.ts::validateDraft`, and any other writer is caught at the first
   `put`/`head`/`delete_remote` by `veloct_core::paths::ensure_within`. Nothing escapes — the write is
   refused rather than landing outside the root — but the comment on the new arm ("refused here rather
   than discovered at the first write") describes something that does not happen, and the `..`,
   leading-`/` and public-URL-prefix rules the function has always held are in the same position.
   `save_storage_target` thus trusts the window, unlike `save_document`'s path in `commands.rs`, whose
   comment is explicitly the opposite ("re-validates the draft rather than trusting that the UI
   checked"). **The fix is a decision, not a line**: calling `validate()` on save would also start
   refusing an SFTP target saved without a host, which today is the only way to record §13.10's gap in
   the target list.
2. **One of §13.8's three refusals has no test.** "the attachment has no recorded object" exists in
   `crates/veloct-attach/src/transaction.rs` and nothing drives it (grepping its message finds the
   code and no test). The other two refusals, and both directions of §13.8's rule, are asserted at two
   layers.
3. **`delete_storage_target`'s warning is asserted; the credential removal is not.** That
   `credentials::remove` is called is one line in `src-tauri/src/attachments.rs`; no test proves it,
   so row `D137` claims only what the window says.
4. **§13.8's cleanup can leave an article pointing at a URL that no longer resolves.** The
   confirmation says the Markdown is not touched, and §13.8 does not ask for a rewrite — but a user
   who cleans up an object a published article cites gets a broken image and no automatic repair.
   Worth a decision if it ever bites.

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

6. **Grep for a CALLER before believing a rule, not only before believing a field.** This is how §5.10.1
   was found, and the entry it deserves is next to §2.6 rather than buried: this round added a
   `StorageConfig::validate` arm and a test for it, which made a refusal look enforced while
   `grep '\.validate()'` showed that no production path calls the function at all. A rule whose only
   reader is a test is documentation with a green tick beside it. **And when a row cannot prove
   something, write it in the row.** The eleven rows this round added say "**Stated limit**" where a
   refusal, a command body or a credential removal is asserted by inspection or at another layer;
   `docs/spec/02-test-matrix.md` §I makes that the rule, and a row that quietly implied more would be
   the drift the whole file exists to prevent.

---

## 7. Suggested order for the next agent

**Step 0, before any of the below:** run §3.1 and make the six results match §1. They were measured on
commit `2496604`, so if they do not reproduce, first check that you are looking at a tree whose
`.rs`/`.ts`/`.tsx` files match that commit — the two documents were updated by the commit right after
it and by nothing else. The working tree is **CLEAN** (§1.1): every round here is committed
(`050a3c5`, `57003e5`, `61ffde7`, `7df0625`, `bf75265`, `2496604`), so there is nothing to decide
about an uncommitted change set — and no `git checkout` can erase uncommitted work, because there is
none.

1. **Read `handoff.md` §28 again** and diff it against §4 above and the H table in the matrix.
   `handoff.md` wins over this file, and over any summary of it, including this one.
2. **§5.5's storage-target form is DONE** — do not build it again. `src/AttachmentPanel.tsx` creates,
   edits and deletes targets, §13.8's 仅删除远程对象 exists per attachment, §13.2.2's reason is
   shown before an attempt, and the kind-gap report is read before the first save; the evidence is
   matrix rows `D130`–`D140` and the runs in §J. **The real next gap is §5.9 / H10**: workspace
   import, the poster-wall screenshot route, and §13.14's remote image search have models in the
   crates (`veloct-content::works`, `veloct-attach::network`) and NO command, so each is a command
   plus a button — and the remote-image one would move H15.
3. **Attempt the NSIS installer (§5.7)** now that the network is confirmed reachable — it is one of
   the four things §4.1 names as keeping §28 unmet, and nothing in this repository has ever built a
   package.
4. Keep SFTP and Lsky Pro (§5.3, §5.4) either implemented properly or **explicitly reported**. Never
   guess a protocol — and note that **Lsky Pro is the last attachment kind with no adapter**, so it is the
   one that would move §28's H15, provided its upload-token protocol can be verified from a real source.
5. **A live smoke test of ONE remote operation is the remaining honesty gap** (§5.8): every path is
   asserted against `FakeHttpClient`, and ADR-0016's SigV4 evidence is AWS's published vectors rather than
   a real request. If the user can supply a throwaway private repository, a private bucket and a token,
   that is worth more than another unit test — and it is the only thing that would let anyone write
   "verified against a live service" anywhere in this repository.
6. **§5.10's four small gaps are worth one pass**, and the first one is a decision rather than a line:
   `StorageConfig::validate` has no caller (still true at `2496604`), so decide whether
   `save_storage_target` should validate (which starts refusing a host-less SFTP target) or whether the
   write-time `ensure_within` guard is the enforcement point and the comment should say so.
7. **When §5 changes, the matrix changes with it.** Every row at `I` must name a test AND the command you
   ran; a row that cannot is a row that is wrong. `docs/spec/02-test-matrix.md` §J is where the recorded
   runs go, and this file's §1 table is where the totals go.
8. **If the tree changes after you record a run, say so in §J rather than replacing the row.** That is
   what this round did with the 1317 snapshot: the tree moved under it, so the snapshot stayed and a
   second, authoritative run at commit `2496604` was recorded beside it. A run whose totals no longer
   match is a fact about the repository, not a mistake to tidy away.

**Two habits worth keeping, both learned the hard way in these rounds:**

- **Prefer a delegation over a second implementation.** `veloct-github` and `veloct-s3` exist because two
  contracts needed one protocol; the extraction was proven faithful by the tests that did NOT change. When
  you find yourself writing a map, a rule or a protocol twice, that is the signal.
- **Redact where the secret is known, not where it is displayed** (§2.8). The §18.1 guarantee held at the
  log sink and failed at the source, and only a test that drove the path end to end found it.
