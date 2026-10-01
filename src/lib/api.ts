// The typed boundary between the workbench and the Rust command surface.
//
// Every type here mirrors a `#[tauri::command]` in `src-tauri/src/commands.rs`. Keeping them in one
// module means a change to the Rust shape breaks the TypeScript build rather than producing a runtime
// `undefined`, which is the only kind of failure this boundary can actually have.
//
// One thing the types deliberately encode: `SaveView.saved` is false for a conflict. A caller that
// checks `saved` cannot mistake "refused to write" for "saved", which is the worst possible failure
// for an editor.

import { invoke } from '@tauri-apps/api/core';

/** The failing shape every command returns. */
export interface CommandError {
  code: string;
  message: string;
  /** True when the user can resolve it, so the UI offers a fix rather than printing a message. */
  actionable: boolean;
  hint: string | null;
}

export type ContentKind = 'article' | 'page' | 'knowledge';

export type WipState = 'draft' | 'published_wip' | 'finished' | 'finished_with_history';

export type WipTransition = 'save_as_draft' | 'publish_wip' | 'unpublish_wip' | 'finish';

/** The observed state of a file: a content hash and its size. */
export interface FileRevision {
  hash: string;
  size: number;
}

export interface AppInfo {
  name: string;
  version: string;
  site_template_version: string;
}

export interface ProjectView {
  path: string;
  name: string;
  opened_at: string;
  is_veloct_project: boolean;
  has_git: boolean;
}

export interface RibbonEntry {
  kind: ContentKind;
  id: string;
  slug: string;
  title: string;
  summary: string | null;
  wip_state: WipState;
  updated_at: string;
  category: string | null;
  tags: string[];
  is_buildable: boolean;
  route: string;
}

export interface DocumentView {
  kind: ContentKind;
  id: string;
  slug: string;
  title: string;
  summary: string | null;
  wip: boolean;
  wip_publish: boolean;
  wip_ever_published: boolean;
  wip_state: WipState;
  created_at: string;
  updated_at: string;
  /** The whole file, so the editor round-trips unknown frontmatter keys and body formatting. */
  source: string;
  body: string;
  /** The token the next save must present. */
  revision: FileRevision;
  is_buildable: boolean;
  route: string;
  diagnostics: string[];
  matches_disk: boolean;
}

export interface IndexView {
  entries: RibbonEntry[];
  issues: string[];
  is_buildable: boolean;
}

export type SaveStatus = 'saved' | 'unchanged' | 'conflict';

export type ConflictKind =
  | 'external_modification'
  | 'slug_taken'
  | 'identity_changed'
  | 'unparsable';

export interface SaveView {
  status: SaveStatus;
  /** False for a conflict, so a caller cannot mistake a refusal for a save. */
  saved: boolean;
  revision: FileRevision | null;
  source: string | null;
  conflict_kind: ConflictKind | null;
  conflict_message: string | null;
  /** The text currently on disk, so the editor can offer a comparison instead of losing work. */
  disk_source: string | null;
  diagnostics: string[];
}

export type PreviewState =
  | { state: 'stopped' }
  | { state: 'starting'; port: number; pid: number }
  | { state: 'running'; port: number; pid: number; restarts: number }
  | { state: 'crashed'; port: number; reason: string; restarts: number; will_retry: boolean }
  | { state: 'exited'; restarts: number };

export interface PreviewView {
  site: PreviewState;
  works: PreviewState;
  site_url: string | null;
  works_url: string | null;
}

/**
 * Whether an error came from a command or from the transport.
 *
 * A rejected `invoke` with an unrecognised value would otherwise be printed as `[object Object]`, and
 * an error nobody can read is an error nobody can fix.
 */
export function asCommandError(error: unknown): CommandError {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    'message' in error
  ) {
    const candidate = error as Partial<CommandError>;
    return {
      code: String(candidate.code),
      message: String(candidate.message),
      actionable: Boolean(candidate.actionable),
      hint: candidate.hint ?? null,
    };
  }
  return {
    code: 'UNKNOWN',
    message: error instanceof Error ? error.message : String(error),
    actionable: false,
    hint: null,
  };
}

/** One line an error can be shown as. */
export function describeError(error: unknown): string {
  const command = asCommandError(error);
  return command.hint ? `${command.message} (${command.hint})` : command.message;
}

export interface LogEntryView {
  timestamp: string;
  level: 'info' | 'warning' | 'error';
  category: string;
  /** The category's label, so the page keeps no table of its own in step with the Rust side. */
  category_label: string;
  message: string;
  fields: Record<string, string>;
}

export interface LogPage {
  entries: LogEntryView[];
  total: number;
  info: number;
  warning: number;
  error: number;
  /** Every category present, so the filter is built from what actually happened. */
  categories: string[];
}

// ---------------------------------------------------------------------------------------------
// The AI subsystem and the page creator (handoff.md sections 8 and 11).
// ---------------------------------------------------------------------------------------------

/** An image the model asked for but did not generate (section 8.1). */
export interface ImagePlaceholder {
  description: string;
  /** The 1-based body line, so the dialog can point at it. */
  line: number;
}

/**
 * A generated page, parsed by the project's own parser.
 *
 * Nothing has been written: this is the thing being previewed between §8.1's "the AI generates the page"
 * and "the user confirms".
 */
export interface PageDraft {
  title: string;
  slug: string;
  summary: string | null;
  tags: string[];
  category: string | null;
  kind: 'page' | 'article' | 'knowledge';
  body: string;
  /** The whole document with the frontmatter VELOCT assigned. This is what a confirm writes. */
  source: string;
  placeholders: ImagePlaceholder[];
  /** Parser diagnostics, so a structural problem is seen BEFORE the page exists. */
  diagnostics: string[];
  provider_id: string | null;
  model_id: string | null;
  used_fallback: boolean;
}

export interface PageDraftView {
  draft: PageDraft;
  prompt_feature: string;
}

export interface AiProviderView {
  id: string;
  name: string;
  protocol: string;
  base_url: string;
  /** The HANDLE, never a credential (section 12.2). */
  credential_ref: string | null;
  /** Whether one is stored. The value is never sent to the frontend. */
  has_credential: boolean;
  enabled: boolean;
  usable: boolean;
}

export interface AiModelView {
  provider_id: string;
  model_id: string;
  label: string;
  context_window: number | null;
  /**
   * `fetched` or `manual` (section 11.2).
   *
   * Shown because the difference is a promise: a fetched model can disappear when the provider stops
   * offering it, and a hand-added one cannot.
   */
  source: string;
}

/** What a model refresh did (section 11.2). */
export interface ModelRefreshView {
  provider_id: string;
  /** How many models the provider reported. */
  fetched: number;
  /** How many hand-added models the refresh kept. */
  kept_manual: number;
  models: AiModelView[];
  message: string;
}

/** A provider connection test (section 11.5). Never carries the credential. */
export interface ConnectionTestView {
  ok: boolean;
  models: number;
  message: string;
}

export interface AiFeatureView {
  id: string;
  label: string;
  provider_id: string | null;
  model_id: string | null;
  configured: boolean;
}

export interface AiSettingsView {
  providers: AiProviderView[];
  models: AiModelView[];
  features: AiFeatureView[];
  fallback_provider_id: string | null;
  fallback_model_id: string | null;
  page_create_ready: boolean;
  /** Why it is not ready, when it is not, so the dialog can say what to do. */
  blocked_reason: string | null;
}

/** What a publish produced (handoff.md sections 14 and 19.3). */
export interface PublishView {
  release_id: string;
  /** Whether the release is anchored by a commit and a tag. */
  anchored: boolean;
  /** Why it is not anchored, when it is not. */
  anchor_error: string | null;
  commit_sha: string | null;
  tag_name: string | null;
  /** False when nothing changed and the existing head was tagged. */
  committed: boolean;
  changed_files: number;
  routes: number;
  fingerprint: string | null;
  /** The build's output tail, so a failure is explained rather than just reported. */
  build_log: string;
}

/** One release on disk. */
export interface ReleaseView {
  release_id: string;
  created_at: string;
  build_fingerprint: string;
  commit_sha: string | null;
  tag_name: string | null;
  /** Whether the tag still resolves to the commit the publish record names. */
  tag_resolves: boolean;
  routes: number;
  created_from_release_id: string | null;
}

/** One attachment (handoff.md section 13). */
export interface AttachmentView {
  id: string;
  original_name: string;
  content_hash: string;
  size_bytes: number;
  cache_path: string;
  storage_target_id: string;
  storage_namespace: string;
  remote_url: string | null;
  remote_key: string | null;
  /** The three independent axes, kept separate (section 13.1). */
  upload_status: string;
  reference_status: string;
  cache_status: string;
  last_error: string | null;
  created_at: string;
  /** How many documents cite it, so the UI can warn before a removal. */
  reference_count: number;
}

export interface AddAttachmentView {
  attachment: AttachmentView;
  /** Whether the bytes were already present for this target and namespace. */
  outcome: 'added' | 'reused';
  /** The Markdown to insert. Built by the Rust side so there is one place that formats the URL. */
  markdown: string;
}

export interface AttachmentOrphans {
  /** Safe to drop: every condition in section 13.2.4 holds. */
  safe: AttachmentView[];
  /** Needs the user to decide, because a condition does not hold. */
  needs_confirmation: AttachmentView[];
}

/** A path left out of an archive, with the reason (handoff.md section 17.1). */
export interface SkippedPathView {
  path: string;
  reason: string;
}

/** What a backup produced. */
export interface BackupOutcomeView {
  archive_path: string;
  archive_bytes: number;
  archive_sha256: string;
  file_count: number;
  total_bytes: number;
  skipped: SkippedPathView[];
  /** Findings by PATH and KIND. The value is never sent, so it cannot be shown by mistake. */
  secret_findings: string[];
  /** Whether a finding blocks an upload (section 17.3). The local archive is still usable. */
  needs_acknowledgement: boolean;
  history_note: string | null;
  created_at: string;
}

export interface BackupArchiveView {
  path: string;
  file_name: string;
  bytes: number;
  created_at: string;
  file_count: number;
  trigger: string;
  secret_findings: number;
}

/** A restore plan. Nothing has been replaced when this exists (section 17.10). */
export interface RestorePlanView {
  mode: string;
  archive_path: string;
  file_count: number;
  total_bytes: number;
  will_apply: string[];
  will_preserve: SkippedPathView[];
  needs_publish_afterwards: boolean;
  credentials_to_rebind: string[];
  history_note: string | null;
  summary: string;
}

export interface StagedRestoreView {
  plan: RestorePlanView;
  staging_directory: string;
}

export interface RestoreAppliedView {
  mode: string;
  files_written: number;
  total_bytes: number;
  preserved: string[];
  replaced: string[];
  needs_index_rebuild: boolean;
  working_tree_changed: boolean;
  credentials_to_rebind: string[];
  summary: string;
}

/** One configured cloud backup target (handoff.md section 17.4). */
export interface CloudBackupTargetView {
  id: string;
  /** `github` or `s3`. */
  kind: string;
  /** The user-facing label for the kind. */
  label: string;
  /** GitHub: `owner/repo`. S3: the bucket. */
  location: string;
  prefix: string | null;
  retention: number;
  /** Whether the user has confirmed the target is private (section 17.2). */
  private_confirmed: boolean;
  enabled: boolean;
  /** The HANDLE, never a value (section 12). */
  credential_ref: string | null;
  has_credential: boolean;
  /** S3: the access key ID, which is an identifier and not a secret. */
  access_key: string | null;
  /** S3: the endpoint, when one is configured. */
  endpoint: string | null;
  /** S3: the region. */
  region: string | null;
  /** Whether this build has an adapter for the kind. */
  implemented: boolean;
  unimplemented_reason: string | null;
  /** Why a run would refuse this target, when it would (section 17.2). */
  refusal_reason: string | null;
}

/** One target's result in a cloud run (section 17.7). */
export interface TargetUploadOutcome {
  target_id: string;
  succeeded: boolean;
  /** Deliberately not attempted — not selected, disabled, or refused. Nothing to retry. */
  skipped: boolean;
  error: string | null;
  /** How many archives the target holds after its retention ran (section 17.6). */
  kept: number;
}

export interface CloudBackupRunView {
  archive_path: string;
  archive_sha256: string;
  archive_bytes: number;
  created_at: string;
  trigger: string;
  targets: TargetUploadOutcome[];
  all_succeeded: boolean;
  /** No target stored the backup, whether they failed or none was attempted. */
  nothing_stored: boolean;
  summary: string;
  /** Whether the run could be recorded in the index, and why not when it could not. */
  index_note: string | null;
}

export interface CloudBackupResultView {
  /** The local archive, which exists whether or not any upload happened (section 17.3). */
  backup: BackupOutcomeView;
  /** Absent when a secret finding blocked the upload. */
  run: CloudBackupRunView | null;
  blocked_by_finding: boolean;
  blocked_reason: string | null;
}

/** One configured deployment target (handoff.md sections 10 and 14). */
export interface TargetView {
  id: string;
  kind: string;
  kind_label: string;
  enabled: boolean;
  /** Whether this kind needs the user to upload by hand (ADR-0012, section 14.11). */
  manual: boolean;
  project: string | null;
  works_project: string | null;
  works_origin: string | null;
  /** The HANDLE, never a value (section 12). */
  credential_ref: string | null;
  has_credential: boolean;
}

export interface TargetListView {
  enabled: boolean;
  targets: TargetView[];
}

/** One target's outcome. `partial` means one sub-target succeeded and the other did not. */
export interface TargetResultView {
  target_id: string;
  kind: string;
  status: string;
  main_status: string;
  main_detail: string | null;
  works_status: string | null;
  works_detail: string | null;
  needs_attention: boolean;
  works_origin: string | null;
}

export interface DeployOutcomeView {
  release_id: string;
  targets: TargetResultView[];
  all_succeeded: boolean;
  needing_attention: string[];
  pushed_files: number;
  started_at: string;
  finished_at: string | null;
  /** Section 15.2: the SEO push is a separate task with its own state. */
  seo_state: string;
}

/** One configured storage target (handoff.md sections 13.10-13.13). */
export interface StorageTargetView {
  id: string;
  name: string;
  kind: string;
  /** Whether this build has an adapter for the kind. */
  implemented: boolean;
  /** Why not, when it does not, so the window can say what to use instead. */
  unimplemented_reason: string | null;
  path_prefix: string | null;
  public_url_prefix: string;
  repository: string | null;
  branch: string | null;
  /** S3: Endpoint, Region and Bucket, so a form can show what it saved (section 13.11). Blank endpoint means AWS. */
  endpoint: string | null;
  region: string | null;
  bucket: string | null;
  /** S3: the access key ID, which is an identifier and not a secret. */
  access_key: string | null;
  /**
   * Lsky Pro: the instance address (section 13.12).
   *
   * Present for the same reason the S3 fields above are: a form that cannot see what it saved would submit
   * nothing for this field on the next save and silently break a working target.
   */
  instance: string | null;
  /** The HANDLE, never a value (section 12). */
  credential_ref: string | null;
  has_credential: boolean;
}

/**
 * One storage KIND's adapter status (handoff.md sections 13.10-13.13).
 *
 * A fact about this build rather than about a project: no target id, no credential handle. This is what lets
 * the window report PROCESSING.md §5.3/§5.4's gap before any target of that kind has been saved.
 */
export interface StorageKindView {
  /** The stored spelling, exactly what `saveStorageTarget` accepts back as `kind`. */
  kind: string;
  /** Whether this build has an adapter for the kind. */
  implemented: boolean;
  /** Why not, in the build's own words, when it does not. Never softened. */
  unimplemented_reason: string | null;
}

/** What an upload produced (section 13.3). */
export interface UploadView {
  attachment: AttachmentView;
  /** Whether the remote object already existed for this dedup key (section 13.4). */
  reused: boolean;
  remote_url: string;
  remote_key: string;
}

/**
 * What an explicit remote cleanup produced (section 13.8).
 *
 * The attachment is still in the pool: section 13.8 removes the REMOTE object, not the user's record of the
 * file, so the cached bytes and the reference records stay and the attachment may be uploaded again.
 */
export interface RemoteDeleteView {
  attachment: AttachmentView;
  /** The target the object was ACTUALLY on, read from its record rather than from the caller's claim. */
  storage_target_id: string;
  key: string;
  public_url: string;
}

export interface UploadEligibility {
  can_upload: boolean;
  upload_status: string;
  reason: string | null;
}

/** One recorded remote object. */
export interface ObjectView {
  dedup_key: string;
  key: string;
  url: string;
  bytes: number;
  /** The target the object is ACTUALLY on, so `deleteRemoteObject` can name it. */
  storage_target_id: string;
}

/** The automatic backup rule's state (handoff.md section 17.5). */
export interface AutoBackupStateView {  tracker: {
    last_change_seconds: number;
    changed_since_backup: boolean;
    idle_seconds: number;
  };
  /** `no_change`, `waiting` or `due`. */
  decision: { decision: string; remaining_seconds?: number };
  /** A phrase for the UI rather than the caller assembling one. */
  description: string;
  idle_seconds: number;
  /** Section 17.6: three per cloud target by default. */
  cloud_retention: number;
  /** Whether a secret finding is waiting for acknowledgement (section 17.3). */
  needs_acknowledgement: boolean;
}

/** How a work's files arrived (handoff.md section 10.2). */
export interface WorkSourceView {
  kind: 'zip' | 'folder';
  original_name: string;
}

/**
 * A work's metadata, verbatim from `work.json` (handoff.md section 10.1).
 *
 * This is the truth source rather than a projection of one, so `updateWork` sends it back whole:
 * a field the window does not understand is still preserved because the window round-trips the
 * object it read.
 */
export interface WorkMetadataView {
  id: string;
  slug: string;
  title: string;
  author: string;
  summary: string;
  completed_at: string | null;
  imported_at: string;
  source: WorkSourceView;
  entry: string;
  tags: string[];
  cover: string | null;
  screenshot: string | null;
  size_bytes: number;
  notes: string;
}

/** One work, as the window sees it. */
export interface WorkView {
  metadata: WorkMetadataView;
  /** Whether `site/` exists at all. */
  has_body: boolean;
  file_count: number;
  /** Whether the build will emit anything for this work. */
  is_servable: boolean;
  /** Whether a poster image exists on disk right now. */
  has_screenshot: boolean;
  /** The poster route the build emits, or null when there is no poster. */
  screenshot_url: string | null;
  /** The poster exists but `work.json` names a different path, so the two disagree. */
  screenshot_drift: boolean;
}

export interface WorksListView {
  works: WorkView[];
  /** How many works the build will actually emit. */
  servable: number;
}

/** Section 10.1's size warning, which never changes the truth source. */
export interface WorkImportWarningView {
  total_bytes: number;
  threshold: number;
  message: string;
}

/** What an import WILL do, produced without writing anything. */
export interface WorkImportPlanView {
  title: string;
  slug: string;
  id: string;
  entry: string;
  file_count: number;
  total_bytes: number;
  metadata: WorkMetadataView;
  skipped_directories: string[];
  stripped_prefix: string | null;
  /** Images the source carries that Veloct will NOT use as the poster. */
  unused_images: string[];
  warning: WorkImportWarningView | null;
  /** Why the import cannot proceed, when it cannot. */
  blocked_reason: string | null;
}

export interface WorkImportOutcomeView {
  work: WorkView;
  files_written: number;
  total_bytes: number;
  directory: string;
  slug: string;
  warning: WorkImportWarningView | null;
}

/** How a chosen file's source reached us, decided by what the path IS. */
export interface WorkImportSourceView {
  kind: 'zip' | 'folder';
  path: string;
  name: string;
}

// ---------------------------------------------------------------------------------------------
// The network image browser (handoff.md section 13.14).
//
// The shape mirrors `veloct-attach::network`. `ImageResultView` round-trips: the window sends a
// chosen result BACK to `chooseNetworkImage`, which is why every field it carries is declared here.
// ---------------------------------------------------------------------------------------------

/** A provider of free network images. */
export type ImageSourceName = 'unsplash' | 'pexels' | 'pixabay';

/** One source as the window sees it (`network_image_sources`). */
export interface NetworkImageSourceView {
  /** The stored spelling, exactly what the commands take back. */
  source: ImageSourceName;
  label: string;
  /** Whether this build can search it. */
  implemented: boolean;
  /** Why not, in the provider's own terms, when it cannot. */
  unimplemented_reason: string | null;
  needs_key: boolean;
  /** Whether a key is STORED. Never the value (section 12.1). */
  has_key: boolean;
  /** What the provider requires of a page that shows its image. */
  attribution_rule: string;
  /** The sentence to show before a search, including what a network image costs the user. */
  notice: string;
  /** `required`, `permitted` or `forbidden`: the provider's stance on embedding its URLs. */
  hotlink: string;
  /**
   * Whether leaving the credit line OUT of the document is the user's choice.
   *
   * Derived in Rust from the provider's own terms, so the window never decides it. False for Unsplash,
   * whose API Guidelines make crediting a condition of use.
   */
  attribution_optional: boolean;
  default_per_page: number;
  max_per_page: number;
}

/** One result from a search. Round-trips back through `chooseNetworkImage`. */
export interface ImageResultView {
  source: ImageSourceName;
  source_id: string;
  url: string;
  thumbnail_url: string;
  width: number;
  height: number;
  author: string;
  licence: string | null;
  source_page: string | null;
  /** The author's own profile page, which Unsplash's attribution rule links to. */
  author_url: string | null;
  /** The provider's OWN description, which is the alt fallback before a generated line. */
  alt: string | null;
  /** Unsplash's download-report endpoint. `null` for a source with no such obligation. */
  download_location: string | null;
}

/** A chosen network image, ready to be written. */
export interface NetworkImageChoiceView {
  source: ImageSourceName;
  source_id: string;
  url: string;
  alt: string | null;
  /** The credit line, when one applies. */
  attribution: string | null;
}

/** What a search returned. */
export interface ImageSearchView {
  source: ImageSourceName;
  label: string;
  query: string;
  results: ImageResultView[];
  has_more: boolean;
  /** How many results were dropped as unusable, so a short list is explained. */
  dropped: number;
  attribution_rule: string;
  /** Whether leaving the credit line OUT of the document is the user's choice. */
  attribution_optional: boolean;
}

/** What a choice produced. */
export interface NetworkImageChoiceResultView {
  choice: NetworkImageChoiceView;
  /** The Markdown to insert, credit line included when there is one. */
  markdown: string;
  attribution: string | null;
  /**
   * `true`/`false` for a source that REQUIRES a download report, `null` where the provider states no
   * such obligation. Unsplash requires one; Pexels does not.
   */
  download_reported: boolean | null;
  /** Why the report did not happen, when it did not. The image is still usable. */
  download_note: string | null;
}

export const api = {
  appInfo: () => invoke<AppInfo>('app_info'),

  nowRfc3339: () => invoke<string>('now_rfc3339'),

  openProject: (path: string) => invoke<ProjectView>('open_project', { path }),

  createProject: (options: {
    path: string;
    name: string;
    defaultAuthor?: string;
    siteTemplate: string;
    siteTemplateVersion: string;
  }) =>
    invoke<ProjectView>('create_project', {
      path: options.path,
      name: options.name,
      defaultAuthor: options.defaultAuthor ?? null,
      siteTemplate: options.siteTemplate,
      siteTemplateVersion: options.siteTemplateVersion,
    }),

  currentProject: () => invoke<ProjectView | null>('current_project'),

  listDocuments: () => invoke<RibbonEntry[]>('list_documents'),

  contentIndex: () => invoke<IndexView>('content_index'),

  readDocument: (kind: ContentKind, id: string) =>
    invoke<DocumentView>('read_document', { kind, id }),

  createDocument: (options: {
    kind: ContentKind;
    title: string;
    slug?: string;
    body?: string;
    startAsWip?: boolean;
  }) =>
    invoke<{ document: DocumentView }>('create_document', {
      kind: options.kind,
      title: options.title,
      slug: options.slug ?? null,
      body: options.body ?? null,
      startAsWip: options.startAsWip ?? null,
    }),

  /**
   * Save a document.
   *
   * The baseline is the revision the editor READ. Passing a fresh revision here would defeat the
   * external-modification check entirely, which is why it is a required parameter rather than
   * something this layer looks up.
   */
  saveDocument: (options: {
    kind: ContentKind;
    id: string;
    baseline: FileRevision;
    source: string;
  }) =>
    invoke<SaveView>('save_document', {
      kind: options.kind,
      id: options.id,
      baseline: options.baseline,
      source: options.source,
    }),

  transitionWip: (options: {
    id: string;
    baseline: FileRevision;
    source: string;
    transition: WipTransition;
  }) => invoke<SaveView>('transition_wip', { ...options }),

  renameSlug: (options: {
    kind: ContentKind;
    id: string;
    baseline: FileRevision;
    source: string;
    newSlug: string;
  }) => invoke<SaveView>('rename_slug', { ...options }),

  repairReference: (options: {
    documentId: string;
    fromContentId: string;
    fromParagraphId: string;
    toContentId: string;
    toParagraphId: string;
    baseline: FileRevision;
    source: string;
  }) => invoke<SaveView>('repair_reference', { ...options }),

  assignParagraphIds: (options: {
    kind: ContentKind;
    id: string;
    baseline: FileRevision;
    source: string;
  }) => invoke<SaveView>('assign_paragraph_ids', { ...options }),

  deleteDocument: (kind: ContentKind, id: string) =>
    invoke<void>('delete_document', { kind, id }),

  referencingDocuments: (contentId: string) =>
    invoke<RibbonEntry[]>('referencing_documents', { contentId }),

  startPreview: (role?: 'site' | 'works') =>
    invoke<PreviewView>('start_preview', { role: role ?? null }),

  stopPreview: () => invoke<PreviewView>('stop_preview'),

  previewStatus: () => invoke<PreviewView>('preview_status'),

  /**
   * Read the project's log.
   *
   * The filtering happens on the Rust side, so the page does not receive the whole log to show part of
   * it. The entries arriving are already redacted either way (handoff.md section 18.1).
   */
  /** The AI configuration. Never carries a credential value (section 12.1). */
  aiSettings: () => invoke<AiSettingsView>('ai_settings'),
  /**
   * Add or update a provider.
   *
   * The credential is a separate parameter because it goes to the OS store and the row keeps only a handle.
   * A single object with the secret inside would invite persisting it (section 12.2).
   */
  saveAiProvider: (options: {
    id: string;
    name: string;
    protocol: string;
    baseUrl: string;
    enabled: boolean;
    /** Omitted or empty means "leave the stored credential alone". */
    credential?: string;
  }) =>
    invoke<void>('save_ai_provider', {
      id: options.id,
      name: options.name,
      protocol: options.protocol,
      baseUrl: options.baseUrl,
      enabled: options.enabled,
      credential: options.credential ?? null,
    }),

  deleteAiProvider: (id: string) => invoke<void>('delete_ai_provider', { id }),

  /**
   * Fetch a provider's model list and store it (section 11.2).
   *
   * The refresh REPLACES the models the provider reports and keeps the ones added by hand, so a model the
   * provider stopped offering cannot stay selectable. `kept_manual` says how many survived.
   */
  refreshAiModels: (providerId: string) =>
    invoke<ModelRefreshView>('refresh_ai_models', { providerId }),

  /** Add a model by hand (section 11.2). A later refresh keeps it. */
  addAiModel: (providerId: string, modelId: string, displayName?: string | null) =>
    invoke<AiModelView[]>('add_ai_model', {
      providerId,
      modelId,
      displayName: displayName ?? null,
    }),

  /** Remove one stored model, so a hand-added typo is not permanent. */
  deleteAiModel: (providerId: string, modelId: string) =>
    invoke<void>('delete_ai_model', { providerId, modelId }),

  /**
   * Test a provider's connection with its REAL configuration (section 11.5).
   *
   * A test with a placeholder credential would pass while the real call failed, so this exercises the
   * provider's own model-list endpoint and reports the provider's own reason.
   */
  testAiProvider: (providerId: string) =>
    invoke<ConnectionTestView>('test_ai_provider', { providerId }),

  /** Map a feature to a model, or clear it by passing nulls (section 11.3). */
  setAiFeature: (options: {
    featureId: string;
    label?: string;
    providerId: string | null;
    modelId: string | null;
  }) =>
    invoke<void>('set_ai_feature', {
      featureId: options.featureId,
      label: options.label ?? null,
      providerId: options.providerId,
      modelId: options.modelId,
    }),

  /** Set the single global fallback (section 11.4). */
  setAiFallback: (providerId: string | null, modelId: string | null) =>
    invoke<void>('set_ai_fallback', { providerId, modelId }),

  /**
   * Generate a page draft.
   *
   * NOTHING is written. Section 8.1 puts the preview before the creation, and this call is the generation
   * step rather than the create step.
   */
  generatePage: (options: {
    title: string;
    brief: string;
    kind?: 'page' | 'article' | 'knowledge';
    tags?: string[];
    category?: string;
    withImagePlaceholders?: boolean;
  }) =>
    invoke<PageDraftView>('generate_page', {
      title: options.title,
      brief: options.brief,
      kind: options.kind ?? null,
      tags: options.tags ?? null,
      category: options.category ?? null,
      withImagePlaceholders: options.withImagePlaceholders ?? null,
    }),

  /**
   * Create the page from a confirmed draft.
   *
   * This IS the confirmation. The Rust side re-parses and refuses a document with a structural error, so
   * section 8.1's "without confirmation, a page must not be created" holds even if a caller skips the view.
   */
  createPageFromDraft: (source: string, kind: ContentKind) =>
    invoke<DocumentView>('create_page_from_draft', { source, kind }),

  /**
   * Publish the project.
   *
   * `handoff.md` section 19.3 from the window: check what changed, commit only that, build once, tag the
   * commit. The build runs the site's own Astro through the bundled Node.
   */
  publishProject: (createdFromReleaseId?: string) =>
    invoke<PublishView>('publish_project', {
      createdFromReleaseId: createdFromReleaseId ?? null,
    }),

  /** Add a file to the attachment pool. Writes no Markdown; the caller applies it (section 13.3). */
  addAttachment: (options: {
    sourcePath: string;
    storageTargetId: string;
    namespace: string;
  }) =>
    invoke<AddAttachmentView>('add_attachment', {
      sourcePath: options.sourcePath,
      storageTargetId: options.storageTargetId,
      namespace: options.namespace,
    }),

  listAttachments: () => invoke<AttachmentView[]>('list_attachments'),

  attachmentOrphans: () => invoke<AttachmentOrphans>('attachment_orphans'),

  removeAttachment: (id: string) => invoke<AttachmentView>('remove_attachment', { id }),

  attachmentMarkdown: (id: string) => invoke<string>('attachment_markdown', { id }),

  referenceAttachment: (options: {
    attachmentId: string;
    contentId: string;
    paragraphId?: string;
  }) =>
    invoke<void>('reference_attachment', {
      attachmentId: options.attachmentId,
      contentId: options.contentId,
      paragraphId: options.paragraphId ?? null,
    }),

  forgetAttachmentReferences: (contentId: string) =>
    invoke<number>('forget_attachment_references', { contentId }),

  syncAttachmentIndex: () => invoke<number>('sync_attachment_index'),

  attachmentCacheDirectory: () => invoke<string>('attachment_cache_directory'),

  deployTargets: () => invoke<TargetListView>('deploy_targets'),

  /**
   * Add or update a target.
   *
   * The credential is a separate parameter because it goes to the OS store and the row keeps only a handle
   * (section 12.2).
   */
  saveDeployTarget: (options: {
    id: string;
    kind: string;
    enabled: boolean;
    /** `undefined` is allowed explicitly, because `exactOptionalPropertyTypes` is on. */
    project?: string | undefined;
    worksProject?: string | undefined;
    worksOrigin?: string | undefined;
    /** Omitted or empty means "leave the stored credential alone". */
    credential?: string | undefined;
  }) =>
    invoke<void>('save_deploy_target', {
      // Tauri matches the command's parameter name, so the fields travel as one `request` object rather than
      // as eight positional arguments.
      request: {
        id: options.id,
        kind: options.kind,
        enabled: options.enabled,
        project: options.project ?? null,
        works_project: options.worksProject ?? null,
        works_origin: options.worksOrigin ?? null,
        credential: options.credential ?? null,
      },
    }),

  deleteDeployTarget: (id: string) => invoke<void>('delete_deploy_target', { id }),

  setDeployEnabled: (enabled: boolean) => invoke<void>('set_deploy_enabled', { enabled }),

  /**
   * Deploy the CURRENT build.
   *
   * A deploy does not build: section 14.13's re-send pushes an existing dist so the fingerprint that
   * authorised it still describes the bytes. Publishing first is the caller's job.
   */
  deployNow: (releaseId?: string) =>
    invoke<DeployOutcomeView>('deploy_now', { releaseId: releaseId ?? null }),

  /** The sub-targets a deployment can re-send. */
  deployResendable: (releaseId?: string) =>
    invoke<string[]>('deploy_resendable', { releaseId: releaseId ?? null }),

  /** Record that the content changed, which RESTARTS the idle timer (section 17.5). */
  noteContentChange: () => invoke<AutoBackupStateView>('note_content_change'),

  /** Whether an automatic backup is due, and how long is left when it is not. */
  autoBackupState: () => invoke<AutoBackupStateView>('auto_backup_state'),

  /**
   * Run the automatic backup if the rule says it is due.
   *
   * `null` when it is not due, which is the common case and not a failure.
   */
  runAutoBackupIfDue: () => invoke<{
    archive_path: string;
    archive_sha256: string;
    targets: Array<{ target_id: string; succeeded: boolean; error: string | null; kept: number }>;
    needs_acknowledgement: boolean;
  } | null>('run_auto_backup_if_due'),

  /** The configured storage targets. Never carries a credential value (section 12.1). */
  storageTargets: () => invoke<StorageTargetView[]>('storage_targets'),

  /**
   * Every storage kind this build knows, with its adapter status (sections 13.10-13.13).
   *
   * Reads no project state deliberately: whether a KIND has an adapter is a fact about the build, so this
   * answers before a target of that kind exists — which is where PROCESSING.md §5.3's gap has to be
   * visible. `storageTargets` can only report a saved target, and a kind with no adapter is exactly the kind a
   * user cannot usefully save (an SFTP target needs a host).
   */
  storageKinds: () => invoke<StorageKindView[]>('storage_kinds'),

  saveStorageTarget: (options: {
    id: string;
    name: string;
    kind: string;
    pathPrefix?: string | undefined;
    publicUrlPrefix?: string | undefined;
    repository?: string | undefined;
    branch?: string | undefined;
    /** S3: Endpoint, Region and Bucket (section 13.11). Blank endpoint means AWS. */
    endpoint?: string | undefined;
    region?: string | undefined;
    bucket?: string | undefined;
    /** S3: the access key ID, which is an identifier rather than a secret. */
    accessKey?: string | undefined;
    /** Lsky Pro: the instance address — its host, or the full API address (section 13.12). */
    instance?: string | undefined;
    /** Written to the OS store; omitted or empty leaves the stored one alone. */
    credential?: string | undefined;
  }) =>
    invoke<void>('save_storage_target', {
      request: {
        id: options.id,
        name: options.name,
        kind: options.kind,
        path_prefix: options.pathPrefix ?? null,
        public_url_prefix: options.publicUrlPrefix ?? null,
        repository: options.repository ?? null,
        branch: options.branch ?? null,
        endpoint: options.endpoint ?? null,
        region: options.region ?? null,
        bucket: options.bucket ?? null,
        access_key: options.accessKey ?? null,
        instance: options.instance ?? null,
        credential: options.credential ?? null,
      },
    }),

  /**
   * Remove a storage target.
   *
   * This ALSO removes that target's stored credential from the OS store, so the window's confirmation has to
   * say so: recreating the target later means entering the credential again.
   *
   * It does NOT remove anything already uploaded to the target: section 13.8 forbids deleting a remote file as
   * a side effect. Use `deleteRemoteObject` for that, one object at a time.
   */
  deleteStorageTarget: (id: string) => invoke<void>('delete_storage_target', { id }),

  /** Whether an attachment may be uploaded, so the button can be disabled WITH a reason. */
  uploadEligibility: (id: string) => invoke<UploadEligibility>('upload_eligibility', { id }),

  /**
   * Upload an attachment to a target.
   *
   * Section 13.3's second step: the bytes reach the target and the object record is written, and the Markdown is
   * still untouched. The caller applies the URL afterwards.
   */
  uploadAttachment: (id: string, storageTargetId: string) =>
    invoke<UploadView>('upload_attachment', { id, storageTargetId }),

  /**
   * Remove ONE remote object, on the target that actually holds it (section 13.8's explicit cleanup).
   *
   * `removeAttachment` deletes the record and the cached bytes and deliberately leaves the remote file alone;
   * this is the only path that asks for the remote object to go. It is refused — removing nothing — when the
   * target cannot delete remotely (SFTP and Lsky Pro report their gap) or when `storageTargetId` is not the
   * target the object's record names, so pass the target the object is actually on: the attachment's own
   * `storage_target_id` for the ordinary flow (add and upload to the same target), or the matching
   * `storage_target_id` from `attachmentObjects()` when it is not.
   *
   * The attachment, its cached bytes and its reference records are left alone, and it becomes uploadable again.
   */
  deleteRemoteObject: (id: string, storageTargetId: string) =>
    invoke<RemoteDeleteView>('delete_remote_object', { id, storageTargetId }),

  /** The recorded remote objects. */
  attachmentObjects: () => invoke<ObjectView[]>('attachment_objects'),

  /** Take a manual backup into the project's own backup directory (section 17.8). */
  manualBackup: () => invoke<BackupOutcomeView>('manual_backup'),

  /** Export a backup to a directory the user chose (section 17.9). */
  exportBackup: (destinationDirectory: string) =>
    invoke<BackupOutcomeView>('export_backup', { destinationDirectory }),

  listBackups: (directory?: string) =>
    invoke<BackupArchiveView[]>('list_backups', { directory: directory ?? null }),

  /** Apply the retention policy (section 17.6). */
  pruneBackups: (options?: { directory?: string; keep?: number }) =>
    invoke<number>('prune_backups', {
      directory: options?.directory ?? null,
      keep: options?.keep ?? null,
    }),

  /**
   * Stage a restore and read the plan. NOTHING is replaced (section 17.10).
   *
   * Deliberately a separate call from `confirmRestore`, because a single restore call would make the
   * confirmation a formality.
   */
  stageRestore: (archivePath: string, mode: 'full' | 'content_only') =>
    invoke<StagedRestoreView>('stage_restore', { archivePath, mode }),

  /** Apply a staged restore. The Rust side refuses an unconfirmed apply. */
  confirmRestore: (archivePath: string, mode: string) =>
    invoke<RestoreAppliedView>('confirm_restore', {
      archivePath,
      mode,
      confirmed: true,
    }),

  backupDirectory: () => invoke<string>('backup_directory'),

  /** The configured cloud backup targets (section 17.4). */
  cloudBackupTargets: () => invoke<CloudBackupTargetView[]>('cloud_backup_targets'),

  /** Add or update a cloud backup target. The credential value goes to the OS store, never to SQLite. */
  saveCloudBackupTarget: (request: {
    id: string;
    kind: string;
    location: string;
    prefix?: string | null;
    retention?: number | null;
    privateConfirmed: boolean;
    enabled?: boolean | null;
    /** GitHub: the token. S3: the SECRET access key. Never persisted outside the OS store. */
    credential?: string | null;
    /** S3: the access key ID, which is configuration rather than a secret. */
    accessKey?: string | null;
    /** S3: the endpoint. Left empty, the AWS endpoint for the region is used. */
    endpoint?: string | null;
    /** S3: the region. */
    region?: string | null;
  }) => invoke<void>('save_cloud_backup_target', { request }),

  deleteCloudBackupTarget: (id: string) => invoke<void>('delete_cloud_backup_target', { id }),

  /**
   * Prove a cloud target is reachable, and that it satisfies section 17.2.
   *
   * A public GitHub repository is reachable and still not a usable backup target, so this reports the
   * privacy fact rather than reachability alone.
   */
  testCloudBackupTarget: (id: string) => invoke<string>('test_cloud_backup_target', { id }),

  /**
   * Take a backup AND send it to the cloud targets (sections 17.7, 17.8).
   *
   * `acknowledgedFindings` is the user saying they have seen the scan's finding and still want THIS
   * archive sent. Without it a finding skips the upload and the local archive is kept, which is what
   * section 17.3 requires.
   */
  backupToCloud: (options?: { targetIds?: string[] | null; acknowledgedFindings?: boolean }) =>
    invoke<CloudBackupResultView>('backup_to_cloud', {
      targetIds: options?.targetIds ?? null,
      acknowledgedFindings: options?.acknowledgedFindings ?? false,
    }),

  /** Re-send an existing archive to the cloud targets (section 17.7's retry path). */
  resendBackupToCloud: (options: { archivePath: string; targetIds?: string[] | null }) =>
    invoke<CloudBackupRunView>('resend_backup_to_cloud', {
      archivePath: options.archivePath,
      targetIds: options.targetIds ?? null,
    }),

  /** Every release the project has, newest first. */
  listReleases: () => invoke<ReleaseView[]>('list_releases'),

  readLog: (options?: {
    limit?: number;
    level?: 'info' | 'warning' | 'error';
    category?: string;
    search?: string;
  }) =>
    invoke<LogPage>('read_log', {
      limit: options?.limit ?? null,
      level: options?.level ?? null,
      category: options?.category ?? null,
      search: options?.search ?? null,
    }),

  // -------------------------------------------------------------------------------------------
  // Works (handoff.md section 10). A work's truth source is `works/<stable-id>/work.json` plus the
  // sibling `site/`, so every call here reads or writes FILES and none of them touches SQLite.
  // -------------------------------------------------------------------------------------------

  /** Every work, newest completion first. */
  listWorks: () => invoke<WorksListView>('list_works'),

  /** How a chosen path should be imported, without importing it. */
  describeWorkSource: (path: string) => invoke<WorkImportSourceView>('describe_work_source', { path }),

  /**
   * What an import would do, writing nothing (section 10.2).
   *
   * Shown before anything happens, so the entry file, the file count, the size warning and the
   * skipped directories are the user's decision rather than a surprise after the fact.
   */
  planWorkImport: (options: {
    sourcePath: string;
    sourceKind: 'zip' | 'folder';
    title: string;
    author: string;
  }) =>
    invoke<WorkImportPlanView>('plan_work_import', {
      sourcePath: options.sourcePath,
      sourceKind: options.sourceKind,
      title: options.title,
      author: options.author,
    }),

  /** Import a work from a ZIP archive or a folder (section 10.2). */
  importWork: (options: {
    sourcePath: string;
    sourceKind: 'zip' | 'folder';
    title: string;
    author: string;
  }) =>
    invoke<WorkImportOutcomeView>('import_work', {
      sourcePath: options.sourcePath,
      sourceKind: options.sourceKind,
      title: options.title,
      author: options.author,
    }),

  /** Export a work's body as an archive that re-imports identically (section 10.5). */
  exportWork: (id: string, destination: string) =>
    invoke<number>('export_work', { id, destination }),

  /**
   * Replace a work's editable metadata.
   *
   * The stable id and the import provenance are the caller's whatever is sent, because section 3.2
   * makes the id the directory name: an edit that could change it would move the body.
   */
  updateWork: (id: string, metadata: WorkMetadataView) =>
    invoke<WorkView>('update_work', { id, metadata }),

  /** Remove a work and its body. */
  removeWork: (id: string) => invoke<void>('remove_work', { id }),

  /** Store a poster image for a work and point its metadata at it (section 10.1's `preview/`). */
  setWorkScreenshot: (id: string, sourcePath: string) =>
    invoke<WorkView>('set_work_screenshot', { id, sourcePath }),

  /** Where a work's files live, so the window can name the directory rather than guess it. */
  workDirectory: (id: string) => invoke<string>('work_directory', { id }),

  /** The largest poster image the crate will store, so the window can say it BEFORE an attempt. */
  workScreenshotLimit: () => invoke<number>('work_screenshot_limit'),

  // -------------------------------------------------------------------------------------------
  // The network image browser (handoff.md section 13.14). A network image is a URL in the Markdown
  // and nothing else: it never enters the attachment cache pool, is never downloaded, and is never
  // re-uploaded. The only local state is the provider's API key, which lives in the OS credential
  // store and is never returned by any of these calls.
  // -------------------------------------------------------------------------------------------

  /**
   * Every source `handoff.md` section 13.1 names, with its state.
   *
   * Takes no project: the answer is a fact about this BUILD, so the window can report a source that
   * is not offered (and why) before anything is configured.
   */
  networkImageSources: () => invoke<NetworkImageSourceView[]>('network_image_sources'),

  /** Store an API key for a source. The value goes to the OS credential store, never to SQLite. */
  saveImageSourceKey: (source: ImageSourceName, apiKey: string) =>
    invoke<void>('save_image_source_key', { source, apiKey }),

  /** Forget a source's stored key. */
  deleteImageSourceKey: (source: ImageSourceName) =>
    invoke<void>('delete_image_source_key', { source }),

  /** Search a provider for a keyword. */
  searchNetworkImages: (options: {
    source: ImageSourceName;
    query: string;
    page?: number;
    perPage?: number;
  }) =>
    invoke<ImageSearchView>('search_network_images', {
      source: options.source,
      query: options.query,
      page: options.page ?? null,
      perPage: options.perPage ?? null,
    }),

  /**
   * Turn a chosen result into Markdown, reporting the download where the provider requires it.
   *
   * The RESULT is sent back rather than an id, because a provider's id alone cannot reconstruct the
   * image URL, the author's profile link or the download endpoint — and those are what the credit
   * line and the compliance step need.
   */
  chooseNetworkImage: (options: {
    source: ImageSourceName;
    result: ImageResultView;
    alt?: string | undefined;
    includeAttribution: boolean;
  }) =>
    invoke<NetworkImageChoiceResultView>('choose_network_image', {
      source: options.source,
      result: options.result,
      alt: options.alt ?? null,
      includeAttribution: options.includeAttribution,
    }),
};
