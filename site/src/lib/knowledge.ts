// Knowledge reference resolution at build time.
//
// `handoff.md` section 9.4: a knowledge update flows into every article that cites it, and the
// resolution happens during the build. Section 9.3 adds the rule that matters most: a reference
// whose target does not exist must STAY BROKEN with a clear diagnostic. Veloct never guesses a
// neighbouring paragraph, and never silently drops the citation — that would make a wrong claim
// look sourced.
//
// Paragraph identity is the hidden `<!--p:ULID-->` comment maintained by `veloct-markdown`
// (section 9.2). The id does not depend on the paragraph text, so editing wording cannot move a
// citation.

import { getCollection } from 'astro:content';

/** `<!--p:01ARZ...-->` on its own, capturing the id. */
const PARAGRAPH_MARKER = /<!--\s*p:([0-9A-HJKMNP-TV-Z]{26})\s*-->/g;

/**
 * Split a knowledge body into paragraphs, each with its optional stable id.
 *
 * Paragraphs are separated by blank lines, which is the default the requirement names. A marker on
 * its own line attaches to the paragraph that follows it.
 */
export function extractParagraphs(body) {
  const text = String(body ?? '').replace(/\r\n?/g, '\n');
  const blocks = text.split(/\n{2,}/);
  const paragraphs = [];
  let pendingId = null;

  for (const raw of blocks) {
    const trimmed = raw.trim();
    if (trimmed.length === 0) continue;

    // A block that is only a marker assigns the id to the next paragraph.
    const wholeMarker = trimmed.match(/^<!--\s*p:([0-9A-HJKMNP-TV-Z]{26})\s*-->$/);
    if (wholeMarker) {
      pendingId = wholeMarker[1];
      continue;
    }

    // A marker at the start of a block, followed by prose on the next line.
    let id = pendingId;
    pendingId = null;
    const leading = trimmed.match(/^<!--\s*p:([0-9A-HJKMNP-TV-Z]{26})\s*-->\s*\n/);
    let content = trimmed;
    if (leading) {
      id = leading[1];
      content = trimmed.slice(leading[0].length).trim();
    }

    // Any remaining inline markers are internal identity and must never reach the reader.
    const found = [...content.matchAll(PARAGRAPH_MARKER)].map((match) => match[1]);
    if (found.length > 0) {
      if (!id) id = found[0];
      content = content.replace(PARAGRAPH_MARKER, '').trim();
    }

    if (content.length === 0) continue;
    paragraphs.push({ id, text: content, order: paragraphs.length + 1 });
  }

  return paragraphs;
}

/** Rough plain text of a Markdown paragraph, for a card excerpt and for hashing. */
export function paragraphPlainText(text) {
  return String(text)
    .replace(/<!--.*?-->/g, '')
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/`{1,3}([^`]*)`{1,3}/g, '$1')
    .replace(/(\*\*|__)(.*?)\1/g, '$2')
    .replace(/(\*|_)(.*?)\1/g, '$2')
    .replace(/^\s{0,3}#{1,6}\s+/gm, '')
    .replace(/^\s{0,3}>\s?/gm, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Truncate by character and add an ellipsis. handoff.md section 9.1 requires the ellipsis. */
export function excerpt(text, limit = 80) {
  const characters = [...text];
  if (characters.length <= limit) return text;
  return `${characters.slice(0, limit).join('')}…`;
}

/**
 * A knowledge corpus, indexed by content id and then by paragraph id.
 *
 * Lookup is a map rather than a scan per reference, because an article may cite dozens of
 * paragraphs and each lookup happens during rendering.
 */
export function buildKnowledgeIndex(entries) {
  const byId = new Map();
  const bySlug = new Map();

  for (const entry of entries) {
    const paragraphs = extractParagraphs(entry.body);
    const byParagraph = new Map();
    const duplicateIds = [];

    for (const paragraph of paragraphs) {
      if (!paragraph.id) continue;
      // A duplicate id is a hard error, not a last-one-wins: two paragraphs claiming one identity
      // means a citation would point at whichever happened to be indexed second.
      if (byParagraph.has(paragraph.id)) {
        duplicateIds.push(paragraph.id);
        continue;
      }
      byParagraph.set(paragraph.id, paragraph);
    }

    const record = {
      id: entry.data.id,
      slug: entry.data.slug,
      title: entry.data.title,
      author: entry.data.author ?? null,
      year: entry.data.year ?? String(entry.data.created_at ?? '').slice(0, 4),
      url: entry.data.url ?? null,
      paragraphs,
      byParagraph,
      duplicateIds,
      entry,
    };

    byId.set(entry.data.id, record);
    bySlug.set(entry.data.slug, record);
  }

  return { byId, bySlug };
}

/**
 * Resolve one reference.
 *
 * Returns a discriminated result rather than throwing, so the caller can render a visible broken
 * state and still fail the build with a useful message. There is deliberately no fallback to "the
 * first paragraph" or "a paragraph with similar text": guessing is exactly what the contract
 * forbids.
 */
export function resolveReference(index, { content, paragraph }) {
  const record = index.byId.get(content);
  if (!record) {
    return {
      status: 'broken',
      code: 'UNKNOWN_CONTENT_ID',
      message: `no knowledge document has the id ${content}`,
      contentId: content,
      paragraphId: paragraph,
    };
  }

  const target = record.byParagraph.get(paragraph);
  if (!target) {
    return {
      status: 'broken',
      code: 'BROKEN_REFERENCE',
      message: `paragraph ${paragraph} does not exist in knowledge document ${record.title} (${content})`,
      contentId: content,
      paragraphId: paragraph,
      title: record.title,
    };
  }

  return {
    status: 'resolved',
    contentId: content,
    paragraphId: paragraph,
    title: record.title,
    slug: record.slug,
    excerpt: excerpt(paragraphPlainText(target.text)),
    citation: citationOf(record),
  };
}

/** Harvard-style citation for one knowledge document, per handoff.md section 9.4. */
export function citationOf(record) {
  return {
    contentId: record.id,
    author: record.author ?? 'Unknown',
    year: record.year || 'n.d.',
    title: record.title,
    url: record.url,
  };
}

/** Render one citation the way the reference list shows it. */
export function citationText(citation) {
  const base = `${citation.author} (${citation.year}) ${citation.title}.`;
  return citation.url ? `${base} Available at: ${citation.url}` : base;
}

/**
 * Every reference in a document, in source order, with its resolution.
 *
 * The document is scanned as text because the directive lives in the Markdown source. This is a
 * scan of a *declared construct*, not an attempt to re-parse Markdown: `veloct-markdown` owns the
 * AST, and this only needs the reference list.
 */
export function collectReferences(body) {
  const text = String(body ?? '');
  const pattern = /:::\s*knowledgeref\s*\{([^}]*)\}/g;
  const references = [];

  for (const match of text.matchAll(pattern)) {
    const attributes = new Map();
    const attributePattern = /([A-Za-z_][\w-]*)\s*=\s*("([^"]*)"|'([^']*)'|([^\s,}]+))/g;
    for (const attribute of match[1].matchAll(attributePattern)) {
      attributes.set(attribute[1].toLowerCase(), attribute[3] ?? attribute[4] ?? attribute[5] ?? '');
    }
    const content = attributes.get('content');
    const paragraph = attributes.get('paragraph');
    if (!content || !paragraph) continue;
    references.push({ content, paragraph, location: `offset:${match.index}` });
  }

  return references;
}

/**
 * The reference list for an article.
 *
 * Every cited knowledge entry appears once, sorted, and the order does not depend on where the
 * directives happened to appear — which is what makes a rebuild comparable
 * (`handoff.md` section 14.9).
 */
export function referenceList(index, references) {
  const seen = new Set();
  const citations = [];

  for (const reference of references) {
    const record = index.byId.get(reference.content);
    if (!record) continue;
    if (seen.has(record.id)) continue;
    seen.add(record.id);
    citations.push(citationOf(record));
  }

  citations.sort((a, b) => a.contentId.localeCompare(b.contentId));
  return citations;
}

/**
 * Resolve every reference and produce both the cards and the article's reference list.
 *
 * Returns a `cards` array keyed by the same `content`/`paragraph` pair the remark plugin emitted, so
 * the client script can match a rendered directive element to its resolution. A broken reference is
 * INCLUDED with `status: 'broken'` rather than omitted, so the page can show it as broken instead of
 * dropping a citation (`handoff.md` section 9.3).
 */
export function resolveReferences(index, references) {
  const cards = [];

  for (const reference of references) {
    const resolution = resolveReference(index, reference);
    if (resolution.status === 'resolved') {
      cards.push({
        status: 'resolved',
        contentId: reference.content,
        paragraphId: reference.paragraph,
        title: resolution.title,
        slug: resolution.slug,
        excerpt: resolution.excerpt,
      });
    } else {
      cards.push({
        status: 'broken',
        contentId: reference.content,
        paragraphId: reference.paragraph,
        message: resolution.message,
      });
    }
  }

  return { cards, citations: referenceList(index, references) };
}

/** Load every knowledge document and build its index. */
export async function loadKnowledgeIndex() {
  const entries = await getCollection('knowledge');
  return { index: buildKnowledgeIndex(entries), entries };
}
