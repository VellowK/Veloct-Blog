# Veloct — Test traceability matrix

> `handoff.md` §23, §24, §25 and §28 define behaviours that **must** be covered by automated
> tests. This file is the authoritative map from contract clause → concrete test, and it is
> the checklist used to decide whether the project is "complete". A clause with no test is an
> open work item, not a finished feature.

Legend: **P** = planned, **I** = implemented, **F** = failing (regression).

## A. Release / build consistency — handoff §23

| # | Clause | Test id | Crate / path | State |
|---|---|---|---|---|
| A1 | Release tag unique and always points at its publish commit | `release::tag_points_at_commit` | veloct-release | **I** |
| A2 | All sub-targets of one Release receive an identical build fingerprint | `release::fingerprint_shared_across_subtargets` | veloct-release | **I** |
| A3 | Re-publishing a failed sub-target never mixes in later working-tree changes | `release::resend_uses_original_commit` | veloct-release | **I** |
| A4 | Rebuilding a historical Release never reads current SQLite config | `release::rebuild_ignores_current_db` | veloct-release | **I** |
| A5 | Two rebuilds of one Release produce the same fingerprint | `release::double_rebuild_fingerprint_stable` | veloct-release | **I** |
| A6 | Normalised route/page/asset/sitemap/RSS/WIP/Works manifests compare stably | `release::normalised_manifest_equal` | veloct-release | **I** |
| A7 | No content change ⇒ publish creates no empty commit | `release::no_change_no_commit` | veloct-git | **I** |
| A8 | `wip_publish=false` never reaches `dist` | `wip::unpublished_wip_absent_from_dist` | veloct-release | **I** |
| A9 | `wip_publish=true` produces `/wip/<stable-id>` | `wip::published_wip_route_emitted` | veloct-release | **I** |
| A10 | A previously-public WIP, once finished, emits a permanent redirect to `/article/<slug>` | `wip::finished_wip_redirects` | veloct-release | **I** |
| A11 | Works shell and Works origin share the same Release fingerprint | `release::works_and_site_same_fingerprint` | veloct-deploy | **I** |
| A12 | `build_inputs_hash` excludes `commit_sha` | `release::inputs_hash_excludes_commit` | veloct-release | **I** |
| A13 | `build_env_hash` hashes only allow-listed vars | `release::env_hash_whitelist_only` | veloct-release | **I** |
| A14 | Only one `dist/` exists after repeated publishes | `release::single_dist_retained` | veloct-release | **I** |

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
| C1 | User cancel ⇒ `CANCELLED`, no fallback | `error::cancellation_is_not_an_error_and_never_falls_back`, `service::cancellation_never_falls_back`, `service::cancelling_mid_flight_does_not_contact_the_fallback` | veloct-ai | **I** |
| C2 | Provider connection failure ⇒ fallback | `error::every_other_failure_does_fall_back`, `error::transport_failures_distinguish_a_timeout_from_a_refusal` | veloct-ai | **I** |
| C3 | Provider timeout ⇒ fallback | `error::transport_failures_distinguish_a_timeout_from_a_refusal`, `error::every_other_failure_does_fall_back` | veloct-ai | **I** |
| C4 | Provider 4xx/5xx ⇒ fallback | `error::http_statuses_are_classified_into_the_documented_kinds`, `service::a_real_failure_falls_back_and_the_result_says_so`, `provider::a_provider_error_status_becomes_a_classified_failure` | veloct-ai | **I** |
| C5 | Fallback failure ⇒ final error, no loop | `error::a_fallback_failure_is_terminal`, `service::a_fallback_failure_is_terminal` | veloct-ai | **I** |
| C6 | AI output never rewrites the body before confirmation | `apply::*` (an `ApplyPlan` is computed for review; `apply` is a separate explicit step), `feature::every_feature_requires_confirmation_before_it_writes` | veloct-ai | **I** |
| C7 | Partial apply changes only the selected range | `apply::a_selection_apply_touches_only_the_selected_range`, `apply::a_selection_apply_reports_the_changed_range`, `apply::an_apply_across_a_multibyte_boundary_preserves_the_rest`, `apply::an_invalid_range_is_an_error_not_a_fallback_to_replacing_everything` | veloct-ai | **I** |
| C8 | Secrets never reach the log | `core::redaction_blocks_secrets` | veloct-core | **I** |
| C9 | Fallback uses an independent provider when configured | `service::a_fallback_on_an_independent_provider_is_used` | veloct-ai | **I** |
| C10 | Model list refresh + manual add both work | `provider::model_lists_are_parsed_from_both_envelopes`, `provider::a_manual_model_survives_a_refresh_that_does_not_return_it`, `provider::a_fetched_model_is_updated_by_a_later_refresh` | veloct-ai | **I** |

## D. Restore / concurrency — handoff §25.1

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| D1 | Full restore leaves Git tags and Release metadata usable | `backup::full_restore_tags_intact` | veloct-backup | P |
| D2 | Content-only restore marks the tree as changed but leaves Releases immutable | `backup::content_restore_marks_changed` | veloct-backup | P |
| D3 | Editing during backup never yields a half-written file in the archive | `backup::snapshot_is_consistent` | veloct-backup | P |
| D4 | Publish holds an exclusive lock; a second publish cannot start | `lock::a_second_publish_cannot_start` | veloct-core | **I** |
| D5 | Restore's exclusive lock blocks competing autosave/backup | `lock::restore_blocks_competing_autosave`, `lock::shared_operations_block_a_later_publish`, `app::shared_lock_mode_permits_concurrent_readers` | veloct-core | **I** |
| D6 | In-flight attachment uploads survive app close as recoverable state | `attach::close_leaves_recoverable_state` | veloct-attach | P |
| D7 | Secret scan blocks silent upload and records the finding | `backup::secret_scan_blocks_upload` | veloct-backup | P |
| D8 | Backup archives contain no secret value and no credential store data | `backup::archive_excludes_secrets` | veloct-backup | P |
| D9 | Retention default is 3 and configurable | `backup::retention_default_three` | veloct-backup | P |
| D10 | One failing cloud target does not fail the others | `backup::partial_target_failure_isolated` | veloct-backup | P |

## E. Content / index integrity — handoff §3, §9, §20, §21

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| E1 | SQLite index can be dropped and rebuilt identically from Markdown | `storage::index_rebuild_is_identical` | veloct-storage | **I** |
| E2 | `id` survives a `slug` change and the file does not move | `lifecycle::a_transition_preserves_the_stable_id_and_the_slug`, `service::renaming_a_slug_does_not_move_the_file_or_change_the_id`, `works::metadata::the_site_directory_is_derived_from_the_id_and_not_the_slug` | veloct-content | **I** |
| E3 | External disk modification is detected by hash, never silently overwritten | `service::saving_after_an_external_edit_is_refused_and_returns_the_disk_text`, `service::a_transition_respects_an_external_edit`, `model::an_invalid_range_is_an_error_not_a_fallback_to_replacing_everything` | veloct-content | **I** |
| E4 | Duplicate paragraph id produces a build error, not a guess | `markdown::duplicate_paragraph_id_fails` | veloct-markdown | **I** |
| E5 | Missing reference id keeps a broken state | `markdown::broken_reference_stays_broken` | veloct-markdown | **I** |
| E6 | Paragraph ids are invisible in rendered HTML | `markdown::paragraph_ids_invisible` | veloct-markdown | **I** |
| E7 | Editing paragraph text preserves its id | `markdown::text_edit_keeps_id` | veloct-markdown | **I** |
| E8 | Harvard-style reference list contains every referenced knowledge entry | `markdown::reference_list_complete` | veloct-markdown | **I** |
| E9 | Autosave never creates a Git commit | `git::autosave_no_commit` | veloct-git | **I** |
| E10 | Unresolved merge conflicts block publishing | `git::conflict_blocks_publish` | veloct-git | **I** |
| E11 | Word count / reading time are computed and stored | `markdown::word_count_and_reading_time` | veloct-markdown | **I** |
| E12 | Work import (zip + folder) produces `work.json` + `site/` truth | `works::import::importing_a_folder_copies_the_body_and_writes_the_metadata`, `works::import::importing_a_zip_strips_the_wrapper_directory`, `works::import::exporting_produces_an_archive_that_re_imports_identically`, `works::import::the_metadata_is_written_after_the_body_so_a_half_copy_is_not_listed` | veloct-content | **I** |
| E13 | Works route set is derived from `work.json`, not from the DB as truth | `site/scripts/verify-build.mjs`: `the work body is emitted into dist/works/<slug>/`, `the works shell page IS part of the main site`, `a frozen works manifest is written` — the integration reads `works/<stable-id>/work.json` and the sibling `site/` directly, with no database involved | veloct-release + site | **I** |

## F. Process / preview lifecycle — handoff §5.3, §20.4

| # | Clause | Test id | Crate | State |
|---|---|---|---|---|
| F1 | A free local port is chosen and never collides | `port::a_picked_port_is_actually_free`, `port::two_picks_do_not_collide_while_both_are_held`, `port::a_bound_port_reports_as_not_free` | veloct-process | **I** |
| F2 | Preview crash surfaces an explicit error state | `supervisor::a_child_that_exits_before_ready_is_reported_with_its_output`, `supervisor::a_crash_while_running_is_detected_by_polling`, `supervisor::a_crash_reports_the_child_output_so_the_cause_is_visible`, `supervisor::a_preview_that_never_answers_is_reported_rather_than_waited_on` | veloct-process | **I** |
| F3 | Preview restarts automatically after abnormal exit | `supervisor::a_crashed_preview_is_restarted_automatically`, `supervisor::restarting_gives_up_after_the_policy_is_exhausted` | veloct-process | **I** |
| F4 | App close kills the whole child process tree | `supervisor::stopping_kills_the_whole_process_tree`, `supervisor::stop_all_stops_every_preview` | veloct-process | **I** |
| F5 | A stale instance cannot hold the port | `supervisor::a_restart_uses_a_different_port_so_a_stale_holder_cannot_block_it`, `supervisor::a_preview_that_never_answers_is_reported_rather_than_waited_on` (the wedged child is killed) | veloct-process | **I** |
| F6 | Works preview runs on a different origin than the main preview | `supervisor::the_two_previews_run_on_different_origins` | veloct-process | **I** |

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
| 2026-09-28 · markdown | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **177 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`) |
| 2026-09-28 · storage | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **215 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`, 38 `veloct-storage`) |
| 2026-09-28 · git | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **239 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`, 38 `veloct-storage`, 24 `veloct-git`) |
| 2026-09-28 · release | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **279 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`, 38 `veloct-storage`, 24 `veloct-git`, 40 `veloct-release`) |
| 2026-09-28 · lints | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 clippy --workspace --exclude veloct-desktop --all-targets` | no warnings |
| 2026-09-28 · site build | `npm.cmd run build` in `site/` then `node scripts/verify-build.mjs` | exit 0; **41/41 build checks passed** |
| 2026-09-28 · workspace | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **536 passed, 0 failed** (88 `veloct-ai`, 9 `veloct-app`, **144 `veloct-content`**, 58 `veloct-core`, 24 `veloct-git`, 110 `veloct-markdown`, 25 `veloct-process`, 40 `veloct-release`, 38 `veloct-storage`) |

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
| E11 | Word count / reading time are computed and stored | `stats::stats_of_ast_counts_prose_and_headings_but_not_code`, `stats::stats_of_ast_includes_lists_and_tables`, `stats::counting_is_deterministic_for_the_same_input` |
| E9 | `publish::publishing_with_no_change_creates_no_commit`, `publish::the_nothing_to_commit_path_leaves_the_index_and_tree_alone` |
| E10 | `publish::a_publish_refuses_to_commit_with_an_unresolved_conflict`, `change::a_merge_conflict_is_detected_and_names_the_files` |
| A7 | covered by the same two E9 tests: a publish with no change returns `NoChange` and the branch does not grow |
| A1 | `publish::the_release_tag_points_at_the_publish_commit`, `publish::a_release_id_cannot_be_reused_for_a_different_commit` |
| A3 | `publish::a_tag_can_be_created_at_an_older_commit_for_a_resend`, `publish::a_historical_worktree_can_be_created_for_a_rebuild` |
| A2, A11 | `pipeline::every_sub_target_of_one_release_receives_the_same_fingerprint` |
| A4 | `pipeline::a_rebuild_reads_the_frozen_snapshot_and_not_current_configuration`, `pipeline::a_release_frozen_from_another_release_keeps_the_same_inputs_hash` |
| A5 | `manifest::the_same_inputs_always_hash_the_same`, `pipeline::two_builds_of_one_release_produce_an_identical_normalised_manifest` |
| A6 | `manifest::two_builds_of_one_release_have_no_differences`, `pipeline::a_build_that_varies_between_runs_is_detected` (the negative case, so the comparison can actually fail) |
| A12 | `manifest::the_inputs_hash_excludes_the_commit_sha` |
| A13 | `manifest::the_environment_hash_covers_only_the_allow_list` |
| A14 | `pipeline::only_one_dist_is_kept_across_repeated_builds` |
| A8, A9, A10 | `site/scripts/verify-build.mjs`, run against a real `dist/`: `wip_publish=false produces no production URL`, `wip_publish=true emits /wip/<stable-id>/`, `a published WIP page is noindex`, `a published WIP is NOT also emitted at /article/<slug>/`, `a once-public WIP address still exists after finishing`, `the old WIP address redirects permanently to /article/<slug>/`. The script exits non-zero and names the failure, so a wrong build cannot be mistaken for a good one. |
| — | `manifest::the_manifest_never_contains_its_own_commit_sha`, `pipeline::a_failed_build_is_reported_and_leaves_no_usable_dist`, `pipeline::a_runner_that_forgets_a_publish_root_is_rejected`, `pipeline::a_secret_in_the_configuration_is_redacted_before_it_reaches_disk`, `pipeline::the_frozen_site_template_is_used_for_a_rebuild`, `manifest::the_host_operating_system_does_not_affect_the_fingerprint` |
| — | `app::create_materialises_the_documented_scaffold`, `app::creating_over_an_existing_project_is_rejected`, `app::opening_a_foreign_directory_fails_instead_of_guessing`, `app::open_then_reopen_detects_the_project_and_updates_last_opened`, `app::creating_a_project_takes_an_exclusive_lock_against_publish` |

Everything else remains **P**: the owning workstream has to land the test and record its command
in this table before the row may move.
