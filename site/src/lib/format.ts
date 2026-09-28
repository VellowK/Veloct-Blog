// Rendering helpers shared by the layouts and pages.
//
// `handoff.md` section 4.4 requires a word count and an estimated reading time on every article,
// and section 14.9 requires time-derived output to be stable across a rebuild of one release. Both
// live here.

import { readSiteConfig } from './site-config.mjs';

/** Whether this build is a production build (`astro build`) rather than the dev server. */
export const isProduction = import.meta.env.PROD;

/** The site configuration for the current build. */
export const siteConfig = readSiteConfig();

/** Whether CJK characters should be counted per character. */
const CJK = /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/;

/**
 * Count words in mixed Latin and CJK text.
 *
 * A whitespace split is wrong here: Veloct targets Chinese-language sites, and `你好世界` is four
 * words to a reader, not one. This mirrors `veloct-markdown`'s `count_words` so the number the
 * editor shows and the number the page shows agree.
 */
export function countWords(text) {
  let words = 0;
  let inLatinRun = false;
  for (const character of text) {
    if (CJK.test(character)) {
      words += 1;
      inLatinRun = false;
    } else if (/[0-9A-Za-z\u00c0-\u024f]/.test(character)) {
      if (!inLatinRun) {
        words += 1;
        inLatinRun = true;
      }
    } else if (character !== "'" && character !== '\u2019' && character !== '-') {
      inLatinRun = false;
    }
  }
  return words;
}

/** Estimated reading minutes, never zero for something with content. */
export function readingMinutes(words) {
  if (words === 0) return 0;
  // 400 CJK characters or 220 Latin words per minute; the body mixes both, so this is an estimate
  // that errs slightly long rather than claiming a dense page is a one-minute read.
  return Math.max(1, Math.ceil(words / 300));
}

/** Word count and reading time for a Markdown body. */
export function readingStats(body) {
  const words = countWords(body ?? '');
  return { words, minutes: readingMinutes(words) };
}

/** Format an ISO timestamp for display, deterministically. */
export function formatDate(value, locale = 'zh-CN') {
  if (!value) return '';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return String(value);
  // `Intl` with an explicit time zone keeps the output stable regardless of the build machine.
  return new Intl.DateTimeFormat(locale, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    timeZone: 'UTC',
  }).format(date);
}

/** The year of an ISO timestamp, used for archive grouping. */
export function yearOf(value) {
  if (!value) return 'unknown';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'unknown';
  return String(date.getUTCFullYear());
}

/** Sort entries newest first, by publication then creation time. */
export function byNewest(a, b) {
  const left = a.data.published_at ?? a.data.created_at;
  const right = b.data.published_at ?? b.data.created_at;
  return String(right).localeCompare(String(left));
}

/** Group entries into a tag -> entries map, sorted by tag name. */
export function groupByTag(entries) {
  const groups = new Map();
  for (const entry of entries) {
    for (const tag of entry.data.tags ?? []) {
      if (!groups.has(tag)) groups.set(tag, []);
      groups.get(tag).push(entry);
    }
  }
  return new Map([...groups.entries()].sort((a, b) => a[0].localeCompare(b[0])));
}

/** Group entries into a category -> entries map, sorted by category name. */
export function groupByCategory(entries) {
  const groups = new Map();
  for (const entry of entries) {
    const category = entry.data.category;
    if (!category) continue;
    if (!groups.has(category)) groups.set(category, []);
    groups.get(category).push(entry);
  }
  return new Map([...groups.entries()].sort((a, b) => a[0].localeCompare(b[0])));
}

/**
 * Build a word cloud from tag frequencies, biggest first.
 *
 * Sizes are relative weights rather than font classes, so the component decides how to render them
 * and a rebuild produces the same ordering.
 */
export function wordCloud(entries) {
  const counts = new Map();
  for (const entry of entries) {
    for (const tag of entry.data.tags ?? []) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
  }
  const max = Math.max(1, ...counts.values());
  return [...counts.entries()]
    .map(([tag, count]) => ({ tag, count, weight: count / max }))
    .sort((a, b) => b.count - a.count || a.tag.localeCompare(b.tag));
}

/**
 * A release-stable "now".
 *
 * A rebuild of a release must not change any emitted time field (handoff.md section 14.9), so the
 * build time comes from the environment the application injects, never from `Date.now()`.
 */
export function buildTime() {
  const injected = import.meta.env.VELOCT_BUILD_TIME;
  return injected ? new Date(injected) : new Date(0);
}

/** Plain text of an mdast or hast-ish node tree, for excerpts. */
export function plainText(node) {
  if (!node) return '';
  if (typeof node === 'string') return node;
  if (Array.isArray(node)) return node.map(plainText).join('');
  if (node.value) return String(node.value);
  if (node.children) return plainText(node.children);
  return '';
}

/** A URL-safe anchor id for a heading, preserving CJK. */
export function anchorId(text) {
  const cleaned = String(text)
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}_-]+/gu, '-')
    .replace(/^-+|-+$/g, '');
  return cleaned.length > 0 ? cleaned : 'section';
}
