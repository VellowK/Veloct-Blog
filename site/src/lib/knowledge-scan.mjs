// Build the knowledge index from disk, before Astro resolves the content collections.
//
// Why this exists: `handoff.md` section 9.4 requires a knowledge reference to resolve to the
// CURRENT text of the cited paragraph at build time, and a card component cannot await a collection
// query. Astro's config is evaluated before anything else, so this is the earliest hook that can
// produce a synchronously importable index.
//
// It reads the knowledge directory directly rather than going through the content layer, which is
// safe here because the index is derived data: the truth is still the Markdown file, and a rebuild
// regenerates this from scratch. It deliberately mirrors the parse rules in `knowledge.ts`.

import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { createHash } from 'node:crypto';

const MARKER = /<!--\s*p:([0-9A-HJKMNP-TV-Z]{26})\s*-->/g;

/** Minimal frontmatter reader: enough for the fields a citation needs. */
function readFrontmatter(text) {
  const match = text.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?/);
  if (!match) return { data: {}, body: text, bodyStartLine: 1 };

  const yaml = match[1];
  const data = {};
  for (const line of yaml.split(/\r?\n/)) {
    const entry = line.match(/^([A-Za-z_][\w-]*)\s*:\s*(.*)$/);
    if (!entry) continue;
    let value = entry[2].trim();
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      value = value.slice(1, -1);
    }
    if (value === '') continue;
    data[entry[1]] = value;
  }

  const bodyStartLine = match[0].split(/\r?\n/).length;
  return { data, body: text.slice(match[0].length), bodyStartLine };
}

/** Strip Markdown down to readable text, for an excerpt. */
function plainText(text) {
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

/**
 * Truncate by character and always mark the result as an excerpt.
 *
 * `handoff.md` section 9.1 requires the card to show an excerpt followed by an ellipsis. A short
 * paragraph is still an excerpt of a larger document, so the ellipsis is unconditional rather than
 * only appearing when text was cut: a card that sometimes has no ellipsis is a card a reader cannot
 * tell apart from a full quotation.
 */
export function excerpt(text, limit = 80) {
  const characters = [...text];
  const body = characters.length <= limit ? text : characters.slice(0, limit).join('');
  return `${body}…`;
}

/**
 * Split a knowledge body into paragraphs keyed by their stable id.
 *
 * A paragraph without an id is still recorded under a synthetic key so it counts toward the
 * document, but it cannot be referenced: handing it a made-up id would make a citation point at
 * something the editor cannot maintain.
 */
export function parseParagraphs(body) {
  const text = String(body).replace(/\r\n?/g, '\n');
  const blocks = text.split(/\n{2,}/);
  const paragraphs = [];
  let pendingId = null;

  for (const raw of blocks) {
    const trimmed = raw.trim();
    if (trimmed.length === 0) continue;

    const wholeMarker = trimmed.match(/^<!--\s*p:([0-9A-HJKMNP-TV-Z]{26})\s*-->$/);
    if (wholeMarker) {
      pendingId = wholeMarker[1];
      continue;
    }

    let id = pendingId;
    pendingId = null;

    const leading = trimmed.match(/^<!--\s*p:([0-9A-HJKMNP-TV-Z]{26})\s*-->\s*\n/);
    let content = trimmed;
    if (leading) {
      id = leading[1];
      content = trimmed.slice(leading[0].length).trim();
    }

    const inline = [...content.matchAll(MARKER)].map((match) => match[1]);
    if (inline.length > 0) {
      if (!id) id = inline[0];
      content = content.replace(MARKER, '').trim();
    }

    if (content.length === 0) continue;

    const clean = plainText(content);
    paragraphs.push({
      id,
      order: paragraphs.length + 1,
      excerpt: excerpt(clean),
      contentHash: createHash('sha256').update(clean).digest('hex').slice(0, 16),
    });
  }

  return paragraphs;
}

/**
 * Scan a knowledge directory and return the index.
 *
 * Also returns the diagnostics a caller can log: a duplicate paragraph id is an error
 * (`handoff.md` section 9.3), and an unidentified paragraph is a warning.
 *
 * Each document carries `orderedIds`: the paragraph ids in document order. That is what lets a
 * reference card report WHICH paragraph it resolves to, so the right sidebar can open the cited
 * location, without the card having to re-parse the body.
 */
export function scanKnowledgeDirectory(directory) {
  const documents = {};
  const diagnostics = [];

  let entries = [];
  try {
    entries = readdirSync(directory, { withFileTypes: true });
  } catch (error) {
    if (error && error.code === 'ENOENT') return { documents, diagnostics };
    throw error;
  }

  const files = [];
  const collect = (current, prefix) => {
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        collect(join(current, entry.name), relative);
      } else if (entry.isFile() && entry.name.endsWith('.md')) {
        files.push({ path: join(current, entry.name), relative });
      }
    }
  };
  collect(directory, '');
  files.sort((a, b) => a.relative.localeCompare(b.relative));

  for (const file of files) {
    const text = readFileSync(file.path, 'utf8');
    const { data, body } = readFrontmatter(text);

    const id = data.id;
    if (!id) {
      diagnostics.push({
        severity: 'error',
        code: 'MISSING_CONTENT_ID',
        message: `${file.relative} has no id in its frontmatter`,
      });
      continue;
    }

    const paragraphs = parseParagraphs(body);
    const byId = {};
    for (const paragraph of paragraphs) {
      if (!paragraph.id) {
        diagnostics.push({
          severity: 'warning',
          code: 'UNIDENTIFIED_PARAGRAPH',
          message: `${file.relative} paragraph ${paragraph.order} has no stable id, so it cannot be referenced`,
        });
        continue;
      }
      if (byId[paragraph.id]) {
        diagnostics.push({
          severity: 'error',
          code: 'DUPLICATE_PARAGRAPH_ID',
          message: `${file.relative} has the paragraph id ${paragraph.id} twice`,
        });
        continue;
      }
      byId[paragraph.id] = paragraph;
    }

    const year = data.year ?? String(data.created_at ?? '').slice(0, 4);

    documents[id] = {
      id,
      slug: data.slug ?? id,
      title: data.title ?? id,
      paragraphCount: paragraphs.length,
      paragraphs: byId,
      orderedIds: paragraphs.filter((paragraph) => paragraph.id).map((paragraph) => paragraph.id),
      citation: {
        contentId: id,
        author: data.author ?? 'Unknown',
        year: year || 'n.d.',
        title: data.title ?? id,
        url: data.url ?? null,
      },
    };
  }

  return { documents, diagnostics };
}

/**
 * Write the generated index.
 *
 * Exported so both the Astro config and the build verification script produce exactly the same
 * artefact, and a test can assert the two agree.
 */
export function writeKnowledgeIndex({ knowledgeDir, outputPath }) {
  const { documents, diagnostics } = scanKnowledgeDirectory(knowledgeDir);

  const payload = {
    version: 1,
    paragraphCount: Object.values(documents).reduce((total, doc) => total + doc.paragraphCount, 0),
    documents,
  };

  mkdirSync(dirname(outputPath), { recursive: true });
  writeFileSync(outputPath, `${JSON.stringify(payload, null, 2)}\n`, 'utf8');

  return { payload, diagnostics };
}

/** The default paths, relative to a site directory. */
export function defaultPaths(siteDir) {
  const projectRoot = resolve(siteDir, '..');
  return {
    knowledgeDir: resolve(projectRoot, 'knowledge'),
    outputPath: resolve(siteDir, '.veloct-generated', 'knowledge-index.json'),
  };
}
