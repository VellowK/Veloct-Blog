# PROCESSING.md — state of Veloct, and what the next agent should do

Written at the end of a long autonomous session, for whoever picks this up next. It is a
**status and orientation document**, not a contract. `handoff.md` remains the locked product
contract and always wins; `docs/spec/*` records the decisions `handoff.md` leaves open.

Read this file first, then `README.md` (build commands), then `handoff.md` §28 (the completion
definition). Then run the verification commands in §3 below before changing anything.

---

## 1. Where the work stands

| Fact | Value |
|---|---|
| Last commit | `2cfbc67 Remove a duplicate implementation of section 17.5's idle rule` |
| Working tree | **clean** |
| Rust tests | **1191 passed, 0 failed** (14 workspace crates + `src-tauri`) |
| Frontend tests | **24 passed** (vitest) |
| `clippy --workspace --all-targets` | **0 warnings** |
| Site build verifier | **46/46 checks** |
| Test matrix | **171 rows at `I`, 0 at `P`** |
| Shell commands registered | **60** |

Every command above exits **0** on success. That matters and is itself a recent fix — see §2.

**The matrix at 171 `I` / 0 `P` is a claim about recorded evidence, not proof that §28 is met.**
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
  file with cp1252 once. Use the `write`/`edit` tools, or Python `pathlib` with explicit UTF-8.

### 3.1 The commands to run before claiming anything

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 test --workspace     # exit 0, 1191 passed
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

All numbers are test counts. The matrix holds **191 rows across tables A-G** (A 23, B 14,
C 11, D 105, E 21, F 6, G 11) plus a separate **20-row H table** which IS the `handoff.md` §28
acceptance checklist, one row per headline requirement. **171 of the A-G rows carry a status and
all 171 are `I`**; the remaining 20 are table headers or rows whose status lives in a linked row.

**The H table is the one to read when judging completion.** It is short, it mirrors §28 clause
by clause, and each row names the evidence it requires.

| Crate | What it owns |
|---|---|
| `veloct-core` | Errors (with `hint()`), secrets/redaction, atomic writes, `ProjectRoot`, `FileRevision`, HTTP client trait + fakes, clock, ids |
| `veloct-markdown` | Frontmatter, directives, paragraph ids `<!--p:ULID-->`, references, rewrite, diagnostics, parse |
| `veloct-storage` | SQLite schema + migrations, rebuildable index, `ai_settings`, model row CRUD |
| `veloct-content` | Content model, WIP lifecycle, paragraph ids, `create_from_source`, works import/metadata |
| `veloct-git` | Change sets, publish commits, release tags, worktrees |
| `veloct-logging` | `[info]/[warning]/[error]`, mandatory redaction AT CONSTRUCTION, rotation, log page |
| `veloct-process` | Preview supervisor, port reservation, readiness probes |
| `veloct-release` | `release.json` (no commit sha), frozen inputs, deterministic fingerprint, one build → two roots, **`publish()`** = freeze → build → commit → tag |
| `veloct-sidecar` | The Astro launcher: resolves the bundled Node from the sidecar path, NEVER `PATH` (ADR-0005) |
| `veloct-deploy` | Three logical targets × main/works sub-targets, Cloudflare adapter, manual exports, roll-up (`partial` vs `failed`), re-send, **SEO push lifecycle** |
| `veloct-ai` | Providers, per-feature model mapping, CANCELLED-safe fallback, context, apply plans, **the page creator** |
| `veloct-attach` | Three independent state axes, reference sets, dedup, cache pool, upload transaction, **GitHub storage target** |
| `veloct-backup` | Selection rules, gzip archive + manifest, secret scan, staging restore, retention, target orchestration |
| `veloct-app` | Orchestration services: attachments, backup/restore, deploy, autobackup, plus `CommandError` |
| `src-tauri` | The Tauri shell: **60 registered commands**, `AppState`, preview wiring, credential store (keyring), sidecar resolution |
| `src/` | The workbench: 3-pane layout, **Tiptap WYSIWYG** editor with a Markdown view, draggable split with a REAL Astro iframe preview, autosave, conflict UI, log page, AI settings, page creator, attachments, publish, backups, deploy |

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
| H1 | Create and manage a project | **done** — `ProjectService`, 60 shell commands |
| H2 | Markdown edit, autosave 2 s / 30 s / save-on-close | **done** — `src/lib/autosave.ts`, 24 frontend tests |
| H3 | SQLite index rebuilds | **done** — `veloct-storage::rebuild` |
| H4 | Real Astro live preview, same project as production | **done** — sidecar smoke test: HTTP 200, article route 200 |
| H5 | WIP control, stable URL, finished redirect | **done** |
| H6 | Knowledge paragraph references + repair | **done** |
| H7 | Page creator | **done** — `veloct-ai::page` + `src/PageCreator.tsx` |
| H8 | AI per-feature provider/model + fallback | **done** |
| H9 | AI confirm / apply-all / apply-partial | **done** (apply lives in `veloct-ai::apply`; the UI exposes confirm) |
| H10 | Work import, screenshot, iframe preview, isolation | **PARTIAL — see §5.8** |
| H11 | Publish commit + Release tag | **done** — real publish integration run |
| H12 | One build per publish, all targets share it | **done** |
| H13 | Failed target re-publish by original Release | **done** — `resendable_sub_targets` |
| H14 | 1Panel manual upload of current `dist` | **done** as a manual export |
| H15 | Attachments: cache pool, **sftp, s3, lsky**, github, remote image | **PARTIAL — sftp, s3 and lsky have NO adapter.** GitHub and the local directory work; the pool, dedup and references work; remote image search (`network.rs`) exists but has no command |
| H16 | Dedup safe, multi-reference safe, no auto remote delete | **done** |
| H17 | Auto backup, manual backup, local export, full restore, content-only restore | **PARTIAL — all five exist and are verified, but see §5.1: the "cloud target" half of §17 is not wired** |
| H18 | No secret in config/log/backup; scan before backup | **done** — scan runs BEFORE the archive is written |
| H19 | SEO, comments, logging operational | **done** — SEO push lifecycle, giscus comments (off by default), log page |
| H20 | Key failure paths explicit, never silent | **done** — `hint()`, `anchor_error`, `unimplemented_reason()`, `capabilities().verified` |

**So: 17 of 20 are done, and H10, H15 and H17 are partial.** The three partials are the honest
answer to "is §28 met" today. Nothing above should be read as a claim that Veloct is complete.

---

## 5. What is NOT implemented

Ordered by how much of §28 they block.

### 5.1 Cloud backup targets (§17.4, §17.7) — the biggest remaining gap

`veloct-backup` has the whole model: `CloudTarget`, `BackupTargetAdapter`, `TargetHandle`,
`TargetSelection`, `run_targets`, `prune`, and `MemoryTarget` for tests. **Nothing implements
`BackupTargetAdapter` against a real service**, so an automatic backup writes the local archive
and reports an EMPTY target list. See the comment in `src-tauri/src/backup.rs`:

```rust
// Section 17.7's cloud targets are not wired yet, and reporting an empty list is honest
targets: Vec::new(),
```

§17.4 names exactly two: **GitHub** and **S3/S3-compatible**. The GitHub storage adapter in
`crates/veloct-attach/src/targets.rs` (`GitHubStore`) already does `put`/`head`/`delete_remote`
over the Contents API and could be adapted; **`list` is the missing piece** — the Contents API
lists a directory with `GET /repos/{owner}/{repo}/contents/{path}`, and `GitHubStore` does not
expose it. `CloudTarget::private_confirmed` is required by §17.2 and must be honoured.

### 5.2 S3 storage AND backup adapter (§13.11)

`crates/veloct-attach/src/targets.rs` declares `StorageKind::S3` **unimplemented** and reports
why. There is **no SigV4 code anywhere** — the only occurrences of "SigV4" are the comments and
the user-facing message explaining its absence.

**New information from the last round, which changes the estimate:** crates.io IS reachable, and
`aws-sigv4 1.6`, `rust-s3 0.37`, `hmac 0.13` are all available. `sha2` and `hex` are **already
workspace dependencies**. So S3 is a signing implementation plus GET/PUT/DELETE, not a research
problem. Adding a dependency is a decision worth making deliberately — read `Cargo.toml`'s
comments, which explain why `ssh`/`https` features were deliberately excluded (they pull OpenSSL
onto Windows/MSVC).

### 5.3 SFTP storage target (§13.10)

Also declared unimplemented and reported. Needs an SSH implementation; no `ssh` feature exists
**on purpose** (see above). Either accept a dependency that brings OpenSSL and verify it builds on
MSVC, or keep reporting the gap. **Do not add a guessed protocol.**

### 5.4 Lsky Pro storage target (§13.12)

Declared unimplemented and reported. Needs Lsky's own upload-token protocol, which is not verified
in `docs/spec/03-upstream-facts.md`. Same rule: verify the protocol or keep reporting.

### 5.5 AI model discovery (§11)

`crates/veloct-ai/src/provider.rs` has `build_models_request()`, and `veloct-storage::replace_models`
exists to store the result. **No command or button calls them.** The AI settings panel asks the
user to TYPE a model id. Wiring this is small: call the provider's models endpoint with the stored
credential, map into `AiModelRow`, call `replace_models`, refresh the picker.

### 5.6 The NSIS installer

`src-tauri/tauri.conf.json` sets `bundle.targets = ["nsis"]`, but **`tauri build` has never run**:
`tauri-cli` is not installed, and installing it needs the network (which IS available — see §5.2
— so `cargo install tauri-cli` or an `npx @tauri-apps/cli` devDependency would work). What is
verified is `cargo build -p veloct-desktop` plus a launch smoke test, NOT a packaged installer.

### 5.7 Smaller, stated gaps

- **A real remote operation has never run.** Every GitHub upload, deploy and cloud push is
  asserted against `FakeHttpClient`. No live token has been used, and nothing has been sent to
  Cloudflare, EdgeOne, github.com or any S3 endpoint. Nothing in the matrix claims otherwise.
- **One DeployPanel distinction is incomplete:** a target's `kind` is filled from configuration
  after a run, and the crate's own `needs_attention` is recomputed in the view rather than read
  from `LogicalTargetRun`.
- `docs/spec/02-test-matrix.md` §J's newest recorded run row still says
  `--exclude veloct-desktop`, from before `src-tauri` joined the workspace. The total (1191) is
  current; that one row's command text is stale.

### 5.8 H10 is partial: work import, screenshots and remote images have no command

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
   `anchor_error`, and the empty cloud target list all exist so a user or the next agent is told
   the truth. Do not replace one of those with a guessed request or a silent no-op.

5. **Prefer a real command over a claim.** Every matrix row at `I` names the test that proves it,
   and several name `#[ignore]`d runs against real processes. If you add a row, add the command.

---

## 7. Suggested order for the next agent

1. **Run §3.1 and confirm 1191 / 24 / 46 / 0.** If those numbers differ, find out why before
   building on them.
2. **Read `handoff.md` §28 again** and diff it against §4 above. `handoff.md` wins over this file.
3. **Finish the cloud backup targets (§5.1).** It is the largest complete gap, the model already
   exists, and GitHub needs only a `list` over the Contents API plus an adapter. It closes §17.7.
4. **Add S3 (§5.2)**, now that the signing crates are known to be reachable. Decide the dependency
   question explicitly and record it in `docs/spec/00-architecture.md` as an ADR — the workspace
   has a documented reason for excluding OpenSSL, so an S3 dependency must be justified against it.
5. **Wire model discovery (§5.5)** — small and self-contained.
6. **Attempt the NSIS installer (§5.6)** now that the network is confirmed reachable.
7. Keep SFTP and Lsky Pro (§5.3, §5.4) either implemented properly or **explicitly reported**.
   Never guess a protocol.

**When you touch any of §5, update `docs/spec/02-test-matrix.md` with the row and the real
command you ran.** That file is the completion checklist, and it is only worth anything if every
`I` is backed by a command someone can re-run.
