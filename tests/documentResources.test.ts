import { describe, expect, it } from 'vitest';
import type { AttachmentView } from '../src/lib/api';
import { attachmentResourceUrls, belongsToDocument, documentResourceNamespace, readDocumentResources, remoteImageMarkdown, resourceMatchesTab, resourceUrl, thumbnailUrl } from '../src/lib/documentResources';

const file: AttachmentView = {
  id: 'file', original_name: 'report.pdf', content_hash: 'hash', size_bytes: 10,
  cache_path: 'hash/report.pdf', storage_target_id: 'local', storage_namespace: 'default',
  remote_url: 'https://assets.test/report.pdf', remote_key: 'report.pdf', upload_status: 'PENDING',
  reference_status: 'UNAPPLIED', cache_status: 'PRESENT', last_error: null, created_at: '', reference_count: 0,
};

describe('current-document attachment and image pools', () => {
  it('uses durable document identities, not titles, and separates kinds', () => {
    expect(documentResourceNamespace('article', 'a')).toBe('document-article-a');
    expect(documentResourceNamespace('article', 'a')).not.toBe(documentResourceNamespace('article', 'b'));
    expect(documentResourceNamespace('article', 'a')).not.toBe(documentResourceNamespace('page', 'a'));
    expect(documentResourceNamespace('article', '../a')).not.toContain('/');
  });
  it('keeps a newly imported file in this pool before insertion, but not in another article', () => {
    const own = { ...file, storage_namespace: documentResourceNamespace('article', 'a') };
    const empty = readDocumentResources('');
    expect(belongsToDocument(own, documentResourceNamespace('article', 'a'), empty)).toBe(true);
    expect(belongsToDocument(own, documentResourceNamespace('article', 'b'), empty)).toBe(false);
    expect(belongsToDocument(file, documentResourceNamespace('article', 'a'), empty)).toBe(false);
  });
  it('finds existing shared local and remote references without assigning unrelated global files', () => {
    for (const url of attachmentResourceUrls(file)) {
      expect(belongsToDocument(file, 'article', readDocumentResources(`[report](${url})`))).toBe(true);
    }
    expect(belongsToDocument(file, 'article', readDocumentResources('[other](https://assets.test/other.pdf)'))).toBe(false);
  });
  it('reads nested list/table links, reference images and deduplicated images using editor grammar', () => {
    const resources = readDocumentResources('- [文件](/a.pdf)\n\n| 图片 | 链接 |\n| --- | --- |\n| ![a](/img.png) | [b](/b.pdf) |\n\n![same](/img.png)\n\n![引用][photo]\n\n[photo]: https://assets.test/photo.png\n');
    expect([...resources.urls]).toEqual(['/a.pdf', '/img.png', '/b.pdf', 'https://assets.test/photo.png']);
    expect(resources.images).toEqual([{ url: '/img.png', alt: 'a' }, { url: 'https://assets.test/photo.png', alt: '引用' }]);
  });
  it('does not mistake code, frontmatter, or plain URL lookalikes for references', () => {
    const resources = readDocumentResources('---\ncover: /img.png\n---\n`![fake](/inline.png)`\n\n```md\n[report](/a.pdf)\n```\n\n只是文字 /.attachments-cache/hash/report.pdf\n');
    expect(resources.urls.size).toBe(0);
    expect(resources.images).toEqual([]);
  });
  it('matches percent-encoded local names without collapsing encoded slash URL identities', () => {
    const named = { ...file, cache_path: 'hash/文件 a.pdf' };
    expect(belongsToDocument(named, 'article', readDocumentResources('[文件](</.attachments-cache/hash/%E6%96%87%E4%BB%B6%20a.pdf>)'))).toBe(true);
    expect(resourceUrl('https://a.test/a%2Fb')).not.toBe(resourceUrl('https://a.test/a/b'));
    expect(resourceUrl('%broken')).toBe('%broken');
  });
  it('classifies image extensions case-insensitively without mixing files into the image tab', () => {
    expect(resourceMatchesTab(file, 'attachments')).toBe(true);
    expect(resourceMatchesTab(file, 'images')).toBe(false);
    for (const extension of ['PNG', 'jpeg', 'webp', 'gif', 'avif', 'svg', 'bmp', 'ico', 'tif', 'tiff']) {
      expect(resourceMatchesTab({ ...file, original_name: `image.${extension}` }, 'images')).toBe(true);
    }
  });
  it('never loads local paths, data/script URLs, mixed content or URL-embedded credentials as thumbnails', () => {
    for (const url of [null, '', '/.attachments-cache/a.png', 'file:///C:/secret.png', 'data:image/svg+xml,test', 'javascript:alert(1)', 'http://a.test/a.png', 'https://user:password@a.test/a.png']) expect(thumbnailUrl(url)).toBeNull();
    expect(thumbnailUrl('https://a.test/a.png')).toBe('https://a.test/a.png');
  });
  it('escapes remote image descriptions and supports parentheses in addresses', () => {
    const markdown = remoteImageMarkdown(' https://a.test/image(1).png ', 'a [b]\\c\nline');
    expect(markdown).toBe('![a \\[b\\]\\\\c line](<https://a.test/image(1).png>)');
    expect(readDocumentResources(markdown).images[0]?.url).toBe('https://a.test/image(1).png');
  });
  it('rejects unsafe, incomplete or markup-breaking remote addresses', () => {
    for (const url of ['', '/local.png', 'http://a.test/a.png', 'https://u:p@a.test/a.png', 'https://a.test/a b.png', 'https://a.test/<x>.png']) expect(() => remoteImageMarkdown(url, 'a')).toThrow();
  });
});
