import StarterKit from '@tiptap/starter-kit';
import { Markdown } from '@tiptap/markdown';
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';
import Image from '@tiptap/extension-image';
import Link from '@tiptap/extension-link';

/** Preview-only image view: local/site-relative paths must not hit the desktop app's origin. */
const PreviewImage = Image.extend({
  renderHTML({ HTMLAttributes }) {
    const src = String(HTMLAttributes.src ?? '');
    let remote = false;
    try { remote = ['http:', 'https:'].includes(new URL(src).protocol); } catch { /* site-relative */ }
    if (!remote) {
      return ['span', { class: 'markdown-preview__image-placeholder' },
        `图片：${HTMLAttributes.alt || src || '未提供地址'}（站点相对图片请在站点预览查看）`];
    }
    return ['img', { ...HTMLAttributes, loading: 'lazy', referrerpolicy: 'no-referrer' }];
  },
});

/** Both writing and local reading use the same Markdown grammar; never round-trip a preview into disk. */
export function markdownExtensions(preview = false) {
  return [
    StarterKit.configure({ heading: { levels: [1, 2, 3, 4] }, codeBlock: {}, link: false }),
    Markdown,
    Table.configure({ resizable: false }), TableRow, TableHeader, TableCell,
    (preview ? PreviewImage : Image).configure({ inline: false, allowBase64: false }),
    Link.configure({ openOnClick: false, autolink: true }),
  ];
}
