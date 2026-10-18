import { describe, expect, it } from 'vitest';
import { MarkdownManager } from '@tiptap/markdown';
import { markdownExtensions } from '../src/lib/markdownExtensions';

const fixture = '# Heading\n\nText **bold** and *italic*, [link](https://example.com).\n\n- item\n- second\n\n```ts\nconst n = 1;\n```\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n![alt](/assets/example.png)\n';

describe('shared writing and local preview Markdown grammar', () => {
  it('parses headings, marks, lists, code, tables and images instead of HTML/plain text', () => {
    const manager = new MarkdownManager({ extensions: markdownExtensions() });
    const doc = manager.parse(fixture);
    expect(doc.content?.map(node => node.type)).toEqual(['heading', 'paragraph', 'bulletList', 'codeBlock', 'table', 'image']);
    expect(doc.content?.[0]?.attrs?.level).toBe(1);
    expect(doc.content?.[3]?.attrs?.language).toBe('ts');
    expect(JSON.stringify(doc)).toContain('"type":"bold"');
    expect(JSON.stringify(doc)).toContain('"type":"italic"');
    expect(JSON.stringify(doc)).toContain('https://example.com');
  });

  it('preview and writing yield identical Markdown documents without disk writes', () => {
    const writing = new MarkdownManager({ extensions: markdownExtensions() });
    const reading = new MarkdownManager({ extensions: markdownExtensions(true) });
    expect(reading.parse(fixture)).toEqual(writing.parse(fixture));
    expect(reading.serialize(reading.parse(fixture))).toEqual(writing.serialize(writing.parse(fixture)));
  });

  it('registers Link only once (StarterKit includes Link in Tiptap v3)', () => {
    const extensions = markdownExtensions();
    const starter = extensions.find(extension => extension.name === 'starterKit');
    expect(starter?.options).toMatchObject({ link: false });
    expect(extensions.filter(extension => extension.name === 'link')).toHaveLength(1);
  });
});
