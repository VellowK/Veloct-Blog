# Veloct — Test traceability matrix

> `handoff.md` §23, §24, §25 and §28 define behaviours that **must** be covered by automated
> tests. This file is the authoritative map from contract clause → concrete test, and it is
> the checklist used to decide whether the project is "complete". A clause with no test is an
> open work item, not a finished feature.

Legend: **P** = planned, **I** = implemented, **F** = failing (regression).

## A. Release / build consistency — handoff §23

| # | Clause | Test id | Crate / path | State |
|---|---|---|---|---|
| A1 | Release tag unique and always points at its publish commit | `release::tag_points_at_commit` | veloct-release | P |
| A2 | All sub-targets of one Release receive an identical build fingerprint | `release::fingerprint_shared_across_subtargets` | veloct-release | P |
| A3 | Re-publishing a failed sub-target never mixes in later working-tree changes | `release::resend_uses_original_commit` | veloct-release | P |
| A4 | Rebuilding a historical Release never reads current SQLite config | `release::rebuild_ignores_current_db` | veloct-release | P |
| A5 | Two rebuilds of one Release produce the same fingerprint | `release::double_rebuild_fingerprint_stable` | veloct-release | P |
| A6 | Normalised route/page/asset/sitemap/RSS/WIP/Works manifests compare stably | `release::normalised_manifest_equal` | veloct-release | P |
| A7 | No content change ⇒ publish creates no empty commit | `release::no_change_no_commit` | veloct-git | P |
| A8 | `wip_publish=false` never reaches `dist` | `wip::unpublished_wip_absent_from_dist` | veloct-release | P |
| A9 | `wip_publish=true` produces `/wip/<stable-id>` | `wip::published_wip_route_emitted` | veloct-release | P |
| A10 | A previously-public WIP, once finished, emits a permanent redirect to `/article/<slug>` | `wip::finished_wip_redirects` | veloct-release | P |
| A11 | Works shell and Works origin share the same Release fingerprint | `release::works_and_site_same_fingerprint` | veloct-deploy | P |
| A12 | `build_inputs_hash` excludes `commit_sha` | `release::inputs_hash_excludes_commit` | veloct-release | P |
| A13 | `build_env_hash` hashes only allow-listed vars | `release::env_hash_whitelist_only` | veloct-release | P |
| A14 | Only one `dist/` exists after repeated publishes | `release::single_dist_retained` | veloct-release | P |

## B. Attachment consistency — handoff §24

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| B1 | Same content + same target + same namespace ⇒ reuse | `attach::dedup_same_namespace_reuses` | veloct-attach | P |
| B2 | Same content + different target ⇒ no wrong URL reuse | `attach::dedup_other_target_not_reused` | veloct-attach | P |
| B3 | Same name, different content ⇒ new random suffix | `attach::same_name_diff_content_new_suffix` | veloct-attach | P |
| B4 | Upload failure keeps the cache file | `attach::upload_failure_keeps_cache` | veloct-attach | P |
| B5 | Remote success + Markdown update failure loses no file | `attach::apply_failure_keeps_recoverable` | veloct-attach | P |
| B6 | Removing an article reference never deletes the remote file | `attach::remove_reference_keeps_remote` | veloct-attach | P |
| B7 | Bulk cleanup cannot delete a still-referenced file | `attach::cleanup_protects_referenced` | veloct-attach | P |
| B8 | Bulk cleanup cannot delete `PENDING`/`UPLOADING` | `attach::cleanup_protects_inflight` | veloct-attach | P |
| B9 | `SUCCEEDED + FAILED + PRESENT` retries reference application without re-upload | `attach::retry_apply_no_reupload` | veloct-attach | P |
| B10 | Multi-reference: dropping one reference does not orphan the attachment | `attach::multi_reference_set_semantics` | veloct-attach | P |
| B11 | Concurrent upload of same target+namespace+hash yields one remote object | `attach::concurrent_upload_single_object` | veloct-attach | P |
| B12 | Filename gets a fixed-6-char suffix before the extension | `attach::filename_suffix_format` | veloct-attach | P |
| B13 | Changing the public URL prefix does not move or delete remote files | `attach::url_prefix_change_no_move` | veloct-attach | P |
| B14 | Network image sources bypass the cache pool entirely | `attach::remote_image_no_cache` | veloct-attach | P |

## C. AI safety & lifecycle — handoff §25

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| C1 | User cancel ⇒ `CANCELLED`, no fallback | `ai::cancel_no_fallback` | veloct-ai | P |
| C2 | Provider connection failure ⇒ fallback | `ai::connection_error_falls_back` | veloct-ai | P |
| C3 | Provider timeout ⇒ fallback | `ai::timeout_falls_back` | veloct-ai | P |
| C4 | Provider 4xx/5xx ⇒ fallback | `ai::http_error_falls_back` | veloct-ai | P |
| C5 | Fallback failure ⇒ final error, no loop | `ai::fallback_failure_terminal` | veloct-ai | P |
| C6 | AI output never rewrites the body before confirmation | `ai::no_write_before_confirm` | veloct-app | P |
| C7 | Partial apply changes only the selected range | `ai::partial_apply_bounded` | veloct-app | P |
| C8 | Secrets never reach the log | `core::redaction_blocks_secrets` | veloct-core | P |
| C9 | Fallback uses an independent provider when configured | `ai::fallback_independent_provider` | veloct-ai | P |
| C10 | Model list refresh + manual add both work | `ai::model_refresh_and_manual_add` | veloct-ai | P |

## D. Restore / concurrency — handoff §25.1

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| D1 | Full restore leaves Git tags and Release metadata usable | `backup::full_restore_tags_intact` | veloct-backup | P |
| D2 | Content-only restore marks the tree as changed but leaves Releases immutable | `backup::content_restore_marks_changed` | veloct-backup | P |
| D3 | Editing during backup never yields a half-written file in the archive | `backup::snapshot_is_consistent` | veloct-backup | P |
| D4 | Publish holds an exclusive lock; a second publish cannot start | `core::exclusive_lock_blocks_second_publish` | veloct-core | P |
| D5 | Restore's exclusive lock blocks competing autosave/backup | `core::lock_blocks_competing_writers` | veloct-core | P |
| D6 | In-flight attachment uploads survive app close as recoverable state | `attach::close_leaves_recoverable_state` | veloct-attach | P |
| D7 | Secret scan blocks silent upload and records the finding | `backup::secret_scan_blocks_upload` | veloct-backup | P |
| D8 | Backup archives contain no secret value and no credential store data | `backup::archive_excludes_secrets` | veloct-backup | P |
| D9 | Retention default is 3 and configurable | `backup::retention_default_three` | veloct-backup | P |
| D10 | One failing cloud target does not fail the others | `backup::partial_target_failure_isolated` | veloct-backup | P |

## E. Content / index integrity — handoff §3, §9, §20, §21

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| E1 | SQLite index can be dropped and rebuilt identically from Markdown | `storage::index_rebuild_is_identical` | veloct-storage | P |
| E2 | `id` survives a `slug` change and the file does not move | `content::slug_change_keeps_id` | veloct-content | P |
| E3 | External disk modification is detected by hash, never silently overwritten | `content::external_change_detected` | veloct-content | P |
| E4 | Duplicate paragraph id produces a build error, not a guess | `markdown::duplicate_paragraph_id_fails` | veloct-markdown | **I** |
| E5 | Missing reference id keeps a broken state | `markdown::broken_reference_stays_broken` | veloct-markdown | **I** |
| E6 | Paragraph ids are invisible in rendered HTML | `markdown::paragraph_ids_invisible` | veloct-markdown | **I** |
| E7 | Editing paragraph text preserves its id | `markdown::text_edit_keeps_id` | veloct-markdown | **I** |
| E8 | Harvard-style reference list contains every referenced knowledge entry | `markdown::reference_list_complete` | veloct-markdown | **I** |
| E9 | Autosave never creates a Git commit | `git::autosave_no_commit` | veloct-git | P |
| E10 | Unresolved merge conflicts block publishing | `git::conflict_blocks_publish` | veloct-git | P |
| E11 | Word count / reading time are computed and stored | `markdown::word_count_and_reading_time` | veloct-markdown | **I** |
| E12 | Work import (zip + folder) produces `work.json` + `site/` truth | `work::import_zip_and_folder` | veloct-content | P |
| E13 | Works route set is derived from `work.json`, not from the DB as truth | `work::routes_from_truth` | veloct-release | P |

## F. Process / preview lifecycle — handoff §5.3, §20.4

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| F1 | A free local port is chosen and never collides | `process::picks_free_port` | veloct-process | P |
| F2 | Preview crash surfaces an explicit error state | `process::crash_is_reported` | veloct-process | P |
| F3 | Preview restarts automatically after abnormal exit | `process::auto_restart` | veloct-process | P |
| F4 | App close kills the whole child process tree | `process::close_kills_tree` | veloct-process | P |
| F5 | A stale instance cannot hold the port | `process::no_stale_port_holder` | veloct-process | P |
| F6 | Works preview runs on a different origin than the main preview | `process::works_origin_separate` | veloct-process | P |

## G. Deployment — handoff §14.10–§14.13, §10.4

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| G1 | Independent checkbox delivery per logical target | `deploy::targets_independent` | veloct-deploy | P |
| G2 | Partial success rolls up to `PARTIAL` and never retracts the success | `deploy::partial_rollup` | veloct-deploy | P |
| G3 | Failure does not auto-retry | `deploy::no_auto_retry` | veloct-deploy | P |
| G4 | Failed sub-target re-publish uses the frozen snapshot | `deploy::resend_uses_frozen_snapshot` | veloct-deploy | P |
| G5 | A target without a Works Origin is reported as degraded | `deploy::degraded_without_works_origin` | veloct-deploy | P |
| G6 | 1Panel path exports `dist/site` + `dist/works` for manual upload | `deploy::manual_export_layout` | veloct-deploy | P |
| G7 | SEO push failure never marks the site release as failed | `deploy::seo_push_failure_isolated` | veloct-deploy | P |

## H. Handoff §28 completion definition — end-to-end acceptance

The following must hold on a real project directory before "complete version" can be claimed:

| # | Acceptance | Evidence required |
|---|---|---|
| H1 | Create and manage one site project | command transcript + resulting tree |
| H2 | Edit Markdown, autosave (2 s idle / 30 s fallback / save on close) | test + manual transcript |
| H3 | SQLite index rebuilds | E1 |
| H4 | Real Astro live preview (same project as production build) | running dev server + fetched HTML |
| H5 | WIP independent static control + stable URL + finished redirect | A8–A10 |
| H6 | Knowledge stable paragraph references + broken-reference repair | E4–E8 |
| H7 | Page creator with templates/blocks/Markdown | page-creator test |
| H8 | AI per feature provider/model + correct fallback | C1–C10 |
| H9 | AI confirm / apply-all / apply-partial | C6, C7 |
| H10 | Work import, screenshot, iframe preview, main-site isolation | E12, E13, F6, G5 |
| H11 | Publish commit + Release tag correct | A1, A7 |
| H12 | One build per publish; all targets share it | A2, A11, A14 |
| H13 | Failed target re-publish by original Release | A3, G4 |
| H14 | 1Panel manual upload of current `dist` | G6 |
| H15 | Attachments: cache pool, sftp, s3, lsky, github, remote image | B1–B14 |
| H16 | Dedup safe; multi-reference safe; no auto remote delete | B6, B7, B10, B11 |
| H17 | Auto backup, manual backup, local export, full restore, content-only restore | D1–D10 |
| H18 | No secret in config, log or backup; secret scan before backup | C8, D7, D8 |
| H19 | SEO, comments, logging operational | seo/comment/log tests |
| H20 | Key failure paths have explicit error states, never silent failure | diagnostics tests |

## I. Honesty rule applied to this matrix

A row may only move to **I** when its test exists **and** passes in the recorded verification
command. Integration points that depend on third-party services Veloct cannot authenticate
against (Cloudflare, EdgeOne, Lsky, S3, GitHub) are tested against a **local fake server**
implementing the documented protocol, and the test name must say so, e.g.
`deploy::cloudflare_upload_against_fake_api`. Vendor behaviour that was never verified against
a live endpoint stays marked `UNVERIFIED` in the matrix and in the UI.

## J. Recorded verification runs

| Run | Command | Result |
|---|---|---|
| 2026-09-28 · foundation | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **67 passed, 0 failed** (58 in `veloct-core`, 9 in `veloct-app`) |
| 2026-09-28 · shell | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 check -p veloct-desktop` | exit 0 |
| 2026-09-28 · markdown | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **172 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 105 `veloct-markdown`) |
| 2026-09-28 · lints | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 clippy --workspace --exclude veloct-desktop --all-targets` | no warnings |

Rows at **I** as a result of these runs:

| Row | Test |
|---|---|
| C8 | `secret::pattern_layer_catches_provider_keys`, `secret::known_value_layer_catches_arbitrary_secrets`, `secret::redact_json_keeps_credential_refs_but_removes_values`, `http::error_for_status_redacts_credentials_from_the_body`, `http::urls_are_redacted_before_logging` |
| D4 | `lock::a_second_publish_cannot_start` |
| D5 | `lock::restore_blocks_competing_autosave`, `lock::shared_operations_block_a_later_publish`, `app::shared_lock_mode_permits_concurrent_readers` |
| E4 | `parse::duplicate_paragraph_ids_are_reported_as_errors`, `references::duplicate_paragraph_ids_in_one_document_are_errors` |
| E5 | `references::a_reference_to_a_missing_paragraph_stays_broken_and_never_guesses`, `references::a_reference_to_an_unknown_document_reports_the_unknown_content_id`, `references::resolve_all_reports_every_broken_reference` |
| E6 | `directives::paragraph_markers_are_read_and_removed_from_the_residual_body`, `directives::an_inline_marker_is_stripped_from_text_without_becoming_a_block_marker`, `parse::a_paragraph_marker_is_attached_to_the_paragraph_that_follows_it` |
| E7 | `rewrite::assigning_ids_twice_adds_nothing_the_second_time`, `rewrite::replacing_a_paragraph_by_id_changes_only_that_paragraph`, `rewrite::replacing_a_paragraph_that_no_longer_exists_reports_that_it_was_not_found` |
| E8 | `references::the_reference_list_contains_each_cited_entry_once_and_is_sorted`, `references::an_article_with_no_references_has_no_reference_section` |
| E11 | `stats::stats_of_ast_counts_prose_and_headings_but_not_code`, `stats::stats_of_ast_includes_lists_and_tables`, `stats::counting_is_deterministic_for_the_same_input` |
| — | `app::create_materialises_the_documented_scaffold`, `app::creating_over_an_existing_project_is_rejected`, `app::opening_a_foreign_directory_fails_instead_of_guessing`, `app::open_then_reopen_detects_the_project_and_updates_last_opened`, `app::creating_a_project_takes_an_exclusive_lock_against_publish` |

Everything else remains **P**: the owning workstream has to land the test and record its command
in this table before the row may move.
