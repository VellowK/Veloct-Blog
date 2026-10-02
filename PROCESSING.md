# PROCESSING.md — state of Veloct, and what the next agent should do

A **status and orientation document** for whoever picks this up next, written after five consecutive
rounds of work: the cloud targets, the AI settings, §13's storage targets, §10's works system, and
§13.14's network image browser.
It is not a contract: `handoff.md` remains the locked product contract and always wins, and
`docs/spec/*` records the decisions `handoff.md` leaves open.

**Read in this order:**
1. `handoff.md` §28 — the completion definition. It is the only authority on what "done" means.
2. This file — §1 for where the work stands, **§5 for what is genuinely missing** (§5.11 is a decision
   that has been TAKEN; **§5.12 is a decision whose Option A was TAKEN on 2026-10-02 — the FACT is now
   recorded in `docs/spec/03-upstream-facts.md` §4.2 and matrix row D165, while the behaviour is
   deliberately unchanged — and it is the one thing
   to read before touching Pexels**), §7 for what to do next.
3. `README.md` — how to build and test on this machine.
4. `docs/spec/00-architecture.md` — the ADRs, including ADR-0015 and ADR-0016.
5. `docs/spec/03-upstream-facts.md` — what has been verified against a real upstream source, live,
   including the two image providers' full contracts (§4).

**Three things about the state you are inheriting, so nothing here reads as vaguer than it is:**
- **The tree is CLEAN, and its CODE is byte-identical to `585de01`** (`git diff --stat 585de01 HEAD` is
  `PROCESSING.md` only), where all six of §3.1's checks were run and reproduced — **and they were run AGAIN
  on `8ba4219` for this handover, reproducing every number exactly** (§1, §J). **The correction you are
  reading changes documentation only**, so it moves none of the code those numbers describe.
- **Superseded runs are kept, not tidied away.** §1.2 and the matrix's §J both carry rows for runs that
  were later replaced, because the history of what a number used to be is a fact about the repository.
- **Two real test races were found by re-running a suite that had already been recorded green** (§2.13,
  §2.14). Both are fixed. The habit that found them is the most useful thing in this file.

**Before changing anything**, run §3.1 and make the results match §1. **They are current as of `585de01`
and were run on that clean tree**, so a mismatch is news about the environment rather than a formality —
§1.2 explains why, and it is a story about two races that only re-running found.

**The single most useful habit for this repository:** when a contract clause says a feature exists,
`grep` for a caller before believing it. Four of the five rounds here found their work that way — a whole
model (`CloudTarget`, `Provider::list_models`) implemented, tested and called by nothing; the §13 round's
validation function whose only reader was a test (§5.10, §2.9); the works round's `import_work`,
`list_works`, `export_work` and the `/work-previews/<slug>/` poster route; and §13.14's whole
`veloct-attach::network` model, which had no caller at all.
**Its mirror image is just as valuable:** read the VENDOR before writing the code that talks to it
(§2.12). Reading three providers' documents changed this round's DESIGN — the answer was "the sources
were wrong, not the clause" — and that is far cheaper to discover before the code exists than after it
has tests.

---

## 1. Where the work stands

| Fact | Value |
|---|---|
| Last commit these numbers describe | **`8ba4219` plus THIS round's change set**, which carries §13.12's Lsky Pro adapter (D166–D167) and is recorded in §J. `8ba4219` itself is **`585de01`** "Verify Unsplash against the LIVE service, the first real remote call in this repository" plus a documentation-only change, and all six of §3.1's checks were reproduced on it before this round began (§1.2) |
| Working tree | **`git status --short` was EMPTY at `8ba4219`**, where the previous handover's numbers were reproduced; **this round's change set was NOT committed when its own run was taken**, so the numbers below are reproducible from the change set the commit carries rather than from a sha — §1.2 records why that distinction is written down rather than glossed. `git diff --stat 585de01 HEAD` was `PROCESSING.md` ONLY until this round; **§13.12's adapter is the first CODE change since `585de01`**, and it moves that line. Both the works and network-image rounds were FIRST measured on unclean trees, which §1.2 records rather than hides; both were then re-run on their commits, and the network-image re-run exposed two real test races (§2.13, §2.14) |
| Rust tests | **1425 passed, 0 failed, 11 ignored** (+30 over `8ba4219`'s 1395: `veloct-attach` +21, `veloct-desktop` +4, **`veloct-core` +1 and `veloct-s3` +4 from the live S3 round and the save-path half of the `{bucket}` rule**; **the workspace is 17 members — 16 crates plus `src-tauri`** — and its per-crate rows in §J sum to 1424, the 1425th being the one `live_providers` test that runs with no key and no network). The 11 ignored are the 4 real-process tests and the 7 live-vendor ones: the 3 real-process integration tests, 1 each in `publish_integration` and `end_to_end_release`, **2 in `crates/veloct-attach/tests/live_providers.rs`**, **2 in `crates/veloct-attach/tests/live_github.rs`** (attachment round trip, §17.2's refusal), **2 in `crates/veloct-backup/tests/live_github.rs`** (the backup round trip and the same privacy rule), and **2 in the S3 pair: `crates/veloct-attach/tests/live_s3.rs` and `crates/veloct-backup/tests/live_s3.rs`** — a vendor's uptime must never gate `cargo test` |
| Frontend tests | **160 passed** (vitest: 24 `frontmatter` + **65** `storageTargetForm` + 36 `workForm` + 35 `networkImageForm`; the three new ones are the `{bucket}` transcription block, which compares the window's verdicts against the crate's for nine endpoint shapes and asserts the refusal names both the placeholder and where it belongs) |
| `clippy --workspace --all-targets` | **0 warnings** |
| Site build verifier | **52/52 checks** — `site/` is untouched by §13.12, and the build was re-run rather than assumed; §J names the command behind every number |
| Test matrix | **247 rows at `I`, 0 at `P`** (224 + 9 for §13.14, the two races it exposed and the live verification, + 1 for §5.12's re-verified Pexels pause, + 3 for §13.12 and the §5.10 members it closed: D166–D168, + 1 for §13.10's SSH/SFTP REMOVAL: D169, + 9 for the live GitHub verifications, the OSS addressing rule and its save-path half, the `x-amz-date` defect a live call found, the two live S3 round trips, and the signing round: D170–D178), out of **267** rows across tables A–G |
| Shell commands registered | **87** (unchanged: §13.12 needed no new command, because the storage commands are generic over the kind) |
| Installer | **`target/release/bundle/nsis/Veloct_0.1.0_x64-setup.exe`, 27.94 MiB (29,305,464 bytes), SIGNED** (`Valid`, `CN=VellowChire Code Signing, O=Veloct`, SHA-256, timestamped — a LOCAL self-signed certificate; §5.10.4 states exactly what that does and does not buy, and D178 records the experiment that showed the app INSIDE it still comes out `NotSigned`). **Rebuilt and re-probed FOUR times on 2026-10-02**: so the package carries §13.12, again after §13.10's SSH/SFTP target was REMOVED, a third time after the live-S3 fix moved `veloct-core` and therefore the committed sidecar LAUNCHER (so the packaged launcher is byte-identical to the committed one — verified by hash, §2.2), and a fourth for the signing round. Each build was followed by the WHOLE §2.10 sequence: silently installed to a temp directory, listed, **the INSTALLED launcher started the real Astro dev server and `GET http://localhost:4399/` returned 200**, then tree-killed (port free, checked with `State -eq 'Listen'`), uninstalled and the directory removed. **Still unsigned inside: `veloct-desktop.exe`, `veloct-astro-sidecar.exe` and `uninstall.exe`** — D178 explains why, and names Tauri's `certificateThumbprint` as the supported fix |

Every command above exits **0** on success. That matters and is itself a recent fix — see §2.

**Every number in the table above was measured on the change set §13.12's commit carries**, on top of a tree
that had already reproduced the previous handover's totals at `8ba4219`. §1.2 tells the older story in full,
including the part that is uncomfortable: the first measurements of the works and network-image rounds were
taken on UNCLEAN trees, and re-running them on their commits is what found two real test races. **This round's
run was taken the same way it recorded that lesson: with `git status --short` NOT empty, so §J says so rather
than implying a commit that did not exist yet — and with every one of the 22 new tests named rather than
counted.** **If §3.1 does not reproduce 1417 / 157 / 52-of-52 / exit-0-clippy from this change set, something
outside this repository changed and that is the first thing to find out.**

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

6. **The save path now ENFORCES the crate's own rules** (`f29dcda`, §2.9) — `StorageConfig::validate`
   got the production caller it never had: `save_target_config` runs it for an implemented kind BEFORE
   anything is written, so a refused configuration leaves no row and no credential, and the drive/UNC
   prefix rule t2 added is now met at the SAVE rather than at the first upload. A kind with no adapter
   is deliberately exempt, and the same commit drove two refusals that had only been written:
   §13.8's cleanup against an attachment with no recorded object, and `delete_storage_target`'s
   credential removal plus its unknown-id error. **+6 Rust tests (1324 → 1330)**, clippy clean.
7. **The form refuses a non-absolute public URL prefix** (`7813fcd`) — `validateDraft` now rejects a
   non-empty `publicUrlPrefix` that is not `http://`/`https://` in the field the user typed, instead of
   letting the crate's English message arrive after a save that could never have succeeded. Empty stays
   allowed, only a kind that SUBMITS the field is checked, and the rule's two implementations are held
   together by a transcription of nine shapes (`CRATE_RULE`) compared against the crate's verdicts.
   **+9 frontend tests (71 → 80)**, `tsc` clean. See §5.10 for the limit this still leaves.

**ALL OF IT IS COMMITTED**: `2496604` (the §13 round and the storage-kind follow-up), then `f29dcda`
and `7813fcd` (the two repairs above). The numbers in §1 are measured on **`7813fcd`**, so they are
reproducible from a checkout rather than from one working directory. `git status --short` is **empty**,
and every commit after `7813fcd` is documentation only — `git diff --stat 7813fcd HEAD` names exactly
this file and the matrix.

The paths those commits changed, so the next agent knows what is new without reading the whole diff:

```text
crates/veloct-attach/src/lib.rs,storage.rs,targets.rs,transaction.rs   the Local kind, StorageKind::ALL, validate()'s doc, the routing model, §13.8's transaction
crates/veloct-app/src/attachments.rs,lib.rs                            the service half, RemoteDeleteView, ObjectView.storage_target_id, the no-recorded-object refusal test
src-tauri/src/attachments.rs,lib.rs                                    storage_target_for, save/delete_target_config, the three commands, StorageTargetView's S3 fields
src/AttachmentPanel.tsx,lib/api.ts,styles.css                          the form, the cleanup confirm step, the eligibility reason, the kind-gap report
src/lib/storageTargetForm.ts                                           NEW  the form's decisions, as a pure module (56 tests)
tests/storageTargetForm.test.ts                                        NEW  those tests, including the nine shapes transcribed from the crate
```

8. **§10's works system is reachable from the window** — the round this file's new numbers describe.
   `import_work`, `list_works` and `export_work` had **no caller above `veloct-content`**, so a user
   could not import a work, see one, give it a poster, or export it: §5.9's gap, and H10 with it.
   A new `crates/veloct-app/src/works.rs` service (19 tests) plus a `src-tauri/src/works.rs` command
   module (10 commands, 5 tests) plus `src/WorksPanel.tsx` and `src/lib/workForm.ts` (36 tests) close
   it. **+19 `veloct-app`, +5 `veloct-desktop`, +2 `veloct-sidecar`, +37 frontend tests.**

9. **§10.1's `preview/` became reachable, and §10.3's poster route is EMITTED** — the same round found
   that `import_work` wrote EVERY planned file under `site/`, so the sibling `preview/` directory the
   layout names could not be produced by an import; and that `site/`'s works index pointed at
   `/work-previews/<slug>/screenshot.webp` while **nothing wrote that route**. The fixture work's
   `work.json` had named `preview/screenshot.webp` since it was created, and the poster wall had shown
   a placeholder ever since. `veloct_content::works::PREVIEW_PREFIX` now lifts `preview/` out of the
   body, `split-publish-roots.mjs` copies it to the main site root, and the verifier goes from
   **46/46 to 52/52 checks**, including a BYTE-FOR-BYTE comparison against the work's own file.

10. **The installer EXISTS, and building it exposed two defects that made a packaged app unusable** —
    §5.7's gap. `scripts/build-installer.ps1` (NEW) wraps the Tauri CLI with the environment fixes
    `scripts/cargo.ps1` documents; the package WAS **27.90 MiB** at that point and installs `veloct-desktop.exe`,
    `veloct-astro-sidecar.exe` and `veloct-node.exe`. See §2.10 and §2.11 — neither defect could be
    seen from a `cargo build`, and both would have shipped.

11. **§13.14's network image browser is IMPLEMENTED, and §5.11's decision was TAKEN by deciding which
    sources to use rather than by softening the clause.** Reading Unsplash's and Pexels's own documents
    showed that §13.14's prescribed flow — *search → the user picks → write the remote URL straight into
    the Markdown, 不进入附件缓存池,不下载,不再上传* — is **required** by Unsplash and permitted by Pexels,
    and **forbidden** by Pixabay. So the clause was kept and Pixabay was refused, in the provider's own
    words. The two obligations that follow are enforced in the crate rather than left to a caller:
    Unsplash's credit line **cannot be switched off**, and choosing one of its images **sends the
    download report** their guidelines demand. **+21 `veloct-attach`, +17 `veloct-desktop` and +35
    frontend tests.** See §5.11 for the decision and §2.12 for what the vendors' documents actually say.

12. **Unsplash was VERIFIED AGAINST THE LIVE SERVICE, which is the first time anything in this repository
    has been.** `crates/veloct-attach/tests/live_providers.rs` (NEW) is an `#[ignore]`d integration test —
    the ordinary suite must never depend on a vendor's uptime — that makes a real search through the
    crate's own request builder and parser and then the download report Unsplash's guidelines require.
    Run on 2026-10-01 it returned **10 results**, kept the `ixid` parameter, used the 1080 px rendition,
    named the photographer with a profile link, rendered an attribution carrying
    `utm_source=veloct&utm_medium=referral`, and **the download endpoint accepted the report**. §5.8's
    "no live service has ever been contacted" is now closed for this one source and stands for every
    other. A missing key **fails** the test rather than skipping it, because a live test that passed
    without running would be evidence of nothing.

13. **§13.12's Lsky Pro adapter is IMPLEMENTED, and its contract came from a source §5.4 did not know
    existed.** The vendor publishes NO API reference — but the reference ships INSIDE the product, as the page
    `Common\ApiController` renders at `{instance}/api`, which is what the Halo note "具体接口定义在 Lsky Pro
    后台可以看到" means literally. `crates/veloct-attach/src/lsky.rs` (NEW, 17 tests) implements upload,
    delete, the credential test and an honest refusal where the API cannot answer, over the contract recorded
    in `docs/spec/03-upstream-facts.md` §5. **+22 Rust tests (1395 → 1417) and +5 frontend tests (152 → 157)**,
    clippy clean, sidecar check green, 52/52 site checks. §5.4 is the gap this closed; **the PAID line is a
    stated limit rather than a claim**, and D166–D167 are the recorded evidence.
14. **§5.10's three remaining members were closed, and the installer was rebuilt so it carries §13.12.**
    **(a)** §5.10.2's ORDER half — the one PROCESSING.md itself called "a one-line test... would close it" —
    is closed by a test that passes a credential WITH a configuration the crate refuses and asserts that the
    target-list file was never created and that the credential is not in the OS store, with a positive control
    (D168). **(b)** The shared target lookup was extracted out of `upload_attachment` and
    `delete_remote_object` into `resolve_stored_target`, which takes no `AppState`, and is driven with an
    injected credential resolver (D168). **(c)** §5.10.1's third two-language member is closed by moving
    §13.12's "absolute http(s)" rule INTO `StorageConfig::validate`, so the crate is the authority and the
    window's copy is a transcription (D168). **(d)** §5.10.3 is a DECISION now rather than an open item: the
    cleanup wording states that the article keeps pointing at the address and goes dead. **+2 Rust tests
    (1417 → 1419)**, and the installer (27.93 MiB) was built, installed, probed and uninstalled.

15. **§13.10's SSH/SFTP target was REMOVED from the locked contract at the owner's request, and the
    "dependency decision" that blocked it for two rounds turned out to rest on a mis-read comment.** The
    reasoning — "an SSH implementation pulls OpenSSL onto Windows/MSVC" — came from the root `Cargo.toml`'s note
    about **git2**'s own features, and an SFTP client has its own choices: `ssh2` needs `libssh2-sys`, while
    **`russh` 0.63 pulls neither OpenSSL nor libssh2** (RustCrypto throughout; `ring`/`aws-lc-rs` are optional
    features) — and this repository's own `docs/research/tauri2-windows-report.md` had ALREADY recommended
    `russh`/`russh-sftp` as the pure-Rust path. So the obstruction was never real, and what made the removal
    right is simpler: it was the last kind with no adapter and no server to verify against. §13.10 is now the
    local / server DIRECTORY target. **A project that already saved an SFTP target still LOADS** — the variant is
    retained, load-only, because the target list is parsed as one document and one unknown variant would take a
    user's other targets with it — reports the removal, can be deleted, and cannot be re-saved. **The D142
    exemption died with it**, so `validate()` now runs for EVERY kind; D169 and ADR-0017 hold the detail.
16. **GitHub was VERIFIED AGAINST THE LIVE SERVICE — the second service, the first CLOUD TARGET, and the
    first time this repository has written to a vendor.** `crates/veloct-attach/tests/live_github.rs` (NEW,
    `#[ignore]`d) drove the §13.13 attachment target through a real `put` → `head` → `delete` → `head` against
    `api.github.com`: **exit 0**, `connection: … is reachable`, `put: created
    veloct-live-probe/live-probe-1790910378530.webp (74 bytes)`, `head: 74 bytes` — **the size GitHub itself
    reported**, so a truncated upload could not have passed — and a SECOND live read proving the delete reached
    the service. Nothing was left in the repository. And `crates/veloct-backup/tests/live_github.rs` (NEW,
    `#[ignore]`d) turned the same run into a finding: **§17.2's privacy rule REFUSED the backup against a real
    PUBLIC repository**, in the adapter's own words, because an unencrypted archive must not be published — the
    rule verified against GitHub's own flag rather than against a fake. **What remains, stated exactly:** the
    backup target's `upload`/`list`/`remove` still have no live run, because that needs a PRIVATE repository and
    the token supplied has Contents write but NOT Administration, so Veloct could not (and did not) change the
    repository's visibility; S3, Cloudflare and EdgeOne remain fake-client-only. **The token was read from the
    environment, never printed, and never written to any file inside this repository** — `git grep` for it
    returns nothing. **And it then ran the OTHER direction too:** once the repository was made private (the
    owner granted the token Administration after a `PATCH` had answered **403** — a repository's
    `permissions.admin` describes the user, not the token), the §17.4 BACKUP target's own `upload` → `list` →
    `remove` round trip passed on the same day: `connection: … is reachable and private (default branch main)`,
    `uploaded: … (158 bytes)`, `listed: … size=158 created_at=2026-10-02T00:00:00Z`. So GitHub's protocol, its
    §17.2 privacy rule AND its backup path are each verified against the vendor (D170–D172) — the first live
    cloud verification in this repository's history, and the first time it has written to a third party.

17. **S3 was verified against a real object store, and the first live call found a defect that had made every
    S3 request this application ever built fail.** A bucket was offered (`vellow-chengdu-archieve`, Alibaba Cloud
    OSS), which was reachable only through a chain worth recording: the rclone configuration on this machine
    names an **AList WebDAV remote**, not an OSS bucket, and the bucket is a storage mounted INSIDE that AList —
    so Veloct could not have used it as Veloct (there is no WebDAV target in the contract) and the bucket's own
    S3 credentials had to be supplied. Three facts were then MEASURED before any code was written: the
    `-internal` endpoint is unreachable from outside Alibaba's network; a path-style bucket request is refused
    with **`403 SecondLevelDomainForbidden`**, so OSS requires the bucket in the HOSTNAME; and SigV4 with the
    service scope `oss` is refused (`400 InvalidArgument`) while **`s3` answers 200** — which is the scope
    `veroct-s3` already signed with, so the signing was right and only the addressing was wrong. The addressing
    is now the user's to state, with **`{bucket}`** in the endpoint (ADR-0018, D173), and a misplaced
    placeholder is refused by name rather than silently rewritten into a host that does not exist.
    **Then the first live request came back `403 AccessDenied: OSS authentication requires a valid Date`** — and
    the value Veloct had sent was `x-amz-date: 20261002T034316.5888216Z`, twenty-four characters where SigV4
    allows sixteen, because `amz_timestamp` built it by deleting `-` and `:` from an RFC 3339 instant and a real
    clock carries sub-second digits. **The whole suite was blind to it by construction**: the SigV4 tests inject
    an already-correct `20130524T000000Z` and every adapter test uses a `FixedClock` with zero nanoseconds, so
    no test input ever contained a fraction — and AWS validates the same format, so the defect was not
    OSS-specific. Fixed by formatting instead of editing (`veroct_core::clock::format_amz_timestamp`, which
    `format_release_stamp` now delegates to so the two shapes cannot drift), and guarded by two tests that are
    deliberate about their input: one feeds nanoseconds and a `+8` offset, the other reads the **SYSTEM** clock
    and prints the signed request. **Both S3 roles then round-tripped live** — the §13.11 attachment target
    (D175) and the §17.4 backup target, whose archive is streamed and whose listing carries the BUCKET's own
    `LastModified` (D176). §5.8's recorded gap was not merely a lack of confidence: it was hiding a total
    failure of the feature.

The paths THIS round changed:

```text
crates/veloct-attach/src/network/providers.rs   NEW  the two providers' contracts + compliance rules (36 network tests)
crates/veloct-attach/src/network.rs             ImageSource::ALL, is_implemented, resolved_alt, the new result fields
crates/veloct-attach/src/lib.rs                 the provider re-exports (and why four are NOT hoisted)
src-tauri/src/images.rs                         NEW  five commands, and the bodies tests can drive (17 tests)
src-tauri/src/lib.rs                            the five registrations (87 commands now)
src/lib/networkImageForm.ts                     NEW  the window's decisions (35 tests)
src/NetworkImagePanel.tsx                       NEW  the browser: sources, key form, result grid, insertion
src/lib/api.ts                                  the five bindings and the view types
src/App.tsx, src/styles.css                      the 网络图片 button, the panel wiring, its styles
tests/networkImageForm.test.ts                  NEW  35 tests
docs/spec/03-upstream-facts.md                  §4 rewritten: both contracts, and why Pixabay is refused
docs/spec/02-test-matrix.md, PROCESSING.md       D155 updated, D156–D161 added, the tables corrected
```

The paths the PREVIOUS (works) round changed, kept here because they are what `5edff10` carries:

```text
crates/veloct-app/src/works.rs                    NEW  the works service, and the poster's rules (19 tests)
crates/veloct-content/src/works/import.rs         PREVIEW_PREFIX, and the body/preview write split
crates/veloct-sidecar/src/lib.rs                  node_bundled_file_name + node_file_names (§2.11)
src-tauri/src/works.rs                            NEW  ten commands (5 tests)
src-tauri/tauri.conf.json                         binaries/veloct-node added to externalBin (§2.10)
src/WorksPanel.tsx, src/lib/workForm.ts           NEW  the panel and its decisions (36 tests)
site/src/integrations/split-publish-roots.mjs     copyWorkPreviews → dist/site/work-previews/
site/scripts/verify-build.mjs                     six checks for the poster route (52/52)
scripts/build-installer.ps1                       NEW  the documented way to build the NSIS package
package.json                                      @tauri-apps/cli and @tauri-apps/plugin-dialog as devDependencies
works/01J8Z5Q0K3W7M9V2B4N6P8R2AG/preview/         the fixture work's real poster images (sharp, once)
```

The many `*.log` files in the working directory are this session's captured verification output, and
`.gitignore` line 25 ignores `*.log`, so they are NOT part of the change set; they are safe to read
(they are the raw evidence for the numbers in §1) and safe to delete.

### 1.2 The numbers were measured on an UNCLEAN tree — twice, and one of the two has since been CONFIRMED

**The works round** was verified **before** its commit, so `git status --short` was not empty at the
moment its numbers were taken. What that does and does not weaken:

- **What is unaffected.** The tree that produced 1356 / 117 / 52-of-52 / 0 clippy warnings is exactly
  the tree the commit carries: no file was edited between the last run and the commit, and the
  per-crate rows sum to 1356 exactly — so the runs describe a real, self-consistent tree rather than an
  aspiration. Every command exited 0, and the site verifier's byte-for-byte poster comparison is a
  stronger check than a count.
- **What was weaker, and is now closed.** The numbers were not reproducible from a checkout *while the
  change set was uncommitted*, which is the standard the previous three rounds were held to. **They were
  then re-run on the committed tree — commit `5edff10` — and every one reproduced**: Rust **1356 passed,
  0 failed, 5 ignored**, frontend **117 passed**, `tsc` clean, the site verifier **52/52**, and
  `build-sidecar.ps1 -Check` reporting the committed launcher matches a fresh build. **`git status
  --short` was EMPTY at the moment of that run**, so those numbers are reproducible from a checkout and a
  later agent can check them the way `bf75265` did for the four cloud-target commits.

**The network-image round's numbers (1394 / 152 / 52-of-52 / 0 warnings) were measured on a tree that was
still uncommitted, and re-running them on commit `c765344` DID NOT reproduce them — TWICE, in two
different ways.** The first re-run exited 101 with 1324 passed (an environment-variable race, §2.13); the
fix for that revealed a second, exit 101 with 893 passed (the OS credential store, §2.14, whose second
victim was pre-existing). **Both are fixed, and the numbers were then confirmed on a CLEAN tree at
`d844f7a`: two consecutive `--workspace` runs at 1394 passed, 0 failed, 5 ignored; clippy 0 warnings;
`build-sidecar.ps1 -Check` green after rebuilding the launcher; 152 frontend tests; `tsc` clean; 52/52
site checks; `git status --short` EMPTY.** Plus 5 further `--workspace` runs and 20 `-p veloct-sidecar`
runs during the diagnosis, all green.

**The live-verification round then moved the totals slightly and was confirmed the same way.** Adding
`crates/veloct-attach/tests/live_providers.rs` put the suite at **1395 passed / 7 ignored** (one offline
test runs; the two live ones are `#[ignore]`d), and on commit **`585de01`** all six checks were run again
on a clean tree: **`cargo test --workspace` 1395/0/7 exit 0, `clippy --workspace --all-targets` exit 0
with 0 warnings, `build-sidecar.ps1 -Check` green, `test:ui` 152 passed, `build:ui` clean, and the site
verifier 52/52.** `git status --short` was empty at that moment and is empty now.

**So §1's table IS reproducible from a checkout.** The two failures that re-running found are the
strongest evidence in this file for the rule that produced them: had either round recorded its number and
moved on, the next agent would have inherited a suite failing roughly one run in three, with no
diagnosis — and one of the flaky tests would have looked like a product bug in a path
(`delete_storage_target` forgetting a credential, D143) that was working correctly all along.

**The matrix at 233 `I` / 0 `P` is a claim about recorded evidence, not proof that §28 is met.**
It has been wrong in that direction before: at one point it read 93 `I` while the desktop
workbench did not exist at all. When you find a gap, the matrix row is what is wrong.
Three of those limits were closed by earlier repairs and the rows that carried them were corrected
rather than left standing (D18, D131, D134, D137); the eighteen rows the last two rounds added
(D146–D163) each name the test AND the crate, D155 is an external fact rather than a test, and §5.11 is
where its consequence is argued.

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

**A TEST-ONLY edit to that crate is still an edit, which is not obvious and cost time this round.** The
network-image round changed nothing in the launcher's behaviour — the edit was a doc comment and a line
inside `#[cfg(test)] mod tests` — and `-Check` still went red, because the built binary's hash changed
anyway. **The build is deterministic** (two consecutive rebuilds give an identical hash, verified), so
the difference is real but invisible from the outside; the new binary is the same SIZE (2 801 152 bytes)
and behaves identically. **The rule is therefore "any touch to `crates/veloct-sidecar` ⇒ rebuild and
commit the launcher"**, not "any change to its behaviour". Rebuilding is cheap; a stale launcher is
silent.

**The staleness is TRANSITIVE, which is the stronger form of the same trap** (found 2026-10-02). The
live-S3 round changed `crates/veloct-core/src/clock.rs` — one function, `amz_timestamp`, nothing to do with
launching a site — and `-Check` went red, because the launcher LINKS `veloct-core` and its bytes moved with
it. So the honest rule is **"any touch to `crates/veloct-sidecar` OR to anything it depends on"**, and
`veloct-core` is the crate most likely to be edited for an unrelated reason. The launcher was rebuilt
(2.67 MB) and re-checked green in the same round; the installer was rebuilt with it, so the packaged launcher
and the committed one are the same bytes.

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

### 2.9 A saved storage target is now REFUSED if the crate's own rules reject it — the one behaviour change of this round

`StorageConfig::validate` had no production caller. Every rule it states — the `Local` arm refusing a
prefix that names a drive letter or a UNC root, S3 needing a bucket, GitHub needing a repository — was
read by nothing but a unit test, so the window accepted the configuration and the user met the mistake
at the first upload instead of at save. `f29dcda` gave the save path the call
(`save_target_config`: validate → credential → list), and the rule is enforced **before anything is
written**: a refused save leaves no row in `.veloct/storage-targets.json` and no credential in the OS
store.

**The behaviour change, stated plainly:** a save that previously SUCCEEDED for an implemented kind
whose configuration `validate()` rejects is now refused at save time, with the configuration problem as
the message. **Every shape it can newly refuse, for the three kinds this build implements** (GitHub,
S3, Local) — an empty id; an empty name; S3 without a bucket; GitHub without a repository; a `Local`
prefix containing `:` or starting with `\`; any kind's prefix containing `..` or starting with `/`; and
a non-empty public URL prefix that does not start with `http://` or `https://`. The window's
`validateDraft` already refuses all seven for the kinds the form can save, so the normal UI flow sends
nothing that is newly refused — this closes the gap for anything else that reaches the command surface.
(Two of the seven cannot even be typed into the form: an empty id or name blocks the save there.)

**What used to be deliberately NOT refused, and no longer needs to be:** a kind this build does not implement.
`save_target_config` called `validate()` only `if is_implemented(config.kind)`, so a half-filled SFTP target
could be saved to keep §13.10's gap visible. **That exemption is GONE (ADR-0017/D169):** the SSH/SFTP target was
removed from the contract, the command surface can no longer produce one (`parse_storage_kind` refuses its
spellings), and the retired kind's arm reports the removal — so `validate()` now runs for EVERY kind, and a
retired configuration is refused with a message naming the removal rather than being written. **A reader who
finds the old text saying "validating everything would delete the only way to record an unimplemented target"
should know what replaced it:** the way to record one is a kind that is OFFERED with a reason (Pixabay's and
Lsky Pro's former shape), not a kind that no longer exists.

### 2.10 A Tauri bundle RENAMES an `externalBin`, so `externalBin` is not the whole story

Two separate defects, both of which a `cargo build -p veloct-desktop` and a launch smoke test cannot
see, and both of which would have shipped in §5.7's installer:

1. **`externalBin` listed only the Astro sidecar.** `src-tauri/tauri.conf.json` said
   `["binaries/veloct-astro-sidecar"]`, while `crates/veloct-sidecar/src/lib.rs` exists precisely to
   resolve a BUNDLED Node runtime (ADR-0005: "Node.js is a bundled sidecar, never a user prerequisite").
   The first NSIS package was **5.49 MiB** and installed `veloct-desktop.exe` and
   `veloct-astro-sidecar.exe` — **no Node at all**. Adding `binaries/veloct-node` took it to
   **27.90 MiB at that point** and the install directory gained `veloct-node.exe`. **A packaged application could
   never have started a preview**, and the error a user would have seen named a file the package simply
   did not contain.

2. **The installed name has the target triple STRIPPED.** This is the part that is easy to miss: the
   build tree ships `binaries/veloct-node-x86_64-pc-windows-msvc.exe` (the suffix is what a Cargo build
   needs), and Tauri installs it as **`veloct-node.exe`**. `resolve_node` searched only the suffixed
   name, so it found the runtime in a development tree and **never** in an installed application — a
   preview that works for the person who built it and fails for the person who installed it, which is
   the exact failure ADR-0005's "never from `PATH`" rule exists to prevent in the other direction.

**Both directions were measured rather than reasoned about.** Against the pre-fix package, the
package's OWN launcher exits 2:

```text
veloct-astro-sidecar: not configured: the bundled Node runtime
  expected veloct-node-x86_64-pc-windows-msvc.exe beside the application; looked in
  …\veloct-install-probe2\veloct-node-x86_64-pc-windows-msvc.exe, …\veloct-node-x86_64-pc-windows-msvc.exe,
  …\binaries\veloct-node-x86_64-pc-windows-msvc.exe, …\veloct-astro-sidecar.exe\veloct-node-…exe
```

…while `veloct-node.exe` sat in that very directory. After the fix, the same installed layout launches
the real Astro dev server (`[vite] connected`, `[content] Syncing content`). `node_bundled_file_name`
and `node_file_names` are the two names, two tests drive them, and one pins the ORDER so a build tree
holding both cannot silently pick the wrong runtime.

**The generalisable rule:** "the bundle builds" and "the installed application works" are different
claims. §5.7 was recorded as an unmet clause for rounds while `cargo build` was green, and the first
attempt at closing it shipped a package with a missing runtime — so a build command is not evidence
that a package is usable until something INSTALLS it and runs it.

### 2.11 The `write` tool mangled `§` into `搂`, in a NEW file

The mirror image of the `Set-Content` corruption §3 warns about. Writing
`crates/veloct-app/src/works.rs` produced **twelve** occurrences of U+6402 (`搂`) where `§` (U+00A7) was
intended, with no BOM and no replacement character — so `read` showed correct-looking prose in the
places that mattered and the damage was only visible from the CHARACTER CODES:

```powershell
$t = [System.IO.File]::ReadAllText($p)
($t.ToCharArray() | Where-Object { [int]$_ -eq 0x6402 }).Count   # 12, and 0 for U+00A7
```

It was repaired by a same-value swap under `[System.IO.File]::WriteAllText` with
`UTF8Encoding($false)`, and the repair is verified by counting BOTH code points afterwards (0 and 12).
**`Get-Content` cannot be used to check this** — PowerShell 5.1 decodes a BOM-less UTF-8 file with the
system code page, so a correct `§` and a mangled one both print as mojibake. Count the code points, or
use the `read` tool.

### 2.12 Reading the VENDOR changed the design, and the answer was "the sources, not the clause"

This is the round that shows the habit in §6.6 pointing the other way. §13.14 prescribes
*search → the user picks → write the remote URL straight into the Markdown*, and §13.1's parenthetical
names Pixabay as the example. Reading the three providers' own documents produced a design change
rather than an implementation:

| | Hotlinking | What that means for §13.14 |
|---|---|---|
| **Unsplash** | **required** — "we **require** the image URLs returned by the API to be directly used or embedded in your applications", and "this applies to all uses of the image and not just search results" | The clause is not merely allowed, it is what they ask for — plus two obligations: credit that **cannot** be omitted, and a download report when a user picks an image |
| **Pexels** | permitted; no download step documented | The clause holds; attribution and a link back are the obligations. **Currently unusable: Pexels has paused new API key issuance, so the source is implemented and tested but not obtainable — §5.12** |
| **Pixabay** | **forbidden** — "permanent hotlinking of images (using Pixabay URLs in your app) is not allowed… please download them to your server first" | The clause is the prohibited use, so the source is refused |

**So the fix was to choose the sources, not to soften the clause** — and the deciding question was not
"which provider is easiest" but "whose terms does the locked clause actually describe". Two further
details made this concrete rather than theoretical: a Pixabay `webformatURL` is **valid for 24 hours**,
so a saved Markdown file would break within a day even if the terms had allowed it; and the same source
requires a 24-hour cache, which a per-keystroke search box would violate.

**Three engineering consequences that are easy to get wrong and are now asserted:**

1. **The two providers authenticate differently** — `Authorization: Client-ID <key>` for Unsplash and a
   bare `Authorization: <key>` for Pexels. Copying one to the other produces a 401 that reads like an
   invalid key.
2. **The provider's obligations belong to the CRATE, not to a caller.**
   `NetworkImageChoice::from_result` forces Unsplash's credit line even when asked not to, and choosing
   an image reports the download. A caller cannot get this wrong, which is the point.
3. **A failed compliance step is a sentence, not a silent failure and not a lost image.** When the
   download report fails, the Markdown is still produced and the user is told (§20) — refusing the choice
   would throw away their work over a statistic the image does not depend on.

**The generalisable rule:** read the vendor before writing the code that talks to it, and expect the
answer to be *"the clause is what is wrong"* — which is far cheaper to discover before the code exists
than after it has tests.

### 2.13 The re-run on the committed tree CAUGHT A REAL INTERMITTENT FAILURE — which is the whole reason §1.2 exists

The network-image round's numbers were taken on an unclean tree, then re-run on commit `c765344` as
§1.2 requires. **The re-run failed**: `cargo test --workspace` exited 101 with 1324 tests instead of
1394, because `veloct-sidecar`'s
`tests::an_explicit_override_is_honoured_when_it_points_at_a_file` panicked:

```text
called `Result::unwrap()` on an `Err` value: NotConfigured {
  what: "the bundled Node runtime",
  hint: Some("expected veloct-node-x86_64-pc-windows-msvc.exe or veloct-node.exe beside the application; …") }
```

**The cause was a race that had been latent in the file for rounds.** That module's test harness holds a
`static ENV_LOCK` around every mutation of `VELOCT_NODE`, and its own doc comment explains why: Cargo runs
tests in parallel within one binary and the environment is process-wide. But one test carried a stray

```rust
std::env::remove_var(NODE_ENV);   // OUTSIDE the lock, after `with_env` had already released it
```

and `with_env` removes the variable itself while holding the guard, so the line was redundant as well as
unsafe. The window is small: `with_env` returns, releasing the lock, and only then does the stray removal
run. If another test acquired the lock and called `set_var` in between, the stray line **stripped the
override out from under it**, the search fell through to the filesystem, and the failure above was the
result.

**Why it fired now and not before:** the two tests this round ADDED to `veloct-sidecar`
(`a_runtime_under_the_name_an_installed_bundle_gives_it_is_found`,
`both_runtime_names_are_tried_and_the_triple_suffixed_one_comes_first`) are two more env-touching tests,
which widened the window enough to lose the race. Four full-suite runs had passed before it fired once.

**The fix is structural, not a retry loop:** the stray `remove_var` is gone, so **every** mutation of the
variable goes through `with_env`/`without_env` and therefore through the lock — a comment in both places
says so, and the test that carried the stray line now explains why it must not come back. Verified by
**20 consecutive `-p veloct-sidecar --lib` runs and 3 consecutive `--workspace` runs, all green** (1394
passed each time), which is the strongest evidence a race can be given short of a model checker.

**The lesson worth keeping, and it is not "re-run your tests".** It is that **a number measured on an
uncommitted tree is a weaker claim, and the difference is not academic**: the pre-commit run of the exact
same code passed, and the post-commit run of the same code failed. Had the round recorded 1394 and moved
on, the next agent would have inherited a flaky suite and no idea why. `bf75265` re-ran four cloud-target
commits for the same reason; this round is what happens when the habit pays off.

### 2.14 The SAME class of race, a second time: the OS credential store is process-wide too

Fixing §2.13 was not the end of it. The very next full-suite run failed **differently** — exit 101 with
893 passed, and this time two tests in `veloct-desktop`:

```text
images::tests::a_key_round_trips_through_the_os_store_and_a_source_reports_that_it_exists
  panicked: the OS store holds it                       <- exists() was FALSE straight after a set
attachments::tests::deleting_a_target_forgets_its_stored_credential_and_an_unknown_id_changes_nothing
  panicked: and its stored credential no longer resolves <- exists() was TRUE straight after a remove
```

**Both symptoms are a STALE read, in opposite directions, of one process-wide resource.**
`--test-threads=1` made all 134 pass, which identified the cause immediately. Windows' credential APIs
are not isolated per handle across threads in one process, and `keyring` gives no such guarantee.

The second failing test is **pre-existing** (`D143`, from the §13 round): it had been passing because it
was the ONLY test in that binary hammering the real store. This round added a second one — §13.14's key
round trip — and two tests sharing the store in parallel is one more than the store tolerates.

**The fix is the same shape as §2.13, deliberately:** one lock, in the module that owns the resource,
with every test that touches it taking it as its first line.

```rust
let _store = crate::credentials::store_guard();
```

`credentials::store_guard()` is the ONE place the lock exists, and it documents why, so the next test
added against the real store copies a line instead of re-deriving a race. `handle_for_image_source` also
moved into `credentials.rs` for the same reason: one definition of the handle format.

**Verified by 5 consecutive `--workspace` runs, all 1394 passed, 0 failed, 5 ignored.**

**The generalisable rule, now demonstrated twice in one round:** a test that touches a process-wide
resource — an environment variable, the OS credential store, a fixed port, a shared temp path — needs a
lock shared with every other test that touches it. **`--test-threads=1` passing is the diagnostic**, and
"the suite passed last time" is not evidence that it will pass next time. Both of this round's races were
found by re-running a suite that had already been recorded green.

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
  `-x86_64-pc-windows-msvc.exe` **in the build tree**. See §2.10: an INSTALLED bundle strips that
  suffix, so both names have to be searched at runtime.
- **The installer is built by `scripts/build-installer.ps1`, not by `tauri build` directly.** The Tauri
  CLI needs `cargo` on `PATH` and the isolated `CARGO_HOME`, which is exactly what `scripts/cargo.ps1`
  fixes for Cargo; without them it fails with `failed to run 'cargo metadata' command ... program not
  found`. The CLI is a devDependency (`@tauri-apps/cli`), so the script runs
  `node node_modules/@tauri-apps/cli/tauri.js build --bundles nsis`. The first run also downloads NSIS
  itself and `nsis_tauri_utils.dll` from GitHub, which is why it needs the network once.
- **The pinned Node runtime is fetched, not committed** (`scripts/fetch-node.ps1`, pin in
  `scripts/node-runtime.toml`, 89 MB, hash-verified against Node's own `SHASUMS256.txt`). The
  2.66 MB launcher IS committed. If `src-tauri/binaries/veloct-node-*.exe` is missing, run the
  fetch script.
- **`git show <blob>` mangles binary output** through line-ending conversion and reports a wrong
  size. Use `git cat-file -s` / `git hash-object` to compare a committed binary with disk.
- **The dev server binds `::1`, so a probe must use `localhost`.** `veloct-astro-sidecar.exe dev --port P`
  with NO `--host` leaves Astro listening on **`::1` only** (verified: the listener's owner is a
  `veloct-node` process on `::1`), so `GET http://localhost:P/` returns **200** while
  `GET http://127.0.0.1:P/` returns NOTHING. D64's earlier probe passed `--host 127.0.0.1` explicitly, which
  is why this was never seen. **And a port check must filter `State`**: `Get-NetTCPConnection -LocalPort P`
  without `-State Listen` also reports TIME_WAIT entries, which reads as "the port is still held after
  `taskkill /T`" when nothing is listening — the filtered check (`State -eq 'Listen'`) is the honest one, and
  with it a launcher started by the same shell does release the port.
- **`github.com` RESOLVES TO `127.0.0.1` on this machine** (a hosts-file accelerator / fake-IP DNS), so the
  read/search bridges REFUSE it as a private target and `raw.githubusercontent.com` fails the same way. The
  machine's own proxy — the one `scripts/cargo.ps1` already reads out of the registry, `127.0.0.1:10818` —
  reaches both: `Invoke-WebRequest -Proxy http://127.0.0.1:10818 <url>`, including
  `https://api.github.com/repos/<owner>/<repo>/git/trees/<branch>?recursive=1` for a tree listing. **This is
  how §13.12's Lsky contract was read** (PROCESSING.md §5.4), and it is the way to read a vendor's source when
  the vendor publishes no reference. Costs nothing and needs no credential for a public repository.
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
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 test --workspace     # exit 0, 1395 passed, 0 failed, 7 ignored
powershell -ExecutionPolicy Bypass -File .\scripts\cargo.ps1 clippy --workspace --all-targets   # exit 0, 0 warnings
powershell -ExecutionPolicy Bypass -File .\scripts\build-sidecar.ps1 -Check       # exit 0, the committed launcher matches a fresh build
cmd /c "npm.cmd run test:ui"                                                       # 152 passed
cmd /c "npm.cmd run build:ui"                                                      # tsc clean, exit 0
# in site/:
$env:VELOCT_BUILD_TIME="2026-01-01T12:00:00.000Z"; npm.cmd run build; node scripts/verify-build.mjs   # 52/52
```

The installer is a separate, slower command (it runs a release build and downloads NSIS on the first
run). Run it when `tauri.conf.json`, the sidecar binaries or the bundle inputs change, and then
**install the package and run it** — §2.10 is why a green build is not evidence that a package works:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-installer.ps1
# -> target/release/bundle/nsis/Veloct_0.1.0_x64-setup.exe
# verify what the package actually CONTAINS, without touching the real install:
#   Start-Process <setup.exe> -ArgumentList '/S',"/D=$env:TEMP\veloct-probe" -Wait
#   Get-ChildItem $env:TEMP\veloct-probe      # veloct-desktop.exe, veloct-astro-sidecar.exe, veloct-node.exe
#   & "$env:TEMP\veloct-probe\veloct-astro-sidecar.exe" dev   # must reach the Astro dev server
#   & "$env:TEMP\veloct-probe\uninstall.exe" /S
```

Three integration tests are `#[ignore]`d because they spawn real processes. Run them explicitly
when you touch the areas they cover:

```powershell
.\scripts\cargo.ps1 test -p veloct-release  --test end_to_end_release   -- --ignored   # real Astro build
.\scripts\cargo.ps1 test -p veloct-desktop  --test publish_integration  -- --ignored   # real publish
.\scripts\cargo.ps1 test -p veloct-desktop  --test backup_integration   -- --ignored   # real backup/restore
```

**Three more are `#[ignore]`d because they reach a VENDOR**, and they FAIL rather than skip when their
credential is absent. Run them when you touch those protocols, with your own keys in the environment:

```powershell
# the two image providers (§13.14) — one key is enough if you filter by name
$env:VELOCT_UNSPLASH_ACCESS_KEY = "<access key>"
.\scripts\cargo.ps1 test -p veloct-attach --test live_providers -- --ignored unsplash --nocapture

# the GitHub Contents API through §13.13's attachment target (D170) — a public throwaway repo is fine
$env:VELOCT_GITHUB_ATTACHMENT_TOKEN = "<fine-grained PAT, Contents: read and write>"
$env:VELOCT_GITHUB_ATTACHMENT_REPO  = "owner/throwaway-repo"
.\scripts\cargo.ps1 test -p veloct-attach --test live_github -- --ignored --nocapture

# §17.4's backup target (D172) and §17.2's privacy rule (D171) — the repository MUST be private
$env:VELOCT_GITHUB_BACKUP_TOKEN = "<fine-grained PAT, Contents: read and write>"
$env:VELOCT_GITHUB_BACKUP_REPO  = "owner/throwaway-PRIVATE-repo"
.\scripts\cargo.ps1 test -p veloct-backup --test live_github -- --ignored --nocapture

# S3 / S3-compatible, in BOTH roles (§13.11's attachment target and §17.4's backup target).
# `{bucket}` in the endpoint is how the ADDRESSING is chosen, and Alibaba Cloud OSS REQUIRES it: OSS refuses
# path-style bucket access with 403 SecondLevelDomainForbidden. A MinIO wants the opposite, so leave it out.
# An `-internal` endpoint will not resolve from outside Alibaba's network; use the public one.
$env:VELOCT_S3_ACCESS_KEY_ID     = "<access key id>"
$env:VELOCT_S3_SECRET_ACCESS_KEY = "<secret access key>"
$env:VELOCT_S3_BUCKET            = "your-bucket"
$env:VELOCT_S3_REGION            = "cn-chengdu"
$env:VELOCT_S3_ENDPOINT          = "https://{bucket}.oss-cn-chengdu.aliyuncs.com"
# the attachment test additionally needs the prefix the object's public URL is built from (§13.6)
$env:VELOCT_S3_PUBLIC_URL_PREFIX = "https://your-bucket.oss-cn-chengdu.aliyuncs.com"
.\scripts\cargo.ps1 test -p veloct-attach --test live_s3 -- --ignored --nocapture
.\scripts\cargo.ps1 test -p veloct-backup --test live_s3 -- --ignored --nocapture
```

Two things learned by running them, so nobody re-learns them: **a repository's `permissions.admin` describes
the USER, not the TOKEN** — making a repository private needs the PAT's own **Administration: read and write**,
or `PATCH /repos/…` answers **403** — and a **millisecond** timestamp does not produce an archive name that
`archive_created_at` can read back, so a live backup test must use the shape `remote_name` itself generates.

---

## 4. What IS implemented

All numbers are test counts. The matrix holds **244 rows across tables A-G** (A 23, B 14,
C 12, D 157, E 21, F 6, G 11) plus a separate **20-row H table** which IS the `handoff.md` §28
acceptance checklist, one row per headline requirement. **224 of the A-G rows carry a status and
all 224 are `I`**; the remaining 20 are table headers or rows whose status lives in a linked row.
The sixteen previous new ones were `D130`–`D145`: §13's form, the local kind, the end of the
unconfigured-SFTP degradation, §13.8's explicit cleanup, §13.2.2's reason, the storage-kind
report the window reads before a target exists, and the save path's enforcement of the crate's
own rules. **This round's ten are `D146`–`D155`**: the works service, the `preview/` split, the
site's poster route, the poster command, the command registrations, the form's pure module, §5.10.1's
last divergence, the installer, the sidecar's bundled name, and the live-verified Pixabay contract.

**The H table is the one to read when judging completion.** It is short, it mirrors §28 clause
by clause, and each row names the evidence it requires.

| Crate | What it owns |
|---|---|
| `veloct-core` | Errors (with `hint()`), secrets/redaction, atomic writes, `ProjectRoot`, `FileRevision`, HTTP client trait + fakes, clock, ids |
| `veloct-markdown` | Frontmatter, directives, paragraph ids `<!--p:ULID-->`, references, rewrite, diagnostics, parse |
| `veloct-storage` | SQLite schema + migrations, rebuildable index, `ai_settings` (providers, models, features, fallback), **and the re-exported `Connection` its own API takes** |
| `veloct-content` | Content model, WIP lifecycle, paragraph ids, `create_from_source`, works import/metadata, **and `PREVIEW_PREFIX`: §10.1's `preview/` is a SIBLING of the body, which is what an import now produces** |
| `veloct-git` | Change sets, publish commits, release tags, worktrees |
| `veloct-logging` | `[info]/[warning]/[error]`, mandatory redaction AT CONSTRUCTION, rotation, log page |
| `veloct-process` | Preview supervisor, port reservation, readiness probes |
| `veloct-release` | `release.json` (no commit sha), frozen inputs, deterministic fingerprint, one build → two roots, **`publish()`** = freeze → build → commit → tag |
| `veloct-sidecar` | The Astro launcher: resolves the bundled Node from the sidecar path, NEVER `PATH` (ADR-0005), **under BOTH names a bundle can give it** (§2.10) |
| `veloct-deploy` | Three logical targets × main/works sub-targets, Cloudflare adapter, manual exports, roll-up (`partial` vs `failed`), re-send, **SEO push lifecycle** |
| `veloct-ai` | Providers, per-feature model mapping, CANCELLED-safe fallback, context, apply plans, **the page creator**, **model discovery + a connection test whose failure message is redacted at the source** (§2.8) |
| `veloct-attach` | Three independent state axes, reference sets, dedup, cache pool, upload transaction, **GitHub, S3, the local-directory attachment targets (`StorageKind::Local`, §13.10 where the server is this machine) and §13.12's Lsky Pro target (`LskyStore`, over the contract read from the vendor's own shipped API page and source — D166)**, **§13.8's explicit `delete_remote_object` — the only path that removes a remote object**, the canonical MIME map, **and §13.14's two network image providers (`network::providers`): Unsplash's and Pexels's search contracts, their OPPOSITE auth headers, the credit line each one requires, and Unsplash's mandatory download report — with Pixabay listed and refused in its own words** |
| `veloct-github` | The GitHub Contents API, ONCE: `Repository`, `GitHubContents` (put/get/list/delete/repository_info), `base64_encode`, the 100 MB ceiling. No policy — that stays with the dependent (ADR-0015) |
| `veloct-s3` | S3 / S3-compatible, ONCE: SigV4 (`CanonicalRequest`, `uri_encode`, the signing-key chain), and `S3Client` (streamed put, HEAD, paginated LIST, DELETE, a signed connection test). No policy either (ADR-0016) |
| `veloct-backup` | Selection rules, gzip archive + manifest, secret scan, staging restore, retention, target orchestration, **the GitHub and S3 cloud backup targets — §17.2 verified against the repository, and S3's streamed upload plus the bucket's own timestamps** |
| `veloct-app` | Orchestration services: attachments, backup/restore, deploy, autobackup, **cloud backup targets in SQLite + the run record**, **the works service (import, list, export, metadata, poster, remove) whose truth source is FILES and which therefore takes no connection**, plus `CommandError` |
| `src-tauri` | The Tauri shell: **87 registered commands**, `AppState`, preview wiring, credential store (keyring), sidecar resolution, **the storage-target commands including §13.8's `delete_remote_object` and `storage_kinds`, which reports each kind's adapter status from the crate without touching project state**, **the ten works commands, and `describe_work_source`, which decides ZIP vs folder from what the path IS**, **and §13.14's five network-image commands — none of which takes an `AppState`, so every one of their bodies is reachable from a test** |
| `src/` | The workbench: 3-pane layout, **Tiptap WYSIWYG** editor with a Markdown view, draggable split with a REAL Astro iframe preview, autosave, conflict UI, log page, **AI settings with model discovery, a connection test and a model list**, page creator, attachments **with the storage-target FORM, §13.8's remote cleanup, §13.2.2's eligibility reason and the kind-gap report read before the first save**, **作品: §10's works panel with the import PLAN shown before anything is written, metadata editing, the poster picker and export**, **网络图片: §13.14's browser, which searches Unsplash and Pexels, stores the key in the OS store, shows what each provider requires, and writes the remote URL plus the credit line into the document**, publish, backups (including cloud targets), deploy |

**Verified end-to-end against real processes**, not only unit tests:

- A real desktop binary builds and launches (`target/debug/veloct-desktop.exe`, ~37 MB).
- The sidecar starts the real Astro dev server through the real bundled Node: `GET /` → HTTP 200,
  an article route → 200, and `taskkill /T` frees the port. **Without `--host` the server binds `::1` only**,
  so a `localhost` probe answers and a `127.0.0.1` one does not — see §3, and D64's probe passed
  `--host 127.0.0.1` explicitly, which is why that stayed invisible.
- **A real NSIS package was built, silently INSTALLED, listed, probed and uninstalled** (§2.10):
  `Veloct_0.1.0_x64-setup.exe` is **27.93 MiB** and installs `veloct-desktop.exe`,
  `veloct-astro-sidecar.exe`, `veloct-node.exe` and `uninstall.exe`; the INSTALLED launcher launches the
  real Astro dev server, and `GET http://localhost:<port>/` returns **200** from a `veloct-node` listener.
  This is the one §5.7 claim that a `cargo build` could not have made. **Rebuilt and re-probed twice on
  2026-10-02** (so the package carries §13.12, and then so it no longer offers a kind the contract has
  removed), with the whole sequence re-run rather than assumed each time.
- **A real vendor was WATCHED answering on 2026-10-02, in FIVE directions**: a live `put`/`head`/`delete` round
  trip through §13.13's attachment target against `api.github.com` (D170), §17.2's privacy refusal against a real
  PUBLIC repository (D171), the §17.4 BACKUP target's own `upload`/`list`/`remove` round trip once that
  repository was private (D172), and then **both S3 roles against a real Alibaba Cloud OSS bucket** — the
  §13.11 attachment target (D175) and the §17.4 backup target (D176). All five are `#[ignore]`d live tests that
  fail without a credential rather than skipping, none prints a credential, and together they are the first time
  this repository has WRITTEN to a third party. **And the S3 pair found a shipped defect on its first run**
  (D174): `x-amz-date` carried a real clock's sub-second digits, so no S3 request this application made could
  have succeeded anywhere — see §5.8, which is where that gap was already written down as the biggest one.
- **A real Astro build emits the works poster**: `dist/site/work-previews/demo-counter/screenshot.webp`
  was compared BYTE FOR BYTE with the work's own `preview/screenshot.webp` (52/52 verifier checks).
- `publish` runs a real Astro build, emits `dist/site` + `dist/works`, writes `release.json`,
  commits, tags, and the tag resolves to the commit the publish record names.
- Backup/restore against the repository's own content: 56 files archived; full restore wrote 56;
  **content-only restore wrote 11 and preserved 45**; an unconfirmed restore changed nothing.

### 4.1 The 20-row acceptance table, judged honestly

`docs/spec/02-test-matrix.md`'s H table IS `handoff.md` §28 clause by clause. Read it before
believing any summary, including this one. Judged against the evidence that exists today:

| # | Requirement | Honest verdict |
|---|---|---|
| H1 | Create and manage a project | **done** — `ProjectService`, 82 shell commands |
| H2 | Markdown edit, autosave 2 s / 30 s / save-on-close | **done** — `src/lib/autosave.ts`, 117 frontend tests |
| H3 | SQLite index rebuilds | **done** — `veloct-storage::rebuild` |
| H4 | Real Astro live preview, same project as production | **done** — sidecar smoke test: HTTP 200, article route 200 |
| H5 | WIP control, stable URL, finished redirect | **done** |
| H6 | Knowledge paragraph references + repair | **done** |
| H7 | Page creator | **done** — `veloct-ai::page` + `src/PageCreator.tsx` |
| H8 | AI per-feature provider/model + fallback | **done** |
| H9 | AI confirm / apply-all / apply-partial | **done** (apply lives in `veloct-ai::apply`; the UI exposes confirm) |
| H10 | Work import, screenshot, iframe preview, isolation | **PARTIAL, and this round moved it to the edge: import (ZIP and folder), the poster image and export are ALL reachable from the window**, the import PLAN is shown before anything is written, the stable id and import provenance are protected from a metadata edit, and §10.1's `preview/` is now produced by an import and EMITTED by the build at `/work-previews/<slug>/screenshot.webp` — the route that had no writer before, so the poster wall always rendered 暂无截图. The iframe preview and the isolation are done (`F6`, `G5`, the sandbox checks). **What is STILL PARTIAL and exactly why: the screenshot is a FILE THE USER CHOOSES rather than one Veloct captures.** This build has no headless browser; `PROCESSING.md` §5.9 records that rather than papering over it, and the command's own doc says so. A headless capture is a new dependency decision (a `chromiumoxide`-class crate or a WebView2 capture path), which is not one to take silently. Everything else in §5.9 is closed; the remaining work is that one decision |
| H11 | Publish commit + Release tag | **done** — real publish integration run |
| H12 | One build per publish, all targets share it | **done** |
| H13 | Failed target re-publish by original Release | **done** — `resendable_sub_targets` |
| H14 | 1Panel manual upload of current `dist` | **done** as a manual export |
| H15 | Attachments: cache pool, **sftp, s3, lsky**, github, remote image | **PARTIAL, and §13.14 is now DONE: the remote image browser is implemented against Unsplash and Pexels** — the two sources whose terms require or permit §13.14's flow — with keys in the OS credential store, the credit line enforced by the crate where the provider demands it, Unsplash's mandatory download report, and Pixabay refused in its own words. **Unsplash is verified against the LIVE service (D164)**; **Pexels is implemented and tested but currently unusable, because it has paused new API key issuance — the fact is recorded in `docs/spec/03-upstream-facts.md` §4.2 and matrix row **D165**, and §5.12 records it as a known and deliberate state rather than fixing it.** What is done besides: the pool, the three axes, dedup, multi-reference, §13.8's no-auto-delete rule and explicit cleanup, the local kind, the kind report, the save-path validation, GitHub and S3, and the target form (B1–B14, D16, D91–D93, D116–D123, D130–D145, D155–D164). What is STILL PARTIAL and exactly why: **(1) ~~`sftp` has NO adapter~~ — CLOSED BY REMOVAL (ADR-0017, D169): the OWNER removed §13.10's SSH/SFTP target from the contract, so it is a RETIRED kind rather than an unadapted one, §13.10 is now the local / server directory target, and a project that saved an SFTP target still loads it and is told it was removed; **(2) `lsky` is now DONE** — §5.4's protocol was read from the vendor's own artifacts (the API page the product renders, plus the routes and handlers behind it) and `veloct-attach/src/lsky.rs` implements it, D166–D167; **(3) GitHub and S3 have both contacted the live service** — GitHub in three directions (D170–D172) and S3 in both roles (D175–D176, after a live call found a defect no test could see: D174) — **while Cloudflare and EdgeOne remain fake-client-only** (§5.8), and Pexels cannot be contacted by a new user at all (D165, §5.12) |
| H16 | Dedup safe, multi-reference safe, no auto remote delete | **done** |
| H17 | Auto backup, manual backup, local export, full restore, content-only restore | **PARTIAL — all five work, and BOTH of §17.4's cloud targets are now real: GitHub (§17.2 verified against the repository on every upload) and S3 (§13.11's client, a streamed upload, the bucket's own timestamps). Targets live in SQLite's `backup_targets`, each run is recorded per target in `backups` (§22.7), and §17.3's finding blocks the upload while the local archive survives. What is still missing is NOT the target model — see §5.8: **BOTH of §17.4's cloud targets are now verified LIVE — GitHub in three directions (D170–D172) and S3 in both roles (D175–D176, after the live S3 call found and fixed the `x-amz-date` defect D174) — while Cloudflare and EdgeOne have never been contacted** |
| H18 | No secret in config/log/backup; scan before backup | **done** — scan runs BEFORE the archive is written; and see §2.8, where wiring a feature found a leak on a path review had missed |
| H19 | SEO, comments, logging operational | **done** — SEO push lifecycle, giscus comments (off by default), log page |
| H20 | Key failure paths explicit, never silent | **done** — `hint()`, `anchor_error`, `unimplemented_reason()`, `capabilities().verified` |

**So: 17 of 20 are done, and H10, H15 and H17 are partial.** The three partials are the honest
answer to "is §28 met" today. Nothing above should be read as a claim that Veloct is complete.

**What changed in this judgement, stated plainly, because a reader of the previous version needs it:**
H10 moved from "no command exists for any of it" to "one decision remains, and it is not a coding
one"; **H15's remote-image reason is GONE — §13.14 is implemented — and its third reason is now purely
§5.8's "no live service has ever been contacted"**; and **§5.7's installer, named in this table as one
of the four things keeping §28 unmet, is DONE.**

---

## 5. What is NOT implemented

Ordered by how much of §28 they block. **§5.1, §5.2, §5.5, §5.6 and §5.7 are CLOSED** — they are kept, clearly
marked, because earlier versions of this file called them the biggest gaps and a reader looking for those
sentences must find out what happened to them. The OPEN items are §5.3, §5.4, §5.8, §5.9, §5.10, §5.11 and
**§5.12 — which is a recorded DECISION rather than a gap, and the one section to read before touching
Pexels.**

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

### 5.3 CLOSED BY REMOVAL: the SFTP storage target is GONE from §13.10

Kept here rather than deleted, because this section carried a "dependency decision" for two rounds and a reader
looking for those sentences must find out what happened to them. **The decision was taken by the OWNER, and it
went the opposite way from the one this text expected: the target was REMOVED from the contract** (ADR-0017,
2026-10-02), rather than implemented.

Two facts make that defensible rather than merely permitted:

- **The reasoning this section repeated was inherited, not measured.** "No `ssh` feature exists on purpose,
  because it pulls OpenSSL onto Windows/MSVC" is the root `Cargo.toml`'s comment about **git2**'s own features.
  An SFTP client has its own choices, and they were never weighed here: `ssh2` needs `libssh2-sys` (a C
  library), while **`russh` 0.63 pulls no OpenSSL and no libssh2 at all** — and this repository's own
  `docs/research/tauri2-windows-report.md` had already recommended `russh`/`russh-sftp` as the pure-Rust path.
  So "it cannot be done without OpenSSL" was never the real obstruction.
- **It was the last kind with no adapter and no server to verify against.** Every SFTP claim in this repository
  was a `NOT_CONFIGURED` and a paragraph of prose, and §28's H15 was PARTIAL for it.

**What replaces it:** §13.10 is now the local / server DIRECTORY target — a path, not a protocol; no SSH client,
no host, no port, no credential. A project that already saved an SFTP target **still LOADS** (the enum variant is
retained, load-only, because the target list is parsed as one document and one unknown variant would take a
user's other targets with it), **reports the removal** in the window, **can be deleted**, and **cannot be
re-saved** — `parse` and `parse_storage_kind` both refuse its spellings now. D169 is the recorded evidence.

### 5.4 CLOSED: Lsky Pro storage target (§13.12) — and where its protocol turned out to live

Kept here rather than deleted, because this section used to say the protocol "is not verified" and the
conclusion drawn from that was wrong in an instructive way. **The vendor publishes no API reference** — the
documentation site covers installation, storage drivers and upgrades — but **the contract ships INSIDE the
product**, as the page `Common\ApiController` renders at `{instance}/api`. Read from that page and from the
routes and handlers behind it (`lsky-org/lsky-pro`, free line), the whole contract is now recorded in
`docs/spec/03-upstream-facts.md` §5, `crates/veloct-attach/src/lsky.rs` implements it, and matrix rows
**D166–D167** carry the evidence. What the reading changed is in §1.1 item 13.

**What stays open, stated rather than hidden:**

- **The PAID line is not claimed.** "Lsky Pro+" 2.x and enterprise 1.x are the same architecture by the
  vendor's own documentation, but **from 2.3.0 the vendor ENCRYPTS the backend source** and still publishes no
  reference, so the adapter is verified against the free line and says so in its own module docs.
- **`head` cannot be answered.** The free line's only lookup is `GET /images`, whose documented `keyword`
  filter the vendor applies to `origin_name`/`alias_name` and **never to `key`**. The adapter reports that it
  cannot answer rather than claiming an object is absent, and nothing needs it: §13.4's reuse is decided by
  the recorded object.
- **The vendor's own delete can report success without removing the file.** Its `deleting` hook removes the
  physical object only when no other row on the same strategy shares the `md5`+`sha1`, and it **catches a
  failure and merely logs it** — so "删除成功" is not proof, and the module says so.
- **A second project pointed at the same instance re-uploads.** `put` always reports a CREATE, because the
  instance cannot be asked whether it already holds these bytes (`head` above), and §13.4's reuse is decided by
  the RECORDED object — which is per project. S3 (`HEAD` first) and GitHub (the status) can reuse; Lsky Pro
  cannot. Invisible from either project, so it is recorded rather than discovered.

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
  `capabilities().can_delete_remotely` is false (SFTP reports its own §13.10 gap; Lsky Pro HAS an adapter and
  can delete remotely now, D166), on a
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
  files are NOT deleted. **The removal is now driven, not just written** (D143): the test puts a value
  in the OS store, proves it resolves, deletes the target, and proves it no longer does — and that an
  unknown id forgets nothing.
- **A save is now VALIDATED by the crate's own rules** for EVERY kind, before anything is written (§2.9,
  D141/D142/D169). That is a behaviour change, and it is the reason a configuration mistake can no longer be
  discovered only at the first upload. The one kind that is not validated-into-acceptance is the RETIRED
  §13.10 target, whose arm refuses it outright (ADR-0017).

**What is still not here, stated rather than papered over:** **nothing is adapterless any more.** SFTP was the
last kind in that state and it is GONE (§5.3: removed from the contract at the owner's request, so it is not a
gap waiting for work). Lsky Pro left the list the same day, when its contract was read from the vendor's own
artifacts and `veloct-attach/src/lsky.rs` implemented it (D166–D167). The smaller limits the recent rounds left
are §5.10.
`docs/spec/02-test-matrix.md` rows `D130`–`D145` are the recorded evidence, each naming the test AND
the crate. The storage-kind report itself (`D139`–`D140`) is part of commit `2496604` and its run:

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

### 5.7 CLOSED: the NSIS installer

Kept here rather than deleted, because the old text called this one of the four things keeping §28
unmet and a reader looking for that sentence must find out what happened to it. **It is no longer a
gap.** `tauri build` has run and a package exists:

- **`target/release/bundle/nsis/Veloct_0.1.0_x64-setup.exe`**, built by
  `scripts/build-installer.ps1` (NEW). **Its size is a moving fact, so §1's table is the current number**:
  27.89 MiB when this section was written (a first run), **27.93 MiB on 2026-10-02 after §13.12 was
  implemented and the package was rebuilt**. The 2026-10-02 run re-did the WHOLE sequence — build, silent
  install to a temp directory, list, probe the installed launcher, uninstall — rather than assuming it,
  and `GET http://localhost:<port>/` returned 200 through the installed `veloct-node`.
- The Tauri CLI is a **devDependency** (`@tauri-apps/cli` 2.12.1), so the build is reproducible from a
  checkout instead of depending on a global install. `scripts/build-installer.ps1` exists because a
  bare `tauri build` fails here with `failed to run 'cargo metadata' command ... program not found`:
  the CLI shells out to Cargo, and `cargo` is not on `PATH` — the same environment problem
  `scripts/cargo.ps1` documents for Cargo itself.
- The package was **silently installed into a temporary directory, listed, probed and uninstalled**,
  not merely produced. It contains `veloct-desktop.exe`, `veloct-astro-sidecar.exe` and
  `veloct-node.exe`, and the installed launcher reaches the real Astro dev server.
- **Building it found two defects that would have shipped**, and they are the reason this section is
  written as more than "the build succeeded" — see §2.10. The first package was 5.49 MiB and carried
  no Node runtime at all, because `externalBin` named only the Astro sidecar; and an installed bundle
  STRIPS the target triple from a sidecar's name, which `resolve_node` did not know.

What is still NOT here: the package is **unsigned**, so Windows SmartScreen will warn a user who runs
it, and `bundleVCRuntime` is `false` (a machine without the MSVC runtime relies on the WebView2
installer path). Neither is a correctness problem and neither is claimed away.

### 5.8 Smaller, stated gaps

- **Two services have been contacted, in FIVE directions, and the second one found a shipped defect.** On
  2026-10-02 GitHub was verified LIVE three ways — a `put`/`head`/`delete` round trip through §13.13's
  attachment target (D170), **§17.2's privacy rule REFUSING a backup against a real PUBLIC repository** in the
  adapter's own words (D171), and the §17.4 backup target's own `upload`/`list`/`remove` round trip once that
  repository was private (D172) — and then **S3 was verified in BOTH of its roles against a real Alibaba Cloud
  OSS bucket**: the §13.11 attachment target (D175) and the §17.4 backup target (D176), whose streamed archive
  and bucket-reported `LastModified` are the two things that make it a different path from GitHub's.
  **The S3 pair paid for itself on its first run:** it was answered `403 AccessDenied: OSS authentication
  requires a valid Date` because `x-amz-date` carried a real clock's sub-second digits
  (`20261002T034316.5888216Z`), which means **no S3 request this application had ever built could succeed
  against any provider, AWS included** (D174, ADR-0018). **That is the sentence to remember about this
  section:** the live gap was not a matter of confidence — it was hiding a total failure of the feature, and the
  entire suite was blind to it because every test fed a fixed clock with zero nanoseconds and an already-correct
  timestamp. **What is still fake-client-only, each for a stated reason:** Cloudflare and EdgeOne need accounts;
  nothing has been sent to either. Three smaller lessons are recorded rather than remembered: a repository's
  `permissions.admin` describes the USER and not the TOKEN (a fine-grained PAT needs its own Administration
  permission, or `PATCH /repos/…` answers 403); a millisecond timestamp does not produce a name
  `archive_created_at` can read back; and an object store may refuse path-style bucket access outright, so the
  ENDPOINT says where the bucket goes (`{bucket}`).
  **Unsplash is the exception and the first live vendor call this repository has made**: on 2026-10-01 a
  real `GET https://api.unsplash.com/search/photos?query=coffee` through the crate's own request builder
  and parser returned **10 results**, with the `ixid` parameter intact, the 1080 px `regular` rendition,
  the photographer named with a profile link, an attribution line carrying
  `utm_source=veloct&utm_medium=referral`, **and the required download report accepted by the live
  endpoint** (`download report answered: true`). The evidence is matrix row **D164** and
  `crates/veloct-attach/tests/live_providers.rs`, which any holder of a key can re-run.
  **Pexels has NOT been contacted, and cannot be by a new user**: it has paused new API key issuance, which
  §5.12 records in full along with the decision to leave the source as it is. Its contract was read from
  its documentation and its protocol is asserted against `FakeHttpClient`, and
  `crates/veloct-attach/tests/live_providers.rs` has a Pexels test ready to run for anyone who already
  holds a key.
  **ADR-0016's SigV4 tests remain AWS's published vectors rather than a live call**, which is the
  strongest claim this environment can support and is labelled as such.
- **One DeployPanel distinction is incomplete:** a target's `kind` is filled from configuration
  after a run, and the crate's own `needs_attention` is recomputed in the view rather than read
  from `LogicalTargetRun`.
- `resend_backup_to_cloud` re-derives the archive's SHA-256 by reading the file, because
  `BackupManifest` does not carry it (it is the digest of the FINISHED archive, which exists only after
  the manifest is written). The value is identical for unchanged bytes — `write_archive` computes it
  the same way — so a re-send names the same remote object rather than creating a second one, but an
  `archive_sha256` field in the sidecar record would be cheaper than a full second read.

### 5.9 CLOSED except for one stated limit: work import, the poster and export are reachable from the window

Kept here rather than deleted, because the old text said `src-tauri/src/*.rs` contained **zero**
references to `import_work`/`work_import`, to a screenshot, and to `network_image`/`image_search`, and
that the poster route had no writer. **Three of those four are now closed**, and re-verified the same
way, by search:

| What the old text said | State now |
|---|---|
| `import_work` has no caller above `veloct-content` | **called** — `veloct_app::works::import`, reached from the `import_work` command, from `src/WorksPanel.tsx` |
| `list_works` / `export_work` had no caller | **called** — `list_works`, `export_work` commands; the panel lists, edits metadata, exports and removes |
| the poster route `/work-previews/<slug>/screenshot.webp` is emitted by nothing | **emitted** — `split-publish-roots.mjs`'s `copyWorkPreviews` writes it to `dist/site/`, the verifier compares it BYTE FOR BYTE, and the works index references it |
| §10.1's `preview/` could not be produced by an import | **fixed** — `PREVIEW_PREFIX` lifts a `preview/` entry out of the body, so an import now produces the layout §10.1 names |
| `veloct-content::works`' import had no plan step in the window | **planned first** — `plan_work_import` writes nothing, and the panel shows the entry file, the file count, the size warning, the skipped directories and the unusable images before the import is offered |

**What is STILL a limit, and it is a decision rather than an omission: the screenshot is a file the
user CHOOSES, because this build has no headless browser.** §10.3 wants a poster; Veloct stores the
image the user picked at `preview/screenshot.webp`, converts nothing, and says so in the command's own
documentation and in the panel's own text. A capture would need a new dependency (a headless-Chromium
class crate, or a WebView2 capture path) — a decision worth taking deliberately, not by adding a crate
to make a checkbox green.

The remote-image half of the old §5.9 is **CLOSED** — see §5.11. `veloct-attach::network`'s shape-only
model now has a caller, a searcher, a key store and a panel.

### 5.10 Limits that REMAIN after this round (three items CLOSED here, a FOURTH closed later by signing)

This list is corrected rather than extended: **three of the four limits it carried are CLOSED**, and a
status document that keeps listing a closed limit is as wrong as one that claims a gap is filled
(`docs/spec/02-test-matrix.md` §I is about exactly that). **A later round (2026-10-02) closed the fourth as
well — the unsigned installer — by signing the app and the package with a locally created certificate, and item
4 records precisely what that does and does not buy.** What is still LIVE below is item 1 (a duplication that
exists by nature, guarded by transcriptions rather than removed) and item 2 (evaluated and deliberately NOT
closed, with its cost named). What came out, and what closed it:

- ~~`StorageConfig::validate` has no production caller~~ — **CLOSED** by `f29dcda`: `save_target_config`
  calls it for an implemented kind before anything is written (D141), with unimplemented kinds
  deliberately exempt (D142). The rule is enforced where a user meets it; §2.9 records the behaviour
  change that follows.
- ~~One of §13.8's three refusals has no test~~ — **CLOSED**: the no-recorded-object refusal is driven
  by `attachments::a_remote_cleanup_of_an_attachment_with_no_recorded_object_removes_nothing` (D144),
  which keeps a real object on the same target and asserts its bytes survive.
- ~~`delete_storage_target`'s credential removal is untested~~ — **CLOSED** by
  `attachments::deleting_a_target_forgets_its_stored_credential_and_an_unknown_id_changes_nothing`
  (D143), which proves the value exists in the OS store first.

What REMAINS, each still a **stated limit** in the matrix row that covers it:

1. **The configuration rules live in two languages, and only one of them is authoritative.** The crate
   (`StorageConfig::validate`) is the authority; the window (`storageTargetForm.ts::validateDraft`) has
   its own copy because the frontend cannot call Rust. Two members of that duplication were closed in
   these rounds — the public URL prefix, which the window now refuses in the field (`7813fcd`, D145), and
   **the `pathPrefix` over-refusal this list recorded LAST round, now gated by kind (D152)**: the leading
   `/` and `..` checks ran for every kind while `buildSaveRequest` submits that field only for
   `s3`/`github`/`local`, so a leftover value blocked a save for a kind whose form does not render the
   field — the user could not clear it without switching back. **A THIRD member was closed by the §13.12
   round**: the Lsky Pro rule (an instance address, and it must be an absolute http(s) URL) existed in the
   WINDOW and in the adapter's `api_base` but NOT in `validate`, which made the window stricter than the
   authority it is supposed to transcribe. It is a `validate` rule now (`lsky::api_base` still refuses it too,
   for a target built without the save path — D168), so the window's copy is a transcription rather than an
   invention. What still guards the pair is a
   TRANSCRIPTION: nine shapes in `CRATE_RULE` whose verdicts are copied from the crate and compared
   against the window's, so a one-sided change is a failing test rather than an oversight. **The one known
   divergence this list used to record is GONE**: the crate's SFTP arm demanded a host and a username while the
   form asked SFTP for no field, because a kind with no adapter was exempt from the save-path validation
   (D142). The kind was removed (ADR-0017) and the exemption with it, so **every kind's rule is mirrored on
   both sides now** and there is no deliberate asymmetry left to document. **A transcription is not a shared
   implementation** — adding a rule to
   one side must add it to `CRATE_RULE` too, and `tests/workForm.test.ts` is now the SECOND transcription
   of the same kind (`WorkMetadata::validate`'s ten shapes), which is the pattern to copy rather than to
   start a third variant of. **A FOURTH member was closed by the live-S3 round**: ADR-0018's `{bucket}`
   rule is ONE function in the client (`veroct_s3::bucket_placeholder_problem`), asked by the client while
   it builds a request AND by `StorageConfig::validate` on the save path, so the window's copy in
   `validateDraft` transcribes an authority that actually exists — and its nine endpoint shapes are
   compared against that authority in `tests/storageTargetForm.test.ts` (D177). **Note the shape rather
   than the count:** the prefix rules share one `CRATE_RULE` table, while a rule about a DIFFERENT field
   gets its own, because the two fields have nothing to do with each other — and the request path is
   deliberately STRICTER than the helper (it refuses any placeholder left after the usable one was
   stripped), which is what catches the second copy in `https://{bucket}.{bucket}.example.com`. The first
   version of that change got it wrong, and an existing test caught it.
2. **A command WRAPPER that needs an `AppState` is still not driven by a test, and the credential half
   of the save path's ORDER has no test that would catch a reorder.** `f29dcda` extracted the save and
   delete sequences (`save_target_config`, `delete_target_config`) precisely so the behaviour is
   reachable from a test, and both are now driven; `storage_kinds` takes no state and its tests call it
   directly. What remains are the four commands whose bodies need a live `AppState`:
   `save_storage_target`, `delete_storage_target`, `upload_attachment` and `delete_remote_object` are one
   or two lines each, and they are asserted by registration (`include_str!("lib.rs")`) and by the helper
   they call, not by a call. **The ORDER half is now CLOSED**: `f29dcda`'s hole was that every refused-save
   test passed `None` as the credential, so code that stored the credential BEFORE validating would have
   passed every test. `attachments::a_refused_configuration_stores_neither_the_target_nor_its_credential`
   (D168) passes a credential WITH a configuration the crate refuses and asserts that the target-list file
   was never created and that the credential is absent from the OS store — with a positive control, so the
   assertion cannot pass vacuously. **A WebView or `AppState` test would close the wrapper half; this
   repository has neither by choice.** **The works round ADDED ten more of these**
   (`src-tauri/src/works.rs`): four need an `AppState`, and six do not and are driven
   (`describe_work_source`, `work_screenshot_limit`, and the import-source decisions behind them). That is the
   same shape and the same stated limit, not a new one.
   **Two §13.12 rounds moved the wrapper half in the right direction rather than restating it**: the
   request→configuration MAPPING was extracted out of `save_storage_target` into `config_from_request`, and the
   shared TARGET LOOKUP out of `upload_attachment`/`delete_remote_object` into `resolve_stored_target` — both
   take no `AppState`, and both are driven by tests
   (`attachments::a_blank_field_never_becomes_configuration_and_the_instance_survives_the_mapping` and
   `attachments::a_stored_target_is_resolved_by_id_and_its_credential_is_read_by_handle`, D167–D168). What is
   left in the four commands is glue: `State` → project root → the helper → a log line. That residual is why
   this item stays, but it is now small and named rather than broad. **Evaluated again on 2026-10-02 and
   deliberately NOT closed, with the cost named:** `AppState` does have a `Default`
   (`src-tauri/src/state.rs`), so the missing piece is not the state — it is that all four commands take
   `tauri::State<'_, AppState>`, which a unit test cannot construct. Driving them needs Tauri's `test` feature
   (`mock_app`/`mock_context`), a dependency feature enabled for tests, and what it would then exercise is
   `State` unwrapping and a log line, because every DECISION in those bodies was already extracted into a helper
   that IS driven. **A green test of plumbing is not worth a feature flag**, so the limit stands as written
   rather than being closed for the appearance of coverage — and this paragraph exists so the next reader does
   not spend a round rediscovering that.
3. ~~§13.8's cleanup can leave an article pointing at a URL that no longer resolves~~ — **DECIDED, and the
   wording now says so.** §13.8 removes the remote object and deliberately does NOT rewrite the article, so the
   decision is to keep that behaviour and make the CONSEQUENCE explicit where the user meets it: the
   confirmation now says "文章里仍然指向这个地址,远程对象删掉后那里会变成断链——Veloct 不会替你改写文章
   (§13.8 不做自动重写)" (`REMOTE_CLEANUP_WORDING`, asserted by the §13.8 test in
   `tests/storageTargetForm.test.ts`). **What is deliberately NOT done:** no refusal when the attachment is
   still referenced, and no automatic Markdown rewrite — a user who asks for a cleanup is asking for it, and
   §13.8's split between the remote object and the user's record stands. If this ever bites in practice, the
   available middle step is to REPORT the reference count before the cleanup rather than to block it.
4. ~~**The installer is unsigned**~~ — **CLOSED by signing, on 2026-10-02, and the honest half of that is what
   a self-signed certificate does NOT do.** `scripts/sign-artifacts.ps1` creates a local code-signing
   certificate, signs the app the user launches and then the setup `.exe`, and verifies both as **`Valid`**
   (subject `CN=VellowChire Code Signing, O=Veloct`, SHA-256, timestamped; thumbprint and expiry are §1's to
   state). **What it does:** proves the bytes were signed by the holder of that key, and gives Windows a
   publisher name to show once the certificate is trusted on the machine. **What it does NOT do:** remove
   SmartScreen's warning on a machine that has not installed `src-tauri/signing/veloct-codesign.cer` into its
   Trusted Publishers, and it is **not** a substitute for a CA-issued certificate — a self-signed signature is
   exactly as trustworthy as the route that `.cer` took. **Stated rather than implied: on a fresh machine this
   is still an unknown publisher, and the fix for THAT is a purchased certificate, not code.**
   **Three binaries are deliberately left unsigned, each for its own reason:** `veloct-node.exe` is a
   third-party runtime this project redistributes — and **it already carries its own vendor signature**
   (`Valid`, `CN=OpenJS Foundation`), so signing it would be a claim about someone else's binary;
   `src-tauri/binaries/veloct-astro-sidecar-<triple>.exe` is the COMMITTED launcher, and §2.2's `-Check`
   compares it **byte for byte** against a fresh `cargo build -p veloct-sidecar`, so signing it would break that
   check permanently — a subtle interaction worth knowing before someone "completes" the signing; and
   `bundleVCRuntime` stays `false`, which is a separate packaging fact §3's `::1` probe note and
   `docs/research/tauri2-windows-report.md`'s `dumpbin` suggestion cover.
   **AND THE PART AN EXPERIMENT CORRECTED, because the obvious assumption is wrong:** signing
   `target/release/veloct-desktop.exe` and then rebuilding the installer leaves the INSTALLED app
   **`NotSigned`**. Measured twice: the file verifies `Valid` before the rebuild and `NotSigned` after it. The
   likely cause is that `target/release/<name>.exe` is a hard link to `target/release/deps/<name>-<hash>.exe`,
   so writing the signature breaks the link and the next build copies the unsigned file back. **So the package
   a user installs currently has a SIGNED setup and an UNSIGNED app**, and the supported way to change that is
   Tauri's own `bundle.windows.certificateThumbprint` (+ `digestAlgorithm`, `timestampUrl`) in
   `src-tauri/tauri.conf.json`, which signs during the build — deliberately NOT configured here, because a
   thumbprint is machine-specific and would make the committed manifest unusable anywhere else.

### 5.11 DECIDED and IMPLEMENTED: §13.14 uses the two sources whose terms describe it, and refuses the third

This section began as a gap found by *verifying* rather than by guessing, written as a decision because
the two things in conflict were both authoritative. **The decision has been taken, and the option chosen
is the one that keeps the locked clause rather than the one that keeps the sample source.**

**What was verified, live, on 2026-10-01** (full contracts in `docs/spec/03-upstream-facts.md` §4):

- **Unsplash REQUIRES the flow §13.14 prescribes.** Its documentation says "we **require** the image
  URLs returned by the API to be directly used or embedded in your applications", and its API Guidelines
  add that this "applies to all uses of the image and not just search results". Two obligations come with
  it: the credit (Unsplash, the photographer, a link to their profile, `utm` parameters) and a **download
  report** whenever "a user chooses the image to include in a blog post".
- **Pexels permits it.** No download step is documented; the obligations are a prominent link to Pexels
  and crediting the photographer with a link to the photo page.
- **Pixabay FORBIDS it**, in its own words:

  > Returned image URLs may be used for temporarily displaying search results. However, permanent
  > hotlinking of images (using Pixabay URLs in your app) is not allowed. If you intend to use the
  > images, please download them to your server first.

**The decision, and why this option.** Of the three options this section used to list, **option 2 was
taken**: keep §13.14 as written and use the sources whose terms permit or require it. The reasoning is
that the deciding question is not "which provider is easiest" but **"whose terms does the locked clause
actually describe"** — and §13.14 describes Unsplash's requirement and Pexels's permission almost
sentence for sentence. Pixabay's parenthetical in §13.1 was an example, not the specification; a sample
named in a parenthetical does not override a locked flow clause.

Options 1 and 3 were NOT taken, and the reason is worth recording because both remain coherent if the
requirements change:

- **Option 1 (route Pixabay through the §13.4 cache pool)** would satisfy Pixabay's terms, but it changes
  what §13.14's flow IS for that source — the image would enter the pool, be downloaded and be uploaded —
  which is the opposite of the clause. It is the right answer only if Pixabay support is required.
- **Option 3 (let the user decide what to write)** is the least code but leaves the compliance step to
  the user, and Unsplash's guidelines put it on the APPLICATION.

**What was built, and the parts that must not be un-built:**

1. `crates/veloct-attach/src/network/providers.rs` — both providers' request and response contracts, read
   from their own documents, with the **opposite auth headers** asserted (`Client-ID <key>` vs a bare
   `<key>`) because copying one to the other produces a 401 that reads like an invalid key.
2. **The obligations live in the crate, not in a caller.**
   `NetworkImageChoice::from_result` forces Unsplash's credit line even when asked not to include one, and
   `report_download` makes the required call. A caller cannot get this wrong, which is the point — and a
   future caller must not "simplify" it by moving the check up a layer.
3. **A failed download report is stated, not silent, and does not lose the image** (§2.12).
4. `ImageSource::Pixabay` remains a variant, in `ImageSource::ALL`, with
   `providers::unimplemented_reason` returning the quote above. **It is refused rather than deleted**, so
   the window can explain the absence — the same shape as `StorageKind`'s SFTP reason. A future
   implementer must not "just wire it up": that would publish pages violating the provider's terms.

**What is NOT in doubt, and what remains:** nothing about the searcher needed a guessed protocol, which is
what made the decision cheap once the vendors had been read. **The live gap is now HALF CLOSED** —
Unsplash was verified against the real service on 2026-10-01 (D164, §1.1 item 12, §5.8), which is the
first live vendor call this repository has made. **Pexels has not been contacted, and as of 2026-10-01 it
CANNOT be by a new user** — see §5.12.

### 5.12 RECORDED — and since 2026-10-02 also WRITTEN DOWN (Option A taken): Pexels has paused new API key issuance, and the source is still offered

**Read this before "fixing" anything about Pexels.** The §13.14 round implemented TWO sources, chose them
because their terms describe the clause (§5.11), and verified Unsplash live. **Pexels's implementation is
complete and is covered by real tests, counted by name rather than estimated: 11 `#[test]` functions in
`providers.rs` exercise it directly, 11 in `network.rs` use it as the generic result fixture, and 8 in
`src-tauri/src/images.rs` go through the command layer** — plus `tests/live_providers.rs`, whose Pexels
case is ready to run for anyone who already holds a key. Its contract was read from its own API reference.

**The problem, verified on 2026-10-01 from Pexels's own API page and RE-VERIFIED on 2026-10-02 — the sentence appears twice there:**

> **New API key issuance is currently paused.**

**The re-verification found something the first reading did not, and it is the reason this is written into
`docs/spec/03-upstream-facts.md` rather than left here:** Pexels's two pages **contradict each other**. The
request page (`https://www.pexels.com/api/`) carries the sentence above twice; the **documentation** page
(`https://www.pexels.com/api/documentation/`) still says, in its Authorization section, "Anyone with a
Pexels account can request an API key, **which you will receive instantly**". §5.11 sent the next reader to
that documentation page for the provider's contract, so a reader who checks the fact there reaches the
opposite conclusion. Matrix row **D165** records this, which is Option A of the table below.

So: the protocol works and its tests pass, the endpoints still answer (an unauthenticated
`GET https://api.pexels.com/v1/search` returns **401**, so the service is up), existing keys still work —
but **a new user cannot obtain a key**, and the window offers Pexels as though they could.

**The user's decision was to LEAVE IT ALONE for now** — their words were
「先留着假接口不管他」, i.e. "keep the not-really-working integration for now, don't bother with it". That is
a deliberate choice, not an oversight, and it is a decision about the BEHAVIOUR: recording the fact
(Option A, below) does not change how the source behaves, and is all that has been done since. So:

- **Do not delete the Pexels implementation.** Its protocol, its `ProviderFacts`, its parser and the 30
  tests counted above are the reason re-enabling it later is one predicate rather than a rewrite. The
  repository's own precedent is `StorageKind`'s SFTP arm: kept, and reported, rather than softened or
  removed (`PROCESSING.md` §2.9, and `StorageKind::unimplemented_reason`).
- **Do not "fix" it by inventing a key, a proxy or a fallback source.** §20 forbids it, and the pause is not
  something code can work around.
- **The known rough edge, stated rather than hidden:** `ImageSource::Pexels` has `is_implemented() == true`
  and is therefore rendered as a normal, selectable source in `src/NetworkImagePanel.tsx`. A user who
  clicks it is asked for a key they cannot obtain, and **nothing in the UI says why**. The REPOSITORY now
  says why — `docs/spec/03-upstream-facts.md` §4.2 carries the fact and its two-page contradiction, matrix
  row **D165** records the verification, and this section carries the decision. **Before 2026-10-02 this
  section was the only record; that is the one thing Option A changed.**

**Two ways to close it; the first was TAKEN on 2026-10-02, and the second is still available:**

| Option | Change | Effect |
|---|---|---|
| **A — record the fact only — TAKEN (2026-10-02)** | The pause, and the contradiction between Pexels's two pages, are now in `docs/spec/03-upstream-facts.md` §4.2; matrix row **D165** records the live verification; this section keeps the decision | No behaviour or UI change, and the fact is no longer findable ONLY in this file. **It is not a fix**: Pexels is still offered, and the UI still does not say why it cannot be used |
| **B — split "implemented" from "offered" — NOT taken** | `is_implemented` (a protocol exists) vs `is_offered` (a user can get a key), a `not_offered_reason`, `search_with` refuses a not-offered source, and `sourceOptions` filters on it | The panel stops listing Pexels and shows the provider's own words instead; a key holder can still search, and the live test still runs; when the pause lifts it is one predicate |

**Option B was written and then reverted on the user's instruction**, so the reasoning survives here even
though the code does not. It is the option that keeps `is_implemented` honest — today that single
predicate conflates "the protocol exists" with "a user can use it", which is exactly the confusion that
makes this situation hard to explain. Nothing else in the repository depends on the distinction.

**A later session re-proposed Option B as free work and then SKIPPED it after reading this section (2026-10-02),
and that is recorded here so it is not re-proposed a third time.** The reasoning at the time was "the panel offers
a source whose key cannot be obtained and says nothing about it", which sounds like a small fix in isolation and
is in fact a re-run of a change the owner already reverted. **The standing decision is the owner's words:
「先留着假接口不管他」** — leave it, do not tidy it. The fact is recorded (Option A), the behaviour is
deliberately unchanged, and a reader who wants Option B must hear the owner ask for it in those terms.

**What is NOT affected:** Unsplash is fully usable and live-verified. §13.14's clause, its flow, its
attribution enforcement and its download report are all correct and tested; only the SECOND source's
availability is in question.

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

6. **Grep for a CALLER before believing a rule, not only before believing a field.** This is how §5.10's
   first limit was found — and how it was later CLOSED, which is the part worth keeping: a later round
   gave `StorageConfig::validate` a caller on the save path (§2.9), and the same habit showed a
   before/after difference rather than a doubt. The lesson from finding it first: a rule whose only
   reader is a test is documentation with a green tick beside it. **And when a row cannot prove
   something, write it in the row.** The sixteen rows this round added say "**Stated limit**" where a
   refusal, a command body or a credential removal is asserted by inspection or at another layer;
   `docs/spec/02-test-matrix.md` §I makes that the rule, and a row that quietly implied more would be
   the drift the whole file exists to prevent.

---

## 7. Suggested order for the next agent

**Step 0, before any of the below:** run §3.1 and make the results match §1 — 1395 Rust, 152 frontend,
52/52 site checks, 0 clippy warnings, exit 0 everywhere. **They were run on the clean committed tree
`585de01` and reproduced, and the tree is byte-identical to it**, so a mismatch means something changed
outside this repository — find that out before changing any code. §1.2 tells the whole story, including
why running these checks rather than assuming them is not a formality: doing so is what exposed two real
test races (§2.13, §2.14) that had been passing by luck.

1. **Read `handoff.md` §28 again** and diff it against §4 above and the H table in the matrix.
   `handoff.md` wins over this file, and over any summary of it, including this one.
2. **Pexels is the one thing to READ BEFORE TOUCHING — §5.12.** Its protocol is implemented and tested,
   its source is still shown in the window, and **no new user can obtain a key** because Pexels has paused
   issuance. That is a deliberate, recorded decision ("先留着假接口不管他"), not an oversight, so **do not
   delete the implementation and do not invent a workaround**. §5.12 has the two small options for closing
   it; **Option A was taken on 2026-10-02** (the fact is in `03-upstream-facts.md` §4.2 and matrix row
   D165), and **Option B is the one still available if the user ever asks for it**. The only thing missing
   right now is that nothing in the UI says why the source cannot be used.
3. **The live smoke test is no longer the same gap it was** (§5.8) — it has been narrowed THREE times, and
   each narrowing was cheap. **Unsplash** (D164): a real search, a real attribution and a real download
   report, all through the crate's own code. **GitHub, in both directions on 2026-10-02**: a real
   `put`/`head`/`delete` round trip through §13.13's attachment target (D170), and **§17.2's privacy refusal
   against a real PUBLIC repository** (D171) — the second one is a rule being verified by watching it say NO,
   which is worth copying. **What is left:** GitHub's own BACKUP round trip needs a PRIVATE repository (the
   token supplied has Contents write but not Administration, so Veloct could not make one private and did not
   try — the honest next step is a repository the user makes private, or a token with Administration)** — and
   that step was then taken, so **GitHub is verified in all THREE directions (D170–D172)**; **S3 followed in
   BOTH of its roles (D175–D176), and its first live call found a shipped defect that made every S3 request fail
   (D174)**. **What is genuinely left:** Cloudflare and EdgeOne need accounts. Pexels has a test ready but no
   obtainable key (§5.12). **The lesson worth carrying forward is not "live tests are nice":** the S3 defect was
   invisible to 1400+ tests because every one of them fed a fixed clock, and only a real service could refuse
   the value Veloct built.
   **A live test that skips silently is worth nothing, which is why a missing key FAILS
   `tests/live_providers.rs` and both `live_github.rs` files.**
4. **The §13.12 round is DONE and the §13.10 SSH/SFTP target is GONE** — Lsky Pro is implemented against a
   contract read from the vendor's own artifacts (§5.4, D166–D167), and SFTP was REMOVED from the contract at
   the owner's request (§5.3, ADR-0017, D169). **So there is no "remaining adapter" work in §13 at all**: the
   four kinds this build offers (local directory, S3, GitHub, Lsky Pro) each have an adapter and a reason to
   exist, and a kind that cannot be implemented is now handled the way Pixabay is (offered, with the vendor's
   own refusal) rather than left as an unimplemented row. **Two worked examples are worth copying from these
   rounds:** when a vendor publishes no reference, the contract may be INSIDE the product (§5.4); and when a
   recorded reason blocks a decision, read what the reason actually says — this one came from a different
   crate's feature list and had never been measured (§5.3).
5. **§5.9's one remaining limit is a dependency decision, not a coding one:** §10.3 wants a poster and
   this build has no headless browser, so the poster is the file the user chooses. If a capture is wanted,
   decide the mechanism (a headless-Chromium-class crate, or a WebView2 capture path) as a deliberate
   choice with its build cost measured — do not add a crate to turn a checkbox green.
6. **The installer is built but NOT in CI**, and it is the only artefact a user runs. Re-run
   `scripts/build-installer.ps1` after any change to `tauri.conf.json`, the sidecar binaries, the bundle
   inputs or `crates/veloct-sidecar`, and then INSTALL it — §2.10 is why a green build is not evidence
   that a package works. **Any touch to `crates/veloct-sidecar` invalidates the committed launcher, even a
   test-only edit** (§2.2). **Signing is now a STEP rather than a gap:** `scripts/sign-artifacts.ps1` creates a
   local self-signed certificate and signs the setup `.exe`, which verifies `Valid` — and §5.10.4 with D178 state
   exactly what that does and does not buy, including the measured fact that the app INSIDE the package still
   comes out unsigned and that Tauri's `bundle.windows.certificateThumbprint` is the supported fix for it.
7. **§5.10 lists the limits that REMAIN, and there are TWO live ones** — the two-language ruleset (now with
   THREE transcriptions beside
   `storageTargetForm.ts::CRATE_RULE`: `tests/workForm.test.ts`'s ten shapes for `WorkMetadata::validate`
   and `tests/networkImageForm.test.ts`'s rules), the untested command WRAPPERS (four storage ones and
   **none of the works or image ones** — the ten works commands need an `AppState` only for four of them,
   and all five image commands take none, so their bodies ARE driven), §13.8's cleanup leaving a cited URL
   dead, and the unsigned installer. **The image round is the model to copy:** extract the sequence, take
   no `State`, and the body becomes testable.
8. **When §5 changes, the matrix changes with it.** Every row at `I` must name a test AND the command you
   ran; a row that cannot is a row that is wrong. `docs/spec/02-test-matrix.md` §J is where the recorded
   runs go, and this file's §1 table is where the totals go.
9. **If the tree changes after you record a run, say so in §J rather than replacing the row.** The last
   four rounds each did, and §1.2 is where the whole story is: the 1317 snapshot stayed when the tree moved
   under it, `2496604` stayed when two repairs landed on top, `7813fcd` stayed when the works round began,
   and the network-image numbers were first recorded on an UNCLEAN tree and then — when genuinely re-run on
   their commit — **failed twice, found two real races (§2.13, §2.14), and only then reproduced.** A run
   whose totals no longer match is a fact about the repository, not a mistake to tidy away.

10. **Before you write anything down, run §3.1.** The six checks are green on `585de01` and the tree is
    byte-identical to it. If they are not green for you, stop and find out why before changing code — the
    last time they were assumed rather than run, two races had been hiding in the suite for rounds.

**Four habits worth keeping, all learned the hard way in these rounds:**

- **Prefer a delegation over a second implementation.** `veloct-github` and `veloct-s3` exist because two
  contracts needed one protocol; `providers::search_request` reuses `veloct_s3::uri_encode` for query
  encoding because that IS RFC 3986; and the credit rule lives once, in the crate, where a caller cannot
  get it wrong. When you find yourself writing a map, a rule or a protocol twice, that is the signal.
- **Redact where the secret is known, not where it is displayed** (§2.8). The §18.1 guarantee held at the
  log sink and failed at the source, and only a test that drove the path end to end found it. **Both image
  provider keys are opaque strings that match no credential pattern**, so the exact-value layer is the only
  one that can catch a provider echoing one back.
- **Verify the vendor BEFORE writing the code that talks to it** (§2.12, §5.11). Reading three providers'
  documents took minutes and changed the DESIGN — the answer was "the sources were wrong, not the clause".
  The identical habit had already found §5.3's and §5.4's gaps.
- **"It builds" and "it works" are different claims** (§2.10). A green `cargo build` shipped an installer
  with no Node runtime and a launcher that could not find it. Install the artefact and run it.
