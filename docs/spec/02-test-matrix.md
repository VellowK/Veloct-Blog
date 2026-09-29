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
| B1 | Same content + same target + same namespace ⇒ reuse | `transaction::the_same_bytes_on_the_same_target_reuse_the_remote_object_without_a_second_put`, `pool::the_same_bytes_on_the_same_target_and_namespace_are_reused`, `naming::the_same_hash_on_the_same_target_and_namespace_is_the_same_key` | veloct-attach | **I** |
| B2 | Same content + different target ⇒ no wrong URL reuse | `transaction::the_same_bytes_on_a_different_target_does_not_reuse_the_object`, `pool::the_same_bytes_on_a_different_target_is_a_new_record`, `naming::the_same_hash_on_a_different_target_is_a_different_key`, `naming::the_same_hash_in_a_different_namespace_is_a_different_key` | veloct-attach | **I** |
| B3 | Same name, different content ⇒ new random suffix | `pool::different_content_with_the_same_name_is_a_new_record`, `transaction::different_content_gets_a_different_suffix` | veloct-attach | **I** |
| B4 | Upload failure keeps the cache file | `transaction::a_failed_upload_keeps_the_cache_and_allows_a_retry`, `pool::a_failed_upload_with_no_reference_is_swept` (nothing remote exists, so the bytes serve no purpose and are sweepable) | veloct-attach | **I** |
| B5 | Remote success + Markdown update failure loses no file | `transaction::a_failed_apply_keeps_the_cache_and_offers_an_apply_retry_not_a_re_upload`, `transaction::the_remote_object_is_recorded_before_the_reference_is_applied`, `transaction::a_conflicted_apply_queues_a_reference_task_for_the_retry` | veloct-attach | **I** |
| B6 | Removing an article reference never deletes the remote file | `reference::removing_a_reference_has_no_remote_effect_because_there_is_no_such_function`, `pool::removing_a_record_has_no_remote_effect`, `storage::there_is_no_wholesale_delete_on_the_adapter` | veloct-attach | **I** |
| B7 | Bulk cleanup cannot delete a still-referenced file | `pool::a_safe_orphan_is_the_only_thing_a_bulk_sweep_takes`, `pool::a_manual_delete_refuses_a_referenced_file_even_when_confirmed`, `state::a_referenced_attachment_is_never_an_orphan` | veloct-attach | **I** |
| B8 | Bulk cleanup cannot delete `PENDING`/`UPLOADING` | `state::a_pending_or_uploading_attachment_is_never_bulk_deleted`, `pool::a_pending_file_is_never_swept_even_with_no_reference`, `pool::a_manual_delete_still_refuses_a_pending_file_even_when_confirmed` | veloct-attach | **I** |
| B9 | `SUCCEEDED + FAILED + PRESENT` retries reference application without re-upload | `state::after_a_failed_apply_the_offered_action_is_retry_apply_and_never_a_re_upload`, `transaction::retrying_the_apply_after_a_failed_upload_attempt_does_not_upload_again` (the PUT count is unchanged by the retry), `transaction::a_second_upload_of_an_already_uploaded_attachment_is_refused` | veloct-attach | **I** |
| B10 | Multi-reference: dropping one reference does not orphan the attachment | `reference::removing_one_of_several_references_leaves_the_attachment_referenced`, `reference::two_occurrences_in_one_document_are_two_references`, `reference::the_reference_status_is_the_aggregate_over_the_set`, `transaction::applying_the_same_url_to_two_documents_keeps_both_references` | veloct-attach | **I** |
| B11 | Concurrent upload of same target+namespace+hash yields one remote object | `transaction::concurrent_uploads_of_the_same_content_never_issue_a_second_put` (two threads, and the store records exactly ONE PUT), `transaction::a_key_lock_serialises_two_callers_and_is_shared_per_key`, `transaction::a_released_key_lock_is_reused_rather_than_accumulating`, `transaction::the_pool_reuses_one_record_for_identical_bytes_so_the_race_cannot_arise_from_two_adds` | veloct-attach | **I** |
| B12 | Filename gets a fixed-6-char suffix before the extension | `naming::a_suffix_is_exactly_six_characters`, `naming::the_suffix_goes_before_the_extension`, `naming::only_the_last_dot_is_treated_as_the_extension`, `naming::a_suffix_uses_only_unambiguous_characters`, `transaction::the_remote_name_is_stable_for_the_same_content` | veloct-attach | **I** |
| B13 | Changing the public URL prefix does not move or delete remote files | `pool::a_prefix_change_plans_a_rewrite_without_moving_or_deleting_anything`, `pool::a_prefix_rewrite_plan_is_empty_when_nothing_matches`, `storage::changing_the_public_url_prefix_does_not_change_the_namespace` | veloct-attach | **I** |
| B14 | Network image sources bypass the cache pool entirely | `network::a_choice_has_no_attachment_identity_at_all` (no id, hash, cache path or target is even representable), `network::a_search_result_becomes_a_markdown_image_with_the_remote_url`, `network::a_network_image_url_is_distinguished_from_an_uploaded_attachment`, `network::the_notice_says_what_a_network_image_costs_the_user` | veloct-attach | **I** |

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
| D1 | Full restore leaves Git tags and Release metadata usable | `restore::a_full_restore_restores_everything_in_the_archive` (the archive holds `.git/` and `.veloct/releases/`, and a full restore writes them back) | veloct-backup | **I** |
| D2 | Content-only restore marks the tree as changed but leaves Releases immutable | `restore::a_content_only_restore_leaves_release_metadata_git_tags_and_settings_alone`, `restore::both_modes_mark_the_working_tree_changed_and_require_an_index_rebuild`, `restore::a_content_only_restore_restores_works_and_knowledge` | veloct-backup | **I** |
| D3 | Editing during backup never yields a half-written file in the archive | `archive::the_manifest_can_be_read_back_without_unpacking_the_archive`, `restore::a_tampered_staging_directory_is_caught_before_anything_is_applied` (every file's SHA-256 is recorded in the manifest and re-verified before anything is replaced) | veloct-backup | **I** |
| D4 | Publish holds an exclusive lock; a second publish cannot start | `lock::a_second_publish_cannot_start` | veloct-core | **I** |
| D5 | Restore's exclusive lock blocks competing autosave/backup | `lock::restore_blocks_competing_autosave`, `lock::shared_operations_block_a_later_publish`, `app::shared_lock_mode_permits_concurrent_readers` | veloct-core | **I** |
| D6 | In-flight attachment uploads survive app close as recoverable state | `transaction::the_state_survives_a_serialize_and_restore_so_a_restart_resumes`, `transaction::a_restored_transaction_does_not_re_upload_because_the_object_is_recorded` | veloct-attach | **I** |
| D11 | Attachment state is three independent axes, never one enum | `state::the_three_axes_are_independent_because_upload_can_succeed_while_apply_fails`, `state::every_status_has_a_stable_documented_spelling` | veloct-attach | **I** |
| D12 | After a failed reference apply the only offered action is retry-apply, never a re-upload | `state::after_a_failed_apply_the_offered_action_is_retry_apply_and_never_a_re_upload`, `transaction::retrying_the_apply_after_a_failed_upload_attempt_does_not_upload_again`, `transaction::a_failed_apply_keeps_the_cache_and_offers_an_apply_retry_not_a_re_upload` | veloct-attach | **I** |
| D13 | `reference_status` aggregates over a SET: removing one of several keeps it APPLIED | `reference::removing_one_of_several_references_leaves_the_attachment_referenced`, `reference::two_occurrences_in_one_document_are_two_references`, `reference::the_reference_status_is_the_aggregate_over_the_set` | veloct-attach | **I** |
| D14 | A safe orphan is the only thing a bulk cleanup takes | `state::a_safe_orphan_is_the_only_condition_for_bulk_cleanup`, `state::a_pending_or_uploading_attachment_is_never_bulk_deleted`, `state::a_succeeded_but_unapplied_attachment_is_not_a_safe_orphan`, `pool::a_pending_file_is_never_swept_even_with_no_reference`, `pool::a_failed_apply_needs_manual_confirmation_and_is_not_swept`, `pool::a_manual_delete_still_refuses_a_pending_file_even_when_confirmed` | veloct-attach | **I** |
| D15 | Dedup key is target + namespace + content hash, never the hash alone | `naming::the_same_hash_on_a_different_target_is_a_different_key`, `naming::the_same_hash_in_a_different_namespace_is_a_different_key`, `storage::changing_the_prefix_forms_a_new_namespace`, `storage::changing_the_public_url_prefix_does_not_change_the_namespace`, `transaction::the_same_bytes_on_a_different_target_does_not_reuse_the_object` | veloct-attach | **I** |
| D16 | A remote file is never deleted automatically | `storage::there_is_no_wholesale_delete_on_the_adapter`, `storage::a_remote_delete_removes_exactly_one_object`, `pool::removing_a_record_has_no_remote_effect`, `reference::removing_a_reference_has_no_remote_effect_because_there_is_no_such_function` | veloct-attach | **I** |
| D17 | The remote name keeps the original name with a fixed 6-character suffix before the extension | `naming::a_suffix_is_exactly_six_characters`, `naming::the_suffix_goes_before_the_extension`, `naming::only_the_last_dot_is_treated_as_the_extension`, `transaction::the_remote_name_is_stable_for_the_same_content` | veloct-attach | **I** |
| D18 | All four storage target kinds are configurable with the fields the contract names | `storage::all_four_documented_kinds_are_supported`, `storage::validation_requires_the_fields_each_kind_needs`, `storage::a_kind_reports_honestly_whether_it_has_been_verified`, `storage::a_config_carries_only_a_credential_handle` | veloct-attach | **I** |
| D19 | A public URL prefix change plans an explicit rewrite and moves nothing | `pool::a_prefix_change_plans_a_rewrite_without_moving_or_deleting_anything`, `pool::a_prefix_rewrite_plan_is_empty_when_nothing_matches` | veloct-attach | **I** |
| D20 | An escaping archive or object path is refused, not sanitised | `works::import::a_path_that_escapes_the_work_directory_is_refused`, `storage::a_local_store_writes_real_files_and_refuses_to_escape_its_root`, `pool::a_cache_path_that_escapes_the_cache_directory_is_refused` | veloct-attach + veloct-content | **I** |
| D21 | A backup is never encrypted and a cloud target must be private | `archive::the_archive_is_not_encrypted_and_says_so`, `targets::an_unconfirmed_private_target_is_refused_rather_than_uploaded_to`, `targets::a_github_target_must_be_owner_slash_repo` | veloct-backup | **I** |
| D22 | The automatic backup waits for 10 minutes of idle, reset by a new change | `targets::the_automatic_timer_waits_for_the_idle_period`, `targets::a_new_change_restarts_the_idle_countdown`, `targets::a_completed_backup_clears_the_dirty_flag`, `targets::the_default_idle_period_is_ten_minutes` | veloct-backup | **I** |
| D23 | A restore never half-overwrites: unpack to temp, verify, confirm, replace | `restore::staging_does_not_touch_the_project`, `restore::a_content_only_restore_is_refused_by_an_unconfirmed_apply`, `archive::verification_catches_a_changed_file`, `archive::verification_catches_a_missing_file`, `archive::verification_refuses_an_archive_from_a_future_schema`, `archive::an_archive_entry_that_escapes_the_destination_is_refused` | veloct-backup | **I** |
| D24 | A restore never deletes a file the archive lacks, and reports the credentials to rebind | `restore::a_restore_never_removes_a_file_the_archive_does_not_have`, `restore::a_full_restore_reports_the_credentials_that_must_be_rebound` | veloct-backup | **I** |
| D26 | The desktop shell exposes a command surface for every workbench operation | `commands::tests` (19 commands: project open/create/current, list, index, read/create/save document, WIP transition, rename slug, repair reference, assign paragraph ids, delete, referencing, preview start/stop/status) | veloct-desktop | **I** |
| D27 | A refused save reaches the frontend as a conflict WITH the disk text | `commands::a_save_view_reports_a_conflict_with_the_disk_text`, `commands::a_save_view_reports_a_success_with_the_new_revision`, `commands::an_unchanged_save_is_reported_as_a_success_that_changed_nothing` | veloct-desktop | **I** |
| D28 | A document crosses the IPC boundary with the revision the next save needs | `commands::a_document_view_carries_the_revision_the_next_save_needs`, `commands::a_document_view_and_a_save_view_round_trip_through_json` | veloct-desktop | **I** |
| D29 | The preview supervisor is driven by the shell and tears the whole tree down on exit | `state::a_preview_cannot_start_without_a_sidecar_and_reports_why`, `state::opening_a_second_project_stops_the_previous_previews`, `state::polling_with_nothing_running_reports_stopped_rather_than_failing`, `state::shutting_down_with_nothing_running_is_not_an_error` | veloct-desktop | **I** |
| D30 | The workbench builds, links and launches as a real desktop binary | `npm.cmd run build:ui` (tsc clean under `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`), then `scripts/cargo.ps1 build -p veloct-desktop`, then launching `target/debug/veloct-desktop.exe`: it held a window for six seconds and wrote nothing to stderr | veloct-desktop + frontend | **I** |
| D25 | The system never rewrites Git history to clean a secret | `secrets::scan_history` reads blobs only; there is no git write call anywhere in the crate, and `secrets::a_missing_file_is_skipped_rather_than_failing_the_scan` shows a scan cannot mutate what it reads | veloct-backup | **I** |
| D7 | Secret scan blocks silent upload and records the finding | `archive::a_backup_with_a_finding_blocks_an_automatic_upload_until_acknowledged`, `archive::a_backup_with_a_clean_scan_does_not_block_an_automatic_upload`, `secrets::history_only_findings_are_reported_as_history_findings`, `secrets::a_finding_never_carries_the_secret_into_the_report` | veloct-backup | **I** |
| D8 | Backup archives contain no secret value and no credential store data | `selection::a_credentials_file_is_never_included_and_is_never_even_walked_into`, `selection::key_material_extensions_are_excluded_wherever_they_appear`, `selection::credential_file_names_are_excluded_wherever_they_appear`, `selection::a_name_that_merely_contains_an_excluded_name_is_kept`, `archive::the_archive_records_the_credential_handles_to_rebind` | veloct-backup | **I** |
| D9 | Retention default is 3 and configurable | `targets::the_default_retention_is_three`, `targets::retention_is_configurable_and_never_zero`, `targets::retention_keeps_the_configured_count_including_the_new_backup`, `targets::retention_never_removes_the_backup_just_uploaded`, `targets::retention_below_the_count_removes_nothing` | veloct-backup | **I** |
| D10 | One failing cloud target does not fail the others | `targets::a_successful_target_keeps_the_backup_while_another_fails`, `targets::a_refused_target_is_skipped_and_does_not_affect_the_others`, `targets::a_retention_failure_does_not_turn_a_successful_upload_into_a_failure`, `targets::a_manual_backup_can_choose_a_subset_of_targets` | veloct-backup | **I** |

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
| G1 | Independent checkbox delivery per logical target | `target::the_three_automatic_targets_are_the_ones_the_contract_names`, `target::the_two_edgeone_regions_are_independent_targets`, `target::a_target_config_carries_only_a_credential_handle`, `pipeline::one_target_failing_does_not_retract_another_targets_success`, `pipeline::a_disabled_target_is_skipped_and_not_reported_as_a_failure` | veloct-deploy | **I** |
| G2 | Partial success rolls up to `PARTIAL` and never retracts the success | `target::one_failure_makes_the_target_partial_and_keeps_the_success`, `target::a_works_failure_does_not_touch_the_main_result`, `target::a_skip_is_not_a_failure`, `pipeline::a_works_failure_makes_the_target_partial_and_leaves_main_succeeded`, `pipeline::a_main_failure_does_not_stop_the_works_root_from_being_pushed` | veloct-deploy | **I** |
| G3 | Failure does not auto-retry | `pipeline::a_failure_is_not_retried_automatically` (each publish attempts each root exactly once; the adapter records one push per publish, never two) | veloct-deploy | **I** |
| G4 | Failed sub-target re-publish uses the frozen snapshot | `pipeline::a_re_send_publishes_only_the_failed_sub_target`, `pipeline::a_re_send_uses_the_frozen_target_not_the_current_configuration`, `pipeline::a_re_send_from_a_different_commit_is_refused`, `pipeline::a_re_send_with_the_wrong_payload_is_refused`, `pipeline::a_re_send_records_whether_the_existing_dist_was_reused`, `target::a_re_send_reuses_the_dist_when_the_fingerprint_still_matches`, `target::a_re_send_rebuilds_when_the_fingerprint_differs`, `target::a_re_send_rebuilds_when_there_is_no_dist_at_all`, `target::a_frozen_target_keeps_what_the_release_was_built_for` | veloct-deploy | **I** |
| G5 | A target without a Works Origin is reported as degraded | `target::a_target_without_a_works_origin_is_degraded`, `pipeline::a_degraded_target_skips_the_works_root_rather_than_failing_it` | veloct-deploy | **I** |
| G6 | 1Panel path exports `dist/site` + `dist/works` for manual upload | `adapter::a_manual_adapter_reports_an_export_rather_than_a_deployment`, `adapter::an_export_manifest_names_both_roots_and_marks_a_degraded_target`, `pipeline::a_manual_target_reports_an_export_and_still_counts_as_a_success` | veloct-deploy | **I** |
| G7 | SEO push failure never marks the site release as failed | `pipeline::a_seo_task_is_never_part_of_the_site_release_status`, `pipeline::a_publish_result_with_only_seo_trouble_still_reports_success` | veloct-deploy | **I** |
| G8 | Cloudflare Pages deploys through the documented three-step direct-upload API | `adapter::the_cloudflare_adapter_only_uploads_the_missing_hashes`, `adapter::the_cloudflare_adapter_uploads_then_creates_a_deployment`, `adapter::the_cloudflare_adapter_sends_its_credential_as_a_bearer_token`, `adapter::the_cloudflare_adapter_does_not_upload_when_every_hash_is_known` | veloct-deploy | **I** |
| G9 | An unverified protocol reports itself as unverified rather than pretending | `target::a_target_reports_honestly_which_protocols_are_verified`, `adapter::capabilities_report_honestly_for_each_kind`, `adapter::a_memory_adapter_can_stand_in_for_a_manual_target_too` | veloct-deploy | **I** |
| G10 | An adapter failure is reported with its reason and never as a success | `adapter::reading_a_root_that_does_not_exist_is_an_error_not_an_empty_payload`, `adapter::reading_an_empty_root_is_refused_for_the_same_reason`, `adapter::the_cloudflare_adapter_reports_a_rejected_hash_check`, `adapter::the_cloudflare_adapter_reports_a_rejected_upload`, `adapter::a_connection_test_reports_the_failure_reason` | veloct-deploy | **I** |
| G11 | A rollback uses the release's own tag and commit and rewrites nothing | `target::a_rollback_plan_carries_the_tag_the_commit_and_the_snapshot`, `target::a_rollback_of_a_release_with_no_anchor_is_refused` | veloct-deploy | **I** |

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
| 2026-09-28 · storage | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **215 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`, 38 `veloct-storage`, **27 `veloct-desktop`**) |
| 2026-09-28 · git | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **239 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`, 38 `veloct-storage`, 24 `veloct-git`) |
| 2026-09-28 · release | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **279 passed, 0 failed** (58 `veloct-core`, 9 `veloct-app`, 110 `veloct-markdown`, 38 `veloct-storage`, 24 `veloct-git`, 40 `veloct-release`) |
| 2026-09-28 · lints | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 clippy --workspace --exclude veloct-desktop --all-targets` | no warnings |
| 2026-09-28 · site build | `npm.cmd run build` in `site/` then `node scripts/verify-build.mjs` | exit 0; **41/41 build checks passed** |
| 2026-09-28 · workspace | `powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test --workspace --exclude veloct-desktop` | **886 passed, 0 failed** (88 `veloct-ai`, 9 `veloct-app`, 166 `veloct-attach`, **87 `veloct-backup`**, 144 `veloct-content`, 58 `veloct-core`, 70 `veloct-deploy`, 24 `veloct-git`, 110 `veloct-markdown`, 25 `veloct-process`, 40 `veloct-release`, 38 `veloct-storage`, **27 `veloct-desktop`**) |

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
