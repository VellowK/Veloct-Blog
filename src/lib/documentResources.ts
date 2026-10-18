import { MarkdownManager } from '@tiptap/markdown';
import type { AttachmentView } from './api';
import { isImageAttachment } from './attachmentSync';
import { splitSource } from './frontmatter';
import { markdownExtensions } from './markdownExtensions';

export type ResourceTab = 'attachments' | 'images';
export interface DocumentImage { url: string; alt: string }
export interface DocumentResources { urls: Set<string>; images: DocumentImage[] }
let parser: MarkdownManager | undefined;

/** A durable per-document pool, using the existing target + namespace + hash dedup contract. */
export function documentResourceNamespace(kind: string, id: string): string {
  return `document-${encodeURIComponent(kind)}-${encodeURIComponent(id)}`;
}

export function resourceUrl(value: string): string {
  try { return decodeURI(value); } catch { return value; }
}

/** Read the editor's grammar, not regex: code, frontmatter and lookalike URL text are not references. */
export function readDocumentResources(source: string): DocumentResources {
  parser ??= new MarkdownManager({ extensions: markdownExtensions() });
  const doc = parser.parse(splitSource(source).body);
  const urls = new Set<string>();
  const images = new Map<string, DocumentImage>();
  const visit = (node: typeof doc) => {
    if (node.type === 'image' && typeof node.attrs?.src === 'string') {
      const url = resourceUrl(node.attrs.src);
      urls.add(url);
      if (!images.has(url)) images.set(url, { url: node.attrs.src, alt: String(node.attrs.alt || '未命名图片') });
    }
    for (const mark of node.marks ?? []) {
      if (mark.type === 'link' && typeof mark.attrs?.href === 'string') urls.add(resourceUrl(mark.attrs.href));
    }
    node.content?.forEach(visit);
  };
  visit(doc);
  return { urls, images: [...images.values()] };
}

export function attachmentResourceUrls(attachment: AttachmentView): string[] {
  return [`/.attachments-cache/${attachment.cache_path}`, ...(attachment.remote_url ? [attachment.remote_url] : [])].map(resourceUrl);
}

export function belongsToDocument(attachment: AttachmentView, namespace: string, resources: DocumentResources): boolean {
  return attachment.storage_namespace === namespace || attachmentResourceUrls(attachment).some(url => resources.urls.has(url));
}

export function resourceMatchesTab(attachment: AttachmentView, tab: ResourceTab): boolean {
  return isImageAttachment(attachment.original_name) === (tab === 'images');
}

/** No file:, data:, javascript:, credentials or insecure mixed-content thumbnails. */
export function thumbnailUrl(value: string | null): string | null {
  if (!value) return null;
  try {
    const url = new URL(value);
    return url.protocol === 'https:' && !url.username && !url.password ? url.href : null;
  } catch { return null; }
}

/** User-supplied remote pictures follow the editor's URL-only flow, never the attachment upload flow. */
export function remoteImageMarkdown(url: string, alt: string): string {
  const value = url.trim();
  if (!thumbnailUrl(value) || /[\s<>]/.test(value)) throw new Error('请输入完整的 HTTPS 图片地址（不含空格或账号密码）。');
  const label = alt.trim().replace(/[\r\n]+/g, ' ').replace(/[\\[\]]/g, '\\$&');
  return `![${label}](<${value}>)`;
}
