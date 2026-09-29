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
};
