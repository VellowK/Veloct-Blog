// The workbench's decisions about a work, as a pure module.
//
// The bulk of the works panel is rules, and a rule that lives in JSX is a rule nothing checks: this
// repository has no React test renderer, so every decision the panel makes is here instead and is
// covered by `tests/workForm.test.ts`.
//
// Two of these rules have a second implementation in Rust, and that duplication is deliberate rather
// than accidental:
//
//  * `validateWorkDraft` restates `WorkMetadata::validate` (crates/veloct-content/src/works/metadata.rs).
//    The crate is the authority and refuses an invalid draft whatever the window does; the copy exists
//    so the user is told in the field they typed rather than by an error after a save that could never
//    have succeeded. `CRATE_RULE` below is the transcription that keeps the two honest, in the same
//    spirit as `storageTargetForm.ts::CRATE_RULE` (PROCESSING.md section 5.10.1).
//  * `slugify` restates `veloct_core::paths::slugify` for the same reason. The import itself sends
//    only a title, so the crate does the real work; this is a PREVIEW of the address the work will get.

/** The fields a user may edit, in the shape the form holds them. */
export interface WorkDraft {
  /** The stable id. Never editable: it is the directory name (handoff.md section 3.2). */
  id: string;
  title: string;
  author: string;
  slug: string;
  summary: string;
  entry: string;
  /** Free text, one tag per line, as the textarea holds it. */
  tags: string;
  notes: string;
  completedAt: string;
  cover: string;
}

/** The subset of a work view this module needs, so a test can build one without an API. */
export interface WorkLike {
  metadata: {
    id: string;
    title: string;
    author: string;
    slug: string;
    summary: string;
    entry: string;
    tags: string[];
    notes: string;
    completed_at: string | null;
    cover: string | null;
  };
}

/**
 * A shape's verdict, transcribed from the crate.
 *
 * Every entry was produced by CALLING `WorkMetadata::validate` with that shape, not by reading it
 * and reasoning. A future rule must be added on both sides and here, or the test that exists to
 * catch a one-sided change will not catch it.
 */
export const CRATE_RULE: ReadonlyArray<{ shape: string; accepted: boolean; why: string }> = [
  { shape: 'a plain title and slug', accepted: true, why: 'nothing to refuse' },
  { shape: 'an empty title', accepted: false, why: 'title must not be empty' },
  { shape: 'a whitespace-only title', accepted: false, why: 'title must not be empty' },
  { shape: 'an empty slug', accepted: false, why: 'slug must not be empty' },
  { shape: 'a slug with a forward slash', accepted: false, why: 'it would escape the works root' },
  { shape: 'a slug with a backslash', accepted: false, why: 'it would escape the works root' },
  { shape: 'an entry containing ..', accepted: false, why: 'it would escape the work directory' },
  { shape: 'an entry starting with /', accepted: false, why: 'the entry must be relative' },
  { shape: 'an entry containing a drive colon', accepted: false, why: 'the entry must be relative' },
  { shape: 'a cover or screenshot containing ..', accepted: false, why: 'it would escape the work directory' },
];

/** Comment or blank lines in a tags textarea are not tags. */
export function parseTags(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of text.split(/\r?\n|,/)) {
    const tag = raw.trim().replace(/^#/, '').trim();
    if (tag === '' || seen.has(tag)) continue;
    seen.add(tag);
    out.push(tag);
  }
  return out;
}

/** The textarea form of a tag list. */
export function formatTags(tags: string[]): string {
  return tags.join('\n');
}

/** The editable draft of a work. */
export function draftFromWork(work: WorkLike): WorkDraft {
  return {
    id: work.metadata.id,
    title: work.metadata.title,
    author: work.metadata.author,
    slug: work.metadata.slug,
    summary: work.metadata.summary,
    entry: work.metadata.entry,
    tags: formatTags(work.metadata.tags),
    notes: work.metadata.notes,
    completedAt: work.metadata.completed_at ?? '',
    cover: work.metadata.cover ?? '',
  };
}

/**
 * The fields a save may change, in one place.
 *
 * `draftIsDirty` iterates this list rather than `Object.keys` of a draft: a caller that spreads an
 * extra field in (a test building a draft, a future panel holding more state) would otherwise make
 * the form look dirty for something it does not render, and Save would be offered for a change
 * nobody made.
 */
export const EDITABLE_FIELDS = [
  'title',
  'author',
  'slug',
  'summary',
  'entry',
  'tags',
  'notes',
  'completedAt',
  'cover',
] as const satisfies ReadonlyArray<keyof WorkDraft>;

/** Whether a draft differs from the work it came from, so Save is not offered for nothing. */
export function draftIsDirty(draft: WorkDraft, work: WorkLike): boolean {
  const original = draftFromWork(work);
  return EDITABLE_FIELDS.some((key) => draft[key] !== original[key]);
}

/**
 * Why a draft cannot be saved, or `null` when it can.
 *
 * The messages are the window's, in the field the user typed. The crate's own message arrives if
 * anything slips through, which is the behaviour `f29dcda` established for storage targets.
 */
export function validateWorkDraft(draft: WorkDraft): string | null {
  if (draft.title.trim() === '') {
    return '作品名称不能为空。';
  }
  if (draft.slug.trim() === '') {
    return '地址片段不能为空。';
  }
  if (draft.slug.includes('/') || draft.slug.includes('\\')) {
    return '地址片段不能包含路径分隔符(/ 或 \\)。';
  }
  if (draft.entry.trim() === '') {
    return '入口文件不能为空,例如 index.html。';
  }
  if (draft.entry.includes('..')) {
    return '入口文件不能包含 ..,否则会跳出作品目录。';
  }
  if (draft.entry.startsWith('/') || draft.entry.includes(':')) {
    return '入口文件必须是相对路径,例如 index.html。';
  }
  for (const [label, value] of [
    ['封面', draft.cover],
  ] as Array<[string, string]>) {
    if (value.includes('..')) {
      return `${label}路径不能包含 ..。`;
    }
  }
  return null;
}

/**
 * A preview of the slug `veloct-core::paths::slugify` will produce, for the import form.
 *
 * A RESTATEMENT of the crate's rule, because the form has to show the address the work will get and
 * the frontend cannot call Rust. The crate is the authority and the import sends only a title, so the
 * plan's slug overrides this preview the moment there is one.
 *
 * The crate's two decisions that are easy to get wrong, both preserved here:
 *
 *  * **CJK characters are KEPT.** Veloct is aimed at Chinese-language personal sites, so `计数器`
 *    is a valid address and only characters unsafe in a URL segment become a dash.
 *  * **A title with nothing sluggable falls back to `untitled`.** `WorkMetadata::new` then uses the
 *    work's stable id instead, which the window cannot know before the import — so the preview shows
 *    the crate's `untitled` and the plan carries the real slug.
 */
export function slugify(text: string): string {
  let out = '';
  let lastDash = false;
  for (const ch of text) {
    const keep = /[\p{L}\p{N}]/u.test(ch) || ch === '_';
    if (keep) {
      out += ch.toLowerCase();
      lastDash = false;
    } else if (!lastDash && out !== '') {
      out += '-';
      lastDash = true;
    }
  }
  const trimmed = out.replace(/^-+|-+$/g, '');
  return trimmed === '' ? 'untitled' : trimmed;
}

/** A byte count in a form a person reads. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '未知大小';
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KiB', 'MiB', 'GiB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

/** The image formats the crate stores a poster as (crates/veloct-app/src/works.rs). */
export const SUPPORTED_IMAGE_FORMATS = ['webp', 'png', 'jpg', 'jpeg'] as const;

/** The crate's poster ceiling, until the command has been read. */
export const DEFAULT_SCREENSHOT_LIMIT = 12 * 1024 * 1024;

/**
 * Why a chosen file cannot be a poster image, or `null` when it can.
 *
 * Stated BEFORE the copy so a mislabelled file is refused in the window rather than after a write
 * that has to be undone. The crate refuses the same shapes; this one exists so the user is told in
 * the picker.
 */
export function validateScreenshotChoice(options: {
  fileName: string;
  bytes: number;
  limit?: number;
}): string | null {
  const name = options.fileName.trim();
  if (name === '') {
    return '没有选择图片文件。';
  }
  const lastDot = name.lastIndexOf('.');
  const extension = lastDot === -1 ? '' : name.slice(lastDot + 1).toLowerCase();
  if (extension === '') {
    return '这个文件没有扩展名,Veloct 无法判断格式;请使用 webp / png / jpg。';
  }
  if (!(SUPPORTED_IMAGE_FORMATS as readonly string[]).includes(extension)) {
    return `.${extension} 不是支持的图片格式;请使用 ${SUPPORTED_IMAGE_FORMATS.join(' / ')}。`;
  }
  const limit = options.limit ?? DEFAULT_SCREENSHOT_LIMIT;
  if (!Number.isFinite(options.bytes) || options.bytes < 0) {
    return '无法读取这个文件的大小。';
  }
  if (options.bytes === 0) {
    return '这个文件是空的,不能作为封面截图。';
  }
  if (options.bytes > limit) {
    return `这个文件是 ${formatBytes(options.bytes)},超过 ${formatBytes(limit)} 的上限。`;
  }
  return null;
}

/**
 * Why a work will not be served, or `null` when it will.
 *
 * A work whose body is missing is listed rather than hidden — hiding it would make a broken import
 * look like a work that was never imported — so the panel needs a sentence for it.
 */
export function servableReason(work: {
  has_body: boolean;
  file_count: number;
  metadata: { entry: string };
}): string | null {
  if (!work.has_body) {
    return '作品本体(site/)不存在,构建不会包含它。可以删除后重新导入。';
  }
  if (work.file_count === 0) {
    return '作品本体是空目录,构建不会包含它。';
  }
  return null;
}

/** What the plan's skipped directories mean, in one sentence, or `null` when nothing was skipped. */
export function skippedNote(directories: string[]): string | null {
  if (directories.length === 0) return null;
  return `以下目录没有导入:${directories.join('、')}。它们不属于作品本体,导入时会跳过。`;
}

/**
 * A stable key for the plan form, so "Plan" is only enabled for a source already inspected.
 *
 * Without it a user can change the path and press import in the same tick, which would import a
 * source whose plan they never saw.
 */
export function planKey(options: { path: string; kind: string; title: string }): string {
  return `${options.kind}\u0000${options.path}\u0000${options.title.trim()}`;
}

/** Whether an import may be sent, and why not when it may not. */
export function importReadiness(options: {
  sourcePath: string;
  title: string;
  planned: boolean;
  blockedReason: string | null;
  busy: boolean;
}): { ready: boolean; reason: string | null } {
  if (options.busy) return { ready: false, reason: '正在处理…' };
  if (options.sourcePath.trim() === '') return { ready: false, reason: '请先选择 ZIP 或文件夹。' };
  if (options.title.trim() === '') return { ready: false, reason: '请填写作品名称。' };
  if (!options.planned) return { ready: false, reason: '请先查看导入预览。' };
  if (options.blockedReason) return { ready: false, reason: options.blockedReason };
  return { ready: true, reason: null };
}

/**
 * Whether a poster may be stored, and why not when it may not.
 *
 * `hasFile` is false when the user typed a path: the window cannot read a file it has not been
 * handed, so it offers the button and lets the crate report a missing path rather than guessing.
 */
export function posterReadiness(options: {
  screenshotPath: string;
  hasFile: boolean;
  fileProblem: string | null;
}): { ready: boolean; reason: string | null } {
  if (options.screenshotPath.trim() === '') {
    return { ready: false, reason: '请先选择一个图片文件。' };
  }
  if (options.hasFile && options.fileProblem) {
    return { ready: false, reason: options.fileProblem };
  }
  return { ready: true, reason: null };
}
