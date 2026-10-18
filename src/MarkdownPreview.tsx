import { useEffect, useMemo } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import { frontmatterSummary, splitSource } from './lib/frontmatter';
import { markdownExtensions } from './lib/markdownExtensions';

/** A read-only view of the CURRENT buffer, independent of autosave, IPC and the Astro process. */
export default function MarkdownPreview({ source }: { source: string }) {
  const { frontmatter, body } = useMemo(() => splitSource(source), [source]);
  const summary = useMemo(() => frontmatterSummary(frontmatter), [frontmatter]);
  const editor = useEditor({
    editable: false,
    extensions: markdownExtensions(true),
    content: body,
    contentType: 'markdown',
    editorProps: { attributes: { 'aria-label': 'Markdown 正文预览', role: 'document' } },
  });
  useEffect(() => {
    if (editor) editor.commands.setContent(body, { contentType: 'markdown', emitUpdate: false });
  }, [editor, body]);
  return (
    <section className="markdown-preview editor-content" aria-label="Markdown 即时预览" onClick={event => {
      // Preview links must not navigate the Tauri window or execute a URL. The site view is separate.
      if ((event.target as Element).closest('a')) event.preventDefault();
    }}>
      {summary && <p className="markdown-preview__summary">{summary}</p>}
      {!body.trim() && <p className="muted">正文为空，开始输入后会在这里即时显示。</p>}
      <EditorContent editor={editor} />
    </section>
  );
}
