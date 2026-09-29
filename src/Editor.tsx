/**
 * The WYSIWYG editor.
 *
 * `handoff.md` §4.2 asks for four things, and each one shaped a decision here:
 *
 *  * **Full Markdown editing, WYSIWYG by default.** Tiptap's StarterKit plus a table, an image and a link
 *    extension covers the dialect §3.3 names: CommonMark, GFM, tables, code blocks, images, links, lists
 *    and quotes.
 *  * **A split mode whose right side is the REAL Astro render.** That is why this component renders no
 *    preview of its own. §5.1 forbids a second renderer, and the moment there were two they would
 *    disagree — at which point the preview stops predicting the site. The preview is an iframe in `App`.
 *  * **A draggable divider.** Owned by `App`, because the split is between this pane and the preview pane
 *    rather than inside the editor.
 *  * **The online frontend contains no Markdown editor.** Everything here lives in `src/`, which is only
 *    ever bundled into the Tauri window; the site is a separate Astro project that never imports it.
 *
 * The one non-obvious boundary: the editor works on the BODY alone, and the frontmatter is re-attached
 * byte-for-byte on every change. §3.2 puts the permanent `id` in the frontmatter, so an editor that
 * re-serialised it could drop an unknown key or reset `wip_ever_published` — and the latter would delete a
 * permanent redirect for good. `replaceBody` is the single place that happens.
 */

import { useEffect, useMemo, useRef, useState } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import { Markdown } from '@tiptap/markdown';
// Tiptap v3 exports the whole table family from one package, so there is one import and no chance of
// the four pieces drifting to different versions.
import { Table, TableCell, TableHeader, TableRow } from '@tiptap/extension-table';
import Image from '@tiptap/extension-image';
import Link from '@tiptap/extension-link';

import { frontmatterFields, splitSource } from './lib/frontmatter';

/** A toolbar action, kept as data so the buttons and their states cannot drift apart. */
interface ToolbarAction {
  label: string;
  title: string;
  /** Whether the action applies to the current selection. */
  isActive: boolean;
  run: () => void;
  disabled?: boolean;
}

export default function Editor({
  source,
  editable,
  onChange,
}: {
  /** The whole file, including the frontmatter, which the editor never touches. */
  source: string;
  editable: boolean;
  onChange: (nextSource: string) => void;
}) {
  const { frontmatter, body } = useMemo(() => splitSource(source), [source]);
  const fields = useMemo(() => frontmatterFields(frontmatter), [frontmatter]);

  // The body the editor last wrote, so an incoming `source` change from Select or from a conflict
  // resolution is applied, while the change the editor itself just produced is not applied back over it.
  const lastWrittenBody = useRef<string | null>(null);
  const [markdownView, setMarkdownView] = useState(false);

  const editor = useEditor({
    editable,
    extensions: [
      StarterKit.configure({
        // The heading levels §3.3 uses; a deeper one would produce Markdown the site does not style.
        heading: { levels: [1, 2, 3, 4] },
        // Markdown is the truth, so a code block is fenced rather than a rich object.
        codeBlock: {},
      }),
      Markdown,
      Table.configure({ resizable: false }),
      TableRow,
      TableHeader,
      TableCell,
      Image.configure({ inline: false, allowBase64: false }),
      // A link is plain Markdown, so it is not opened on click: clicking inside the editor should place
      // the cursor, not navigate away from the document being written.
      Link.configure({ openOnClick: false, autolink: true }),
    ],
    content: body,
    onUpdate: ({ editor: instance }) => {
      const nextBody = instance.getMarkdown();
      lastWrittenBody.current = nextBody;
      onChange(`${frontmatter}${frontmatter.endsWith('\n') || !frontmatter ? '' : '\n'}${nextBody}`);
    },
  });

  // Apply an EXTERNAL change, such as selecting another document or taking the disk version of a
  // conflict. The guard is what stops the editor's own keystrokes from being re-applied and moving the
  // cursor to the start on every character.
  useEffect(() => {
    if (!editor) return;
    if (lastWrittenBody.current === body) return;
    if (editor.getMarkdown() === body) return;
    lastWrittenBody.current = body;
    editor.commands.setContent(body);
  }, [editor, body]);

  useEffect(() => {
    editor?.setEditable(editable);
  }, [editor, editable]);

  if (!editor) {
    return <div className="editor-loading muted">正在加载编辑器…</div>;
  }

  const addLink = () => {
    const previous = editor.getAttributes('link').href as string | undefined;
    const url = window.prompt('链接地址', previous ?? 'https://');
    if (url === null) return;
    if (url.trim() === '') {
      editor.chain().focus().extendMarkRange('link').unsetLink().run();
      return;
    }
    editor.chain().focus().extendMarkRange('link').setLink({ href: url.trim() }).run();
  };

  const addImage = () => {
    // A URL, not a file picker: handoff.md section 13 has its own attachment pipeline for local files,
    // and an editor that silently embedded a local path would produce Markdown the site cannot serve.
    const url = window.prompt('图片地址(可先用附件功能上传本地图片)');
    if (!url || url.trim() === '') return;
    editor.chain().focus().setImage({ src: url.trim() }).run();
  };

  const actions: ToolbarAction[] = [
    {
      label: 'H1',
      title: '一级标题',
      isActive: editor.isActive('heading', { level: 1 }),
      run: () => editor.chain().focus().toggleHeading({ level: 1 }).run(),
    },
    {
      label: 'H2',
      title: '二级标题',
      isActive: editor.isActive('heading', { level: 2 }),
      run: () => editor.chain().focus().toggleHeading({ level: 2 }).run(),
    },
    {
      label: 'H3',
      title: '三级标题',
      isActive: editor.isActive('heading', { level: 3 }),
      run: () => editor.chain().focus().toggleHeading({ level: 3 }).run(),
    },
    {
      label: 'B',
      title: '加粗',
      isActive: editor.isActive('bold'),
      run: () => editor.chain().focus().toggleBold().run(),
    },
    {
      label: 'I',
      title: '斜体',
      isActive: editor.isActive('italic'),
      run: () => editor.chain().focus().toggleItalic().run(),
    },
    {
      label: 'S',
      title: '删除线',
      isActive: editor.isActive('strike'),
      run: () => editor.chain().focus().toggleStrike().run(),
    },
    {
      label: '`',
      title: '行内代码',
      isActive: editor.isActive('code'),
      run: () => editor.chain().focus().toggleCode().run(),
    },
    {
      label: '{ }',
      title: '代码块',
      isActive: editor.isActive('codeBlock'),
      run: () => editor.chain().focus().toggleCodeBlock().run(),
    },
    {
      label: '•',
      title: '无序列表',
      isActive: editor.isActive('bulletList'),
      run: () => editor.chain().focus().toggleBulletList().run(),
    },
    {
      label: '1.',
      title: '有序列表',
      isActive: editor.isActive('orderedList'),
      run: () => editor.chain().focus().toggleOrderedList().run(),
    },
    {
      label: '❝',
      title: '引用',
      isActive: editor.isActive('blockquote'),
      run: () => editor.chain().focus().toggleBlockquote().run(),
    },
    {
      label: '—',
      title: '分隔线',
      isActive: editor.isActive('horizontalRule'),
      run: () => editor.chain().focus().setHorizontalRule().run(),
    },
    {
      label: '⌗',
      title: '表格',
      isActive: editor.isActive('table'),
      run: () =>
        editor
          .chain()
          .focus()
          .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
          .run(),
    },
    {
      label: '🔗',
      title: '链接',
      isActive: editor.isActive('link'),
      run: addLink,
    },
    {
      label: '🖼',
      title: '图片(远程地址)',
      isActive: editor.isActive('image'),
      run: addImage,
    },
    {
      label: '↶',
      title: '撤销',
      isActive: false,
      run: () => editor.chain().focus().undo().run(),
      disabled: !editor.can().undo(),
    },
    {
      label: '↷',
      title: '重做',
      isActive: false,
      run: () => editor.chain().focus().redo().run(),
      disabled: !editor.can().redo(),
    },
  ];

  return (
    <div className="wysiwyg">
      {fields.length > 0 && (
        <div className="frontmatter-strip" title="frontmatter 由 Veloct 维护,编辑器不会改动它">
          {fields.map((field) => (
            <span key={field.key} className={`fm ${field.key}`}>
              <span className="fm-key">{field.key}</span>
              <span className="fm-value">{field.value || '—'}</span>
            </span>
          ))}
        </div>
      )}

      <div className="toolbar">
        {actions.map((action) => (
          <button
            type="button"
            key={action.title}
            title={action.title}
            className={action.isActive ? 'active' : ''}
            disabled={action.disabled ?? false}
            // `preventDefault` on mouse-down keeps the selection: a toolbar click that blurred the editor
            // would apply the command to nothing.
            onMouseDown={(event) => event.preventDefault()}
            onClick={action.run}
          >
            {action.label}
          </button>
        ))}
        <button
          type="button"
          className={markdownView ? 'active' : ''}
          title="查看 / 编辑原始 Markdown"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => setMarkdownView((showing) => !showing)}
        >
          MD
        </button>
      </div>

      {markdownView ? (
        <textarea
          className="markdown-source"
          value={body}
          spellCheck={false}
          readOnly={!editable}
          onChange={(event) => {
            lastWrittenBody.current = event.target.value;
            onChange(`${frontmatter}${frontmatter.endsWith('\n') || !frontmatter ? '' : '\n'}${event.target.value}`);
          }}
        />
      ) : (
        <EditorContent editor={editor} className="editor-content" />
      )}
    </div>
  );
}
