/**
 * The frontmatter contract, as a run test.
 *
 * `handoff.md` §3.2 puts the permanent `id` in the frontmatter and §7.2 makes `wip_ever_published`
 * irreversible. Both live in the block this module hands to the editor as an opaque prefix, so a
 * round trip that reformatted or dropped a key would either orphan a document's identity or delete a
 * permanent redirect for good.
 *
 * That is not a rule to state in a comment. It is the kind of thing that breaks silently the first time
 * someone "tidies up" the join, so it is asserted here, byte for byte.
 */

import { describe, expect, it } from 'vitest';

import {
  bodiesDifferMeaningfully,
  frontmatterFields,
  joinSource,
  replaceBody,
  splitSource,
} from '../src/lib/frontmatter';

/** A realistic article, with the awkward parts: an unknown key and an irreversible flag. */
const SOURCE = [
  '---',
  'id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY',
  'type: article',
  'slug: hello-veloct',
  'title: Hello Veloct',
  'wip: false',
  'wip_ever_published: true',
  'a_custom_key: the user added this by hand',
  'tags:',
  '  - veloct',
  'created_at: 2026-01-01T00:00:00Z',
  '---',
  '',
  '# Heading',
  '',
  'Body text.',
  '',
].join('\n');

describe('splitSource', () => {
  it('keeps the frontmatter block verbatim, including both fences', () => {
    const { frontmatter } = splitSource(SOURCE);
    expect(frontmatter.startsWith('---\n')).toBe(true);
    expect(frontmatter.endsWith('---\n')).toBe(true);
    expect(frontmatter).toContain('id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY');
  });

  it('puts everything after the closing fence in the body', () => {
    const { body } = splitSource(SOURCE);
    expect(body.startsWith('\n# Heading')).toBe(true);
    expect(body).toContain('Body text.');
    expect(body).not.toContain('---');
  });

  it('round trips the whole source byte for byte', () => {
    // The single most important assertion in this file.
    const { frontmatter, body } = splitSource(SOURCE);
    expect(joinSource(frontmatter, body)).toBe(SOURCE);
  });

  it('treats a file with no frontmatter as all body rather than inventing a block', () => {
    const { frontmatter, body, hasFrontmatter } = splitSource('just text\n');
    expect(hasFrontmatter).toBe(false);
    expect(frontmatter).toBe('');
    expect(body).toBe('just text\n');
  });

  it('treats an unterminated block as all body rather than guessing where it ends', () => {
    // A guessed block would be written back and could lose the content that followed it.
    const unterminated = '---\nid: 01ABC\ntitle: x\n\n# Heading\n';
    const { frontmatter, hasFrontmatter } = splitSource(unterminated);
    expect(hasFrontmatter).toBe(false);
    expect(frontmatter).toBe('');
  });

  it('does not treat a `---` inside the body as a closing fence', () => {
    // The block has already ended, so a horizontal rule later in the document is body content.
    const withRule = `---\nid: 01ABC\n---\n\nText\n\n---\n\nMore text\n`;
    const { frontmatter, body } = splitSource(withRule);
    expect(frontmatter).toBe('---\nid: 01ABC\n---\n');
    expect(body).toContain('---\n\nMore text');
    expect(joinSource(frontmatter, body)).toBe(withRule);
  });

  it('accepts CRLF line endings without corrupting them', () => {
    const crlf = '---\r\nid: 01ABC\r\n---\r\n\r\nBody\r\n';
    const { frontmatter, body } = splitSource(crlf);
    expect(frontmatter).toBe('---\r\nid: 01ABC\r\n---\r\n');
    expect(joinSource(frontmatter, body)).toBe(crlf);
  });

  it('handles a block with no trailing newline on the closing fence', () => {
    const tight = '---\nid: 01ABC\n---';
    const { frontmatter, body } = splitSource(tight);
    expect(frontmatter).toBe('---\nid: 01ABC\n---');
    expect(body).toBe('');
    expect(joinSource(frontmatter, body)).toBe(tight);
  });
});

describe('replaceBody', () => {
  it('changes only the body', () => {
    const edited = replaceBody(SOURCE, '\n# Heading\n\nEdited.\n');
    const original = splitSource(SOURCE);
    const result = splitSource(edited);
    expect(result.frontmatter).toBe(original.frontmatter);
    expect(result.body).toContain('Edited.');
    expect(result.body).not.toContain('Body text.');
  });

  it('preserves the irreversible flag and the unknown key', () => {
    // handoff.md section 7.2: clearing wip_ever_published would delete a permanent redirect forever.
    const edited = replaceBody(SOURCE, '\nSomething else entirely.\n');
    expect(edited).toContain('wip_ever_published: true');
    expect(edited).toContain('a_custom_key: the user added this by hand');
    expect(edited).toContain('id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY');
  });

  it('adds the body to a file that has no frontmatter without adding one', () => {
    expect(replaceBody('old body\n', 'new body\n')).toBe('new body\n');
  });

  it('does not glue the body onto the closing fence', () => {
    // A block whose last line lost its newline would otherwise produce `---body`.
    const tight = '---\nid: 01ABC\n---';
    const edited = replaceBody(tight, 'Body\n');
    expect(edited).toBe('---\nid: 01ABC\n---\nBody\n');
  });
});

describe('frontmatterFields', () => {
  it('reads the top-level keys for display', () => {
    const fields = frontmatterFields(splitSource(SOURCE).frontmatter);
    const keys = fields.map((field) => field.key);
    expect(keys).toContain('id');
    expect(keys).toContain('wip_ever_published');
    expect(keys).toContain('a_custom_key');
  });

  it('skips the fences and an indented continuation of a nested value', () => {
    const fields = frontmatterFields(splitSource(SOURCE).frontmatter);
    // `tags:` is a key; `  - veloct` is its continuation and must not become a key of its own.
    expect(fields.find((field) => field.key === 'tags')?.value).toBe('');
    expect(fields.some((field) => field.key.startsWith('-'))).toBe(false);
    expect(fields.some((field) => field.key === '---')).toBe(false);
  });

  it('strips surrounding quotes so a display value is readable', () => {
    const fields = frontmatterFields('---\ntitle: "Quoted Title"\nslug: \'single\'\n---\n');
    expect(fields.find((field) => field.key === 'title')?.value).toBe('Quoted Title');
    expect(fields.find((field) => field.key === 'slug')?.value).toBe('single');
  });

  it('returns nothing for an empty block', () => {
    expect(frontmatterFields('')).toEqual([]);
  });

  it('does not treat a value containing a colon as a key', () => {
    const fields = frontmatterFields('---\ncreated_at: 2026-01-01T00:00:00Z\n---\n');
    expect(fields).toHaveLength(1);
    expect(fields[0]?.value).toBe('2026-01-01T00:00:00Z');
  });
});

describe('the page creator preview', () => {
  /**
   * The AI draft's source, as `veloct-ai` builds it: frontmatter carrying the id Veloct just assigned,
   * then the model's body.
   */
  const AI_SOURCE = [
    '---',
    'id: 01J8Z5Q0K3W7M9V2B4N6P8R9AZ',
    'type: page',
    'slug: about-us',
    'title: About Us',
    'wip: false',
    'wip_publish: false',
    'wip_ever_published: false',
    'created_at: 2026-01-01T12:00:00Z',
    'updated_at: 2026-01-01T12:00:00Z',
    'tags: []',
    '---',
    '',
    '# About Us',
    '',
    'We are a team.',
    '',
    ':::image{placeholder="a photo of the team"}',
    ':::',
    '',
  ].join('\n');

  it('shows only the body, so the assigned id cannot be edited away', () => {
    // The frontmatter holds the id Veloct assigned a moment ago. A textarea over the whole document would
    // make it possible to type an id that disagrees with how the document is addressed.
    const { body, frontmatter } = splitSource(AI_SOURCE);
    expect(body).not.toContain('id:');
    expect(body.startsWith('\n# About Us')).toBe(true);
    expect(frontmatter).toContain('id: 01J8Z5Q0K3W7M9V2B4N6P8R9AZ');
  });

  it('keeps the assigned identity when the user edits the preview', () => {
    // Section 8.1 lets the user modify the draft before confirming; the identity is not theirs to edit.
    const edited = replaceBody(AI_SOURCE, '\n# About Us\n\nWe are a small team.\n');
    expect(edited).toContain('id: 01J8Z5Q0K3W7M9V2B4N6P8R9AZ');
    expect(edited).toContain('slug: about-us');
    expect(edited).toContain('created_at: 2026-01-01T12:00:00Z');
    expect(edited).toContain('We are a small team.');
    expect(edited).not.toContain('We are a team.');
  });

  it('preserves the image placeholder through an edit of another part', () => {
    // A placeholder is the project's own construct and the user replaces it deliberately, not by accident.
    const edited = replaceBody(AI_SOURCE, '\n# About Us\n\nNew text.\n\n:::image{placeholder="a photo of the team"}\n:::\n');
    expect(edited).toContain(':::image{placeholder="a photo of the team"}');
    const roundTrip = splitSource(edited);
    expect(roundTrip.frontmatter).toBe(splitSource(AI_SOURCE).frontmatter);
  });

  it('round trips an untouched draft byte for byte', () => {
    // Confirming without editing must write exactly what was previewed.
    const { frontmatter, body } = splitSource(AI_SOURCE);
    expect(joinSource(frontmatter, body)).toBe(AI_SOURCE);
  });
});

describe('bodiesDifferMeaningfully', () => {
  it('ignores the incidental whitespace a rich-text round trip changes', () => {
    // A Markdown round trip WILL change trailing spaces and blank-line runs. Reporting that as a real
    // edit would make the first save look like a large change.
    expect(bodiesDifferMeaningfully('a\n\n\n\nb\n', 'a\n\nb')).toBe(false);
    expect(bodiesDifferMeaningfully('a   \nb\n', 'a\nb')).toBe(false);
  });

  it('reports a real content change', () => {
    expect(bodiesDifferMeaningfully('a\n\nb\n', 'a\n\nc\n')).toBe(true);
  });

  it('reports an added or removed paragraph', () => {
    expect(bodiesDifferMeaningfully('a\n', 'a\n\nb\n')).toBe(true);
  });
});
