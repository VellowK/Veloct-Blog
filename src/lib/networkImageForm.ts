// The network image browser's decisions, as a pure module.
//
// `handoff.md` §13.14 is a locked section and this repository has no React test renderer — deliberately.
// So every decision the panel makes lives here and is covered by `tests/networkImageForm.test.ts`: which
// sources are offered, what a source needs before a search may be sent, what the user is committed to
// when they pick an image, and whether the credit line may be left out.
//
// **The one rule that must not be re-derived in the window is the attribution.** Whether a credit line is
// optional is a property of the PROVIDER's terms, and `veloct-attach` enforces it at the point a choice
// is built. The window reads `attribution_optional` from the search result rather than deciding for
// itself, because a second implementation of "does Unsplash require credit" is a second chance to get it
// wrong — and getting it wrong publishes a page that violates the provider's terms.

import type {
  ImageResultView,
  ImageSearchView,
  NetworkImageChoiceResultView,
  NetworkImageSourceView,
} from './api';

/** The sources the panel lists, split by whether this build can search them. */
export interface SourceOptions {
  /** The sources a search may be sent to. */
  offered: NetworkImageSourceView[];
  /** The sources this build cannot search, each with the provider's own reason. */
  refused: NetworkImageSourceView[];
}

/**
 * Split the reported sources.
 *
 * A source is offered when the BUILD says it is implemented AND it is not carrying a reason, which the
 * crate guarantees cannot disagree. A refused source is still returned rather than filtered away: its
 * absence is explained in the window, not left for the user to notice.
 */
export function sourceOptions(sources: NetworkImageSourceView[]): SourceOptions {
  const offered: NetworkImageSourceView[] = [];
  const refused: NetworkImageSourceView[] = [];
  for (const source of sources) {
    if (source.implemented && !source.unimplemented_reason) offered.push(source);
    else refused.push(source);
  }
  return { offered, refused };
}

/**
 * One sentence naming every source this build will not search, or `null` when it will search them all.
 *
 * The same device as the attachment panel's kind-gap summary: a source named by the contract that is
 * NOT here is a sentence rather than a silence.
 */
export function refusedSummary(refused: NetworkImageSourceView[]): string | null {
  const explained = refused.filter((source) => source.unimplemented_reason);
  if (explained.length === 0) return null;
  const parts = explained.map(
    (source) => `${source.label}:${source.unimplemented_reason}`,
  );
  return `以下来源未接入 —— ${parts.join(' ')}`;
}

/** Whether a source has a key stored, read from the view. */
export function hasKey(source: NetworkImageSourceView | undefined): boolean {
  return Boolean(source?.has_key);
}

/** What a source still needs before a search may be sent. */
export interface SearchReadiness {
  ready: boolean;
  reason: string | null;
  /** True when the missing thing is the key, so the panel can offer the key field rather than explain. */
  needsKey: boolean;
}

/**
 * Whether a search may be sent, and if not, what is missing.
 *
 * A disabled button with no explanation is exactly what §13.2.2's reason exists to prevent elsewhere in
 * this window, so this returns the sentence rather than only a boolean.
 */
export function searchReadiness(options: {
  source: NetworkImageSourceView | undefined;
  query: string;
  busy: boolean;
}): SearchReadiness {
  const { source, query, busy } = options;
  if (!source) {
    return { ready: false, reason: '请先选择一个来源。', needsKey: false };
  }
  if (!source.implemented) {
    return {
      ready: false,
      reason: source.unimplemented_reason ?? `${source.label} 在当前构建中不可用。`,
      needsKey: false,
    };
  }
  if (busy) {
    return { ready: false, reason: '正在搜索…', needsKey: false };
  }
  if (query.trim() === '') {
    return { ready: false, reason: '请输入搜索关键词。', needsKey: false };
  }
  if (source.needs_key && !source.has_key) {
    return {
      ready: false,
      reason: `请先保存 ${source.label} 的访问密钥;它会存进操作系统凭据库(§12.2)。`,
      needsKey: true,
    };
  }
  return { ready: true, reason: null, needsKey: false };
}

/** Why a key cannot be saved, or `null` when it can. */
export function validateKeyInput(key: string): string | null {
  if (key.trim() === '') {
    return '密钥不能为空;要去掉已保存的密钥请用「删除密钥」。';
  }
  // A key is pasted more often than typed, and a trailing newline from a copy is the common mistake.
  if (/\s/.test(key.trim())) {
    return '密钥中不能包含空格或换行,请检查是否复制了多余字符。';
  }
  return null;
}

/**
 * How many results to ask for, kept inside the provider's own limits.
 *
 * The ceiling comes from the source's view (which reads it from the crate), so the window cannot offer a
 * page size the provider would refuse.
 */
export function pageSizeFor(
  source: NetworkImageSourceView | undefined,
  requested: number,
): number {
  if (!source || source.default_per_page <= 0) return 0;
  if (!Number.isFinite(requested) || requested <= 0) return source.default_per_page;
  return Math.min(Math.floor(requested), source.max_per_page);
}

/** A human line for a result's dimensions and author. */
export function resultMeta(result: ImageResultView): string {
  const parts: string[] = [];
  if (result.author.trim() !== '') parts.push(result.author.trim());
  if (result.width > 0 && result.height > 0) parts.push(`${result.width}×${result.height}`);
  if (result.licence) parts.push(result.licence);
  return parts.join(' · ');
}

/** The alt text a result would carry if the user types none, so the field can show the fallback. */
export function suggestedAlt(result: ImageResultView): string {
  if (result.alt && result.alt.trim() !== '') return result.alt.trim();
  if (result.author.trim() !== '') {
    return `Image by ${result.author.trim()}`;
  }
  return '';
}

/**
 * What choosing from this source commits the user to.
 *
 * Read from the view rather than decided here: `attribution_optional` is the crate's answer, derived
 * from the provider's own terms.
 */
export function attributionNotice(view: {
  label: string;
  attribution_rule: string;
  attribution_optional: boolean;
}): { text: string; required: boolean } {
  const required = !view.attribution_optional;
  const text = required
    ? `${view.label}:${view.attribution_rule} 署名会一并写入 Markdown,无法取消。`
    : `${view.label}:${view.attribution_rule} 署名可以选择是否写入 Markdown。`;
  return { text, required };
}

/**
 * Whether the panel should offer the "include the credit line" switch.
 *
 * Naming the two states explicitly means the component cannot render a switch that does nothing: for
 * Unsplash the crate forces the credit in whatever the switch says, so a switch there would be a lie.
 */
export function attributionToggle(view: {
  attribution_optional: boolean;
}): { shown: boolean; label: string } {
  return view.attribution_optional
    ? { shown: true, label: '把署名写入文章' }
    : { shown: false, label: '署名必须保留(来源条款要求)' };
}

/** Whether a chosen result may be inserted. */
export function insertReadiness(options: {
  markdown: string;
  busy: boolean;
}): { ready: boolean; reason: string | null } {
  if (options.busy) return { ready: false, reason: '正在处理…' };
  if (options.markdown.trim() === '') {
    return { ready: false, reason: '还没有可插入的 Markdown。' };
  }
  if (!options.markdown.includes('](')) {
    // A guard against inserting something that is not an image at all.
    return { ready: false, reason: '这段 Markdown 不是图片语法,已拒绝插入。' };
  }
  return { ready: true, reason: null };
}

/**
 * What to say after a choice, including the compliance step's outcome.
 *
 * Unsplash requires a download report, and this module's job is to make sure a FAILED report is a
 * sentence rather than a silence — §20 forbids a silent failure, and the image itself is still fine, so
 * the user is told rather than blocked.
 */
export function choiceNotice(result: NetworkImageChoiceResultView): {
  level: 'ok' | 'warning';
  text: string;
} {
  if (result.download_reported === true) {
    return { level: 'ok', text: '已插入,并按来源要求上报了下载。' };
  }
  if (result.download_reported === false) {
    const why = result.download_note ? ` ${result.download_note}` : '';
    return {
      level: 'warning',
      text: `已插入,但下载上报没有成功。图片本身可以使用。${why}`,
    };
  }
  return { level: 'ok', text: '已插入。' };
}

/** The note shown above the results, which the source's own notice carries. */
export function searchNotice(view: ImageSearchView): string {
  const dropped =
    view.dropped > 0 ? ` 有 ${view.dropped} 条结果因为缺少可用地址被隐藏。` : '';
  return `${view.label} · 共 ${view.results.length} 条结果。${dropped}`;
}

/** The label for a "load more" control, or `null` when there is nothing more. */
export function moreLabel(view: ImageSearchView, page: number): string | null {
  if (!view.has_more) return null;
  return `加载第 ${page + 1} 页`;
}

/** The article-ready Markdown, so the panel and the command cannot disagree about what was inserted. */
export function markdownFor(result: NetworkImageChoiceResultView): string {
  return result.markdown;
}
