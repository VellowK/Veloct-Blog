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
