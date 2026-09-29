/**
 * The frontmatter and body, kept apart.
 *
 * `handoff.md` §3.2 puts the stable `id` in the frontmatter, and the id is what addresses the file, what
 * a WIP URL is built from and what a reference points at. A round-trip through a rich-text editor must
 * therefore not touch the frontmatter at all: if the editor re-serialised it, a key order change would be
 * cosmetic but a dropped or reformatted `id` would be data loss, and `wip_ever_published` being reset
 * would break a permanent redirect forever.
 *
 * So the split is byte-exact. Everything from the opening `---` through the closing `---` and its newline
 * is kept VERBATIM as an opaque prefix, the editor works on the body alone, and the two are re-joined
 * unchanged. That is the whole reason this module exists rather than the editor doing its own parsing.
 *
 * A file with no frontmatter is treated as all body, because that is what the parser sees, and inventing
 * a block would produce a file the content layer would refuse.
 */

export interface SplitSource {
  /** The verbatim frontmatter block INCLUDING both `---` lines and the trailing newline. */
  frontmatter: string;
  /** Everything after the block. */
  body: string;
  /** Whether a block was found at all. */
  hasFrontmatter: boolean;
}

/** The opening fence, which must be the file's first line. */
const OPENING = /^---\r?\n/;

/**
 * Split a document into its frontmatter block and its body.
 *
 * The closing fence is the first line that is exactly `---`, which is what the YAML frontmatter
 * convention means and what `veloct-markdown`'s own splitter does. A `---` inside the body is not a
 * closing fence because the block has already ended by then.
 */
export function splitSource(source: string): SplitSource {
  const opening = source.match(OPENING);
  if (!opening) {
    return { frontmatter: '', body: source, hasFrontmatter: false };
  }

  // Search only after the opening fence, so an opening `---` cannot be mistaken for the closing one.
  const afterOpening = source.slice(opening[0].length);
  const closing = /^---[ \t]*\r?\n?/m.exec(afterOpening);

  if (!closing) {
    // An unterminated block. Treated as all body rather than guessed at, because a guessed block would
    // be written back and could lose the content that followed it.
    return { frontmatter: '', body: source, hasFrontmatter: false };
  }

  const end = opening[0].length + closing.index + closing[0].length;
  return {
    frontmatter: source.slice(0, end),
    body: source.slice(end),
    hasFrontmatter: true,
  };
}

/**
 * Re-join a frontmatter block and a body.
 *
 * The frontmatter is written VERBATIM. There is deliberately no normalisation step: a normaliser is
 * exactly the thing that would silently reorder keys or drop one it did not recognise.
 */
export function joinSource(frontmatter: string, body: string): string {
  if (!frontmatter) return body;
  // A block that somehow lost its trailing newline would glue the body onto the closing fence — but only
  // when there IS a body. Adding one to an empty body would make a round trip of a file whose closing
  // fence has no newline produce a different file, which is exactly the kind of drift this module exists
  // to prevent.
  const separator = frontmatter.endsWith('\n') || body === '' ? '' : '\n';
  return `${frontmatter}${separator}${body}`;
}

/**
 * Replace only the body of a source, keeping its frontmatter byte-identical.
 *
 * This is what the editor calls on every change. It is a single function so there is one place where the
 * frontmatter could be lost, and it is not in the editor.
 */
export function replaceBody(source: string, body: string): string {
  const { frontmatter } = splitSource(source);
  return joinSource(frontmatter, body);
}

/**
 * The frontmatter as key/value pairs, for display.
 *
 * Deliberately a shallow, line-based read rather than a YAML parser: this is for SHOWING the user which
 * fields a document has, and a parser here would invite someone to write the values back, which is the
 * thing the module exists to prevent.
 */
export function frontmatterFields(frontmatter: string): Array<{ key: string; value: string }> {
  if (!frontmatter) return [];
  const lines = frontmatter.split(/\r?\n/);
  const fields: Array<{ key: string; value: string }> = [];

  for (const line of lines) {
    // Skip the fences and any indented continuation of a nested value.
    if (line.trim() === '---' || line.trim() === '') continue;
    if (/^\s/.test(line)) continue;

    const match = /^([A-Za-z_][\w-]*)\s*:\s*(.*)$/.exec(line);
    if (!match) continue;
    const [, key, rawValue] = match;
    if (key === undefined) continue;
    fields.push({ key, value: stripQuotes(rawValue ?? '') });
  }

  return fields;
}

function stripQuotes(value: string): string {
  const trimmed = value.trim();
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"') && trimmed.length >= 2) ||
    (trimmed.startsWith("'") && trimmed.endsWith("'") && trimmed.length >= 2)
  ) {
    return trimmed.slice(1, -1);
  }
  return trimmed;
}

/**
 * Whether the body has unsaved-looking whitespace differences.
 *
 * Used to decide whether to show the "the editor normalised your Markdown" note. A Markdown round trip
 * through a rich-text editor WILL change incidental whitespace, and pretending otherwise would make the
 * first save look like a large edit.
 */
export function bodiesDifferMeaningfully(before: string, after: string): boolean {
  return normaliseWhitespace(before) !== normaliseWhitespace(after);
}

function normaliseWhitespace(body: string): string {
  return body
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}
