/**
 * The attachment pool.
 *
 * `handoff.md` §13 is a locked section, and two of its rules are visible in this component's shape rather
 * than merely honoured by it:
 *
 *  * **§13.1's three axes are shown separately.** An attachment can be cached but not uploaded, uploaded but
 *    not referenced by any document, and so on. A single "status" column would flatten three independent
 *    facts into one and make it impossible to tell "the picture is on the server but no article uses it" from
 *    "an article uses it but the upload failed" — which are different problems with different fixes.
 *  * **The Markdown comes from the Rust side.** `markdown` arrives in the add result, so there is exactly one
 *    place that formats an attachment URL. A second formatter here would be a second thing that can disagree
 *    with the site about where the file is.
 *
 * The orphan section shows §13.2.4's two outcomes as two lists, because the distinction IS the information:
 * an attachment that fails a condition is not a cleanup candidate but a question for the user.
 */

import { useCallback, useEffect, useState } from 'react';

import {
  api,
  describeError,
  type AddAttachmentView,
  type AttachmentOrphans,
  type AttachmentView,
} from './lib/api';

/** The axes, in the order §13.1 lists them, with the values the crate uses. */
const AXES: Array<{ key: keyof AttachmentView; label: string; values: Record<string, string> }> = [
  {
    key: 'upload_status',
    label: '上传',
    values: {
      PENDING: '未上传',
      UPLOADING: '上传中',
      SUCCEEDED: '已上传',
      FAILED: '上传失败',
    },
  },
  {
    key: 'reference_status',
    label: '引用',
    values: {
      NOT_APPLIED: '未被引用',
      APPLYING: '写入中',
      APPLIED: '已被引用',
      FAILED: '引用失败',
    },
  },
  {
    key: 'cache_status',
    label: '缓存',
    values: { PRESENT: '已缓存', CLEANED: '已清理' },
  },
];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function AttachmentPanel({
  enabled,
  onInsertMarkdown,
}: {
  enabled: boolean;
  /** Called with the Markdown for an attachment, so the editor decides what to do with it. */
  onInsertMarkdown?: (markdown: string) => void;
}) {
  const [attachments, setAttachments] = useState<AttachmentView[]>([]);
  const [orphans, setOrphans] = useState<AttachmentOrphans | null>(null);
  const [cacheDirectory, setCacheDirectory] = useState<string>('');
  const [sourcePath, setSourcePath] = useState('');
  const [target, setTarget] = useState('local');
  const [namespace, setNamespace] = useState('default');
  const [lastAdd, setLastAdd] = useState<AddAttachmentView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!enabled) return;
    try {
      const [list, found, directory] = await Promise.all([
        api.listAttachments(),
        api.attachmentOrphans(),
        api.attachmentCacheDirectory(),
      ]);
      setAttachments(list);
      setOrphans(found);
      setCacheDirectory(directory);
    } catch (caught) {
      setError(describeError(caught));
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = useCallback(
    async (action: () => Promise<void>, done: string) => {
      setBusy(true);
      setError(null);
      setNotice(null);
      try {
        await action();
        await reload();
        setNotice(done);
      } catch (caught) {
        setError(describeError(caught));
      } finally {
        setBusy(false);
      }
    },
    [reload],
  );

  const add = () =>
    run(async () => {
      if (!sourcePath.trim()) {
        throw new Error('请填写要添加的文件的完整路径。');
      }
      const result = await api.addAttachment({
        sourcePath: sourcePath.trim(),
        storageTargetId: target.trim() || 'local',
        namespace: namespace.trim() || 'default',
      });
      setLastAdd(result);
      setSourcePath('');
    }, '已加入附件池。添加不会修改任何文章,请自己决定何时插入 Markdown。');

  const syncIndex = () =>
    run(async () => {
      await api.syncAttachmentIndex();
    }, '附件索引已同步。');

  if (!enabled) {
    return (
      <div className="attachment-panel">
        <p className="muted">打开一个项目后可以管理附件。</p>
      </div>
    );
  }

  return (
    <div className="attachment-panel">
      <header className="attachment-header">
        <strong>附件</strong>
        <button type="button" onClick={() => void reload()} disabled={busy}>
          刷新
        </button>
        <button type="button" onClick={() => void syncIndex()} disabled={busy}>
          同步索引
        </button>
      </header>

      {error && (
        <p className="attachment-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p className="attachment-notice">{notice}</p>}

      <section className="attachment-add">
        <h3>添加文件</h3>
        <div className="row">
          <label>
            <span>文件路径</span>
            <input
              value={sourcePath}
              onChange={(event) => setSourcePath(event.target.value)}
              placeholder="D:/图片/photo.png"
              spellCheck={false}
            />
          </label>
          <label>
            <span>存储目标</span>
            <input value={target} onChange={(event) => setTarget(event.target.value)} />
          </label>
          <label>
            <span>命名空间</span>
            <input value={namespace} onChange={(event) => setNamespace(event.target.value)} />
          </label>
        </div>
        <button type="button" onClick={() => void add()} disabled={busy}>
          加入附件池
        </button>
        {lastAdd && (
          <p className="attachment-result">
            {lastAdd.outcome === 'reused' ? '相同内容已存在,复用了原记录: ' : '已添加: '}
            <code>{lastAdd.markdown}</code>
            {onInsertMarkdown && (
              <button type="button" onClick={() => onInsertMarkdown(lastAdd.markdown)}>
                插入到文章
              </button>
            )}
          </p>
        )}
        <p className="muted hint">
          缓存目录:<code>{cacheDirectory || '(未知)'}</code>
        </p>
      </section>

      <section>
        <h3>附件池({attachments.length})</h3>
        {attachments.length === 0 && <p className="muted">还没有附件。</p>}
        <ul className="attachment-list">
          {attachments.map((attachment) => (
            <li key={attachment.id}>
              <span className="name" title={attachment.id}>
                {attachment.original_name}
              </span>
              <span className="muted">{formatBytes(attachment.size_bytes)}</span>
              {/* The three axes, each with its own label, because they fail independently. */}
              {AXES.map((axis) => {
                const value = String(attachment[axis.key]);
                return (
                  <span key={axis.key} className={`axis axis-${value.toLowerCase()}`}>
                    {axis.label}:{axis.values[value] ?? value}
                  </span>
                );
              })}
              <span className={attachment.reference_count > 0 ? 'ok' : 'muted'}>
                被 {attachment.reference_count} 篇引用
              </span>
              {attachment.remote_url && (
                <span className="muted" title={attachment.remote_url}>
                  远程已上传
                </span>
              )}
              {attachment.last_error && (
                <span className="missing" title={attachment.last_error}>
                  最近错误
                </span>
              )}
              <button
                type="button"
                onClick={() =>
                  void api
                    .attachmentMarkdown(attachment.id)
                    .then((markdown) => {
                      if (onInsertMarkdown) onInsertMarkdown(markdown);
                      else void navigator.clipboard?.writeText(markdown);
                      setNotice('Markdown 已就绪。');
                    })
                    .catch((caught: unknown) => setError(describeError(caught)))
                }
                disabled={busy}
              >
                取 Markdown
              </button>
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    // A referenced attachment is removed only after the user is told how many articles cite it,
                    // because section 13.2's reference set exists to make that consequence visible.
                    if (attachment.reference_count > 0) {
                      const confirmed = window.confirm(
                        `这个附件被 ${attachment.reference_count} 篇文章引用,删除后这些引用会失效。继续?`,
                      );
                      if (!confirmed) return;
                    }
                    await api.removeAttachment(attachment.id);
                  }, '附件已删除,缓存文件一并移除。')
                }
                disabled={busy}
              >
                删除
              </button>
            </li>
          ))}
        </ul>
      </section>

      {orphans && (orphans.safe.length > 0 || orphans.needs_confirmation.length > 0) && (
        <section>
          <h3>可以清理的附件</h3>
          {orphans.safe.length > 0 && (
            <>
              <p className="muted">
                以下附件没有任何引用,且满足第 13.2.4 节的全部条件,可以安全删除:
              </p>
              <ul className="orphan-list safe">
                {orphans.safe.map((attachment) => (
                  <li key={attachment.id}>
                    <span>{attachment.original_name}</span>
                    <button
                      type="button"
                      onClick={() =>
                        void run(
                          () => api.removeAttachment(attachment.id).then(() => undefined),
                          '已删除。',
                        )
                      }
                      disabled={busy}
                    >
                      删除
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
          {orphans.needs_confirmation.length > 0 && (
            <>
              <p className="muted">
                以下附件需要你确认:它们没有引用,但不满足自动清理的全部条件(例如可能正在上传)。
              </p>
              <ul className="orphan-list confirm">
                {orphans.needs_confirmation.map((attachment) => (
                  <li key={attachment.id}>
                    <span>{attachment.original_name}</span>
                    <span className="muted">上传状态:{attachment.upload_status}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}
    </div>
  );
}
